---
title: Quick Start
description: Get tenement running in 5 minutes
---

## Install

```bash
cargo install tenement-cli
ten --version
```

## Write an app

Your app needs to do two things: listen on the port in the `PORT` environment variable, and serve a health endpoint that returns HTTP 200. That's the whole contract. Any language works.

Here's a notes API in Python. It doesn't know anything about tenants. It just reads `PORT` and `DATA_DIR` from the environment, serves notes from a SQLite database, and returns 200 at `/health`.

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

Bind to `127.0.0.1`, not `0.0.0.0`. tenement handles external access.

## Configure

The config tells tenement how to run your app. The `command` field is shell-split automatically, so `"uv run python app.py"` works the way you'd expect. The `{data_dir}` and `{id}` in the env section get replaced with real values when tenement spawns an instance.

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

Setting `isolation = "process"` works on both macOS and Linux. On Linux in production, you'd use `"namespace"` for PID isolation at zero overhead.

## Run

Start the server and spawn a couple tenants:

```bash
ten serve --port 8080 --domain localhost
ten token-gen
ten spawn notes:alice
ten spawn notes:bob
```

Each tenant gets their own process, their own data directory, and their own SQLite database. You can verify they're isolated:

```bash
curl -X POST http://alice.notes.localhost:8080/notes \
  -H "Content-Type: application/json" -d '{"text":"hello from alice"}'

curl http://alice.notes.localhost:8080/notes
# [{"id": 1, "text": "hello from alice"}]

curl http://bob.notes.localhost:8080/notes
# []  (bob has a completely separate database)

ten ps
# INSTANCE        LISTEN              UPTIME   HEALTH   WEIGHT
# notes:alice     127.0.0.1:30000     15s      healthy  100
# notes:bob       127.0.0.1:30001     12s      healthy  100
```

After 5 minutes with no requests, tenement kills the process. The next request spawns a new one in under a second. The database file is still there because we set `storage_persist = true` by default.

## Skip --server on every command

If you're running on a non-default port, set `TENEMENT_SERVER` once instead of passing `--server` every time:

```bash
export TENEMENT_SERVER=http://localhost:9090
ten ps
ten spawn notes:carol
ten logs notes:carol
```

## More examples

The [examples directory](https://github.com/russellromney/tenement/tree/main/examples) has the same pattern in Python, Node.js, and Go, plus a [multi-runtime example](https://github.com/russellromney/tenement/tree/main/examples/multi-runtime) that runs all three simultaneously with a 56-test integration script. The [auth-test example](https://github.com/russellromney/tenement/tree/main/examples/auth-test) demonstrates that tenement proxies all headers through unchanged, so your app's own auth works exactly as it would without tenement.

## Next

- [Why tenement?](/intro/02-economics) explains the economics of running mostly-idle tenants on one machine.
- [Concepts](/intro/03-concepts) covers the architecture, instance lifecycle, and auth model.
- [Configuration](/guides/03-configuration) is the full TOML reference.
- [Production](/guides/04-production) covers TLS and systemd for real deployments.
