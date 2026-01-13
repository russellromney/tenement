# Multi-Environment Example

This example demonstrates deploying 4 apps in different languages across 2 environments (prod/staging), each with different isolation levels.

## Apps

| App | Language | Isolation | Description |
|-----|----------|-----------|-------------|
| api | Python | namespace | REST API with /proc isolation |
| web | Node.js | process | Web frontend, no isolation (trusted) |
| worker | Go | sandbox | Background worker with gVisor syscall filtering |
| cache | Rust | namespace | Cache service with resource limits |

## Isolation Levels

| Level | Overhead | /proc Isolated | Syscalls Filtered | Use Case |
|-------|----------|----------------|-------------------|----------|
| `process` | ~0 | No | No | Same trust boundary, fastest |
| `namespace` | ~0 | Yes | No | **Default** - trusted code |
| `sandbox` | ~20MB | Yes | Yes | Untrusted/multi-tenant code |

## Prerequisites

```bash
# Python 3.x (for api)
python3 --version

# Node.js (for web)
node --version

# Go (for worker) - compile first
cd apps/go && go build -o go-worker && cd ../..

# Rust (for cache) - compile first
cd apps/rust && cargo build --release && cd ../..

# For sandbox isolation, install gVisor
# https://gvisor.dev/docs/user_guide/install/
```

## Quick Start

```bash
# 1. Build the compiled apps
cd apps/go && go build -o go-worker && cd ../..
cd apps/rust && cargo build --release && cd ../..

# 2. Start tenement
ten serve --port 8080 --domain localhost

# 3. Test the apps (in another terminal)
curl http://prod.api.localhost:8080/
curl http://staging.api.localhost:8080/
curl http://web.localhost:8080/           # Weighted routing across prod+staging
curl http://prod.worker.localhost:8080/
curl http://cache.localhost:8080/          # Weighted routing
```

## Deployment Workflows

### Blue/Green Deployment

Deploy a new version with zero downtime:

```bash
# 1. Deploy new version (weight 0 = no traffic yet)
ten deploy api --version v2 --weight 0

# 2. Test the new version directly
curl http://v2.api.localhost:8080/health

# 3. Atomic traffic swap
ten route api --from prod --to v2

# 4. Cleanup old version
ten stop api:prod
```

### Canary Deployment

Gradually roll out a new version:

```bash
# 1. Deploy new version with 10% traffic
ten deploy api --version v2 --weight 10

# 2. Monitor metrics, increase traffic
ten weight api:v2 25
ten weight api:v2 50
ten weight api:v2 100

# 3. Remove old version from rotation
ten weight api:prod 0

# 4. Cleanup
ten stop api:prod
```

## Routing

### Direct Routing

Access a specific instance: `{instance}.{service}.{domain}`

```bash
curl http://prod.api.localhost:8080/      # api:prod
curl http://staging.api.localhost:8080/   # api:staging
curl http://v2.api.localhost:8080/        # api:v2
```

### Weighted Routing

Access any instance via weighted selection: `{service}.{domain}`

```bash
curl http://api.localhost:8080/    # Routes to prod OR staging based on weight
curl http://web.localhost:8080/    # Routes to prod OR staging based on weight
```

## Commands

```bash
# List running instances
ten ps

# Check health
ten health api:prod

# View logs
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/logs

# Set traffic weight
ten weight api:staging 50

# Stop an instance
ten stop api:staging

# Restart an instance
ten restart api:prod
```

## Environment Variables

Each app receives:

| Variable | Description |
|----------|-------------|
| `SOCKET_PATH` | Unix socket path to listen on |
| `APP_ENV` | Environment (prod/staging) |
| `APP_VERSION` | App version from config |

## Directory Structure

```
multi-env/
├── tenement.toml          # Main config
├── README.md              # This file
├── data/                  # Runtime data (auto-created)
│   ├── api/
│   │   ├── prod/
│   │   └── staging/
│   └── ...
└── apps/
    ├── python/
    │   └── server.py      # Python HTTP server
    ├── node/
    │   └── server.js      # Node.js HTTP server
    ├── go/
    │   ├── go.mod
    │   ├── main.go
    │   └── go-worker      # Compiled binary
    └── rust/
        ├── Cargo.toml
        ├── src/main.rs
        └── target/release/rust-cache  # Compiled binary
```
