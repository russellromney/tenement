---
title: Why tenement?
description: The problem tenement solves and how it compares to alternatives
---

**Cramped housing for your processes.**

Your apps don't need a penthouse—a roof, some supervision, and the occasional health check are probably fine. tenement packs processes into Unix sockets, watches them like a suspicious landlord, and restarts them when they misbehave.

## The Problem

Running multi-tenant SaaS traditionally means:
- Build complex multi-tenant code (data isolation, tenant routing, security)
- Pay per-instance pricing ($5-100/month per customer)
- Charge $500+/month to cover your infrastructure costs
- Or accept razor-thin margins

## The tenement Solution

A lightweight hypervisor that:
- Spawns isolated processes on-demand
- Routes requests by subdomain
- Automatically stops idle processes (scale-to-zero)
- Restarts failed instances with exponential backoff
- Runs in ~10MB, one Rust binary

## Comparison

| Alternative | Problem |
|---|---|
| **Docker** | Heavy, slow cold starts (~500ms), network overhead |
| **systemd** | No on-demand spawn, no routing, no idle timeout |
| **K8s/Nomad** | Overkill for single server, massive control plane overhead |
| **Fly Machines** | Pay per machine—for 1000 services, costs explode |
| **nginx + uWSGI** | No namespace isolation, manual supervision, fragile restart logic |
| **Bash scripts** | No health checks, no proper supervision, debugging nightmare |

**tenement gives you:**
- Sub-second cold starts (Unix sockets, no network layer)
- On-demand spawn (wake-on-request)
- Auto-restart (health checks + exponential backoff)
- Zero overhead (direct process supervision)
- Isolation levels (process, namespace, sandbox)
- Simple config (one TOML file)

## What Makes tenement Unique

tenement is the only tool that solves this specific problem: **pack 1000 rarely-active isolated services on a single small server.**

No orchestrator. No control plane overhead. Just fast process supervision and intelligent routing.
