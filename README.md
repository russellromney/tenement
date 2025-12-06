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

### 3. Manage instances

```bash
$ tenement ps
INSTANCE             SOCKET                         UPTIME     HEALTH
api:user123          /tmp/api-user123.sock          2m         healthy

$ tenement stop api:user123
Stopped api:user123
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

## Documentation

Full documentation at [tenement.dev](https://tenement.dev)

## License

Apache 2.0 - Use it however you want.
