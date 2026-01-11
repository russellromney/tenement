//! HTTP server for slum fleet orchestration
//!
//! Provides API for managing servers and tenants, plus reverse proxy to route
//! requests to the appropriate tenement server.

use crate::db::{Server, ServerStatus, SlumDb, Tenant};
use anyhow::Result;
use axum::{
    body::Body,
    extract::{Host, Path, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use chrono::Utc;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

/// Application state for slum server
#[derive(Clone)]
pub struct SlumState {
    pub db: Arc<SlumDb>,
    pub client: Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
}

impl SlumState {
    pub fn new(db: Arc<SlumDb>) -> Self {
        let client = Client::builder(TokioExecutor::new()).build_http();
        Self { db, client }
    }
}

/// Create the slum router
pub fn create_router(state: SlumState) -> Router {
    Router::new()
        // Dashboard/API at root
        .route("/", get(dashboard))
        .route("/health", get(health))
        // Server management
        .route("/api/servers", get(list_servers).post(add_server))
        .route(
            "/api/servers/:id",
            get(get_server).delete(delete_server),
        )
        .route("/api/servers/:id/status", post(update_server_status))
        // Tenant management
        .route("/api/tenants", get(list_tenants).post(add_tenant))
        .route(
            "/api/tenants/:id",
            get(get_tenant).delete(delete_tenant),
        )
        // Aggregated metrics and logs
        .route("/api/metrics", get(aggregated_metrics))
        .route("/api/logs", get(aggregated_logs))
        // Fallback routes to tenant servers
        .fallback(proxy_request)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Start the slum HTTP server
pub async fn serve(db: Arc<SlumDb>, port: u16) -> Result<()> {
    let state = SlumState::new(db);
    let app = create_router(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("slum fleet orchestrator listening on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

// --- Handlers ---

async fn dashboard() -> impl IntoResponse {
    "slum fleet orchestrator"
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

// Server handlers

async fn list_servers(State(state): State<SlumState>) -> impl IntoResponse {
    match state.db.list_servers().await {
        Ok(servers) => Json(servers).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateServer {
    id: String,
    name: String,
    url: String,
    region: Option<String>,
}

async fn add_server(
    State(state): State<SlumState>,
    Json(input): Json<CreateServer>,
) -> impl IntoResponse {
    let server = Server {
        id: input.id,
        name: input.name,
        url: input.url,
        region: input.region,
        status: ServerStatus::Unknown,
        last_seen: None,
        created_at: Utc::now(),
    };

    match state.db.add_server(&server).await {
        Ok(()) => (StatusCode::CREATED, Json(server)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn get_server(
    State(state): State<SlumState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.db.get_server(&id).await {
        Ok(Some(server)) => Json(server).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_server(
    State(state): State<SlumState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.db.delete_server(&id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct UpdateStatus {
    status: String,
}

async fn update_server_status(
    State(state): State<SlumState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateStatus>,
) -> impl IntoResponse {
    let status: ServerStatus = input.status.parse().unwrap_or(ServerStatus::Unknown);
    match state.db.update_server_status(&id, status).await {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// Tenant handlers

async fn list_tenants(State(state): State<SlumState>) -> impl IntoResponse {
    match state.db.list_tenants().await {
        Ok(tenants) => Json(tenants).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateTenant {
    id: String,
    name: String,
    domain: String,
    server_id: String,
    process: String,
    instance_id: String,
}

async fn add_tenant(
    State(state): State<SlumState>,
    Json(input): Json<CreateTenant>,
) -> impl IntoResponse {
    let tenant = Tenant {
        id: input.id,
        name: input.name,
        domain: input.domain,
        server_id: input.server_id,
        process: input.process,
        instance_id: input.instance_id,
        created_at: Utc::now(),
    };

    match state.db.add_tenant(&tenant).await {
        Ok(()) => (StatusCode::CREATED, Json(tenant)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn get_tenant(
    State(state): State<SlumState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.db.get_tenant(&id).await {
        Ok(Some(tenant)) => Json(tenant).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_tenant(
    State(state): State<SlumState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.db.delete_tenant(&id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// Aggregation handlers

#[derive(Serialize)]
struct AggregatedMetrics {
    servers: Vec<ServerMetrics>,
}

#[derive(Serialize)]
struct ServerMetrics {
    server_id: String,
    server_name: String,
    status: String,
    metrics: Option<String>,
}

async fn aggregated_metrics(State(state): State<SlumState>) -> impl IntoResponse {
    let servers = match state.db.list_servers().await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let mut results = Vec::new();
    for server in servers {
        let metrics = fetch_server_metrics(&state.client, &server.url).await;
        results.push(ServerMetrics {
            server_id: server.id,
            server_name: server.name,
            status: server.status.to_string(),
            metrics,
        });
    }

    Json(AggregatedMetrics { servers: results }).into_response()
}

async fn fetch_server_metrics(
    client: &Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
    base_url: &str,
) -> Option<String> {
    let url = format!("{}/metrics", base_url);
    let uri: hyper::Uri = url.parse().ok()?;

    let req = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .ok()?;

    match client.request(req).await {
        Ok(resp) => {
            use http_body_util::BodyExt;
            let body = resp.into_body().collect().await.ok()?.to_bytes();
            Some(String::from_utf8_lossy(&body).to_string())
        }
        Err(_) => None,
    }
}

async fn aggregated_logs(State(state): State<SlumState>) -> impl IntoResponse {
    // For now, just return a placeholder
    // Full implementation would stream logs from all servers
    let servers = match state.db.list_servers().await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    Json(serde_json::json!({
        "message": "Log aggregation endpoint",
        "server_count": servers.len()
    }))
    .into_response()
}

// Proxy handler

async fn proxy_request(
    Host(host): Host,
    State(state): State<SlumState>,
    req: Request<Body>,
) -> Response {
    // Extract domain from host
    let domain = host.split(':').next().unwrap_or(&host);

    // Look up routing
    let (tenant, server) = match state.db.route(domain).await {
        Ok(Some((t, s))) => (t, s),
        Ok(None) => {
            return (StatusCode::NOT_FOUND, format!("No tenant for domain: {}", domain))
                .into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    // Check server status
    if server.status == ServerStatus::Offline {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Server is offline",
        )
            .into_response();
    }

    // Build target URL
    // Routes to {instance_id}.{process}.{server_domain}
    let target_host = format!(
        "{}.{}.{}",
        tenant.instance_id,
        tenant.process,
        server.url.trim_start_matches("http://").trim_start_matches("https://")
    );

    info!(
        "Routing {} -> {} (server: {})",
        domain, target_host, server.id
    );

    // Proxy the request
    let uri = req.uri().clone();
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let target_url = format!("http://{}{}", target_host, path_and_query);

    let target_uri: hyper::Uri = match target_url.parse() {
        Ok(u) => u,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let (parts, body) = req.into_parts();
    let mut proxy_req = Request::from_parts(parts, body);
    *proxy_req.uri_mut() = target_uri;

    match state.client.request(proxy_req).await {
        Ok(resp) => resp.into_response(),
        Err(e) => {
            warn!("Proxy error: {}", e);
            (StatusCode::BAD_GATEWAY, format!("Proxy error: {}", e)).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    use tempfile::TempDir;

    async fn create_test_state() -> (SlumState, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let db = SlumDb::init(&path).await.unwrap();
        (SlumState::new(Arc::new(db)), dir)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let (state, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server.get("/health").await;
        response.assert_status_ok();
    }

    #[tokio::test]
    async fn test_server_crud_api() {
        let (state, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Create server
        let response = server
            .post("/api/servers")
            .json(&serde_json::json!({
                "id": "srv1",
                "name": "Test Server",
                "url": "http://localhost:8080",
                "region": "us-east-1"
            }))
            .await;
        response.assert_status(StatusCode::CREATED);

        // List servers
        let response = server.get("/api/servers").await;
        response.assert_status_ok();
        let servers: Vec<serde_json::Value> = response.json();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0]["id"], "srv1");

        // Get server
        let response = server.get("/api/servers/srv1").await;
        response.assert_status_ok();

        // Delete server
        let response = server.delete("/api/servers/srv1").await;
        response.assert_status(StatusCode::NO_CONTENT);

        // Verify deleted
        let response = server.get("/api/servers/srv1").await;
        response.assert_status_not_found();
    }

    #[tokio::test]
    async fn test_tenant_crud_api() {
        let (state, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Create server first
        server
            .post("/api/servers")
            .json(&serde_json::json!({
                "id": "srv1",
                "name": "Test Server",
                "url": "http://localhost:8080"
            }))
            .await;

        // Create tenant
        let response = server
            .post("/api/tenants")
            .json(&serde_json::json!({
                "id": "tenant1",
                "name": "Test Tenant",
                "domain": "test.example.com",
                "server_id": "srv1",
                "process": "api",
                "instance_id": "prod"
            }))
            .await;
        response.assert_status(StatusCode::CREATED);

        // List tenants
        let response = server.get("/api/tenants").await;
        response.assert_status_ok();
        let tenants: Vec<serde_json::Value> = response.json();
        assert_eq!(tenants.len(), 1);

        // Delete tenant
        let response = server.delete("/api/tenants/tenant1").await;
        response.assert_status(StatusCode::NO_CONTENT);
    }
}
