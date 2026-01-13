---
title: Troubleshooting
description: Common issues and solutions
---

## TLS / Certificate Issues

### "ACME challenge failed"

**Symptom:** TLS certificate request fails with challenge error.

**Causes:**
1. Port 80 not accessible (HTTP-01 challenge)
2. DNS not pointing to server
3. Firewall blocking Let's Encrypt

**Solutions:**

```bash
# Check port 80 is open
curl http://your-domain.com

# Verify DNS
dig your-domain.com

# Check firewall
ufw status
ufw allow 80/tcp
ufw allow 443/tcp
```

### "Wildcard cert failed"

**Symptom:** Wildcard certificate (`*.example.com`) fails to issue.

**Cause:** Wildcard certs require DNS-01 challenge, not HTTP-01.

**Solution:** Use `--dns-provider` flag:

```bash
ten serve --tls --domain example.com \
  --dns-provider cloudflare --dns-token $CF_TOKEN
```

### "Certificate expired"

**Symptom:** HTTPS fails with certificate error.

**Cause:** Auto-renewal failed or tenement wasn't running.

**Solution:**

```bash
# Restart to trigger renewal
systemctl restart tenement

# Check certificate status
curl https://example.com/api/tls/status
```

## Port Conflicts

### "Address already in use"

**Symptom:** `ten serve` fails with "address already in use".

**Cause:** Another process is using the port.

**Solutions:**

```bash
# Find what's using the port
lsof -i :8080

# Kill it or use different port
ten serve --port 8081

# Or stop the conflicting service
systemctl stop nginx
```

### "Permission denied" on port 80/443

**Symptom:** Can't bind to privileged ports.

**Cause:** Non-root user can't bind ports below 1024.

**Solutions:**

```bash
# Option 1: Use Caddy (recommended)
ten caddy --domain example.com

# Option 2: Use higher port + reverse proxy
ten serve --port 8080

# Option 3: Grant capability (not recommended)
sudo setcap 'cap_net_bind_service=+ep' $(which ten)
```

## Cgroup / Resource Limit Issues

### "Failed to create cgroup"

**Symptom:** Instances fail to spawn with cgroup error.

**Causes:**
1. Not running on Linux
2. cgroups v2 not available
3. Permission denied

**Solutions:**

```bash
# Check cgroup version
mount | grep cgroup

# For cgroup v2, check if unified
ls /sys/fs/cgroup/cgroup.controllers

# Run as root or with proper permissions
sudo ten serve

# Or disable resource limits in config
[service.api]
# Remove memory_limit_mb and cpu_shares
```

### "Memory limit not enforced"

**Symptom:** Process exceeds `memory_limit_mb`.

**Cause:** cgroups not properly configured.

**Solutions:**

```bash
# Verify cgroups v2
cat /sys/fs/cgroup/cgroup.controllers

# Check if memory controller is enabled
cat /sys/fs/cgroup/cgroup.subtree_control

# Enable memory controller
echo "+memory" | sudo tee /sys/fs/cgroup/cgroup.subtree_control
```

## Instance Issues

### "Instance won't start"

**Symptom:** `ten spawn` succeeds but instance immediately stops.

**Diagnosis:**

```bash
# Check logs
ten logs api:myid

# Check if command exists
which my-command

# Test command manually
cd /var/lib/tenement/api/myid && ./my-command
```

**Common causes:**
1. Command not found
2. Missing dependencies
3. Permission issues
4. Socket path doesn't exist

### "Health check failing"

**Symptom:** Instance shows "unhealthy" in `ten ps`.

**Diagnosis:**

```bash
# Check health endpoint directly
curl --unix-socket /tmp/tenement/api-myid.sock http://localhost/health

# Check instance logs
ten logs api:myid
```

**Common causes:**
1. Health endpoint returns non-200
2. Health endpoint path wrong in config
3. Instance crashed but socket remains

### "Socket not created"

**Symptom:** Instance starts but socket file doesn't appear.

**Causes:**
1. App not listening on socket
2. Socket directory doesn't exist
3. Permission denied

**Solutions:**

```bash
# Create socket directory
mkdir -p /tmp/tenement

# Check your app actually listens on socket
# Python example:
app.run(unix_socket=os.getenv("SOCKET_PATH"))

# Check socket path in config matches app
[service.api]
socket = "/tmp/tenement/api-{id}.sock"
```

## Routing Issues

### "404 on subdomain"

**Symptom:** `myid.api.example.com` returns 404.

**Causes:**
1. Instance not running
2. Wrong domain in `ten serve`
3. DNS not configured for wildcard

**Solutions:**

```bash
# Check instance exists
ten ps

# Verify domain setting
ten serve --domain example.com

# Check DNS has wildcard record
dig *.example.com
```

### "Wrong instance routed"

**Symptom:** Requests go to wrong instance.

**Cause:** Routing pattern mismatch.

**Understanding routing:**
- `v1.api.example.com` → direct to `api:v1`
- `api.example.com` → weighted across all `api:*`

```bash
# Check weights
ten ps

# Adjust weights if needed
ten weight api:v1 100
ten weight api:v2 0
```

## Storage Issues

### "Storage quota exceeded"

**Symptom:** Instance shows warning or stops accepting writes.

**Solutions:**

```bash
# Check storage usage
curl https://example.com/api/instances/api:myid/storage

# Increase quota
[service.api]
storage_quota_mb = 500

# Or clean up old data
rm -rf /var/lib/tenement/api/myid/cache/*
```

### "Data lost on restart"

**Symptom:** Instance data disappears after stop/start.

**Cause:** `storage_persist = false` in config.

**Solution:**

```toml
[service.api]
storage_persist = true  # Keep data on stop
```

## Systemd Issues

### "Service won't start"

**Symptom:** `systemctl start tenement` fails.

**Diagnosis:**

```bash
# Check service status
systemctl status tenement

# Check logs
journalctl -u tenement -n 50

# Verify config
ten serve --config /etc/tenement/tenement.toml --dry-run
```

### "Service keeps restarting"

**Symptom:** Service restarts repeatedly.

**Cause:** tenement crashing or config error.

**Solutions:**

```bash
# Check crash logs
journalctl -u tenement -n 100

# Test config manually
ten serve --config /etc/tenement/tenement.toml

# Increase restart delay
sudo systemctl edit tenement
# Add: RestartSec=30
```

## Getting Help

If these solutions don't help:

1. Check logs: `ten logs` and `journalctl -u tenement`
2. Search [GitHub Issues](https://github.com/russellromney/tenement/issues)
3. Open a new issue with:
   - tenement version (`ten --version`)
   - OS and kernel version
   - Config file (sanitized)
   - Full error message
   - Steps to reproduce
