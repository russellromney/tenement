---
title: Why tenement?
description: The problem tenement solves
---

## The gap

You have a side project you want to offer as SaaS. You want each customer to get their own process and database. You don't want to deal with multi-tenant database schemas.

Your options:

- **systemd** - No routing, no idle timeout, managing 100+ unit files is painful
- **Docker** - Container overhead, slower startup, overkill for trusted code on one server
- **Kubernetes** - Control plane overhead exceeds the workloads on a small server
- **Fly Machines** - Great, but you pay per machine and can't overstuff

tenement fills the gap: lightweight process management with routing, on a single server.

## Why not systemd?

| | systemd | tenement |
|---|---------|----------|
| Routing | You configure nginx/caddy per service | `alice.notes.example.com` just works |
| Scale to zero | No. Processes run forever | Idle processes stop, wake on first request |
| Per-tenant data | You manage it | Each instance gets `{data_dir}/{id}/` automatically |
| New tenant | Write a unit file, reload | `ten spawn notes:alice` |
| Health + restart | Basic restart-on-failure | HTTP health endpoint checks, exponential backoff, max restart limits |
| Deployment | Rolling restart scripts | `ten deploy notes:v2` + `ten route --from v1 --to v2` |
| Metrics | You set up prometheus exporters | Built-in per-tenant request counts and latencies |
| Logs | journalctl | `ten logs notes:alice`, full-text search, SSE streaming |
| Auth | N/A | Bearer token API with admin + tenant-scoped tokens |
| Process cleanup | Your problem | Process groups: kill an instance, kill all its children |

systemd is a process manager. tenement is a tenant manager.

## The economics

Most SaaS customers aren't active simultaneously. You configure all of them, but only pay for what's running.

```
1000 tenants configured
  20 actually running (active users right now)
 980 sleeping (zero resources)
```

Traditional approach: 1000 always-on processes = 20GB RAM = 10 machines @ $500/month.

tenement approach: 1000 configured, ~20 running = 400MB RAM = 1 machine @ $5/month.

Wake-on-request latency: sub-second. Users don't notice.

## Single-tenant code, multi-tenant deployment

Write your app as if it serves one customer. No tenant ID checks, no row-level security, no shared database complexity.

```python
# app.py
db_path = os.environ["DATA_DIR"] + "/app.db"
# That's it. No tenant_id, no filtering, no RLS.
```

```toml
# tenement.toml
[service.api]
command = "python3 app.py"
health = "/health"

[service.api.env]
DATA_DIR = "{data_dir}/{id}"
```

Each customer gets their own process, their own database, their own environment. tenement handles the multiplexing.

## Auth passthrough

tenement does not touch your app's auth. Requests proxy through with all headers intact (including `Authorization`). Your app handles authentication however it wants: JWT, sessions, API keys.

tenement's own auth (Bearer tokens) is only for the management API (spawn, stop, deploy). See the [auth-test example](https://github.com/russellromney/tenement/tree/main/examples/auth-test) for a working demo.

## Pairs well with

**SQLite + WAL replication** (walrust, Litestream, LiteFS, etc.)

Each customer gets their own SQLite database. Replicate to S3 for durability. No shared PostgreSQL, no connection pooling.

```
customer1.api.example.com -> api:customer1 -> /data/customer1/app.db
customer2.api.example.com -> api:customer2 -> /data/customer2/app.db
```

## What tenement is not

- **Not a container runtime.** No images, no registries. Dependencies live on the host.
- **Not multi-server.** Use slum for fleet orchestration.
- **Not for untrusted code.** Use `isolation = "sandbox"` (gVisor) if you need syscall filtering.
- **Not Kubernetes.** Single server, single config file.
