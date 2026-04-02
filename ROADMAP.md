# Roadmap

See [CHANGELOG.md](CHANGELOG.md) for completed work.

## Phase Break Stuff -- Security Hardening (deferred: network namespace isolation)
> After: (none) · Before: Phase Counterfeit

### a. Network namespace isolation (DEFERRED)
- Requires adding `CLONE_NEWNET` to namespace runtime with veth pair setup
- Would break current TCP port proxying model (tenant in new netns unreachable from host)
- Needs design decision: switch to Unix socket communication, or implement full container networking
- Revisit when the single-server story is more mature

## Phase Counterfeit -- Fix Fake Features
> After: Phase Break Stuff · Before: Phase Rollin

Features that exist in code but don't actually work end-to-end.

### a. Record request metrics
- `requests_total` and `request_duration_ms` are defined in Metrics but never incremented
- The proxy path doesn't count requests or measure latency
- Add ~20 lines of middleware to record per-tenant request count and duration
- The Prometheus endpoint currently shows zeros for these; make it real

### b. Storage quota enforcement
- `storage_quota_mb` is a config option that does nothing
- No periodic check, no warnings emitted, no metrics updated
- The health monitor should check storage usage, emit warnings at 80%/90%, update `instance_storage_bytes` gauge
- Decide: hard kill at 100%? or just loud warnings + metrics?

### c. Defer slum
- `slum` is 90% CRUD with no health checking, no real proxying, no failover
- It's a distraction from making the core product solid
- Move to a separate repo or gate behind a feature flag
- Revisit after the single-server story is airtight

### d. FTS5 search injection
- FTS5 MATCH queries accept special syntax (NOT, OR, NEAR, column filters)
- User search input is wrapped in double quotes but not properly escaped for FTS5
- Sanitize or use parameterized FTS5 queries

## Phase Rollin -- Process Reliability
> After: Phase Counterfeit · Before: Phase Re-Arranged

### a. Process exit monitoring
- Spawn a `child.wait()` task per instance that detects exit immediately
- Trigger restart/cleanup on exit instead of waiting for next health check cycle (up to 10s delay)

### b. Restart history persistence
- `restart()` calls `stop()` which removes instance from map, then `spawn()` creates fresh with empty `restart_times`
- `max_restarts` within `restart_window` can never trigger `Failed` because history resets each restart
- Preserve restart history across stop/spawn cycles

### c. Graceful shutdown + signal handling
- `ten serve` doesn't handle SIGTERM/SIGINT
- If tenement dies, all child processes become orphans on their allocated ports
- On shutdown: stop accepting new connections, drain in-flight requests, kill all children, release ports

### d. `spawn_and_wait` TCP readiness
- For TCP-based runtimes, `spawn_and_wait` checks `socket.exists()` but should check via TCP connect
- `spawn()` handles both modes correctly; `spawn_and_wait` does not

### e. Request queuing during wake
- Multiple requests hitting a sleeping instance each call `spawn_and_wait` independently
- Use a `tokio::sync::Notify` or similar per-instance wake-once pattern

## Phase Re-Arranged -- State & Persistence
> After: Phase Rollin · Before: Phase Nookie

### a. Persistent instance state
- All instance state is in-memory; a tenement crash loses knowledge of running processes
- Orphaned processes keep running on their ports with no management
- Write state to SQLite: `{instance_id, pid, port, started_at, process_config_hash}`
- On startup: re-adopt orphaned processes or kill them

### b. Default `storage_persist` to `true`
- Current default of `false` silently destroys tenant data directories on idle timeout
- For the stated use case (databases per tenant), this causes data loss every idle cycle
- Change default, add migration note

### c. Cgroup failure handling
- Cgroup creation failure currently `warn!`s and continues
- Process runs unrestricted; one tenant can OOM the whole server
- Fail loudly when resource limits are configured but can't be applied

## Phase Nookie -- Multi-Tenant Demo & Getting Started
> After: Phase Re-Arranged · Before: Phase Full Nelson

Prove the value proposition with a real demo that shows why tenement exists.

### a. Killer multi-tenant example
- 30-line Python app: reads `PORT`, serves a simple API backed by SQLite in `{data_dir}`
- `tenement.toml` with `idle_timeout = 300`, `storage_persist = true`
- Script that creates 10 tenants, shows them all running, idles them, wakes one
- Show: 10 tenants on one box, 20MB total, sub-second wake from idle

### b. Getting started guide
- README quick start that goes from zero to multi-tenant in under 2 minutes
- Cover the actual value prop, not just "here's how to spawn a process"
- Show the subdomain routing, idle timeout, wake-on-request flow

### c. Deployment guide
- Single Hetzner VPS setup with Caddy + tenement
- DNS wildcard setup for `*.app.example.com`
- systemd service with proper resource limits

## Phase Full Nelson -- Operational Hardening
> After: Phase Nookie · Before: Phase Behind Blue Eyes

### a. Request timeout on proxy
- No timeout on reverse proxy; a hung tenant holds the connection forever
- Add configurable per-service `request_timeout` (default 30s)

### b. Unix socket client pooling
- `proxy_to_unix_socket` creates a new `Client<UnixConnector>` per request
- Share a pooled client like the TCP proxy does

### c. Connection draining on stop
- When an instance is stopped (manually or via idle timeout), active connections are severed immediately
- Add a brief drain period: reject new requests, allow in-flight to complete (configurable, default 5s)

### d. Connection-aware idle timeout
- Currently tracks "last request" via `touch()`, but no tracking of active connections
- An instance could be reaped while serving a long WebSocket or download
- Track active connection count; don't reap while connections > 0

### e. File splits
- `hypervisor.rs` and `server.rs` are both approaching 1000 lines
- Split hypervisor into: lifecycle, health, routing, deploy
- Split server into: routes, middleware, proxy

## Phase Behind Blue Eyes -- Multi-Tenant Auth & Observability
> After: Phase Full Nelson · Before: (none)

### a. Per-tenant API tokens
- Currently one token for the entire system
- No scoped access, no token rotation without downtime
- Add per-tenant tokens with scoped access to their own logs/metrics/instances

### b. Deployment history/audit log
- Track: who, what, when, success/fail in SQLite
- Rollback needs to know "previous version"

### c. OpenTelemetry integration
- Distributed tracing
- Alert webhooks

### d. Service discovery
- DNS-based service discovery between tenant processes

## Design Principles

1. **Same API, different isolation** -- All levels use the same routing, supervision, and health checks
2. **Fail loudly** -- Clear errors when isolation isn't available
3. **No magic** -- Explicit configuration, no auto-detection
4. **Linux only** -- Production tool for Linux servers
