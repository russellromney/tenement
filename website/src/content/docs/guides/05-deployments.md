---
title: Deployment Patterns
description: Blue-green, canary, and weighted routing deployments
---

tenement supports zero-downtime deployments through weighted routing. Route traffic between instances using the `ten weight` command.

## Weighted Routing

Traffic to `{service}.{domain}` is distributed across all instances of that service based on their weights.

```bash
# Set instance weight (0-100)
ten weight api:v1 80
ten weight api:v2 20

# Check current weights
ten ps
```

```
INSTANCE     SOCKET                           UPTIME    HEALTH    WEIGHT
api:v1       /tmp/tenement/api-v1.sock        2d        healthy   80
api:v2       /tmp/tenement/api-v2.sock        5m        healthy   20
```

**How it works:**
- Requests to `api.example.com` are load-balanced by weight
- Direct requests to `v1.api.example.com` bypass weights (always route to v1)
- Weight 0 excludes instance from traffic (but keeps it running)

## Blue-Green Deployment

Deploy new versions with zero downtime by switching all traffic at once.

### 1. Deploy New Version

```bash
# Current: api:blue handling all traffic
ten ps
# api:blue   weight=100

# Deploy new version as "green"
ten spawn api --id green
```

### 2. Test Green

```bash
# Direct requests to green bypass routing
curl https://green.api.example.com/health
```

### 3. Switch Traffic

```bash
# Instant cutover
ten weight api:blue 0
ten weight api:green 100
```

### 4. Cleanup

After verifying green works:

```bash
ten stop api:blue
```

### Rollback

If issues arise:

```bash
ten weight api:green 0
ten weight api:blue 100
```

## Canary Deployment

Gradually shift traffic to test new versions with real users.

### 1. Deploy Canary

```bash
# Current: api:v1 at 100%
ten spawn api --id v2
ten weight api:v2 0  # Start at 0%
```

### 2. Gradual Rollout

```bash
# 5% canary
ten weight api:v1 95
ten weight api:v2 5

# Monitor for errors...

# 25% canary
ten weight api:v1 75
ten weight api:v2 25

# 50/50
ten weight api:v1 50
ten weight api:v2 50

# Full rollout
ten weight api:v1 0
ten weight api:v2 100
```

### 3. Monitor

Watch metrics during rollout:

```bash
# Prometheus metrics
curl https://example.com/metrics | grep api

# Instance health
ten ps
```

### Rollback

At any point:

```bash
ten weight api:v2 0
ten weight api:v1 100
```

## A/B Testing

Run experiments by splitting traffic between variants.

```bash
# 50/50 split
ten spawn api --id control
ten spawn api --id experiment

ten weight api:control 50
ten weight api:experiment 50
```

Each variant can run different code, configuration, or flags.

## Multi-Instance Load Balancing

Scale horizontally by running multiple instances of the same service.

```bash
# Spawn multiple instances
ten spawn api --id prod-1
ten spawn api --id prod-2
ten spawn api --id prod-3

# Equal weights for round-robin-ish distribution
ten weight api:prod-1 33
ten weight api:prod-2 33
ten weight api:prod-3 34
```

Traffic is distributed randomly based on weights.

## Deployment Commands

The `ten deploy` and `ten route` commands automate common deployment patterns:

### ten deploy

Spawn a new version and wait for it to become healthy:

```bash
# Deploy v2 with full traffic
ten deploy api --version v2

# Deploy v2 with initial weight (for canary)
ten deploy api --version v2 --weight 10

# Deploy with custom health timeout
ten deploy api --version v2 --timeout 60
```

The deploy command:
1. Spawns a new instance with the version as instance ID
2. Waits for health checks to pass (default 30s timeout)
3. Sets the initial traffic weight

### ten route

Atomically swap traffic between versions (blue-green):

```bash
# Route all traffic from v1 to v2
ten route api --from v1 --to v2
```

This sets `v1` weight to 0 and `v2` weight to 100 in a single operation.

## Best Practices

### Always Test First

```bash
# Spawn new version
ten spawn api --id new

# Test directly (bypasses routing)
curl https://new.api.example.com/health
curl https://new.api.example.com/test-endpoint

# Then route traffic
ten weight api:new 10
```

### Monitor During Rollout

- Watch error rates in logs
- Check Prometheus metrics
- Verify health checks pass

### Keep Old Version Running

Don't stop the old version until the new one is verified:

```bash
# Old version at 0% but still running
ten weight api:old 0

# Can instant rollback if needed
ten weight api:old 100
ten weight api:new 0
```

### Use Instance Auto-Start for Critical Services

```toml
[instances]
api = ["prod"]  # Always spawn on boot
```

## Routing Reference

| URL Pattern | Behavior |
|-------------|----------|
| `{id}.{service}.{domain}` | Direct to specific instance |
| `{service}.{domain}` | Weighted routing across instances |
| `{domain}` | Dashboard |

**Examples:**
- `v2.api.example.com` → always routes to `api:v2`
- `api.example.com` → weighted across all `api:*` instances
- `example.com` → dashboard

## Next Steps

- [Configuration Reference](/guides/03-configuration) - Full config options
- [Production Deployment](/guides/04-production) - TLS and systemd setup
- [Troubleshooting](/reference/troubleshooting) - Common issues
