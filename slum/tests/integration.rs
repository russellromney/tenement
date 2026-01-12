//! Slum Integration Tests - Session 8
//!
//! Tests for fleet orchestration functionality including:
//! - Server health check and status updates
//! - Tenant routing rules
//! - Multi-tenant scenarios
//! - Tenant migration between servers
//! - Server offline handling

use axum::http::StatusCode;
use axum_test::TestServer;
use slum::{Server, SlumDb, Tenant};
use slum::db::ServerStatus;
use slum::server::{create_router, SlumState};
use std::sync::Arc;
use tempfile::TempDir;
use chrono::Utc;

/// Create a test database and state
async fn create_test_state() -> (SlumState, Arc<SlumDb>, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    let db = SlumDb::init(&path).await.unwrap();
    let db = Arc::new(db);
    let state = SlumState::new(db.clone());
    (state, db, dir)
}

/// Create a test server struct
fn test_server(id: &str, url: &str) -> Server {
    Server {
        id: id.to_string(),
        name: format!("Server {}", id),
        url: url.to_string(),
        region: Some("us-east-1".to_string()),
        status: ServerStatus::Online,
        last_seen: Some(Utc::now()),
        created_at: Utc::now(),
    }
}

/// Create a test tenant struct
fn test_tenant(id: &str, domain: &str, server_id: &str) -> Tenant {
    Tenant {
        id: id.to_string(),
        name: format!("Tenant {}", id),
        domain: domain.to_string(),
        server_id: server_id.to_string(),
        process: "api".to_string(),
        instance_id: "prod".to_string(),
        created_at: Utc::now(),
    }
}

// =============================================================================
// Session 8.1: Server Health Check Tests
// =============================================================================

/// Test that updating server status via API works correctly
#[tokio::test]
async fn test_server_health_check_updates_status() {
    let (state, db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // Create a server with unknown status
    let srv = Server {
        status: ServerStatus::Unknown,
        ..test_server("srv1", "http://localhost:8080")
    };
    db.add_server(&srv).await.unwrap();

    // Verify initial status is unknown
    let response = server.get("/api/servers/srv1").await;
    response.assert_status_ok();
    let data: serde_json::Value = response.json();
    assert_eq!(data["status"], "unknown");

    // Update status to online via API
    let response = server
        .post("/api/servers/srv1/status")
        .json(&serde_json::json!({ "status": "online" }))
        .await;
    response.assert_status_ok();

    // Verify status updated
    let response = server.get("/api/servers/srv1").await;
    let data: serde_json::Value = response.json();
    assert_eq!(data["status"], "online");

    // Verify last_seen was updated
    assert!(data["last_seen"].is_string());
}

/// Test status transitions: online -> degraded -> offline -> online
#[tokio::test]
async fn test_server_status_transitions() {
    let (state, db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    db.add_server(&test_server("srv1", "http://localhost:8080"))
        .await
        .unwrap();

    let transitions = ["degraded", "offline", "online"];
    for status in transitions {
        let response = server
            .post("/api/servers/srv1/status")
            .json(&serde_json::json!({ "status": status }))
            .await;
        response.assert_status_ok();

        let response = server.get("/api/servers/srv1").await;
        let data: serde_json::Value = response.json();
        assert_eq!(data["status"], status);
    }
}

/// Test updating status of non-existent server returns 404
#[tokio::test]
async fn test_server_status_update_not_found() {
    let (state, _db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    let response = server
        .post("/api/servers/nonexistent/status")
        .json(&serde_json::json!({ "status": "online" }))
        .await;
    response.assert_status_not_found();
}

// =============================================================================
// Session 8.2: Tenant Routing Tests
// =============================================================================

/// Test that routing returns correct server for tenant's domain
#[tokio::test]
async fn test_tenant_routing_to_server() {
    let (_state, db, _dir) = create_test_state().await;

    // Setup: server and tenant
    db.add_server(&test_server("srv1", "http://server1.example.com"))
        .await
        .unwrap();
    db.add_tenant(&test_tenant("tenant1", "app.customer.com", "srv1"))
        .await
        .unwrap();

    // Test routing via db layer
    let result = db.route("app.customer.com").await.unwrap();
    assert!(result.is_some());

    let (tenant, server) = result.unwrap();
    assert_eq!(tenant.id, "tenant1");
    assert_eq!(tenant.domain, "app.customer.com");
    assert_eq!(server.id, "srv1");
    assert_eq!(server.url, "http://server1.example.com");
}

/// Test that routing non-existent domain returns None
#[tokio::test]
async fn test_tenant_routing_unknown_domain() {
    let (_state, db, _dir) = create_test_state().await;

    // Setup: server and tenant
    db.add_server(&test_server("srv1", "http://server1.example.com"))
        .await
        .unwrap();
    db.add_tenant(&test_tenant("tenant1", "app.customer.com", "srv1"))
        .await
        .unwrap();

    // Test routing unknown domain
    let result = db.route("unknown.example.com").await.unwrap();
    assert!(result.is_none());
}

/// Test tenant lookup by domain via API (get_tenant_by_domain)
#[tokio::test]
async fn test_tenant_domain_lookup() {
    let (_state, db, _dir) = create_test_state().await;

    db.add_server(&test_server("srv1", "http://localhost:8080"))
        .await
        .unwrap();
    db.add_tenant(&test_tenant("tenant1", "myapp.example.com", "srv1"))
        .await
        .unwrap();

    // Lookup by domain
    let tenant = db.get_tenant_by_domain("myapp.example.com").await.unwrap();
    assert!(tenant.is_some());
    assert_eq!(tenant.unwrap().id, "tenant1");

    // Lookup non-existent domain
    let tenant = db.get_tenant_by_domain("other.example.com").await.unwrap();
    assert!(tenant.is_none());
}

// =============================================================================
// Session 8.3: Multiple Tenants Tests
// =============================================================================

/// Test that multiple tenants can share the same server
#[tokio::test]
async fn test_multiple_tenants_same_server() {
    let (state, db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // Create one server
    db.add_server(&test_server("srv1", "http://localhost:8080"))
        .await
        .unwrap();

    // Create multiple tenants on same server
    let tenants = [
        test_tenant("tenant1", "app1.example.com", "srv1"),
        test_tenant("tenant2", "app2.example.com", "srv1"),
        test_tenant("tenant3", "app3.example.com", "srv1"),
    ];

    for tenant in &tenants {
        db.add_tenant(tenant).await.unwrap();
    }

    // Verify all tenants exist
    let response = server.get("/api/tenants").await;
    response.assert_status_ok();
    let data: Vec<serde_json::Value> = response.json();
    assert_eq!(data.len(), 3);

    // Verify all tenants point to same server
    for t in &data {
        assert_eq!(t["server_id"], "srv1");
    }

    // Verify list_tenants_by_server returns all
    let server_tenants = db.list_tenants_by_server("srv1").await.unwrap();
    assert_eq!(server_tenants.len(), 3);

    // Verify routing works for each tenant
    for tenant in &tenants {
        let result = db.route(&tenant.domain).await.unwrap();
        assert!(result.is_some());
        let (t, s) = result.unwrap();
        assert_eq!(t.id, tenant.id);
        assert_eq!(s.id, "srv1");
    }
}

/// Test that tenants on different servers route correctly
#[tokio::test]
async fn test_multiple_tenants_different_servers() {
    let (_state, db, _dir) = create_test_state().await;

    // Create two servers
    db.add_server(&test_server("srv1", "http://server1.example.com"))
        .await
        .unwrap();
    db.add_server(&test_server("srv2", "http://server2.example.com"))
        .await
        .unwrap();

    // Create tenants on different servers
    db.add_tenant(&test_tenant("tenant1", "app1.example.com", "srv1"))
        .await
        .unwrap();
    db.add_tenant(&test_tenant("tenant2", "app2.example.com", "srv2"))
        .await
        .unwrap();

    // Verify routing to correct servers
    let (t1, s1) = db.route("app1.example.com").await.unwrap().unwrap();
    assert_eq!(t1.server_id, "srv1");
    assert_eq!(s1.id, "srv1");

    let (t2, s2) = db.route("app2.example.com").await.unwrap().unwrap();
    assert_eq!(t2.server_id, "srv2");
    assert_eq!(s2.id, "srv2");
}

/// Test domain uniqueness constraint
#[tokio::test]
async fn test_tenant_domain_unique() {
    let (_state, db, _dir) = create_test_state().await;

    db.add_server(&test_server("srv1", "http://localhost:8080"))
        .await
        .unwrap();

    // Create first tenant
    db.add_tenant(&test_tenant("tenant1", "app.example.com", "srv1"))
        .await
        .unwrap();

    // Try to create second tenant with same domain - should fail
    let result = db
        .add_tenant(&test_tenant("tenant2", "app.example.com", "srv1"))
        .await;
    assert!(result.is_err(), "Should fail due to unique domain constraint");
}

// =============================================================================
// Session 8.4: Tenant Migration Tests
// =============================================================================

/// Test migrating a tenant to a different server
#[tokio::test]
async fn test_tenant_migration() {
    let (state, db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // Create two servers
    db.add_server(&test_server("srv1", "http://server1.example.com"))
        .await
        .unwrap();
    db.add_server(&test_server("srv2", "http://server2.example.com"))
        .await
        .unwrap();

    // Create tenant on srv1
    db.add_tenant(&test_tenant("tenant1", "app.example.com", "srv1"))
        .await
        .unwrap();

    // Verify initial routing
    let (_, s) = db.route("app.example.com").await.unwrap().unwrap();
    assert_eq!(s.id, "srv1");

    // Migrate: delete and recreate on different server
    // (In production, you'd have an update_tenant method)
    db.delete_tenant("tenant1").await.unwrap();

    let migrated_tenant = Tenant {
        id: "tenant1".to_string(),
        name: "Tenant tenant1".to_string(),
        domain: "app.example.com".to_string(),
        server_id: "srv2".to_string(), // New server
        process: "api".to_string(),
        instance_id: "prod".to_string(),
        created_at: Utc::now(),
    };
    db.add_tenant(&migrated_tenant).await.unwrap();

    // Verify routing now goes to srv2
    let (t, s) = db.route("app.example.com").await.unwrap().unwrap();
    assert_eq!(t.server_id, "srv2");
    assert_eq!(s.id, "srv2");

    // Verify via API
    let response = server.get("/api/tenants/tenant1").await;
    response.assert_status_ok();
    let data: serde_json::Value = response.json();
    assert_eq!(data["server_id"], "srv2");
}

/// Test that tenant cannot be migrated to non-existent server
#[tokio::test]
async fn test_tenant_migration_invalid_server() {
    let (_state, db, _dir) = create_test_state().await;

    db.add_server(&test_server("srv1", "http://localhost:8080"))
        .await
        .unwrap();
    db.add_tenant(&test_tenant("tenant1", "app.example.com", "srv1"))
        .await
        .unwrap();

    // Delete tenant
    db.delete_tenant("tenant1").await.unwrap();

    // Try to recreate with non-existent server - should fail FK constraint
    let bad_tenant = test_tenant("tenant1", "app.example.com", "nonexistent");
    let result = db.add_tenant(&bad_tenant).await;
    assert!(result.is_err(), "Should fail due to FK constraint");
}

// =============================================================================
// Session 8.5: Server Offline Tests
// =============================================================================

/// Test that routing to offline server returns server offline status
#[tokio::test]
async fn test_server_offline_tenant_unreachable() {
    let (_state, db, _dir) = create_test_state().await;

    // Create server and tenant
    let srv = Server {
        status: ServerStatus::Offline,
        ..test_server("srv1", "http://localhost:8080")
    };
    db.add_server(&srv).await.unwrap();
    db.add_tenant(&test_tenant("tenant1", "app.example.com", "srv1"))
        .await
        .unwrap();

    // Route returns tenant and server, but server is offline
    let result = db.route("app.example.com").await.unwrap();
    assert!(result.is_some());

    let (tenant, server) = result.unwrap();
    assert_eq!(tenant.id, "tenant1");
    assert_eq!(server.status, ServerStatus::Offline);

    // Application layer should check server.status and return appropriate error
    // This is tested via the proxy handler which checks server status
}

/// Test status filtering - list only online servers
#[tokio::test]
async fn test_server_status_filtering() {
    let (_state, db, _dir) = create_test_state().await;

    // Create servers with different statuses
    let servers = [
        Server {
            status: ServerStatus::Online,
            ..test_server("srv1", "http://server1.example.com")
        },
        Server {
            status: ServerStatus::Offline,
            ..test_server("srv2", "http://server2.example.com")
        },
        Server {
            status: ServerStatus::Degraded,
            ..test_server("srv3", "http://server3.example.com")
        },
    ];

    for srv in &servers {
        db.add_server(srv).await.unwrap();
    }

    // List all servers
    let all = db.list_servers().await.unwrap();
    assert_eq!(all.len(), 3);

    // Count by status
    let online_count = all.iter().filter(|s| s.status == ServerStatus::Online).count();
    let offline_count = all.iter().filter(|s| s.status == ServerStatus::Offline).count();
    let degraded_count = all.iter().filter(|s| s.status == ServerStatus::Degraded).count();

    assert_eq!(online_count, 1);
    assert_eq!(offline_count, 1);
    assert_eq!(degraded_count, 1);
}

/// Test deleting server with tenants fails (FK constraint)
#[tokio::test]
async fn test_delete_server_with_tenants_fails() {
    let (state, db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // Create server with tenant
    db.add_server(&test_server("srv1", "http://localhost:8080"))
        .await
        .unwrap();
    db.add_tenant(&test_tenant("tenant1", "app.example.com", "srv1"))
        .await
        .unwrap();

    // Try to delete server via API - should fail due to FK
    let response = server.delete("/api/servers/srv1").await;
    // SQLite FK violation returns error, which handler converts to 500
    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);

    // Server should still exist
    let response = server.get("/api/servers/srv1").await;
    response.assert_status_ok();
}

/// Test deleting tenant then server works
#[tokio::test]
async fn test_delete_tenant_then_server() {
    let (state, db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // Create server with tenant
    db.add_server(&test_server("srv1", "http://localhost:8080"))
        .await
        .unwrap();
    db.add_tenant(&test_tenant("tenant1", "app.example.com", "srv1"))
        .await
        .unwrap();

    // Delete tenant first
    let response = server.delete("/api/tenants/tenant1").await;
    response.assert_status(StatusCode::NO_CONTENT);

    // Now delete server - should succeed
    let response = server.delete("/api/servers/srv1").await;
    response.assert_status(StatusCode::NO_CONTENT);

    // Both should be gone
    let response = server.get("/api/servers/srv1").await;
    response.assert_status_not_found();
    let response = server.get("/api/tenants/tenant1").await;
    response.assert_status_not_found();
}

// =============================================================================
// Additional Integration Tests
// =============================================================================

/// Test full server CRUD lifecycle via API
#[tokio::test]
async fn test_server_full_lifecycle_api() {
    let (state, _db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // Create
    let response = server
        .post("/api/servers")
        .json(&serde_json::json!({
            "id": "srv1",
            "name": "Production Server",
            "url": "http://prod.example.com",
            "region": "us-west-2"
        }))
        .await;
    response.assert_status(StatusCode::CREATED);

    // Read
    let response = server.get("/api/servers/srv1").await;
    response.assert_status_ok();
    let data: serde_json::Value = response.json();
    assert_eq!(data["name"], "Production Server");
    assert_eq!(data["region"], "us-west-2");

    // Update status
    server
        .post("/api/servers/srv1/status")
        .json(&serde_json::json!({ "status": "online" }))
        .await
        .assert_status_ok();

    // List
    let response = server.get("/api/servers").await;
    let servers: Vec<serde_json::Value> = response.json();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["status"], "online");

    // Delete
    server.delete("/api/servers/srv1").await.assert_status(StatusCode::NO_CONTENT);
    server.get("/api/servers/srv1").await.assert_status_not_found();
}

/// Test full tenant CRUD lifecycle via API
#[tokio::test]
async fn test_tenant_full_lifecycle_api() {
    let (state, db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // Setup server first
    db.add_server(&test_server("srv1", "http://localhost:8080"))
        .await
        .unwrap();

    // Create tenant
    let response = server
        .post("/api/tenants")
        .json(&serde_json::json!({
            "id": "tenant1",
            "name": "Acme Corp",
            "domain": "acme.example.com",
            "server_id": "srv1",
            "process": "webapp",
            "instance_id": "prod"
        }))
        .await;
    response.assert_status(StatusCode::CREATED);

    // Read
    let response = server.get("/api/tenants/tenant1").await;
    response.assert_status_ok();
    let data: serde_json::Value = response.json();
    assert_eq!(data["name"], "Acme Corp");
    assert_eq!(data["process"], "webapp");

    // List
    let response = server.get("/api/tenants").await;
    let tenants: Vec<serde_json::Value> = response.json();
    assert_eq!(tenants.len(), 1);

    // Delete
    server.delete("/api/tenants/tenant1").await.assert_status(StatusCode::NO_CONTENT);
    server.get("/api/tenants/tenant1").await.assert_status_not_found();
}

/// Test health endpoint always returns ok
#[tokio::test]
async fn test_health_endpoint() {
    let (state, _db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/health").await;
    response.assert_status_ok();
    let data: serde_json::Value = response.json();
    assert_eq!(data["status"], "ok");
}

/// Test aggregated metrics endpoint
#[tokio::test]
async fn test_aggregated_metrics_endpoint() {
    let (state, db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // Add some servers
    db.add_server(&test_server("srv1", "http://localhost:8081"))
        .await
        .unwrap();
    db.add_server(&test_server("srv2", "http://localhost:8082"))
        .await
        .unwrap();

    let response = server.get("/api/metrics").await;
    response.assert_status_ok();
    let data: serde_json::Value = response.json();

    // Should have servers array
    assert!(data["servers"].is_array());
    let servers = data["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 2);
}

/// Test aggregated logs endpoint
#[tokio::test]
async fn test_aggregated_logs_endpoint() {
    let (state, db, _dir) = create_test_state().await;
    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // Add a server
    db.add_server(&test_server("srv1", "http://localhost:8080"))
        .await
        .unwrap();

    let response = server.get("/api/logs").await;
    response.assert_status_ok();
    let data: serde_json::Value = response.json();
    assert_eq!(data["server_count"], 1);
}
