---
title: Why tenement?
description: The problem tenement solves
---

## The Gap

I built tenement because I wanted Fly Machines capabilities on my own server. Spawn processes on demand, stop them when idle, route by subdomain. Without Kubernetes.

Existing options:
- **systemd** - No routing, no idle timeout, managing 100+ unit files is painful
- **Docker** - Heavy (~100MB per container), slow startup, overkill for trusted code
- **Kubernetes** - Control plane overhead exceeds the workloads on a small server
- **Fly Machines** - Great product, but you pay per machine. Can't overstuff.

tenement fills the gap: lightweight process management with routing, for a single server.

## The Use Case

**Run many isolated services, mostly idle, on one machine.**

```
1000 services configured
├── 20 actually running (active users)
├── 980 sleeping (zero resources)
├── Wake on request: user.api.example.com → spawn → proxy
└── Stop after idle: 5 min no requests → kill
```

This works because most SaaS customers aren't active simultaneously. You configure all of them, but only pay for what's running.

## Who This Is For

You're building multi-tenant software. You want:

- **Process isolation** without container overhead
- **Subdomain routing** without nginx config sprawl
- **Scale-to-zero** without paying per-machine
- **Simple deployment** - one server, one config file

You're running **trusted code** - your own apps, not arbitrary user code. (For untrusted code, use `isolation = "sandbox"`.)

## Single-Tenant Code, Multi-Tenant Deployment

Write your app as if it serves one customer. No tenant ID checks, no row-level security, no shared database complexity.

```python
# app.py - single-tenant code
db_path = os.getenv("DATABASE_PATH")  # /data/customer123/app.db
```

```toml
# tenement.toml
[service.api]
command = "python app.py"

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
```

Each customer gets their own process, their own database file, their own environment. tenement handles the multiplexing.

## Pairs Well With

**SQLite + WAL replication** (Litestream, LiteFS, etc.)

Each customer gets their own SQLite database. Replicate to S3 for durability. No shared PostgreSQL, no connection pooling complexity. Your app stays simple.

```
customer1.api.example.com → api:customer1 → /data/customer1/app.db
customer2.api.example.com → api:customer2 → /data/customer2/app.db
```

## What tenement Is Not

- **Not a container runtime** - No images, no registries. Pre-install dependencies on the host.
- **Not multi-server** - Use [slum](https://github.com/russellromney/tenement) for fleet orchestration.
- **Not for untrusted code** - Use `sandbox` isolation if you need syscall filtering.
- **Not Kubernetes** - Single server, single config file, that's it.
