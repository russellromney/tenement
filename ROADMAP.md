# Roadmap

See [CHANGELOG.md](CHANGELOG.md) for completed work.

## Phase Break Stuff -- Security Hardening (deferred: network namespace isolation)
> After: (none) · Before: Phase Counterfeit

### a. Network namespace isolation (DEFERRED)
- Requires adding `CLONE_NEWNET` to namespace runtime with veth pair setup
- Would break current TCP port proxying model (tenant in new netns unreachable from host)
- Needs design decision: switch to Unix socket communication, or implement full container networking
- Revisit when the single-server story is more mature


## Runtime Direction: gVisor, Quark, LiteBox

Tenement stays runtime-pluggable. It should not bake in Tinyhost/Soup's storage
or deployment opinions.

### Shipped / Kept

- `namespace` can consume `service.rootfs` on Linux by chrooting into the
  provided bundle root. This is useful for trusted/self-hosted loops, not as the
  hosted untrusted boundary.
- `sandbox` / gVisor stays in Tenement and remains the portable, no-KVM
  baseline for untrusted apps.
- `RuntimeType::Litebox` + `LiteBoxRuntime` is kept as an optional OSS runtime.
  Tenement supervises an external, configurable runner binary and carries no
  LiteBox, Cinch, or object-store dependency.

LiteBox runner contract (Tenement -> runner):

```text
<runner> run --rootfs <abs> --workdir <guest-path> --env K=V... -- <cmd> [args]
```

- Discovery: explicit path -> `TENEMENT_LITEBOX_RUNNER` -> `litebox` on PATH.
- Tenement allocates the TCP port, injects `PORT` via `--env`, health-checks
  `127.0.0.1:PORT`, and supervises the child.
- `service.rootfs` is required for LiteBox.
- Local-filesystem rootfs is the only mode Tenement knows about; any object
  storage or CinchFS behavior belongs in the runner.

### Tinyhost/Soup Runtime Decision

Tinyhost/Soup targets Hetzner dedicated / bare-metal hosts, where `/dev/kvm` is
available. With KVM on the table, **Quark is the chosen CinchFS runtime bet**:

- OCI drop-in runtime: runs unmodified images, no LiteBox-style ELF rewriting.
- Standard container networking: no per-instance TUN device plus host proxy.
- KVM VM-level isolation.
- Rust VFS seam under `qlib/kernel/fs/` with `Filesystem`,
  `InodeOperations`, and `FileOperations` traits that can model a CinchFS
  backend.

**gVisor remains the no-KVM portable baseline** for macOS/dev loops, CI, and
non-KVM deployment tiers such as Fly.io or Hetzner Cloud. Do not remove it.

**LiteBox is shelved for the CinchFS bet.** It is technically interesting and
remains an optional Tenement runtime, but it is no longer the primary Tinyhost
runtime path. Its drawbacks for this product are:

- every executable ELF must be syscall-rewritten ahead of time;
- networking wants a per-instance TUN device and host proxy;
- stock filesystem behavior is tar/in-memory oriented rather than a normal
  persistent host rootfs;
- it is pre-1.0, so the filesystem adapter would ride an unstable API.

### Next Runtime Work

1. Add Quark to Tenement as either:
   - `RuntimeType::Quark`, modeled on the existing gVisor sandbox runtime; or
   - a generalized configurable OCI runtime backend where `sandbox` can choose
     `runsc` or `quark`.
2. Run the Quark L0 proof on a KVM Linux host:
   - build/install Quark as an OCI runtime;
   - run a static HTTP server with `docker run --runtime=quark`;
   - confirm `curl http://127.0.0.1:<host-port>/health` works with standard
     container networking;
   - measure startup and memory directly, not through Docker harness overhead;
   - stub `qlib/kernel/fs/cinchfs/`, modeled on `qlib/kernel/fs/host/`, to
     prove a custom Rust VFS backend services `open/read/write/stat` from inside
     a Quark guest.

Do not build CinchFS yet. Runtime and filesystem-seam viability come first.


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
