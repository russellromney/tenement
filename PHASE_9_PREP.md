# Phase 9 Prep: Slum Multi-Provider Orchestration

**Status:** Foundation complete, operators pending

## Overview

Slum orchestrates multiple tenement servers across different cloud providers, providing:
- Unified routing to tenant-specific instances
- Server health monitoring
- Metrics/log aggregation
- Multi-tenant isolation

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

---

## ✅ What's Implemented (Solid Foundation)

### 1. Database Layer (`slum/src/db.rs`)

**Schema:**
```sql
servers (
  id, name, url, region,
  status [online|offline|degraded|unknown],
  last_seen, created_at
)

tenants (
  id, name, domain, server_id,
  process, instance_id, created_at,
  UNIQUE(domain), FK(server_id → servers.id)
)
```

**Operations:**
- Full CRUD for servers and tenants
- `route(domain)` → (Tenant, Server) lookup
- Status updates with timestamp tracking
- Foreign key constraints (cascade protection)
- SQLite with WAL mode

**Test Coverage:** 9 unit tests (100% pass)

### 2. HTTP Server (`slum/src/server.rs`)

**API Endpoints:**
```
GET  /health                    → Health check
GET  /api/servers               → List servers
POST /api/servers               → Add server
GET  /api/servers/:id           → Get server
DELETE /api/servers/:id         → Delete server
POST /api/servers/:id/status    → Update status

GET  /api/tenants               → List tenants
POST /api/tenants               → Add tenant
GET  /api/tenants/:id           → Get tenant
DELETE /api/tenants/:id         → Delete tenant

GET  /api/metrics               → Aggregated Prometheus metrics
GET  /api/logs                  → Aggregated logs (stub)

FALLBACK (all other routes)     → Proxy to tenant server
```

**Proxy Logic:**
1. Extract domain from `Host` header
2. Lookup tenant + server via `db.route(domain)`
3. Check server.status (reject if offline)
4. Build target: `{instance_id}.{process}.{server_url}`
5. Proxy request with Hyper client

**Test Coverage:** 20 integration tests (100% pass)

### 3. Test Suite (`slum/tests/integration.rs`)

**Scenarios Covered:**
- Server health status updates
- Status transitions (online → degraded → offline)
- Tenant routing by domain
- Multiple tenants per server
- Multiple servers with different tenants
- Tenant migration between servers
- Foreign key constraint validation
- Server offline handling
- Domain uniqueness enforcement

---

## ❌ What's Missing (Phase 9 Work)

### 1. Operators (Provider-Specific Provisioning)

**Not Yet Implemented:**
```rust
trait Operator {
    async fn provision(&self, config: &TenementConfig) -> Result<Server>;
    async fn deprovision(&self, server_id: &str) -> Result<()>;
    async fn check_health(&self, server_id: &str) -> Result<ServerStatus>;
}
```

**Required Operators:**

#### `LocalOperator`
- Spawns tenement on same machine as slum
- Uses systemd or direct process spawn
- **Use case:** Development, single-server deployment

#### `SshOperator`
- Generic SSH-based provisioning
- Works with any VPS (DigitalOcean, Linode, etc.)
- **Steps:**
  1. SSH to remote host
  2. Install tenement binary
  3. Copy tenement.toml config
  4. Start as systemd service
  5. Return server URL

#### `FlyOperator`
- Provisions Fly.io Machines via API
- **API:** `https://api.machines.dev/v1/apps/{app}/machines`
- **Steps:**
  1. Create machine with tenement image
  2. Set environment variables
  3. Allocate IP address
  4. Return `*.fly.dev` URL

#### `HetznerOperator`
- Provisions Hetzner Cloud servers via API
- **API:** `https://api.hetzner.cloud/v1/servers`
- **Steps:**
  1. Create server (CX21, fsn1)
  2. Install tenement via cloud-init
  3. Configure firewall rules
  4. Return server IP

### 2. CLI Binary (`slum-cli/src/main.rs`)

**Not Yet Created.** Needs:

```bash
slum serve [--port 8080]           # Start slum server
slum server add <id> <url>         # Manually add server
slum server list                   # List all servers
slum server status <id> <status>   # Update server status
slum server delete <id>            # Remove server

slum tenant add <id> <domain> <server-id> <process> <instance>
slum tenant list                   # List all tenants
slum tenant migrate <id> <new-server-id>
slum tenant delete <id>

slum provision <config.toml>       # Auto-provision via operators
slum deprovision <server-id>       # Remove server and migrate tenants
```

### 3. Configuration Format (`slum.toml`)

**Example config (not yet parsed):**

```toml
# Slum control plane settings
[slum]
port = 9000
db_path = "/var/lib/slum/slum.db"

# Define operators (provisioners)
[operators.local]
type = "local"

[operators.fly-prod]
type = "fly"
org = "my-org"
region = "iad"
image = "tenement:latest"

[operators.hetzner-eu]
type = "hetzner"
token_env = "HETZNER_API_TOKEN"
location = "fsn1"
server_type = "cx21"

[operators.ssh-vps]
type = "ssh"
host = "vps.example.com"
user = "root"
key_path = "~/.ssh/id_rsa"

# Define tenement servers (will be provisioned)
[tenement.api-prod]
operator = "fly-prod"
config = "configs/api-prod.toml"

[tenement.api-staging]
operator = "local"
config = "configs/api-staging.toml"

[tenement.workers-eu]
operator = "hetzner-eu"
config = "configs/workers.toml"

# Define tenant routing
[tenants.acme]
domain = "acme.example.com"
server = "api-prod"
process = "api"
instance = "prod"

[tenants.acme-staging]
domain = "staging.acme.example.com"
server = "api-staging"
process = "api"
instance = "staging"
```

### 4. High Availability (Future)

**Not Yet Designed:**

#### Option A: Litestream Replication
```
Leader slum → writes SQLite → Litestream → S3/R2
                                              ↓
Follower slum(s) ← continuously pulls ← S3/R2
```

- Leader handles all writes
- Followers sync from cloud storage (~ms lag)
- Failover via DNS/health check switch

#### Option B: raft-lite Embedded Consensus
```
Slum1 (leader) ──┐
Slum2 (follower) ├─ Raft cluster
Slum3 (follower) ┘
```

- Built-in leader election
- No external dependencies
- More complex but more robust

**Decision needed:** Pick one for Phase 9 implementation

### 5. Metrics Aggregation (Incomplete)

**Current:** Fetches `/metrics` from each server, returns raw text

**Needed:**
- Parse Prometheus metrics from all servers
- Merge duplicate metric families
- Add `server_id` label to distinguish sources
- Return unified Prometheus output

**Example merged output:**
```
# HELP tenement_instances_up Number of running instances
# TYPE tenement_instances_up gauge
tenement_instances_up{server_id="srv1"} 5
tenement_instances_up{server_id="srv2"} 3

# HELP tenement_request_count Total requests
# TYPE tenement_request_count counter
tenement_request_count{server_id="srv1"} 1234
tenement_request_count{server_id="srv2"} 567
```

### 6. Log Aggregation (Stub)

**Current:** Returns placeholder JSON

**Needed:**
- Stream logs from all servers via SSE or WebSocket
- Merge by timestamp
- Filter by server/tenant/level
- Full-text search across all servers

---

## Architecture Decisions

### 1. Operator Pattern

**Trait-based abstraction:**
```rust
#[async_trait]
trait Operator {
    fn name(&self) -> &str;
    async fn provision(&self, config: &TenementConfig) -> Result<ProvisionResult>;
    async fn deprovision(&self, server_id: &str) -> Result<()>;
    async fn check_health(&self, server_id: &str) -> Result<ServerStatus>;
}

struct ProvisionResult {
    server_id: String,
    url: String,         // HTTP endpoint for this server
    region: String,
    metadata: HashMap<String, String>,
}
```

**Operator Registry:**
```rust
struct OperatorRegistry {
    operators: HashMap<String, Box<dyn Operator>>,
}

impl OperatorRegistry {
    fn register(&mut self, name: &str, operator: Box<dyn Operator>);
    fn get(&self, name: &str) -> Option<&dyn Operator>;
}
```

### 2. Config Loading

**Two-phase approach:**
1. Load `slum.toml` → parse `[operators]` and `[tenement.X]` sections
2. For each tenement, load its `tenement.toml` config file
3. Call appropriate operator to provision

### 3. Server Discovery

**Options:**
- **Manual:** Operators register servers in slum DB on provision
- **Auto-discovery:** Slum polls known URLs for tenement health endpoints
- **Hybrid:** Provision registers + periodic health checks update status

**Recommendation:** Manual registration with health check polling

### 4. Tenant Migration

**Zero-downtime migration:**
1. Provision new server (if needed)
2. Spawn instance on new server
3. Wait for health check = healthy
4. Update tenant routing in DB (atomic)
5. Wait for in-flight requests to drain on old server
6. Stop old instance

**Rollback:** Keep old instance running until new one stable

---

## File Structure Proposal

```
slum/
├── src/
│   ├── lib.rs                  # ✅ Exists
│   ├── db.rs                   # ✅ Exists
│   ├── server.rs               # ✅ Exists
│   ├── config.rs               # ❌ NEW - Parse slum.toml
│   ├── operators/
│   │   ├── mod.rs              # ❌ NEW - Trait definition
│   │   ├── local.rs            # ❌ NEW - LocalOperator
│   │   ├── ssh.rs              # ❌ NEW - SshOperator
│   │   ├── fly.rs              # ❌ NEW - FlyOperator
│   │   └── hetzner.rs          # ❌ NEW - HetznerOperator
│   ├── provisioner.rs          # ❌ NEW - Orchestrates operators
│   ├── health.rs               # ❌ NEW - Health check loop
│   └── metrics.rs              # ❌ NEW - Metrics aggregation logic
│
├── cli/
│   └── src/
│       └── main.rs             # ❌ NEW - slum CLI binary
│
├── tests/
│   └── integration.rs          # ✅ Exists (20 tests)
│
├── Cargo.toml                  # ✅ Exists (lib only)
└── examples/
    └── slum.toml               # ❌ NEW - Example config
```

---

## Implementation Roadmap

### Session 1: Operator Foundation
- [ ] Create `slum/src/operators/mod.rs` with `Operator` trait
- [ ] Implement `LocalOperator` (simplest - systemd spawn)
- [ ] Add `OperatorRegistry` and tests
- [ ] Update ROADMAP to mark LocalOperator complete

### Session 2: SSH Operator
- [ ] Implement `SshOperator` (generic VPS provisioning)
- [ ] SSH connection, file upload, systemd setup
- [ ] Integration test with local SSH server

### Session 3: Cloud Operators (Fly + Hetzner)
- [ ] Implement `FlyOperator` (Fly Machines API)
- [ ] Implement `HetznerOperator` (Hetzner Cloud API)
- [ ] API authentication, server creation, health checks

### Session 4: Config + Provisioner
- [ ] Create `config.rs` to parse `slum.toml`
- [ ] Create `provisioner.rs` to orchestrate provision/deprovision
- [ ] Integration tests for full provision workflow

### Session 5: CLI Binary
- [ ] Create `slum-cli/src/main.rs` with clap
- [ ] Commands: serve, server, tenant, provision
- [ ] E2E test: `slum provision → adds servers → tenants route`

### Session 6: Health Checks + Metrics
- [ ] Background health check loop (poll servers every 30s)
- [ ] Auto-update server status in DB
- [ ] Metrics aggregation with server_id labels

### Session 7: HA + Log Aggregation
- [ ] Choose: Litestream vs raft-lite
- [ ] Implement log streaming via SSE
- [ ] Full production deployment guide

---

## Key Design Principles

1. **Fail explicitly** - No silent fallbacks, operators error loudly
2. **Idempotent provisioning** - Re-running provision should be safe
3. **Graceful degradation** - Offline server doesn't break routing for others
4. **Zero-downtime migrations** - Atomic tenant routing updates
5. **Observable** - All operations logged, metrics for every server
6. **Minimal external deps** - SQLite + HTTP, no Redis/etcd required

---

## Testing Strategy

**Unit Tests:**
- Each operator provisions/deprovisions correctly
- Config parsing validates all fields
- Provisioner coordinates multi-server setup

**Integration Tests:**
- Full provision → route → deprovision flow
- Health check updates server status automatically
- Metrics aggregation merges from multiple servers
- Tenant migration with zero dropped requests

**E2E Tests:**
- Provision 3 servers (local, SSH, Fly)
- Add 5 tenants across servers
- Migrate tenant from local → Fly
- Deprovision SSH server
- Verify all tenants still route correctly

---

## Next Actions

**Immediate (Session 1):**
1. Create `slum/src/operators/mod.rs` with `Operator` trait
2. Implement `LocalOperator` as proof-of-concept
3. Write 5-10 unit tests for LocalOperator

**Short-term (Sessions 2-3):**
1. Implement SSH and cloud operators
2. Build provisioner orchestration layer

**Medium-term (Sessions 4-5):**
1. Create CLI binary with full command set
2. Write comprehensive integration tests

**Long-term (Sessions 6-7):**
1. Add HA via Litestream or raft-lite
2. Complete metrics/log aggregation
3. Production deployment documentation

---

## Questions to Answer

1. **HA Decision:** Litestream (simpler) vs raft-lite (more robust)?
2. **Operator Priority:** Which operators to build first? (LocalOperator → SshOperator → FlyOperator → HetznerOperator?)
3. **Migration Strategy:** Blue-green or rolling migration?
4. **Metrics Format:** Parse and merge Prometheus, or proxy raw?
5. **Log Aggregation:** SSE stream or batch API endpoint?

---

## References

- **Fly Machines API:** https://fly.io/docs/machines/api/
- **Hetzner Cloud API:** https://docs.hetzner.cloud/
- **Litestream:** https://litestream.io/
- **raft-lite:** https://github.com/liangrunda/raft-lite
- **Prometheus Format:** https://prometheus.io/docs/instrumenting/exposition_formats/

---

## Summary

**Slum foundation is solid:**
- ✅ Database layer with routing
- ✅ HTTP server with proxy
- ✅ 20 integration tests passing

**Missing pieces for Phase 9:**
- ❌ Operator implementations (local, SSH, cloud)
- ❌ CLI binary
- ❌ Config parsing (`slum.toml`)
- ❌ Provisioner orchestration
- ❌ Health check loop
- ❌ Metrics/log aggregation

**Estimated Work:**
- ~7 sessions to complete Phase 9
- ~2,000 LOC across operators, CLI, provisioner
- ~30-40 additional tests

**Ready to proceed with Session 1: LocalOperator implementation.**
