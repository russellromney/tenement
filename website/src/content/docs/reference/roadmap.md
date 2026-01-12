---
title: Roadmap
description: Future development and features
---

## Completed

- ✅ Hibernation - Scale to zero, wake on request
- ✅ Exponential backoff restarts
- ✅ Namespace isolation - Zero-overhead `/proc` protection (Linux)
- ✅ Sandbox isolation (gVisor) - Syscall filtering for untrusted code
- ✅ Resource limits - Memory and CPU limits via cgroups v2
- ✅ Comprehensive test suite (256+ tests)
- ✅ Unix socket proxy - Full request routing to backends
- ✅ Auth middleware - Bearer token authentication
- ✅ Dashboard - Svelte web UI for instance management
- ✅ Prometheus metrics - Full metrics endpoint
- ✅ Log capture - Full-text search with streaming
- ✅ Fleet mode (slum) - Multi-server orchestration

## In Progress

- 🔄 E2E integration tests (Sessions 2-8)
  - Basic spawn/stop workflows
  - Health check recovery
  - Isolation level verification
  - Fleet mode operations

## Planned (Next)

### WASM Runtime (Session 2)
- Lightweight compute sandbox using wasmtime
- ~5-10MB overhead per instance
- Fast startup (<50ms)
- Useful for user plugins, functions-as-a-service

### Storage Quotas (Session 3)
- Limit disk space per instance
- Automatic cleanup of old data
- Quota enforcement via file system

### Enhanced Monitoring (Session 4)
- OpenTelemetry integration
- Distributed tracing
- Custom metrics API
- Alert webhooks

### Advanced Networking (Session 5)
- Custom network namespaces (full network isolation)
- Service discovery (DNS-based)
- Load balancing per service

### Persistence & Snapshots (Session 6)
- Checkpoint/restore (CRIU)
- Instance snapshots for faster spawn
- State migration between servers

### Auto-scaling (Session 7)
- Automatically spawn based on load
- Metrics-driven scaling policies
- Cost optimization

### Firecracker Support (Session 8)
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
- Automatic vendor lock-in detection
- Cost optimization across clouds

### Edge Computing

- Deploy to edge locations
- Coordinate workloads across distributed edge
- Lower latency for users globally

### Compliance & Security

- FedRAMP/SOC2 compliance features
- Encryption at rest
- Audit logging
- Network policy enforcement

## Contributing

Want to help? Check out:
- [GitHub Issues](https://github.com/yourusername/tenement/issues)
- [Contributing Guide](https://github.com/yourusername/tenement/blob/main/CONTRIBUTING.md)
- [Development Setup](/guides/getting-started#development)

## Version History

### v0.1.0 (Current)
- Initial release
- Core process supervision
- Unix socket routing
- Basic isolation levels
- Web dashboard

### v0.2.0 (Q1 2024)
- WASM runtime
- Storage quotas
- Enhanced monitoring

### v1.0.0 (Target Q2 2024)
- Stable API
- Full test coverage
- Production hardening
- Documentation completeness

## Feedback

Share ideas and feedback:
- [GitHub Discussions](https://github.com/yourusername/tenement/discussions)
- Email: tenement@example.com
- Discord: [Join our community](https://discord.gg/...)

We're building in the open. Your input shapes the roadmap!
