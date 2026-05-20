# Roadmap

See [CHANGELOG.md](CHANGELOG.md) for completed work.

## Phase Break Stuff -- Security Hardening (deferred: network namespace isolation)
> After: (none) · Before: Phase Counterfeit

### a. Network namespace isolation (DEFERRED)
- Requires adding `CLONE_NEWNET` to namespace runtime with veth pair setup
- Would break current TCP port proxying model (tenant in new netns unreachable from host)
- Needs design decision: switch to Unix socket communication, or implement full container networking
- Revisit when the single-server story is more mature


## LiteBox Runtime

### Shipped
`RuntimeType::Litebox` + `LiteBoxRuntime`. Tenement supervises an external,
configurable runner binary; it does **not** embed LiteBox and carries no
Cinch/object-store dependency. gVisor is untouched and stays the mature sandbox.

Runner contract (Tenement -> runner):
```
<runner> run --rootfs <abs> --workdir <guest-path> --env K=V… -- <cmd> [args]
```
- Discovery: explicit path -> `TENEMENT_LITEBOX_RUNNER` -> `litebox` on PATH.
- Tenement allocates the TCP port, injects `PORT` via `--env`, health-checks
  `127.0.0.1:PORT`, and supervises the child (process-group kill, log capture).
- `service.rootfs` is required (fail-closed validation).
- Local-filesystem rootfs is the only mode Tenement knows about; any
  object-store backend lives entirely in the runner.

### Runner roadmap (downstream `tinyhost-litebox`/`soup-litebox`, not Tenement)

LiteBox is Microsoft's pre-1.0 Rust **library OS** (North = rustix-like guest
syscall surface; South = host Platform interface), MIT-licensed. We build the
runner ourselves — it links LiteBox and hosts the guest. Conforming to the
contract above is trivial; the hard parts are below.

#### L-1: viability spike (do this first, throwaway code, no runner)
Each question is a potential project-killer; answer from LiteBox's own
examples/docs before writing anything.
- **Networking.** Can a LiteBox guest `bind`/`listen` a TCP port the host can
  `connect` to on loopback? A library OS may have no network stack, or a
  userspace one with no host bridge. No host-reachable socket => the
  supervised-process-over-TCP model is dead; we'd need a stdio/vsock proxy
  transport, or LiteBox is the wrong tool.
- **Filesystem shape.** Does the stock FS take a host *directory* as the guest
  root, or only an in-memory/tar image? If tar-only, the first boot needs an
  `app.tar` (not the extracted `rootfs/`) and the FS-seam work starts at L0,
  not L1.
- **Embedding API.** Turnkey "run this ELF as a guest", or do we implement a
  chunk of the loader/Platform ourselves? This sets L0's true size.
- **Isolation strength on plain Linux.** LiteBox's security pitch leans on
  confidential computing (SEV-SNP) and Linux-on-Windows. On a normal Linux host
  it may be a syscall-subset with no privilege barrier, not a real boundary.
  **Until proven, treat LiteBox as a CinchFS-integration vehicle, NOT a sandbox
  for untrusted multi-tenant code.** gVisor stays the boundary for untrusted work.

#### L0: boot + reach, smallest possible app (no Cinch)
- Runner skeleton + contract parser; pin a LiteBox fork commit.
- Boot a **statically linked Rust/Go "hello" HTTP server** — NOT FastAPI; keep
  the LiteBox-viability question separate from CPython complexity — with the
  bundle rootfs as guest `/`, env incl. `PORT`, chdir `/app`.
- Tenement reaches it on `127.0.0.1:PORT`; health checks pass.
- Gate: if boot or networking fails here, stop. Off-ramp = gVisor + a CinchFS
  shim, or drop LiteBox.

#### H0: real-app compatibility (parallel probe)
- Run a Railpack-built CPython/FastAPI rootfs under LiteBox.
- Expect missing syscalls (epoll, mmap, threads, `fcntl` locking). Document each
  one the app hits; this defines the `litebox_compatible` set. If CPython won't
  boot, the first hosted templates must be static-binary apps.

#### L1: filesystem seam
- Make LiteBox's `FileSystem` backend pluggable (sealed in-crate today;
  fork-local impl point or upstreamable patch).
- Wire a trivial pass-through backend to prove the plumbing before Cinch.

#### L2: CinchFS adapter (`tinyhost-litebox-cinchfs`)
- Per-path routing: `/app` = bundle rootfs (read-only), `/data/files` = CinchFS
  volume (read-write), `/tmp` = ephemeral.
- `read`/`pread` -> `read_range`; writes -> `begin_write`/`write_chunk`/
  `commit_write`/`abort_write`; `stat`/`readdir`/`mkdir`/`unlink`/`rename` ->
  namespace mutations with grant + quota checks.
- Counters: whole-file fallbacks, range reads, extent writes, checkpoint
  latency, denied ops — to *prove* `pread` uses `read_range`.
- Fail-closed: missing/failed volume or revoked/read-only grant fails before any
  object IO, never open.
- **Hard sub-problem: SQLite over CinchFS.** App DBs use mmap, `fcntl` locking,
  and WAL. Object-backed mmap + locking is the riskiest case; scope it as its
  own spike, don't assume `write_chunk` covers it.

#### L3: hardening
- Anvil LiteBox profile (hostile harness).
- **Runner privilege posture.** The runner runs with Tenement's privileges; for
  untrusted apps it must drop privileges so a guest escape doesn't inherit them.
- **Checkpoint-on-stop.** Tenement currently SIGKILLs the process group, giving
  the runner no chance to flush a CinchFS checkpoint. A graceful-stop signal
  (SIGTERM + grace window) is a **Tenement-contract change** L3 depends on.
- Flip `litebox_compatible` per app once it passes Anvil.

**Carry cost.** LiteBox is pre-1.0 ("expect breaking changes"). The CinchFS
adapter rides an unstable internal FS API; budget for churn on every fork bump.


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
