---
title: Upgrading tenement
description: Zero-downtime upgrade procedures
---

This guide covers upgrading tenement with minimal or zero downtime.

## Quick Upgrade (With Downtime)

For development or when brief downtime is acceptable:

```bash
# Stop tenement
systemctl stop tenement

# Upgrade binary
cargo install tenement-cli

# Start tenement
systemctl start tenement
```

Downtime: ~10-30 seconds depending on instance count.

## Zero-Downtime Upgrade

For production deployments requiring continuous availability.

### Prerequisites

- Two tenement binaries (old and new)
- Socket-based communication (Caddy/nginx proxying)

### Procedure

**1. Download new binary**

```bash
# Download to temp location
cargo install tenement-cli --root /tmp/tenement-new
NEW_BIN="/tmp/tenement-new/bin/ten"
```

**2. Test new binary**

```bash
# Verify it runs
$NEW_BIN --version

# Test config parsing
$NEW_BIN config
```

**3. Graceful switchover**

```bash
# Get current PID
OLD_PID=$(pgrep -f "ten serve")

# Start new instance on different port
$NEW_BIN serve --port 8081 --domain $DOMAIN &
NEW_PID=$!

# Wait for new instance to be ready
sleep 5
curl -f http://localhost:8081/health || exit 1

# Update Caddy/nginx to point to new port
# (Edit Caddyfile and reload)
caddy reload

# Stop old instance
kill -TERM $OLD_PID

# Move new binary to standard location
mv $NEW_BIN /usr/local/bin/ten

# Update systemd to use standard location
systemctl daemon-reload
```

### Simpler Alternative: Systemd Socket Activation

If using systemd socket activation (advanced setup):

```bash
# Systemd handles the socket, just restart the service
systemctl restart tenement
```

Downtime: ~1-2 seconds (requests queue during restart).

## Version Compatibility

### Config File Compatibility

tenement maintains backwards compatibility for config files:
- `runtime` field still works (alias for `isolation`)
- New fields have sensible defaults

### Database Compatibility

The SQLite database schema is versioned:
- Upgrades run migrations automatically
- Downgrades may lose new features' data
- Backup before major version upgrades

### API Compatibility

REST API is stable within major versions:
- New endpoints may be added
- Existing endpoints maintain response format
- Breaking changes only in major versions

## Rollback Procedure

If the new version has issues:

```bash
# Stop new version
systemctl stop tenement

# Restore old binary
mv /usr/local/bin/ten.bak /usr/local/bin/ten

# Start old version
systemctl start tenement
```

**Tip:** Keep the previous binary around for quick rollbacks:

```bash
# Before upgrade
cp /usr/local/bin/ten /usr/local/bin/ten.bak
```

## Checking Current Version

```bash
ten --version
```

## Changelog

Check [CHANGELOG.md](https://github.com/russellromney/tenement/blob/main/CHANGELOG.md) for:
- Breaking changes
- New features
- Bug fixes
- Migration guides

## Next Steps

- [Backup and Restore](/guides/08-backup) - Data preservation
- [Monitoring Setup](/guides/09-monitoring) - Observability
- [Troubleshooting](/reference/troubleshooting) - Common issues
