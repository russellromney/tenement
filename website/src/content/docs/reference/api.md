---
title: API Reference
description: HTTP API endpoints
---

## Base URL

```
http://localhost:8000
```

## Authentication

All API endpoints (except `/health`, `/metrics`) require bearer token:

```bash
curl -H "Authorization: Bearer YOUR_TOKEN" \
  http://localhost:8000/api/instances
```

## Endpoints

### GET /

Dashboard web UI. View instances, logs, metrics in browser.

### GET /health

Health check endpoint (no auth required).

```bash
curl http://localhost:8000/health

Response: 200 OK
{
  "status": "ok",
  "version": "0.1.0"
}
```

### GET /metrics

Prometheus metrics endpoint (no auth required).

```bash
curl http://localhost:8000/metrics

Response: 200 OK
# HELP tenement_instances_total Total instances spawned
# TYPE tenement_instances_total counter
tenement_instances_total 42
...
```

## Instance Management API

### POST /api/instances

Spawn a new instance.

```bash
curl -X POST http://localhost:8000/api/instances \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "service": "api",
    "id": "user123",
    "env": {
      "CUSTOM_VAR": "value"
    }
  }'

Response: 201 Created
{
  "instance": "api:user123",
  "socket": "/tmp/api-user123.sock",
  "status": "running",
  "uptime_ms": 125
}
```

### GET /api/instances

List all instances.

```bash
curl http://localhost:8000/api/instances \
  -H "Authorization: Bearer TOKEN"

Response: 200 OK
{
  "instances": [
    {
      "instance": "api:user123",
      "service": "api",
      "id": "user123",
      "socket": "/tmp/api-user123.sock",
      "status": "running",
      "uptime_ms": 125456,
      "health": "healthy",
      "restart_count": 0
    },
    ...
  ]
}
```

### GET /api/instances/:service/:id

Get single instance details.

```bash
curl http://localhost:8000/api/instances/api/user123 \
  -H "Authorization: Bearer TOKEN"

Response: 200 OK
{
  "instance": "api:user123",
  "service": "api",
  "id": "user123",
  "socket": "/tmp/api-user123.sock",
  "status": "running",
  "uptime_ms": 125456,
  "health": "healthy",
  "restart_count": 0,
  "created_at": "2024-01-12T10:30:00Z",
  "last_health_check": "2024-01-12T10:31:45Z"
}
```

### POST /api/instances/:service/:id/restart

Restart an instance.

```bash
curl -X POST http://localhost:8000/api/instances/api/user123/restart \
  -H "Authorization: Bearer TOKEN"

Response: 200 OK
{
  "instance": "api:user123",
  "status": "restarting"
}
```

### DELETE /api/instances/:service/:id

Stop an instance.

```bash
curl -X DELETE http://localhost:8000/api/instances/api/user123 \
  -H "Authorization: Bearer TOKEN"

Response: 200 OK
{
  "instance": "api:user123",
  "status": "stopped"
}
```

## Logs API

### GET /api/logs

Get logs (all instances).

```bash
curl "http://localhost:8000/api/logs?limit=50&follow=false" \
  -H "Authorization: Bearer TOKEN"

Query Parameters:
  limit=N         Last N lines (default: 50)
  offset=N        Offset (for pagination)
  service=NAME    Filter by service
  instance=NAME   Filter by instance
  level=LEVEL     Filter: debug, info, warn, error
  grep=PATTERN    Search pattern
  follow=true     Stream logs (WebSocket upgrade)

Response: 200 OK
{
  "logs": [
    {
      "timestamp": "2024-01-12T10:31:45.123Z",
      "instance": "api:user123",
      "level": "info",
      "message": "Request completed"
    },
    ...
  ]
}
```

### GET /api/logs/:service/:id

Get logs for single instance.

```bash
curl "http://localhost:8000/api/logs/api/user123?limit=100" \
  -H "Authorization: Bearer TOKEN"

Response: 200 OK
{
  "instance": "api:user123",
  "logs": [...]
}
```

### WebSocket /api/logs/stream

Stream logs in real-time.

```javascript
const token = "YOUR_TOKEN";
const ws = new WebSocket(`ws://localhost:8000/api/logs/stream`);

ws.onopen = () => {
  ws.send(JSON.stringify({
    type: "auth",
    token: token
  }));

  ws.send(JSON.stringify({
    type: "filter",
    service: "api",
    level: "error"
  }));
};

ws.onmessage = (event) => {
  const log = JSON.parse(event.data);
  console.log(`[${log.instance}] ${log.message}`);
};
```

## Search API

### POST /api/logs/search

Full-text search logs.

```bash
curl -X POST http://localhost:8000/api/logs/search \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "database error",
    "limit": 100,
    "since": "1h"
  }'

Response: 200 OK
{
  "results": [
    {
      "timestamp": "...",
      "instance": "api:user123",
      "message": "database error connecting to pg"
    }
  ],
  "total": 3
}
```

## Metrics API

### GET /api/metrics/:service/:id

Get metrics for single instance.

```bash
curl "http://localhost:8000/api/metrics/api/user123" \
  -H "Authorization: Bearer TOKEN"

Response: 200 OK
{
  "instance": "api:user123",
  "uptime_seconds": 125.456,
  "memory_mb": 120.5,
  "cpu_percent": 12.3,
  "requests_total": 1523,
  "requests_per_second": 12.2,
  "errors_total": 2,
  "error_rate": 0.13,
  "health_checks": {
    "total": 12,
    "passed": 12,
    "failed": 0
  }
}
```

## Config API

### GET /api/config

Get current config.

```bash
curl http://localhost:8000/api/config \
  -H "Authorization: Bearer TOKEN"

Response: 200 OK
{
  "settings": {
    "data_dir": "/var/lib/tenement",
    "health_check_interval": 10,
    ...
  },
  "services": {
    "api": {
      "command": "./api",
      "socket": "/tmp/api-{id}.sock",
      "health": "/health",
      ...
    }
  }
}
```

### GET /api/config/validate

Validate config without applying.

```bash
curl http://localhost:8000/api/config/validate \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d @tenement.toml

Response: 200 OK
{
  "valid": true,
  "warnings": []
}

Or: 400 Bad Request
{
  "valid": false,
  "errors": ["Service 'api' missing required field: command"]
}
```

## Auth API

### POST /api/tokens

Generate new auth token.

```bash
curl -X POST http://localhost:8000/api/tokens \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "CI/CD",
    "expires_in_days": 30
  }'

Response: 201 Created
{
  "token": "tenement_abc123xyz...",
  "name": "CI/CD",
  "created_at": "2024-01-12T10:30:00Z",
  "expires_at": "2024-02-11T10:30:00Z"
}
```

### GET /api/tokens

List all tokens.

```bash
curl http://localhost:8000/api/tokens \
  -H "Authorization: Bearer TOKEN"

Response: 200 OK
{
  "tokens": [
    {
      "id": "...",
      "name": "CI/CD",
      "created_at": "2024-01-12T10:30:00Z",
      "last_used": "2024-01-12T10:31:00Z",
      "expires_at": "2024-02-11T10:30:00Z"
    }
  ]
}
```

### DELETE /api/tokens/:id

Revoke token.

```bash
curl -X DELETE http://localhost:8000/api/tokens/token-id \
  -H "Authorization: Bearer TOKEN"

Response: 204 No Content
```

## Error Responses

All errors follow this format:

```json
{
  "error": "instance_not_found",
  "message": "Instance 'api:user123' not found",
  "status": 404
}
```

Common status codes:

| Code | Error | Cause |
|------|-------|-------|
| 400 | bad_request | Invalid parameters |
| 401 | unauthorized | Missing/invalid token |
| 403 | forbidden | Token lacks permission |
| 404 | not_found | Instance/resource doesn't exist |
| 409 | conflict | Instance already running |
| 422 | unprocessable_entity | Config validation error |
| 500 | internal_error | Server error |
| 503 | service_unavailable | Temporarily unavailable |

## Rate Limiting

API is rate-limited per token:
- 1000 requests per minute
- 10000 requests per hour

Responses include:

```
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 999
X-RateLimit-Reset: 1704976800
```

## Pagination

List endpoints support pagination:

```bash
curl "http://localhost:8000/api/instances?limit=20&offset=40" \
  -H "Authorization: Bearer TOKEN"

Response includes:
{
  "instances": [...],
  "pagination": {
    "limit": 20,
    "offset": 40,
    "total": 150,
    "has_more": true
  }
}
```

## Next Steps

- [CLI Reference](/reference/cli) - Command-line interface
- [Getting Started](/guides/getting-started) - Setup guide
