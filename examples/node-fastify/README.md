# Node.js Fastify Example

A Fastify application demonstrating tenement integration.

## Prerequisites

- Node.js 18+
- npm, pnpm, or bun

## Setup

```bash
npm install
# or: bun install
```

## Run

```bash
# Start tenement
ten serve --port 8080 --domain localhost

# Spawn instance
ten spawn web --id prod

# Test
curl http://prod.web.localhost:8080/
curl http://prod.web.localhost:8080/health
curl http://prod.web.localhost:8080/users/42
```

## Features Demonstrated

- **Health checks**: `/health` endpoint for tenement monitoring
- **Memory limits**: Instance limited to 128MB via cgroups
- **Environment variables**: `NODE_ENV` and `PORT` configured per-instance

## Configuration

```toml
[service.web]
command = "node server.js"
health = "/health"
memory_limit_mb = 128

[service.web.env]
NODE_ENV = "production"
```

The `PORT` environment variable is automatically set by tenement.
