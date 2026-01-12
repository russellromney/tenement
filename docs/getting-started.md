# Getting Started with tenement

This guide walks you through setting up tenement for your project.

## Installation

```bash
curl -LsSf https://tenement.dev/install.sh | sh
```

Or with Cargo:

```bash
cargo install tenement
```

## Step 1: Create Your Process Binary

tenement manages any process that:

1. Listens on a Unix socket (path provided via `SOCKET_PATH` env var)
2. Optionally exposes a health endpoint

Example (Rust with actix-web):

```rust
use actix_web::{web, App, HttpServer};
use std::env;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let socket_path = env::var("SOCKET_PATH")
        .expect("SOCKET_PATH not set");

    HttpServer::new(|| {
        App::new()
            .route("/health", web::get().to(|| async { "ok" }))
            .route("/", web::get().to(|| async { "Hello!" }))
    })
    .bind_uds(&socket_path)?
    .run()
    .await
}
```

## Step 2: Create tenement.toml

Create a config file in your project root:

```toml
[settings]
data_dir = "/var/lib/myapp"
health_check_interval = 10

[service.api]
command = "./my-api"
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "on-failure"
isolation = "namespace"      # process, namespace, or sandbox

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"
```

## Step 3: Spawn an Instance

```bash
$ tenement spawn api --id user123
Spawned api:user123
Socket: /tmp/api-user123.sock
```

tenement:
1. Reads the `api` process config
2. Interpolates `{id}` → `user123`
3. Creates data directory `/var/lib/myapp/api/user123/`
4. Spawns `./my-api` with the configured env vars
5. Waits for the socket to be ready

## Step 4: Manage Instances

```bash
# List running instances
$ tenement ps
INSTANCE             SOCKET                         UPTIME     HEALTH
api:user123          /tmp/api-user123.sock          2m         healthy

# Check health
$ tenement health api:user123
api:user123: healthy

# Restart
$ tenement restart api:user123
Restarted api:user123

# Stop
$ tenement stop api:user123
Stopped api:user123
```

## Step 5: Route Requests

tenement includes built-in subdomain routing. Start the server with a domain:

```bash
$ ten serve --port 8080 --domain myapp.com
```

Requests to `{id}.{service}.myapp.com` route to the corresponding instance:

```
user123.api.myapp.com → api:user123
```

### Production HTTPS with Caddy

For automatic HTTPS with Let's Encrypt, use the built-in Caddy integration:

```bash
# Generate Caddyfile
$ ten caddy --domain myapp.com --port 8080
# Output: Caddyfile with wildcard subdomain routing

# Or install Caddy and enable as systemd service
$ sudo ten caddy --domain myapp.com --install --systemd
```

The generated Caddyfile handles:
- Automatic HTTPS via Let's Encrypt
- Wildcard subdomain routing (`*.myapp.com`)
- HTTP to HTTPS redirect
- Compression (gzip/zstd)

### Alternative: nginx or custom proxy

You can also use nginx or your own proxy:

```nginx
# nginx.conf
upstream api_user123 {
    server unix:/tmp/api-user123.sock;
}

server {
    listen 80;
    server_name user123.api.myapp.com;

    location / {
        proxy_pass http://api_user123;
    }
}
```

## Configuration Reference

### Settings

| Field | Default | Description |
|-------|---------|-------------|
| `data_dir` | `/var/lib/tenement` | Base directory for instance data |
| `health_check_interval` | `10` | Seconds between health checks |
| `max_restarts` | `3` | Max restarts within window |
| `restart_window` | `300` | Window in seconds for restart limit |

### Service Config

| Field | Required | Description |
|-------|----------|-------------|
| `command` | Yes | Command to run |
| `args` | No | Arguments to pass |
| `socket` | No | Socket path pattern (default: `/tmp/{name}-{id}.sock`) |
| `health` | No | Health check endpoint (e.g., `/health`) |
| `env` | No | Environment variables |
| `workdir` | No | Working directory |
| `restart` | No | Policy: `always`, `on-failure`, `never` |
| `isolation` | No | Isolation: `process`, `namespace` (default), `sandbox` |
| `idle_timeout` | No | Auto-stop after N seconds idle |
| `memory_limit_mb` | No | Memory limit in MB (cgroups v2) |
| `cpu_shares` | No | CPU weight 1-10000 (cgroups v2) |

### Variable Interpolation

These variables are replaced in `command`, `args`, `socket`, and `env` values:

| Variable | Description |
|----------|-------------|
| `{name}` | Service name (from config key) |
| `{id}` | Instance ID (from spawn command) |
| `{data_dir}` | Settings data directory |
| `{socket}` | Computed socket path |

## What Happens Under the Hood

**On spawn:**
1. Create instance data directory: `{data_dir}/{name}/{id}/`
2. Remove old socket if exists
3. Create cgroup with resource limits (if configured, Linux only)
4. Spawn process with configured isolation level
5. Add process to cgroup
6. Wait for socket to appear (up to `startup_timeout` seconds)
7. Track instance in memory

**On health check:**
1. Connect to Unix socket
2. Send `GET {health} HTTP/1.1`
3. Check for `200 OK` response
4. Update health status
5. Restart if unhealthy (respecting restart policy)

**On stop:**
1. Send SIGKILL to process
2. Remove cgroup (if created)
3. Remove socket file
4. Remove from tracking

## Next Steps

- [Configuration Reference](configuration.md) - Full config options
