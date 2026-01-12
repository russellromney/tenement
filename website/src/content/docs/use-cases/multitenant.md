---
title: Multi-tenant SaaS
description: Running isolated services per customer
---

The primary use case for tenement: **give each customer their own isolated process**.

## The Pattern

```
customer1.myapp.com → api:customer1 → /tmp/api-customer1.sock
customer2.myapp.com → api:customer2 → /tmp/api-customer2.sock
customer3.myapp.com → api:customer3 → /tmp/api-customer3.sock
...
```

Each customer gets:
- Isolated process (no data leaks)
- Own database (separate SQLite, PostgreSQL, etc.)
- Own configuration
- Independent lifecycle (can stop one without affecting others)

## Architecture

### Single-tenant App Code

Your app has no multi-tenant logic:

```python
# app.py - Single tenant only
import os
from flask import Flask

app = Flask(__name__)
db_path = os.getenv("DATABASE_PATH")  # /data/customer1/app.db

@app.route("/")
def hello():
    return "Hello from this customer!"

if __name__ == "__main__":
    socket_path = os.getenv("SOCKET_PATH")
    app.run(socket=socket_path)
```

### Configuration

```toml
[settings]
data_dir = "/var/lib/myapp"

[service.api]
command = "python app.py"
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "on-failure"
isolation = "namespace"
idle_timeout = 300              # Auto-stop after 5 mins

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"
```

### Reverse Proxy Routing

Use nginx/Caddy to route by subdomain:

```nginx
# nginx.conf - Dynamic upstream
upstream api_customer1 {
    server unix:/tmp/api-customer1.sock;
}

upstream api_customer2 {
    server unix:/tmp/api-customer2.sock;
}

server {
    listen 80;
    server_name ~^(?<customer>.+)\.myapp\.com$;

    location / {
        set $upstream "api_${customer}";
        proxy_pass http://$upstream;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

Or integrate tenement library directly:

```rust
// Custom reverse proxy
use tenement::Hypervisor;

let hypervisor = Hypervisor::load("tenement.toml")?;
let instance = hypervisor.get_instance(&format!("api:{}", customer_id))?;
// Route request to instance.socket_path
```

## Spawning Customers

### On Signup

```bash
#!/bin/bash
CUSTOMER_ID=$1

# Spawn isolated process for this customer
tenement spawn api --id $CUSTOMER_ID

# Configure DNS (or CNAME wildcard)
# CNAME *.myapp.com → myapp.com

# Now customer.$CUSTOMER_ID.myapp.com routes to their isolated process
```

### On Signup (API)

```rust
// POST /api/customers
#[post("/customers")]
async fn create_customer(
    db: web::Data<Database>,
    hypervisor: web::Data<Hypervisor>,
    form: web::Json<SignupForm>,
) -> Result<HttpResponse> {
    // Create customer in database
    let customer = db.create_customer(&form).await?;

    // Spawn isolated tenement instance
    hypervisor.spawn("api", &customer.id)?;

    Ok(HttpResponse::Created().json(customer))
}
```

## Multi-region Scaling

Use slum to distribute customers across servers:

```rust
let db = SlumDb::init("slum.db").await?;

// Add regional servers
db.add_server(&Server {
    id: "us-east".into(),
    url: "http://api-east.example.com".into(),
    region: Some("us-east".into()),
    ..Default::default()
}).await?;

db.add_server(&Server {
    id: "us-west".into(),
    url: "http://api-west.example.com".into(),
    region: Some("us-west".into()),
    ..Default::default()
}).await?;

// Route new customer to closest region
let server_id = route_to_region(&customer.region);
db.spawn_instance(&customer.id, "api", "prod", &server_id).await?;
```

## Billing Integration

Stop instances when subscription expires:

```rust
// Scheduled task (e.g., nightly)
async fn stop_expired_customers(db: &Database, hypervisor: &Hypervisor) {
    let expired = db.get_expired_customers().await?;

    for customer in expired {
        hypervisor.stop(&format!("api:{}", customer.id))?;
    }
}
```

## Database Per Customer

Each customer gets their own database:

```toml
[service.api]
command = "python app.py"
socket = "/tmp/api-{id}.sock"

[service.api.env]
# Separate database per customer
DATABASE_PATH = "/data/{id}/app.db"

# Or PostgreSQL:
DATABASE_URL = "postgresql://user:pass@localhost/{id}_db"

# Or Redis:
REDIS_NAMESPACE = "{id}"
```

Initialize on first spawn:

```python
import os
import sqlite3
from pathlib import Path

db_path = os.getenv("DATABASE_PATH")
Path(db_path).parent.mkdir(parents=True, exist_ok=True)

if not os.path.exists(db_path):
    # First run - initialize schema
    conn = sqlite3.connect(db_path)
    conn.executescript(INIT_SCHEMA)
    conn.close()
```

## Resource Limits

Constrain each customer to prevent noisy neighbor problems:

```toml
[service.api]
command = "python app.py"
socket = "/tmp/api-{id}.sock"
memory_limit_mb = 256           # Each customer gets max 256MB
cpu_shares = 100                # Fair CPU distribution
```

With 20 running customers on a 4GB machine:
- 20 × 256MB = 5GB maximum
- With overhead: fits within 8GB machine

## Example: Complete Setup

1. **App code** (single-tenant):

```python
# app.py
from flask import Flask
import os

app = Flask(__name__)

@app.route("/")
def hello():
    return "Hello from your isolated instance!"

@app.route("/health")
def health():
    return "ok"

if __name__ == "__main__":
    socket_path = os.getenv("SOCKET_PATH")
    app.run(unix_socket=socket_path)
```

2. **Configuration**:

```toml
[settings]
data_dir = "/var/lib/myapp"

[service.api]
command = "python app.py"
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "on-failure"
isolation = "namespace"
memory_limit_mb = 256
```

3. **Spawn on signup**:

```bash
#!/bin/bash
curl http://localhost:8000/spawn/api/--id=$CUSTOMER_ID
```

4. **Route requests**:

```nginx
server {
    listen 80;
    server_name ~^(?<id>.+)\.myapp\.com$;

    location / {
        proxy_pass http://unix:/tmp/api-$id.sock;
    }
}
```

Now each customer has an isolated, independently-managed instance.

## Why This Works

- **Simple code**: No multi-tenant logic in your app
- **Strong isolation**: Process/namespace boundary, no data leaks
- **Easy debugging**: Each customer's database is separate and inspectable
- **Cheap**: Idle customers cost $0
- **Scalable**: 1000 customers, 20 running, ~5% infrastructure cost

## Next Steps

- [Quick Start](/intro/quick-start) - Get running in 5 minutes
- [Configuration](/guides/configuration) - All config options
- [Isolation Levels](/guides/isolation) - Security models
