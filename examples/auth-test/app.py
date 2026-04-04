"""Simple notes API with per-tenant auth. Each tenant has its own token and data."""

import os
import json
import hashlib
from fastapi import FastAPI, Header, HTTPException
from pydantic import BaseModel
import uvicorn

PORT = int(os.getenv("PORT", "8000"))
DATA_DIR = os.getenv("DATA_DIR", "./data")
TENANT_ID = os.getenv("TENANT_ID", "unknown")

app = FastAPI(title=f"Notes API ({TENANT_ID})")

NOTES_FILE = os.path.join(DATA_DIR, "notes.json")
TOKEN_FILE = os.path.join(DATA_DIR, "token.txt")


def _ensure_data_dir():
    os.makedirs(DATA_DIR, exist_ok=True)


def _load_notes() -> list[dict]:
    if not os.path.exists(NOTES_FILE):
        return []
    with open(NOTES_FILE) as f:
        return json.load(f)


def _save_notes(notes: list[dict]):
    _ensure_data_dir()
    with open(NOTES_FILE, "w") as f:
        json.dump(notes, f)


def _get_token() -> str:
    """Each tenant gets a deterministic token derived from their ID."""
    _ensure_data_dir()
    if os.path.exists(TOKEN_FILE):
        with open(TOKEN_FILE) as f:
            return f.read().strip()
    # Generate a simple token on first run
    token = hashlib.sha256(f"secret-{TENANT_ID}".encode()).hexdigest()[:32]
    with open(TOKEN_FILE, "w") as f:
        f.write(token)
    return token


def _require_auth(authorization: str | None):
    if not authorization:
        raise HTTPException(status_code=401, detail="Missing Authorization header")
    parts = authorization.split(" ", 1)
    if len(parts) != 2 or parts[0].lower() != "bearer":
        raise HTTPException(status_code=401, detail="Expected: Bearer <token>")
    if parts[1] != _get_token():
        raise HTTPException(status_code=403, detail="Invalid token")


class NoteCreate(BaseModel):
    text: str


@app.get("/health")
def health():
    return {"status": "ok", "tenant": TENANT_ID}


@app.get("/whoami")
def whoami(authorization: str | None = Header(default=None)):
    _require_auth(authorization)
    return {"tenant": TENANT_ID, "data_dir": DATA_DIR}


@app.get("/token")
def get_token():
    """Show this tenant's token (for demo purposes only)."""
    return {"tenant": TENANT_ID, "token": _get_token()}


@app.get("/notes")
def list_notes(authorization: str | None = Header(default=None)):
    _require_auth(authorization)
    return {"tenant": TENANT_ID, "notes": _load_notes()}


@app.post("/notes")
def create_note(note: NoteCreate, authorization: str | None = Header(default=None)):
    _require_auth(authorization)
    notes = _load_notes()
    entry = {"id": len(notes) + 1, "text": note.text}
    notes.append(entry)
    _save_notes(notes)
    return {"tenant": TENANT_ID, "note": entry}


if __name__ == "__main__":
    print(f"[{TENANT_ID}] Starting on port {PORT}, data at {DATA_DIR}")
    uvicorn.run(app, host="127.0.0.1", port=PORT)
