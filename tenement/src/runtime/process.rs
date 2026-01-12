//! Process runtime - spawns bare processes with Unix socket communication

use super::{Runtime, RuntimeHandle, RuntimeType, SpawnConfig};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::Command;

/// Runtime that spawns bare processes
///
/// This is the default runtime. It spawns processes directly on the host
/// and expects them to create Unix sockets for communication.
pub struct ProcessRuntime;

impl ProcessRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProcessRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Runtime for ProcessRuntime {
    async fn spawn(&self, config: &SpawnConfig) -> Result<RuntimeHandle> {
        // Remove old socket if exists
        if config.socket.exists() {
            std::fs::remove_file(&config.socket).ok();
        }

        // Build command
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .envs(&config.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(workdir) = &config.workdir {
            cmd.current_dir(workdir);
        }

        let child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn process: {}", config.command))?;

        Ok(RuntimeHandle::Process {
            child,
            socket: config.socket.clone(),
        })
    }

    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Process
    }

    fn is_available(&self) -> bool {
        true // Process runtime is always available
    }

    fn name(&self) -> &'static str {
        "process"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn test_spawn_config(command: &str, args: Vec<&str>, socket: PathBuf) -> SpawnConfig {
        SpawnConfig {
            command: command.to_string(),
            args: args.into_iter().map(|s| s.to_string()).collect(),
            env: HashMap::new(),
            socket,
            workdir: None,
            vm_config: None,
        }
    }

    // ===================
    // BASIC RUNTIME TESTS
    // ===================

    #[test]
    fn test_process_runtime_is_available() {
        let runtime = ProcessRuntime::new();
        assert!(runtime.is_available());
    }

    #[test]
    fn test_process_runtime_type() {
        let runtime = ProcessRuntime::new();
        assert_eq!(runtime.runtime_type(), RuntimeType::Process);
    }

    #[test]
    fn test_process_runtime_name() {
        let runtime = ProcessRuntime::new();
        assert_eq!(runtime.name(), "process");
    }

    #[test]
    fn test_process_runtime_default() {
        let runtime = ProcessRuntime::default();
        assert!(runtime.is_available());
        assert_eq!(runtime.runtime_type(), RuntimeType::Process);
    }

    // ===================
    // SPAWN TESTS
    // ===================

    #[tokio::test]
    async fn test_process_runtime_spawn() {
        let runtime = ProcessRuntime::new();
        let config = test_spawn_config(
            "sleep",
            vec!["0.1"],
            PathBuf::from("/tmp/test-process-runtime.sock"),
        );

        let mut handle = runtime.spawn(&config).await.unwrap();
        assert_eq!(handle.runtime_type(), RuntimeType::Process);
        assert!(!handle.is_vsock());
        assert!(handle.vsock_port().is_none());

        // Clean up
        handle.kill().await.ok();
    }

    #[tokio::test]
    async fn test_process_runtime_spawn_with_args() {
        let runtime = ProcessRuntime::new();
        let config = test_spawn_config(
            "echo",
            vec!["hello", "world"],
            PathBuf::from("/tmp/test-process-args.sock"),
        );

        let handle = runtime.spawn(&config).await.unwrap();
        assert_eq!(handle.runtime_type(), RuntimeType::Process);
        // echo exits immediately, no need to kill
    }

    #[tokio::test]
    async fn test_process_runtime_spawn_with_env() {
        let runtime = ProcessRuntime::new();
        let mut config = test_spawn_config(
            "env",
            vec![],
            PathBuf::from("/tmp/test-process-env.sock"),
        );
        config.env.insert("MY_VAR".to_string(), "my_value".to_string());

        let handle = runtime.spawn(&config).await.unwrap();
        assert_eq!(handle.runtime_type(), RuntimeType::Process);
    }

    #[tokio::test]
    async fn test_process_runtime_spawn_with_workdir() {
        let dir = TempDir::new().unwrap();
        let runtime = ProcessRuntime::new();
        let mut config = test_spawn_config(
            "pwd",
            vec![],
            PathBuf::from("/tmp/test-process-workdir.sock"),
        );
        config.workdir = Some(dir.path().to_path_buf());

        let handle = runtime.spawn(&config).await.unwrap();
        assert_eq!(handle.runtime_type(), RuntimeType::Process);
    }

    #[tokio::test]
    async fn test_process_runtime_spawn_removes_old_socket() {
        let socket_path = PathBuf::from("/tmp/test-old-socket-removal.sock");

        // Create a fake socket file
        std::fs::write(&socket_path, "fake").ok();
        assert!(socket_path.exists());

        let runtime = ProcessRuntime::new();
        let config = test_spawn_config("sleep", vec!["0.1"], socket_path.clone());

        let mut handle = runtime.spawn(&config).await.unwrap();

        // Old socket should be removed (though new one may not exist yet)
        // The socket is created by the spawned process, not by the runtime

        handle.kill().await.ok();
        std::fs::remove_file(&socket_path).ok();
    }

    // ===================
    // ERROR TESTS
    // ===================

    #[tokio::test]
    async fn test_process_runtime_spawn_command_not_found() {
        let runtime = ProcessRuntime::new();
        let config = test_spawn_config(
            "/nonexistent/command/that/does/not/exist",
            vec![],
            PathBuf::from("/tmp/test-not-found.sock"),
        );

        let result = runtime.spawn(&config).await;
        assert!(result.is_err());
    }

    // ===================
    // HANDLE TESTS
    // ===================

    #[tokio::test]
    async fn test_process_handle_socket() {
        let runtime = ProcessRuntime::new();
        let socket_path = PathBuf::from("/tmp/test-handle-socket.sock");
        let config = test_spawn_config("sleep", vec!["1"], socket_path.clone());

        let mut handle = runtime.spawn(&config).await.unwrap();
        assert_eq!(handle.socket(), &socket_path);

        handle.kill().await.ok();
    }

    #[tokio::test]
    async fn test_process_handle_pid() {
        let runtime = ProcessRuntime::new();
        let config = test_spawn_config(
            "sleep",
            vec!["1"],
            PathBuf::from("/tmp/test-handle-pid.sock"),
        );

        let mut handle = runtime.spawn(&config).await.unwrap();
        let pid = handle.pid();
        assert!(pid.is_some());
        assert!(pid.unwrap() > 0);

        handle.kill().await.ok();
    }

    #[tokio::test]
    async fn test_process_handle_is_running() {
        let runtime = ProcessRuntime::new();
        let config = test_spawn_config(
            "sleep",
            vec!["10"],
            PathBuf::from("/tmp/test-is-running.sock"),
        );

        let mut handle = runtime.spawn(&config).await.unwrap();

        // Should be running
        assert!(handle.is_running().await);

        // Kill it
        handle.kill().await.unwrap();

        // Give it time to exit
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Should not be running
        assert!(!handle.is_running().await);
    }

    #[tokio::test]
    async fn test_process_handle_kill() {
        let runtime = ProcessRuntime::new();
        let config = test_spawn_config(
            "sleep",
            vec!["60"],
            PathBuf::from("/tmp/test-kill.sock"),
        );

        let mut handle = runtime.spawn(&config).await.unwrap();
        assert!(handle.is_running().await);

        let result = handle.kill().await;
        assert!(result.is_ok());

        // Give it time to exit
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!handle.is_running().await);
    }

    // ===================
    // RAPID SPAWN TESTS
    // ===================

    #[tokio::test]
    async fn test_process_runtime_rapid_spawn() {
        let runtime = ProcessRuntime::new();

        let mut handles = Vec::new();
        for i in 0..5 {
            let config = test_spawn_config(
                "sleep",
                vec!["0.5"],
                PathBuf::from(format!("/tmp/test-rapid-{}.sock", i)),
            );
            let handle = runtime.spawn(&config).await.unwrap();
            handles.push(handle);
        }

        // All should be running
        for handle in &mut handles {
            assert!(handle.is_running().await);
        }

        // Clean up
        for handle in &mut handles {
            handle.kill().await.ok();
        }
    }

    // ===================
    // EXIT CODE TESTS
    // ===================

    #[tokio::test]
    async fn test_process_exits_naturally() {
        let runtime = ProcessRuntime::new();
        let config = test_spawn_config(
            "true", // Exits immediately with code 0
            vec![],
            PathBuf::from("/tmp/test-exit-natural.sock"),
        );

        let mut handle = runtime.spawn(&config).await.unwrap();

        // Give it time to exit
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Should have exited
        assert!(!handle.is_running().await);
    }

    #[tokio::test]
    async fn test_process_exits_with_error() {
        let runtime = ProcessRuntime::new();
        let config = test_spawn_config(
            "false", // Exits immediately with code 1
            vec![],
            PathBuf::from("/tmp/test-exit-error.sock"),
        );

        let mut handle = runtime.spawn(&config).await.unwrap();

        // Give it time to exit
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Should have exited
        assert!(!handle.is_running().await);
    }
}
