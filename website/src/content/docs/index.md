---
title: tenement
description: Hyperlightweight process hypervisor for single-server deployments
---

**Lightweight process hypervisor in Rust.**

Pack **100 services on a single $5 server**, with 20% typically active.

## What You Get

- **Single binary** (~10MB Rust)
- **Built-in routing** - `user.api.example.com` → `api:user` automatically
- **Integrated TLS** - Automatic HTTPS with Let's Encrypt
- **Process isolation** - Namespace separation (zero overhead) or gVisor sandbox (untrusted code)
- **Auto-restart** - Health checks with exponential backoff
- **Scale-to-zero** - Stop idle instances, auto-start on request
- **Weighted routing** - Blue-green and canary deployments
- **One TOML config** - All services defined in one file

## Perfect For

- 10-1000 customer instances on one $5 server
- Multi-tenant SaaS (each tenant = isolated process)
- Microservices without Kubernetes overhead
- Avoiding Docker complexity for small deployments

## vs Alternatives

| Tool | Why not |
|------|---------|
| Docker | Heavy, slow startup, network overhead |
| systemd | No routing, no idle timeout |
| K8s | Massive overhead for single server |
| Fly Machines | Per-machine pricing kills margin at scale |

## Get Started

- [Quick Start](/intro/01-quick-start) - Installation & first spawn
- [The Economics](/intro/02-economics) - Detailed cost breakdown

## Guides

- [Isolation Levels](/guides/01-isolation) - Namespace vs sandbox
- [Fleet Mode](/guides/02-fleet) - Multi-server orchestration
- [Configuration](/guides/03-configuration) - Full TOML reference
- [Production Setup](/guides/04-production) - TLS, systemd, Caddy
- [Deployments](/guides/05-deployments) - Blue-green, canary routing

## Use Cases

- [Multi-tenant SaaS](/use-cases/01-multitenant) - Primary use case
- [Scale-to-Zero Services](/use-cases/02-scale-to-zero) - Idle timeout & wake-on-request

## Reference

- [Roadmap](/reference/roadmap) - What's coming
- [Troubleshooting](/reference/troubleshooting) - Common issues
