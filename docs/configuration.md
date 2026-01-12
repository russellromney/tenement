# Configuration Reference

Complete reference for `tenement.toml` configuration.

## Config File Location

tenement looks for `tenement.toml` in:
1. Current directory
2. Parent directories (walks up until found)

## Full Example

```toml
[settings]
data_dir = "/var/lib/tenement"
health_check_interval = 10
max_restarts = 3
restart_window = 300
backoff_base_ms = 1000
backoff_max_ms = 60000

[service.api]
command = "./my-api"
args = ["--verbose"]
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "on-failure"
workdir = "/opt/myapp"
isolation = "namespace"      # Isolation level
startup_timeout = 10         # Seconds to wait for socket
idle_timeout = 300           # Auto-stop after idle (0 = never)
memory_limit_mb = 256        # Memory limit (cgroups v2)
cpu_shares = 100             # CPU weight 1-10000 (cgroups v2)

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
SOCKET_PATH = "{socket}"
LOG_LEVEL = "info"

[service.worker]
command = "./worker"
socket = "/tmp/worker-{id}.sock"
restart = "always"
isolation = "sandbox"        # gVisor for untrusted code
memory_limit_mb = 128

[routing]
default = "api"

[routing.subdomain]
"api.example.com" = "api"
"*.example.com" = "api"

[routing.path]
"/api" = "api"
"/worker" = "worker"
```

> **Note:** Both `[service.X]` (preferred) and `[process.X]` (legacy) section names are supported.

## Settings

Global settings that apply to all services.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `data_dir` | path | `/var/lib/tenement` | Base directory for instance data |
| `health_check_interval` | u64 | `10` | Seconds between health checks |
| `max_restarts` | u32 | `3` | Maximum restart attempts within window |
| `restart_window` | u64 | `300` | Window in seconds for restart limit |
| `backoff_base_ms` | u64 | `1000` | Base delay for exponential backoff (ms) |
| `backoff_max_ms` | u64 | `60000` | Maximum backoff delay (ms) |

### Example

```toml
[settings]
data_dir = "/data/myapp"
health_check_interval = 30
max_restarts = 5
restart_window = 600
backoff_base_ms = 2000
backoff_max_ms = 120000
```

## Service Configuration

Define service templates under `[service.<name>]` (or `[process.<name>]` for legacy support).

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `command` | string | **Yes** | - | Command to run |
| `args` | array | No | `[]` | Arguments to pass |
| `socket` | string | No | `/tmp/{name}-{id}.sock` | Socket path pattern |
| `health` | string | No | - | Health check endpoint (e.g., `/health`) |
| `env` | table | No | `{}` | Environment variables |
| `workdir` | path | No | - | Working directory |
| `restart` | string | No | `on-failure` | Restart policy: `always`, `on-failure`, `never` |
| `isolation` | string | No | `namespace` | Isolation level: `process`, `namespace`, `sandbox` |
| `startup_timeout` | u64 | No | `10` | Seconds to wait for socket creation |
| `idle_timeout` | u64 | No | - | Auto-stop after N seconds idle (0 = never) |
| `memory_limit_mb` | u32 | No | - | Memory limit in MB (cgroups v2) |
| `cpu_shares` | u32 | No | - | CPU weight 1-10000 (cgroups v2, default 100) |

### Command

The executable to run. Supports variable interpolation.

```toml
[service.api]
command = "./api-server"

[service.python-app]
command = "python"
args = ["-m", "myapp.server"]
```

### Socket

Unix socket path pattern. Defaults to `/tmp/{name}-{id}.sock`.

```toml
[service.api]
socket = "/var/run/myapp/{name}-{id}.sock"
```

### Health Check

HTTP endpoint for health checks. tenement connects via Unix socket and sends:

```
GET {health} HTTP/1.1
Host: localhost
```

A `200 OK` response indicates healthy status.

```toml
[service.api]
health = "/health"
```

### Restart Policy

| Policy | Behavior |
|--------|----------|
| `always` | Always restart when process exits |
| `on-failure` | Restart only on non-zero exit code (default) |
| `never` | Never restart |

```toml
[service.api]
restart = "always"

[service.oneshot]
restart = "never"
```

### Isolation Level

Control how services are isolated from each other.

| Level | Tool | Overhead | Startup | Use Case |
|-------|------|----------|---------|----------|
| `process` | bare process | ~0 | <10ms | Same trust boundary, debugging |
| `namespace` | Linux unshare | ~0 | <10ms | **Default** - trusted code, /proc isolated |
| `sandbox` | gVisor (runsc) | ~20MB | <100ms | Untrusted/multi-tenant code |

```toml
# Default: namespace isolation
[service.api]
command = "./api"
# isolation = "namespace" (implicit)

# No isolation (same trust boundary)
[service.debug]
command = "./debug"
isolation = "process"

# gVisor sandbox (syscall filtering)
[service.untrusted]
command = "./third-party"
isolation = "sandbox"
```

**Notes:**
- `namespace` requires Linux (fails loudly on other platforms)
- `sandbox` requires gVisor (runsc) installed and `--features sandbox` compile flag

### Resource Limits

Apply memory and CPU limits via cgroups v2 (Linux only).

```toml
[service.api]
command = "./api"
memory_limit_mb = 256    # Hard memory limit in MB
cpu_shares = 500         # CPU weight (1-10000, default 100)
```

| Field | Range | Description |
|-------|-------|-------------|
| `memory_limit_mb` | 1+ | Hard memory limit. Process killed if exceeded. |
| `cpu_shares` | 1-10000 | Relative CPU priority. 100 = normal, higher = more CPU time. |

Resource limits work with all isolation levels (process, namespace, sandbox).

**Notes:**
- Requires Linux with cgroups v2 enabled (kernel 4.5+)
- On non-Linux, resource limits are silently ignored
- Cgroup created at `/sys/fs/cgroup/tenement/{instance_id}/`

### Environment Variables

Define environment variables in `[service.<name>.env]`:

```toml
[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
SOCKET_PATH = "{socket}"
LOG_LEVEL = "info"
API_KEY = "secret"
```

### Hibernation

Auto-stop idle instances to save resources. They wake automatically on first request.

```toml
[service.api]
command = "./api"
idle_timeout = 300       # Stop after 5 minutes idle
startup_timeout = 10     # Max 10 seconds to create socket on wake
```

- `idle_timeout = 0` means never auto-stop
- Health checks do NOT count as activity
- Only real requests reset the idle timer

## Variable Interpolation

These variables are replaced in `command`, `args`, `socket`, and `env` values:

| Variable | Description | Example |
|----------|-------------|---------|
| `{name}` | Process name from config | `api` |
| `{id}` | Instance ID from spawn | `user123` |
| `{data_dir}` | Settings data directory | `/var/lib/tenement` |
| `{socket}` | Computed socket path | `/tmp/api-user123.sock` |

### Example

```toml
[service.api]
command = "./api"
socket = "/tmp/{name}-{id}.sock"

[service.api.env]
DB = "{data_dir}/{name}/{id}/app.db"
SOCK = "{socket}"
```

When spawned with `tenement spawn api --id user123`:
- `{name}` -> `api`
- `{id}` -> `user123`
- `{data_dir}` -> `/var/lib/tenement`
- `{socket}` -> `/tmp/api-user123.sock`
- `DB` -> `/var/lib/tenement/api/user123/app.db`

## Routing

Optional routing rules for use with a reverse proxy.

### Default Process

```toml
[routing]
default = "api"
```

### Subdomain Routing

```toml
[routing.subdomain]
"api.example.com" = "api"
"admin.example.com" = "admin"
"*.example.com" = "api"  # Wildcard match
```

### Path Routing

```toml
[routing.path]
"/api" = "api"
"/admin" = "admin"
"/" = "web"
```

## Data Directory Structure

tenement creates the following structure:

```
{data_dir}/
  {process_name}/
    {instance_id}/
      app.db       # Your app's data
      ...          # Other instance files
```

Example with defaults:

```
/var/lib/tenement/
  api/
    user123/
      app.db
    user456/
      app.db
  worker/
    job1/
```

## Minimal Config

The simplest possible config:

```toml
[service.myapp]
command = "./myapp"
```

This uses all defaults:
- Socket: `/tmp/myapp-{id}.sock`
- Data dir: `/var/lib/tenement`
- Isolation: `namespace`
- Health check: disabled
- Restart: `on-failure`
- Resource limits: unlimited
