# E2E Testing Plan

Comprehensive testing plan for tenement, broken into sequential implementation sessions.

---

## Session 1: Test Infrastructure Setup

**Goal:** Create shared test utilities and fixtures that all subsequent sessions depend on.

### 1.1 Create Test Utilities Module

**File:** `tenement/tests/common/mod.rs`

```rust
//! Shared test utilities for integration and E2E tests

use tenement::{Config, ProcessConfig, Hypervisor};
use tenement::runtime::RuntimeType;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

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
pub fn test_config_with_limits(name: &str, command: &str, memory_mb: u32, cpu_shares: u32) -> Config {
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

/// Create test database
pub async fn create_test_db() -> (tenement::store::DbPool, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    let pool = tenement::store::init_db(&path).await.unwrap();
    (pool, dir)
}
```

### 1.2 Create Test Fixtures

**File:** `tenement/tests/fixtures/mock_server.sh`
```bash
#!/bin/bash
# Mock server that creates a socket and responds to HTTP requests
SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
rm -f "$SOCKET_PATH"

# Use socat if available, otherwise Python
if command -v socat &> /dev/null; then
    socat UNIX-LISTEN:"$SOCKET_PATH",fork EXEC:"echo -e 'HTTP/1.1 200 OK\r\n\r\nOK'"
else
    python3 -c "
import socket
import os
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.bind('$SOCKET_PATH')
sock.listen(1)
while True:
    conn, _ = sock.accept()
    conn.recv(1024)
    conn.sendall(b'HTTP/1.1 200 OK\r\n\r\nOK')
    conn.close()
"
fi
```

**File:** `tenement/tests/fixtures/slow_startup.sh`
```bash
#!/bin/bash
# Server that delays socket creation (for testing startup timeout)
SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
DELAY="${DELAY:-3}"

sleep "$DELAY"
rm -f "$SOCKET_PATH"
touch "$SOCKET_PATH"
sleep 30
```

**File:** `tenement/tests/fixtures/crash_on_health.sh`
```bash
#!/bin/bash
# Server that creates socket but fails health checks
SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
rm -f "$SOCKET_PATH"

# Create socket that returns 500 on health checks
python3 -c "
import socket
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.bind('$SOCKET_PATH')
sock.listen(1)
while True:
    conn, _ = sock.accept()
    data = conn.recv(1024).decode()
    if '/health' in data:
        conn.sendall(b'HTTP/1.1 500 Internal Server Error\r\n\r\nFailed')
    else:
        conn.sendall(b'HTTP/1.1 200 OK\r\n\r\nOK')
    conn.close()
"
```

**File:** `tenement/tests/fixtures/exit_immediately.sh`
```bash
#!/bin/bash
# Process that exits immediately (for testing restart behavior)
SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
touch "$SOCKET_PATH"
exit 1
```

### 1.3 Tasks

- [x] Create `tenement/tests/common/mod.rs`
- [x] Create `tenement/tests/fixtures/` directory
- [x] Create all fixture scripts (mock_server.sh, slow_startup.sh, crash_on_health.sh, exit_immediately.sh)
- [x] Make fixture scripts executable
- [x] Create `tenement/tests/test_utils_verify.rs` with 9 verification tests
- [x] Verify fixtures work standalone

---

## Session 2: Auth Integration Tests

**Goal:** Test authentication middleware integration with API endpoints.

**Prerequisite:** Auth middleware must be wired to server (see FIX_PLAN.md)

### 2.1 Tests to Implement

**File:** `cli/tests/auth_integration.rs`

| Test | Description |
|------|-------------|
| `test_api_instances_requires_auth` | GET /api/instances without token returns 401 |
| `test_api_instances_with_valid_token` | GET /api/instances with valid Bearer token returns 200 |
| `test_api_instances_with_invalid_token` | GET /api/instances with bad token returns 401 |
| `test_api_logs_requires_auth` | GET /api/logs without token returns 401 |
| `test_api_logs_stream_requires_auth` | GET /api/logs/stream without token returns 401 |
| `test_health_no_auth_required` | GET /health works without token |
| `test_metrics_no_auth_required` | GET /metrics works without token |
| `test_dashboard_no_auth_required` | GET / works without token |
| `test_token_in_header` | Authorization: Bearer <token> format |
| `test_token_case_insensitive` | "bearer" vs "Bearer" both work |

### 2.2 Test Implementation Template

```rust
use axum_test::TestServer;
use cli::server::{create_router, AppState};
use tenement::{Config, Hypervisor};
use tenement::auth::{generate_token, TokenStore};
use tenement::store::{init_db, ConfigStore};

async fn setup_with_auth() -> (TestServer, String) {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = init_db(&db_path).await.unwrap();
    let config_store = ConfigStore::new(pool.clone());
    let token_store = TokenStore::new(&config_store);

    let token = token_store.generate_and_store().await.unwrap();

    let config = Config::default();
    let hypervisor = Hypervisor::new(config);
    let state = AppState {
        hypervisor,
        domain: "example.com".to_string(),
        // ... add token_store to state
    };

    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    (server, token)
}

#[tokio::test]
async fn test_api_instances_requires_auth() {
    let (server, _token) = setup_with_auth().await;

    let response = server.get("/api/instances").await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_api_instances_with_valid_token() {
    let (server, token) = setup_with_auth().await;

    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_ok();
}
```

### 2.3 Tasks

- [ ] Wire auth middleware to server (FIX_PLAN prerequisite)
- [ ] Create `cli/tests/auth_integration.rs`
- [ ] Implement all 10 auth tests
- [ ] Verify tests pass with middleware

---

## Session 3: Hypervisor Integration Tests

**Goal:** Test hypervisor + server + storage integration.

### 3.1 Tests to Implement

**File:** `cli/tests/hypervisor_integration.rs`

| Test | Description |
|------|-------------|
| `test_spawn_appears_in_api_list` | Spawn instance, GET /api/instances shows it |
| `test_stop_removes_from_api_list` | Stop instance, GET /api/instances excludes it |
| `test_spawn_logs_captured` | Spawn echo process, logs appear in GET /api/logs |
| `test_metrics_update_on_spawn` | instances_up metric increments after spawn |
| `test_metrics_update_on_stop` | instances_up metric decrements after stop |
| `test_restart_increments_counter` | Restart instance, check restarts field in API |
| `test_health_status_in_api` | Health status reflected in /api/instances response |

### 3.2 Tasks

- [ ] Create `cli/tests/hypervisor_integration.rs`
- [ ] Implement all 7 tests
- [ ] Ensure cleanup in test teardown

---

## Session 4: E2E Lifecycle Tests

**Goal:** Test complete instance lifecycle from spawn to cleanup.

### 4.1 Tests to Implement

**File:** `tenement/tests/e2e/lifecycle.rs`

| Test | Description |
|------|-------------|
| `test_full_spawn_to_stop_lifecycle` | Spawn → verify running → stop → verify cleaned up |
| `test_health_check_updates_status` | Spawn → check health → verify status updates |
| `test_idle_timeout_triggers_reap` | Spawn with idle_timeout → wait → verify reaped |
| `test_wake_on_request` | Spawn → reap → request → verify respawned |
| `test_restart_on_unhealthy` | Spawn unhealthy → verify auto-restart |
| `test_max_restarts_enters_failed` | Configure max_restarts → trigger → verify Failed |
| `test_backoff_delay_applied` | Restart → verify delay increases |
| `test_socket_cleanup_on_stop` | Stop → verify socket file removed |
| `test_data_dir_created` | Spawn → verify data dir exists |

### 4.2 Full Lifecycle Test Implementation

```rust
#[tokio::test]
async fn test_full_spawn_to_stop_lifecycle() {
    let dir = TempDir::new().unwrap();
    let script = create_fixture_script(dir.path(), "mock_server.sh");

    let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
    let hypervisor = Hypervisor::new(config);

    // 1. Spawn
    let socket = hypervisor.spawn("api", "user1").await.unwrap();

    // 2. Verify running
    assert!(hypervisor.is_running("api", "user1").await);
    let list = hypervisor.list().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id.to_string(), "api:user1");

    // 3. Verify socket created
    assert!(wait_for_socket(&socket, 1000).await);

    // 4. Stop
    hypervisor.stop("api", "user1").await.unwrap();

    // 5. Verify not running
    assert!(!hypervisor.is_running("api", "user1").await);
    let list = hypervisor.list().await;
    assert!(list.is_empty());

    // 6. Verify socket removed
    assert!(wait_for_socket_removed(&socket, 1000).await);
}

#[tokio::test]
async fn test_idle_timeout_triggers_reap() {
    let dir = TempDir::new().unwrap();
    let script = create_fixture_script(dir.path(), "mock_server.sh");

    // 2 second idle timeout
    let config = test_config_with_idle_timeout("api", script.to_str().unwrap(), 2);
    let hypervisor = Hypervisor::new(config);

    // Spawn
    hypervisor.spawn("api", "user1").await.unwrap();
    assert!(hypervisor.is_running("api", "user1").await);

    // Wait for idle timeout + reap cycle (health_check_interval default is 10s)
    // For testing, we call reap_idle_instances directly
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Manually trigger reap (normally done by health monitor)
    hypervisor.reap_idle_instances().await;

    // Should be reaped
    assert!(!hypervisor.is_running("api", "user1").await);
}
```

### 4.3 Tasks

- [ ] Create `tenement/tests/e2e/` directory
- [ ] Create `tenement/tests/e2e/lifecycle.rs`
- [ ] Implement all 9 lifecycle tests
- [ ] Add `mod e2e;` to tests module

---

## Session 5: Cgroup Lifecycle Tests (Linux Only)

**Goal:** Test cgroup creation, limits application, and cleanup.

### 5.1 Tests to Implement

**File:** `tenement/tests/e2e/cgroup.rs`

| Test | Description |
|------|-------------|
| `test_cgroup_created_with_limits` | Spawn with limits → verify cgroup dir exists |
| `test_cgroup_memory_limit_set` | Verify memory.max contains correct value |
| `test_cgroup_cpu_weight_set` | Verify cpu.weight contains correct value |
| `test_cgroup_process_added` | Verify PID in cgroup.procs |
| `test_cgroup_removed_on_stop` | Stop → verify cgroup dir removed |
| `test_cgroup_no_limits_no_cgroup` | Spawn without limits → no cgroup created |

### 5.2 Notes

- All tests should be marked `#[cfg(target_os = "linux")]`
- Tests requiring root should be marked `#[ignore]`
- Consider using a mock cgroup filesystem for non-privileged tests

### 5.3 Tasks

- [ ] Create `tenement/tests/e2e/cgroup.rs`
- [ ] Implement cgroup tests
- [ ] Add CI configuration for Linux-only tests

---

## Session 6: Stress Tests

**Goal:** Test system behavior under concurrent load.

### 6.1 Tests to Implement

**File:** `tenement/tests/stress/concurrent.rs`

| Test | Description | Target |
|------|-------------|--------|
| `test_stress_concurrent_spawns` | Spawn N instances simultaneously | 100 |
| `test_stress_concurrent_spawn_stop` | Rapid spawn/stop cycles | 50 cycles |
| `test_stress_concurrent_log_entries` | Push N log entries concurrently | 1000 |
| `test_stress_concurrent_health_checks` | Health check N instances | 100 |
| `test_stress_log_buffer_capacity` | Fill buffer, verify eviction | 10k entries |
| `test_stress_broadcast_slow_subscriber` | Slow subscriber doesn't block | - |

### 6.2 Implementation Pattern

```rust
#[tokio::test]
async fn test_stress_concurrent_spawns() {
    let config = test_config_with_process("api", "sleep", vec!["30"]);
    let hypervisor = Arc::new(Hypervisor::new(config));

    let mut handles = vec![];
    for i in 0..100 {
        let h = hypervisor.clone();
        handles.push(tokio::spawn(async move {
            h.spawn("api", &format!("user{}", i)).await
        }));
    }

    let results = futures::future::join_all(handles).await;
    let successes: usize = results.iter()
        .filter(|r| r.as_ref().map(|r| r.is_ok()).unwrap_or(false))
        .count();

    assert!(successes >= 95, "Expected 95%+ success, got {}", successes);

    // Cleanup
    for i in 0..100 {
        hypervisor.stop("api", &format!("user{}", i)).await.ok();
    }
}
```

### 6.3 Tasks

- [ ] Create `tenement/tests/stress/` directory
- [ ] Create `tenement/tests/stress/concurrent.rs`
- [ ] Implement all 6 stress tests
- [ ] Add CI configuration with extended timeout

---

## Session 7: Performance Benchmarks

**Goal:** Establish performance baselines with criterion benchmarks.

### 7.1 Benchmarks to Implement

**File:** `tenement/benches/performance.rs`

| Benchmark | Target Metric |
|-----------|---------------|
| `bench_spawn_latency` | <500ms to socket ready |
| `bench_health_check_latency` | <10ms roundtrip |
| `bench_log_buffer_push` | >100k entries/sec |
| `bench_log_buffer_query` | <1ms for 100 results |
| `bench_fts_search` | <50ms on 100k entries |
| `bench_metrics_format` | <1ms |
| `bench_subdomain_parse` | <1μs |
| `bench_config_parse` | <1ms |

### 7.2 Setup

**Add to `tenement/Cargo.toml`:**
```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["async_tokio"] }

[[bench]]
name = "performance"
harness = false
```

### 7.3 Implementation Template

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use tokio::runtime::Runtime;

fn bench_log_buffer_push(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let buffer = rt.block_on(async { tenement::logs::LogBuffer::new() });

    c.bench_function("log_buffer_push", |b| {
        b.to_async(&rt).iter(|| async {
            buffer.push_stdout("api", "prod", "test message".to_string()).await;
        })
    });
}

fn bench_subdomain_parse(c: &mut Criterion) {
    c.bench_function("subdomain_parse", |b| {
        b.iter(|| {
            cli::server::parse_subdomain("prod.api.example.com", "example.com")
        })
    });
}

criterion_group!(benches, bench_log_buffer_push, bench_subdomain_parse);
criterion_main!(benches);
```

### 7.4 Tasks

- [ ] Add criterion dependency
- [ ] Create `tenement/benches/performance.rs`
- [ ] Implement all 8 benchmarks
- [ ] Run baseline and document results
- [ ] Add benchmark CI job (optional)

---

## Session 8: Slum Integration Tests

**Goal:** Test slum fleet orchestration integration.

### 8.1 Tests to Implement

**File:** `slum/tests/integration.rs`

| Test | Description |
|------|-------------|
| `test_server_health_check_updates_status` | Ping server → status updates |
| `test_tenant_routing_to_server` | Route domain → correct server |
| `test_multiple_tenants_same_server` | Multiple tenants coexist |
| `test_tenant_migration` | Move tenant to different server |
| `test_server_offline_tenant_unreachable` | Offline server → tenant errors |

### 8.2 Tasks

- [ ] Create `slum/tests/integration.rs`
- [ ] Implement all 5 tests
- [ ] Test with mock HTTP servers

---

## CI Configuration

### GitHub Actions Additions

**File:** `.github/workflows/test.yml` (additions)

```yaml
jobs:
  integration-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        uses: dtolnay/rust-action@stable
      - name: Run integration tests
        run: cargo test --test '*_integration' -- --test-threads=1
        timeout-minutes: 10

  stress-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        uses: dtolnay/rust-action@stable
      - name: Run stress tests
        run: cargo test --release stress_ -- --test-threads=1
        timeout-minutes: 15

  benchmarks:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        uses: dtolnay/rust-action@stable
      - name: Run benchmarks
        run: cargo bench --bench performance -- --noplot
```

---

## Summary

| Session | Focus | Tests | Prerequisite |
|---------|-------|-------|--------------|
| 1 | Infrastructure | Setup | None |
| 2 | Auth Integration | 10 | Auth middleware (FIX_PLAN) |
| 3 | Hypervisor Integration | 7 | Session 1 |
| 4 | E2E Lifecycle | 9 | Session 1 |
| 5 | Cgroup Lifecycle | 6 | Session 1, Linux |
| 6 | Stress Tests | 6 | Session 1 |
| 7 | Benchmarks | 8 | Session 1 |
| 8 | Slum Integration | 5 | Session 1 |

**Total: 51 new tests + 8 benchmarks**
