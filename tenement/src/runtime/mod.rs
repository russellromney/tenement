//! Runtime abstraction for process and VM execution
//!
//! Provides a trait-based abstraction that allows different runtime backends
//! (bare processes, Linux namespaces, Firecracker VMs, QEMU, etc.) to be used interchangeably.

mod process;
mod namespace;

#[cfg(feature = "firecracker")]
mod firecracker;

#[cfg(feature = "qemu")]
mod qemu;

#[cfg(feature = "sandbox")]
mod sandbox;

pub use process::ProcessRuntime;
pub use namespace::NamespaceRuntime;

#[cfg(feature = "firecracker")]
pub use firecracker::FirecrackerRuntime;

#[cfg(feature = "qemu")]
pub use qemu::QemuRuntime;

#[cfg(feature = "sandbox")]
pub use sandbox::SandboxRuntime;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::process::Child;

/// Runtime type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeType {
    /// Bare process - no isolation, same trust boundary
    Process,
    /// Linux namespace isolation (PID + Mount namespaces) - default
    #[default]
    Namespace,
    /// gVisor sandbox - syscall filtering for untrusted code
    Sandbox,
    Firecracker,
    Qemu,
}

impl std::fmt::Display for RuntimeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeType::Process => write!(f, "process"),
            RuntimeType::Namespace => write!(f, "namespace"),
            RuntimeType::Sandbox => write!(f, "sandbox"),
            RuntimeType::Firecracker => write!(f, "firecracker"),
            RuntimeType::Qemu => write!(f, "qemu"),
        }
    }
}

impl std::str::FromStr for RuntimeType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "process" => Ok(RuntimeType::Process),
            "namespace" => Ok(RuntimeType::Namespace),
            "sandbox" | "gvisor" => Ok(RuntimeType::Sandbox),
            "firecracker" => Ok(RuntimeType::Firecracker),
            "qemu" => Ok(RuntimeType::Qemu),
            _ => anyhow::bail!("Unknown runtime type: {}. Use 'process', 'namespace', 'sandbox', 'firecracker', or 'qemu'", s),
        }
    }
}

/// Handle to a running instance
///
/// This provides a runtime-agnostic way to track and manage a spawned instance.
/// For processes, this wraps a tokio Child. For VMs, this would wrap VM handles.
#[derive(Debug)]
pub enum RuntimeHandle {
    /// A bare process
    Process {
        child: Child,
        socket: PathBuf,
    },
    /// A namespaced process (Linux PID + Mount namespaces)
    Namespace {
        child: Child,
        socket: PathBuf,
    },
    /// A Firecracker microVM
    #[allow(dead_code)]
    Firecracker {
        /// Path to Firecracker API socket
        api_socket: PathBuf,
        /// Path to vsock Unix socket for guest communication
        vsock_socket: PathBuf,
        /// Guest CID for vsock
        cid: u32,
        /// Guest vsock port
        port: u32,
    },
    /// A QEMU microVM
    #[allow(dead_code)]
    Qemu {
        /// The QEMU process
        child: Child,
        /// Path to QMP (QEMU Machine Protocol) socket for control
        qmp_socket: PathBuf,
        /// Path to virtio-serial socket for guest communication
        serial_socket: PathBuf,
    },
    /// A gVisor sandboxed container
    #[allow(dead_code)]
    Sandbox {
        /// Container ID (for runsc commands)
        container_id: String,
        /// Path to OCI bundle directory
        bundle_path: PathBuf,
        /// Path to runsc state directory
        state_dir: PathBuf,
        /// Socket path (bind-mounted into container)
        socket: PathBuf,
    },
}

impl RuntimeHandle {
    /// Get the socket path for this instance
    pub fn socket(&self) -> &PathBuf {
        match self {
            RuntimeHandle::Process { socket, .. } => socket,
            RuntimeHandle::Namespace { socket, .. } => socket,
            RuntimeHandle::Firecracker { vsock_socket, .. } => vsock_socket,
            RuntimeHandle::Qemu { serial_socket, .. } => serial_socket,
            RuntimeHandle::Sandbox { socket, .. } => socket,
        }
    }

    /// Get the runtime type
    pub fn runtime_type(&self) -> RuntimeType {
        match self {
            RuntimeHandle::Process { .. } => RuntimeType::Process,
            RuntimeHandle::Namespace { .. } => RuntimeType::Namespace,
            RuntimeHandle::Sandbox { .. } => RuntimeType::Sandbox,
            RuntimeHandle::Firecracker { .. } => RuntimeType::Firecracker,
            RuntimeHandle::Qemu { .. } => RuntimeType::Qemu,
        }
    }

    /// Check if this is a vsock-based runtime (requires CONNECT protocol)
    pub fn is_vsock(&self) -> bool {
        matches!(self, RuntimeHandle::Firecracker { .. })
    }

    /// Get vsock port if applicable
    pub fn vsock_port(&self) -> Option<u32> {
        match self {
            RuntimeHandle::Firecracker { port, .. } => Some(*port),
            _ => None,
        }
    }

    /// Get the process ID (for process/namespace runtimes)
    pub fn pid(&self) -> Option<u32> {
        match self {
            RuntimeHandle::Process { child, .. } | RuntimeHandle::Namespace { child, .. } => {
                child.id()
            }
            RuntimeHandle::Qemu { child, .. } => child.id(),
            // VM/sandbox runtimes don't expose a simple PID
            RuntimeHandle::Firecracker { .. } | RuntimeHandle::Sandbox { .. } => None,
        }
    }

    /// Kill the underlying process/VM
    pub async fn kill(&mut self) -> Result<()> {
        match self {
            RuntimeHandle::Process { child, .. } | RuntimeHandle::Namespace { child, .. } => {
                child.kill().await?;
                Ok(())
            }
            RuntimeHandle::Firecracker {
                api_socket,
                vsock_socket,
                ..
            } => {
                // For Firecracker, we need to find and kill the process that owns the API socket.
                // The cleanest way is to use pkill or find the PID via lsof.
                // For simplicity, we'll use the Unix socket path to find the process.
                #[cfg(target_os = "linux")]
                {
                    use tokio::process::Command;

                    // Try to send InstanceHalt action first (graceful shutdown)
                    if api_socket.exists() {
                        // Best effort graceful shutdown
                        let _ = Self::fc_api_put(api_socket, "/actions", r#"{"action_type": "SendCtrlAltDel"}"#).await;
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }

                    // Find and kill the firecracker process using fuser
                    if api_socket.exists() {
                        let _ = Command::new("fuser")
                            .arg("-k")
                            .arg(api_socket)
                            .output()
                            .await;
                    }

                    // Clean up sockets
                    std::fs::remove_file(api_socket).ok();
                    std::fs::remove_file(vsock_socket).ok();

                    Ok(())
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = (api_socket, vsock_socket);
                    anyhow::bail!("Firecracker only supported on Linux")
                }
            }
            RuntimeHandle::Qemu {
                child,
                qmp_socket,
                serial_socket,
            } => {
                // For QEMU, we can send quit command via QMP or just kill the process
                // Try graceful shutdown first via QMP
                if qmp_socket.exists() {
                    let _ = Self::qemu_qmp_quit(qmp_socket).await;
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }

                // If still running, force kill
                let _ = child.kill().await;

                // Clean up sockets
                std::fs::remove_file(qmp_socket).ok();
                std::fs::remove_file(serial_socket).ok();

                Ok(())
            }
            RuntimeHandle::Sandbox {
                container_id,
                bundle_path,
                state_dir,
                socket,
            } => {
                // For gVisor sandbox, use runsc commands to stop and clean up
                #[cfg(target_os = "linux")]
                {
                    use tokio::process::Command;

                    // Kill the container: runsc kill <id> SIGKILL
                    let _ = Command::new("runsc")
                        .arg("kill")
                        .arg("--root")
                        .arg(&state_dir)
                        .arg(&container_id)
                        .arg("SIGKILL")
                        .output()
                        .await;

                    // Wait briefly for cleanup
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                    // Delete the container: runsc delete <id>
                    let _ = Command::new("runsc")
                        .arg("delete")
                        .arg("--root")
                        .arg(&state_dir)
                        .arg("--force")
                        .arg(&container_id)
                        .output()
                        .await;

                    // Clean up bundle directory
                    std::fs::remove_dir_all(&bundle_path).ok();

                    // Clean up socket
                    std::fs::remove_file(&socket).ok();

                    Ok(())
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = (container_id, bundle_path, state_dir, socket);
                    anyhow::bail!("Sandbox (gVisor) only supported on Linux")
                }
            }
        }
    }

    /// Helper to send quit command via QMP (QEMU Machine Protocol)
    #[allow(dead_code)]
    async fn qemu_qmp_quit(socket_path: &PathBuf) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixStream;

        let stream = UnixStream::connect(socket_path).await?;
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Read QMP greeting
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        // Send qmp_capabilities to enter command mode
        writer
            .write_all(b"{\"execute\": \"qmp_capabilities\"}\n")
            .await?;
        line.clear();
        reader.read_line(&mut line).await?;

        // Send quit command
        writer.write_all(b"{\"execute\": \"quit\"}\n").await?;

        Ok(())
    }

    /// Helper to send HTTP PUT to Firecracker API (used for shutdown)
    #[cfg(target_os = "linux")]
    async fn fc_api_put(socket_path: &PathBuf, endpoint: &str, body: &str) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::UnixStream;

        let mut stream = UnixStream::connect(socket_path).await?;
        let request = format!(
            "PUT {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            endpoint,
            body.len(),
            body
        );
        stream.write_all(request.as_bytes()).await?;
        let mut buf = vec![0u8; 1024];
        let _ = stream.read(&mut buf).await;
        Ok(())
    }

    /// Check if the process/VM is still running
    pub async fn is_running(&mut self) -> bool {
        match self {
            RuntimeHandle::Process { child, .. } | RuntimeHandle::Namespace { child, .. } => {
                // try_wait returns Ok(Some(status)) if exited, Ok(None) if still running
                matches!(child.try_wait(), Ok(None))
            }
            RuntimeHandle::Firecracker { api_socket, .. } => {
                // Check if API socket exists
                api_socket.exists()
            }
            RuntimeHandle::Qemu { child, .. } => {
                // try_wait returns Ok(Some(status)) if exited, Ok(None) if still running
                matches!(child.try_wait(), Ok(None))
            }
            RuntimeHandle::Sandbox {
                container_id,
                state_dir,
                ..
            } => {
                // Use runsc state to check if container is running
                #[cfg(target_os = "linux")]
                {
                    use tokio::process::Command;

                    let output = Command::new("runsc")
                        .arg("state")
                        .arg("--root")
                        .arg(&state_dir)
                        .arg(&container_id)
                        .output()
                        .await;

                    match output {
                        Ok(o) if o.status.success() => {
                            // Parse JSON output, check status == "running"
                            if let Ok(state) =
                                serde_json::from_slice::<serde_json::Value>(&o.stdout)
                            {
                                state["status"] == "running"
                            } else {
                                false
                            }
                        }
                        _ => false,
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = (container_id, state_dir);
                    false
                }
            }
        }
    }
}

/// Configuration for spawning an instance
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    /// Command to run (for process runtime)
    pub command: String,
    /// Command arguments
    pub args: Vec<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Socket path for the instance
    pub socket: PathBuf,
    /// Working directory
    pub workdir: Option<PathBuf>,
    /// Firecracker-specific config
    pub vm_config: Option<VmConfig>,
}

/// Firecracker VM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    /// Memory in MB
    pub memory_mb: u32,
    /// Number of vCPUs
    pub vcpus: u8,
    /// Path to kernel image
    pub kernel: PathBuf,
    /// Path to root filesystem
    pub rootfs: PathBuf,
    /// vsock port inside guest
    pub vsock_port: u32,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            memory_mb: 128,
            vcpus: 1,
            kernel: PathBuf::new(),
            rootfs: PathBuf::new(),
            vsock_port: 5000,
        }
    }
}

/// Trait for runtime backends
///
/// Implement this trait to add new runtime types (process, Firecracker, WASM, etc.)
#[async_trait]
pub trait Runtime: Send + Sync {
    /// Spawn a new instance
    async fn spawn(&self, config: &SpawnConfig) -> Result<RuntimeHandle>;

    /// Runtime type identifier
    fn runtime_type(&self) -> RuntimeType;

    /// Check if this runtime is available on the current system
    fn is_available(&self) -> bool;

    /// Human-readable name for error messages
    fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_type_default() {
        let rt: RuntimeType = Default::default();
        assert_eq!(rt, RuntimeType::Namespace);
    }

    #[test]
    fn test_runtime_type_display() {
        assert_eq!(RuntimeType::Process.to_string(), "process");
        assert_eq!(RuntimeType::Namespace.to_string(), "namespace");
        assert_eq!(RuntimeType::Sandbox.to_string(), "sandbox");
        assert_eq!(RuntimeType::Firecracker.to_string(), "firecracker");
        assert_eq!(RuntimeType::Qemu.to_string(), "qemu");
    }

    #[test]
    fn test_runtime_type_from_str() {
        assert_eq!("process".parse::<RuntimeType>().unwrap(), RuntimeType::Process);
        assert_eq!("namespace".parse::<RuntimeType>().unwrap(), RuntimeType::Namespace);
        assert_eq!("sandbox".parse::<RuntimeType>().unwrap(), RuntimeType::Sandbox);
        assert_eq!("gvisor".parse::<RuntimeType>().unwrap(), RuntimeType::Sandbox);
        assert_eq!("firecracker".parse::<RuntimeType>().unwrap(), RuntimeType::Firecracker);
        assert_eq!("qemu".parse::<RuntimeType>().unwrap(), RuntimeType::Qemu);
        assert_eq!("PROCESS".parse::<RuntimeType>().unwrap(), RuntimeType::Process);
        assert_eq!("NAMESPACE".parse::<RuntimeType>().unwrap(), RuntimeType::Namespace);
        assert_eq!("SANDBOX".parse::<RuntimeType>().unwrap(), RuntimeType::Sandbox);
        assert_eq!("QEMU".parse::<RuntimeType>().unwrap(), RuntimeType::Qemu);
        assert!("invalid".parse::<RuntimeType>().is_err());
    }

    #[test]
    fn test_runtime_type_serde() {
        let rt = RuntimeType::Firecracker;
        let json = serde_json::to_string(&rt).unwrap();
        assert_eq!(json, "\"firecracker\"");

        let rt_namespace = RuntimeType::Namespace;
        let json_namespace = serde_json::to_string(&rt_namespace).unwrap();
        assert_eq!(json_namespace, "\"namespace\"");

        let rt_sandbox = RuntimeType::Sandbox;
        let json_sandbox = serde_json::to_string(&rt_sandbox).unwrap();
        assert_eq!(json_sandbox, "\"sandbox\"");

        let rt_qemu = RuntimeType::Qemu;
        let json_qemu = serde_json::to_string(&rt_qemu).unwrap();
        assert_eq!(json_qemu, "\"qemu\"");

        let parsed: RuntimeType = serde_json::from_str("\"process\"").unwrap();
        assert_eq!(parsed, RuntimeType::Process);

        let parsed_namespace: RuntimeType = serde_json::from_str("\"namespace\"").unwrap();
        assert_eq!(parsed_namespace, RuntimeType::Namespace);

        let parsed_sandbox: RuntimeType = serde_json::from_str("\"sandbox\"").unwrap();
        assert_eq!(parsed_sandbox, RuntimeType::Sandbox);

        let parsed_qemu: RuntimeType = serde_json::from_str("\"qemu\"").unwrap();
        assert_eq!(parsed_qemu, RuntimeType::Qemu);
    }

    #[test]
    fn test_vm_config_default() {
        let config = VmConfig::default();
        assert_eq!(config.memory_mb, 128);
        assert_eq!(config.vcpus, 1);
        assert_eq!(config.vsock_port, 5000);
    }
}
