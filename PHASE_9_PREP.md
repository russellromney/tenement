# Phase 9: Deployment Tooling + Slum Simplification

## TL;DR

1. **Slum is ~90% done** - just needs health check loop
2. **Real work is deployment tooling** - blue/green, canary, weighted routing
3. **No provisioning code needed** - just register URLs, route traffic
4. **Scales from simple to paranoid** - add redundancy as needed

---

## Architecture

Slum is a **stateless URL registry + router**. It doesn't provision anything.

```
Your SaaS / Terraform / Fly CLI
            │
            │ provisions tenements (out of band)
            ▼
┌─────────────────────────────────────────────┐
│                   Slum                       │
│  • Knows tenement URLs                       │
│  • Health checks them                        │
│  • Routes traffic to healthy ones            │
└─────────────────────────────────────────────┘
            │
            ▼
    Tenements (anywhere)
```

**Slum's entire job:**
1. `register(url)` - "this tenement exists"
2. `health_check()` - poll URLs, mark healthy/unhealthy
3. `route(domain)` - send traffic to healthy tenement

That's it. How tenements get provisioned is someone else's problem.

---

## Deployment Configurations

| Setup | Slum | Tenements | Data Sync | Use Case |
|-------|------|-----------|-----------|----------|
| Simple | 1 | 1 | Local SQLite | Dev, small prod |
| Basic HA | 1 | 2 | Neon/Turso | Handle tenement failure |
| Multi-region | 1 | 3 | WALsync to Tigris | Latency + region failure |
| Full paranoid | 3 (raft) | 3+ | WALsync | Survive anything |

**Start simple, add redundancy when needed.**

Most apps never need more than row 2.

---

## What Exists Today

**Slum (`slum/src/`):**
- ✅ Server registry (add/list/delete URLs)
- ✅ Tenant routing (domain → server lookup)
- ✅ HTTP proxy (routes requests to tenements)
- ✅ 29 tests passing
- ❌ Background health check loop (small addition)

**Tenement:**
- ✅ Multiple instances per service
- ✅ Health checks per instance
- ✅ Metrics per instance
- ✅ Weighted routing (`ten weight`, `select_weighted`)
- ❌ Blue/green commands (`ten deploy`, `ten route`)
- ❌ Canary commands (uses weighted routing)

---

## Phase 9 Work

### Priority 1: Deployment Tooling (tenement)

**Blue/Green:**
```bash
ten deploy api --version v2              # Spawn api:v2, wait healthy
ten route api --from v1 --to v2          # Atomic swap
ten stop api:v1                          # Cleanup old version
```

**Canary:**
```bash
ten deploy api --version v2 --weight 10  # 10% to v2
ten weight api:v2 50                     # Increase to 50%
ten weight api:v2 100                    # Full rollout
ten stop api:v1                          # Cleanup
```

**Implementation:**
- Weighted routing in proxy (~50 LOC)
- `ten deploy` command (spawn + wait healthy)
- `ten route` / `ten weight` commands
- Instance version tagging

### Priority 2: Slum Health Loop

```rust
// Run every 30s
async fn health_check_loop(db: &SlumDb, client: &Client) {
    for server in db.list_servers().await {
        let status = match client.get(&server.url).await {
            Ok(_) => ServerStatus::Online,
            Err(_) => ServerStatus::Offline,
        };
        db.update_server_status(&server.id, status).await;
    }
}
```

~30 LOC. Just poll URLs, update status.

### Priority 3: Failure Webhook (optional)

```rust
// If server stays unhealthy for 5 minutes
if server.unhealthy_since > Duration::minutes(5) {
    webhook.notify(&server).await;  // Your SaaS handles it
}
```

Slum stays dumb. Your SaaS decides what to do (scale up, page someone, etc.).

---

## What We're NOT Building

- ❌ Provisioning operators (Fly/Hetzner/SSH)
- ❌ Complex raft consensus (unless needed)
- ❌ Auto-scaling logic
- ❌ Secrets management
- ❌ CI/CD integration

These are either platform concerns or SaaS concerns.

---

## Implementation Plan

**Session 1: Weighted Routing - DONE**
- [x] Add weight field to instances (0-100, default 100)
- [x] Implement weighted selection in proxy (`select_weighted`)
- [x] `ten weight api:v2 50` CLI command
- [x] `ten ps` shows weight column
- [x] Support both routing patterns:
  - `{id}.{process}.{domain}` → direct route
  - `{process}.{domain}` → weighted route
- [x] 11 tests for traffic distribution

**Session 2: Deploy Commands - DONE**
- [x] `ten deploy api --version v2` - spawn + wait healthy
- [x] `ten route api --from v1 --to v2` - atomic swap
- [x] `deploy_and_wait_healthy()` method with configurable timeout
- [x] `route_swap()` method for atomic weight updates
- [x] 11 new tests (deploy, route, blue/green workflow, canary workflow)

**Session 3: Slum Health Loop**
- Background polling task
- Auto-update server status
- Optional webhook on prolonged failure

**Session 4: Polish + Docs**
- CLI help improvements
- Deployment guide
- Example configs

**Estimated: 4 sessions, ~500 LOC, ~20 new tests**

---

## Example Multi-Region Setup

```bash
# Provision tenements (using Fly, Terraform, whatever)
fly deploy --region iad
fly deploy --region fra
fly deploy --region syd

# Register with Slum
slum server add iad https://myapp-iad.fly.dev
slum server add fra https://myapp-fra.fly.dev
slum server add syd https://myapp-syd.fly.dev

# Add tenant routing
slum tenant add acme acme.example.com iad api prod

# Slum handles:
# - Health checks all three
# - Routes acme.example.com → iad (or failover to fra/syd)
# - You get paged if all three die
```

---

## Summary

| Component | Status | Work Remaining |
|-----------|--------|----------------|
| Slum routing | ✅ Done | Health loop (~30 LOC) |
| Weighted routing | ❌ | ~50 LOC in tenement proxy |
| Blue/green CLI | ❌ | ~100 LOC |
| Canary CLI | ❌ | ~100 LOC |
| `ten deploy` | ❌ | ~150 LOC |

**Total: ~4 sessions, ~500 LOC**

Much simpler than the original 7-session plan with 2K LOC of operator code.
