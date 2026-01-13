"""FastAPI app for tenement example."""

import os
from fastapi import FastAPI
import uvicorn

app = FastAPI(title="Tenement FastAPI Example")

# Get configuration from environment
DATABASE_PATH = os.getenv("DATABASE_PATH", "./app.db")
PORT = int(os.getenv("PORT", "8000"))


@app.get("/")
def root():
    """Root endpoint."""
    return {"message": "Hello from FastAPI!", "database": DATABASE_PATH}


@app.get("/health")
def health():
    """Health check endpoint for tenement."""
    return {"status": "ok"}


@app.get("/items/{item_id}")
def get_item(item_id: int):
    """Example endpoint."""
    return {"item_id": item_id, "name": f"Item {item_id}"}


if __name__ == "__main__":
    print(f"Starting server on port {PORT}")
    print(f"Database path: {DATABASE_PATH}")
    uvicorn.run(app, host="127.0.0.1", port=PORT)
