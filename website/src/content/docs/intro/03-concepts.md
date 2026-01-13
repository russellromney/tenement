---
title: Concepts
description: Understanding tenement architecture and terminology
---

This page explains how tenement works internally: architecture, terminology, and routing.

## Architecture

```
                        Internet
                            │
                            ▼
                    ┌───────────────┐
                    │   tenement    │
                    │   (server)    │
                    │   :8080       │
                    └───────┬───────┘
                            │
            ┌───────────────┼───────────────┐
            │               │               │
            ▼               ▼               ▼
      ┌──────────┐    ┌──────────┐    ┌──────────┐
      │ api:prod │    │ api:stg  │    │ web:prod │
      │ :30001   │    │ :30002   │    │ :30003   │
      └──────────┘    └──────────┘    └──────────┘
```

**Request flow:**

1. Request arrives at `prod.api.example.com:8080`
2. tenement parses subdomain: `{id}.{service}.{domain}` → `api:prod`
3. Request proxied to instance's allocated port (30001)
4. Instance processes request and returns response
5. tenement forwards response to client

## Terminology

| Term | Definition | Example |
|------|------------|---------|
| **Service** | A template defining how to run your app. Defined in `[service.X]` config sections. | `[service.api]` defines the "api" service |
| **Instance** | A running copy of a service with a unique ID. Multiple instances can run from one service. | `api:prod`, `api:staging`, `api:customer123` |
| **Spawn** | Start a new instance of a service. | `ten spawn api --id prod` |
| **Health check** | HTTP request tenement makes to verify instance is running. | `GET /health` returns 200 OK |
| **Isolation** | The level of separation between instances. | `namespace`, `sandbox`, `process` |
| **Weight** | Traffic percentage an instance receives (0-100). | `ten weight api:prod 80` |

## Instance Lifecycle

```
                    ten spawn
                        │
                        ▼
┌─────────┐        ┌─────────┐        ┌─────────┐
│ stopped │───────▶│ starting│───────▶│ running │
└─────────┘        └─────────┘        └────┬────┘
     ▲                                     │
     │         ┌─────────────┐             │
     │         │  unhealthy  │◀────────────┤ health check fails
     │         └──────┬──────┘             │
     │                │ max_restarts       │
     │                ▼                    │
     │         ┌─────────────┐             │
     └─────────│   failed    │             │
               └─────────────┘             │
                                           │
     ┌────────────────────┬────────────────┘
     │                    │
     ▼                    ▼
┌─────────┐        ┌─────────┐
│ idle    │───────▶│ stopped │
│ timeout │        │         │
└─────────┘        └─────────┘
```

## Routing Patterns

| URL Pattern | Routing Behavior |
|-------------|------------------|
| `{id}.{service}.{domain}` | Direct to specific instance |
| `{service}.{domain}` | Weighted load balance across all instances |
| `{domain}` | Dashboard |

**Examples:**
- `prod.api.example.com` → always routes to `api:prod`
- `api.example.com` → load balanced across `api:prod`, `api:staging`, etc.
- `example.com` → tenement dashboard

## When to Use tenement

**Good fit:**

- Multi-tenant SaaS - one process per customer
- Microservices on a single server
- Development environments needing isolation
- Cost-sensitive deployments (overstuff a VPS)
- Scale-to-zero requirements
- Blue-green and canary deployments

**Not a good fit:**

- **Multi-server deployments** - use Kubernetes, Nomad, or Fly.io
- **Windows/macOS production** - tenement is Linux-only
- **Container ecosystem** - if you need Docker images, OCI registries
- **Serverless functions** - use Lambda, CloudFlare Workers
- **Stateful workloads** - tenement doesn't manage databases
- **High-availability requirements** - single server = single point of failure

## Comparison

| Need | tenement | Alternative |
|------|----------|-------------|
| Multi-tenant on one server | Yes | Docker + nginx |
| Scale-to-zero | Yes (idle_timeout) | Fly Machines |
| Subdomain routing | Built-in | nginx/Caddy config |
| Process isolation | namespace/sandbox | Docker/gVisor |
| Multi-server | No (use slum) | Kubernetes |
| Container images | No | Docker |

## Next Steps

- [Configuration Reference](/guides/03-configuration) - All config options
- [Isolation Levels](/guides/01-isolation) - namespace vs sandbox vs process
- [Production Deployment](/guides/04-production) - TLS and systemd
