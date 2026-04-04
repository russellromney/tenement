---
title: tenement
description: Process hypervisor for single-server deployments
---

tenement is a process hypervisor for running multi-tenant services on a single server. It spawns one process per tenant, routes requests by subdomain, runs HTTP health checks, and stops idle instances automatically. When the next request arrives, it wakes them back up in under a second.

You write your app as if it serves one customer. tenement runs a copy for each of them.

```
alice.notes.example.com  ->  notes:alice  ->  isolated process + own database
bob.notes.example.com    ->  notes:bob    ->  isolated process + own database
```

## Why not just use systemd?

systemd runs processes, but it doesn't route requests or stop idle ones. You'd write a unit file for each customer and wire up nginx yourself. If you want to add a new customer, that's a unit file, an nginx config block, and a reload. If you have 200 customers and only 10 are active at any time, systemd keeps all 200 processes running.

tenement is [Fly Machines](https://fly.io/docs/machines/) on your own hardware. Spawn a process with one command, give it a subdomain automatically, let it sleep when nobody's using it, wake it up on the next request.

| | systemd | tenement |
|---|---------|----------|
| Routing | You configure nginx per service | `alice.notes.example.com` just works |
| Scale to zero | Processes run forever | Idle processes stop, wake on first request |
| Per-tenant data | You manage it | Each instance gets its own data directory |
| New customer | Write a unit file, reload | `ten spawn notes:alice` |
| Health + restart | Basic restart-on-failure | HTTP health checks, exponential backoff |
| Deployment | Rolling restart scripts | `ten deploy notes:v2` then `ten route --from v1 --to v2` |
| Logs | journalctl | `ten logs notes:alice` with full-text search |

## Get started

```bash
cargo install tenement-cli
```

The [Quick Start](/intro/01-quick-start) walks through a complete example, from writing an app to spawning tenants to watching them scale to zero.

If you'd rather read code, the [examples](https://github.com/russellromney/tenement/tree/main/examples) directory has working setups in Python, Node.js, and Go that you can run immediately. The [multi-runtime example](https://github.com/russellromney/tenement/tree/main/examples/multi-runtime) runs all three at once and includes a 56-test integration script.

## What's in the box

tenement does subdomain routing, scale-to-zero with wake-on-request, per-tenant data directories, process isolation via Linux namespaces, HTTP health checks with exponential backoff, weighted routing for blue-green and canary deployments, built-in TLS via Let's Encrypt, Prometheus metrics, log capture with full-text search, and bearer token auth for the management API.

It doesn't touch your app's auth. All request headers, including `Authorization`, pass through to your process untouched. Your app handles authentication however it wants.

## Docs

- [Quick Start](/intro/01-quick-start) walks through writing an app, configuring tenement, and spawning your first tenants.
- [Why tenement?](/intro/02-economics) explains the problem in more detail and the economics of running mostly-idle tenants.
- [Concepts](/intro/03-concepts) covers how tenement works internally: the request flow, instance lifecycle, health checks, and the auth model.
- [Configuration](/guides/03-configuration) is the full TOML reference.
- [Production](/guides/04-production) covers TLS, systemd, and Caddy for real deployments.
- [Deployment Patterns](/guides/05-deployments) covers blue-green swaps and canary rollouts.
- [Troubleshooting](/reference/troubleshooting) has solutions for common issues.
