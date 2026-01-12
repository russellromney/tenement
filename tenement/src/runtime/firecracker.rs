//! Firecracker runtime - spawns microVMs with vsock communication
//!
//! Requires KVM support (bare metal or nested virtualization).
//! Will NOT work on Fly.io or most cloud VMs without nested virt.
//!
//! ## Platform Requirements
//! - Linux with KVM enabled (/dev/kvm accessible)
//! - Firecracker binary in PATH or specified location
//! - Kernel image and rootfs for VMs
//!
//! ## Unsupported Platforms
//! - macOS (no KVM)
//! - Windows (no KVM)
//! - Fly.io (nested virt explicitly disabled)
//! - Most cloud VMs without nested virtualization

use super::{Runtime, RuntimeHandle, RuntimeType, SpawnConfig};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use anyhow::Context;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(target_os = "linux")]
use std::time::Duration;
#[cfg(target_os = "linux")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(target_os = "linux")]
use tokio::net::UnixStream;
#[cfg(target_os = "linux")]
use tokio::process::{Child, Command};
#[cfg(target_os = "linux")]
use tracing::{debug, info};

/// Global CID counter for vsock (starts at 3, as 0/1/2 are reserved)
#[cfg(target_os = "linux")]
static NEXT_CID: AtomicU32 = AtomicU32::new(3);

/// Runtime that spawns Firecracker microVMs
///
/// This runtime uses Firecracker's HTTP API over a Unix socket to configure
/// and manage microVMs. Guest applications communicate via vsock.
pub struct FirecrackerRuntime {
    /// Path to firecracker binary (defaults to finding in PATH)
    firecracker_bin: Option<PathBuf>,
}

impl FirecrackerRuntime {
    pub fn new() -> Self {
        Self {
            firecracker_bin: None,
        }
    }

    pub fn with_binary(path: PathBuf) -> Self {
        Self {
            firecracker_bin: Some(path),
        }
    }

    /// Check if KVM is available
    fn check_kvm() -> bool {
        #[cfg(target_os = "linux")]
        {
            std::path::Path::new("/dev/kvm").exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    /// Find firecracker binary in common locations
    fn find_firecracker(&self) -> Option<PathBuf> {
        if let Some(ref path) = self.firecracker_bin {
            if path.exists() {
                return Some(path.clone());
            }
        }

        // Check common locations
        for path in &[
            "/usr/local/bin/firecracker",
            "/usr/bin/firecracker",
            "/opt/firecracker/bin/firecracker",
        ] {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }

        // Try PATH via which
        std::env::var("PATH").ok().and_then(|path| {
            for dir in path.split(':') {
                let p = PathBuf::from(dir).join("firecracker");
                if p.exists() {
                    return Some(p);
                }
            }
            None
        })
    }

    /// Allocate a unique CID for a new VM
    #[cfg(target_os = "linux")]
    fn allocate_cid() -> u32 {
        NEXT_CID.fetch_add(1, Ordering::SeqCst)
    }

    /// Send an HTTP PUT request to Firecracker's API socket
    #[cfg(target_os = "linux")]
    async fn api_put(socket_path: &PathBuf, endpoint: &str, body: &str) -> Result<()> {
        let mut stream = UnixStream::connect(socket_path)
            .await
            .with_context(|| format!("Failed to connect to Firecracker API at {:?}", socket_path))?;

        let request = format!(
            "PUT {} HTTP/1.1\r\n\
             Host: localhost\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}",
            endpoint,
            body.len(),
            body
        );

        stream
            .write_all(request.as_bytes())
            .await
            .context("Failed to write request")?;

        // Read response
        let mut response = vec![0u8; 4096];
        let n = stream.read(&mut response).await.context("Failed to read response")?;
        let response_str = String::from_utf8_lossy(&response[..n]);

        debug!("Firecracker API {} response: {}", endpoint, response_str.lines().next().unwrap_or(""));

        // Check for success (2xx status)
        if response_str.contains("HTTP/1.1 2") {
            Ok(())
        } else {
            // Extract error message from response body
            let body_start = response_str.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
            let error_body = &response_str[body_start..];
            anyhow::bail!(
                "Firecracker API error on {}: {}",
                endpoint,
                error_body.trim()
            )
        }
    }

    /// Wait for the API socket to become available
    #[cfg(target_os = "linux")]
    async fn wait_for_api_socket(socket_path: &PathBuf, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if socket_path.exists() {
                // Try to connect to verify it's ready
                if UnixStream::connect(socket_path).await.is_ok() {
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        anyhow::bail!(
            "Firecracker API socket {:?} not ready after {:?}",
            socket_path,
            timeout
        )
    }

    /// Get detailed availability status
    pub fn availability_details(&self) -> String {
        let mut issues = Vec::new();

        #[cfg(not(target_os = "linux"))]
        issues.push("Firecracker requires Linux (current OS is not Linux)");

        #[cfg(target_os = "linux")]
        if !Self::check_kvm() {
            issues.push("/dev/kvm not found (KVM not enabled or not available)");
        }

        if self.find_firecracker().is_none() {
            issues.push("Firecracker binary not found in PATH or common locations");
        }

        if issues.is_empty() {
            "Firecracker runtime available".to_string()
        } else {
            format!("Firecracker runtime not available:\n  - {}", issues.join("\n  - "))
        }
    }
}

impl Default for FirecrackerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Runtime for FirecrackerRuntime {
    #[allow(unused_variables)]
    async fn spawn(&self, config: &SpawnConfig) -> Result<RuntimeHandle> {
        // Validate platform requirements first
        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!(
                "Firecracker runtime requires Linux. \
                Current platform is not supported."
            );
        }

        #[cfg(target_os = "linux")]
        {
            let vm_config = config
                .vm_config
                .as_ref()
                .context("VmConfig is required for Firecracker runtime")?;

            if !Self::check_kvm() {
                anyhow::bail!(
                    "KVM not available (/dev/kvm not found).\n\
                    Firecracker requires bare metal or nested virtualization.\n\
                    This will NOT work on:\n\
                      - Fly.io (nested virt disabled)\n\
                      - Most cloud VMs without nested virt\n\
                      - macOS or Windows\n\n\
                    For development, use the process runtime instead."
                );
            }

            let _firecracker_bin = self.find_firecracker().context(
                "Firecracker binary not found.\n\
                Install from: https://github.com/firecracker-microvm/firecracker/releases\n\
                Place in /usr/local/bin/firecracker or add to PATH."
            )?;

            if !vm_config.kernel.exists() {
                anyhow::bail!(
                    "Kernel image not found: {}\n\
                    Download from: https://github.com/firecracker-microvm/firecracker/blob/main/docs/getting-started.md",
                    vm_config.kernel.display()
                );
            }

            if !vm_config.rootfs.exists() {
                anyhow::bail!(
                    "Root filesystem not found: {}\n\
                    See: https://github.com/firecracker-microvm/firecracker/blob/main/docs/getting-started.md",
                    vm_config.rootfs.display()
                );
            }

            // Allocate CID and create socket paths
            let cid = Self::allocate_cid();
            let socket_dir = config.socket.parent().unwrap_or_else(|| std::path::Path::new("/tmp"));
            let instance_name = config
                .socket
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("fc");

            let api_socket = socket_dir.join(format!("fc-{}-api.sock", instance_name));
            let vsock_socket = socket_dir.join(format!("fc-{}-vsock.sock", instance_name));

            // Clean up old sockets
            std::fs::remove_file(&api_socket).ok();
            std::fs::remove_file(&vsock_socket).ok();

            info!(
                "Spawning Firecracker VM: kernel={}, rootfs={}, cid={}, memory={}MB, vcpus={}",
                vm_config.kernel.display(),
                vm_config.rootfs.display(),
                cid,
                vm_config.memory_mb,
                vm_config.vcpus
            );

            // 1. Spawn firecracker process with API socket
            let child = Command::new(&firecracker_bin)
                .arg("--api-sock")
                .arg(&api_socket)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .with_context(|| format!("Failed to spawn firecracker at {:?}", firecracker_bin))?;

            info!("Firecracker process started, waiting for API socket...");

            // 2. Wait for API socket to be ready
            if let Err(e) = Self::wait_for_api_socket(&api_socket, Duration::from_secs(5)).await {
                // Kill the process if API socket never became ready
                drop(child);
                std::fs::remove_file(&api_socket).ok();
                return Err(e);
            }

            info!("Firecracker API socket ready, configuring VM...");

            // Helper to clean up on error
            let cleanup = |child: Child, api_socket: &PathBuf, vsock_socket: &PathBuf| {
                drop(child);
                std::fs::remove_file(api_socket).ok();
                std::fs::remove_file(vsock_socket).ok();
            };

            // 3. Configure boot source
            let boot_args = "console=ttyS0 reboot=k panic=1 pci=off";
            let boot_source = format!(
                r#"{{"kernel_image_path": "{}", "boot_args": "{}"}}"#,
                vm_config.kernel.display(),
                boot_args
            );
            if let Err(e) = Self::api_put(&api_socket, "/boot-source", &boot_source).await {
                cleanup(child, &api_socket, &vsock_socket);
                return Err(e.context("Failed to configure boot source"));
            }

            // 4. Configure root drive
            let drive_config = format!(
                r#"{{"drive_id": "rootfs", "path_on_host": "{}", "is_root_device": true, "is_read_only": false}}"#,
                vm_config.rootfs.display()
            );
            if let Err(e) = Self::api_put(&api_socket, "/drives/rootfs", &drive_config).await {
                cleanup(child, &api_socket, &vsock_socket);
                return Err(e.context("Failed to configure root drive"));
            }

            // 5. Configure machine (vcpus and memory)
            let machine_config = format!(
                r#"{{"vcpu_count": {}, "mem_size_mib": {}}}"#,
                vm_config.vcpus,
                vm_config.memory_mb
            );
            if let Err(e) = Self::api_put(&api_socket, "/machine-config", &machine_config).await {
                cleanup(child, &api_socket, &vsock_socket);
                return Err(e.context("Failed to configure machine"));
            }

            // 6. Configure vsock device
            let vsock_config = format!(
                r#"{{"guest_cid": {}, "uds_path": "{}"}}"#,
                cid,
                vsock_socket.display()
            );
            if let Err(e) = Self::api_put(&api_socket, "/vsock", &vsock_config).await {
                cleanup(child, &api_socket, &vsock_socket);
                return Err(e.context("Failed to configure vsock"));
            }

            // 7. Start the VM
            let start_action = r#"{"action_type": "InstanceStart"}"#;
            if let Err(e) = Self::api_put(&api_socket, "/actions", start_action).await {
                cleanup(child, &api_socket, &vsock_socket);
                return Err(e.context("Failed to start VM"));
            }

            info!(
                "Firecracker VM started: cid={}, vsock={}",
                cid,
                vsock_socket.display()
            );

            Ok(RuntimeHandle::Firecracker {
                api_socket,
                vsock_socket,
                cid,
                port: vm_config.vsock_port,
            })
        }
    }

    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Firecracker
    }

    fn is_available(&self) -> bool {
        Self::check_kvm() && self.find_firecracker().is_some()
    }

    fn name(&self) -> &'static str {
        "firecracker"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_firecracker_runtime_type() {
        let runtime = FirecrackerRuntime::new();
        assert_eq!(runtime.runtime_type(), RuntimeType::Firecracker);
    }

    #[test]
    fn test_firecracker_runtime_name() {
        let runtime = FirecrackerRuntime::new();
        assert_eq!(runtime.name(), "firecracker");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_cid_allocation() {
        let cid1 = FirecrackerRuntime::allocate_cid();
        let cid2 = FirecrackerRuntime::allocate_cid();
        assert!(cid2 > cid1);
    }

    #[test]
    fn test_availability_details() {
        let runtime = FirecrackerRuntime::new();
        let details = runtime.availability_details();
        // Should contain some message about availability
        assert!(!details.is_empty());
    }

    #[test]
    fn test_with_binary() {
        let runtime = FirecrackerRuntime::with_binary(PathBuf::from("/custom/firecracker"));
        assert_eq!(runtime.firecracker_bin, Some(PathBuf::from("/custom/firecracker")));
    }

    // Integration tests require KVM and are marked as ignored
    #[tokio::test]
    #[ignore = "Requires KVM and Firecracker binary"]
    async fn test_firecracker_spawn_validation() {
        use std::collections::HashMap;

        let runtime = FirecrackerRuntime::new();

        // Test with missing vm_config
        let config = SpawnConfig {
            command: String::new(),
            args: vec![],
            env: HashMap::new(),
            socket: PathBuf::from("/tmp/test-fc.sock"),
            workdir: None,
            vm_config: None,
        };

        let result = runtime.spawn(&config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("VmConfig is required"));
    }
}
