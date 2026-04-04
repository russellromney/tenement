# Roadmap

See [CHANGELOG.md](CHANGELOG.md) for completed work.

## Phase Break Stuff -- Security Hardening (deferred: network namespace isolation)
> After: (none) · Before: Phase Counterfeit

### a. Network namespace isolation (DEFERRED)
- Requires adding `CLONE_NEWNET` to namespace runtime with veth pair setup
- Would break current TCP port proxying model (tenant in new netns unreachable from host)
- Needs design decision: switch to Unix socket communication, or implement full container networking
- Revisit when the single-server story is more mature


## Phase Forge -- Dashboard Rewrite
> After: Phase Break Stuff · Before: Phase Everest

Replace Svelte dashboard with HTMX + server-rendered templates embedded in the Rust binary. No JS build step, no npm. Just Rust.

### a. Template infrastructure
- [ ] Add `askama` (compile-time checked templates) as dependency
- [ ] Embed templates in binary via `rust-embed`
- [ ] Base layout template with HTMX CDN script
- [ ] Auth middleware: token stored in HTTP-only cookie
- [ ] Tests: template rendering, auth flow

### b. Auth + Token Management
- [ ] Login page with token input
- [ ] Store token in HTTP-only cookie (secure, no XSS)
- [ ] All routes protected by auth middleware
- [ ] Graceful 401 → redirect to login
- [ ] Tests: login flow, auth bypass, session expiry

### c. Instance Management
- [ ] Instance table with HTMX-powered actions (stop, restart, spawn)
- [ ] Spawn modal with process:id input
- [ ] Weight slider for canary routing (HTMX + inline JS)
- [ ] Deploy UI with version selection
- [ ] Tests: all CRUD operations, error states

### d. Log Viewer
- [ ] SSE endpoint for live streaming (HTMX `hx-sse`)
- [ ] Filter by process/level via HTMX requests
- [ ] Text search input
- [ ] Auto-scroll toggle
- [ ] Tests: stream connection, filtering, disconnect handling

### e. Overview
- [ ] Summary cards (instances, healthy, requests, storage)
- [ ] Auto-refresh via HTMX `hx-trigger` polling
- [ ] Instance detail modal (HTMX `hx-target` swap)
- [ ] Tests: data rendering, empty states, polling

## Phase Everest -- slum: Fleet Control Plane
> After: Phase Forge · Before: Phase Kilimanjaro

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
