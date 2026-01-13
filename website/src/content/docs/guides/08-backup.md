---
title: Backup and Restore
description: Preserving and recovering tenement state
---

This guide covers what state to backup and how to restore it.

## What to Backup

### Critical (Must Backup)

| Item | Location | Purpose |
|------|----------|---------|
| **tenement.toml** | Project root | Service configuration |
| **Instance data** | `{data_dir}/{service}/{id}/` | Per-instance persistent data |
| **Auth tokens** | In SQLite DB | API authentication |

### Optional (Nice to Have)

| Item | Location | Purpose |
|------|----------|---------|
| **SQLite database** | `{data_dir}/tenement.db` | Instance metadata, logs |
| **ACME certificates** | `{data_dir}/acme/` | TLS certificates (auto-renewed) |

### Not Needed

| Item | Why |
|------|-----|
| **Logs** | Stored in memory, ship to external service |
| **Metrics** | Ephemeral, stored in external Prometheus |
| **Sockets** | Recreated on startup |

## Backup Procedure

### Simple: Full Directory Backup

```bash
# Stop tenement (for consistency)
systemctl stop tenement

# Backup everything
tar -czvf tenement-backup-$(date +%Y%m%d).tar.gz \
  /etc/tenement/tenement.toml \
  /var/lib/tenement/

# Restart
systemctl start tenement
```

### Online: Hot Backup

For minimal downtime, backup while running:

```bash
# Backup config (always safe)
cp /etc/tenement/tenement.toml ~/backups/

# Backup SQLite with sqlite3 backup command
sqlite3 /var/lib/tenement/tenement.db ".backup ~/backups/tenement.db"

# Backup instance data (may be inconsistent if actively writing)
rsync -av /var/lib/tenement/ ~/backups/tenement-data/
```

**Note:** Instance data backup may be inconsistent if instances are actively writing. For consistency, stop specific instances before backing up their data.

### Per-Instance Backup

For multi-tenant scenarios, backup individual instances:

```bash
# Stop the instance
ten stop api:customer123

# Backup its data
tar -czvf customer123-$(date +%Y%m%d).tar.gz \
  /var/lib/tenement/api/customer123/

# Restart
ten spawn api --id customer123
```

## Restore Procedure

### Full Restore

```bash
# Stop tenement
systemctl stop tenement

# Restore from backup
tar -xzvf tenement-backup-20240115.tar.gz -C /

# Start tenement
systemctl start tenement

# Verify instances
ten ps
```

### Restore Specific Instance

```bash
# Stop instance
ten stop api:customer123

# Restore data
tar -xzvf customer123-20240115.tar.gz -C /

# Restart
ten spawn api --id customer123
```

### Restore to New Server

```bash
# On new server
cargo install tenement-cli

# Copy backup
scp backup-server:~/backups/tenement-backup.tar.gz .

# Restore
tar -xzvf tenement-backup.tar.gz -C /

# Install systemd service
ten install --domain example.com

# Verify
ten ps
```

## Automated Backups

### Cron Job

```bash
# /etc/cron.daily/tenement-backup
#!/bin/bash
BACKUP_DIR="/backups/tenement"
DATE=$(date +%Y%m%d)

# Create daily backup
sqlite3 /var/lib/tenement/tenement.db ".backup $BACKUP_DIR/tenement-$DATE.db"
cp /etc/tenement/tenement.toml $BACKUP_DIR/tenement-$DATE.toml
tar -czf $BACKUP_DIR/data-$DATE.tar.gz /var/lib/tenement/

# Keep last 7 days
find $BACKUP_DIR -name "*.tar.gz" -mtime +7 -delete
find $BACKUP_DIR -name "*.db" -mtime +7 -delete
```

### S3 Backup with Litestream

For continuous SQLite replication:

```bash
# Install litestream
wget https://github.com/benbjohnson/litestream/releases/latest/download/litestream-linux-amd64.tar.gz

# Configure /etc/litestream.yml
dbs:
  - path: /var/lib/tenement/tenement.db
    replicas:
      - url: s3://my-bucket/tenement

# Run as service
litestream replicate
```

## Disaster Recovery

### Complete Loss Recovery

1. Provision new server
2. Install tenement
3. Restore config and data from backup
4. Update DNS to point to new server
5. Verify all instances running

### Partial Recovery

If only some data is lost:

```bash
# Identify affected instances
ten ps

# Restore from backup
tar -xzf backup.tar.gz -C / var/lib/tenement/api/customer123/

# Restart affected instances
ten restart api:customer123
```

## Testing Backups

Regularly verify backups work:

```bash
# Create test environment
mkdir /tmp/tenement-test

# Restore to test location
tar -xzf backup.tar.gz -C /tmp/tenement-test

# Verify config parses
cd /tmp/tenement-test/etc/tenement
ten config

# Verify database integrity
sqlite3 /tmp/tenement-test/var/lib/tenement/tenement.db "PRAGMA integrity_check"
```

## Next Steps

- [Upgrading](/guides/07-upgrading) - Version upgrades
- [Monitoring Setup](/guides/09-monitoring) - Observability
- [Troubleshooting](/reference/troubleshooting) - Common issues
