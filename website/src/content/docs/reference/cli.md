---
title: CLI Reference
description: Complete tenement command-line interface
---

## Global Options

```bash
tenement [OPTIONS] [COMMAND]

Options:
  --config <FILE>      Config file path (default: tenement.toml)
  --data-dir <PATH>    Data directory (overrides config)
  --verbose, -v        Verbose output
  --quiet, -q          Quiet output
  --help, -h           Show help
  --version, -V        Show version
```

## Commands

### serve

Start the tenement server and HTTP API.

```bash
tenement serve [OPTIONS]

Options:
  --port <PORT>         HTTP port (default: 8000)
  --domain <DOMAIN>     Base domain for routing (default: localhost)
  --host <HOST>         Bind address (default: 0.0.0.0)
  --token <TOKEN>       API auth token (generate if not provided)
```

Example:

```bash
tenement serve --port 8080 --domain example.com
```

### spawn

Spawn a new instance of a service.

```bash
tenement spawn <SERVICE> --id <ID> [OPTIONS]

Arguments:
  <SERVICE>  Service name from config

Options:
  --id <ID>             Instance ID (required)
  --env <KEY=VALUE>     Override env variables
  --restart             Restart if already exists
  --wait                Wait for health check
  --timeout <SECS>      Startup timeout (default: 10)
```

Examples:

```bash
# Basic spawn
tenement spawn api --id user123

# With custom env
tenement spawn api --id user123 --env DATABASE_PATH=/custom/path

# Wait for startup
tenement spawn api --id user123 --wait --timeout 20

# Restart if exists
tenement spawn api --id user123 --restart
```

### stop

Stop a running instance.

```bash
tenement stop <INSTANCE>

Arguments:
  <INSTANCE>  Instance name (format: service:id)
```

Examples:

```bash
tenement stop api:user123
tenement stop worker:background-1
```

### restart

Restart a running instance.

```bash
tenement restart <INSTANCE> [OPTIONS]

Arguments:
  <INSTANCE>  Instance name

Options:
  --timeout <SECS>      Startup timeout (default: 10)
  --wait                Wait for health check
```

Examples:

```bash
tenement restart api:user123
tenement restart api:user123 --wait
```

### ps

List all running instances.

```bash
tenement ps [OPTIONS]

Options:
  --service <NAME>      Filter by service
  --format <FORMAT>     Output format: table (default), json, csv
  --watch, -w           Watch for changes
  --no-header           Hide column headers
```

Examples:

```bash
# List all
tenement ps

# Watch for changes
tenement ps -w

# Filter by service
tenement ps --service api

# JSON output
tenement ps --format json | jq .

# CSV output
tenement ps --format csv > instances.csv
```

Output:

```
INSTANCE             SOCKET                         UPTIME     HEALTH     RESTARTS
api:user123          /tmp/api-user123.sock          2m 30s     healthy    0
api:user456          /tmp/api-user456.sock          5m 15s     healthy    1
worker:job1          /tmp/worker-job1.sock          10m        unhealthy  3
```

### health

Check health of a specific instance.

```bash
tenement health <INSTANCE>

Arguments:
  <INSTANCE>  Instance name
```

Examples:

```bash
tenement health api:user123
# Output: api:user123: healthy

tenement health worker:job1
# Output: worker:job1: unhealthy (restart count: 3/3)
```

### logs

View instance logs.

```bash
tenement logs [OPTIONS] [INSTANCE]

Arguments:
  [INSTANCE]  Instance name (optional, defaults to all)

Options:
  --follow, -f          Follow log stream (like tail -f)
  --lines <N>           Last N lines (default: 50)
  --since <TIME>        Logs since time (e.g., "5m", "1h")
  --grep <PATTERN>      Filter by pattern
  --level <LEVEL>       Filter by level: debug, info, warn, error
  --json                Output as JSON
  --service <NAME>      Filter by service
```

Examples:

```bash
# Last 50 lines
tenement logs api:user123

# Last 100 lines
tenement logs api:user123 --lines 100

# Follow logs
tenement logs api:user123 -f

# Last 5 minutes
tenement logs api:user123 --since 5m

# Only errors
tenement logs api:user123 --level error

# Search pattern
tenement logs api:user123 --grep "database"

# All API service logs
tenement logs --service api

# JSON format
tenement logs api:user123 --json
```

### metrics

Show Prometheus metrics.

```bash
tenement metrics [OPTIONS] [INSTANCE]

Arguments:
  [INSTANCE]  Instance name (optional)

Options:
  --format <FORMAT>     Format: prometheus (default), json
  --interval <SECS>     Update interval for watch
  --watch, -w           Watch for changes
```

Examples:

```bash
# Show metrics
tenement metrics

# Watch metrics
tenement metrics -w

# Single instance
tenement metrics api:user123

# JSON format
tenement metrics --format json | jq .
```

### config

Show current configuration.

```bash
tenement config [OPTIONS]

Options:
  --service <NAME>      Show single service config
  --json                Output as JSON
  --validate            Validate config
```

Examples:

```bash
# Show full config
tenement config

# Show API service config
tenement config --service api

# JSON format
tenement config --json

# Validate config
tenement config --validate
```

### token-gen

Generate API authentication token.

```bash
tenement token-gen [OPTIONS]

Options:
  --name <NAME>         Token name/description
  --expires <DAYS>      Expiration (days from now)
  --save                Save to config file
```

Examples:

```bash
# Generate token
tenement token-gen --name "CI/CD"

# With expiration
tenement token-gen --name "Temporary" --expires 7

# Save to config
tenement token-gen --save
```

Output:

```
Generated token: tenement_abc123xyz...
Keep this safe! You won't be able to see it again.
Use in requests: Authorization: Bearer tenement_abc123xyz...
```

### status

Show server status.

```bash
tenement status

Output:
Version: 0.1.0
Uptime: 2 days, 3 hours
Instances: 42 running / 150 total
Memory: 512MB / 1GB available
Requests (24h): 1.2M
Health check interval: 10s
```

### shell

Interactive shell for common commands.

```bash
tenement shell

Commands:
  spawn api --id <id>
  stop <instance>
  restart <instance>
  ps
  health <instance>
  logs [instance]
  metrics
  help
  exit
```

## Authentication

When `serve` is running with a token:

```bash
# Set auth token
export TENEMENT_TOKEN="your-token-here"

# Or use in requests
curl -H "Authorization: Bearer your-token-here" \
  http://localhost:8000/api/instances
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Config error |
| 3 | Instance not found |
| 4 | Already running |
| 5 | Timeout |
| 6 | Permission denied |
| 127 | Command not found |

## Examples

### Multi-tenant SaaS

```bash
# Spawn per customer (via API or script)
for customer in $(get-customers); do
  tenement spawn api --id $customer.id --wait
done

# View all
tenement ps

# Stop expired
for expired in $(get-expired-customers); do
  tenement stop api:$expired
done
```

### Development

```bash
# Start dev instance
tenement spawn api --id dev --wait

# Watch logs while developing
tenement logs api:dev -f &

# Edit code...
# (auto-restart on changes with watch script)

# Check health
tenement health api:dev

# Stop when done
tenement stop api:dev
```

### Monitoring

```bash
# Check all instances
while true; do
  tenement ps
  tenement metrics
  sleep 60
done
```

## Next Steps

- [API Reference](/reference/api) - HTTP API
- [Configuration](/guides/configuration) - Config options
