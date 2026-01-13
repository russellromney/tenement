---
title: Quick Start
description: Get tenement running in 5 minutes
---

## Install

```bash
cargo install tenement-cli
```

This installs the `ten` command. Verify:

```bash
$ ten --version
ten 0.1.0
```

## 1. Create a Config

Create `tenement.toml`:

```toml
[settings]
data_dir = "/var/lib/myapp"

[service.api]
command = "./my-api"
socket = "/tmp/api-{id}.sock"
health = "/health"
idle_timeout = 300        # Auto-stop after 5 mins
restart = "on-failure"    # Restart on crashes

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"
```

## 2. Spawn an Instance

```bash
$ ten spawn api --id user123
Spawned api:user123
Socket: /tmp/api-user123.sock
```

Your process is running and listening on `/tmp/api-user123.sock`.

## 3. List Instances

```bash
$ ten ps
INSTANCE             SOCKET                      UPTIME     HEALTH
api:user123          /tmp/api-user123.sock       2m         healthy
```

## 4. Route Traffic

Point a reverse proxy to the socket:

```nginx
server {
    listen 80;
    server_name user123.myapp.com;

    location / {
        proxy_pass http://unix:/tmp/api-user123.sock;
    }
}
```

## 5. Stop When Done

```bash
$ ten stop api:user123
Stopped api:user123
```

Or it stops automatically after `idle_timeout` (5 minutes in this example).

## Next Steps

- [Configuration Reference](/guides/03-configuration) - Full TOML config options
- [Production Deployment](/guides/04-production) - TLS, systemd, and Caddy setup
- [Use Cases](/use-cases/01-multitenant) - Real-world examples
