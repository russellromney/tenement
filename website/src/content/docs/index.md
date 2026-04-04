---
title: tenement
description: Process hypervisor for single-server deployments
---

**Process hypervisor for single-server deployments.**

Run 100+ isolated services on a single server. Each customer gets their own process. Spawn on demand, stop when idle, wake on first request.

```
alice.notes.example.com  ->  notes:alice  ->  isolated process + own database
bob.notes.example.com    ->  notes:bob    ->  isolated process + own database
```

Write single-tenant code. Deploy it for every customer.

## Why not systemd?

systemd runs processes. tenement runs *tenants*.

| | systemd | tenement |
|---|---------|----------|
| Routing | You configure nginx per service | `alice.notes.example.com` just works |
| Scale to zero | No | Idle processes stop, wake on first request |
| Per-tenant data | You manage it | Each instance gets its own data dir |
| Spawn tenant | Write a unit file, reload | `ten spawn notes:alice` |
| Health + restart | Basic restart-on-failure | HTTP health checks, exponential backoff |
| Deployment | Rolling restart scripts | `ten deploy` + `ten route` (blue-green) |
| Metrics | Set up exporters | Built-in per-tenant request counts |
| Logs | journalctl | `ten logs notes:alice`, full-text search |

tenement is for when you want [Fly Machines](https://fly.io/docs/machines/) on your own hardware.

## Get started

```bash
cargo install tenement-cli
ten serve --port 8080 --domain localhost
ten token-gen
ten spawn api:prod
curl http://prod.api.localhost:8080/
```

See the [Quick Start](/intro/01-quick-start) for a complete walkthrough, or jump to the [examples](https://github.com/russellromney/tenement/tree/main/examples).

## Features

- **Subdomain routing** - `alice.api.example.com` routes to `api:alice`
- **Scale-to-zero** - idle processes stop, wake on first request (sub-second)
- **Per-tenant data** - each instance gets `{data_dir}/{id}/`
- **Process isolation** - Linux namespaces (zero overhead) or gVisor sandbox
- **Health checks** - HTTP endpoint checks with exponential backoff
- **Process groups** - kill an instance, kill all its children (no orphans)
- **Shell command parsing** - `command = "uv run python app.py"` just works
- **Weighted routing** - blue-green and canary deployments
- **Built-in TLS** - Let's Encrypt certificates
- **Auth** - admin tokens + tenant-scoped tokens
- **Prometheus metrics** - per-tenant request counts and latencies
- **Log capture** - full-text search, SSE streaming, CLI

## Examples

| Example | What it shows |
|---------|---------------|
| [hello-world](https://github.com/russellromney/tenement/tree/main/examples/hello-world) | Simplest setup (bash + netcat) |
| [python-fastapi](https://github.com/russellromney/tenement/tree/main/examples/python-fastapi) | FastAPI with per-tenant database |
| [node-fastify](https://github.com/russellromney/tenement/tree/main/examples/node-fastify) | Node.js Fastify server |
| [go-http](https://github.com/russellromney/tenement/tree/main/examples/go-http) | Go net/http server |
| [multi-runtime](https://github.com/russellromney/tenement/tree/main/examples/multi-runtime) | Python + Node + Go in one config, 56-test integration script |
| [auth-test](https://github.com/russellromney/tenement/tree/main/examples/auth-test) | App-level auth passthrough |
| [multi-tenant](https://github.com/russellromney/tenement/tree/main/examples/multi-tenant) | Per-tenant notes API with SQLite |

## Docs

- [Quick Start](/intro/01-quick-start) - Installation and first spawn
- [Why tenement?](/intro/02-economics) - The problem it solves
- [Concepts](/intro/03-concepts) - Architecture and terminology
- [Configuration](/guides/03-configuration) - Full TOML reference
- [Production](/guides/04-production) - TLS, systemd, Caddy
- [Deployment Patterns](/guides/05-deployments) - Blue-green, canary
- [Troubleshooting](/reference/troubleshooting) - Common issues
