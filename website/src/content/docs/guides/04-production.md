---
title: Production Deployment
description: Deploy tenement with TLS, systemd, and Caddy
---

This guide covers deploying tenement to production with HTTPS, automatic restarts, and proper security.

## Quick Production Setup

The fastest path to production on a Linux server:

```bash
# Install tenement
cargo install tenement-cli

# Create config
cat > /etc/tenement/tenement.toml << 'EOF'
[settings]
data_dir = "/var/lib/tenement"

[service.api]
command = "./my-api"
socket = "/tmp/tenement/api-{id}.sock"
health = "/health"

[instances]
api = ["prod"]
EOF

# Install as systemd service and generate Caddy config
ten install --caddy --domain example.com
```

This creates:
- A systemd service (`tenement.service`)
- A Caddyfile with automatic HTTPS
- Proper file permissions and security hardening

## Option 1: Built-in TLS

tenement can handle TLS directly using Let's Encrypt.

```bash
ten serve --tls --domain example.com --email admin@example.com
```

### How It Works

1. tenement requests certificates from Let's Encrypt
2. HTTP-01 challenge verifies domain ownership
3. Certificates auto-renew before expiry
4. All traffic is encrypted

### TLS Status

Check certificate status:

```bash
curl https://example.com/api/tls/status
```

```json
{
  "enabled": true,
  "domain": "example.com",
  "valid_until": "2024-04-15T00:00:00Z",
  "issuer": "Let's Encrypt"
}
```

### Wildcard Certificates (DNS-01)

For wildcard subdomain routing (`*.example.com`), use DNS-01 challenge:

```bash
ten serve --tls --domain example.com --email admin@example.com \
  --dns-provider cloudflare --dns-token $CF_API_TOKEN
```

**Supported DNS providers:**
- `cloudflare` - Cloudflare API token
- `route53` - AWS Route53 (uses AWS credentials)
- `digitalocean` - DigitalOcean API token

The DNS-01 challenge creates a TXT record to prove domain ownership, enabling wildcard certificates.

## Option 2: Caddy Reverse Proxy

Use Caddy for TLS termination with tenement handling routing.

### Generate Caddyfile

```bash
ten caddy --domain example.com --output /etc/caddy/Caddyfile
```

Generated Caddyfile:

```caddyfile
{
    email admin@example.com
}

example.com {
    reverse_proxy unix//tmp/tenement/tenement.sock
}

*.example.com {
    reverse_proxy unix//tmp/tenement/tenement.sock
}
```

### Install Caddy

```bash
# Debian/Ubuntu
apt install caddy

# Or with ten caddy --install
ten caddy --domain example.com --install
```

### Start Services

```bash
# tenement listens on Unix socket
ten serve --socket /tmp/tenement/tenement.sock

# Caddy handles TLS and proxies to socket
systemctl start caddy
```

### Why Caddy?

- Automatic HTTPS with Let's Encrypt
- Wildcard certificates with DNS challenge
- Zero-downtime certificate renewals
- Battle-tested TLS configuration

## Systemd Service

### Install Service

```bash
ten install
```

This creates `/etc/systemd/system/tenement.service`:

```ini
[Unit]
Description=tenement process hypervisor
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/ten serve
Restart=always
RestartSec=5

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/tenement /tmp/tenement
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
```

### Service Commands

```bash
# Start/stop/restart
systemctl start tenement
systemctl stop tenement
systemctl restart tenement

# View logs
journalctl -u tenement -f

# Enable on boot
systemctl enable tenement
```

### Uninstall

```bash
ten uninstall
```

Removes the systemd service file and disables the service.

## All-in-One Setup

Install tenement with systemd + Caddy + TLS in one command:

```bash
ten install --caddy --domain example.com --dns-provider cloudflare
```

**What this does:**
1. Creates systemd service for tenement
2. Generates Caddyfile with wildcard support
3. Configures DNS-01 challenge for wildcards
4. Enables both services on boot

### Flags

| Flag | Description |
|------|-------------|
| `--caddy` | Generate Caddyfile |
| `--domain <domain>` | Domain for routing |
| `--email <email>` | Email for Let's Encrypt |
| `--dns-provider <provider>` | DNS provider for wildcard certs |
| `--dns-token <token>` | API token for DNS provider |
| `--install` | Also install Caddy (apt) |
| `--systemd` | Enable systemd services on boot |
| `--dry-run` | Show what would be done |

## File Locations

| File | Purpose |
|------|---------|
| `/etc/tenement/tenement.toml` | Main configuration |
| `/var/lib/tenement/` | Instance data directories |
| `/tmp/tenement/` | Unix sockets |
| `/etc/systemd/system/tenement.service` | systemd unit |
| `/etc/caddy/Caddyfile` | Caddy configuration |

## Security Considerations

### Firewall

Only expose ports 80 and 443:

```bash
ufw allow 80/tcp
ufw allow 443/tcp
ufw enable
```

### API Authentication

Generate and use auth tokens:

```bash
# Generate token
ten token-gen

# Use token for API calls
curl -H "Authorization: Bearer $TOKEN" https://example.com/api/instances
```

### Resource Limits

Prevent runaway processes:

```toml
[service.api]
memory_limit_mb = 256
cpu_shares = 100
storage_quota_mb = 100
```

## Monitoring

### Prometheus Metrics

Scrape `https://example.com/metrics` for:
- Instance counts and states
- Request latencies
- Memory/CPU per instance
- Storage usage

### Health Endpoint

```bash
curl https://example.com/health
```

Returns 200 if the server is healthy.

## Next Steps

- [Configuration Reference](/guides/03-configuration) - Full TOML options
- [Deployments](/guides/05-deployments) - Blue-green and canary patterns
- [Troubleshooting](/reference/troubleshooting) - Common issues
