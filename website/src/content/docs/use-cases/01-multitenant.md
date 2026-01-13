---
title: Multi-tenant SaaS
description: Give each customer their own isolated process
---

**The primary use case: one app, multiple customers, each with their own process.**

```
customer1.myapp.com → api:customer1
customer2.myapp.com → api:customer2
```

tenement handles subdomain routing automatically.

## Setup

**1. Single-tenant app (no multi-tenant logic)**
```python
# app.py
import os
from fastapi import FastAPI
import uvicorn

app = FastAPI()
db_path = os.getenv("DATABASE_PATH")  # /data/customer123/app.db

@app.get("/")
def hello():
    return "Hello!"

@app.get("/health")
def health():
    return {"status": "ok"}

if __name__ == "__main__":
    port = int(os.getenv("PORT", "8000"))
    uvicorn.run(app, host="127.0.0.1", port=port)
```

**2. Configure tenement**
```toml
[settings]
data_dir = "/var/lib/myapp"

[service.api]
command = "python app.py"
health = "/health"
isolation = "namespace"
idle_timeout = 300

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
```

**3. Spawn per customer**
```bash
ten spawn api --id customer123
# customer123.api.myapp.com now routes to their isolated instance
```

**4. Add HTTPS (production)**

See [Production Deployment](/guides/04-production) for TLS setup with Caddy.

## Why This Works

- **Simple app code** - No multi-tenant data isolation complexity
- **Strong isolation** - Process boundary, data can't leak
- **Per-customer billing** - Easy to track and charge
- **Cost effective** - Idle customers cost $0 (scale-to-zero)

Each customer is just: `ten spawn api --id $customer_id`
