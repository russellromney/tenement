---
title: Multi-tenant SaaS
description: Give each customer their own isolated process
---

**The primary use case: one app, multiple customers, each with their own process.**

```
customer1.myapp.com → api:customer1 → /tmp/api-customer1.sock
customer2.myapp.com → api:customer2 → /tmp/api-customer2.sock
```

## Setup

**1. Single-tenant app (no multi-tenant logic)**
```python
# app.py
import os
from flask import Flask

app = Flask(__name__)
db_path = os.getenv("DATABASE_PATH")  # /data/customer123/app.db

@app.route("/")
def hello():
    return "Hello!"

if __name__ == "__main__":
    socket_path = os.getenv("SOCKET_PATH")
    app.run(unix_socket=socket_path)
```

**2. Configure tenement**
```toml
[settings]
data_dir = "/var/lib/myapp"

[service.api]
command = "python app.py"
socket = "/tmp/api-{id}.sock"
health = "/health"
isolation = "namespace"
idle_timeout = 300

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
```

**3. Spawn per customer**
```bash
tenement spawn api --id customer123
# customer123.myapp.com now routes to their isolated instance
```

**4. Route with nginx/Caddy**
```nginx
server {
  listen 80;
  server_name ~^(?<id>.+)\.myapp\.com$;
  location / {
    proxy_pass http://unix:/tmp/api-$id.sock;
  }
}
```

## Why This Works

- **Simple app code** - No multi-tenant data isolation complexity
- **Strong isolation** - Process boundary, data can't leak
- **Per-customer billing** - Easy to track and charge
- **Cost effective** - Idle customers cost $0 (scale-to-zero)

Each customer is just: `tenement spawn api --id $customer_id`
