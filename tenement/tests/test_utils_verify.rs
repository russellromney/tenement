//! Verification test for common test utilities
//! This test ensures the test infrastructure is working correctly.

mod common;

use common::{
    copy_fixture_to_temp, create_test_db, fixture_path, socket_env, test_config_with_idle_timeout,
    test_config_with_limits, test_config_with_process, wait_for_socket, wait_for_socket_removed,
};
use std::path::Path;
use tempfile::TempDir;

#[test]
fn test_fixture_path_exists() {
    let mock_server = fixture_path("mock_server.sh");
    assert!(mock_server.exists(), "mock_server.sh fixture should exist");

    let slow_startup = fixture_path("slow_startup.sh");
    assert!(slow_startup.exists(), "slow_startup.sh fixture should exist");

    let crash_on_health = fixture_path("crash_on_health.sh");
    assert!(
        crash_on_health.exists(),
        "crash_on_health.sh fixture should exist"
    );

    let exit_immediately = fixture_path("exit_immediately.sh");
    assert!(
        exit_immediately.exists(),
        "exit_immediately.sh fixture should exist"
    );
}

#[test]
fn test_copy_fixture_to_temp() {
    let temp_dir = TempDir::new().unwrap();
    let script_path = copy_fixture_to_temp("exit_immediately.sh", &temp_dir);

    assert!(script_path.exists(), "Copied fixture should exist");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&script_path).unwrap().permissions();
        assert!(perms.mode() & 0o111 != 0, "Fixture should be executable");
    }
}

#[test]
fn test_config_with_process_creates_valid_config() {
    let config = test_config_with_process("test-api", "/bin/echo", vec!["hello"]);

    assert!(config.service.contains_key("test-api"));
    let process = config.service.get("test-api").unwrap();
    assert_eq!(process.command, "/bin/echo");
    assert_eq!(process.args, vec!["hello"]);
    assert_eq!(config.settings.backoff_base_ms, 0);
}

#[test]
fn test_config_with_idle_timeout_sets_timeout() {
    let config = test_config_with_idle_timeout("test-api", "/bin/sleep", 30);

    let process = config.service.get("test-api").unwrap();
    assert_eq!(process.idle_timeout, Some(30));
}

#[test]
fn test_config_with_limits_sets_limits() {
    let config = test_config_with_limits("test-api", "/bin/sleep", 512, 200);

    let process = config.service.get("test-api").unwrap();
    assert_eq!(process.memory_limit_mb, Some(512));
    assert_eq!(process.cpu_shares, Some(200));
}

#[test]
fn test_socket_env_creates_env_map() {
    let env = socket_env(Path::new("/tmp/test.sock"));
    assert_eq!(env.get("SOCKET_PATH"), Some(&"/tmp/test.sock".to_string()));
}

#[tokio::test]
async fn test_create_test_db() {
    let (pool, _dir) = create_test_db().await;

    // Verify we can query the database
    let result = sqlx::query("SELECT 1 as val")
        .fetch_one(&pool)
        .await
        .unwrap();

    let val: i32 = sqlx::Row::get(&result, "val");
    assert_eq!(val, 1);
}

#[tokio::test]
async fn test_wait_for_socket_timeout() {
    let path = Path::new("/tmp/nonexistent_socket_12345.sock");

    // Should timeout quickly
    let result = wait_for_socket(path, 50).await;
    assert!(!result, "Should return false for non-existent socket");
}

#[tokio::test]
async fn test_wait_for_socket_removed_timeout() {
    // Create a temp file
    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("test.sock");
    std::fs::write(&socket_path, "").unwrap();

    // Should timeout because file exists
    let result = wait_for_socket_removed(&socket_path, 50).await;
    assert!(!result, "Should return false when socket still exists");

    // Remove and try again
    std::fs::remove_file(&socket_path).unwrap();
    let result = wait_for_socket_removed(&socket_path, 50).await;
    assert!(result, "Should return true when socket is gone");
}
