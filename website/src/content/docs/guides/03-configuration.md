---
title: Configuration Reference
description: Complete tenement.toml configuration options
---

All tenement configuration lives in a single `tenement.toml` file.

## Settings Section

Global configuration for the tenement server.

```toml
[settings]
data_dir = "/var/lib/tenement"      # Base data directory
health_check_interval = 10          # Health check interval (seconds)
max_restarts = 3                    # Max restarts within window
restart_window = 300                # Restart window (seconds)
backoff_base_ms = 1000              # Exponential backoff base (1s)
backoff_max_ms = 60000              # Max backoff delay (60s)
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `data_dir` | string | `/var/lib/tenement` | Base directory for instance data |
| `health_check_interval` | int | `10` | Seconds between health checks |
| `max_restarts` | int | `3` | Max restarts before giving up |
| `restart_window` | int | `300` | Window for counting restarts (seconds) |
| `backoff_base_ms` | int | `1000` | Initial backoff delay (ms) |
| `backoff_max_ms` | int | `60000` | Maximum backoff delay (ms) |

## Service Section

Define services that tenement can spawn. Each service is a template for instances.

```toml
[service.api]
command = "uv run python app.py"    # Command to run
socket = "/tmp/tenement/api-{id}.sock"  # Socket path template
health = "/health"                  # Health check endpoint
startup_timeout = 10                # Max seconds to create socket
idle_timeout = 300                  # Auto-stop after N seconds idle
restart = "on-failure"              # Restart policy
isolation = "namespace"             # Isolation level

# Resource limits (cgroups v2, Linux only)
memory_limit_mb = 256               # Memory limit in MB
cpu_shares = 100                    # CPU weight (1-10000)

# Storage quotas
storage_quota_mb = 100              # Max storage per instance (MB)
storage_persist = true              # Persist data on stop
```

### Service Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `command` | string | required | Shell command to start the service |
| `socket` | string | required | Unix socket path template (`{id}` is replaced) |
| `health` | string | `/` | HTTP path for health checks |
| `startup_timeout` | int | `30` | Max seconds to wait for socket |
| `idle_timeout` | int | `0` | Stop after N seconds idle (0 = never) |
| `restart` | string | `on-failure` | Restart policy: `always`, `on-failure`, `never` |
| `isolation` | string | `namespace` | Isolation level (see below) |
| `memory_limit_mb` | int | none | Memory limit in MB (Linux cgroups v2) |
| `cpu_shares` | int | `100` | CPU weight 1-10000 (Linux cgroups v2) |
| `storage_quota_mb` | int | none | Max disk usage per instance (MB) |
| `storage_persist` | bool | `true` | Keep data directory on instance stop |

### Isolation Levels

| Value | Description | Overhead | Use Case |
|-------|-------------|----------|----------|
| `process` | No isolation | ~0 | Debugging, trusted code |
| `namespace` | PID + Mount namespace | ~0 | **Default** - multi-tenant trusted code |
| `sandbox` | gVisor syscall filtering | ~20MB | Untrusted/third-party code |

### Restart Policies

| Value | Behavior |
|-------|----------|
| `always` | Always restart on exit |
| `on-failure` | Restart only on non-zero exit |
| `never` | Never restart |

## Environment Variables

Per-service environment variables with template support.

```toml
[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"
SECRET_KEY = "${SECRET_KEY}"        # From environment
```

**Template variables:**
- `{id}` - Instance ID
- `{data_dir}` - The `data_dir` from settings
- `${VAR}` - Value from host environment

## Instances Section

Auto-spawn instances on `ten serve` startup.

```toml
[instances]
api = ["prod", "staging"]           # Spawn api:prod and api:staging
worker = ["default"]                # Spawn worker:default
```

Each key is a service name, value is an array of instance IDs to spawn.

**Behavior:**
- Validates that referenced services exist at config load time
- Individual spawn failures are logged but don't block others
- Instances spawn after the server starts listening

## Complete Example

```toml
[settings]
data_dir = "/var/lib/myapp"
health_check_interval = 10
max_restarts = 3
restart_window = 300

[service.api]
command = "uv run python app.py"
socket = "/tmp/tenement/api-{id}.sock"
health = "/health"
idle_timeout = 300
restart = "on-failure"
isolation = "namespace"
memory_limit_mb = 256
cpu_shares = 100
storage_quota_mb = 100

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"

[service.worker]
command = "./worker"
socket = "/tmp/tenement/worker-{id}.sock"
health = "/health"
isolation = "namespace"
memory_limit_mb = 512
cpu_shares = 200

[instances]
api = ["prod", "staging"]
worker = ["default"]
```

## CLI Overrides

Some config values can be overridden via CLI flags:

```bash
ten serve --port 8080               # Override listen port
ten serve --domain example.com      # Set domain for routing
ten serve --tls --email you@x.com   # Enable TLS with ACME
ten serve --config custom.toml      # Use different config file
```

## Next Steps

- [Production Deployment](/guides/04-production) - TLS, systemd, and Caddy setup
- [Deployments](/guides/05-deployments) - Blue-green and canary patterns
