# tenement

> **Experimental.** Actively developed, not yet production-ready. APIs and config format may change.

**Process hypervisor for single-server multi-tenant deployments.**

Pack 100+ tenants on a $5 VPS. Each customer gets their own process and database. Spawn on demand, stop when idle, wake on first request.

```
alice.notes.example.com  ->  notes:alice  ->  isolated process + SQLite
bob.notes.example.com    ->  notes:bob    ->  isolated process + SQLite
```

Write single-tenant code. Deploy it for every customer.

## Why

You have a side project you want to offer as SaaS. You don't want to deal with multi-tenant database schemas, row-level security, or shared-state bugs. You want each customer to get their own everything (process, database, files), but you only have one server.

tenement gives you Fly Machines-style process management on your own VPS: subdomain routing, scale-to-zero, wake-on-request, health checks, and resource limits. No Docker, no Kubernetes, no containers. Just processes.

## Quick Start

```bash
cargo install tenement-cli
```

**1. Create your app** (any language, just read `PORT` and `DATA_DIR` from env):

```python
# app.py - per-tenant notes API
import os, json, sqlite3
from http.server import HTTPServer, BaseHTTPRequestHandler

PORT = int(os.environ.get("PORT", "8000"))
DB = os.path.join(os.environ.get("DATA_DIR", "."), "notes.db")

def get_db():
    os.makedirs(os.path.dirname(DB) or ".", exist_ok=True)
    db = sqlite3.connect(DB)
    db.execute("CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, text TEXT)")
    return db

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            self.respond(200, {"status": "ok"})
        else:
            db = get_db()
            notes = [{"id": r[0], "text": r[1]} for r in db.execute("SELECT * FROM notes").fetchall()]
            db.close()
            self.respond(200, notes)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        db = get_db()
        db.execute("INSERT INTO notes (text) VALUES (?)", (body["text"],))
        db.commit()
        db.close()
        self.respond(201, {"ok": True})

    def respond(self, code, data):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
```

**2. Configure tenement:**

```toml
# tenement.toml
[service.notes]
command = "python3 app.py"
health = "/health"
idle_timeout = 300        # stop after 5 min idle
storage_persist = true    # keep database files across restarts

[service.notes.env]
DATA_DIR = "{data_dir}"
```

**3. Run:**

```bash
ten token-gen                # create API token
ten serve --port 8080 --domain localhost

# In another terminal:
ten spawn notes:alice
ten spawn notes:bob

# Each tenant has their own process and database:
curl -X POST http://alice.notes.localhost:8080/notes \
  -H "Content-Type: application/json" -d '{"text":"hello from alice"}'

curl http://alice.notes.localhost:8080/notes   # alice's notes
curl http://bob.notes.localhost:8080/notes     # bob's notes (empty)

ten ps                       # see running instances
ten logs notes:alice         # tail alice's logs
```

After 5 minutes idle, tenement stops the process. The next request wakes it automatically (sub-second).

## Features

- **Subdomain routing** -- `alice.notes.example.com` routes to `notes:alice`
- **Scale-to-zero** -- idle processes stop automatically, wake on first request
- **Per-tenant data** -- each tenant gets their own `{data_dir}` for databases and files
- **Process isolation** -- Linux namespace separation (PID + mount), no container overhead
- **Health checks** -- automatic restart with exponential backoff
- **Weighted routing** -- blue-green and canary deployments
- **Built-in TLS** -- Let's Encrypt certificates, or use Caddy
- **Prometheus metrics** -- per-tenant request counts and latencies
- **Log capture** -- full-text search, SSE streaming, `ten logs` CLI

## CLI

```bash
ten init                     # scaffold tenement.toml
ten serve                    # start server
ten spawn notes:alice        # create instance
ten stop notes:alice         # stop instance
ten restart notes:alice      # restart instance
ten ps                       # list instances
ten logs notes:alice         # tail logs
ten logs -f                  # follow all logs
ten health notes:alice       # check health
ten weight notes:alice 50    # set traffic weight
ten deploy notes:v2          # deploy + wait healthy
ten route notes --from v1 --to v2  # blue-green swap
ten config                   # show config
ten token-gen                # generate API token
```

All commands (except `serve`, `init`, `config`, `token-gen`) talk to the running server via HTTP.

## Production Deployment

For a single Hetzner/DigitalOcean VPS with wildcard HTTPS:

**1. DNS:** Add a wildcard A record `*.app.example.com` pointing to your server IP.

**2. Install and configure:**

```bash
cargo install tenement-cli
cd /opt/myapp
ten init --name myapp --command "python3 app.py"
ten token-gen
```

**3. Install as systemd service with Caddy for HTTPS:**

```bash
ten install --domain app.example.com --caddy --dns-provider cloudflare
```

This creates a systemd service and a Caddyfile with wildcard TLS. Caddy handles HTTPS termination, tenement handles routing and process management.

**4. Manage:**

```bash
ten spawn myapp:customer1
ten spawn myapp:customer2
ten ps
ten logs myapp:customer1
```

## Development

```bash
cargo test    # 556 tests
cargo bench   # benchmarks
```

See [ROADMAP.md](ROADMAP.md) for planned work and [CHANGELOG.md](CHANGELOG.md) for history.

## License

Apache 2.0
