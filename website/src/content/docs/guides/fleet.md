---
title: Fleet Mode (slum)
description: Scale tenement across multiple servers
---

When one tenement isn't enough, use **slum** - the multi-server fleet orchestrator.

## What is slum?

**slum** (the neighborhood) coordinates **tenement** (the building) instances across a fleet of servers.

```
┌─────────────────────────────────────────┐
│     slum (Fleet Database + Router)       │
├──────────────┬──────────────┬───────────┤
│  tenement    │  tenement    │ tenement  │
│  (east-1)    │  (west-1)    │ (south-1) │
└──────────────┴──────────────┴───────────┘
```

## Setup

### 1. Initialize the Fleet Database

```rust
use slum::{SlumDb, Server};

#[tokio::main]
async fn main() -> Result<()> {
    // Create/open the fleet database
    let db = SlumDb::init("slum.db").await?;

    // Add buildings (tenement servers) to the fleet
    db.add_server(&Server {
        id: "east".into(),
        url: "http://east.example.com".into(),
        region: Some("us-east".into()),
        capacity_mb: 1024,
        ..Default::default()
    }).await?;

    db.add_server(&Server {
        id: "west".into(),
        url: "http://west.example.com".into(),
        region: Some("us-west".into()),
        capacity_mb: 1024,
        ..Default::default()
    }).await?;

    Ok(())
}
```

### 2. Route Tenants to Servers

```rust
use slum::{Tenant};

// Assign a tenant to a specific server
db.add_tenant(&Tenant {
    domain: "customer-1.example.com".into(),
    server_id: "east".into(),
    process: "api".into(),
    instance_id: "prod".into(),
    ..Default::default()
}).await?;

// Lookup where a tenant lives
let tenant = db.get_tenant_by_domain("customer-1.example.com").await?;
println!("Tenant is on server: {}", tenant.server_id);
```

### 3. Orchestrate Spawn/Stop Across Fleet

```rust
// Spawn instance across fleet
db.spawn_instance(
    "customer-2",
    "api",
    "prod",
    "west"  // target server
).await?;

// Stop instance (slum finds the right server)
db.stop_instance("customer-2:prod").await?;

// List all instances across all servers
let instances = db.list_all_instances().await?;
```

## API

### Server Management

```rust
// Add server
db.add_server(&Server {
    id: "east".into(),
    url: "http://east.example.com".into(),
    capacity_mb: 2048,
    ..Default::default()
}).await?;

// Update server
db.update_server(&updated_server).await?;

// List servers
let servers = db.list_servers().await?;

// Get server stats
let stats = db.get_server_stats("east").await?;
```

### Tenant Management

```rust
// Add tenant
db.add_tenant(&Tenant {
    domain: "customer.example.com".into(),
    server_id: "east".into(),
    process: "api".into(),
    instance_id: "prod".into(),
    ..Default::default()
}).await?;

// Get tenant
let tenant = db.get_tenant_by_domain("customer.example.com").await?;

// List tenants on server
let tenants = db.list_tenants_on_server("east").await?;

// Reassign tenant to different server
db.update_tenant(&updated_tenant).await?;
```

### Instance Management

```rust
// Spawn on specific server
db.spawn_instance(
    "customer-3",
    "api",
    "prod",
    "east"
).await?;

// Stop (slum finds server automatically)
db.stop_instance("customer-3:prod").await?;

// List instances on server
let instances = db.list_instances_on_server("east").await?;

// Get instance status
let status = db.get_instance_status("customer-3:prod").await?;
```

## Use Cases

### Geographic Distribution

Route tenants to nearest server:

```rust
let tenant = db.get_tenant_by_domain("customer.example.com").await?;
if let Some(region) = customer.preferred_region {
    // Find server in that region
    let server = db.find_server_in_region(&region).await?;
    // Reassign tenant
    tenant.server_id = server.id;
    db.update_tenant(&tenant).await?;
}
```

### Load Balancing

Distribute new tenants based on server capacity:

```rust
let servers = db.list_servers().await?;
let least_used = servers
    .iter()
    .min_by_key(|s| s.used_capacity_mb())
    .expect("No servers available");

db.spawn_instance(
    &new_customer_id,
    "api",
    "prod",
    &least_used.id
).await?;
```

### High Availability

Mirror tenants across servers:

```rust
// Primary instance
db.spawn_instance("customer-4", "api", "prod", "east").await?;

// Backup instance
db.spawn_instance("customer-4", "api", "backup", "west").await?;

// Route to primary, fail over to backup if needed
```

## Data Model

### Server

| Field | Type | Description |
|-------|------|---|
| `id` | String | Unique server ID (e.g., "east-1") |
| `url` | String | Server URL (e.g., "http://east-1.example.com") |
| `region` | Option<String> | Region (e.g., "us-east") |
| `capacity_mb` | i64 | Total available memory (MB) |
| `created_at` | DateTime | Creation timestamp |

### Tenant

| Field | Type | Description |
|-------|------|---|
| `domain` | String | Customer domain (e.g., "acme.example.com") |
| `server_id` | String | Home server ID |
| `process` | String | Process name (e.g., "api") |
| `instance_id` | String | Instance ID (e.g., "prod") |
| `created_at` | DateTime | Creation timestamp |

## Foreign Key Enforcement

slum enforces referential integrity:
- Tenant's `server_id` must exist
- Can't delete server with active tenants
- Can't update non-existent tenant

## Next Steps

- [Getting Started](/guides/getting-started) - Single-server setup
- [Configuration](/guides/configuration) - tenement config reference
