---
title: Migrating from Docker
description: Moving containerized apps to tenement
---

If you're coming from Docker, this guide helps you translate concepts and migrate your workloads.

## Concept Mapping

| Docker | tenement | Notes |
|--------|----------|-------|
| Dockerfile | Your app code | No container images needed |
| docker-compose.yml | tenement.toml | Similar structure, simpler |
| Container | Instance | Running process |
| Image | Command | Just the executable |
| Volume | data_dir | Persistent storage |
| Network | (automatic) | tenement handles routing |
| Port mapping | PORT env var | Auto-allocated |
| Health check | health = "/health" | Same concept |

## Migration Steps

### 1. Extract Your App

From Docker:
```dockerfile
FROM python:3.11
WORKDIR /app
COPY . .
RUN pip install -r requirements.txt
CMD ["python", "app.py"]
```

To tenement:
```bash
# Just run directly with your package manager
uv run python app.py
# or: bun run server.ts
# or: ./compiled-binary
```

**Key point:** No container image. Your app runs directly on the host with its dependencies.

### 2. Convert docker-compose.yml

From docker-compose:
```yaml
version: "3"
services:
  api:
    build: .
    ports:
      - "3000:3000"
    environment:
      - DATABASE_URL=postgres://...
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/health"]
    deploy:
      resources:
        limits:
          memory: 256M
```

To tenement.toml:
```toml
[service.api]
command = "uv run python app.py"
health = "/health"
memory_limit_mb = 256

[service.api.env]
DATABASE_URL = "postgres://..."
```

### 3. Update Your App

**Before (Docker):** Hardcoded port
```python
app.run(host="0.0.0.0", port=3000)
```

**After (tenement):** Read from PORT
```python
import os
port = int(os.getenv("PORT", "3000"))
app.run(host="127.0.0.1", port=port)
```

**Key changes:**
1. Read `PORT` from environment (tenement sets it)
2. Bind to `127.0.0.1` not `0.0.0.0` (tenement handles external access)

## What You Lose

| Docker Feature | tenement Alternative |
|----------------|---------------------|
| Container images | Pre-install deps, use uv/bun/nix |
| Image registry | Deploy binaries directly |
| Layer caching | Not needed (no build step) |
| Compose networks | Localhost + routing |
| `docker exec` | `ten health`, API logs |
| Multi-arch images | Build for target arch |

## What You Gain

| Benefit | Why |
|---------|-----|
| **Faster startup** | No container overhead |
| **Less memory** | ~20MB vs ~50-100MB per container |
| **Simpler debugging** | No container abstraction |
| **Native performance** | Syscalls go directly to kernel |
| **Smaller footprint** | No Docker daemon |
| **Scale-to-zero** | Built-in idle timeout |

## Common Patterns

### Multi-Service (formerly docker-compose)

```yaml
# docker-compose.yml
services:
  api:
    build: ./api
    ports: ["3000:3000"]
  worker:
    build: ./worker
```

```toml
# tenement.toml
[service.api]
command = "uv run python api/app.py"
health = "/health"

[service.worker]
command = "./worker/worker"
health = "/health"

[instances]
api = ["prod"]
worker = ["bg-1", "bg-2"]
```

### Environment Files

```yaml
# docker-compose.yml
services:
  api:
    env_file: .env
```

```toml
# tenement.toml
[service.api.env]
DATABASE_URL = "${DATABASE_URL}"  # From host environment
SECRET_KEY = "${SECRET_KEY}"
```

Load with: `source .env && ten serve`

### Volumes / Persistent Data

```yaml
# docker-compose.yml
services:
  api:
    volumes:
      - ./data:/app/data
```

```toml
# tenement.toml
[settings]
data_dir = "./data"

[service.api]
storage_persist = true

[service.api.env]
DATA_PATH = "{data_dir}/{id}"
```

## Deployment Workflow

### Docker
```bash
docker build -t myapp .
docker push registry.example.com/myapp:v2
docker-compose up -d
```

### tenement
```bash
# Build locally (if compiled)
cargo build --release
# or just deploy code directly

# Deploy
rsync -avz ./app server:/opt/myapp/
ssh server "ten deploy api --version v2"
```

## FAQ

**Q: Do I need to rewrite my Dockerfile?**
A: No Dockerfile needed. Just ensure your app reads `PORT` from environment.

**Q: What about my CI/CD pipeline?**
A: Skip the image build step. Deploy code directly via rsync, git pull, or your preferred method.

**Q: Can I use container images with tenement?**
A: No. tenement runs processes, not containers. Extract your app from the container.

**Q: What about Docker secrets?**
A: Use environment variables. For production, inject secrets at runtime (Vault, Doppler, etc.).

## Next Steps

- [Quick Start](/intro/01-quick-start) - Get running in 5 minutes
- [Configuration Reference](/guides/03-configuration) - Full config options
- [Production Deployment](/guides/04-production) - TLS and systemd setup
