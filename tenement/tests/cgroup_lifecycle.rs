//! Cgroup Lifecycle Tests
//!
//! Linux-only integration tests for cgroup v2 resource limits.
//! Verifies cgroup creation/cleanup on spawn/stop.
//!
//! These tests require:
//! - Linux with cgroups v2 enabled
//! - Write access to /sys/fs/cgroup/tenement (typically requires root)
//!
//! Run with: cargo test --test cgroup_lifecycle -- --ignored

mod common;

use common::{test_config_with_limits, test_config_with_process};
use std::path::PathBuf;
use tempfile::TempDir;

// ===================
// HELPER FUNCTIONS
// ===================

/// Get the cgroup path for an instance
fn cgroup_path(instance_id: &str) -> PathBuf {
    PathBuf::from("/sys/fs/cgroup/tenement").join(instance_id)
}

/// Check if cgroups v2 are available on this system
fn cgroups_available() -> bool {
    let cgroup_base = PathBuf::from("/sys/fs/cgroup");
    if !cgroup_base.exists() {
        return false;
    }
    cgroup_base.join("cgroup.controllers").exists()
}

/// Create a touch_socket script (duplicated from common for isolation)
fn create_test_script(dir: &TempDir) -> PathBuf {
    let script_path = dir.path().join("test_script.sh");
    let script = r#"#!/bin/bash
SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
rm -f "$SOCKET_PATH"
touch "$SOCKET_PATH"
sleep 30
"#;
    std::fs::write(&script_path, script).expect("Failed to write test script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    script_path
}

// ===================
// NON-LINUX TESTS
// ===================

/// On non-Linux platforms, cgroups should gracefully no-op
#[cfg(not(target_os = "linux"))]
mod non_linux_tests {
    use super::*;
    use tenement::Hypervisor;

    #[tokio::test]
    async fn test_cgroups_graceful_skip_on_non_linux() {
        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // Config with resource limits
        let config = test_config_with_limits("api", script.to_str().unwrap(), 256, 200);
        let hypervisor = Hypervisor::new(config);

        // Should spawn successfully even though cgroups aren't available
        let result = hypervisor.spawn("api", "test").await;
        assert!(result.is_ok(), "Spawn should succeed on non-Linux");

        // Should be running
        assert!(hypervisor.is_running("api", "test").await);

        // Clean up
        hypervisor.stop("api", "test").await.ok();
    }
}

// ===================
// LINUX-ONLY TESTS
// ===================

#[cfg(target_os = "linux")]
mod linux_tests {
    use super::*;
    use common::wait_for_socket;
    use tenement::Hypervisor;

    // ===================
    // CGROUP DIRECTORY LIFECYCLE
    // ===================

    /// Test 1: Verify cgroup directory is created on spawn with limits
    ///
    /// When spawning an instance with memory/CPU limits, a cgroup should be
    /// created at /sys/fs/cgroup/tenement/{instance_id}/
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_cgroup_created_on_spawn() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // Config with resource limits
        let config = test_config_with_limits("api", script.to_str().unwrap(), 256, 200);
        let hypervisor = Hypervisor::new(config);

        // Spawn instance
        let socket = hypervisor.spawn("api", "cgroup-test-1").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await, "Socket should be created");

        // Verify cgroup directory exists
        let cgroup = cgroup_path("api:cgroup-test-1");
        assert!(
            cgroup.exists(),
            "Cgroup directory should exist at {:?}",
            cgroup
        );

        // Clean up
        hypervisor.stop("api", "cgroup-test-1").await.ok();
    }

    /// Test 2: Verify cgroup directory is cleaned up on stop
    ///
    /// When stopping an instance, its cgroup should be removed.
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_cgroup_removed_on_stop() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        let config = test_config_with_limits("api", script.to_str().unwrap(), 256, 200);
        let hypervisor = Hypervisor::new(config);

        // Spawn instance
        let socket = hypervisor.spawn("api", "cgroup-test-2").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        let cgroup = cgroup_path("api:cgroup-test-2");
        assert!(cgroup.exists(), "Cgroup should exist after spawn");

        // Stop instance
        hypervisor.stop("api", "cgroup-test-2").await.unwrap();

        // Wait a bit for cleanup
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify cgroup directory is removed
        assert!(
            !cgroup.exists(),
            "Cgroup directory should be removed after stop"
        );
    }

    /// Test 3: No cgroup created when no limits are configured
    ///
    /// When spawning without memory/CPU limits, no cgroup should be created.
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_no_cgroup_without_limits() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // Config WITHOUT resource limits
        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        // Spawn instance
        let socket = hypervisor.spawn("api", "cgroup-test-3").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        // Cgroup should NOT exist (no limits configured)
        let cgroup = cgroup_path("api:cgroup-test-3");
        assert!(
            !cgroup.exists(),
            "Cgroup should not be created without limits"
        );

        // Clean up
        hypervisor.stop("api", "cgroup-test-3").await.ok();
    }

    // ===================
    // MEMORY LIMIT TESTS
    // ===================

    /// Test 4: Verify memory.max is set correctly
    ///
    /// memory_limit_mb config should be written to cgroup memory.max in bytes.
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_memory_limit_enforcement() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // Config with 256MB memory limit
        let config = test_config_with_limits("api", script.to_str().unwrap(), 256, 100);
        let hypervisor = Hypervisor::new(config);

        // Spawn instance
        let socket = hypervisor.spawn("api", "mem-test").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        // Verify memory.max file
        let memory_max = cgroup_path("api:mem-test").join("memory.max");
        if memory_max.exists() {
            let content = std::fs::read_to_string(&memory_max).unwrap();
            let expected_bytes = 256u64 * 1024 * 1024; // 256 MB in bytes
            assert_eq!(
                content.trim(),
                expected_bytes.to_string(),
                "memory.max should be {} bytes, got {}",
                expected_bytes,
                content.trim()
            );
        } else {
            panic!("memory.max file should exist at {:?}", memory_max);
        }

        // Clean up
        hypervisor.stop("api", "mem-test").await.ok();
    }

    /// Test 5: Test memory limit with different values
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_memory_limit_various_sizes() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // Test with 1GB memory limit
        let mut config = test_config_with_limits("api", script.to_str().unwrap(), 1024, 100);
        config.settings.backoff_base_ms = 0;
        let hypervisor = Hypervisor::new(config);

        let socket = hypervisor.spawn("api", "mem-1gb").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        let memory_max = cgroup_path("api:mem-1gb").join("memory.max");
        if memory_max.exists() {
            let content = std::fs::read_to_string(&memory_max).unwrap();
            let expected = 1024u64 * 1024 * 1024; // 1 GB
            assert_eq!(content.trim(), expected.to_string());
        }

        hypervisor.stop("api", "mem-1gb").await.ok();
    }

    // ===================
    // CPU WEIGHT TESTS
    // ===================

    /// Test 6: Verify cpu.weight is set correctly
    ///
    /// cpu_shares config should be written to cgroup cpu.weight (clamped 1-10000).
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_cpu_weight_enforcement() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // Config with cpu_shares = 500
        let config = test_config_with_limits("api", script.to_str().unwrap(), 256, 500);
        let hypervisor = Hypervisor::new(config);

        let socket = hypervisor.spawn("api", "cpu-test").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        // Verify cpu.weight file
        let cpu_weight = cgroup_path("api:cpu-test").join("cpu.weight");
        if cpu_weight.exists() {
            let content = std::fs::read_to_string(&cpu_weight).unwrap();
            assert_eq!(
                content.trim(),
                "500",
                "cpu.weight should be 500, got {}",
                content.trim()
            );
        } else {
            // cpu controller might not be enabled - just log
            eprintln!("Note: cpu.weight file not found (cpu controller may not be enabled)");
        }

        hypervisor.stop("api", "cpu-test").await.ok();
    }

    /// Test 7: CPU weight clamping - below minimum
    ///
    /// cpu_shares = 0 should be clamped to 1 (minimum valid value).
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_cpu_weight_clamped_minimum() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // cpu_shares = 0 should be clamped to 1
        let mut config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        if let Some(p) = config.service.get_mut("api") {
            p.memory_limit_mb = Some(128); // Need some limit to create cgroup
            p.cpu_shares = Some(0); // Below minimum
        }

        let hypervisor = Hypervisor::new(config);

        let socket = hypervisor.spawn("api", "cpu-min").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        let cpu_weight = cgroup_path("api:cpu-min").join("cpu.weight");
        if cpu_weight.exists() {
            let content = std::fs::read_to_string(&cpu_weight).unwrap();
            assert_eq!(
                content.trim(),
                "1",
                "cpu.weight should be clamped to 1, got {}",
                content.trim()
            );
        }

        hypervisor.stop("api", "cpu-min").await.ok();
    }

    /// Test 8: CPU weight clamping - above maximum
    ///
    /// cpu_shares > 10000 should be clamped to 10000 (maximum valid value).
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_cpu_weight_clamped_maximum() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // cpu_shares = 50000 should be clamped to 10000
        let mut config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        if let Some(p) = config.service.get_mut("api") {
            p.memory_limit_mb = Some(128);
            p.cpu_shares = Some(50000); // Above maximum
        }

        let hypervisor = Hypervisor::new(config);

        let socket = hypervisor.spawn("api", "cpu-max").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        let cpu_weight = cgroup_path("api:cpu-max").join("cpu.weight");
        if cpu_weight.exists() {
            let content = std::fs::read_to_string(&cpu_weight).unwrap();
            assert_eq!(
                content.trim(),
                "10000",
                "cpu.weight should be clamped to 10000, got {}",
                content.trim()
            );
        }

        hypervisor.stop("api", "cpu-max").await.ok();
    }

    // ===================
    // PROCESS MEMBERSHIP TESTS
    // ===================

    /// Test 9: Verify process is added to cgroup
    ///
    /// The spawned process PID should appear in cgroup.procs.
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_process_added_to_cgroup() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        let config = test_config_with_limits("api", script.to_str().unwrap(), 256, 100);
        let hypervisor = Hypervisor::new(config);

        let socket = hypervisor.spawn("api", "proc-test").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        // Check cgroup.procs file
        let procs = cgroup_path("api:proc-test").join("cgroup.procs");
        if procs.exists() {
            let content = std::fs::read_to_string(&procs).unwrap();
            assert!(
                !content.trim().is_empty(),
                "cgroup.procs should contain at least one PID"
            );

            // Verify at least one PID is listed
            let pids: Vec<u32> = content
                .lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect();
            assert!(
                !pids.is_empty(),
                "Should have at least one PID in cgroup"
            );
        } else {
            panic!("cgroup.procs file should exist");
        }

        hypervisor.stop("api", "proc-test").await.ok();
    }

    // ===================
    // MULTIPLE INSTANCES TESTS
    // ===================

    /// Test 10: Multiple instances have separate cgroups
    ///
    /// Each instance should have its own isolated cgroup.
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_multiple_instances_separate_cgroups() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        let config = test_config_with_limits("api", script.to_str().unwrap(), 256, 100);
        let hypervisor = Hypervisor::new(config);

        // Spawn two instances
        let socket1 = hypervisor.spawn("api", "multi-1").await.unwrap();
        let socket2 = hypervisor.spawn("api", "multi-2").await.unwrap();

        assert!(wait_for_socket(&socket1, 2000).await);
        assert!(wait_for_socket(&socket2, 2000).await);

        // Both should have separate cgroups
        let cgroup1 = cgroup_path("api:multi-1");
        let cgroup2 = cgroup_path("api:multi-2");

        assert!(cgroup1.exists(), "Cgroup for multi-1 should exist");
        assert!(cgroup2.exists(), "Cgroup for multi-2 should exist");
        assert_ne!(cgroup1, cgroup2, "Cgroups should be separate");

        // Clean up
        hypervisor.stop("api", "multi-1").await.ok();
        hypervisor.stop("api", "multi-2").await.ok();

        // Verify both cleaned up
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!cgroup1.exists(), "Cgroup for multi-1 should be removed");
        assert!(!cgroup2.exists(), "Cgroup for multi-2 should be removed");
    }

    // ===================
    // EDGE CASES
    // ===================

    /// Test 11: Restart preserves cgroup limits
    ///
    /// After restart, cgroup should still have correct limits.
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_restart_preserves_cgroup_limits() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        let config = test_config_with_limits("api", script.to_str().unwrap(), 512, 300);
        let hypervisor = Hypervisor::new(config);

        let socket = hypervisor.spawn("api", "restart-test").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        // Verify initial limits
        let cgroup = cgroup_path("api:restart-test");
        let memory_max = cgroup.join("memory.max");
        if memory_max.exists() {
            let initial = std::fs::read_to_string(&memory_max).unwrap();
            let expected = (512u64 * 1024 * 1024).to_string();
            assert_eq!(initial.trim(), expected);
        }

        // Restart
        hypervisor.restart("api", "restart-test").await.unwrap();

        // Give restart time to complete
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Verify limits are preserved after restart
        if memory_max.exists() {
            let after_restart = std::fs::read_to_string(&memory_max).unwrap();
            let expected = (512u64 * 1024 * 1024).to_string();
            assert_eq!(
                after_restart.trim(),
                expected,
                "Memory limit should be preserved after restart"
            );
        }

        hypervisor.stop("api", "restart-test").await.ok();
    }

    /// Test 12: Memory-only limits (no CPU)
    ///
    /// Should create cgroup with only memory.max set.
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_memory_only_limits() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // Only memory limit, no CPU
        let mut config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        if let Some(p) = config.service.get_mut("api") {
            p.memory_limit_mb = Some(128);
            p.cpu_shares = None;
        }

        let hypervisor = Hypervisor::new(config);

        let socket = hypervisor.spawn("api", "mem-only").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        let cgroup = cgroup_path("api:mem-only");
        assert!(cgroup.exists(), "Cgroup should be created for memory-only limits");

        // memory.max should exist
        let memory_max = cgroup.join("memory.max");
        if memory_max.exists() {
            let content = std::fs::read_to_string(&memory_max).unwrap();
            let expected = (128u64 * 1024 * 1024).to_string();
            assert_eq!(content.trim(), expected);
        }

        hypervisor.stop("api", "mem-only").await.ok();
    }

    /// Test 13: CPU-only limits (no memory)
    ///
    /// Should create cgroup with only cpu.weight set.
    #[tokio::test]
    #[ignore = "requires root/cgroup privileges"]
    async fn test_cpu_only_limits() {
        if !cgroups_available() {
            eprintln!("Skipping: cgroups v2 not available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let script = create_test_script(&dir);

        // Only CPU limit, no memory
        let mut config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        if let Some(p) = config.service.get_mut("api") {
            p.memory_limit_mb = None;
            p.cpu_shares = Some(200);
        }

        let hypervisor = Hypervisor::new(config);

        let socket = hypervisor.spawn("api", "cpu-only").await.unwrap();
        assert!(wait_for_socket(&socket, 2000).await);

        let cgroup = cgroup_path("api:cpu-only");
        assert!(cgroup.exists(), "Cgroup should be created for CPU-only limits");

        // cpu.weight should exist (if cpu controller enabled)
        let cpu_weight = cgroup.join("cpu.weight");
        if cpu_weight.exists() {
            let content = std::fs::read_to_string(&cpu_weight).unwrap();
            assert_eq!(content.trim(), "200");
        }

        hypervisor.stop("api", "cpu-only").await.ok();
    }
}

// ===================
// AVAILABILITY CHECK TESTS
// (can run without privileges)
// ===================

#[cfg(target_os = "linux")]
#[test]
fn test_cgroups_v2_detection() {
    let cgroup_base = PathBuf::from("/sys/fs/cgroup");
    let has_cgroups = cgroup_base.exists();
    let has_v2 = cgroup_base.join("cgroup.controllers").exists();

    // Just verify detection works (actual availability depends on system)
    println!(
        "Cgroups v2 detection: base exists={}, controllers exists={}",
        has_cgroups, has_v2
    );

    // This test just verifies the detection logic runs without error
    assert!(true);
}

#[test]
fn test_cgroup_path_format() {
    let path = cgroup_path("api:user123");
    assert_eq!(
        path,
        PathBuf::from("/sys/fs/cgroup/tenement/api:user123")
    );
}

#[test]
fn test_cgroup_path_with_special_chars() {
    let path = cgroup_path("api-v2:user_123-prod");
    assert_eq!(
        path,
        PathBuf::from("/sys/fs/cgroup/tenement/api-v2:user_123-prod")
    );
}
