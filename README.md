# tenement

**Lightweight process hypervisor for single-server deployments in Rust.**

Pack 100+ services on a $5 VPS. Each customer gets their own process. Spawn on demand, stop when idle, wake on first request.

```
alice.notes.example.com  ->  notes:alice  ->  isolated process + own database
bob.notes.example.com    ->  notes:bob    ->  isolated process + own database
```

Write single-tenant code. Deploy it for every customer.

> Experimental. Actively developed. APIs may change.

## Why not systemd?

systemd runs processes. tenement runs *tenants*.

| | systemd | tenement |
|---|---------|----------|
| Routing | You configure nginx/caddy per service | `alice.notes.example.com` just works |
| Scale to zero | No. Processes run forever | Idle processes stop, wake on first request |
| Per-tenant data | You manage it | Each instance gets `{data_dir}/{id}/` automatically |
| Spawn new tenant | Write a unit file, reload | `ten spawn notes:alice` |
| Health + restart | Basic restart-on-failure | Health endpoint checks, exponential backoff, max restart limits |
| Deployment | Rolling restart scripts | `ten deploy notes:v2` + `ten route notes --from v1 --to v2` |
| Metrics | You set up prometheus exporters | Built-in per-tenant request counts and latencies |
| Logs | journalctl | `ten logs notes:alice`, full-text search, SSE streaming |
| Auth | N/A | Bearer token API with admin + tenant-scoped tokens |

tenement is for when you want Fly Machines on your own hardware. You have one server, many customers, and you want each one isolated without Kubernetes complexity.

## Install

```bash
cargo install tenement-cli
```

## Quick Start

**1. Write your app** (any language, read `PORT` from env):

```python
# app.py
import os, json, sqlite3
from http.server import HTTPServer, BaseHTTPRequestHandler

PORT = int(os.environ["PORT"])
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

**2. Configure:**

```toml
# tenement.toml
[service.notes]
command = "python3 app.py"
health = "/health"
idle_timeout = 300
isolation = "process"

[service.notes.env]
DATA_DIR = "{data_dir}/{id}"
```

**3. Run:**

```bash
ten serve --port 8080 --domain localhost
ten token-gen
ten spawn notes:alice
ten spawn notes:bob

curl http://alice.notes.localhost:8080/notes   # alice's data
curl http://bob.notes.localhost:8080/notes     # bob's data (separate)
ten ps                                         # list instances
```

After 5 minutes idle, tenement stops the process. The next request wakes it automatically (sub-second).

## How it works

1. You define a **service** in `tenement.toml` (command, health endpoint, env vars)
2. You **spawn instances** of that service (`ten spawn notes:alice`)
3. tenement allocates a port, sets `PORT` and `DATA_DIR` env vars, starts the process
4. Requests to `alice.notes.example.com` route to that instance
5. Health checks run automatically; unhealthy instances restart with backoff
6. Idle instances stop after the configured timeout, wake on the next request

Your app handles its own auth, business logic, and data. tenement handles routing, lifecycle, and isolation.

## Features

- **Subdomain routing** - `alice.notes.example.com` routes to `notes:alice`
- **Scale-to-zero** - idle processes stop, wake on first request (sub-second)
- **Per-tenant data** - each instance gets its own `{data_dir}/{id}/` directory
- **Process isolation** - Linux namespace isolation (PID + mount), or bare process on macOS
- **Health checks** - HTTP health endpoint checks with exponential backoff restart
- **Process groups** - killing an instance kills all its child processes (no orphans)
- **Shell command parsing** - `command = "uv run python app.py"` just works
- **Weighted routing** - blue-green and canary deployments via traffic weights
- **Built-in TLS** - Let's Encrypt certificates, or use Caddy as a reverse proxy
- **Prometheus metrics** - per-tenant request counts and latencies at `/metrics`
- **Log capture** - full-text search, SSE streaming, `ten logs` CLI
- **Auth** - admin tokens for management API, tenant-scoped tokens for limited access
- **TENEMENT_SERVER env var** - set once, skip `--server` on every CLI command

## CLI

```bash
# Server
ten serve --port 8080 --domain localhost
ten init --name myapp

# Instance management
ten spawn notes:alice
ten stop notes:alice
ten restart notes:alice
ten ps
ten health notes:alice

# Logs
ten logs notes:alice
ten logs -f                  # follow all

# Deployment
ten deploy notes:v2
ten route notes --from v1 --to v2
ten weight notes:alice 50    # canary: 50% traffic

# Auth
ten token-gen                # admin token
ten token-gen --tenant alice # scoped token

# Config
ten config
export TENEMENT_SERVER=http://localhost:9090  # skip --server on every command
```

## Configuration

```toml
[settings]
data_dir = "./data"              # global data dir (DB, tokens, per-instance dirs)
health_check_interval = 10       # seconds between health checks

[service.api]
command = "uv run python app.py" # shell-split automatically if no args field
health = "/health"               # HTTP endpoint for health checks
isolation = "process"            # "process" (macOS/Linux) or "namespace" (Linux)
idle_timeout = 300               # stop after N seconds idle (0 = never)
startup_timeout = 10             # seconds to wait for first health check (increase for go run)
storage_persist = true           # keep data dir on stop
memory_limit_mb = 256            # cgroups memory limit (Linux)
cpu_shares = 100                 # cgroups CPU weight (Linux)

[service.api.env]
DATA_DIR = "{data_dir}/{id}"     # interpolation: {name}, {id}, {data_dir}, {port}
```

## Examples

See [`examples/`](examples/) for complete working setups:

| Example | What it shows |
|---------|---------------|
| [`hello-world`](examples/hello-world/) | Simplest possible setup (bash + netcat) |
| [`python-fastapi`](examples/python-fastapi/) | FastAPI with per-tenant database |
| [`node-fastify`](examples/node-fastify/) | Node.js Fastify server |
| [`go-http`](examples/go-http/) | Go net/http server |
| [`multi-tenant`](examples/multi-tenant/) | Per-tenant notes API with SQLite |
| [`multi-runtime`](examples/multi-runtime/) | Python + Node + Go in one config, with test script |
| [`multi-env`](examples/multi-env/) | Multiple services, environments, and isolation levels |
| [`auth-test`](examples/auth-test/) | App-level auth passthrough (tenement doesn't touch your auth) |

The `multi-runtime` example includes a 56-test integration script that verifies auth, data isolation, and cross-service isolation across all three runtimes.

## Production

For a single Hetzner/DigitalOcean VPS with wildcard HTTPS:

```bash
# DNS: *.app.example.com -> your server IP

cargo install tenement-cli
cd /opt/myapp
ten init --name myapp --command "python3 app.py"
ten token-gen

# Install as systemd service with Caddy for HTTPS:
ten install --domain app.example.com --caddy --dns-provider cloudflare

# Manage tenants:
ten spawn myapp:customer1
ten spawn myapp:customer2
ten ps
ten logs -f
```

## Development

```bash
cargo test    # 559 tests
cargo bench   # benchmarks
```

See [ROADMAP.md](ROADMAP.md) for planned work and [CHANGELOG.md](CHANGELOG.md) for history.

## Documentation

Full docs at [tenement.dev](https://tenement.dev).

## License

Apache 2.0
