# Roadmap

See [CHANGELOG.md](CHANGELOG.md) for completed work.

## Phase Break Stuff -- Security Hardening (deferred: network namespace isolation)
> After: (none) · Before: Phase Counterfeit

### a. Network namespace isolation (DEFERRED)
- Requires adding `CLONE_NEWNET` to namespace runtime with veth pair setup
- Would break current TCP port proxying model (tenant in new netns unreachable from host)
- Needs design decision: switch to Unix socket communication, or implement full container networking
- Revisit when the single-server story is more mature


## Runtime Direction

Tenement stays runtime-pluggable. For Soup/Tinyhost, the production bet is
DigitalOcean or other KVM-capable hosts running Quark through Docker/containerd:
OCI images, normal container networking for the first pass, and a future Quark
VFS path for CinchFS. gVisor remains the portable no-KVM fallback. LiteBox stays
as an optional external-runner runtime in Tenement OSS, but it is no longer the
Soup/Tinyhost filesystem bet because it needs ELF rewriting, custom networking,
and a less direct durable-filesystem path.


## Phase Everest -- slum: Fleet Control Plane
> After: Phase Break Stuff · Before: Phase Kilimanjaro

Make slum a real standalone binary that health-checks tenement servers and routes requests.

- [ ] `slum` CLI binary with clap: `slum serve`, `slum add-server`, `slum add-tenant`, `slum ps`, `slum servers`
- [ ] Background server health checker: poll `/health`, auto-detect offline after N failures
- [ ] Bearer token auth on slum's management API
- [ ] Store server API tokens in DB
- [ ] Tests: health check loop, auto-offline, CLI commands, auth

## Phase Kilimanjaro -- Tenant Placement and Failover
> After: Phase Everest · Before: Phase Denali

Multi-server tenant assignments with per-tenant failover strategy.

- [ ] `tenant_servers` join table (tenant_id, server_id, role, status)
- [ ] Failover modes: cold (re-spawn), warm (pre-spawned secondary), active-active (multi-server)
- [ ] Capacity-based auto-placement: `slum add-tenant --auto`
- [ ] Failover execution: server goes offline, slum executes per-tenant strategy
- [ ] Routing: pick server based on role/health/region
- [ ] Tests: failover simulation, placement logic, multi-server routing

## Phase Denali -- Remote Instance Management
> After: Phase Kilimanjaro · Before: Phase Olympus

slum calls tenement's HTTP API to manage instances across the fleet.

- [ ] HTTP client for tenement API (spawn, stop, deploy, logs, metrics)
- [ ] `slum spawn/stop/deploy <tenant>` CLI commands
- [ ] `slum logs <tenant>` aggregates from all servers
- [ ] `slum metrics` fleet-wide aggregation

## Phase Olympus -- Geographic Routing and Polish
> After: Phase Denali · Before: (none)

- [ ] Region-aware routing (client IP or tenant preference)
- [ ] `slum migrate <tenant> --from east-1 --to west-1`
- [ ] Fleet dashboard
- [ ] TLS termination at slum level
- [ ] Fleet-level Prometheus metrics

## Remaining Work

### File splits (deferred from Phase Full Nelson)
- `hypervisor.rs` at 2730 lines (1400 code + 1300 tests)
- Split hypervisor into: lifecycle, health, routing, deploy
- Split server into: routes, middleware, proxy

### Alert webhooks
- Configurable webhooks for health state changes, storage warnings, restart loops

### Service discovery
- DNS-based service discovery between tenant processes

### Container runtime hardening
- Reap orphaned `ten-*` Docker containers on Tenement startup after a crash.
- Stream `docker logs` for Quark/gVisor containers into Tenement's `LogBuffer`
  so `ten logs` works the same across runtimes.
- Replace host networking with per-app bridge networking once sidecars move into
  the same network boundary.

## Design Principles

1. **Same API, different isolation** -- All levels use the same routing, supervision, and health checks
2. **Fail loudly** -- Clear errors when isolation isn't available
3. **No magic** -- Explicit configuration, no auto-detection
4. **Linux only** -- Production tool for Linux servers
