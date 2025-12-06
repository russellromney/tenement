# tenement

**Hyperlightweight process hypervisor for single-server deployments.**

tenement spawns and supervises processes with Unix socket communication, health checks, and automatic restarts. No Docker, no Kubernetes, no complexity—just fast, simple process management.

## Installation

```bash
curl -LsSf https://tenement.dev/install.sh | sh
```

Or with pip/uv:

```bash
pip install tenement
# or
uv tool install tenement
```

Or with Cargo:

```bash
cargo install tenement
```

## Why tenement?

| Alternative | Problem |
|-------------|---------|
| Docker | Heavy, slow cold starts, network overhead |
| systemd | No on-demand spawn, no routing |
| K8s/Nomad | Overkill for single server |
| Bash scripts | No health checks, no supervision |

tenement gives you:

- **Sub-second cold starts** - Rust binaries + Unix sockets = instant
- **On-demand spawn** - Processes start when first requested
- **Auto-restart** - Health checks with automatic recovery
- **Zero overhead** - Direct Unix socket IPC, no network layer
- **Simple config** - One TOML file defines everything

## Quick Start

### 1. Create a config file

```toml
# tenement.toml
[process.api]
command = "./my-api"
socket = "/tmp/api-{id}.sock"
health = "/health"

[process.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
```

### 2. Spawn an instance

```bash
$ tenement spawn api --id user123
Spawned api:user123
Socket: /tmp/api-user123.sock
```

### 3. List running instances

```bash
$ tenement ps
INSTANCE             SOCKET                         UPTIME     HEALTH
api:user123          /tmp/api-user123.sock          2m         healthy
api:user456          /tmp/api-user456.sock          5m         healthy
```

### 4. Stop when done

```bash
$ tenement stop api:user123
Stopped api:user123
```

## How It Works

```
tenement spawn api --id user123
    │
    ├── Read process config from tenement.toml
    │
    ├── Interpolate variables: {id}, {data_dir}, {socket}
    │
    ├── Spawn process with env vars
    │   └── Process listens on Unix socket
    │
    └── Wait for socket ready (~10ms)
```

tenement manages **process templates** (defined in config) and **instances** (spawned at runtime). Each instance gets its own:

- Unix socket for communication
- Data directory for state
- Health monitoring

## CLI Reference

```bash
tenement spawn <process> --id <id>  # Spawn new instance
tenement stop <process>:<id>        # Stop instance
tenement restart <process>:<id>     # Restart instance
tenement ps                         # List instances
tenement health <process>:<id>      # Check health
tenement config                     # Show config
```

## Use Cases

### Multi-tenant apps
Spawn a process per user, route requests by subdomain:
```bash
tenement spawn api --id alice   # alice.myapp.com
tenement spawn api --id bob     # bob.myapp.com
```

### Dev environments
Fast iteration with instant restarts:
```bash
tenement restart api:dev
```

### Microservices on a VPS
Run multiple services without container overhead:
```toml
[process.api]
command = "./api"

[process.worker]
command = "./worker"

[process.web]
command = "./web"
```

## Next Steps

- [Getting Started](getting-started.md) - Detailed setup guide
- [Configuration](configuration.md) - Full config reference

## License

Apache 2.0 - Use it however you want.
