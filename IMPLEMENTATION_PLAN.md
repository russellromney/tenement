# Tenement Implementation Plan

**Goal:** Lightweight Rust PaaS - "Piku but Rust, batteries included, plays nice with others"

## What We're Building

Single binary that replaces:
- nginx (routing)
- uWSGI (process management)
- Basic monitoring stack (metrics/logs)

```
tenement (single binary, ~10MB RAM)
├── HTTP server + reverse proxy (axum)
├── Process hypervisor (spawn, health, restart)
├── Metrics: in-memory + /metrics (Prometheus format)
├── Logs: SQLite + SSE stream
└── Dashboard: embedded Svelte SPA
```

## Routing

Subdomain pattern: `{id}.{process}.example.com`

```
prod.api.example.com      → api:prod
staging.api.example.com   → api:staging
user123.app.example.com   → app:user123
pr-456.web.example.com    → web:pr-456
example.com               → tenement dashboard
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    tenement serve                            │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  HTTP Server (axum) :8080                                   │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ example.com        → Dashboard + API                   │ │
│  │ *.*.example.com    → Reverse proxy to process          │ │
│  │ /metrics           → Prometheus format                 │ │
│  │ /api/instances     → Management API                    │ │
│  │ /api/logs/stream   → SSE log stream                    │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  Hypervisor                                                  │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ api:prod    → /tmp/tenement/api-prod.sock             │ │
│  │ api:staging → /tmp/tenement/api-staging.sock          │ │
│  │ app:user123 → /tmp/tenement/app-user123.sock          │ │
│  │                                                        │ │
│  │ • Captures stdout/stderr → SQLite                      │ │
│  │ • Health checks → auto-restart                         │ │
│  │ • Metrics collection                                   │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  SQLite (tenement.db)                                       │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ logs      - process logs with FTS5 search             │ │
│  │ metrics   - historical metrics (optional)             │ │
│  │ config    - auth tokens, settings                     │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Routes

### Dashboard (root domain)
```
GET  /                    → Dashboard SPA
GET  /api/instances       → List instances
GET  /api/instances/:id   → Instance details
POST /api/instances/:id/restart
POST /api/instances/:id/stop
GET  /api/logs            → Query logs
GET  /api/logs/stream     → SSE stream
GET  /metrics             → Prometheus format
GET  /health              → Server health
```

### App Traffic (subdomains)
```
{id}.{process}.domain/* → proxy to process:id unix socket
```

## Metrics

**In-memory (real-time):**
- `tenement_requests_total{process, id, status}`
- `tenement_request_duration_ms{process, id}`
- `tenement_instance_up{process, id}`
- `tenement_instance_restarts{process, id}`

**Prometheus endpoint:**
```
GET /metrics
```

**Optional:** Flush to SQLite every minute for historical queries.

## Logs

**Capture:**
- stdout/stderr from each process
- Store in SQLite with FTS5 for search

**Schema:**
```sql
CREATE TABLE logs (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    level TEXT NOT NULL,      -- info, warn, error
    process TEXT NOT NULL,    -- process name
    instance_id TEXT NOT NULL,-- instance id
    message TEXT NOT NULL
);

CREATE VIRTUAL TABLE logs_fts USING fts5(message, content='logs', content_rowid='id');
```

**API:**
```
GET /api/logs?process=api&id=prod&search=error&limit=100
GET /api/logs/stream?process=api&id=prod  (SSE)
```

## CLI Commands

```bash
# Server
tenement serve                    # Start server
tenement serve --port 8080        # Custom port
tenement serve --domain example.com

# Process management
tenement spawn api --id prod      # Spawn instance
tenement stop api:prod            # Stop instance
tenement restart api:prod         # Restart instance
tenement ps                       # List instances
tenement logs api:prod            # Tail logs
tenement logs api:prod --search "error"

# Auth
tenement token-gen                # Generate API token

# Config
tenement config                   # Show config
```

## Config File (tenement.toml)

```toml
[settings]
domain = "example.com"
port = 8080
data_dir = "/var/lib/tenement"

[process.api]
command = "node server.js"
directory = "/app/api"
socket = "{data_dir}/{process}-{id}.sock"
env = { NODE_ENV = "production" }
health = "http://localhost:{port}/health"
instances = 1  # default instances per spawn

[process.web]
command = "python -m uvicorn main:app --uds {socket}"
directory = "/app/web"
```

## Implementation Phases

### Phase 1: HTTP Server + Routing ✅
- Add axum to tenement/cli
- Subdomain parsing → process:id lookup
- Reverse proxy to unix sockets
- Basic health endpoint

**Tests:**
- [x] `test_parse_subdomain` - valid patterns (id.process.domain)
- [x] `test_parse_subdomain` - invalid patterns (no subdomain, single subdomain, wrong domain)
- [x] Integration: `test_health_endpoint` - GET /health returns 200 + JSON
- [x] Integration: `test_instances_endpoint_empty` - GET /api/instances returns JSON array
- [x] Integration: `test_dashboard_endpoint` - GET / returns dashboard text
- [x] Integration: `test_unknown_subdomain_returns_404` - unknown subdomain returns 404

### Phase 2: Log Capture ✅
- Capture stdout/stderr from spawned processes
- Broadcast channel for real-time streaming
- Store in memory buffer (ring buffer, last N lines per instance)

**Tests:**
- [x] Unit: `LogBuffer::push` adds entries
- [x] Unit: `LogBuffer::query` filters by process/id
- [x] Unit: `LogBuffer::query` respects limit
- [x] Unit: Ring buffer evicts old entries when full
- [x] Integration: spawned process stdout captured (hypervisor spawns capture tasks)
- [x] Integration: spawned process stderr captured (hypervisor spawns capture tasks)
- [x] Integration: `GET /api/logs?process=x&id=y` returns logs
- [x] Integration: `GET /api/logs/stream` SSE sends new logs

### Phase 3: Metrics ✅
- Request counter middleware
- Instance health metrics
- Prometheus /metrics endpoint

**Tests:**
- [x] Unit: `Metrics::inc` increments counter
- [x] Unit: `Metrics::observe` records histogram value
- [x] Unit: `Metrics::format_prometheus` outputs valid format
- [x] Integration: `GET /metrics` returns Prometheus format
- [ ] Integration: request increments `tenement_requests_total` (middleware not yet added)
- [x] Integration: `tenement_instance_up` reflects running instances
- [x] Integration: `tenement_instance_restarts` increments on restart

### Phase 4: SQLite Storage ✅
- Persist logs to SQLite with FTS5
- Config storage (auth tokens, settings)
- Log rotation (delete old entries)
- 250ms batch flush for efficiency

**Tests:**
- [x] Unit: `LogStore::insert` writes to SQLite
- [x] Unit: `LogStore::query` with FTS5 search
- [x] Unit: `LogStore::rotate` deletes old entries
- [x] Unit: `ConfigStore::get/set` roundtrip
- [ ] Integration: logs persist across restart (not wired into hypervisor yet)
- [x] Integration: FTS search finds matching logs

### Phase 5: Dashboard ✅
- Simple Svelte 5 SPA with Tailwind
- Instance list, logs viewer, metrics
- Embedded via rust-embed

**Tests:**
- [x] Unit: `Assets::get("index.html")` returns content
- [x] Unit: `Assets::get` returns JS files
- [x] Unit: `Assets::get` returns CSS files
- [x] Integration: `GET /` returns HTML dashboard
- [ ] E2E: Dashboard loads in browser
- [ ] E2E: Instance list shows running processes
- [ ] E2E: Log viewer streams new entries

### Phase 6: Auth ✅
- Bearer token for API
- `ten token-gen` command
- Argon2 hashing for secure token storage

**Tests:**
- [x] Unit: `generate_token` creates valid token
- [x] Unit: `hash_and_verify` validates correct token
- [x] Unit: `verify_token` rejects wrong token
- [x] Unit: `verify_token` handles invalid hash format
- [x] Unit: `TokenStore` roundtrip (generate, verify, clear)
- [ ] Integration: API without token returns 401 (middleware not yet wired)
- [ ] Integration: API with valid token returns 200 (middleware not yet wired)
- [x] CLI: `ten token-gen` generates and stores token

### Phase 7: Slum Integration ✅
- Add slum crate for fleet orchestration
- Server and tenant management (SQLite)
- Domain-based routing to tenement servers
- Aggregated metrics endpoint

**Tests:**
- [x] Unit: `SlumDB` server CRUD operations
- [x] Unit: `SlumDB` tenant CRUD operations
- [x] Unit: subdomain → tenant → server routing
- [x] Integration: Server CRUD API endpoints
- [x] Integration: Tenant CRUD API endpoints
- [x] Integration: Aggregated metrics endpoint
- [ ] E2E: slum proxies to tenement server (requires running servers)

## File Structure

```
tenement/
├── Cargo.toml              # Workspace
├── tenement/               # Core library
│   └── src/
│       ├── lib.rs
│       ├── config.rs       # existing
│       ├── hypervisor.rs   # existing
│       ├── instance.rs     # existing
│       ├── logs.rs         # NEW: log capture + storage
│       └── metrics.rs      # NEW: metrics collection
├── cli/                    # Binary
│   └── src/
│       ├── main.rs         # CLI + serve command
│       ├── server.rs       # NEW: axum server
│       ├── proxy.rs        # NEW: reverse proxy
│       └── dashboard.rs    # NEW: embedded SPA
├── slum/                   # Fleet orchestration (optional)
│   └── src/
│       ├── lib.rs
│       ├── db.rs
│       ├── api.rs
│       └── proxy.rs
└── dashboard/              # Svelte SPA
    ├── package.json
    └── src/
```

## Dependencies to Add

```toml
# cli/Cargo.toml
axum = { version = "0.7", features = ["macros"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["trace", "cors"] }
hyper = { version = "1", features = ["client", "http1"] }
hyper-util = { version = "0.1", features = ["tokio", "client-legacy"] }
sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite"] }
rust-embed = { version = "8", features = ["compression"] }
mime_guess = "2"
tokio-stream = "0.1"
```

## What This Replaces

| Before | After |
|--------|-------|
| nginx + config files | tenement routing |
| uWSGI emperor | tenement hypervisor |
| Prometheus + Grafana | tenement /metrics + dashboard |
| Loki/journald | tenement SQLite logs |
| Multiple services | Single binary |

## Standard Interfaces (Play Nice)

- `/metrics` → Prometheus can scrape
- SQLite logs → Easy to export/query
- SSE streams → Any client can consume
- JSON API → Standard REST

Users can use tenement standalone OR plug it into existing Prometheus/Grafana if they prefer.
