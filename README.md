# tenement

> Early alpha

**Lightweight process hypervisor for single-server deployments.**

Pack 100+ isolated services on a $5 VPS. Each customer gets their own process - spawn on demand, stop when idle, wake on first request.

## Features

- Process supervision with auto-restart
- Subdomain routing (`prod.api.example.com` → `api:prod`)
- Weighted load balancing for canary/blue-green deployments
- Integrated TLS with automatic Let's Encrypt certificates
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
ten serve                                                    # Start server (HTTP)
ten serve --tls --domain example.com --email you@email.com   # HTTPS with Let's Encrypt
ten spawn api --id prod   # Start instance
ten stop api:prod         # Stop instance
ten ps                    # List instances (with weights)
ten weight api:prod 50    # Set traffic weight (0-100)
ten token-gen             # Generate auth token
ten install               # Install as systemd service
ten caddy                 # Generate Caddyfile for HTTPS
```

## Routing

```
{id}.{service}.{domain}  → direct to instance     (prod.api.example.com → api:prod)
{service}.{domain}       → weighted load balance  (api.example.com → all api:* instances)
{domain}                 → dashboard
```

Use `ten weight api:v1 90` to shift traffic for canary deployments.

## API

```
GET /              Dashboard (web UI)
GET /health        Health check
GET /metrics       Prometheus metrics
GET /api/instances Instance list (auth required)
GET /api/logs      Log search (auth required)
```

API endpoints require `Authorization: Bearer <token>` header. Generate tokens with `ten token-gen`.

## Configuration

```toml
# tenement.toml
[settings]
data_dir = "/var/lib/tenement"

[service.api]
command = "uv run python app.py"
socket = "/tmp/tenement/api-{id}.sock"   # {id} = instance ID
health = "/health"
idle_timeout = 300           # Auto-stop after 5 mins idle (0 = never)
restart = "on-failure"       # always | on-failure | never
isolation = "namespace"      # process | namespace | sandbox
memory_limit_mb = 256        # cgroups v2 (Linux)
cpu_shares = 100             # cgroups v2 (Linux)
storage_quota_mb = 100       # Max disk per instance

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"

[instances]
api = ["prod", "staging"]    # Auto-spawn on `ten serve`
```

- **`{id}`** in paths is replaced with instance ID (e.g., `api-prod.sock`)
- **`idle_timeout`** enables scale-to-zero: instances stop when idle, wake on request
- **`[instances]`** spawns listed instances on startup

## Fleet Mode (slum)

`slum` orchestrates tenement across multiple servers - route tenants to specific servers, balance load, geographic distribution.

```rust
use slum::{SlumDb, Server, Tenant};

let db = SlumDb::init("slum.db").await?;
db.add_server(&Server { id: "east".into(), url: "http://east.example.com".into(), ..Default::default() }).await?;
db.add_tenant(&Tenant { domain: "customer.example.com".into(), server_id: "east".into(), process: "api".into(), instance_id: "prod".into(), ..Default::default() }).await?;
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

| Level | Overhead | Use Case |
|-------|----------|----------|
| `process` | ~0 | Debugging, same trust boundary |
| `namespace` | ~0 | **Default** - /proc isolated, env vars hidden |
| `sandbox` | ~20MB | Untrusted code (gVisor syscall filtering) |

```toml
[service.api]
isolation = "namespace"      # Default
memory_limit_mb = 256        # cgroups v2 memory limit
cpu_shares = 100             # cgroups v2 CPU weight
```

Namespace isolation uses Linux PID + Mount namespaces (kernel built-in). Sandbox requires gVisor and `--features sandbox`.

## Development

```bash
cargo test          # 340+ tests
cargo bench         # 8 benchmarks
```

Tests use real processes and TempDir - no mocking.

## Roadmap

See [ROADMAP.md](ROADMAP.md) for details.

**Next:** `ten deploy` / `ten route` commands, slum health checks

## License

Apache 2.0 - Subletting permitted.
