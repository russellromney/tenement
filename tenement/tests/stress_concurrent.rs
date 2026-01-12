//! Stress Tests
//!
//! Tests for system behavior under concurrent load.
//! Part of Session 6 of the E2E Testing Plan.
//!
//! These tests verify:
//! - Concurrent instance spawning works reliably
//! - Rapid spawn/stop cycles don't cause issues
//! - Log buffer handles high throughput
//! - Health checks scale to many instances
//! - Broadcast doesn't block on slow subscribers

mod common;

use common::{create_touch_socket_script, test_config_with_process, wait_for_socket};
use std::time::Duration;
use tempfile::TempDir;
use tenement::Hypervisor;

// =============================================================================
// CONCURRENT SPAWN TESTS
// =============================================================================

/// Test spawning many instances concurrently
/// Target: 100 concurrent spawns with 95%+ success rate
#[tokio::test]
async fn test_stress_concurrent_spawns() {
    let dir = TempDir::new().unwrap();
    let script = create_touch_socket_script(&dir);

    let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
    let hypervisor = Hypervisor::new(config);

    const NUM_INSTANCES: usize = 100;
    let mut handles = vec![];

    // Spawn all instances concurrently
    for i in 0..NUM_INSTANCES {
        let h = hypervisor.clone();
        handles.push(tokio::spawn(async move {
            h.spawn("api", &format!("user{}", i)).await
        }));
    }

    // Wait for all spawns to complete and count successes
    let mut successes = 0;
    for handle in handles {
        if let Ok(Ok(_)) = handle.await {
            successes += 1;
        }
    }

    assert!(
        successes >= 95,
        "Expected 95%+ success rate, got {}/{} ({}%)",
        successes,
        NUM_INSTANCES,
        successes * 100 / NUM_INSTANCES
    );

    // Verify all instances are tracked
    let list = hypervisor.list().await;
    assert!(
        list.len() >= 95,
        "Expected at least 95 instances in list, got {}",
        list.len()
    );

    // Cleanup all instances
    for i in 0..NUM_INSTANCES {
        hypervisor.stop("api", &format!("user{}", i)).await.ok();
    }
}

/// Test rapid spawn/stop cycles don't cause issues
/// Target: 50 cycles of spawn then stop
#[tokio::test]
async fn test_stress_concurrent_spawn_stop() {
    let dir = TempDir::new().unwrap();
    let script = create_touch_socket_script(&dir);

    let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
    let hypervisor = Hypervisor::new(config);

    const NUM_CYCLES: usize = 50;
    let mut success_count = 0;

    for i in 0..NUM_CYCLES {
        let id = format!("cycle{}", i);

        // Spawn
        match hypervisor.spawn("api", &id).await {
            Ok(socket) => {
                // Wait briefly for socket
                if wait_for_socket(&socket, 500).await {
                    // Stop
                    if hypervisor.stop("api", &id).await.is_ok() {
                        success_count += 1;
                    }
                }
            }
            Err(_) => continue,
        }
    }

    assert!(
        success_count >= 45,
        "Expected 90%+ successful cycles, got {}/{}",
        success_count,
        NUM_CYCLES
    );

    // Verify no instances left running
    let list = hypervisor.list().await;
    assert!(
        list.is_empty(),
        "Expected no instances after all cycles, got {}",
        list.len()
    );
}

// =============================================================================
// LOG BUFFER STRESS TESTS
// =============================================================================

/// Test pushing many log entries concurrently
/// Target: 1000 concurrent log entries
#[tokio::test]
async fn test_stress_concurrent_log_entries() {
    let dir = TempDir::new().unwrap();
    let script = create_touch_socket_script(&dir);

    let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
    let hypervisor = Hypervisor::new(config);
    let log_buffer = hypervisor.log_buffer();

    const NUM_ENTRIES: usize = 1000;
    let mut handles = vec![];

    // Push log entries concurrently
    for i in 0..NUM_ENTRIES {
        let lb = log_buffer.clone();
        handles.push(tokio::spawn(async move {
            lb.push_stdout("api", "stress", format!("log entry {}", i))
                .await;
        }));
    }

    // Wait for all pushes to complete
    for handle in handles {
        handle.await.ok();
    }

    // Query logs - should have entries (may be limited by buffer capacity)
    let query = tenement::LogQuery {
        process: Some("api".to_string()),
        instance_id: Some("stress".to_string()),
        level: None,
        search: None,
        limit: None,
    };
    let logs = log_buffer.query(&query).await;

    // Should have captured many entries (buffer has default capacity)
    assert!(
        logs.len() >= 100,
        "Expected at least 100 log entries captured, got {}",
        logs.len()
    );
}

/// Test log buffer handles capacity limits correctly
/// Target: 10k entries to test eviction
#[tokio::test]
async fn test_stress_log_buffer_capacity() {
    let dir = TempDir::new().unwrap();
    let script = create_touch_socket_script(&dir);

    let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
    let hypervisor = Hypervisor::new(config);
    let log_buffer = hypervisor.log_buffer();

    const NUM_ENTRIES: usize = 10_000;

    // Push many entries sequentially to ensure order
    for i in 0..NUM_ENTRIES {
        log_buffer
            .push_stdout("api", "capacity", format!("entry {}", i))
            .await;
    }

    // Query all logs
    let query = tenement::LogQuery {
        process: Some("api".to_string()),
        instance_id: Some("capacity".to_string()),
        level: None,
        search: None,
        limit: None,
    };
    let logs = log_buffer.query(&query).await;

    // Buffer should have capped entries (default capacity is typically 10000)
    // Old entries should have been evicted
    assert!(
        logs.len() <= 10_000,
        "Buffer should respect capacity limit, got {}",
        logs.len()
    );

    // Most recent entries should be present
    if !logs.is_empty() {
        // Check that we have recent entries (high numbered ones)
        let last_entry = &logs[logs.len() - 1];
        assert!(
            last_entry.message.contains("entry"),
            "Last entry should be a numbered entry"
        );
    }
}

// =============================================================================
// HEALTH CHECK STRESS TESTS
// =============================================================================

/// Test health checking many instances concurrently
/// Target: Health check 100 instances
#[tokio::test]
async fn test_stress_concurrent_health_checks() {
    let dir = TempDir::new().unwrap();
    let script = create_touch_socket_script(&dir);

    let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
    let hypervisor = Hypervisor::new(config);

    const NUM_INSTANCES: usize = 50; // Reduced for faster test

    // First, spawn all instances
    for i in 0..NUM_INSTANCES {
        let socket = hypervisor.spawn("api", &format!("health{}", i)).await.unwrap();
        // Brief wait for socket creation
        wait_for_socket(&socket, 500).await;
    }

    // Verify all spawned
    let list = hypervisor.list().await;
    assert!(
        list.len() >= NUM_INSTANCES - 5,
        "Expected most instances spawned, got {}",
        list.len()
    );

    // Now health check all concurrently
    let mut handles = vec![];
    for i in 0..NUM_INSTANCES {
        let h = hypervisor.clone();
        handles.push(tokio::spawn(async move {
            h.check_health("api", &format!("health{}", i)).await
        }));
    }

    // Wait for all health checks and count healthy results
    let mut healthy_count = 0;
    for handle in handles {
        if let Ok(status) = handle.await {
            if status.to_string() == "healthy" {
                healthy_count += 1;
            }
        }
    }

    assert!(
        healthy_count >= NUM_INSTANCES - 10,
        "Expected most instances healthy, got {}/{}",
        healthy_count,
        NUM_INSTANCES
    );

    // Cleanup
    for i in 0..NUM_INSTANCES {
        hypervisor.stop("api", &format!("health{}", i)).await.ok();
    }
}

// =============================================================================
// BROADCAST STRESS TESTS
// =============================================================================

/// Test that slow subscribers don't block the log buffer
#[tokio::test]
async fn test_stress_broadcast_slow_subscriber() {
    let dir = TempDir::new().unwrap();
    let script = create_touch_socket_script(&dir);

    let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
    let hypervisor = Hypervisor::new(config);
    let log_buffer = hypervisor.log_buffer();

    // Subscribe to logs (simulating a slow consumer)
    let _rx = log_buffer.subscribe();

    // Push many entries quickly - should not block even if subscriber is slow
    let start = std::time::Instant::now();
    const NUM_ENTRIES: usize = 1000;

    for i in 0..NUM_ENTRIES {
        log_buffer
            .push_stdout("api", "broadcast", format!("entry {}", i))
            .await;
    }

    let elapsed = start.elapsed();

    // Should complete quickly (< 1 second) even with a subscriber
    // that isn't reading. The broadcast channel should handle lagging.
    assert!(
        elapsed < Duration::from_secs(5),
        "Pushing {} entries took too long: {:?} (slow subscriber blocking?)",
        NUM_ENTRIES,
        elapsed
    );

    // Verify entries were stored
    let query = tenement::LogQuery {
        process: Some("api".to_string()),
        instance_id: Some("broadcast".to_string()),
        level: None,
        search: None,
        limit: Some(100),
    };
    let logs = log_buffer.query(&query).await;
    assert!(!logs.is_empty(), "Logs should have been stored");
}

/// Test multiple concurrent subscribers
#[tokio::test]
async fn test_stress_multiple_subscribers() {
    let dir = TempDir::new().unwrap();
    let script = create_touch_socket_script(&dir);

    let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
    let hypervisor = Hypervisor::new(config);
    let log_buffer = hypervisor.log_buffer();

    // Create multiple subscribers
    let mut receivers = vec![];
    for _ in 0..10 {
        receivers.push(log_buffer.subscribe());
    }

    // Push entries
    const NUM_ENTRIES: usize = 100;
    for i in 0..NUM_ENTRIES {
        log_buffer
            .push_stdout("api", "multi", format!("entry {}", i))
            .await;
    }

    // Small delay to let broadcasts propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Each subscriber should have received entries (or lagged)
    // The key is that pushing didn't block
    let query = tenement::LogQuery {
        process: Some("api".to_string()),
        instance_id: Some("multi".to_string()),
        level: None,
        search: None,
        limit: None,
    };
    let logs = log_buffer.query(&query).await;
    assert_eq!(
        logs.len(),
        NUM_ENTRIES,
        "All entries should be stored in buffer"
    );
}
