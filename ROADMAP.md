# Roadmap

See [CHANGELOG.md](CHANGELOG.md) for completed work.

## Phase Break Stuff -- Security Hardening (deferred: network namespace isolation)
> After: (none) · Before: Phase Counterfeit

### a. Network namespace isolation (DEFERRED)
- Requires adding `CLONE_NEWNET` to namespace runtime with veth pair setup
- Would break current TCP port proxying model (tenant in new netns unreachable from host)
- Needs design decision: switch to Unix socket communication, or implement full container networking
- Revisit when the single-server story is more mature


## Phase Counterfeit -- libkrun MicroVM Runtime
> After: Phase Break Stuff · Before: Phase Everest

Add one VM-backed isolation level for untrusted code: `isolation = "microvm"`, powered by libkrun.

Tenement's microVM story should be opinionated. libkrun fits the project better than Firecracker because it is an embedded library for virtualization-backed process isolation, not a separate VM orchestration surface. Firecracker can be revisited later if users specifically need its ecosystem, but it should not shape the first microVM implementation.

### Product shape
- [ ] User-facing config is `isolation = "microvm"`; docs say it is powered by libkrun
- [ ] Keep `process`, `namespace`, and `sandbox`; do not expose Firecracker/QEMU as supported choices
- [ ] Treat microVM as the recommended level for hostile or unknown code that needs a guest kernel boundary
- [ ] Fail loudly when KVM/libkrun/libkrunfw or required host isolation is missing

### Security model
- [ ] One libkrun VMM per tenant instance
- [ ] Treat the guest and VMM as the same security context; the guest may influence host resources proxied by the VMM
- [ ] Run each VMM inside host namespaces with a tenant-specific UID/GID, mount view, cgroup, and network policy
- [ ] Expose only tenant-owned root/data directories to virtio-fs; use host mount isolation around the VMM
- [ ] Apply memory, CPU, PID, storage, and network controls to the VMM process
- [ ] Prefer explicit Tenement ingress to the guest app over broad guest networking
- [ ] Document that the guest kernel boundary is defense in depth, not a replacement for jailing the VMM

### Implementation checklist
- [ ] Add `RuntimeType::Microvm` and parse `microvm` (optionally accept `krun` as an alias)
- [ ] Add a `krun` cargo feature and a `KrunRuntime` implementing the existing `Runtime` trait
- [ ] Decide binding strategy: existing Rust bindings if viable, otherwise minimal `bindgen`/`libloading` wrapper around libkrun's C API
- [ ] Add config fields for microVM root, kernel/firmware path, memory, vCPUs, virtio-fs mounts, and network mode
- [ ] Build a per-instance root/data layout from Tenement's existing `{data_dir}/{service}/{id}` model
- [ ] Connect Tenement's proxy and health checks to the guest through libkrun's supported socket/vsock/network mechanism
- [ ] Capture guest stdout/stderr or console logs into the existing log buffer
- [ ] Ensure `stop`, idle shutdown, restart, cgroups, storage accounting, and metrics work for VMM-backed instances
- [ ] Add availability diagnostics for `/dev/kvm`, libkrun, libkrunfw, and host namespace/cgroup support
- [ ] Add ignored Linux/KVM integration tests plus unit tests for config parsing, availability errors, and lifecycle bookkeeping


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

## Design Principles

1. **Same API, different isolation** -- All levels use the same routing, supervision, and health checks
2. **Fail loudly** -- Clear errors when isolation isn't available
3. **No magic** -- Explicit configuration, no auto-detection
4. **Linux only** -- Production tool for Linux servers
