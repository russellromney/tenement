---
title: The Economics
description: Why tenement works financially
---

## The Use Case

Run **1000 isolated services on a $5/month server**.

Each customer gets their own process (no multi-tenant data isolation complexity). You pay for only the 20 that are active at any moment.

## The Math

```
1000 services configured
├── 5-20 actually running (rarely active)
├── 980+ sleeping (zero cost)
├── Wake on request: user.service.example.com → spawn → proxy
└── Reap after idle: 5min no requests → kill → free resources
```

**On a $5/month machine (1 vCPU, 256MB RAM):**

- 1000 tenants
- ~2% active at any time = 20 running instances
- 20 × 20MB per instance = 400MB RAM
- Tenement overhead: ~10MB
- Fits with room to spare

**Economics:**

| Metric | Value |
|--------|-------|
| Cost per tenant | <$0.01/month |
| Charge per tenant | $5-10/month |
| Profit per tenant | $4.99-9.99/month |
| Margin | **500-1000x** |

## Three Deployment Models, One Codebase

Build your app as **single-tenant code** (simpler, fewer bugs). Deploy it three ways:

### SaaS (Multi-tenant Overstuffed)
- Infrastructure: Single $5 Fly.io machine
- Cost per customer: <$0.01/month
- Charge per customer: $5-10/month
- Margin: 500-1000x
- How: tenement spawns isolated process per user

### Enterprise (Dedicated)
- Infrastructure: Dedicated VM (customer's choice or yours)
- Cost per customer: ~$100/month
- Charge per customer: $500+/month
- Margin: 5x
- How: Same binary, different deployment

### Open Source (Self-hosted)
- Infrastructure: User's servers
- Cost per customer: $0 (theirs to maintain)
- Charge per customer: Free (or support/addons)
- Margin: Trust + adoption
- How: Same binary, no tenement layer

**Your code doesn't change.** Tenement handles the "make it multi-tenant" part at the infrastructure layer.

## Why This Works

| Alternative | Why it doesn't fit |
|---|---|
| **systemd units** | 1000 unit files, custom routing layer anyway, no built-in idle reap |
| **Docker** | ~100MB per container × 20 = 2GB for 20 instances. Overkill + slow startup |
| **Fly Machines** | $5/machine × 1000 services = $5000/month to have all instances running. Not viable |
| **k8s/Nomad** | Control plane overhead > your actual workloads on a small server |
| **Cloudflare Workers** | Can't run arbitrary processes, limited to their runtime environment |

## The Key Insight

At 1000 services that are rarely active, your marginal cost per additional tenant is essentially **zero** once you hit the machine's resource ceiling.

Traditional SaaS pays per instance. tenement pays once and serves 1000.

---

**Build single-tenant. Deploy multi-tenant. Profit.**
