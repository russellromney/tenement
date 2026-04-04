"""Minimal Python notes API with auth."""

import os
import json
import hashlib
from http.server import HTTPServer, BaseHTTPRequestHandler

PORT = int(os.getenv("PORT", "8000"))
DATA_DIR = os.getenv("DATA_DIR", "./data")
TENANT_ID = os.getenv("TENANT_ID", "unknown")
NOTES_FILE = os.path.join(DATA_DIR, "notes.json")
TOKEN_FILE = os.path.join(DATA_DIR, "token.txt")


def ensure_data_dir():
    os.makedirs(DATA_DIR, exist_ok=True)


def get_token():
    ensure_data_dir()
    if os.path.exists(TOKEN_FILE):
        return open(TOKEN_FILE).read().strip()
    token = hashlib.sha256(f"py-{TENANT_ID}".encode()).hexdigest()[:32]
    open(TOKEN_FILE, "w").write(token)
    return token


def load_notes():
    if not os.path.exists(NOTES_FILE):
        return []
    return json.load(open(NOTES_FILE))


def save_notes(notes):
    ensure_data_dir()
    json.dump(notes, open(NOTES_FILE, "w"))


def check_auth(headers):
    auth = headers.get("Authorization", "")
    if not auth:
        return 401, {"error": "Missing Authorization header"}
    parts = auth.split(" ", 1)
    if len(parts) != 2 or parts[0].lower() != "bearer" or parts[1] != get_token():
        return 403, {"error": "Invalid token"}
    return None, None


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass  # quiet

    def respond(self, code, body):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(body).encode())

    def do_GET(self):
        if self.path == "/health":
            self.respond(200, {"status": "ok", "tenant": TENANT_ID, "runtime": "python"})
        elif self.path == "/token":
            self.respond(200, {"tenant": TENANT_ID, "token": get_token(), "runtime": "python"})
        elif self.path == "/notes":
            err_code, err_body = check_auth(self.headers)
            if err_code:
                self.respond(err_code, err_body)
                return
            self.respond(200, {"tenant": TENANT_ID, "notes": load_notes(), "runtime": "python"})
        else:
            self.respond(404, {"error": "not found"})

    def do_POST(self):
        if self.path == "/notes":
            err_code, err_body = check_auth(self.headers)
            if err_code:
                self.respond(err_code, err_body)
                return
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length))
            notes = load_notes()
            entry = {"id": len(notes) + 1, "text": body["text"]}
            notes.append(entry)
            save_notes(notes)
            self.respond(201, {"tenant": TENANT_ID, "note": entry, "runtime": "python"})
        else:
            self.respond(404, {"error": "not found"})


if __name__ == "__main__":
    print(f"[python:{TENANT_ID}] listening on :{PORT}")
    HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
