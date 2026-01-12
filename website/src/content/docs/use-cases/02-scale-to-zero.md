---
title: Scale-to-Zero Services
description: Services stop when idle, auto-start on request
---

**Idle services stop and cost nothing. Restart automatically on first request.**

```
service running → 5min idle → stop → memory freed → $0
new request → socket missing → spawn → route
```

## Setup

**1. Single-tenant app (no hibernation logic)**
```python
from flask import Flask
import os

app = Flask(__name__)

@app.route("/health")
def health():
    return {"status": "ok"}

@app.route("/work", methods=["POST"])
def work():
    return {"result": expensive_computation()}

if __name__ == "__main__":
    app.run(unix_socket=os.getenv("SOCKET_PATH"))
```

**2. Configure tenement with idle timeout**
```toml
[service.worker]
command = "python app.py"
socket = "/tmp/worker-{id}.sock"
health = "/health"
idle_timeout = 300              # Stop after 5 minutes idle
```

When idle_timeout expires:
- Instance stops
- Socket is removed
- Memory is freed
- Cost: $0

**3. Spawn per job/request**
```bash
tenement spawn worker --id job123
```

**4. Wake on request**

tenement's routing detects missing socket and spawns automatically:
```
job123.api.example.com → socket missing → spawn worker:job123 → route
```

## Why This Works

- **Zero cost when idle** - Stopped services use no memory or CPU
- **Instant wake** - First request spawns in <200ms (imperceptible)
- **No app changes** - Service code has zero hibernation logic
- **Simple scaling** - Just: `tenement spawn worker --id $job_id`

## Economics

```
Traditional: 1000 services always-on
├── 20MB per service = 20GB RAM
└── Cost: 10 machines @ $500/month

Scale-to-zero: 1000 services, ~2% active
├── 20 running × 20MB = 400MB RAM
└── Cost: 1 machine @ $5/month
    Savings: 100x cheaper
```

## Cold Start Reality

Typical wake time: **65-220ms**

- Process spawn: 5-10ms
- App startup: 50-200ms
- Network round-trip: 5ms

Humans perceive ~250ms as instant, so this is imperceptible to users.
