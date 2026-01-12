---
title: Scale-to-zero Services
description: Services that cost nothing when idle
---

Use tenement's **idle timeout** to automatically stop services that aren't being used.

## The Problem

Traditional deployments:
- Reserve capacity for all services, even rarely-used ones
- Pay continuously, even when the service is sleeping
- Cost grows linearly with number of services

## The Solution

tenement's hibernation mode:
- Services stop after N seconds idle
- Automatically start on first request
- Cost per unused service: $0

## Configuration

```toml
[service.api]
command = "./api"
socket = "/tmp/api-{id}.sock"
health = "/health"
idle_timeout = 300              # Auto-stop after 5 minutes
```

When `idle_timeout` expires with no requests:
1. Instance is stopped
2. Socket is removed
3. Process is killed
4. Memory is freed

On next request:
1. Reverse proxy detects no socket
2. Triggers spawn (or proxy calls spawn endpoint)
3. Instance starts
4. Request is routed

## Example: Microservices

A typical architecture with multiple services:

```toml
# 1000 small services, ~5MB each
# Only 20 running at any time = 100MB memory
# Cost: 20 × $0.0001/hour = $0.002/hour

[service.analytics]
command = "./analytics-worker"
socket = "/tmp/analytics-{id}.sock"
idle_timeout = 300

[service.reports]
command = "./reports-worker"
socket = "/tmp/reports-{id}.sock"
idle_timeout = 300

[service.sync]
command = "./sync-worker"
socket = "/tmp/sync-{id}.sock"
idle_timeout = 300

# ... 1000 services total ...
```

## Wake-on-Request

Your reverse proxy detects missing socket and spawns:

### With Built-in Proxy

```rust
// Built into tenement HTTP server
// GET customer1.api.example.com/
// → Checks if socket exists
// → If not: spawns api:customer1
// → Routes request
```

### With nginx

Use a custom nginx module or external service:

```nginx
error_page 502 = @spawn;

location @spawn {
    # Call tenement API to spawn
    proxy_pass http://tenement:8000/spawn/api/$customer_id;

    # Retry original request
    error_page 502 = @error;
    proxy_pass http://unix:/tmp/api-$customer_id.sock;
}
```

### With Custom Proxy

```rust
use tenement::Hypervisor;

#[tokio::main]
async fn main() {
    let hypervisor = Hypervisor::load("tenement.toml")?;

    // Spawn on-demand
    if !hypervisor.instance_exists("api:customer1") {
        hypervisor.spawn("api", "customer1")?;
    }

    // Route to socket
    let instance = hypervisor.get_instance("api:customer1")?;
    proxy_to_socket(&instance.socket_path);
}
```

## Cost Analysis

### Traditional (Always-On)

```
1000 services × 5MB each = 5GB RAM
5GB RAM × 1000 customers = 5TB total (not realistic)

Reality: Need 10-20 machines @ $50/month = $500-1000/month
Plus management overhead
```

### Scale-to-Zero (tenement)

```
1000 services
~2% active = 20 running × 5MB = 100MB RAM
Runs on 1 machine @ $5/month = $5/month
Savings: 99x cheaper
```

## Metrics

Track hibernation effectiveness:

```toml
[service.api]
# Metrics available at /metrics (Prometheus format)
```

Query Prometheus:

```promql
# Instance uptime percentage
sum(tenement_instance_uptime_seconds) / count(tenement_instances) * 100

# Average instances running
avg(tenement_instances_running)

# Hibernation events (wakes)
rate(tenement_hibernation_wakes_total[5m])
```

## Cold Start Latency

Typical numbers:

| Component | Time |
|-----------|------|
| Process spawn | 5-10ms |
| Socket bind | 5ms |
| App startup | 50-200ms (depends on app) |
| First request | 5ms |
| **Total** | **65-220ms** |

User impact: imperceptible (humans perceive ~250ms as instant)

For critical services, keep a few instances warm:

```rust
// Pre-warm critical services
hypervisor.spawn("api", "always-on-1")?;
hypervisor.spawn("api", "always-on-2")?;

// Everything else hibernates
```

## Example: Reporting Service

A reporting service used once a day:

```toml
[service.reports]
command = "python generate-reports.py"
socket = "/tmp/reports-{id}.sock"
health = "/health"
idle_timeout = 60               # Stop after 1 minute idle
```

```python
# generate-reports.py
from flask import Flask
import time

app = Flask(__name__)

@app.route("/health")
def health():
    return {"status": "ok"}

@app.route("/generate", methods=["POST"])
def generate():
    # Expensive reporting job
    start = time.time()
    result = compute_reports()
    elapsed = time.time() - start

    return {"status": "done", "elapsed_seconds": elapsed}
```

Usage pattern:
- 23:59 - User requests report
- 00:00 - Instance spawns (<200ms), report generates
- 00:05 - Instance idles 5 minutes, stops (no cost)
- 23:59 - Repeat next day

Cost: 5 minutes of computation per day = ~0.003% of month.

## Next Steps

- [Quick Start](/intro/quick-start) - Get running
- [The Economics](/intro/economics) - Cost breakdown
- [Configuration](/guides/configuration) - idle_timeout option
