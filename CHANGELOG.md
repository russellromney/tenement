# Changelog

## v0.2.0

### Phase Re-Arranged (State & Persistence)
- Persistent instance state: SQLite table records pid/port/started_at per instance for crash recovery
- Orphan recovery on startup: kills still-running processes from previous crashes
- Default storage_persist to true: tenant data directories preserved across idle cycles
- Cgroup failure handling: spawn fails loudly if resource limits can't be applied (was warn-and-continue)

### Phase Rollin (Process Reliability)
- Process exit monitoring: detects crashes within 1s via kill(pid, 0) background task
- Restart history persistence: restart count and timestamps survive stop/spawn cycles
- Graceful shutdown: SIGTERM/SIGINT stops all instances before server exit
- TCP readiness in spawn_and_wait: tries TCP connect for port-based runtimes
- Wake-once pattern: concurrent wake requests share a single spawn via tokio::sync::Notify

### Phase Counterfeit (Fix Fake Features)
- Request metrics now recorded in proxy: requests_total and request_duration_ms per process/instance
- Storage quota enforcement: health monitor checks usage, updates metrics, warns at 80%, errors at 100%
- Removed slum from workspace (was 90% CRUD with no real functionality)
- FTS5 search injection fix: strip metacharacters from user search input
- Removed backward-compat code: [process.X] alias, deprecated methods, runtime serde alias

### Phase Break Stuff (Security Hardening)
- Auth DoS prevention: rate limiting on failed auth attempts (10 failures triggers 5-second cooldown, skipping Argon2)
- Spawn race condition fix: "spawning" guard set prevents concurrent spawns of the same instance
- Error message sanitization: subdomain proxy responses no longer leak internal paths or process details to unauthenticated users
- Network namespace isolation deferred (requires veth pair setup, would break TCP port proxying)

### Phase My Way (CLI Overhaul)
- CLI commands now HTTP to the running server instead of creating local Hypervisors
- `ten spawn api:prod`, `ten stop api:prod` use consistent colon notation everywhere
- Added server-side mutation API: POST /api/instances/spawn, DELETE /api/instances/:id, POST /api/instances/:id/restart, PUT /api/instances/:id/weight, GET /api/instances/:id/health, POST /api/deploy, POST /api/route
- Added `ten logs` command with filters (--level, --search, --limit) and real-time follow mode (-f)
- Added `ten init` command to scaffold a new tenement.toml with framework detection (Python, Node, Go, Rust)
- Improved `ten ps` output: shows idle time, instance count, server URL
- API token auto-read from `{data_dir}/api_token` file, with `--server` and `--token` global flags
- `ten token-gen` now saves plaintext token to file for CLI auto-read
- Fixed Axum path parameters (`:id` not `{id}` for Axum 0.7)
- Fixed preexisting test failures: config interpolation, doctest assertions, flaky e2e timeouts, test isolation (unique socket paths per test)
- Added `tokio-test` dev dependency for doctests
- Created socket parent directories automatically on spawn
- 559 tests passing (was 336 passing with 4 failures)

## v0.1.4

### Phase Deployment (Deployment Tooling)
- Weighted routing for blue-green/canary deployments (`ten weight`, `select_weighted`)
- `ten deploy api --version v2` spawns instance and waits for health check
- `ten route api --from v1 --to v2` atomic traffic swap
- `deploy_and_wait_healthy()` with configurable timeout
- `route_swap()` for atomic weight updates
- Support both routing patterns: `{id}.{process}.{domain}` (direct) and `{process}.{domain}` (weighted)
- 22 new tests for traffic distribution and deployment workflows

### Phase Firecracker (VM Runtime)
- Runtime trait abstraction (`Runtime`, `RuntimeHandle`, `SpawnConfig`)
- Firecracker runtime implementation (spawn via HTTP API, lifecycle management)
- QEMU runtime skeleton
- VSOCK-aware health checks (CONNECT protocol)
- Config parsing for VM-specific fields (`kernel`, `rootfs`, `memory_mb`, `vcpus`, `vsock_port`)
- Note: implemented but untested on KVM hardware

## v0.1.0

### Core
- Process supervision with auto-restart and exponential backoff
- Subdomain routing (`prod.api.example.com` to `api:prod`)
- Unix socket and TCP port proxy for request routing
- Scale-to-zero with wake-on-request (`idle_timeout`)
- Port allocator for TCP ports (30000-40000 range)
- TOML configuration with `[service.X]` and `[process.X]` sections
- Environment variable interpolation (`{name}`, `{id}`, `{data_dir}`, `{port}`)
- Auto-spawn configured instances on boot via `[instances]` section

### Isolation
- **Namespace isolation** (default) -- Zero-overhead `/proc` protection via Linux PID + Mount namespaces
- **Process isolation** -- Bare process, no isolation (development)
- **Sandbox isolation** -- gVisor syscall filtering for untrusted code (feature flag)
- Resource limits via cgroups v2 (`memory_limit_mb`, `cpu_shares`)
- Storage quotas per instance (`storage_quota_mb`, `storage_persist`)

### Production
- Built-in TLS with Let's Encrypt via `rustls-acme` (`ten serve --tls`)
- Caddy integration for wildcard certificates (`ten caddy`)
- systemd service installation (`ten install`)
- HTTP-to-HTTPS redirect server
- ACME error tracking with troubleshooting hints

### Observability
- Svelte dashboard at root domain
- Prometheus metrics at `/metrics` (counters, gauges, histograms with labels)
- Log capture (stdout/stderr) with SQLite persistence and FTS5 full-text search
- Log streaming via SSE (`/api/logs/stream`)
- Bearer token authentication (Argon2 hashed, stored in SQLite)

### CLI
- `ten serve` -- Start server with optional TLS
- `ten spawn/stop/restart` -- Instance management
- `ten ps` -- List instances with health, uptime, weight
- `ten health` -- Check instance health
- `ten weight` -- Set traffic weight (0-100)
- `ten deploy` -- Deploy and wait for healthy
- `ten route` -- Atomic traffic swap
- `ten config` -- Show configuration
- `ten token-gen` -- Generate API token
- `ten install` -- Install as systemd service
- `ten caddy` -- Generate Caddyfile

### Fleet Mode (slum)
- Multi-server URL registry and reverse proxy
- Server registration and deregistration
- Tenant routing (domain to server mapping)
- SQLite-based coordination
