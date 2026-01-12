---
title: Configuration Reference
description: Complete tenement.toml configuration options
---

## Settings (Global)

```toml
[settings]
data_dir = "/var/lib/tenement"           # Base dir for instance data
health_check_interval = 10                # Seconds between health checks
max_restarts = 3                          # Max restarts in window
restart_window = 300                      # Window in seconds (5 minutes)
backoff_base_ms = 1000                    # Exponential backoff base (1s)
backoff_max_ms = 60000                    # Max backoff (60s)
```

## Service Configuration

```toml
[service.api]
command = "./api"                         # Command to run (required)
args = ["--flag=value"]                   # Optional arguments
socket = "/tmp/api-{id}.sock"             # Socket path pattern
health = "/health"                        # Health endpoint
workdir = "/path/to/workdir"              # Working directory
restart = "on-failure"                    # always, on-failure, never
isolation = "namespace"                   # process, namespace, sandbox
startup_timeout = 10                      # Max seconds to create socket
idle_timeout = 300                        # Auto-stop after N seconds idle
memory_limit_mb = 256                     # Memory limit (cgroups v2)
cpu_shares = 100                          # CPU weight (1-10000)

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"
```

## Variable Interpolation

These variables are replaced in `command`, `args`, `socket`, and `env`:

| Variable | Description |
|----------|---|
| `{name}` | Service name (from config key) |
| `{id}` | Instance ID (from spawn command) |
| `{data_dir}` | Settings data directory |
| `{socket}` | Computed socket path |

### Example

```toml
[service.worker]
command = "python {name}-worker.py"
socket = "/tmp/{name}-{id}.sock"

[service.worker.env]
INSTANCE_ID = "{id}"
DATA_DIR = "{data_dir}/{name}/{id}"
```

Spawning `tenement spawn worker --id job123` expands to:

```
command: python worker-worker.py
socket: /tmp/worker-job123.sock
env:
  INSTANCE_ID: job123
  DATA_DIR: /var/lib/tenement/worker/job123
```

## Restart Policies

- `always` - Restart regardless of exit code
- `on-failure` - Restart only if process exits with non-zero code (default)
- `never` - Never restart

With exponential backoff: 1s → 2s → 4s → ... → 60s max.

## Isolation Levels

### Process (No Isolation)
```toml
[service.debug]
command = "./app"
isolation = "process"  # Same process group, no isolation
```

Bare process execution. For debugging only.

### Namespace (Default)
```toml
[service.api]
command = "./app"
isolation = "namespace"  # /proc isolated, zero overhead
```

Uses Linux namespaces (PID + Mount). Each process sees its own `/proc`. Zero memory overhead, zero startup overhead. Requires Linux.

### Sandbox (gVisor)
```toml
[service.untrusted]
command = "./third-party"
isolation = "sandbox"  # Syscall filtering via gVisor
```

Uses gVisor (runsc) to filter syscalls. ~20MB memory overhead, <100ms startup. Perfect for untrusted code. Requires `--features sandbox` and gVisor installed.

## Resource Limits (cgroups v2)

Linux only. Uses cgroups v2 for memory and CPU limits.

```toml
[service.worker]
command = "./worker"
memory_limit_mb = 512    # Hard memory limit in MB
cpu_shares = 500         # CPU weight (1-10000)
```

- `memory_limit_mb`: Hard limit. Process OOMKilled if exceeded.
- `cpu_shares`: Relative weight (default 100). Higher = more CPU time.

Works with all isolation levels.

## Hibernation (Idle Timeout)

```toml
[service.api]
command = "./api"
idle_timeout = 300  # Auto-stop after 5 mins of no requests
```

After `idle_timeout` seconds with no requests, the instance is stopped. Automatically starts on next request. Zero cost when sleeping.

## Health Checks

```toml
[service.api]
health = "/health"                        # Health endpoint
health_check_interval = 10                # Check every 10s (global setting)
```

Sends `GET {health} HTTP/1.1` to the Unix socket. Expects `200 OK`.

On repeated failures (respecting `max_restarts` / `restart_window`), the instance is restarted.

## Complete Example

```toml
[settings]
data_dir = "/var/lib/myapp"
health_check_interval = 10
max_restarts = 3
restart_window = 300
backoff_base_ms = 1000
backoff_max_ms = 60000

# Multi-tenant API
[service.api]
command = "uv run python app.py"
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "on-failure"
isolation = "namespace"
idle_timeout = 300
memory_limit_mb = 512
cpu_shares = 100

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"

# Untrusted user code
[service.sandbox]
command = "./user-plugin"
socket = "/tmp/sandbox-{id}.sock"
health = "/health"
isolation = "sandbox"        # gVisor isolation
memory_limit_mb = 128        # Constrained
cpu_shares = 50              # Limited CPU

# Background worker
[service.worker]
command = "deno run worker.ts"
socket = "/tmp/worker-{id}.sock"
restart = "always"           # Always restart
isolation = "process"        # No isolation needed for trusted code

[service.worker.env]
INSTANCE_ID = "{id}"
JOB_DIR = "{data_dir}/{name}/{id}"
```

## Full Option Reference

| Option | Type | Required | Default | Description |
|--------|------|----------|---------|---|
| `command` | string | Yes | — | Command to execute |
| `args` | array | No | [] | Arguments to pass |
| `socket` | string | No | `/tmp/{name}-{id}.sock` | Socket path pattern |
| `health` | string | No | — | Health check endpoint |
| `workdir` | string | No | cwd | Working directory |
| `restart` | string | No | `on-failure` | Restart policy |
| `isolation` | string | No | `namespace` | Isolation level |
| `startup_timeout` | number | No | 10 | Startup timeout (seconds) |
| `idle_timeout` | number | No | — | Auto-stop timeout (seconds) |
| `memory_limit_mb` | number | No | — | Memory limit (MB) |
| `cpu_shares` | number | No | 100 | CPU weight (1-10000) |
