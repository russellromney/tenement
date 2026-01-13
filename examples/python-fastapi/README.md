# Python FastAPI Example

A FastAPI application demonstrating tenement integration.

## Prerequisites

- Python 3.9+
- [uv](https://github.com/astral-sh/uv) (recommended) or pip

## Setup

```bash
# Using uv (recommended)
uv sync

# Or using pip
pip install fastapi uvicorn
```

## Run

```bash
# Start tenement
ten serve --port 8080 --domain localhost

# Spawn instances
ten spawn api --id prod
ten spawn api --id staging

# Test
curl http://prod.api.localhost:8080/
curl http://prod.api.localhost:8080/health
curl http://prod.api.localhost:8080/items/42
```

## Features Demonstrated

- **Health checks**: `/health` endpoint for tenement monitoring
- **Per-instance data**: Each instance gets its own database path via `{data_dir}/{id}`
- **Idle timeout**: Instances stop after 5 minutes of inactivity
- **Auto-restart**: Failed instances are automatically restarted

## Configuration

```toml
[service.api]
command = "uv run python app.py"
health = "/health"
idle_timeout = 300

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
```

The `PORT` environment variable is automatically set by tenement.
