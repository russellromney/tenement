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
args = ["--port", "8000"]           # Optional command arguments
workdir = "/app"                    # Optional working directory
socket = "/tmp/tenement/{name}-{id}.sock"  # Socket path template (default)
health = "/health"                  # Health check endpoint
startup_timeout = 10                # Max seconds to create socket
idle_timeout = 300                  # Auto-stop after N seconds idle
restart = "on-failure"              # Restart policy
isolation = "namespace"             # Isolation level (alias: runtime)

# Resource limits (cgroups v2, Linux only)
memory_limit_mb = 256               # Memory limit in MB
cpu_shares = 100                    # CPU weight (1-10000)

# Storage quotas
storage_quota_mb = 100              # Max storage per instance (MB)
storage_persist = false             # Persist data on stop (default: false)
```

### Service Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `command` | string | required | Shell command to start the service |
| `args` | array | `[]` | Optional command arguments |
| `workdir` | string | none | Working directory for the process |
| `socket` | string | `/tmp/tenement/{name}-{id}.sock` | Unix socket path template |
| `health` | string | none | HTTP path for health checks |
| `startup_timeout` | int | `10` | Max seconds to wait for socket |
| `idle_timeout` | int | `0` | Stop after N seconds idle (0 = never) |
| `restart` | string | `on-failure` | Restart policy: `always`, `on-failure`, `never` |
| `isolation` | string | `namespace` | Isolation level (alias: `runtime`) |
| `memory_limit_mb` | int | none | Memory limit in MB (Linux cgroups v2) |
| `cpu_shares` | int | none | CPU weight 1-10000 (Linux cgroups v2) |
| `storage_quota_mb` | int | none | Max disk usage per instance (MB) |
| `storage_persist` | bool | `false` | Keep data directory on instance stop |

### Isolation Levels

| Value | Description | Overhead | Use Case |
|-------|-------------|----------|----------|
| `process` | No isolation | ~0 | Debugging, trusted code |
| `namespace` | PID + Mount namespace | ~0 | **Default** - multi-tenant trusted code |
| `sandbox` | gVisor syscall filtering | ~20MB | Untrusted/third-party code |
| `firecracker` | Firecracker microVM | ~128MB | Full VM isolation (requires KVM) |
| `qemu` | QEMU VM | ~128MB | Cross-platform VM (requires KVM/HVF) |

**Note:** The `runtime` field is accepted as an alias for `isolation` for backwards compatibility.

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
- `{name}` - Service name (e.g., "api")
- `{id}` - Instance ID (e.g., "prod")
- `{data_dir}` - The `data_dir` from settings
- `{socket}` - The resolved socket path
- `{port}` - Auto-allocated TCP port (30000-40000 range)
- `${VAR}` - Value from host environment

**Auto-set environment variables:**
- `PORT` - TCP port allocated for the instance
- `SOCKET_PATH` - Unix socket path for the instance

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

## Routing Section

Configure custom routing rules beyond the default subdomain-based routing.

```toml
[routing]
default = "api"                     # Default service for root domain

[routing.subdomain]
"admin" = "admin-service"           # admin.example.com → admin-service

[routing.path]
"/api" = "api-service"              # example.com/api/* → api-service
"/docs" = "docs-service"            # example.com/docs/* → docs-service
```

| Field | Type | Description |
|-------|------|-------------|
| `default` | string | Service to route root domain requests to |
| `subdomain` | map | Subdomain → service name mappings |
| `path` | map | Path prefix → service name mappings |

**Default routing (without config):**
- `{id}.{service}.{domain}` → routes to specific instance
- `{service}.{domain}` → weighted routing across all instances of service
- `{domain}` → dashboard

## TLS Section

Configure automatic HTTPS with Let's Encrypt certificates.

```toml
[settings.tls]
enabled = true
acme_email = "admin@example.com"    # Required for Let's Encrypt
domain = "example.com"
cache_dir = "/var/lib/tenement/acme"  # Certificate cache (default: {data_dir}/acme)
staging = false                     # Use staging for testing (avoids rate limits)
https_port = 443
http_port = 80
dns_provider = "cloudflare"         # For wildcard certs via Caddy
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Enable TLS |
| `acme_email` | string | none | Email for Let's Encrypt registration |
| `domain` | string | none | Domain for TLS certificate |
| `cache_dir` | string | `{data_dir}/acme` | Directory for certificate cache |
| `staging` | bool | `false` | Use Let's Encrypt staging environment |
| `https_port` | int | `443` | HTTPS listening port |
| `http_port` | int | `80` | HTTP port (redirects + ACME challenges) |
| `dns_provider` | string | none | DNS provider for Caddy wildcard certs |

**CLI flags take precedence:**
```bash
ten serve --tls --domain example.com --email admin@example.com --staging
```

## Complete Example

```toml
[settings]
data_dir = "/var/lib/myapp"
health_check_interval = 10
max_restarts = 3
restart_window = 300

[settings.tls]
enabled = true
acme_email = "admin@example.com"
domain = "example.com"

[service.api]
command = "uv run python app.py"
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

# Use different config file via environment variable
TENEMENT_CONFIG=custom.toml ten serve
```

## Next Steps

- [Production Deployment](/guides/04-production) - TLS, systemd, and Caddy setup
- [Deployments](/guides/05-deployments) - Blue-green and canary patterns
