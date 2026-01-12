---
title: Getting Started
description: Complete guide to set up tenement
---

## Installation

### Option 1: Install Script

```bash
curl -LsSf https://tenement.dev/install.sh | sh
```

### Option 2: Cargo

```bash
cargo install tenement-cli
```

### Option 3: From Source

```bash
git clone https://github.com/yourusername/tenement
cd tenement
cargo install --path cli
```

## Create Your Process

tenement manages any process that:
1. Listens on a Unix socket (path via `SOCKET_PATH` env var)
2. Optionally exposes a health endpoint

### Example: Rust with actix-web

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

### Example: Node.js with Express

```javascript
const express = require('express');
const fs = require('fs');
const app = express();

app.get('/health', (req, res) => res.send('ok'));
app.get('/', (req, res) => res.send('Hello!'));

const socketPath = process.env.SOCKET_PATH;
if (fs.existsSync(socketPath)) fs.unlinkSync(socketPath);

app.listen(socketPath, () => {
  console.log(`Listening on ${socketPath}`);
});
```

## Create Config File

Create `tenement.toml` in your project root:

```toml
[settings]
data_dir = "/var/lib/myapp"
health_check_interval = 10

[service.api]
command = "./target/release/my-api"
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "on-failure"
isolation = "namespace"     # process, namespace, or sandbox
idle_timeout = 300          # Auto-stop after 5 mins (optional)

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"

# Optional: Resource limits (cgroups v2, Linux only)
memory_limit_mb = 256
cpu_shares = 100
```

## Spawn an Instance

```bash
$ tenement spawn api --id prod
Spawned api:prod
Socket: /tmp/api-prod.sock
```

What happens:
1. Reads the `api` process config from `tenement.toml`
2. Interpolates `{id}` → `prod`
3. Creates data directory: `/var/lib/myapp/api/prod/`
4. Spawns `./target/release/my-api` with env vars
5. Waits for Unix socket to appear
6. Starts health checks

## Manage Instances

```bash
# List all instances
$ tenement ps
INSTANCE             SOCKET                      UPTIME     HEALTH
api:prod             /tmp/api-prod.sock          2m         healthy

# Check health
$ tenement health api:prod
api:prod: healthy

# Restart
$ tenement restart api:prod
Restarted api:prod

# Stop
$ tenement stop api:prod
Stopped api:prod

# View config
$ tenement config
```

## Route Traffic

### Using nginx

```nginx
upstream api_prod {
    server unix:/tmp/api-prod.sock;
}

server {
    listen 80;
    server_name prod.api.myapp.com;

    location / {
        proxy_pass http://api_prod;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

### Using Caddy

```caddy
prod.api.myapp.com {
    reverse_proxy unix//tmp/api-prod.sock
}
```

### Custom Reverse Proxy

Use the tenement library in your own proxy:

```rust
use tenement::Hypervisor;

let hypervisor = Hypervisor::load("tenement.toml")?;
let instance = hypervisor.get_instance("api:prod")?;
// Route to instance.socket_path
```

## What Happens Under the Hood

### On spawn:
1. Create instance data directory
2. Create cgroup with resource limits (if configured)
3. Spawn process with isolation level
4. Add process to cgroup
5. Wait for socket to appear
6. Track instance in memory

### On health check:
1. Connect to Unix socket
2. Send `GET {health} HTTP/1.1`
3. Check for `200 OK` response
4. Update health status
5. Restart if unhealthy (respecting restart policy)

### On stop:
1. Send SIGKILL to process
2. Remove cgroup
3. Remove socket file
4. Remove from tracking

## Next Steps

- [Configuration Reference](/guides/configuration) - All options explained
- [Isolation Levels](/guides/isolation) - Security models
- [Use Cases](/use-cases/multitenant) - Real-world examples
