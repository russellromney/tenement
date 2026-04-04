---
title: Quick Start
description: Get tenement running in 5 minutes
---

## Install

```bash
cargo install tenement-cli
ten --version
```

## Your app's contract

Your app needs to do two things:

1. **Listen on `PORT`** (read from environment, don't hardcode)
2. **Serve a health endpoint** (return HTTP 200 at `/health` or your configured path)

Bind to `127.0.0.1`, not `0.0.0.0`. tenement handles external access.

Any language works. tenement doesn't care what's behind the port.

## Example: Python notes API

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

## Configure

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

**Notes:**
- `command` is shell-split automatically. `"uv run python app.py"` works as expected.
- `isolation = "process"` works on macOS and Linux. Use `"namespace"` on Linux for PID isolation.
- `{data_dir}` and `{id}` are interpolated at spawn time.

## Run

```bash
# Terminal 1: start the server
ten serve --port 8080 --domain localhost

# Terminal 2: generate an API token, spawn instances
ten token-gen
ten spawn notes:alice
ten spawn notes:bob

# Test: each tenant has separate data
curl -X POST http://alice.notes.localhost:8080/notes \
  -H "Content-Type: application/json" -d '{"text":"hello from alice"}'

curl http://alice.notes.localhost:8080/notes   # alice's notes
curl http://bob.notes.localhost:8080/notes     # bob's notes (empty)

# Manage
ten ps                      # list instances with health status
ten logs notes:alice        # tail alice's logs
ten stop notes:alice        # stop alice
```

After 5 minutes idle, tenement stops the process. The next request wakes it automatically (sub-second).

## Set TENEMENT_SERVER to skip --server

If running on a non-default port:

```bash
export TENEMENT_SERVER=http://localhost:9090
ten ps          # no --server needed
ten spawn notes:carol
```

## More examples

See the [examples directory](https://github.com/russellromney/tenement/tree/main/examples) for complete working setups in Python, Node.js, Go, and multi-runtime configurations.

The [multi-runtime example](https://github.com/russellromney/tenement/tree/main/examples/multi-runtime) includes a 56-test integration script covering auth, data isolation, and cross-service isolation.

## Next steps

- [Why tenement?](/intro/02-economics) - The problem it solves
- [Concepts](/intro/03-concepts) - Architecture and terminology
- [Configuration](/guides/03-configuration) - Full config reference
- [Production](/guides/04-production) - TLS and systemd setup
