# tenement

**Cramped housing for your processes.**

Your apps deserve better than Kubernetes. They don't need a penthouse—just a roof, some supervision, and the occasional health check. tenement packs processes into Unix sockets, watches them like a suspicious landlord, and restarts them when they misbehave.

## The Stack

- **tenement** - The building. Spawns and supervises processes.
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
[process.api]
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

## Runtimes

```toml
[process.api]
command = "uv run python app.py"
runtime = "process"  # default: bare process, Unix socket

[process.untrusted]
command = "./runner"
runtime = "firecracker"  # micro-VM, actual isolation
memory_mb = 512
vcpus = 1
```

Same routing, same supervision, same API. Some tenants get curtains, some get walls.

## Roadmap

- [ ] **Firecracker runtime** - Micro-VMs for tenants who need real isolation. Same API, different walls.
- [ ] **Resource limits** - Cap memory, CPU, storage per tenant. Metered utilities.
- [ ] **Hibernation** - Scale to zero. Wake on first request.
- [ ] **Infra adapters** - Export your slum to Fly.io, AWS, GCP.

## License

Apache 2.0 - Subletting permitted.
