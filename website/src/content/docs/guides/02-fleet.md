---
title: Fleet Mode (slum)
description: Scale tenement across multiple servers
---

> **Experimental.** slum exists as a library (~950 lines) but is not yet production-ready. The API described here works but will change.

## What is slum?

tenement runs on a single server. slum coordinates multiple tenement instances across a fleet of servers, so you can distribute tenants geographically or run them with high availability.

```
+------------------------------------------+
|     slum (fleet database + router)       |
+-------------+-------------+-------------+
|  tenement   |  tenement   |  tenement   |
|  (east-1)   |  (west-1)   |  (south-1)  |
+-------------+-------------+-------------+
```

slum is a Rust library (not a standalone binary yet). You register servers, assign tenants to them, and slum tracks where everything lives in a SQLite database.

## The idea

The core API is straightforward. You add servers, add tenants with a home server assignment, and spawn/stop instances across the fleet:

```rust
let db = SlumDb::init("slum.db").await?;

db.add_server(&Server {
    id: "east".into(),
    url: "http://east.example.com".into(),
    region: Some("us-east".into()),
    capacity_mb: 1024,
    ..Default::default()
}).await?;

db.add_tenant(&Tenant {
    domain: "customer-1.example.com".into(),
    server_id: "east".into(),
    process: "api".into(),
    instance_id: "prod".into(),
    ..Default::default()
}).await?;

db.spawn_instance("customer-1", "api", "prod", "east").await?;
```

## What's next

The main planned integration is with [haqlite](https://github.com/russellromney/haqlite) for high-availability tenants. The idea is that two tenement servers + S3 gives you HA without Kubernetes: haqlite handles SQLite WAL replication to S3, and slum handles failover (detecting a dead server and spawning tenants on the surviving one).

This would make the single-server limitation soft rather than hard. You'd still write single-tenant code and deploy with tenement, but your tenants would survive a server failure.

## Current status

slum handles server registration, tenant assignment, and instance orchestration across servers. It enforces referential integrity (can't delete a server with active tenants, tenant's server must exist). What it doesn't do yet is automatic failover, health monitoring of servers, or geographic routing. Those are planned.

## Next steps

- [Quick Start](/intro/01-quick-start) for single-server setup
- [Configuration](/guides/03-configuration) for tenement config reference
