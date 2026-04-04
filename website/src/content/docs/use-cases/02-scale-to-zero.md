---
title: Scale-to-Zero Services
description: Services stop when idle, auto-start on request
---

Idle services stop and cost nothing. The next request spawns them back.

## How it works

Set `idle_timeout` in your config and tenement handles the rest. After five minutes with no requests, tenement kills the process and frees the memory. When a request arrives for that subdomain, tenement spawns a fresh instance, waits for the health check to pass, and proxies the request through.

```toml
[service.worker]
command = "python app.py"
health = "/health"
idle_timeout = 300              # stop after 5 minutes idle
```

Your app doesn't need any hibernation logic. It just starts, serves requests, and exits when killed.

## Measured cold wake times

These are real numbers from stopping an instance and immediately hitting its subdomain (measured on a MacBook, debug build):

| Runtime | Cold wake (median) | Range |
|---------|-------------------|-------|
| Python (stdlib http.server) | 65ms | 13-174ms |
| Node.js (http module) | 105ms | 104-110ms |
| Go (`go run`, cached compile) | 140ms | 100-224ms |

Humans perceive anything under ~250ms as instant. These are all well under that threshold.

## The economics

Most SaaS customers aren't active at the same time. If you have 1000 tenants configured and 20 are active right now, the other 980 are costing you nothing.

```
Traditional: 1000 services always-on
  20MB per service = 20GB RAM
  Cost: 10 machines @ $500/month

Scale-to-zero: 1000 services, ~2% active
  20 running x 20MB = 400MB RAM
  Cost: 1 machine @ $5/month
```

The savings are roughly 100x, and the user experience is identical because the wake latency is imperceptible.
