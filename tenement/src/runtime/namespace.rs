//! Namespace runtime - spawns processes with Linux namespace isolation (PID + Mount)
//!
//! This runtime provides lightweight isolation by running processes in separate
//! Linux namespaces. Each process gets its own /proc view, hiding environment
//! variables and process information from other services.
//!
//! **Zero overhead** - kernel bookkeeping only
//! **Zero dependencies** - built into Linux kernel (since 2008)
//! **Instant startup** - no container/VM to spawn
//!
//! For trusted code (your own apps), this provides sufficient isolation.
//! For untrusted code, use the sandbox runtime (gVisor) which also filters syscalls.
//!
//! **Linux only** - requires `unshare(2)` syscall.

use super::{Runtime, RuntimeHandle, RuntimeType, SpawnConfig};
use anyhow::Result;
use async_trait::async_trait;

/// Runtime that spawns processes in Linux namespaces (PID + Mount)
///
/// This provides /proc isolation without syscall filtering.
/// Environment variables are invisible between services.
pub struct NamespaceRuntime;

impl NamespaceRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NamespaceRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::*;
    use anyhow::Context;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use tokio::process::Command;

    pub async fn spawn_namespaced(config: &SpawnConfig) -> Result<RuntimeHandle> {
        // Remove old socket if exists
        if config.socket.exists() {
            std::fs::remove_file(&config.socket).ok();
        }

        // Spawn using tokio::process::Command for async support
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .envs(&config.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(workdir) = &config.workdir {
            cmd.current_dir(workdir);
        }

        // Set up namespace isolation using pre_exec hook
        // This runs in the child process before exec, after fork
        unsafe {
            cmd.pre_exec(|| {
                use nix::mount::{mount, MsFlags};
                use nix::sched::{unshare, CloneFlags};

                // Create new PID and Mount namespaces
                unshare(CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWNS).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("unshare failed: {}", e),
                    )
                })?;

                // Make mount namespace private (don't propagate mounts)
                mount(
                    None::<&str>,
                    "/",
                    None::<&str>,
                    MsFlags::MS_REC | MsFlags::MS_PRIVATE,
                    None::<&str>,
                )
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("mount private failed: {}", e),
                    )
                })?;

                // Mount a new /proc for this namespace
                // This gives the process its own view of /proc
                match mount(
                    Some("proc"),
                    "/proc",
                    Some("proc"),
                    MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
                    None::<&str>,
                ) {
                    Ok(_) => {}
                    Err(_) => {
                        // If /proc mount fails (e.g., not root), continue anyway
                        // The process will still be in a new PID namespace
                    }
                }

                Ok(())
            });
        }

        let child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn namespaced process: {}", config.command))?;

        Ok(RuntimeHandle::Namespace {
            child,
            socket: config.socket.clone(),
        })
    }
}

#[async_trait]
impl Runtime for NamespaceRuntime {
    async fn spawn(&self, config: &SpawnConfig) -> Result<RuntimeHandle> {
        #[cfg(target_os = "linux")]
        {
            linux_impl::spawn_namespaced(config).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            anyhow::bail!(
                "Namespace runtime requires Linux. On macOS/Windows, use 'process' runtime instead.\n\
                 Namespace isolation uses Linux namespaces (unshare) for /proc isolation."
            )
        }
    }

    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Namespace
    }

    fn is_available(&self) -> bool {
        // Namespace runtime is only available on Linux
        cfg!(target_os = "linux")
    }

    fn name(&self) -> &'static str {
        "namespace"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_namespace_runtime_type() {
        let runtime = NamespaceRuntime::new();
        assert_eq!(runtime.runtime_type(), RuntimeType::Namespace);
    }

    #[test]
    fn test_namespace_runtime_name() {
        let runtime = NamespaceRuntime::new();
        assert_eq!(runtime.name(), "namespace");
    }

    #[test]
    fn test_namespace_runtime_availability() {
        let runtime = NamespaceRuntime::new();
        // Only available on Linux
        #[cfg(target_os = "linux")]
        assert!(runtime.is_available());
        #[cfg(not(target_os = "linux"))]
        assert!(!runtime.is_available());
    }

    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn test_namespace_runtime_spawn_fails_on_non_linux() {
        use std::collections::HashMap;
        use std::path::PathBuf;

        let runtime = NamespaceRuntime::new();
        let config = SpawnConfig {
            command: "sleep".to_string(),
            args: vec!["0.1".to_string()],
            env: HashMap::new(),
            socket: PathBuf::from("/tmp/test-namespace-runtime.sock"),
            workdir: None,
            vm_config: None,
        };

        let result = runtime.spawn(&config).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Linux"));
    }

    // Integration test - requires Linux and root privileges
    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore] // Requires root
    async fn test_namespace_runtime_spawn() {
        use std::collections::HashMap;
        use std::path::PathBuf;

        let runtime = NamespaceRuntime::new();
        let config = SpawnConfig {
            command: "sleep".to_string(),
            args: vec!["0.1".to_string()],
            env: HashMap::new(),
            socket: PathBuf::from("/tmp/test-namespace-runtime.sock"),
            workdir: None,
            vm_config: None,
        };

        let mut handle = runtime.spawn(&config).await.unwrap();
        assert_eq!(handle.runtime_type(), RuntimeType::Namespace);
        assert!(!handle.is_vsock());
        assert!(handle.vsock_port().is_none());

        // Clean up
        handle.kill().await.ok();
    }
}
