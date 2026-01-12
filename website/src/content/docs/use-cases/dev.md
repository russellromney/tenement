---
title: Dev Environments
description: Fast iteration with instant restarts
---

Use tenement for local development with fast, reliable process management.

## Local Dev Setup

```toml
[settings]
data_dir = "/tmp/myapp"

[service.api]
command = "cargo run -- --port 3000"
socket = "/tmp/api-dev.sock"
health = "/health"
restart = "on-failure"
isolation = "process"       # Debugging mode - no isolation
```

## Hot Reload

### Option 1: Automatic with File Watcher

```bash
#!/bin/bash
# watch-and-restart.sh

while true; do
    cargo build 2>&1 | grep -q "Finished" && {
        tenement restart api:dev
        echo "✓ Restarted api:dev"
    }
    inotifywait -r -e modify src/
done
```

Run in background:

```bash
./watch-and-restart.sh &
```

Now every time you save a file, the service is rebuilt and restarted.

### Option 2: Manual Restart

```bash
# After editing code
cargo build
tenement restart api:dev
```

### Option 3: Live Reload (Built-in)

Some frameworks support hot reload:

```toml
[service.api]
command = "cargo watch -q -c -w src -x run"  # cargo-watch
socket = "/tmp/api-dev.sock"
health = "/health"
```

## Development Workflow

### Setup

```bash
# Install tenement
cargo install tenement-cli

# Create config
cat > tenement.toml << EOF
[service.api]
command = "cargo run"
socket = "/tmp/api-dev.sock"
health = "/health"
restart = "on-failure"
EOF

# Spawn dev instance
tenement spawn api --id dev
```

### Edit → Test → View Logs

```bash
# View logs in real-time
tenement logs api:dev -f

# See metrics
tenement metrics api:dev

# Check status
tenement health api:dev
```

### Testing

```bash
# Run tests in a separate instance
tenement spawn api --id test
curl http://unix:/tmp/api-test.sock/test/run

# Keep your dev instance untouched
tenement health api:dev  # Still running
```

## Multiple Services

Dev environment with API + Database + Cache:

```toml
[service.api]
command = "cargo run"
socket = "/tmp/api.sock"
health = "/health"

[service.db]
command = "sqlite3 /tmp/dev.db"
socket = "/tmp/db.sock"
# (Or postgres, mysql, etc.)

[service.redis]
command = "redis-server --unixsocket /tmp/redis.sock"
socket = "/tmp/redis.sock"
```

Start all:

```bash
tenement spawn api --id dev
tenement spawn db --id dev
tenement spawn redis --id dev

# View all
tenement ps
```

## Team Development

Share development environment:

```bash
# On shared machine:
tenement serve --port 8000 --domain localhost

# Developer 1:
ssh shared-machine tenement spawn api --id alice
curl http://localhost:8000/api/instances

# Developer 2:
ssh shared-machine tenement spawn api --id bob
curl http://localhost:8000/api/instances
```

Each developer gets isolated instances + dashboard.

## Debugging

### Bare Process Mode

No isolation for easier debugging:

```toml
[service.api]
command = "cargo run"
isolation = "process"  # Can inspect all processes
```

### View Process Tree

```bash
# See what's running
tenement ps

# View actual process
ps aux | grep "cargo run"

# Attach debugger
gdb -p $(pgrep cargo)

# Or with VSCode/Rust Analyzer directly
```

### Logs and Metrics

```bash
# View service logs
tenement logs api:dev

# Tail logs
tenement logs api:dev -f

# Filter by level
tenement logs api:dev | grep ERROR

# Export logs
tenement logs api:dev > debug.log
```

## Integration Tests

Run tests in isolated instances:

```bash
#!/bin/bash
# test.sh

# Setup
tenement spawn api --id test
sleep 2  # Wait for startup

# Run tests
curl -X POST http://unix:/tmp/api-test.sock/api/test/run
result=$?

# Cleanup
tenement stop api:test

exit $result
```

```toml
[service.api]
command = "cargo run --features test-mode"
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "never"  # Don't restart on failure (let test fail)
isolation = "process"
```

## CI/CD Integration

Use tenement in CI pipeline:

```yaml
# .github/workflows/test.yml
name: Test

on: [push]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Install tenement
        run: cargo install tenement-cli

      - name: Create config
        run: |
          cat > tenement.toml << EOF
          [service.api]
          command = "cargo test"
          socket = "/tmp/api-ci.sock"
          EOF

      - name: Run tests
        run: |
          tenement spawn api --id ci
          sleep 5
          tenement health api:ci
          tenement stop api:ci
```

## Comparison: Dev Workflows

### Traditional (no tool)

```bash
# Terminal 1
cargo run &

# Terminal 2
# Edit code... Ctrl+C, up arrow, Enter to restart
# Lose previous output
```

### With tenement

```bash
# Terminal 1
tenement serve

# Terminal 2 (during editing)
tenement logs api:dev -f
# Logs continue, see restarts clearly

# Terminal 3
# Your editor, no manual restarts
# Auto-restart on changes (with watch script)
```

## Next Steps

- [Quick Start](/intro/quick-start) - Get running
- [Configuration](/guides/configuration) - All options
- [Getting Started](/guides/getting-started) - Detailed setup
