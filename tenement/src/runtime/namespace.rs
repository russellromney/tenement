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
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::process::Stdio;
    use tokio::process::Command;

    pub async fn spawn_namespaced(config: &SpawnConfig) -> Result<RuntimeHandle> {
        // Remove old socket if exists
        if config.socket.exists() {
            std::fs::remove_file(&config.socket).ok();
        }

        // Validate rootfs up front so the caller gets a clear error before fork.
        if let Some(rootfs) = &config.rootfs {
            if !rootfs.is_dir() {
                anyhow::bail!(
                    "namespace rootfs {:?} does not exist or is not a directory",
                    rootfs
                );
            }
        }

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .envs(&config.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // When rootfs is set, chdir happens *inside* the new root in pre_exec.
        // Calling current_dir() here would chdir on the host, failing for guest-only
        // paths like /app.
        if config.rootfs.is_none() {
            if let Some(workdir) = &config.workdir {
                cmd.current_dir(workdir);
            }
        }

        // Build CStrings for the rootfs path and workdir-inside-root before fork.
        // pre_exec is async-signal-context; no allocations or panics allowed.
        let rootfs_c = match &config.rootfs {
            Some(p) => Some(
                CString::new(p.as_os_str().as_bytes()).context("rootfs path contains NUL byte")?,
            ),
            None => None,
        };
        let chdir_target_c = if config.rootfs.is_some() {
            let inside = config
                .workdir
                .as_ref()
                .map(|p| p.as_os_str().as_bytes().to_vec())
                .unwrap_or_else(|| b"/".to_vec());
            Some(CString::new(inside).context("workdir contains NUL byte")?)
        } else {
            None
        };

        unsafe {
            cmd.pre_exec(move || {
                // Put child in its own process group so we can kill all descendants
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }

                use nix::mount::{mount, MsFlags};
                use nix::sched::{unshare, CloneFlags};

                // Create new PID and Mount namespaces
                unshare(CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWNS)
                    .map_err(|e| std::io::Error::other(format!("unshare failed: {}", e)))?;

                // Make mount namespace private (don't propagate mounts)
                mount(
                    None::<&str>,
                    "/",
                    None::<&str>,
                    MsFlags::MS_REC | MsFlags::MS_PRIVATE,
                    None::<&str>,
                )
                .map_err(|e| std::io::Error::other(format!("mount private failed: {}", e)))?;

                if let Some(rootfs) = rootfs_c.as_ref() {
                    // Bind-mount rootfs onto itself so it becomes a mount point we can chroot into.
                    mount(
                        Some(rootfs.as_c_str()),
                        rootfs.as_c_str(),
                        None::<&std::ffi::CStr>,
                        MsFlags::MS_BIND | MsFlags::MS_REC,
                        None::<&std::ffi::CStr>,
                    )
                    .map_err(|e| {
                        std::io::Error::other(format!("rootfs bind-mount failed: {}", e))
                    })?;

                    // chroot into the new rootfs.
                    if libc::chroot(rootfs.as_ptr()) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }

                    // chdir to workdir (or "/") *inside* the new root.
                    let chdir_target = chdir_target_c
                        .as_ref()
                        .expect("chdir_target_c set when rootfs is Some");
                    if libc::chdir(chdir_target.as_ptr()) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }

                    // Mount /proc inside the new root. Required for the new PID namespace
                    // to be useful; fail-closed because /proc-less containers will surprise
                    // any caller expecting namespace semantics.
                    mount(
                        Some("proc"),
                        "/proc",
                        Some("proc"),
                        MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
                        None::<&str>,
                    )
                    .map_err(|e| {
                        std::io::Error::other(format!("/proc mount in rootfs failed: {}", e))
                    })?;
                } else {
                    // Legacy path: no rootfs, mount /proc on host's /proc.
                    // Best-effort; missing CAP_SYS_ADMIN is tolerated here for back-compat.
                    let _ = mount(
                        Some("proc"),
                        "/proc",
                        Some("proc"),
                        MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
                        None::<&str>,
                    );
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
            rootfs: None,
            vm_config: None,
            ..Default::default()
        };

        let result = runtime.spawn(&config).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Linux"));
    }

    // Pre-fork validation: bogus rootfs is rejected before we even spawn.
    // Runs without root because the check is a plain stat().
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn test_namespace_rejects_missing_rootfs() {
        use std::collections::HashMap;
        use std::path::PathBuf;

        let runtime = NamespaceRuntime::new();
        let config = SpawnConfig {
            command: "/bin/true".to_string(),
            args: vec![],
            env: HashMap::new(),
            socket: PathBuf::from("/tmp/test-namespace-bad-rootfs.sock"),
            workdir: Some(PathBuf::from("/app")),
            rootfs: Some(PathBuf::from("/nonexistent/tenement/rootfs-xyz")),
            vm_config: None,
            ..Default::default()
        };

        let err = runtime.spawn(&config).await.unwrap_err().to_string();
        assert!(err.contains("rootfs"), "got: {}", err);
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
            rootfs: None,
            vm_config: None,
            ..Default::default()
        };

        let mut handle = runtime.spawn(&config).await.unwrap();
        assert_eq!(handle.runtime_type(), RuntimeType::Namespace);
        assert!(!handle.is_vsock());
        assert!(handle.vsock_port().is_none());

        // Clean up
        handle.kill().await.ok();
    }
}
