# Go net/http Example

A Go HTTP server demonstrating tenement integration with resource limits.

## Prerequisites

- Go 1.21+

## Setup

```bash
go build -o server
```

## Run

```bash
# Start tenement
ten serve --port 8080 --domain localhost

# Spawn instance
ten spawn api --id prod

# Test
curl http://prod.api.localhost:8080/
curl http://prod.api.localhost:8080/health
curl http://prod.api.localhost:8080/items/42
```

## Features Demonstrated

- **Health checks**: `/health` endpoint for tenement monitoring
- **Resource limits**: 64MB memory, 50 CPU shares
- **Compiled binary**: Fast startup, low memory footprint

## Configuration

```toml
[service.api]
command = "./server"
health = "/health"
memory_limit_mb = 64
cpu_shares = 50
```

The `PORT` environment variable is automatically set by tenement.

## Building for Production

```bash
# Build optimized binary
CGO_ENABLED=0 go build -ldflags="-s -w" -o server

# Or for Linux (cross-compile from macOS)
GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -ldflags="-s -w" -o server
```
