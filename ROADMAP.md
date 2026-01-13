# Roadmap

See [CHANGELOG.md](CHANGELOG.md) for what's been implemented.

## In Progress

- Slum health check loop

## Planned

### Deployment & Rollout System

#### The Two Deployment Dimensions

There are two orthogonal concerns:

1. **Which tenants** get the deploy (tenant-level rollout)
   - "Roll out to canary tenants first, then exponential to the rest"
   - Targeting: `api:prod:canary*` then `api:prod:*`

2. **Canary within a tenant** (version-level weighted routing)
   - "Give 10% of tenant1's traffic to v2"
   - Weighted: `api:prod:tenant1:v1` (90%) + `api:prod:tenant1:v2` (10%)

Both should be supported. A canary tenant can also have per-tenant canary versioning.

#### Instance ID Structure

Four-part colon-separated ID: `{process}:{env}:{tenant}:{version}`

```
api:prod:tenant1:v1      # production tenant1 running v1
api:prod:tenant1:v2      # same tenant, canary version
api:prod:canary1:v2      # canary tenant on new version
api:dev:test1:abc123f    # dev tenant on specific git commit
api:staging:demo:blue    # staging with blue-green naming
```

**Version is freeform** - tenement doesn't interpret it:
- Semver: `v2.1.0`
- Names: `blue`, `green`, `canary`
- Git hash: `abc123f`
- Date: `2024-01-13`
- PR: `pr-456`

#### CLI Syntax

Both colon notation and flags, equivalent:

```bash
# Colon notation (concise)
ten deploy api:prod:tenant1:v2
ten deploy api:prod:tenant1:v2 --weight 10

# Flag notation (explicit)
ten deploy --process api --env prod --tenant tenant1 --version v2
ten deploy -p api -e prod -t tenant1 -v v2 -w 10

# Mixed - target + options
ten deploy api:prod:tenant1 --version v2 --weight 10

# Wildcards for batch operations
ten deploy api:prod:*:v2           # all tenants, specific version
ten deploy api:prod:canary*        # canary tenants
ten ps api:prod:*                  # list all prod instances
```

Flags override colon notation if both provided.

#### Deployment Workflows

**Simple rollout** (no version split):
```bash
ten deploy api:prod:tenant1
# Stops existing instance, starts new one with current code
# Single instance per tenant, version implicit
```

**Per-tenant canary** (weighted traffic within one tenant):
```bash
ten deploy api:prod:tenant1 --version v2 --weight 10
# Now: api:prod:tenant1:v1 (existing, 100%) + api:prod:tenant1:v2 (new, 10%)

ten weight api:prod:tenant1:v2 50    # increase canary
ten route api:prod:tenant1 --from v1 --to v2   # atomic cutover
ten stop api:prod:tenant1:v1         # cleanup old version
```

**Tenant-level canary** (some tenants get new version first):
```bash
# Phase 1: canary tenants
ten deploy api:prod:canary1 --version v2
ten deploy api:prod:canary2 --version v2

# Phase 2: first batch
ten deploy api:prod:tenant[1-10] --version v2

# Phase 3: all remaining
ten deploy api:prod:* --version v2
```

**Combined** (canary tenant with canary weight):
```bash
ten deploy api:prod:canary1 --version v2 --weight 10
# Canary tenant gets 10% traffic to v2, 90% to v1
# All other tenants still 100% on v1
```

**Blue-green**:
```bash
ten deploy api:prod:tenant1 --version green --weight 0
# Test green directly: curl green.tenant1.api.example.com
ten route api:prod:tenant1 --from blue --to green
ten stop api:prod:tenant1:blue
```

#### Safety Rails

```bash
# Broad patterns require confirmation
ten deploy api:prod:*
> This will deploy to 47 instances. Continue? [y/N]

# Skip confirmation
ten deploy api:prod:* --yes

# Preview without executing
ten deploy api:prod:* --dry-run
> Would deploy to:
>   api:prod:tenant1
>   api:prod:tenant2
>   ...
```

#### Routing Logic

Request for `tenant1.api.example.com`:
1. Parse: process=`api`, tenant=`tenant1`
2. Find all instances matching `api:*:tenant1:*`
3. Filter by env if configured (e.g., prod only for this domain)
4. Weighted random select among matching instances
5. Route to selected instance's socket

#### Open Design Questions

**Where does code come from?**
- Option A: Git-based - `ten deploy` does `git pull && build && restart`
- Option B: Binary swap - build externally, `ten deploy --binary /path/to/new`
- Option C: Implicit - whatever's at configured path, just restart
- Leaning toward (C) with optional (A) for convenience

**Deployment history/audit log:**
- Track: who, what, when, success/fail
- SQLite table? Append-only log file?
- Rollback needs to know "previous version"

**Failed deploy handling:**
- Current: health check timeout → stop unhealthy instance → error
- Missing: auto-rollback of weight changes if canary fails
- Options: keep manual, health-based auto-revert, circuit breaker

**Slum coordination for multi-server:**
- Option A: Slum as orchestrator - `slum deploy api:prod:*` fans out
- Option B: Slum as registry - tenements pull desired state
- Option C: Keep decoupled - operators SSH to each server
- Single-server is priority; multi-server can stay manual initially

**Scale-to-zero interaction:**
- Deploying to sleeping instance: wake it? or update config for next wake?
- Weight=0 should prevent wake-on-request (confirm this works)

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
