//! Runtime abstraction for process and VM execution
//!
//! Provides a trait-based abstraction that allows different runtime backends
//! (bare processes, Linux namespaces, Firecracker VMs, QEMU, etc.) to be used interchangeably.

mod litebox;
mod namespace;
mod process;

#[cfg(feature = "firecracker")]
mod firecracker;

#[cfg(feature = "qemu")]
mod qemu;

#[cfg(feature = "sandbox")]
mod sandbox;

#[cfg(feature = "quark")]
mod quark;

// Shared docker/containerd helper for the container runtimes (quark, sandbox).
#[cfg(any(feature = "quark", feature = "sandbox"))]
mod container;

pub use litebox::LiteBoxRuntime;
pub use namespace::NamespaceRuntime;
pub use process::ProcessRuntime;

#[cfg(feature = "firecracker")]
pub use firecracker::FirecrackerRuntime;

#[cfg(feature = "qemu")]
pub use qemu::QemuRuntime;

#[cfg(feature = "sandbox")]
pub use sandbox::SandboxRuntime;

#[cfg(feature = "quark")]
pub use quark::QuarkRuntime;

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
    /// LiteBox library-OS sandbox - run via a configurable external runner
    Litebox,
    /// Quark - KVM-backed OCI runtime that boots the bundle rootfs as the guest /
    Quark,
    Firecracker,
    Qemu,
}

impl std::fmt::Display for RuntimeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeType::Process => write!(f, "process"),
            RuntimeType::Namespace => write!(f, "namespace"),
            RuntimeType::Sandbox => write!(f, "sandbox"),
            RuntimeType::Litebox => write!(f, "litebox"),
            RuntimeType::Quark => write!(f, "quark"),
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
            "litebox" => Ok(RuntimeType::Litebox),
            "quark" => Ok(RuntimeType::Quark),
            "firecracker" => Ok(RuntimeType::Firecracker),
            "qemu" => Ok(RuntimeType::Qemu),
            _ => anyhow::bail!("Unknown runtime type: {}. Use 'process', 'namespace', 'sandbox', 'litebox', 'quark', 'firecracker', or 'qemu'", s),
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
    Process { child: Child, socket: PathBuf },
    /// A namespaced process (Linux PID + Mount namespaces)
    Namespace { child: Child, socket: PathBuf },
    /// A LiteBox-sandboxed process, supervised via an external runner binary
    Litebox { child: Child, socket: PathBuf },
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
    /// A gVisor (runsc) container, run via docker/containerd
    /// (`docker run -d --runtime=runsc ...`). Tracked by container name, like
    /// [`RuntimeHandle::Quark`].
    #[allow(dead_code)]
    Sandbox {
        /// docker container name
        name: String,
        /// Socket path (unused for TCP routing; kept for the trait)
        socket: PathBuf,
    },
    /// A Quark (KVM) OCI container, run via docker/containerd
    /// (`docker run -d --runtime=quark ...`). We track it by container name and
    /// drive it with `docker` rather than holding a process — the container is
    /// owned by the docker/containerd daemon, not by us.
    #[allow(dead_code)]
    Quark {
        /// docker container name
        name: String,
        /// Socket path (unused for TCP routing; kept for the trait)
        socket: PathBuf,
    },
}

impl RuntimeHandle {
    /// Get the socket path for this instance
    pub fn socket(&self) -> &PathBuf {
        match self {
            RuntimeHandle::Process { socket, .. } => socket,
            RuntimeHandle::Namespace { socket, .. } => socket,
            RuntimeHandle::Litebox { socket, .. } => socket,
            RuntimeHandle::Firecracker { vsock_socket, .. } => vsock_socket,
            RuntimeHandle::Qemu { serial_socket, .. } => serial_socket,
            RuntimeHandle::Sandbox { socket, .. } => socket,
            RuntimeHandle::Quark { socket, .. } => socket,
        }
    }

    /// Get the runtime type
    pub fn runtime_type(&self) -> RuntimeType {
        match self {
            RuntimeHandle::Process { .. } => RuntimeType::Process,
            RuntimeHandle::Namespace { .. } => RuntimeType::Namespace,
            RuntimeHandle::Litebox { .. } => RuntimeType::Litebox,
            RuntimeHandle::Sandbox { .. } => RuntimeType::Sandbox,
            RuntimeHandle::Quark { .. } => RuntimeType::Quark,
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
            RuntimeHandle::Process { child, .. }
            | RuntimeHandle::Namespace { child, .. }
            | RuntimeHandle::Litebox { child, .. } => child.id(),
            RuntimeHandle::Qemu { child, .. } => child.id(),
            // VM/sandbox/container runtimes don't expose a simple PID
            RuntimeHandle::Firecracker { .. }
            | RuntimeHandle::Sandbox { .. }
            | RuntimeHandle::Quark { .. } => None,
        }
    }

    /// Stop the underlying process/VM gracefully, then force-kill on timeout.
    ///
    /// `grace` is the bounded window the instance gets to flush state and exit
    /// after the graceful stop signal:
    /// - **Process/Namespace/Litebox**: SIGTERM the process group, poll for exit
    ///   up to `grace`, then SIGKILL + reap if still alive.
    /// - **Quark/Sandbox**: `docker stop -t <grace_secs>` (Docker sends SIGTERM,
    ///   waits, then SIGKILL), then best-effort `docker rm -f`.
    /// - **Qemu/Firecracker**: already attempt a graceful shutdown of their own;
    ///   `grace` is unused for them.
    pub async fn kill(&mut self, grace: std::time::Duration) -> Result<()> {
        match self {
            RuntimeHandle::Process { child, .. }
            | RuntimeHandle::Namespace { child, .. }
            | RuntimeHandle::Litebox { child, .. } => {
                // Graceful stop: SIGTERM the whole process group (child + all
                // descendants), then give it `grace` to exit before SIGKILL.
                #[cfg(unix)]
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGTERM);
                    }

                    // Poll for a clean exit at ~100ms intervals up to `grace`.
                    let deadline = std::time::Instant::now() + grace;
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => {
                                // Exited on its own; reap and we're done.
                                let _ = child.wait().await;
                                return Ok(());
                            }
                            Ok(None) => {
                                if std::time::Instant::now() >= deadline {
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }
                            // Already reaped / unknown error: nothing more to do.
                            Err(_) => return Ok(()),
                        }
                    }
                }

                // Fallback: grace elapsed (or non-unix). Force-kill the group and reap.
                #[cfg(unix)]
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    }
                }
                let _ = child.kill().await;
                let _ = child.wait().await;
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
                        let _ = Self::fc_api_put(
                            api_socket,
                            "/actions",
                            r#"{"action_type": "SendCtrlAltDel"}"#,
                        )
                        .await;
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }

                    // Find and kill the firecracker process using fuser
                    if api_socket.exists() {
                        let _ = Command::new("fuser")
                            .arg("-k")
                            .arg(&*api_socket)
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
            RuntimeHandle::Quark { name, socket } | RuntimeHandle::Sandbox { name, socket } => {
                // Container runtimes (quark, gVisor) run via docker; the
                // container is owned by the daemon, so stop it gracefully by name.
                #[cfg(target_os = "linux")]
                {
                    use tokio::process::Command;
                    // `docker stop -t <secs>` sends SIGTERM, waits up to <secs>,
                    // then SIGKILL. Containers run with `--rm`, so a clean stop
                    // auto-removes them (and avoids the hard-kill re-spawn race).
                    let grace_secs = grace.as_secs().to_string();
                    let _ = Command::new("docker")
                        .arg("stop")
                        .arg("-t")
                        .arg(&grace_secs)
                        .arg(name.as_str())
                        .output()
                        .await;
                    // Best-effort cleanup in case `--rm` didn't fire (e.g. the
                    // container was already gone, or not started with --rm).
                    let _ = Command::new("docker")
                        .arg("rm")
                        .arg("-f")
                        .arg(name.as_str())
                        .output()
                        .await;
                    std::fs::remove_file(socket).ok();
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = (name, socket, grace);
                }
                Ok(())
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
            RuntimeHandle::Process { child, .. }
            | RuntimeHandle::Namespace { child, .. }
            | RuntimeHandle::Litebox { child, .. } => {
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
            RuntimeHandle::Quark { name, .. } | RuntimeHandle::Sandbox { name, .. } => {
                // Container runtimes (quark, gVisor): ask docker.
                #[cfg(target_os = "linux")]
                {
                    use tokio::process::Command;
                    let out = Command::new("docker")
                        .args(["inspect", "-f", "{{.State.Running}}", name.as_str()])
                        .output()
                        .await;
                    matches!(out, Ok(o) if o.status.success()
                        && String::from_utf8_lossy(&o.stdout).trim() == "true")
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = name;
                    false
                }
            }
        }
    }
}

/// A host->guest bind mount (used by OCI runtimes like Quark).
#[derive(Debug, Clone)]
pub struct Mount {
    /// Host source path
    pub source: PathBuf,
    /// Guest destination path (absolute, inside the rootfs)
    pub destination: PathBuf,
    /// Mount read-only
    pub readonly: bool,
}

/// Configuration for spawning an instance
#[derive(Debug, Clone, Default)]
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
    /// Bundle rootfs to boot as the guest root (Quark / OCI runtimes).
    /// For the Linux namespace runtime, this path becomes the chroot and
    /// `workdir` is interpreted inside the new root.
    pub rootfs: Option<PathBuf>,
    /// Host->guest bind mounts (Quark): e.g. app data dir -> /data.
    pub mounts: Vec<Mount>,
    /// OCI image reference to run (container runtimes that go through
    /// docker/containerd, e.g. Quark via `docker run --runtime=quark`).
    pub image: Option<String>,
    /// Memory limit in MB for container runtimes. Process-like runtimes use
    /// Tenement's cgroup manager instead.
    pub memory_limit_mb: Option<u32>,
    /// CPU weight/shares for container runtimes. Process-like runtimes use
    /// Tenement's cgroup manager instead.
    pub cpu_shares: Option<u32>,
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
        assert_eq!(RuntimeType::Litebox.to_string(), "litebox");
        assert_eq!(RuntimeType::Quark.to_string(), "quark");
        assert_eq!(RuntimeType::Firecracker.to_string(), "firecracker");
        assert_eq!(RuntimeType::Qemu.to_string(), "qemu");
    }

    #[test]
    fn test_runtime_type_from_str() {
        assert_eq!(
            "process".parse::<RuntimeType>().unwrap(),
            RuntimeType::Process
        );
        assert_eq!(
            "namespace".parse::<RuntimeType>().unwrap(),
            RuntimeType::Namespace
        );
        assert_eq!(
            "sandbox".parse::<RuntimeType>().unwrap(),
            RuntimeType::Sandbox
        );
        assert_eq!(
            "gvisor".parse::<RuntimeType>().unwrap(),
            RuntimeType::Sandbox
        );
        assert_eq!(
            "litebox".parse::<RuntimeType>().unwrap(),
            RuntimeType::Litebox
        );
        assert_eq!("quark".parse::<RuntimeType>().unwrap(), RuntimeType::Quark);
        assert_eq!(
            "firecracker".parse::<RuntimeType>().unwrap(),
            RuntimeType::Firecracker
        );
        assert_eq!("qemu".parse::<RuntimeType>().unwrap(), RuntimeType::Qemu);
        assert_eq!(
            "PROCESS".parse::<RuntimeType>().unwrap(),
            RuntimeType::Process
        );
        assert_eq!(
            "NAMESPACE".parse::<RuntimeType>().unwrap(),
            RuntimeType::Namespace
        );
        assert_eq!(
            "SANDBOX".parse::<RuntimeType>().unwrap(),
            RuntimeType::Sandbox
        );
        assert_eq!(
            "LITEBOX".parse::<RuntimeType>().unwrap(),
            RuntimeType::Litebox
        );
        assert_eq!("QUARK".parse::<RuntimeType>().unwrap(), RuntimeType::Quark);
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

        let rt_litebox = RuntimeType::Litebox;
        let json_litebox = serde_json::to_string(&rt_litebox).unwrap();
        assert_eq!(json_litebox, "\"litebox\"");

        let rt_quark = RuntimeType::Quark;
        let json_quark = serde_json::to_string(&rt_quark).unwrap();
        assert_eq!(json_quark, "\"quark\"");

        let rt_qemu = RuntimeType::Qemu;
        let json_qemu = serde_json::to_string(&rt_qemu).unwrap();
        assert_eq!(json_qemu, "\"qemu\"");

        let parsed: RuntimeType = serde_json::from_str("\"process\"").unwrap();
        assert_eq!(parsed, RuntimeType::Process);

        let parsed_namespace: RuntimeType = serde_json::from_str("\"namespace\"").unwrap();
        assert_eq!(parsed_namespace, RuntimeType::Namespace);

        let parsed_sandbox: RuntimeType = serde_json::from_str("\"sandbox\"").unwrap();
        assert_eq!(parsed_sandbox, RuntimeType::Sandbox);

        let parsed_litebox: RuntimeType = serde_json::from_str("\"litebox\"").unwrap();
        assert_eq!(parsed_litebox, RuntimeType::Litebox);

        let parsed_quark: RuntimeType = serde_json::from_str("\"quark\"").unwrap();
        assert_eq!(parsed_quark, RuntimeType::Quark);

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
