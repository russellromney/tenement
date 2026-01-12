# tenement

**Cramped housing for your processes.**

Your apps don't need a penthouse - a roof, some supervision, and the occasional health check are probably fine. `tenement` packs processes into Unix sockets, watches them like a suspicious landlord, and restarts them when they misbehave.

---

`tenement` (single-server hypervisor) is a lightweight Rust binary that manages processes/VMs on a single server. The speicfic goal is to easily "overstuff" semi-ephemeral processes into a server.

`slum` (multi-server `tenement` orchestrator) is a lightwight Rust binary to horizontally scale `tenement` across a server fleet. 

## The Stack

- **tenement** - The building. Spawns and supervises processes/VMs.
- **slum** - The neighborhood. Fleet orchestration across multiple tenements.
- **tenant** - The occupant. Your app, paying rent in CPU cycles.

## Features

- Process supervision with auto-restart
- Subdomain routing (`prod.api.example.com` → `api:prod`)
- Built-in dashboard (Svelte)
- Prometheus metrics at `/metrics`
- Log capture with full-text search
- Bearer token auth
- Single binary, ~10MB

## Install

```bash
cargo install tenement-cli
```

## Quick Start

```toml
# tenement.toml
[service.api]
command = "uv run python app.py"  # or: bun run server.ts, deno run main.ts, ./binary
socket = "/tmp/tenement/api-{id}.sock"
health = "/health"
```

```bash
ten serve --port 8080 --domain example.com
ten spawn api --id prod
# prod.api.example.com now routes to api:prod
```

Bring your own environment. Use `uv`, `bun`, `deno`, or a compiled binary. No shared runtimes on the host.

## CLI

```bash
ten serve              # Open for business
ten spawn api --id prod # Move in a tenant
ten stop api:prod      # Eviction
ten ps                 # Census
ten token-gen          # New keys
```

## Routing

```
prod.api.example.com   → api:prod
staging.web.example.com → web:staging
example.com            → dashboard (the lobby)
```

## API

```
GET /              Dashboard
GET /health        Building inspection
GET /metrics       Utility bills (Prometheus)
GET /api/instances Tenant registry
GET /api/logs      Complaint box
```

## Configuration

```toml
# tenement.toml
[settings]
health_check_interval = 10   # Health check every 10s
max_restarts = 3             # Max restarts in window
restart_window = 300         # 5 minute restart window
backoff_base_ms = 1000       # Exponential backoff base (1s)
backoff_max_ms = 60000       # Max backoff delay (60s)

[service.api]
command = "uv run python app.py"
socket = "/tmp/tenement/api-{id}.sock"
health = "/health"
startup_timeout = 10         # Max 10s to create socket
idle_timeout = 300           # Auto-stop after 5 mins idle
restart = "on-failure"       # Restart policy: always, on-failure, never
isolation = "namespace"      # Isolation: process, namespace, sandbox

# Resource limits (cgroups v2, Linux only)
memory_limit_mb = 256        # Memory limit in MB
cpu_shares = 100             # CPU weight (1-10000)
```

**Hibernation:** Set `idle_timeout` to auto-stop idle instances. They wake automatically on first request.

**Restarts:** Failed instances restart with exponential backoff (1s → 2s → 4s → ... → 60s max).

**Resource limits:** Set `memory_limit_mb` and `cpu_shares` to constrain instance resources via cgroups v2.

## Fleet Mode (slum)

When one tenement isn't enough:

```rust
use slum::{SlumDb, Server, Tenant};

let db = SlumDb::init("slum.db").await?;

// Add buildings to the slum
db.add_server(&Server {
    id: "east".into(),
    url: "http://east.example.com".into(),
    ..Default::default()
}).await?;

// Assign tenants to buildings
db.add_tenant(&Tenant {
    domain: "customer.example.com".into(),
    server_id: "east".into(),
    process: "api".into(),
    instance_id: "prod".into(),
    ..Default::default()
}).await?;
```

## Why?

| Alternative | Problem |
|-------------|---------|
| piku/dokku | Git push magic. Python overhead. You have CI/CD. |
| Fly Machines | Pay per room. No overstuffing allowed. |
| Docker | Luxury apartments. Slow elevators. |
| systemd | No vacancy sign. No routing. |
| K8s | You don't need a city planner. |
| nginx + uWSGI | Too many landlords. |

tenement: sub-second cold starts, zero network overhead, one config file.

## Isolation Levels

```toml
# Namespace isolation (default) - /proc isolation, zero overhead
[service.api]
command = "uv run python app.py"
# isolation = "namespace" (implicit default)

# Bare process - no isolation, for debugging
[service.debug]
command = "uv run python app.py"
isolation = "process"

# gVisor sandbox - syscall filtering for untrusted code
[service.untrusted]
command = "./third-party"
isolation = "sandbox"

# With resource limits (cgroups v2)
[service.worker]
command = "./worker"
memory_limit_mb = 256    # Memory limit in MB
cpu_shares = 200         # CPU weight (1-10000, default 100)
```

Same routing, same supervision, same API. Some tenants get curtains, some get walls.

### Isolation Spectrum

| Isolation | Tool | Overhead | Startup | Use Case |
|-----------|------|----------|---------|----------|
| `process` | bare | ~0 | <10ms | Same trust boundary, debugging |
| `namespace` | unshare | ~0 | <10ms | **Default** - trusted code, /proc isolated |
| `sandbox` | gVisor | ~20MB | <100ms | Untrusted/multi-tenant code |
| `firecracker` | microVM | ~128MB | ~125ms | Compliance, custom kernel |

**Namespace isolation** (default) uses Linux namespaces (PID + Mount) to give each process its own view of `/proc`. Environment variables are hidden between services. Zero overhead, zero dependencies (kernel built-in since 2008). Requires Linux.

**Sandbox isolation** uses gVisor (runsc) to filter syscalls. ~20MB memory overhead, <100ms startup. Perfect for untrusted third-party code. Requires `--features sandbox` and gVisor installed.

### Resource Limits

Apply memory and CPU limits via cgroups v2 (Linux only):

```toml
[service.api]
command = "./api"
memory_limit_mb = 512    # Hard memory limit
cpu_shares = 500         # CPU weight (higher = more CPU time)
```

Resource limits work with all isolation levels (process, namespace, sandbox).

## Development

### Testing

320+ tests + 8 benchmarks covering all core modules:

```bash
cd tenement && cargo test
# test result: ok. 320+ passed

cargo bench --bench performance
# 8 benchmarks, all passing targets
```

Tests use real processes (`sleep`, `echo`, `env`) and TempDir for file operations—no mocking.

| Module | Tests |
|--------|-------|
| Hypervisor | 32 |
| Instance | 48 |
| Cgroup | 26 |
| Runtime | 25 |
| Logs | 45 |
| Store | 34 |
| Auth | 22 |
| Config | 39 |
| CLI (unit) | 18 |
| Auth Integration | 38 |
| Hypervisor Integration | 10 |
| E2E Lifecycle | 12 |
| Stress Tests | 7 |
| Benchmarks | 8 |

See [TEST_PLAN.md](TEST_PLAN.md) for unit test breakdown.
See [E2E_TESTING_PLAN.md](E2E_TESTING_PLAN.md) for integration test plan.

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the full isolation spectrum vision.

**Done:**
- Hibernation - Scale to zero, wake on request
- Exponential backoff restarts
- Namespace isolation - Zero-overhead `/proc` protection (Linux)
- Sandbox isolation (gVisor) - Syscall filtering for untrusted code
- Resource limits - Memory and CPU limits via cgroups v2
- Comprehensive test suite (320+ tests + 8 benchmarks)
- Unix socket proxy - Full request routing to backends
- Auth middleware - Bearer token authentication on API endpoints
- Foreign key enforcement in slum fleet orchestration
- E2E test infrastructure (Session 1) - shared utilities and fixture scripts
- Auth integration tests (Session 2) - 38 comprehensive auth tests
- Hypervisor integration tests (Session 3) - 10 tests for hypervisor + server + storage
- E2E lifecycle tests (Session 4) - 12 tests for instance lifecycle
- Stress tests (Session 6) - 7 concurrent load tests
- Performance benchmarks (Session 7) - 8 criterion benchmarks, all passing targets
- Race condition fix - Atomic get-and-touch for proxy requests
- Improved logging - Cgroup cleanup, auth failures, CPU weight clamping
- Dashboard caching - Cache-Control headers for static assets

**Next up:**
- Cgroup lifecycle tests (Session 5) - Linux-only cgroup verification
- Slum integration tests (Session 8) - Fleet orchestration tests
- WASM runtime (wasmtime) - Lightweight compute sandbox
- Storage quotas per instance

## License

Apache 2.0 - Subletting permitted.
