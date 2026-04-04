---
title: Concepts
description: Understanding tenement architecture and terminology
---

## How it works

1. You define a **service** in `tenement.toml` (command, health endpoint, env vars)
2. You **spawn instances** of that service (`ten spawn notes:alice`)
3. tenement allocates a TCP port, sets `PORT` env var, starts the process
4. Requests to `alice.notes.example.com` route to that instance
5. Health checks run automatically; unhealthy instances restart with backoff
6. Idle instances stop after the configured timeout, wake on the next request

Your app handles its own auth, business logic, and data. tenement handles routing, lifecycle, and isolation.

## Architecture

```
                        Internet
                            |
                            v
                    +---------------+
                    |   tenement    |
                    |   (server)    |
                    |   :8080       |
                    +-------+-------+
                            |
            +---------------+---------------+
            |               |               |
            v               v               v
      +----------+    +----------+    +----------+
      | api:prod |    | api:stg  |    | web:prod |
      | :30001   |    | :30002   |    | :30003   |
      +----------+    +----------+    +----------+
```

**Request flow:**

1. Request arrives at `prod.api.example.com:8080`
2. tenement parses subdomain: `{id}.{service}.{domain}` -> `api:prod`
3. Request proxied to instance's TCP port (30001)
4. Instance processes request and returns response
5. All headers pass through (including Authorization)

## Terminology

| Term | What it is | Example |
|------|------------|---------|
| **Service** | A template in `[service.X]` config | `[service.api]` defines the "api" service |
| **Instance** | A running copy of a service with a unique ID | `api:prod`, `api:alice`, `api:customer123` |
| **Spawn** | Start a new instance | `ten spawn api:alice` |
| **Health check** | HTTP GET tenement sends to verify the instance | `GET /health` returns 200 |
| **Isolation** | Separation level between instances | `process`, `namespace`, `sandbox` |
| **Weight** | Traffic percentage (0-100) for canary/blue-green | `ten weight api:prod 80` |

## Instance lifecycle

```
                    ten spawn
                        |
                        v
+---------+        +---------+        +---------+
| stopped |------->| starting|------->| running |
+---------+        +---------+        +----+----+
     ^                                     |
     |         +-------------+             |
     |         |  unhealthy  |<------------+ health check fails
     |         +------+------+             |
     |                | max_restarts       |
     |                v                    |
     |         +-------------+             |
     +---------+   failed    |             |
               +-------------+             |
                                           |
     +--------------------+----------------+
     |                    |
     v                    v
+---------+        +---------+
| idle    |------->| stopped |
| timeout |        |         |
+---------+        +---------+
```

## Routing

| URL pattern | Routes to |
|-------------|-----------|
| `alice.api.example.com` | Instance `api:alice` (direct) |
| `api.example.com` | Weighted across all `api` instances |
| `example.com` | Dashboard |

## Health checks

tenement checks instance health via HTTP:

- **TCP health checks** for process/namespace/sandbox runtimes: sends `GET /health` to `127.0.0.1:{port}`
- **Socket health checks** for VM runtimes: sends through Unix socket

Status progression: **healthy** -> **degraded** (1-2 failures) -> **unhealthy** (3+, triggers restart) -> **failed** (exceeded max_restarts).

## Process groups

Every instance is spawned in its own process group. When tenement kills an instance, it kills the entire group, including any child processes. This means commands like `go run` or `uv run` (which spawn subprocesses) clean up correctly.

## Auth model

tenement has two layers of auth:

1. **Management API auth** (tenement's concern): Bearer tokens protect spawn/stop/deploy/logs endpoints. Admin tokens have full access; tenant-scoped tokens can only access their own instance.

2. **App auth** (your concern): tenement proxies all headers through unchanged. Your app handles its own authentication however it wants.

These are completely independent. See the [auth-test example](https://github.com/russellromney/tenement/tree/main/examples/auth-test).

## When to use tenement

**Good fit:**
- Multi-tenant SaaS on one server
- Scale-to-zero without per-machine pricing
- Side projects you want to offer as SaaS
- Development environments needing isolation

**Not a good fit:**
- Multi-server deployments (use Kubernetes, Nomad, Fly.io)
- Container ecosystem (need Docker images, OCI registries)
- Serverless functions (use Lambda, CloudFlare Workers)
- High-availability (single server = single point of failure)

## Next steps

- [Configuration](/guides/03-configuration) - Full TOML reference
- [Isolation Levels](/guides/01-isolation) - namespace vs sandbox vs process
- [Production](/guides/04-production) - TLS and systemd
- [Examples](https://github.com/russellromney/tenement/tree/main/examples) - Working setups in Python, Node, Go
