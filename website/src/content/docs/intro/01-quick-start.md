---
title: Quick Start
description: Get tenement running in 5 minutes
---

## Prerequisites

- **Linux** - tenement uses Linux-specific features (namespaces, cgroups)
- **Rust toolchain** - for installation via cargo
- **Your app** - must listen on the `PORT` environment variable

## Install

```bash
cargo install tenement-cli
```

Verify installation:

```bash
ten --version
```

## Complete Example

Let's run a Python app with tenement. This example uses FastAPI but any HTTP server works.

### 1. Create Your App

Create `app.py`:

```python
import os
from fastapi import FastAPI
import uvicorn

app = FastAPI()

@app.get("/")
def root():
    return {"message": "Hello from tenement!"}

@app.get("/health")
def health():
    return {"status": "ok"}

if __name__ == "__main__":
    port = int(os.getenv("PORT", "8000"))
    uvicorn.run(app, host="127.0.0.1", port=port)
```

### 2. Create Config

Create `tenement.toml`:

```toml
[service.api]
command = "uv run python app.py"
health = "/health"
```

That's it! Tenement auto-allocates a port and sets the `PORT` environment variable.

### 3. Start the Server

```bash
ten serve --port 8080 --domain localhost
```

### 4. Spawn an Instance

In another terminal:

```bash
ten spawn api --id prod
```

### 5. Test It

```bash
# Direct access via subdomain
curl http://prod.api.localhost:8080/
# {"message": "Hello from tenement!"}

# Health check
curl http://prod.api.localhost:8080/health
# {"status": "ok"}

# List instances
ten ps
# INSTANCE    PORT    UPTIME   HEALTH   WEIGHT
# api:prod    30001   2m       healthy  100
```

### 6. Stop When Done

```bash
ten stop api:prod
```

## Key Concepts

- **Service**: A template defining how to run your app (`[service.api]`)
- **Instance**: A running copy of a service with an ID (`api:prod`, `api:staging`)
- **PORT**: Auto-set environment variable your app should listen on
- **Health check**: HTTP endpoint tenement polls to verify your app is running

## What Your App Needs

1. **Listen on PORT**: Read `PORT` from environment, don't hardcode
2. **Health endpoint**: Return HTTP 200 at `/health` (or your configured path)
3. **Bind to 127.0.0.1**: Not 0.0.0.0 (tenement handles external access)

## Next Steps

- [Configuration Reference](/guides/03-configuration) - Full config options
- [Production Deployment](/guides/04-production) - TLS and systemd setup
- [Deployment Patterns](/guides/05-deployments) - Blue-green and canary
- [Economics](/intro/02-economics) - Why tenement saves money
