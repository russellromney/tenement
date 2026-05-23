//! LiteBox runtime - runs an app under a configurable external LiteBox runner.
//!
//! LiteBox is a Rust library-OS sandbox. Tenement does **not** embed it; this
//! runtime supervises an external *runner* binary that loads the app's rootfs
//! and executes the command inside the LiteBox sandbox. This keeps Tenement
//! dependency-free (no LiteBox/Cinch crates) and lets downstreams swap in a
//! patched runner (e.g. `soup-litebox`/`tinyhost-litebox` with a CinchFS
//! backend) without changing Tenement.
//!
//! ## Runner contract (Tenement -> runner)
//!
//! ```text
//! <runner> run \
//!   --rootfs  <ABS path to extracted rootfs directory> \
//!   --workdir <path inside the rootfs, e.g. /app> \
//!   --env     KEY=VALUE   (repeated once per env var; includes PORT) \
//!   -- <command> [args...]
//! ```
//!
//! The runner owns sandbox setup: enter the rootfs as `/`, set the guest env,
//! `chdir` to `workdir` inside the new root, and `exec` the command. The app is
//! expected to listen on the TCP `PORT` Tenement allocated (passed via `--env`),
//! same as the process/namespace runtimes. Tenement supervises the runner as an
//! ordinary child: it lives in its own process group so the whole tree can be
//! killed, and exit of the runner means exit of the instance.
//!
//! ## Runner discovery (first match wins)
//!
//! 1. explicit path passed to [`LiteBoxRuntime::with_runner`];
//! 2. `TENEMENT_LITEBOX_RUNNER` environment variable;
//! 3. `litebox` in common locations / `PATH`.
//!
//! Local-filesystem rootfs is the only mode here; any object-store/CinchFS
//! behavior lives entirely in a downstream runner, never in Tenement.

use super::{Runtime, RuntimeHandle, RuntimeType, SpawnConfig};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;

/// Environment variable that overrides the LiteBox runner binary path.
pub const RUNNER_ENV: &str = "TENEMENT_LITEBOX_RUNNER";

/// Runtime that runs apps under an external LiteBox runner binary.
pub struct LiteBoxRuntime {
    /// Explicit runner path. When `None`, discovered from env/PATH at use time.
    runner: Option<PathBuf>,
}

impl LiteBoxRuntime {
    pub fn new() -> Self {
        Self { runner: None }
    }

    /// Construct with an explicit runner path (highest precedence).
    pub fn with_runner(path: PathBuf) -> Self {
        Self { runner: Some(path) }
    }

    /// Resolve the runner binary: explicit path -> env var -> PATH/common dirs.
    fn find_runner(&self) -> Option<PathBuf> {
        if let Some(path) = &self.runner {
            return path.exists().then(|| path.clone());
        }
        if let Ok(env_path) = std::env::var(RUNNER_ENV) {
            let p = PathBuf::from(env_path);
            if p.exists() {
                return Some(p);
            }
            // An explicit env override that doesn't exist is a config error, not
            // a reason to silently fall back to PATH. Surface it via is_available.
            return None;
        }
        for path in &["/usr/local/bin/litebox", "/usr/bin/litebox"] {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
        if let Ok(path_env) = std::env::var("PATH") {
            for dir in path_env.split(':') {
                let p = PathBuf::from(dir).join("litebox");
                if p.exists() {
                    return Some(p);
                }
            }
        }
        None
    }
}

impl Default for LiteBoxRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Runtime for LiteBoxRuntime {
    async fn spawn(&self, config: &SpawnConfig) -> Result<RuntimeHandle> {
        use std::process::Stdio;
        use tokio::process::Command;

        let runner = self.find_runner().with_context(|| {
            format!(
                "LiteBox runner not found. Set {RUNNER_ENV}=/path/to/runner, \
                 place a `litebox` binary on PATH, or pass an explicit path. \
                 Tenement does not embed LiteBox; it supervises an external runner."
            )
        })?;

        // LiteBox is a sandbox: it needs a rootfs to use as `/`. Fail closed
        // rather than run the app against the host filesystem.
        let rootfs = config.rootfs.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "LiteBox isolation requires `rootfs` (the extracted app root); none was provided"
            )
        })?;
        if !rootfs.is_dir() {
            bail!(
                "LiteBox rootfs {:?} does not exist or is not a directory",
                rootfs
            );
        }

        // Workdir is a guest path inside the rootfs; default to "/".
        let workdir = config.workdir.clone().unwrap_or_else(|| PathBuf::from("/"));

        // Remove a stale socket if one is lingering.
        if config.socket.exists() {
            std::fs::remove_file(&config.socket).ok();
        }

        let mut cmd = Command::new(&runner);
        cmd.arg("run")
            .arg("--rootfs")
            .arg(rootfs)
            .arg("--workdir")
            .arg(&workdir);
        // Guest env is passed explicitly (incl. PORT). We do NOT leak it into the
        // runner's own environment — the runner injects it into the sandbox.
        for (k, v) in &config.env {
            cmd.arg("--env").arg(format!("{k}={v}"));
        }
        cmd.arg("--").arg(&config.command).args(&config.args);

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Own process group so killing the runner kills the whole tree.
        #[cfg(unix)]
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn LiteBox runner {:?} for command: {}",
                runner, config.command
            )
        })?;

        Ok(RuntimeHandle::Litebox {
            child,
            socket: config.socket.clone(),
        })
    }

    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Litebox
    }

    fn is_available(&self) -> bool {
        self.find_runner().is_some()
    }

    fn name(&self) -> &'static str {
        "litebox"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn spawn_config(rootfs: Option<PathBuf>) -> SpawnConfig {
        let mut env = HashMap::new();
        env.insert("PORT".to_string(), "31000".to_string());
        SpawnConfig {
            command: "/app/server".to_string(),
            args: vec!["--flag".to_string()],
            env,
            socket: std::env::temp_dir().join("tenement-litebox-test.sock"),
            workdir: Some(PathBuf::from("/app")),
            rootfs,
            vm_config: None,
            mounts: Vec::new(),
            image: None,
            memory_limit_mb: None,
            cpu_shares: None,
        }
    }

    #[test]
    fn test_litebox_runtime_type_and_name() {
        let rt = LiteBoxRuntime::new();
        assert_eq!(rt.runtime_type(), RuntimeType::Litebox);
        assert_eq!(rt.name(), "litebox");
    }

    #[test]
    fn test_explicit_missing_runner_not_available() {
        let rt = LiteBoxRuntime::with_runner(PathBuf::from("/nonexistent/litebox-xyz"));
        assert!(!rt.is_available());
    }

    #[tokio::test]
    async fn test_spawn_fails_without_runner() {
        // Point at a runner that does not exist; spawn must fail closed.
        let rt = LiteBoxRuntime::with_runner(PathBuf::from("/nonexistent/litebox-xyz"));
        let dir = tempfile::tempdir().unwrap();
        let err = rt
            .spawn(&spawn_config(Some(dir.path().to_path_buf())))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("runner not found"), "got: {err}");
    }

    #[tokio::test]
    async fn test_spawn_requires_rootfs() {
        // Use a real runner (a shell script) so we get past discovery and hit
        // the rootfs check.
        let dir = tempfile::tempdir().unwrap();
        let runner = dir.path().join("fake-litebox");
        std::fs::write(&runner, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let rt = LiteBoxRuntime::with_runner(runner);
        let err = rt.spawn(&spawn_config(None)).await.unwrap_err().to_string();
        assert!(err.contains("requires `rootfs`"), "got: {err}");
    }

    // Proves the runner contract: a fake runner records the argv it received and
    // we assert Tenement passed run/--rootfs/--workdir/--env/-- as documented.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_runner_receives_documented_contract() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let rootfs = dir.path().join("rootfs");
        std::fs::create_dir_all(rootfs.join("app")).unwrap();
        let argv_dump = dir.path().join("argv.txt");

        // Runner writes its own argv (one per line) and exits.
        let runner = dir.path().join("fake-litebox");
        std::fs::write(
            &runner,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > {}\n",
                argv_dump.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755)).unwrap();

        let rt = LiteBoxRuntime::with_runner(runner);
        let cfg = spawn_config(Some(rootfs.clone()));
        let mut handle = rt.spawn(&cfg).await.unwrap();
        assert_eq!(handle.runtime_type(), RuntimeType::Litebox);

        // Poll for the runner to write its argv (robust under load) rather than
        let mut argv = String::new();
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(&argv_dump) {
                if !s.trim().is_empty() {
                    argv = s;
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        handle.kill(std::time::Duration::from_secs(1)).await.ok();
        let lines: Vec<&str> = argv.lines().collect();
        assert_eq!(lines.first(), Some(&"run"), "argv: {argv:?}");
        assert!(lines.contains(&"--rootfs"), "argv: {argv:?}");
        assert!(
            lines.contains(&rootfs.to_string_lossy().as_ref()),
            "argv: {argv:?}"
        );
        assert!(lines.contains(&"--workdir"), "argv: {argv:?}");
        assert!(lines.contains(&"/app"), "argv: {argv:?}");
        assert!(lines.contains(&"--env"), "argv: {argv:?}");
        assert!(lines.contains(&"PORT=31000"), "argv: {argv:?}");
        assert!(lines.contains(&"--"), "argv: {argv:?}");
        assert!(lines.contains(&"/app/server"), "argv: {argv:?}");
        assert!(lines.contains(&"--flag"), "argv: {argv:?}");
    }
}
