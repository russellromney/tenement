"""Per-tenant notes API backed by SQLite.

Each tenant gets their own process, their own database file, and their own
subdomain. Write single-tenant code, deploy it for 100+ customers on one VPS.

Environment variables (set by tenement):
  PORT       - TCP port to listen on
  DATA_DIR   - Directory for this tenant's data (unique per tenant)
"""

import json
import os
import sqlite3
from http.server import HTTPServer, BaseHTTPRequestHandler

PORT = int(os.environ.get("PORT", "8000"))
DATA_DIR = os.environ.get("DATA_DIR", ".")
DB_PATH = os.path.join(DATA_DIR, "notes.db")


def get_db():
    os.makedirs(DATA_DIR, exist_ok=True)
    db = sqlite3.connect(DB_PATH)
    db.execute(
        "CREATE TABLE IF NOT EXISTS notes "
        "(id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)"
    )
    return db


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            self.respond(200, {"status": "ok"})
        elif self.path == "/notes":
            db = get_db()
            rows = db.execute("SELECT id, text, created_at FROM notes ORDER BY id DESC").fetchall()
            notes = [{"id": r[0], "text": r[1], "created_at": r[2]} for r in rows]
            db.close()
            self.respond(200, notes)
        else:
            self.respond(200, {
                "service": "tenant-notes",
                "database": DB_PATH,
                "endpoints": ["GET /notes", "POST /notes", "GET /health"],
            })

    def do_POST(self):
        if self.path == "/notes":
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length)) if length else {}
            text = body.get("text", "")
            if not text:
                self.respond(400, {"error": "text is required"})
                return
            db = get_db()
            cursor = db.execute("INSERT INTO notes (text) VALUES (?)", (text,))
            db.commit()
            note_id = cursor.lastrowid
            db.close()
            self.respond(201, {"id": note_id, "text": text})
        else:
            self.respond(404, {"error": "not found"})

    def respond(self, code, data):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def log_message(self, format, *args):
        pass  # Silence request logs


if __name__ == "__main__":
    server = HTTPServer(("127.0.0.1", PORT), Handler)
    print(f"Tenant notes API listening on port {PORT}, db at {DB_PATH}")
    server.serve_forever()
