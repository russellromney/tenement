//! Sandbox runtime - spawns processes in gVisor (runsc) containers
//!
//! This runtime provides syscall-filtered execution for untrusted or multi-tenant
//! workloads using Google's gVisor. It creates an OCI bundle and runs the command
//! inside a sandboxed container.
//!
//! **~20MB memory overhead** per container
//! **<100ms startup** time
//! **Syscall filtering** - blocks dangerous syscalls
//! **Runs normal Linux binaries** - no recompilation needed
//!
//! For trusted code, use the namespace runtime (zero overhead).
//! For untrusted/multi-tenant code, use this sandbox runtime.
//!
//! **Linux only** - requires `runsc` (gVisor) installed.

use super::{Runtime, RuntimeHandle, RuntimeType, SpawnConfig};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

/// Runtime that spawns processes in gVisor sandboxed containers
///
/// This provides syscall filtering for untrusted code.
/// Uses OCI bundles with host filesystem symlinks.
pub struct SandboxRuntime {
    /// Optional custom path to runsc binary
    runsc_path: Option<PathBuf>,
}

impl SandboxRuntime {
    pub fn new() -> Self {
        Self { runsc_path: None }
    }
}

impl Default for SandboxRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::*;
    use anyhow::Context;
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;
    use tokio::process::Command;

    /// Find runsc binary in common locations
    pub fn find_runsc(custom_path: &Option<PathBuf>) -> Result<PathBuf> {
        // Check custom path first
        if let Some(ref path) = custom_path {
            if path.exists() {
                return Ok(path.clone());
            }
        }

        // Check standard locations
        for path in &[
            "/usr/local/bin/runsc",
            "/usr/bin/runsc",
            "/opt/gvisor/bin/runsc",
        ] {
            let p = PathBuf::from(path);
            if p.exists() {
                return Ok(p);
            }
        }

        // Try PATH environment variable
        if let Ok(path_env) = std::env::var("PATH") {
            for dir in path_env.split(':') {
                let p = PathBuf::from(dir).join("runsc");
                if p.exists() {
                    return Ok(p);
                }
            }
        }

        anyhow::bail!(
            "gVisor (runsc) not found.\n\n\
            Install gVisor:\n  \
            curl -fsSL https://gvisor.dev/archive.key | sudo gpg --dearmor -o /usr/share/keyrings/gvisor-archive-keyring.gpg\n  \
            echo 'deb [arch=amd64 signed-by=/usr/share/keyrings/gvisor-archive-keyring.gpg] https://storage.googleapis.com/gvisor/releases release main' | sudo tee /etc/apt/sources.list.d/gvisor.list\n  \
            sudo apt-get update && sudo apt-get install -y runsc\n\n\
            Or download directly:\n  \
            https://github.com/google/gvisor/releases"
        )
    }

    /// Create minimal rootfs with symlinks to host filesystem
    pub fn create_rootfs(rootfs_path: &Path) -> Result<()> {
        std::fs::create_dir_all(rootfs_path)?;

        // Create symlinks to host filesystem directories
        // This allows running any binary available on the host
        let symlinks = [
            ("bin", "/bin"),
            ("sbin", "/sbin"),
            ("lib", "/lib"),
            ("lib64", "/lib64"),
            ("usr", "/usr"),
            ("etc", "/etc"),
        ];

        for (name, target) in &symlinks {
            let link_path = rootfs_path.join(name);
            let target_path = Path::new(target);

            // Only create symlink if target exists on host
            if target_path.exists() && !link_path.exists() {
                std::os::unix::fs::symlink(target, &link_path).ok();
            }
        }

        // Create necessary empty directories
        for dir in &["tmp", "var", "run", "proc", "dev"] {
            let dir_path = rootfs_path.join(dir);
            std::fs::create_dir_all(&dir_path).ok();
        }

        Ok(())
    }

    /// Generate OCI config.json from spawn config
    pub fn generate_oci_config(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        workdir: Option<&PathBuf>,
        socket_path: &Path,
    ) -> serde_json::Value {
        // Build args array: command + args
        let mut process_args: Vec<String> = vec![command.to_string()];
        process_args.extend(args.iter().cloned());

        // Build env array: key=value format
        // Only add PATH if user didn't provide one
        let has_path = env.keys().any(|k| k.eq_ignore_ascii_case("PATH"));
        let process_env: Vec<String> = env
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .chain(if has_path {
                None
            } else {
                Some("PATH=/usr/local/bin:/usr/bin:/bin:/sbin".to_string())
            })
            .collect();

        // Get current working directory
        let cwd = workdir
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());

        // Get socket directory for bind mount
        let socket_dir = socket_path
            .parent()
            .unwrap_or(Path::new("/tmp"))
            .to_string_lossy()
            .to_string();

        json!({
            "ociVersion": "1.0.0",
            "root": {
                "path": "rootfs",
                "readonly": false
            },
            "process": {
                "terminal": false,
                "user": {
                    "uid": 0,
                    "gid": 0
                },
                "args": process_args,
                "env": process_env,
                "cwd": cwd
            },
            "hostname": "sandbox",
            "mounts": [
                {
                    "destination": "/proc",
                    "type": "proc",
                    "source": "proc"
                },
                {
                    "destination": "/dev",
                    "type": "tmpfs",
                    "source": "tmpfs",
                    "options": ["nosuid", "strictatime", "mode=755", "size=65536k"]
                },
                {
                    "destination": "/tmp",
                    "type": "tmpfs",
                    "source": "tmpfs",
                    "options": ["nosuid", "noexec", "nodev"]
                },
                // Bind mount for socket directory - allows process to create socket
                {
                    "destination": socket_dir,
                    "type": "bind",
                    "source": socket_dir,
                    "options": ["rbind", "rw"]
                }
            ],
            "linux": {
                "namespaces": [
                    { "type": "pid" },
                    { "type": "network" },
                    { "type": "ipc" },
                    { "type": "uts" },
                    { "type": "mount" }
                ],
                "resources": {
                    "devices": [
                        { "allow": false, "access": "rwm" }
                    ]
                }
            }
        })
    }

    /// Wait for socket to appear (with timeout)
    pub async fn wait_for_socket(socket_path: &Path, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if socket_path.exists() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        anyhow::bail!(
            "Socket {} not created after {:?}",
            socket_path.display(),
            timeout
        )
    }

    pub async fn spawn_sandboxed(
        config: &SpawnConfig,
        runsc_path: &Option<PathBuf>,
    ) -> Result<RuntimeHandle> {
        // Find runsc binary
        let runsc_bin = find_runsc(runsc_path)?;

        // Generate unique container ID
        let container_id = format!("tenement-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);

        // Create bundle directory
        let bundle_path = PathBuf::from(format!("/tmp/tenement-sandbox-{}", container_id));
        std::fs::create_dir_all(&bundle_path)
            .with_context(|| format!("Failed to create bundle directory: {}", bundle_path.display()))?;

        // Create rootfs with host symlinks
        let rootfs_path = bundle_path.join("rootfs");
        create_rootfs(&rootfs_path)
            .with_context(|| format!("Failed to create rootfs: {}", rootfs_path.display()))?;

        // Generate OCI config
        let oci_config = generate_oci_config(
            &config.command,
            &config.args,
            &config.env,
            config.workdir.as_ref(),
            &config.socket,
        );

        // Write config.json
        let config_path = bundle_path.join("config.json");
        std::fs::write(&config_path, oci_config.to_string())
            .with_context(|| format!("Failed to write config.json: {}", config_path.display()))?;

        // Create state directory
        let state_dir = PathBuf::from(format!("/var/run/tenement/sandbox/{}", container_id));
        if let Err(e) = std::fs::create_dir_all(&state_dir) {
            // Clean up bundle on failure
            std::fs::remove_dir_all(&bundle_path).ok();
            return Err(e).with_context(|| {
                format!(
                    "Failed to create state directory: {}\n\
                    Try: sudo mkdir -p /var/run/tenement/sandbox && sudo chmod 755 /var/run/tenement",
                    state_dir.display()
                )
            });
        }

        // Ensure socket parent directory exists
        if let Some(socket_dir) = config.socket.parent() {
            std::fs::create_dir_all(socket_dir).ok();
        }

        // Remove old socket if exists
        if config.socket.exists() {
            std::fs::remove_file(&config.socket).ok();
        }

        // Run: runsc run --root <state_dir> --bundle <bundle_path> --detach <container_id>
        let output = Command::new(&runsc_bin)
            .arg("run")
            .arg("--root")
            .arg(&state_dir)
            .arg("--bundle")
            .arg(&bundle_path)
            .arg("--detach")
            .arg(&container_id)
            .output()
            .await
            .with_context(|| format!("Failed to execute runsc: {}", runsc_bin.display()))?;

        if !output.status.success() {
            // Clean up on failure
            std::fs::remove_dir_all(&bundle_path).ok();
            std::fs::remove_dir_all(&state_dir).ok();

            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "runsc run failed:\n{}\n\nBundle path: {}\nConfig: {}",
                stderr,
                bundle_path.display(),
                config_path.display()
            );
        }

        // Wait for socket (if health checks are expected)
        // Give the process time to start and create its socket
        if let Err(e) = wait_for_socket(&config.socket, Duration::from_secs(10)).await {
            // Log warning but don't fail - process may not create socket immediately
            tracing::warn!("Socket wait timeout: {}", e);
        }

        Ok(RuntimeHandle::Sandbox {
            container_id,
            bundle_path,
            state_dir,
            socket: config.socket.clone(),
        })
    }
}

#[async_trait]
impl Runtime for SandboxRuntime {
    async fn spawn(&self, config: &SpawnConfig) -> Result<RuntimeHandle> {
        #[cfg(target_os = "linux")]
        {
            linux_impl::spawn_sandboxed(config, &self.runsc_path).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            anyhow::bail!(
                "Sandbox (gVisor) runtime requires Linux.\n\n\
                For local development on macOS/Windows:\n  \
                - Use isolation = \"process\" in tenement.toml\n  \
                - Deploy to Linux for sandbox testing\n\n\
                gVisor cannot run on non-Linux platforms."
            )
        }
    }

    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Sandbox
    }

    fn is_available(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            linux_impl::find_runsc(&self.runsc_path).is_ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    fn name(&self) -> &'static str {
        "sandbox"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_runtime_type() {
        let runtime = SandboxRuntime::new();
        assert_eq!(runtime.runtime_type(), RuntimeType::Sandbox);
    }

    #[test]
    fn test_sandbox_runtime_name() {
        let runtime = SandboxRuntime::new();
        assert_eq!(runtime.name(), "sandbox");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_generate_oci_config() {
        use std::collections::HashMap;

        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());

        let config = linux_impl::generate_oci_config(
            "/bin/echo",
            &["hello".to_string()],
            &env,
            None,
            &PathBuf::from("/tmp/test.sock"),
        );

        assert_eq!(config["ociVersion"], "1.0.0");

        let args = config["process"]["args"].as_array().unwrap();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "/bin/echo");
        assert_eq!(args[1], "hello");

        let env_arr = config["process"]["env"].as_array().unwrap();
        assert!(env_arr.iter().any(|e| e == "FOO=bar"));
    }

    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn test_sandbox_runtime_spawn_fails_on_non_linux() {
        use std::collections::HashMap;

        let runtime = SandboxRuntime::new();
        let config = SpawnConfig {
            command: "sleep".to_string(),
            args: vec!["0.1".to_string()],
            env: HashMap::new(),
            socket: PathBuf::from("/tmp/test-sandbox-runtime.sock"),
            workdir: None,
            vm_config: None,
        };

        let result = runtime.spawn(&config).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Linux"));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_sandbox_runtime_not_available_on_non_linux() {
        let runtime = SandboxRuntime::new();
        assert!(!runtime.is_available());
    }

    // Integration tests - require Linux and runsc installed
    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "Requires runsc installed"]
    async fn test_sandbox_spawn_and_kill() {
        use std::collections::HashMap;

        let runtime = SandboxRuntime::new();
        if !runtime.is_available() {
            eprintln!("Skipping: runsc not available");
            return;
        }

        let socket = PathBuf::from("/tmp/test-sandbox-spawn.sock");
        let config = SpawnConfig {
            command: "sleep".to_string(),
            args: vec!["30".to_string()],
            env: HashMap::new(),
            socket: socket.clone(),
            workdir: None,
            vm_config: None,
        };

        let mut handle = runtime.spawn(&config).await.unwrap();
        assert_eq!(handle.runtime_type(), RuntimeType::Sandbox);

        // Give it time to start
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Check it's running
        assert!(handle.is_running().await);

        // Kill it
        handle.kill().await.unwrap();

        // Give it time to stop
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Check it's stopped
        assert!(!handle.is_running().await);
    }
}
