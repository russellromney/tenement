//! Shared test utilities for integration and E2E tests

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tenement::runtime::RuntimeType;
use tenement::{Config, DbPool};

/// Re-export commonly used types for test convenience
pub use tenement::config::ProcessConfig;

/// Create a test config with a simple process
pub fn test_config_with_process(name: &str, command: &str, args: Vec<&str>) -> Config {
    let mut config = Config::default();
    config.settings.data_dir = std::env::temp_dir().join("tenement-test");
    config.settings.backoff_base_ms = 0; // No backoff delay in tests

    let process = ProcessConfig {
        command: command.to_string(),
        args: args.into_iter().map(|s| s.to_string()).collect(),
        socket: "/tmp/tenement-test/{name}-{id}.sock".to_string(),
        isolation: RuntimeType::Process,
        health: None,
        env: HashMap::new(),
        workdir: None,
        restart: "on-failure".to_string(),
        idle_timeout: None,
        startup_timeout: 5,
        memory_limit_mb: None,
        cpu_shares: None,
        kernel: None,
        rootfs: None,
        memory_mb: 256,
        vcpus: 1,
        vsock_port: 5000,
    };

    config.service.insert(name.to_string(), process);
    config
}

/// Create a test config with idle timeout
pub fn test_config_with_idle_timeout(name: &str, command: &str, idle_secs: u64) -> Config {
    let mut config = test_config_with_process(name, command, vec![]);
    if let Some(p) = config.service.get_mut(name) {
        p.idle_timeout = Some(idle_secs);
    }
    config
}

/// Create a test config with resource limits
pub fn test_config_with_limits(
    name: &str,
    command: &str,
    memory_mb: u32,
    cpu_shares: u32,
) -> Config {
    let mut config = test_config_with_process(name, command, vec![]);
    if let Some(p) = config.service.get_mut(name) {
        p.memory_limit_mb = Some(memory_mb);
        p.cpu_shares = Some(cpu_shares);
    }
    config
}

/// Wait for a socket file to exist
pub async fn wait_for_socket(path: &Path, timeout_ms: u64) -> bool {
    let iterations = timeout_ms / 10;
    for _ in 0..iterations {
        if path.exists() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    false
}

/// Wait for a socket file to be removed
pub async fn wait_for_socket_removed(path: &Path, timeout_ms: u64) -> bool {
    let iterations = timeout_ms / 10;
    for _ in 0..iterations {
        if !path.exists() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    false
}

/// Create test database and return pool with temp directory
/// The TempDir must be kept alive for the duration of the test
pub async fn create_test_db() -> (DbPool, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    let pool = tenement::init_db(&path).await.unwrap();
    (pool, dir)
}

/// Get the path to a fixture script
pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Create a temporary copy of a fixture script
/// Returns the path to the temp script which will be cleaned up with the TempDir
pub fn copy_fixture_to_temp(fixture_name: &str, temp_dir: &TempDir) -> PathBuf {
    let fixture = fixture_path(fixture_name);
    let dest = temp_dir.path().join(fixture_name);
    std::fs::copy(&fixture, &dest).expect("Failed to copy fixture");

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&dest).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dest, perms).unwrap();
    }

    dest
}

/// Helper to set SOCKET_PATH env var for fixture scripts
pub fn socket_env(socket_path: &Path) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert(
        "SOCKET_PATH".to_string(),
        socket_path.to_string_lossy().to_string(),
    );
    env
}
