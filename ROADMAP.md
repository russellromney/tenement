# Tenement Roadmap

## Current Status (v0.1)

**What works now:**
- Process supervision with auto-restart
- Subdomain routing
- Health checks over Unix sockets
- Log capture with search
- Bearer token auth
- Prometheus metrics
- Dashboard (Svelte)

---

## The Isolation Spectrum

The core vision: **one server, one CLI, multiple isolation levels.**

Optimized for: **overstuff servers with small programs that need isolation**.

```
Lightest                                              Heaviest
   |                                                      |
process  →  namespace  →  sandbox  →  wasm  →  vm
   |            |            |          |        |
 none       unshare       gvisor    wasmtime   firecracker
```

### Isolation Levels

| Level | Tool | Overhead | Startup | `/proc` Isolated | Syscall Filtered | Use Case |
|-------|------|----------|---------|------------------|------------------|----------|
| `process` | bare process | ~0 | <10ms | ❌ | ❌ | same trust boundary |
| `namespace` | unshare (PID+Mount) | ~0 | <10ms | ✅ | ❌ | **default** - trusted code |
| `sandbox` | gVisor (runsc) | ~20MB | <100ms | ✅ | ✅ | untrusted/multi-tenant |
| `wasm` | wasmtime | ~5MB | ~1ms | ✅ | N/A (no syscalls) | compiled languages (Rust/Go/C) |
| `vm` | Firecracker/QEMU | ~128MB | ~125ms | ✅ | ✅ (kernel) | compliance, custom kernel |

### Why `namespace` as default?

Linux namespaces (PID + Mount) provide `/proc` isolation with:
- **Zero overhead** - kernel bookkeeping only
- **Zero dependencies** - built into Linux kernel (since 2008)
- **Instant startup** - no container/VM to spawn
- **Battle-tested** - same foundation as Docker, Kubernetes, systemd

For **trusted code** (your own apps, same trust boundary), this is sufficient. Environment variables are hidden between services, but syscalls are not filtered.

For **untrusted code** (third-party, multi-tenant), use `sandbox` which filters syscalls via gVisor.

### When to use VMs?

VMs add significant overhead but are available for edge cases:
- Compliance requirements that mandate full kernel isolation
- Custom kernel needs (specific kernel version, modules)
- Maximum paranoia

**Note:** VMs require KVM (bare metal or nested virt). Won't work on Fly.io or most cloud VMs.

### Target Config

```toml
# Default: namespace isolation (trusted code with /proc isolation)
[service.api]
command = "uv run python app.py"
# isolation = "namespace" (implicit default)

# No isolation (same trust boundary, shared /proc)
[service.internal]
command = "./internal-tool"
isolation = "process"

# Untrusted code (syscall filtering via gVisor)
[service.untrusted]
command = "./third-party-worker"
isolation = "sandbox"

# Full VM for paranoid isolation
[service.secure]
command = "./worker"
isolation = "vm"
profile = "minimal"
memory_mb = 256
vcpus = 1
```

---

## Phases

### Phase 1: VM Runtime (v0.2) - DONE

- [x] Runtime trait abstraction (`Runtime`, `RuntimeHandle`)
- [x] Process runtime (extracted from hypervisor)
- [x] Firecracker runtime (Linux + KVM)
- [x] QEMU runtime (cross-platform with HVF/KVM/TCG)
- [x] VSOCK integration for health checks
- [x] Config extensions (`runtime`, `kernel`, `rootfs`, etc.)
- [ ] Integration tests on KVM hardware

### Phase 2: Namespace Runtime (v0.3) - DONE

Linux namespace isolation - zero overhead `/proc` protection.

- [x] `NamespaceRuntime` using `unshare` (PID + Mount namespaces)
- [x] Private `/proc` mount per service
- [x] Environment variables invisible between services
- [x] No external dependencies (uses `nix` crate, kernel built-in)
- [x] Make `namespace` the default isolation level

**Implementation:**
```rust
// Uses unshare(2) syscall via nix crate
use nix::sched::{unshare, CloneFlags};
unshare(CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWNS)?;
// Mount private /proc
mount(Some("proc"), "/proc", Some("proc"), MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC, None::<&str>)?;
```

**Usage:** `namespace` is now the default runtime. Use `runtime = "process"` for explicit no-isolation. On non-Linux, fails loudly with clear error message.

### Phase 3: Config Refactor (v0.4) - DONE

- [x] Rename `[process.X]` to `[service.X]`
- [x] Add `isolation` field (replacing `runtime`)
- [x] **Default isolation = `namespace`** (trusted code with /proc isolation)
  - `process` for same trust boundary (no isolation)
  - `sandbox` for untrusted code (gVisor)
- [x] Backwards compat: `[process.X]` works as alias, `runtime` field works as alias
- [ ] VM profiles system (`~/.tenement/profiles/`) - deferred to later phase
  - Named profiles instead of raw kernel/rootfs paths
  - Built-in: `minimal`, `wolfi`
  - Custom profile support

**Implementation notes:**
- `Config::from_str()` merges both `[service.X]` and `[process.X]` sections
- `isolation` field accepts `#[serde(alias = "runtime")]` for backwards compat
- Error on duplicate: defining same service in both `[service.X]` and `[process.X]`
- CLI updated: `ten config` shows "Services" with isolation level

**Config format:** TOML only (`tenement.toml`)

**New format (preferred):**
```toml
# Defaults to namespace (trusted code with /proc isolation)
[service.api]
command = "./api"
# isolation = "namespace" (implicit default)

# No isolation (same trust boundary)
[service.internal]
command = "./internal-tool"
isolation = "process"

# Untrusted code
[service.worker]
command = "./third-party"
isolation = "sandbox"
```

**Legacy format (still supported):**
```toml
[process.api]
runtime = "process"  # 'runtime' is alias for 'isolation'
command = "./api"
```

### Phase 4: Sandbox Runtime (v0.5) - DONE

gVisor integration for syscall-filtered execution.

- [x] `SandboxRuntime` using `runsc` (gVisor)
- [x] OCI bundle generation (auto-generated from command config)
- [ ] Syscall policy configuration (deferred - using default gVisor policies)
- [x] Linux-only with clear error messages elsewhere
- [x] Helpful error messages:
  - gVisor not installed → installation instructions
  - macOS/Windows → suggest `isolation = "process"` for dev

**Implementation notes:**
- Uses host filesystem symlinks for rootfs (zero copy, instant setup)
- Socket directory bind-mounted for health check communication
- Feature-gated: compile with `--features sandbox`

**gVisor details:**
- ~20MB memory overhead
- ~1μs syscall latency (vs ~100ns native)
- <100ms startup
- OCI compatible
- Runs normal Linux binaries (no recompilation)

### Phase 5: WASM Runtime (v0.6)

WebAssembly sandbox for WASI-compiled workloads.

- [ ] `WasmRuntime` using wasmtime
- [ ] WASI support for filesystem/network (capability-based)
- [ ] Best for Rust/Go/C compiled to WASM

**WASM details:**
- ~5MB runtime overhead
- ~1ms cold start
- True sandbox: no syscalls unless granted
- **Limitation:** code must compile to WASM (no Python/Node/Ruby)
- **Limitation:** limited thread support (WASI threads evolving)

### Phase 6: Resource Limits (v0.7) - DONE

Metered utilities via cgroups v2 on Linux.

- [x] Memory limits (`memory_limit_mb` config field, cgroups v2 memory.max)
- [x] CPU limits (`cpu_shares` config field, cgroups v2 cpu.weight)
- [ ] Storage quotas per instance (future)
- [ ] Network bandwidth limits (future)

**Implementation notes:**
- New config fields: `memory_limit_mb` and `cpu_shares` in `[service.X]`
- Cgroup created at `/sys/fs/cgroup/tenement/{instance_id}/` on spawn
- Memory limit set via `memory.max`, CPU weight via `cpu.weight`
- Cgroup cleaned up on instance stop
- No-op on non-Linux (graceful degradation)

**Config example:**
```toml
[service.api]
command = "./api"
memory_limit_mb = 256    # Memory limit in MB
cpu_shares = 200         # CPU weight (1-10000, default 100)
```

### Phase 7: Hibernation (v0.8) - DONE

Scale to zero, wake on first request.

- [x] `idle_timeout` config field per process
- [x] `last_activity` tracking (updated on real requests, NOT health checks)
- [x] `is_idle()` helper for timeout detection
- [x] Reaper loop to auto-stop idle instances
- [x] `touch_activity()` / `spawn_and_wait()` for router integration
- [x] Wake-on-request in HTTP router (spawn if sleeping, wait for ready, proxy)
- [ ] Snapshot/restore for VM runtimes (future)
- [ ] Process state serialization for sandbox/wasm (future)

**Config:**
```toml
[service.api]
command = "uv run python app.py"
idle_timeout = 300  # stop after 5 mins idle (None = never)
```

### Phase 8: Comprehensive Testing - DONE

Test coverage expansion from 130 to 256 tests (+97%).

- [x] Hypervisor tests (lifecycle, health, errors, metrics, log capture)
- [x] Instance tests (InstanceId parsing, status serialization, uptime formatting)
- [x] Cgroup tests (resource limits, cgroup paths, Linux-specific)
- [x] Runtime/Process tests (spawn, handles, exit codes)
- [x] Logs tests (ring buffer, queries, search, async LogBuffer)
- [x] Store tests (SQLite, FTS5, rotation, ConfigStore)
- [x] Auth tests (token generation, Argon2 hashing, TokenStore)

**Test design principles:**
- Real behavior over mocking - tests use actual processes (`sleep`, `echo`, `env`)
- Real files - TempDir for actual file operations
- Edge cases - empty strings, long strings, special characters, boundaries
- Error paths - command not found, invalid input, nonexistent resources
- Platform awareness - Linux-specific tests marked `#[cfg(target_os = "linux")]` or `#[ignore]`

See [TEST_PLAN.md](TEST_PLAN.md) for full breakdown.

### Phase 8.5: Critical Infrastructure (v0.8.5) - DONE

Core fixes for production readiness.

- [x] **Unix socket proxy** - Actual request routing to backend processes via `hyperlocal`
- [x] **Auth middleware** - Bearer token authentication wired to API endpoints
  - Protected: `/api/instances`, `/api/logs`, `/api/logs/stream`
  - Public: `/health`, `/metrics`, `/`, `/assets/*`, subdomain routes
- [x] **Foreign key enforcement** - `PRAGMA foreign_keys = ON` in slum DB

**CLI tests:** 18 passing (including auth tests)
**Slum tests:** 9 passing (including FK constraint test)

See [FIX_PLAN.md](FIX_PLAN.md) for remaining P1/P2 improvements.
See [E2E_TESTING_PLAN.md](E2E_TESTING_PLAN.md) for comprehensive E2E test plan.

### Phase 8.6: E2E Test Infrastructure - IN PROGRESS

Comprehensive E2E testing foundation.

- [x] **Session 1: Test Infrastructure** - Shared utilities and fixtures
  - `tenement/tests/common/mod.rs` - Config builders, socket waiters, DB helpers
  - `tenement/tests/fixtures/` - mock_server.sh, slow_startup.sh, crash_on_health.sh, exit_immediately.sh
  - 9 verification tests passing
- [x] **Session 2: Auth Integration Tests (38 tests)** - Comprehensive auth coverage
  - `cli/tests/auth_integration.rs` - Full auth middleware testing
  - Core API auth, public endpoints, token formats, wrong schemes, malformed headers
  - Token rotation, subdomain bypass, edge cases, load testing
  - Added `cli/src/lib.rs` to expose server module for integration tests
- [ ] Session 3: Hypervisor Integration Tests (7 tests)
- [ ] Session 4: E2E Lifecycle Tests (9 tests)
- [ ] Session 5: Cgroup Lifecycle Tests (6 tests, Linux only)
- [ ] Session 6: Stress Tests (6 tests)
- [ ] Session 7: Performance Benchmarks (8 benchmarks)
- [ ] Session 8: Slum Integration Tests (5 tests)

**Total planned: 79 tests + 8 benchmarks** (38 auth + 7 hypervisor + 9 lifecycle + 6 cgroup + 6 stress + 5 slum)

### Phase 8.7: Code Quality Fixes - DONE

Bug fixes and improved observability from FIX_PLAN.md.

- [x] **P1-4: Race condition fix** - Atomic `get_and_touch()` for proxy requests
  - Prevents instance from being reaped between checking if running and touching activity
  - Single write lock instead of separate is_running + touch_activity + get calls
- [x] **P1-6: Cgroup warning order** - Check availability before limits
- [x] **P2-8: Cgroup cleanup logging** - Warn on PID migration and rmdir failures
- [x] **P3-11: Dashboard caching** - Cache-Control headers (24h for static, must-revalidate for HTML)
- [x] **P3-12: Auth verification logging** - Debug log for invalid password hash format
- [x] **P3-13: CPU weight clamping logging** - Info log when weight clamped to 1-10000
- [x] **P3-14: Idle timeout documentation** - Document that 0 means "never stop"

**Remaining (deferred - code aesthetics, not functional):**
- P2-7: QueryBuilder refactor (verbose but works)
- P2-9: TokenStore lifetime (intentional borrow semantics)
- P2-10: Slum route JOIN (N+1 fine for SQLite local queries)

### Phase 9: Slum - Multi-Provider Orchestration (v0.9)

Fleet orchestration across multiple tenements on different providers.

```
                    ┌─────────────┐
                    │    slum     │
                    │ (control)   │
                    └──────┬──────┘
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
   ┌────▼────┐        ┌────▼────┐        ┌────▼────┐
   │ tenement│        │ tenement│        │ tenement│
   │ (fly.io)│        │(hetzner)│        │ (local) │
   └─────────┘        └─────────┘        └─────────┘
```

**Operators** (provider-specific provisioning):
- [ ] `LocalOperator` - tenement on same machine as slum
- [ ] `SshOperator` - generic SSH-based (any VPS)
- [ ] `FlyOperator` - Fly Machines API
- [ ] `HetznerOperator` - Hetzner Cloud API

**High Availability** (single leader + failover):
```
Leader slum → writes SQLite → Litestream → S3/R2
                                              ↓
Follower slum(s) ← continuously pulls ← S3/R2
```
- Leader handles all writes
- Followers sync from cloud storage (millisecond lag)
- Failover via DNS/healthcheck → follower becomes leader
- Alternative: embed [raft-lite](https://github.com/liangrunda/raft-lite)

**Config:**
```toml
# slum.toml
[operators.local]
type = "local"

[operators.fly-prod]
type = "fly"
org = "my-org"
region = "iad"

[operators.hetzner-eu]
type = "hetzner"
location = "fsn1"
server_type = "cx21"

[tenement.api-prod]
operator = "fly-prod"
config = "tenement-api.toml"

[tenement.workers]
operator = "hetzner-eu"
config = "tenement-workers.toml"
```

### Phase 10: Infra Export (v1.0)

Export your tenement/slum config to cloud providers.

- [ ] Fly.io adapter (fly.toml generation)
- [ ] Terraform/Pulumi export
- [ ] Docker Compose export (for testing)

---

## Platform Support

**Linux only.** Tenement is a production tool for Linux servers.

| Isolation | Linux | macOS/Windows |
|-----------|-------|---------------|
| process | ✅ | ❌ |
| namespace | ✅ (default) | ❌ |
| wasm | ✅ | ❌ |
| sandbox | ✅ | ❌ |
| vm | ✅ KVM | ❌ |

For local development, test components individually and deploy to Linux (Fly.io, bare metal, etc.).

---

## Design Principles

1. **Same API, different walls** - All isolation levels use the same routing, supervision, and health check APIs
2. **Fail loudly** - When an isolation level isn't available, fail with clear error message suggesting alternatives
3. **No magic** - Explicit configuration, no auto-detection or silent fallback
4. **Single binary** - All runtimes compile into one `ten` binary with feature flags
5. **Linux only** - Production tool for Linux servers, no cross-platform compromises

---

## Security Model

### Environment Variable Isolation

| Isolation | Env Protection | Syscall Filtered | External Deps | Overhead |
|-----------|---------------|------------------|---------------|----------|
| `process` | ❌ None | ❌ | None | ~0 |
| `namespace` | ✅ Full | ❌ | None | ~0 |
| `sandbox` | ✅ Full | ✅ | gVisor | ~20MB |
| `wasm` | ✅ Full | N/A | wasmtime | ~5MB |
| `vm` | ✅ Full | ✅ (kernel) | KVM | ~128MB |

**When to use what:**
- `process` - same trust boundary, no isolation needed
- `namespace` (default) - trusted code, `/proc` isolated, zero overhead
- `sandbox` - untrusted/multi-tenant code, syscall filtering
- `wasm` - compiled languages (Rust/Go/C) with minimal overhead
- `vm` - compliance, custom kernel, maximum paranoia

### Secrets Management

Tenement does not manage secrets. Use external secrets managers:
- Fly secrets, Doppler, Vault, AWS Secrets Manager, etc.
- Inject at runtime, not in config files
- For `process` isolation: accept that co-tenants on same server share trust boundary

---

## References

- [Firecracker](https://github.com/firecracker-microvm/firecracker) - AWS's microVM
- [gVisor](https://gvisor.dev/) - Google's userspace kernel
- [wasmtime](https://wasmtime.dev/) - Bytecode Alliance WASM runtime
- [QEMU](https://www.qemu.org/) - Cross-platform virtualization
