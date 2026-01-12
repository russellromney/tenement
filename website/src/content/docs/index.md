---
title: tenement
description: Hyperlightweight process hypervisor for single-server deployments
template: splash
hero:
  tagline: Cramped housing for your processes
  image:
    file: ../../assets/hero.svg
  actions:
    - text: Get Started
      link: /intro/quick-start/
      icon: right-arrow
      variant: primary
    - text: Learn More
      link: /intro/why/
      icon: external
---

## Why tenement?

Pack **1000 rarely-active services on a single $5 server**.

- **Scale to zero** - Services sleep when idle, wake on request
- **Isolated processes** - Namespace separation, zero overhead
- **Simple config** - One TOML file defines everything
- **No orchestrator** - Just fast process supervision

## The Economics

```
1000 customers
├── 2% active = 20 running
├── 20 × 20MB = 400MB RAM
└── Cost: $5/month
    Charge: $5-10/month
    Margin: 500-1000x
```

Build single-tenant code. Deploy multi-tenant. Profit.

## Quick Start

```bash
# Install
cargo install tenement-cli

# Configure
cat > tenement.toml << EOF
[service.api]
command = "./api"
socket = "/tmp/api-{id}.sock"
health = "/health"
idle_timeout = 300
EOF

# Spawn
tenement spawn api --id user123
```

Now `user123.api.example.com` routes to their isolated instance.

## Features

- **Sub-second cold starts** - Unix sockets, no network overhead
- **On-demand spawn** - Processes start when first requested
- **Auto-restart** - Health checks with exponential backoff
- **Resource limits** - Memory and CPU constraints via cgroups v2
- **Isolation levels** - Namespace, sandbox (gVisor), or bare process
- **Web dashboard** - Monitor and manage instances
- **Prometheus metrics** - Full observability
- **Log capture** - Full-text search with streaming

## What's Next?

Choose your path:

- **Just want to run it?** → [Quick Start](/intro/quick-start)
- **Want to understand it?** → [Why tenement?](/intro/why)
- **Care about the money?** → [The Economics](/intro/economics)
- **Building a SaaS?** → [Multi-tenant Guide](/use-cases/multitenant)
- **Running microservices?** → [Microservices on VPS](/use-cases/microservices)
- **Need details?** → [CLI Reference](/reference/cli)
