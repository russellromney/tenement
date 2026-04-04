---
title: Concepts
description: How tenement works
---

## The request flow

When a request comes in for `alice.notes.example.com`, tenement parses the subdomain into a service name and instance ID: `notes:alice`. If that instance is running, the request gets proxied to its TCP port. If it's stopped (because it was idle), tenement spawns it first, waits for the health check to pass, then proxies the request. The whole wake-from-sleep path takes under a second.

```
                        Internet
                            |
                            v
                    +---------------+
                    |   tenement    |
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

All request headers pass through unchanged, including `Authorization`. tenement is invisible to your app's auth layer.

## Services and instances

A service is a template defined in your config: the command to run, the health endpoint, environment variables. An instance is a running copy of that service with a unique ID. You can run many instances of the same service, each with their own data directory and TCP port.

```toml
[service.notes]            # this is the service template
command = "python3 app.py"
health = "/health"
```

```bash
ten spawn notes:alice      # this creates an instance
ten spawn notes:bob        # another instance of the same service
```

Alice and Bob run the same code but get different `PORT`, `DATA_DIR`, and `SOCKET_PATH` values in their environment. They can't see each other's data.

## Routing

tenement routes by subdomain. The pattern is `{id}.{service}.{domain}` for a specific instance, or `{service}.{domain}` for weighted load balancing across all instances of a service.

| URL | Where it goes |
|-----|---------------|
| `alice.api.example.com` | Instance `api:alice` directly |
| `api.example.com` | Weighted across all `api` instances |
| `example.com` | Dashboard |

Weighted routing is how you do blue-green deployments and canary rollouts. Set `api:v1` to weight 0 and `api:v2` to weight 100, and all traffic flips instantly.

## Health checks

When you configure a `health` endpoint, tenement sends HTTP GET requests to `http://127.0.0.1:{port}{health}` on a regular interval. If the endpoint returns 200, the instance is healthy. After one or two consecutive failures it's degraded. After three or more it's unhealthy and tenement restarts it with exponential backoff. If it exceeds the max restart count within the restart window, it's marked as failed and tenement stops trying.

If you don't configure a health endpoint, tenement falls back to checking whether the Unix socket file exists. This is less reliable but works for simple cases.

## Instance lifecycle

An instance moves through these states:

1. You call `ten spawn notes:alice`. tenement allocates a TCP port, starts the process, and begins health checking.
2. Once the health check passes, the instance is running and receives traffic.
3. If health checks fail repeatedly, tenement restarts the process with exponential backoff.
4. If nobody makes a request for `idle_timeout` seconds, tenement kills the process.
5. The next request to `alice.notes.example.com` wakes it back up, starting from step 1.

The data directory survives across restarts and wake cycles (unless you set `storage_persist = false`). So a SQLite database, for example, is still there when the process comes back.

## Process groups

Every instance runs in its own process group. When tenement kills an instance, it sends SIGKILL to the entire group, not just the parent process. This matters for commands like `go run` or `uv run`, which spawn a child process that does the actual work. Without process groups, killing the parent would leave the child running as an orphan.

## Isolation levels

tenement supports three isolation levels for separating instances from each other. On macOS, only `process` is available. On Linux, `namespace` is the default and recommended option for production.

| Level | What it does | Overhead | When to use it |
|-------|-------------|----------|----------------|
| `process` | No isolation, just a separate process | ~0 | Development, or when you trust all the code |
| `namespace` | PID and mount namespace isolation | ~0 | Production on Linux. Instances can't see each other's processes or mounts. |
| `sandbox` | gVisor syscall filtering | ~20MB per instance | Untrusted or third-party code |

## The auth model

tenement has two completely independent auth layers, and understanding the boundary between them matters.

The first layer is tenement's management API. This is protected by bearer tokens that you generate with `ten token-gen`. Admin tokens can spawn, stop, deploy, and read logs for any instance. Tenant-scoped tokens (generated with `ten token-gen --tenant alice`) can only read logs and check health for their own instance. This auth layer exists so that you can expose the management API safely, or give customers limited access to their own instance.

The second layer is your app's auth. tenement doesn't participate in this at all. When a request arrives at `alice.notes.example.com`, tenement proxies it to alice's process with every header intact. If your app checks for a JWT in the `Authorization` header, that works exactly as it would without tenement.

The [auth-test example](https://github.com/russellromney/tenement/tree/main/examples/auth-test) demonstrates this by running a Python API with its own bearer token auth through tenement. Each tenant's token is rejected by other tenants' processes, and tenement doesn't interfere.

## Next

- [Configuration](/guides/03-configuration) is the full TOML reference with every field documented.
- [Production](/guides/04-production) covers TLS, systemd, and Caddy.
- [Deployment Patterns](/guides/05-deployments) covers blue-green and canary.
- The [examples directory](https://github.com/russellromney/tenement/tree/main/examples) has working setups in Python, Node.js, and Go.
