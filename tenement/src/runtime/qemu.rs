//! QEMU runtime - spawns microVMs using QEMU
//!
//! Works on Linux (with KVM acceleration) and macOS (with HVF acceleration).
//! Falls back to software emulation (TCG) if no hardware acceleration available.
//!
//! ## Platform Support
//! - Linux with KVM: Full speed hardware virtualization
//! - macOS with HVF: Near-native speed via Hypervisor.framework
//! - Any platform: Works with TCG (slow, for testing only)
//!
//! ## QEMU Binary
//! Requires `qemu-system-x86_64` (or `qemu-system-aarch64` on ARM) in PATH.

use super::{Runtime, RuntimeHandle, RuntimeType, SpawnConfig};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Counter for unique instance IDs
static NEXT_INSTANCE_ID: AtomicU32 = AtomicU32::new(1);

/// Runtime that spawns QEMU microVMs
///
/// This runtime spawns QEMU processes with virtio-serial for guest communication.
/// Uses QMP (QEMU Machine Protocol) for VM control.
pub struct QemuRuntime {
    /// Path to QEMU binary (defaults to finding in PATH)
    qemu_bin: Option<PathBuf>,
    /// Use microvm machine type (faster boot) if available
    use_microvm: bool,
}

impl QemuRuntime {
    pub fn new() -> Self {
        Self {
            qemu_bin: None,
            use_microvm: false,
        }
    }

    pub fn with_binary(path: PathBuf) -> Self {
        Self {
            qemu_bin: Some(path),
            use_microvm: false,
        }
    }

    pub fn with_microvm(mut self, use_microvm: bool) -> Self {
        self.use_microvm = use_microvm;
        self
    }

    /// Find QEMU binary for current architecture
    fn find_qemu(&self) -> Option<PathBuf> {
        if let Some(ref path) = self.qemu_bin {
            if path.exists() {
                return Some(path.clone());
            }
        }

        // Determine binary name based on host architecture
        let binary_name = if cfg!(target_arch = "aarch64") {
            "qemu-system-aarch64"
        } else {
            "qemu-system-x86_64"
        };

        // Check common locations
        for dir in &[
            "/usr/local/bin",
            "/usr/bin",
            "/opt/homebrew/bin", // macOS ARM homebrew
            "/opt/local/bin",    // MacPorts
        ] {
            let p = PathBuf::from(dir).join(binary_name);
            if p.exists() {
                return Some(p);
            }
        }

        // Try PATH
        std::env::var("PATH").ok().and_then(|path| {
            for dir in path.split(':') {
                let p = PathBuf::from(dir).join(binary_name);
                if p.exists() {
                    return Some(p);
                }
            }
            None
        })
    }

    /// Check if KVM is available (Linux)
    fn has_kvm() -> bool {
        std::path::Path::new("/dev/kvm").exists()
    }

    /// Check if HVF is available (macOS)
    fn has_hvf() -> bool {
        #[cfg(target_os = "macos")]
        {
            // Check if running on Apple Silicon or Intel with HVF support
            // HVF is available on macOS 10.10+ for Intel and all Apple Silicon
            true
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// Get the best available acceleration method
    fn get_accel() -> &'static str {
        if Self::has_kvm() {
            "kvm"
        } else if Self::has_hvf() {
            "hvf"
        } else {
            "tcg" // Software emulation fallback
        }
    }

    /// Allocate a unique instance ID
    fn allocate_id() -> u32 {
        NEXT_INSTANCE_ID.fetch_add(1, Ordering::SeqCst)
    }

    /// Wait for QMP socket to become available and perform handshake
    async fn wait_for_qmp(socket_path: &PathBuf, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if socket_path.exists() {
                // Try to connect and do QMP handshake
                if let Ok(stream) = UnixStream::connect(socket_path).await {
                    let (reader, mut writer) = stream.into_split();
                    let mut reader = BufReader::new(reader);

                    // Read QMP greeting
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.is_ok() && line.contains("QMP") {
                        // Send qmp_capabilities to enter command mode
                        if writer
                            .write_all(b"{\"execute\": \"qmp_capabilities\"}\n")
                            .await
                            .is_ok()
                        {
                            line.clear();
                            if reader.read_line(&mut line).await.is_ok()
                                && line.contains("return")
                            {
                                return Ok(());
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        anyhow::bail!(
            "QMP socket {:?} not ready after {:?}",
            socket_path,
            timeout
        )
    }

    /// Get detailed availability status
    pub fn availability_details(&self) -> String {
        let mut details = Vec::new();

        if let Some(qemu_path) = self.find_qemu() {
            details.push(format!("QEMU binary: {}", qemu_path.display()));
        } else {
            details.push("QEMU binary: NOT FOUND".to_string());
        }

        let accel = Self::get_accel();
        details.push(format!(
            "Acceleration: {} ({})",
            accel,
            match accel {
                "kvm" => "hardware - Linux KVM",
                "hvf" => "hardware - macOS Hypervisor.framework",
                "tcg" => "software emulation (slow)",
                _ => "unknown",
            }
        ));

        details.join("\n")
    }
}

impl Default for QemuRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Runtime for QemuRuntime {
    async fn spawn(&self, config: &SpawnConfig) -> Result<RuntimeHandle> {
        let vm_config = config
            .vm_config
            .as_ref()
            .context("VmConfig is required for QEMU runtime")?;

        let qemu_bin = self.find_qemu().context(
            "QEMU binary not found.\n\
            Install QEMU:\n\
              - macOS: brew install qemu\n\
              - Ubuntu/Debian: apt install qemu-system-x86\n\
              - Fedora: dnf install qemu-system-x86",
        )?;

        if !vm_config.kernel.exists() {
            anyhow::bail!(
                "Kernel image not found: {}\n\
                For testing, you can use a minimal Linux kernel.",
                vm_config.kernel.display()
            );
        }

        if !vm_config.rootfs.exists() {
            anyhow::bail!(
                "Root filesystem not found: {}",
                vm_config.rootfs.display()
            );
        }

        let instance_id = Self::allocate_id();
        let socket_dir = config
            .socket
            .parent()
            .unwrap_or_else(|| std::path::Path::new("/tmp"));
        let instance_name = config
            .socket
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("qemu");

        let qmp_socket = socket_dir.join(format!("qemu-{}-{}-qmp.sock", instance_name, instance_id));
        let serial_socket =
            socket_dir.join(format!("qemu-{}-{}-serial.sock", instance_name, instance_id));

        // Clean up old sockets
        std::fs::remove_file(&qmp_socket).ok();
        std::fs::remove_file(&serial_socket).ok();

        let accel = Self::get_accel();
        info!(
            "Spawning QEMU VM: kernel={}, rootfs={}, memory={}MB, vcpus={}, accel={}",
            vm_config.kernel.display(),
            vm_config.rootfs.display(),
            vm_config.memory_mb,
            vm_config.vcpus,
            accel
        );

        // Build QEMU command
        let mut cmd = Command::new(&qemu_bin);

        // Machine type
        if self.use_microvm && accel == "kvm" {
            // microvm is only available on Linux with KVM
            cmd.arg("-M").arg("microvm,x-option-roms=off,rtc=off");
        } else if cfg!(target_arch = "aarch64") {
            cmd.arg("-M").arg("virt");
        } else {
            cmd.arg("-M").arg("q35");
        }

        // CPU and memory
        cmd.arg("-accel").arg(accel);
        cmd.arg("-cpu").arg(if accel == "hvf" && cfg!(target_arch = "x86_64") {
            "host"
        } else if accel == "kvm" {
            "host"
        } else {
            "max"
        });
        cmd.arg("-smp").arg(vm_config.vcpus.to_string());
        cmd.arg("-m").arg(format!("{}M", vm_config.memory_mb));

        // Kernel and boot args
        cmd.arg("-kernel").arg(&vm_config.kernel);
        let boot_args = "console=ttyS0 root=/dev/vda rw";
        cmd.arg("-append").arg(boot_args);

        // Root filesystem
        cmd.arg("-drive")
            .arg(format!(
                "file={},format=raw,if=virtio",
                vm_config.rootfs.display()
            ));

        // QMP control socket
        cmd.arg("-qmp")
            .arg(format!("unix:{},server,nowait", qmp_socket.display()));

        // Serial console as Unix socket (for guest communication)
        cmd.arg("-serial")
            .arg(format!("unix:{},server,nowait", serial_socket.display()));

        // No display
        cmd.arg("-nographic");
        cmd.arg("-nodefaults");

        // Disable unnecessary devices for faster boot
        cmd.arg("-no-user-config");

        debug!("QEMU command: {:?}", cmd);

        let child = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn QEMU at {:?}", qemu_bin))?;

        info!("QEMU process started, waiting for QMP socket...");

        // Wait for QMP socket to be ready
        if let Err(e) = Self::wait_for_qmp(&qmp_socket, Duration::from_secs(10)).await {
            warn!("QMP socket not ready: {}", e);
            // Don't fail - QEMU might still be booting
        } else {
            info!("QMP socket ready");
        }

        info!(
            "QEMU VM started: qmp={}, serial={}",
            qmp_socket.display(),
            serial_socket.display()
        );

        Ok(RuntimeHandle::Qemu {
            child,
            qmp_socket,
            serial_socket,
        })
    }

    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Qemu
    }

    fn is_available(&self) -> bool {
        self.find_qemu().is_some()
    }

    fn name(&self) -> &'static str {
        "qemu"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qemu_runtime_type() {
        let runtime = QemuRuntime::new();
        assert_eq!(runtime.runtime_type(), RuntimeType::Qemu);
    }

    #[test]
    fn test_qemu_runtime_name() {
        let runtime = QemuRuntime::new();
        assert_eq!(runtime.name(), "qemu");
    }

    #[test]
    fn test_instance_id_allocation() {
        let id1 = QemuRuntime::allocate_id();
        let id2 = QemuRuntime::allocate_id();
        assert!(id2 > id1);
    }

    #[test]
    fn test_availability_details() {
        let runtime = QemuRuntime::new();
        let details = runtime.availability_details();
        assert!(!details.is_empty());
        assert!(details.contains("QEMU binary"));
        assert!(details.contains("Acceleration"));
    }

    #[test]
    fn test_with_binary() {
        let runtime = QemuRuntime::with_binary(PathBuf::from("/custom/qemu"));
        assert_eq!(runtime.qemu_bin, Some(PathBuf::from("/custom/qemu")));
    }

    #[test]
    fn test_with_microvm() {
        let runtime = QemuRuntime::new().with_microvm(true);
        assert!(runtime.use_microvm);
    }

    #[test]
    fn test_get_accel() {
        let accel = QemuRuntime::get_accel();
        // Should return one of: kvm, hvf, or tcg
        assert!(["kvm", "hvf", "tcg"].contains(&accel));
    }

    #[tokio::test]
    async fn test_qemu_spawn_missing_vm_config() {
        use std::collections::HashMap;

        let runtime = QemuRuntime::new();
        let config = SpawnConfig {
            command: String::new(),
            args: vec![],
            env: HashMap::new(),
            socket: PathBuf::from("/tmp/test-qemu.sock"),
            workdir: None,
            vm_config: None,
        };

        let result = runtime.spawn(&config).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("VmConfig is required"));
    }
}
