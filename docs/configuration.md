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

[process.api]
command = "./my-api"
args = ["--verbose"]
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "on-failure"
workdir = "/opt/myapp"

[process.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
SOCKET_PATH = "{socket}"
LOG_LEVEL = "info"

[process.worker]
command = "./worker"
socket = "/tmp/worker-{id}.sock"
restart = "always"

[routing]
default = "api"

[routing.subdomain]
"api.example.com" = "api"
"*.example.com" = "api"

[routing.path]
"/api" = "api"
"/worker" = "worker"
```

## Settings

Global settings that apply to all processes.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `data_dir` | path | `/var/lib/tenement` | Base directory for instance data |
| `health_check_interval` | u64 | `10` | Seconds between health checks |
| `max_restarts` | u32 | `3` | Maximum restart attempts within window |
| `restart_window` | u64 | `300` | Window in seconds for restart limit |

### Example

```toml
[settings]
data_dir = "/data/myapp"
health_check_interval = 30
max_restarts = 5
restart_window = 600
```

## Process Configuration

Define process templates under `[process.<name>]`.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `command` | string | **Yes** | Command to run |
| `args` | array | No | Arguments to pass |
| `socket` | string | No | Socket path pattern (default: `/tmp/{name}-{id}.sock`) |
| `health` | string | No | Health check endpoint (e.g., `/health`) |
| `env` | table | No | Environment variables |
| `workdir` | path | No | Working directory |
| `restart` | string | No | Restart policy: `always`, `on-failure`, `never` |

### Command

The executable to run. Supports variable interpolation.

```toml
[process.api]
command = "./api-server"

[process.python-app]
command = "python"
args = ["-m", "myapp.server"]
```

### Socket

Unix socket path pattern. Defaults to `/tmp/{name}-{id}.sock`.

```toml
[process.api]
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
[process.api]
health = "/health"
```

### Restart Policy

| Policy | Behavior |
|--------|----------|
| `always` | Always restart when process exits |
| `on-failure` | Restart only on non-zero exit code (default) |
| `never` | Never restart |

```toml
[process.api]
restart = "always"

[process.oneshot]
restart = "never"
```

### Environment Variables

Define environment variables in `[process.<name>.env]`:

```toml
[process.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
SOCKET_PATH = "{socket}"
LOG_LEVEL = "info"
API_KEY = "secret"
```

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
[process.api]
command = "./api"
socket = "/tmp/{name}-{id}.sock"

[process.api.env]
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
[process.myapp]
command = "./myapp"
```

This uses all defaults:
- Socket: `/tmp/myapp-{id}.sock`
- Data dir: `/var/lib/tenement`
- Health check: disabled
- Restart: `on-failure`
