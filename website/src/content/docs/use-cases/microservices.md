---
title: Microservices on VPS
description: Run multiple services without container overhead
---

Deploy a microservices architecture on a single VPS without Docker or Kubernetes complexity.

## The Architecture

```
        nginx (reverse proxy)
        ↓ ↓ ↓ ↓ ↓
        tenement (supervises all)
        ├── API service
        ├── Worker service
        ├── Webhook service
        ├── Sync service
        └── Background jobs
```

## Configuration

```toml
[settings]
data_dir = "/var/lib/myapp"

[service.api]
command = "./dist/api"
socket = "/tmp/api.sock"
health = "/health"
restart = "on-failure"
isolation = "namespace"
memory_limit_mb = 512
cpu_shares = 200

[service.worker]
command = "./dist/worker"
socket = "/tmp/worker.sock"
health = "/status"
restart = "always"
isolation = "process"      # Trusted code
memory_limit_mb = 256
cpu_shares = 100

[service.webhook]
command = "./dist/webhook"
socket = "/tmp/webhook.sock"
health = "/health"
restart = "on-failure"
isolation = "namespace"
memory_limit_mb = 256
cpu_shares = 100

[service.sync]
command = "deno run sync.ts"
socket = "/tmp/sync.sock"
health = "/health"
restart = "on-failure"
isolation = "sandbox"      # Untrusted/risky third-party code
memory_limit_mb = 128
cpu_shares = 50

[service.jobs]
command = "python jobs.py"
socket = "/tmp/jobs.sock"
health = "/health"
restart = "always"
isolation = "namespace"
memory_limit_mb = 256
cpu_shares = 100
```

## Reverse Proxy Routing

Route different paths to different services:

```nginx
upstream api {
    server unix:/tmp/api.sock;
}

upstream worker {
    server unix:/tmp/worker.sock;
}

upstream webhook {
    server unix:/tmp/webhook.sock;
}

upstream sync {
    server unix:/tmp/sync.sock;
}

server {
    listen 80;
    server_name api.example.com;

    location /api/ {
        proxy_pass http://api;
    }

    location /webhooks/ {
        proxy_pass http://webhook;
    }

    location /sync/ {
        proxy_pass http://sync;
    }

    location /jobs/ {
        proxy_pass http://worker;
    }
}
```

Or use host-based routing:

```nginx
server {
    listen 80;
    server_name api.example.com;
    location / {
        proxy_pass http://api;
    }
}

server {
    listen 80;
    server_name worker.example.com;
    location / {
        proxy_pass http://worker;
    }
}

server {
    listen 80;
    server_name webhook.example.com;
    location / {
        proxy_pass http://webhook;
    }
}
```

## Service Dependencies

Services can communicate via localhost:

```python
# worker/worker.py - calls API service

import socket

def call_api(endpoint):
    # Unix socket to API service
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect("/tmp/api.sock")

    request = f"GET {endpoint} HTTP/1.1\r\nHost: api\r\n\r\n"
    sock.sendall(request.encode())

    response = sock.recv(4096).decode()
    sock.close()
    return response
```

Or use HTTP via localhost:

```python
# services can run on different ports if needed
import requests

# If services expose HTTP on localhost:9000, 9001, etc.
response = requests.get("http://localhost:9000/api/data")
```

## Deployment Workflow

### Build Phase

```bash
#!/bin/bash
set -e

# Build all services
cargo build --release -p api
cargo build --release -p worker
deno bundle webhook.ts dist/webhook.js
deno bundle sync.ts dist/sync.js
python -m pip install -r requirements.txt

# Run tests
cargo test
deno test
pytest tests/
```

### Deploy Phase

```bash
#!/bin/bash
set -e

# Copy binaries
cp dist/* /opt/myapp/

# Reload services (graceful restart)
tenement restart api:default
tenement restart worker:default
tenement restart webhook:default
tenement restart sync:default
tenement restart jobs:default

# Verify health
sleep 2
tenement ps
```

## Monitoring

All services expose metrics:

```bash
# Get Prometheus metrics from all services
curl http://example.com/metrics

# Or integrate with tenement's metrics endpoint
curl http://localhost:8000/metrics | grep tenement_instance
```

Monitor in Grafana:

```promql
# Running instances
count(tenement_instance_uptime_seconds)

# Memory usage per service
tenement_instance_memory_mb by (service)

# Request rate per service
rate(tenement_requests_total[5m]) by (service)

# Error rate
rate(tenement_errors_total[5m]) by (service)
```

## Resource Allocation

Example: 4GB VPS with 5 services

```toml
# Total: 512 + 256 + 256 + 128 + 256 = 1408 MB
# Headroom for OS + overhead: 2GB

[service.api]
memory_limit_mb = 512
cpu_shares = 200        # 20% CPU (shared with others)

[service.worker]
memory_limit_mb = 256
cpu_shares = 100

[service.webhook]
memory_limit_mb = 256
cpu_shares = 100

[service.sync]
memory_limit_mb = 128
cpu_shares = 50

[service.jobs]
memory_limit_mb = 256
cpu_shares = 100
# Total cpu_shares: 550 (10000 = full CPU, so each gets 550/10000 = 5.5% of time)
```

Adjust based on actual load. Monitor with:

```bash
tenement ps  # Shows health
docker stats  # If you have other containers
free -h       # Memory usage
```

## Why This Works

### vs Docker

```
Docker overhead per service: 50-100MB (images, layers, etc.)
5 services × 75MB = 375MB for overhead alone

tenement overhead: ~0 (just the supervisor process)
5 services × 5MB (native binaries) = 25MB
Savings: 350MB per 5 services
```

### vs Kubernetes

```
K8s control plane: 500MB-2GB minimum
K8s overhead: 200MB-500MB per service
5 services × 350MB = 1.75GB overhead

tenement overhead: ~10MB
5 services = ~50MB total
Savings: 1.7GB on a 4GB machine
```

### vs systemd units

```
systemd doesn't provide:
- Service isolation (namespace separation)
- Resource limits (cgroups)
- Unified health checking
- Per-service configuration file
- Restart with exponential backoff

tenement gives you all of this + socket-based IPC + metrics
```

## Example: Complete Stack

```bash
# Development
tenement spawn api --id dev
tenement spawn worker --id dev

# Test
tenement spawn api --id test
tenement spawn worker --id test
tenement spawn webhook --id test

# Production
tenement spawn api --id prod
tenement spawn worker --id prod
tenement spawn webhook --id prod
tenement spawn sync --id prod
tenement spawn jobs --id prod
```

## Scaling

When one machine isn't enough, use slum:

```rust
let db = SlumDb::init("slum.db").await?;

// Add servers
db.add_server(&Server {
    id: "web-1".into(),
    url: "http://web-1.example.com".into(),
    ..Default::default()
}).await?;

db.add_server(&Server {
    id: "worker-1".into(),
    url: "http://worker-1.example.com".into(),
    ..Default::default()
}).await?;

// Route services by load
db.spawn_instance("api", "prod", "web-1").await?;
db.spawn_instance("worker", "prod", "worker-1").await?;
```

## Next Steps

- [Quick Start](/intro/quick-start) - Get running
- [Configuration](/guides/configuration) - All options
- [Fleet Mode](/guides/fleet) - Multi-server scaling
