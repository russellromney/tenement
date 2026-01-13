---
title: Roadmap
description: Future development and features
---

## Completed

### Core Features
- ✅ Process supervision with auto-restart
- ✅ Hibernation - Scale to zero, wake on request
- ✅ Exponential backoff restarts
- ✅ Unix socket proxy - Full request routing to backends
- ✅ Subdomain routing (`prod.api.example.com` → `api:prod`)

### Isolation & Security
- ✅ Namespace isolation - Zero-overhead `/proc` protection (Linux)
- ✅ Sandbox isolation (gVisor) - Syscall filtering for untrusted code
- ✅ Resource limits - Memory and CPU limits via cgroups v2
- ✅ Auth middleware - Bearer token authentication

### Production Setup
- ✅ `ten install` - Install as systemd service with security hardening
- ✅ `ten uninstall` - Clean removal of systemd service
- ✅ `ten caddy` - Generate Caddyfile with automatic HTTPS via Let's Encrypt
- ✅ `ten serve --tls` - Built-in TLS with Let's Encrypt certificates
- ✅ DNS-01 challenge support for wildcard certificates

### Storage & Persistence
- ✅ Storage quotas per instance (`storage_quota_mb`, `storage_persist`)
- ✅ Storage API endpoint (`GET /api/instances/:id/storage`)
- ✅ Prometheus metrics for storage monitoring
- ✅ Dashboard storage display with color-coded usage

### Instance Management
- ✅ Instance auto-start - Declare instances in `[instances]` section
- ✅ Weighted routing for canary/blue-green deployments
- ✅ `ten weight` command for traffic distribution

### Observability
- ✅ Dashboard - Svelte web UI for instance management
- ✅ Prometheus metrics at `/metrics`
- ✅ Log capture with full-text search

### Testing
- ✅ Comprehensive test suite (340+ tests + 8 benchmarks)
- ✅ E2E integration tests
- ✅ Fleet mode (slum) - Multi-server orchestration

## In Progress

- 🔄 Deploy commands (`ten deploy`, `ten route`) for blue/green deployments
- 🔄 Slum health check loop

## Planned (Next)

### WASM Runtime
- Lightweight compute sandbox using wasmtime
- ~5-10MB overhead per instance
- Fast startup (<50ms)
- Useful for user plugins, functions-as-a-service

### Enhanced Monitoring
- OpenTelemetry integration
- Distributed tracing
- Custom metrics API
- Alert webhooks

### Advanced Networking
- Custom network namespaces (full network isolation)
- Service discovery (DNS-based)

### Persistence & Snapshots
- Checkpoint/restore (CRIU)
- Instance snapshots for faster spawn
- State migration between servers

### Firecracker Support
- MicroVM isolation (128MB overhead)
- Custom kernel support
- Compliance-grade isolation

## Long-term Vision

### Isolation Spectrum

```
Bare Process ──→ Namespace ──→ Sandbox ──→ MicroVM
0ms, 0MB         0ms, 0MB      100ms,20MB  125ms,128MB
```

Support all isolation levels seamlessly:
- Same API and CLI
- Configuration-driven isolation selection
- Automatic fallback if unavailable

### Multi-cloud Orchestration

- Orchestrate across cloud providers (AWS, GCP, Fly.io, etc.)
- Cost optimization across clouds

### Edge Computing

- Deploy to edge locations
- Coordinate workloads across distributed edge

## Contributing

Want to help? Check out:
- [GitHub Issues](https://github.com/russellromney/tenement/issues)

## Feedback

Share ideas and feedback on GitHub. We're building in the open!
