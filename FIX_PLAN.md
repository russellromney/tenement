# Implementation Fix Plan

Fixes and improvements identified during phases 5-8 code review, organized by priority.

---

## Critical (P0) - Blocking Functionality

### 1. Unix Socket Proxy Not Implemented

**Location:** `cli/src/server.rs:317-326`

**Problem:** The entire routing system returns `BAD_GATEWAY` with "not yet implemented". Wake-on-request works but traffic can't actually be served.

**Fix:**

```rust
// Replace placeholder in proxy_to_instance()

use hyper::Uri;
use hyperlocal::{UnixClientExt, UnixConnector};

async fn proxy_to_instance(
    state: &AppState,
    process: &str,
    id: &str,
    req: Request<Body>,
) -> Response {
    // ... existing instance lookup code ...

    // Create Unix socket client
    let connector = UnixConnector;
    let client: Client<UnixConnector, Body> = Client::builder(TokioExecutor::new())
        .build(connector);

    // Build URI for Unix socket
    let socket_uri = hyperlocal::Uri::new(&socket_path, req.uri().path_and_query().map(|x| x.as_str()).unwrap_or("/"));

    // Forward request
    let mut proxy_req = Request::builder()
        .method(req.method())
        .uri(socket_uri);

    // Copy headers
    for (key, value) in req.headers() {
        proxy_req = proxy_req.header(key, value);
    }

    let proxy_req = proxy_req.body(req.into_body()).unwrap();

    match client.request(proxy_req).await {
        Ok(response) => response.into_response(),
        Err(e) => {
            tracing::error!("Proxy error: {}", e);
            (StatusCode::BAD_GATEWAY, format!("Proxy error: {}", e)).into_response()
        }
    }
}
```

**Dependencies to add:**
```toml
# cli/Cargo.toml
hyperlocal = "0.9"
```

**Tasks:**
- [x] Add hyperlocal dependency
- [x] Implement Unix socket proxy
- [x] Add tests for proxy functionality
- [ ] Test with real backend process

---

### 2. Auth Middleware Not Wired

**Location:** `cli/src/server.rs`

**Problem:** API endpoints are unauthenticated. Auth module exists but isn't connected.

**Fix:**

```rust
// Add to cli/src/server.rs

use axum::{
    extract::Request,
    middleware::{self, Next},
    response::Response,
};
use tenement::auth::TokenStore;
use tenement::store::ConfigStore;

// Add to AppState
pub struct AppState {
    pub hypervisor: Arc<Hypervisor>,
    pub domain: String,
    pub client: Client<...>,
    pub config_store: Arc<ConfigStore>,  // NEW
}

// Auth middleware
async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Skip auth for public endpoints
    let path = req.uri().path();
    if path == "/health" || path == "/metrics" || path == "/" || path.starts_with("/assets/") {
        return Ok(next.run(req).await);
    }

    // Extract token from Authorization header
    let auth_header = req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.to_lowercase().starts_with("bearer ") => &h[7..],
        _ => return Err(StatusCode::UNAUTHORIZED),
    };

    // Verify token
    let token_store = TokenStore::new(&state.config_store);
    if !token_store.verify(token).await.unwrap_or(false) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

// Update create_router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/health", get(health))
        .route("/metrics", get(metrics_endpoint))
        .route("/api/instances", get(list_instances))
        .route("/api/logs", get(query_logs))
        .route("/api/logs/stream", get(stream_logs))
        .route("/assets/*path", get(dashboard_asset))
        .fallback(handle_request)
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

**Tasks:**
- [x] Add ConfigStore to AppState
- [x] Create auth middleware function
- [x] Wire middleware to router
- [x] Update serve() to initialize ConfigStore
- [x] Add CLI flag for initial token generation (already exists as `ten token-gen`)

---

### 3. Foreign Keys Not Enabled in Slum DB

**Location:** `slum/src/db.rs:100`

**Problem:** Foreign key constraint defined but SQLite FOREIGN_KEYS pragma not enabled.

**Fix:**

```rust
// In SlumDb::init(), after pool creation:

sqlx::query("PRAGMA foreign_keys = ON")
    .execute(&pool)
    .await
    .context("Failed to enable foreign keys")?;
```

**Tasks:**
- [x] Add PRAGMA foreign_keys = ON
- [x] Add test that violating FK constraint fails

---

## High Priority (P1) - Important Improvements

### 4. Race Condition in Proxy Request Handling

**Location:** `cli/src/server.rs:279-315`

**Problem:** After checking `is_running`, instance could be reaped before `touch_activity`.

**Fix:**

```rust
// Use atomic get-and-touch operation
async fn proxy_to_instance(state: &AppState, process: &str, id: &str, req: Request<Body>) -> Response {
    // Single atomic operation: get instance info AND touch activity
    match state.hypervisor.get_and_touch(process, id).await {
        Some(info) => {
            // Instance running, proceed with proxy
            proxy_request_to_socket(&info.socket, req).await
        }
        None => {
            // Not running, try wake-on-request
            if !state.hypervisor.has_process(process) {
                return (StatusCode::NOT_FOUND, "Process not configured").into_response();
            }
            match state.hypervisor.spawn_and_wait(process, id).await {
                Ok(socket) => proxy_request_to_socket(&socket, req).await,
                Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e.to_string()).into_response(),
            }
        }
    }
}
```

**Add to Hypervisor:**
```rust
/// Get instance info and touch activity atomically
pub async fn get_and_touch(&self, process: &str, id: &str) -> Option<InstanceInfo> {
    let mut instances = self.instances.write().await;
    let instance_id = InstanceId::new(process, id);
    if let Some(instance) = instances.get_mut(&instance_id) {
        instance.touch();
        Some(instance.info())
    } else {
        None
    }
}
```

**Tasks:**
- [x] Add get_and_touch() to Hypervisor
- [x] Update proxy_to_instance to use atomic operation
- [x] Add test for race condition

---

### 5. Duplicate Test Helpers

**Location:** Multiple files

**Problem:** `create_test_db()` duplicated in `store.rs` and `slum/db.rs`.

**Fix:**

Create shared test utilities module (covered in E2E_TESTING_PLAN Session 1).

**Tasks:**
- [ ] Create tenement/tests/common/mod.rs
- [ ] Move shared helpers there
- [ ] Update existing tests to use common module

---

### 6. Cgroup Warning Order Issue

**Location:** `tenement/src/cgroup.rs:78-81`

**Problem:** When limits exist but cgroups unavailable, warning isn't logged.

**Fix:**

```rust
pub fn setup(name: &str, limits: &ResourceLimits) -> Result<Option<PathBuf>> {
    // Check availability first
    if !is_available() {
        if limits.has_limits() {
            tracing::warn!(
                "Cgroup limits requested for '{}' but cgroups v2 not available",
                name
            );
        }
        return Ok(None);
    }

    // Then check if limits are needed
    if !limits.has_limits() {
        return Ok(None);
    }

    // ... rest of implementation
}
```

**Tasks:**
- [x] Fix check order in cgroup::create_cgroup()
- [x] Existing tests cover behavior (warning only logged when limits exist)

---

## Medium Priority (P2) - Code Quality

### 7. Verbose Dynamic Query Binding

**Location:** `tenement/src/store.rs:170-202`

**Problem:** Manual binding for different parameter counts is verbose and error-prone.

**Fix:** Use sqlx::QueryBuilder

```rust
use sqlx::QueryBuilder;

pub async fn query(&self, query: &LogQuery) -> Result<Vec<LogEntry>> {
    let limit = query.limit.unwrap_or(100);

    if let Some(ref search) = query.search {
        return self.query_fts(query, search, limit).await;
    }

    let mut builder = QueryBuilder::new(
        "SELECT id, timestamp, level, process, instance_id, message FROM logs WHERE 1=1"
    );

    if let Some(ref process) = query.process {
        builder.push(" AND process = ");
        builder.push_bind(process);
    }

    if let Some(ref id) = query.instance_id {
        builder.push(" AND instance_id = ");
        builder.push_bind(id);
    }

    if let Some(level) = query.level {
        builder.push(" AND level = ");
        builder.push_bind(level.to_string());
    }

    builder.push(" ORDER BY timestamp DESC LIMIT ");
    builder.push_bind(limit as i64);

    let rows = builder.build().fetch_all(&self.pool).await?;

    Ok(rows.into_iter().map(|row| /* ... */).collect())
}
```

**Tasks:**
- [ ] Refactor query() to use QueryBuilder
- [ ] Refactor query_fts() similarly
- [ ] Verify tests still pass

---

### 8. Silent Cgroup Cleanup Errors

**Location:** `tenement/src/cgroup.rs:199-209`

**Problem:** Errors moving processes to parent cgroup are silently ignored.

**Fix:**

```rust
pub fn remove(name: &str) -> Result<()> {
    let cgroup_path = cgroup_path(name);
    if !cgroup_path.exists() {
        return Ok(());
    }

    // Move processes to parent first
    let procs_file = cgroup_path.join("cgroup.procs");
    if procs_file.exists() {
        let pids = std::fs::read_to_string(&procs_file)?;
        let parent_procs = cgroup_path.parent()
            .map(|p| p.join("cgroup.procs"))
            .filter(|p| p.exists());

        for pid in pids.lines() {
            if !pid.is_empty() {
                if let Some(ref parent) = parent_procs {
                    if let Err(e) = std::fs::write(parent, pid) {
                        tracing::warn!("Failed to move PID {} to parent cgroup: {}", pid, e);
                        // Continue anyway - process may have exited
                    }
                }
            }
        }
    }

    // Remove cgroup directory
    if let Err(e) = std::fs::remove_dir(&cgroup_path) {
        tracing::warn!("Failed to remove cgroup dir {}: {}", cgroup_path.display(), e);
    }

    Ok(())
}
```

**Tasks:**
- [x] Add logging for PID migration failures
- [x] Add logging for rmdir failures

---

### 9. TokenStore Lifetime Awkwardness

**Location:** `tenement/src/auth.rs`

**Problem:** `TokenStore<'a>` borrows ConfigStore, making it awkward to store in AppState.

**Fix:**

```rust
// Change from borrowed to owned
pub struct TokenStore {
    config_store: Arc<ConfigStore>,
}

impl TokenStore {
    pub fn new(config_store: Arc<ConfigStore>) -> Self {
        Self { config_store }
    }

    // ... methods unchanged
}
```

**Tasks:**
- [ ] Change TokenStore to use Arc<ConfigStore>
- [ ] Update all call sites

---

### 10. Slum Route Query Inefficiency

**Location:** `slum/src/db.rs:301-313`

**Problem:** `route()` makes two separate queries.

**Fix:**

```rust
pub async fn route(&self, domain: &str) -> Result<Option<(Tenant, Server)>> {
    let row = sqlx::query(r#"
        SELECT
            t.id as tenant_id, t.name as tenant_name, t.domain,
            t.server_id, t.process, t.instance_id, t.created_at as tenant_created,
            s.id as server_id, s.name as server_name, s.url,
            s.region, s.status, s.last_seen, s.created_at as server_created
        FROM tenants t
        JOIN servers s ON t.server_id = s.id
        WHERE t.domain = ?
    "#)
    .bind(domain)
    .fetch_optional(&self.pool)
    .await?;

    Ok(row.map(|r| {
        let tenant = Tenant { /* extract from row */ };
        let server = Server { /* extract from row */ };
        (tenant, server)
    }))
}
```

**Tasks:**
- [ ] Refactor route() to use JOIN
- [ ] Verify tests pass

---

## Low Priority (P3) - Nice to Have

### 11. Dashboard Asset Caching

**Location:** `cli/src/dashboard.rs`

**Problem:** Static assets served without cache headers.

**Fix:**

```rust
async fn serve_asset(path: &str) -> impl IntoResponse {
    // ... existing code ...

    let headers = [
        (header::CONTENT_TYPE, content_type),
        (header::CACHE_CONTROL, "public, max-age=86400"), // 24 hours
    ];

    (headers, body).into_response()
}
```

**Tasks:**
- [ ] Add Cache-Control header
- [ ] Consider ETag support

---

### 12. Silent Verification Failures in Auth

**Location:** `tenement/src/auth.rs:34-42`

**Problem:** `verify_token` returns false for malformed hashes without logging.

**Fix:**

```rust
pub fn verify_token(token: &str, hash: &str) -> bool {
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!("Invalid password hash format: {}", e);
            return false;
        }
    };

    Argon2::default()
        .verify_password(token.as_bytes(), &parsed_hash)
        .is_ok()
}
```

**Tasks:**
- [x] Add debug logging for parse failures

---

### 13. CPU Weight Clamping Logging

**Location:** `tenement/src/cgroup.rs:129`

**Problem:** CPU weight clamped silently.

**Fix:**

```rust
if let Some(cpu_weight) = limits.cpu_shares {
    let clamped = cpu_weight.clamp(1, 10000);
    if clamped != cpu_weight {
        tracing::info!(
            "CPU weight {} clamped to {} for '{}'",
            cpu_weight, clamped, name
        );
    }
    std::fs::write(cgroup_path.join("cpu.weight"), clamped.to_string())?;
}
```

**Tasks:**
- [x] Add logging when value is clamped

---

### 14. Idle Timeout Zero Documentation

**Location:** `tenement/src/instance.rs:141-146`

**Problem:** `idle_timeout = 0` means "never stop" but isn't documented.

**Fix:**

```rust
/// Check if this instance has been idle longer than its timeout.
///
/// Returns false if:
/// - No idle_timeout is configured (None)
/// - idle_timeout is set to 0 (explicit "never stop")
///
/// Only returns true when idle_timeout > 0 AND the instance has been
/// idle for longer than that duration.
pub fn is_idle(&self) -> bool {
    // ...
}
```

**Tasks:**
- [ ] Add documentation for edge cases

---

## Implementation Order

| Priority | Item | Blocks |
|----------|------|--------|
| P0-1 | Unix Socket Proxy | E2E tests, production use |
| P0-2 | Auth Middleware | Auth integration tests |
| P0-3 | FK Pragma | Data integrity |
| P1-4 | Race Condition | Reliability |
| P1-5 | Test Helpers | Test sessions 2-8 |
| P1-6 | Cgroup Warning | Debug experience |
| P2-7 | QueryBuilder | Code quality |
| P2-8 | Cgroup Cleanup Logging | Debug experience |
| P2-9 | TokenStore Lifetime | Code ergonomics |
| P2-10 | Slum Route JOIN | Performance |
| P3-* | Nice to haves | - |

---

## Next Session Starter

After completing P0 fixes, proceed to E2E_TESTING_PLAN.md Session 1.
