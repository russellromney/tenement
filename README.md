# tenement

**Lightweight Rust hypervisor for single-server deployments of many single-tenant processes.**

---

tenement is a process hypervisor for running multi-tenant services on a single server. It spawns one process per tenant, routes requests by subdomain, runs HTTP health checks, and stops idle instances automatically. When the next request arrives, it wakes them back up in under a second.

You write your app as if it serves one customer. tenement runs a copy for each of them.

```
alice.notes.example.com  ->  notes:alice  ->  isolated process + own database
bob.notes.example.com    ->  notes:bob    ->  isolated process + own database
```

> Experimental. Actively developed. APIs may change.

## Why this exists

systemd can run processes, but it doesn't route requests or stop idle ones. You'd write a unit file for each customer and wire up nginx yourself. Docker adds container overhead you don't need for trusted code on one machine. Kubernetes is absurd for a $5 VPS.

tenement is [Fly Machines](https://fly.io/docs/machines/) on your own hardware. Spawn a process, give it a subdomain, let it sleep when nobody's using it, wake it up on the next request.

| | systemd | tenement |
|---|---------|----------|
| Routing | You configure nginx per service | `alice.notes.example.com` just works |
| Scale to zero | Processes run forever | Idle processes stop, wake on first request |
| Per-tenant data | You manage it | Each instance gets its own data directory |
| New customer | Write a unit file, reload | `ten spawn notes:alice` |
| Health + restart | Basic restart-on-failure | HTTP health checks, exponential backoff |
| Deployment | Rolling restart scripts | `ten deploy notes:v2` then `ten route --from v1 --to v2` |
| Logs | journalctl | `ten logs notes:alice` with full-text search |

## Quick start

Install the CLI and start the server:

```bash
cargo install tenement-cli
ten serve --port 8080 --domain localhost
ten token-gen
```

Here's a complete app. It's a notes API backed by SQLite, and it doesn't know anything about tenants. It just reads `PORT` from the environment and serves whoever's asking.

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

The config is six lines. You tell tenement what command to run, where the health endpoint is, and what environment variables to set. The `{id}` in `DATA_DIR` gets replaced with the tenant name at spawn time.

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

Now spawn a couple tenants and try it:

```bash
ten spawn notes:alice
ten spawn notes:bob

curl -X POST http://alice.notes.localhost:8080/notes \
  -H "Content-Type: application/json" -d '{"text":"hello from alice"}'

curl http://alice.notes.localhost:8080/notes   # alice's notes
curl http://bob.notes.localhost:8080/notes     # bob's notes (empty, different database)
ten ps                                         # list running instances
```

Alice and Bob each get their own process, their own SQLite database, their own data directory. After 5 minutes of no requests, tenement kills the process. The next request wakes it back up:

| Runtime | Cold wake (median) |
|---------|-------------------|
| Python | ~65ms |
| Node.js | ~105ms |
| Go (`go run`) | ~140ms |

## How it works

You define a service in your config (the command, health endpoint, and environment variables). When you spawn an instance, tenement allocates a TCP port, sets `PORT` in the environment, and starts the process. Requests to `alice.notes.example.com` get proxied to alice's port. tenement polls the health endpoint and restarts unhealthy instances with exponential backoff. When nobody's made a request for a while, it kills the process. When someone does, it spawns a fresh one.

Your app handles its own auth, business logic, and data. tenement handles routing, lifecycle, and isolation. These two layers are completely independent, which means tenement doesn't touch your `Authorization` headers or care what framework you're using. You can verify this yourself with the [auth-test example](examples/auth-test/).

## The economics

Most SaaS customers aren't active simultaneously. If you have 1000 customers and only 20 are using the product at any given moment, the traditional approach keeps all 1000 processes running. That's 20GB of RAM across 10 machines at maybe $500/month. With tenement, the 980 idle instances cost nothing. You run 20 processes on one machine for $5/month. The wake-on-request latency is under a second, so users don't notice.

This pairs well with SQLite. Each customer gets their own database file, replicated to S3 with something like [walrust](https://github.com/russellromney/walrust) or Litestream. No shared Postgres, no connection pooling, no schema migrations that touch everyone's data at once.

## What's in the box

Tenement does subdomain routing (`alice.api.example.com` routes to `api:alice`), scale-to-zero with wake-on-request, per-tenant data directories, process isolation via Linux namespaces, HTTP health checks with exponential backoff, weighted routing for blue-green and canary deployments, built-in TLS via Let's Encrypt, Prometheus metrics, log capture with full-text search, and a bearer token auth system for the management API with both admin and tenant-scoped tokens.

Commands like `uv run python app.py` or `go run main.go` are shell-split automatically, and every instance runs in its own process group so killing it also kills any child processes. No orphans.

## CLI

```bash
ten serve --port 8080 --domain localhost    # start the server
ten spawn notes:alice                       # create a tenant
ten stop notes:alice                        # stop a tenant
ten ps                                      # list everything
ten logs notes:alice                        # tail logs
ten logs -f                                 # follow all logs
ten deploy notes:v2                         # deploy a new version
ten route notes --from v1 --to v2           # blue-green swap
ten weight notes:alice 50                   # canary: 50% traffic
ten token-gen                               # admin API token
ten token-gen --tenant alice                # scoped token for alice
```

Set `TENEMENT_SERVER` to skip passing `--server` on every command:

```bash
export TENEMENT_SERVER=http://localhost:9090
ten ps    # just works
```

## Configuration

```toml
[settings]
data_dir = "./data"

[service.api]
command = "uv run python app.py"
health = "/health"
isolation = "process"            # "process" (macOS/Linux) or "namespace" (Linux, PID isolation)
idle_timeout = 300               # stop after 5 min idle
startup_timeout = 10             # increase for go run (30s)
storage_persist = true           # keep data across restarts
memory_limit_mb = 256            # cgroups limit (Linux)

[service.api.env]
DATA_DIR = "{data_dir}/{id}"     # {name}, {id}, {data_dir}, {port} all interpolate
```

Full reference at [tenement.dev/guides/03-configuration](https://tenement.dev/guides/03-configuration).

## Examples

The [examples/ directory](examples/) has complete working setups you can run immediately:

- [hello-world](examples/hello-world/) is the simplest possible setup, a bash script and netcat.
- [python-fastapi](examples/python-fastapi/) and [node-fastify](examples/node-fastify/) and [go-http](examples/go-http/) show the same pattern in three languages.
- [multi-runtime](examples/multi-runtime/) runs all three at once with a 56-test integration script that verifies auth, data isolation, and cross-service isolation.
- [auth-test](examples/auth-test/) demonstrates that tenement passes all request headers through untouched, so your app's auth works exactly as it would without tenement.
- [multi-tenant](examples/multi-tenant/) is a per-tenant notes API with SQLite, which is probably closest to what you'd actually build.

## Production

For a Hetzner or DigitalOcean VPS with wildcard HTTPS:

```bash
# Point *.app.example.com at your server IP, then:
cargo install tenement-cli
cd /opt/myapp
ten init --name myapp --command "python3 app.py"
ten token-gen
ten install --domain app.example.com --caddy --dns-provider cloudflare

ten spawn myapp:customer1
ten spawn myapp:customer2
ten ps
```

The `ten install` command creates a systemd service and a Caddyfile with wildcard TLS. Caddy handles HTTPS, tenement handles everything else.

## Development

```bash
cargo test    # 566 tests
cargo bench
```

Full docs at [tenement.dev](https://tenement.dev). See [ROADMAP.md](ROADMAP.md) for what's next.

## License

Apache 2.0
