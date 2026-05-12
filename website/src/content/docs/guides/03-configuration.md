---
title: Configuration Reference
description: Complete tenement.toml configuration options
---

All tenement configuration lives in a single `tenement.toml` file.

## Minimal example

```toml
[service.api]
command = "python3 app.py"
health = "/health"
isolation = "process"
```

That's it. tenement auto-allocates a port and sets `PORT` in the environment.

## Settings

Global configuration for the tenement server.

```toml
[settings]
data_dir = "/var/lib/tenement"      # Base data directory
health_check_interval = 10          # Seconds between health checks
max_restarts = 3                    # Max restarts within window
restart_window = 300                # Restart window (seconds)
backoff_base_ms = 1000              # Exponential backoff base (1s)
backoff_max_ms = 60000              # Max backoff delay (60s)
```

The `data_dir` serves double duty: tenement stores its own state here (DB, tokens, certs), and also creates per-instance directories at `{data_dir}/{process}/{id}/`.

## Services

Define services that tenement can spawn. Each service is a template for instances.

```toml
[service.api]
command = "uv run python app.py"    # Shell-split automatically
health = "/health"                  # HTTP endpoint for health checks
isolation = "process"               # process (macOS/Linux) or namespace (Linux)
idle_timeout = 300                  # Stop after N seconds idle (0 = never)
startup_timeout = 10                # Seconds to wait for first health check
storage_persist = true              # Keep data dir on stop
restart = "on-failure"              # always, on-failure, never

# Resource limits (Linux cgroups v2)
memory_limit_mb = 256
cpu_shares = 100
storage_quota_mb = 100
```

### Command parsing

The `command` field is shell-split automatically when no `args` field is provided:

```toml
# These are equivalent:
command = "uv run python app.py"

command = "uv"
args = ["run", "python", "app.py"]
```

Shell quoting works: `command = 'my-app --name "hello world"'` splits correctly.

For commands that compile before serving (like `go run`), increase `startup_timeout`:

```toml
[service.goapi]
command = "go run main.go"
startup_timeout = 30
```

### Isolation levels

| Value | Platform | Overhead | Use case |
|-------|----------|----------|----------|
| `process` | macOS + Linux | ~0 | Development, trusted code |
| `namespace` | Linux only | ~0 | **Production default.** PID + mount isolation |
| `sandbox` | Linux only | ~20MB | Untrusted/third-party code (gVisor) |
| `microvm` | Linux KVM | TBD | Future libkrun runtime with guest kernel boundary |

### Health checks

When a `health` endpoint is configured, tenement sends HTTP GET requests to verify the instance is running:

- **TCP-based instances** (process/namespace/sandbox): health checks go to `http://127.0.0.1:{port}{health}` over TCP
- **Socket-based instances** (future microVM runtime): health checks go over the runtime socket

Health status progression: healthy -> degraded (1-2 failures) -> unhealthy (3+ failures, triggers restart) -> failed (exceeded max_restarts).

If no `health` endpoint is configured, tenement checks whether the socket file exists.

### Process groups

Instances are spawned in their own process group. When you stop or kill an instance, all of its child processes are also killed. This prevents orphaned processes from commands like `go run` or `uv run` that spawn subprocesses.

## Environment variables

Per-service environment variables with template support.

```toml
[service.api.env]
DATA_DIR = "{data_dir}/{id}"
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"
```

### Template variables

| Variable | Description | Example value |
|----------|-------------|---------------|
| `{name}` | Service name | `api` |
| `{id}` | Instance ID | `alice` |
| `{data_dir}` | Global data directory from settings | `/var/lib/tenement` |
| `{port}` | Auto-allocated TCP port | `30001` |
| `{socket}` | Resolved socket path | `/tmp/tenement/api-alice.sock` |

### Auto-set variables

tenement always sets these for every instance:

- `PORT` - TCP port allocated for the instance (30000-40000 range)
- `SOCKET_PATH` - Unix socket path

Your app should read `PORT` and listen on `127.0.0.1:{PORT}`.

## Auto-spawn instances

Start instances automatically when the server starts:

```toml
[instances]
api = ["prod", "staging"]
worker = ["default"]
```

## Routing

Default routing works by subdomain:

| URL pattern | Routes to |
|-------------|-----------|
| `alice.api.example.com` | Instance `api:alice` |
| `api.example.com` | Weighted across all `api` instances |
| `example.com` | Dashboard |

Custom routing overrides:

```toml
[routing]
default = "api"                     # Root domain -> api service

[routing.subdomain]
"admin" = "admin-service"           # admin.example.com -> admin-service

[routing.path]
"/api" = "api-service"              # example.com/api/* -> api-service
```

## TLS

Automatic HTTPS with Let's Encrypt:

```toml
[settings.tls]
enabled = true
acme_email = "admin@example.com"
domain = "example.com"
```

Or via CLI:

```bash
ten serve --tls --domain example.com --email admin@example.com
```

For wildcard certs (required for subdomain routing over HTTPS), use Caddy as a reverse proxy. See [Production Deployment](/guides/04-production).

## CLI environment

Set `TENEMENT_SERVER` to avoid passing `--server` on every command:

```bash
export TENEMENT_SERVER=http://localhost:9090
ten ps              # no --server needed
ten spawn api:alice
ten logs api:alice
```

## Complete example

```toml
[settings]
data_dir = "/var/lib/myapp"
health_check_interval = 10

[service.api]
command = "uv run python app.py"
health = "/health"
isolation = "namespace"
idle_timeout = 300
memory_limit_mb = 256
storage_persist = true

[service.api.env]
DATA_DIR = "{data_dir}/{id}"
DATABASE_PATH = "{data_dir}/{id}/app.db"

[service.worker]
command = "./worker"
health = "/health"
isolation = "namespace"
memory_limit_mb = 512

[instances]
api = ["prod", "staging"]
worker = ["default"]
```

See [examples/](https://github.com/russellromney/tenement/tree/main/examples) for complete working setups in Python, Node.js, Go, and multi-runtime configurations.

## Next steps

- [Production Deployment](/guides/04-production) - TLS, systemd, and Caddy
- [Deployment Patterns](/guides/05-deployments) - Blue-green and canary
