---
title: tenement
description: Lightweight process hypervisor for single-server deployments
---

**Lightweight process hypervisor in Rust.**

Run 100+ isolated services on a single server, with most idle at any time.

## Features

- **Subdomain routing** - `user.api.example.com` → `api:user`
- **Scale-to-zero** - Stop idle instances, wake on request
- **Process isolation** - Namespace separation (zero overhead) or gVisor sandbox
- **Weighted routing** - Blue-green and canary deployments
- **Auto-restart** - Health checks with exponential backoff
- **Built-in TLS** - Let's Encrypt certificates
- **Single binary** - ~10MB, one TOML config file

## Use Cases

- Multi-tenant SaaS (each tenant = isolated process)
- Microservices on a single server
- Scale-to-zero services without per-machine pricing

## Comparison

| Tool | Trade-off |
|------|-----------|
| Docker | Container overhead, slower startup |
| systemd | No routing, no idle timeout |
| Kubernetes | Complex for single-server deployments |
| Fly Machines | Pay per machine, can't overstuff |

## Get Started

- [Quick Start](/intro/01-quick-start) - Installation & first spawn
- [Why tenement?](/intro/02-economics) - The problem it solves

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
