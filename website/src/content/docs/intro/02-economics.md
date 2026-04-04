---
title: Why tenement?
description: The problem tenement solves
---

## The problem

The simplest multi-tenant architecture is one process and one database per customer. No shared state, no row-level security, no multi-tenant schema migrations. Each customer's data lives in its own SQLite file. The code is trivial to write and trivial to reason about.

The hard part is running 500 copies of it on one server.

systemd can run processes, but it doesn't know about routing. You'd write a unit file for each customer, configure nginx to route subdomains, set up health checks, figure out log aggregation, and script your own deployments. When a customer churns, you clean up all of that manually. If you have 500 customers and only 30 are active at any moment, systemd keeps all 500 processes running anyway.

Docker adds container overhead and image management that you don't need when you're running your own trusted code on your own server. Kubernetes is absurd for a single VPS. Fly Machines is the closest thing to what you want, but you're paying per machine and you can't overstuff them the way you can overstuff your own box.

tenement is a process hypervisor that handles routing, lifecycle, health checks, and scale-to-zero. You write single-tenant code, and tenement handles the multiplexing.

## The economics

Most SaaS customers aren't active simultaneously. If you have 1000 customers, maybe 20 are making requests right now. The other 980 haven't logged in today.

The traditional approach keeps all 1000 processes running. That's roughly 20GB of RAM across 10 machines, costing maybe $500/month. With tenement, the 980 idle customers cost nothing. Their processes are stopped. You're running 20 processes on one $5 VPS, and when customer #21 shows up, their process starts in under a second.

This is the same trick that makes serverless platforms economical, except you're running it on your own hardware. The difference is you keep the margin.

## Single-tenant code

The key insight is that you don't change your code at all. Your app reads a database path from the environment and serves whoever's asking. It has no concept of tenants.

```python
# app.py
db = sqlite3.connect(os.environ["DATA_DIR"] + "/app.db")
```

```toml
# tenement.toml
[service.api]
command = "python3 app.py"
health = "/health"

[service.api.env]
DATA_DIR = "{data_dir}/{id}"
```

When tenement spawns `api:alice`, it sets `DATA_DIR` to `/var/lib/tenement/api/alice` and starts the process. When it spawns `api:bob`, bob gets `/var/lib/tenement/api/bob`. Same code, different environment, complete isolation.

## Auth passthrough

A reasonable question is how authentication works when tenement is proxying requests. The answer is that tenement doesn't touch your auth at all. Every request header, including `Authorization`, passes through to your process unchanged. Your app authenticates users however it normally would: JWT, sessions, API keys, cookies. tenement is invisible to your auth layer.

tenement has its own auth system, but it's only for the management API. The tokens that let you spawn and stop instances are completely separate from whatever your app uses to authenticate its users. You can see this working in the [auth-test example](https://github.com/russellromney/tenement/tree/main/examples/auth-test), which runs a Python API with its own bearer token auth through tenement and verifies that cross-tenant tokens are rejected by the app, not by tenement.

## Pairs well with SQLite

Each customer gets their own database file. Replicate to S3 with something like [walrust](https://github.com/russellromney/walrust) or Litestream for durability. No shared Postgres, no connection pooling, no schema migrations that affect everyone at once.

```
customer1.api.example.com -> api:customer1 -> /data/customer1/app.db
customer2.api.example.com -> api:customer2 -> /data/customer2/app.db
```

When you need to migrate a schema, you can roll it out to one customer at a time. If something breaks, only that customer is affected.

## What tenement is not

tenement is not a container runtime. There are no images and no registries. Your app's dependencies need to be installed on the host. It's not multi-server, though slum (experimental) is a fleet orchestrator that will coordinate multiple tenement instances. It's not for untrusted code by default, though you can use `isolation = "sandbox"` (gVisor) if you need syscall filtering. And it's not Kubernetes. It's one server, one config file, and a CLI.
