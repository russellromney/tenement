# Roadmap

See [CHANGELOG.md](CHANGELOG.md) for what's been implemented.

## In Progress

- Slum health check loop

## Planned

### Enhanced Monitoring
- OpenTelemetry integration
- Distributed tracing
- Alert webhooks

### Advanced Networking
- Custom network namespaces (full network isolation)
- Service discovery (DNS-based)

### Persistence
- Checkpoint/restore (CRIU)
- Instance snapshots for faster spawn

## Maybe Later

### WASM Runtime
WebAssembly sandbox for WASI-compiled workloads. Deprioritized because most tenement users run Python/Node apps that can't compile to WASM. Namespace + sandbox covers most isolation needs.

### Firecracker MicroVMs
Full VM isolation with ~128MB overhead. Requires KVM (bare metal). For compliance requirements that mandate kernel isolation.

## Design Principles

1. **Same API, different isolation** - All levels use the same routing, supervision, and health checks
2. **Fail loudly** - Clear errors when isolation isn't available
3. **No magic** - Explicit configuration, no auto-detection
4. **Linux only** - Production tool for Linux servers
