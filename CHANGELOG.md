# Changelog

## v0.1.0 (Current)

### Core
- Process supervision with auto-restart and exponential backoff
- Subdomain routing (`prod.api.example.com` → `api:prod`)
- Unix socket proxy for request routing
- Scale-to-zero with wake-on-request (`idle_timeout`)
- Weighted routing for blue-green/canary deployments

### Isolation
- **Namespace isolation** (default) - Zero-overhead `/proc` protection via Linux namespaces
- **Sandbox isolation** - gVisor syscall filtering for untrusted code
- Resource limits via cgroups v2 (`memory_limit_mb`, `cpu_shares`)

### Production
- Built-in TLS with Let's Encrypt (`ten serve --tls`)
- Caddy integration for wildcard certificates (`ten caddy`)
- systemd service installation (`ten install`)
- Storage quotas per instance (`storage_quota_mb`)

### Observability
- Svelte dashboard at root domain
- Prometheus metrics at `/metrics`
- Log capture with full-text search
- Bearer token authentication

### CLI
- `ten serve` - Start server
- `ten spawn/stop/restart` - Instance management
- `ten ps` - List instances
- `ten weight` - Traffic distribution
- `ten deploy` - Deploy and wait for healthy
- `ten route` - Atomic traffic swap
- `ten config` - Show configuration

### Fleet Mode (slum)
- Multi-server orchestration library
- Tenant routing to specific servers
- SQLite-based coordination
