//! Server-side API routes for instance mutation (spawn, stop, restart, etc.)
//!
//! These endpoints are called by the CLI client (see client.rs).
//! All routes are under /api/* and protected by Bearer token auth.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::server::AppState;

// ===================
// Request/Response types
// ===================

#[derive(Debug, Serialize, Deserialize)]
pub struct SpawnRequest {
    pub process: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpawnResponse {
    pub instance: String,
    pub socket: String,
    pub port: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WeightRequest {
    pub weight: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WeightResponse {
    pub instance: String,
    pub weight: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeployRequest {
    pub process: String,
    pub version: String,
    #[serde(default = "default_weight")]
    pub weight: u8,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_weight() -> u8 {
    100
}
fn default_timeout() -> u64 {
    30
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeployResponse {
    pub instance: String,
    pub socket: String,
    pub weight: u8,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RouteRequest {
    pub process: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RouteResponse {
    pub from_instance: String,
    pub to_instance: String,
    pub from_weight: u8,
    pub to_weight: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

impl ApiError {
    fn new(msg: impl Into<String>) -> Self {
        Self {
            error: msg.into(),
        }
    }
}

// ===================
// Route builder
// ===================

// Routes are wired in server.rs create_router() directly
// to avoid Axum path parameter conflicts with merged routers.

// ===================
// Handlers
// ===================

/// Spawn a new instance: POST /api/instances/spawn
pub async fn post_spawn(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<crate::server::AuthIdentity>,
    Json(req): Json<SpawnRequest>,
) -> Result<Json<SpawnResponse>, (StatusCode, Json<ApiError>)> {
    check_tenant_access(&auth, &req.id)?;
    let socket = state
        .hypervisor
        .spawn(&req.process, &req.id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to spawn {}:{}: {}", req.process, req.id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(e.to_string())),
            )
        })?;

    let port = state
        .hypervisor
        .get(&req.process, &req.id)
        .await
        .and_then(|info| info.port);

    // Audit log
    if let Err(e) = state.deploy_log.log("spawn", &req.process, &req.id, None, true).await {
        tracing::error!("Audit log failed: {}", e);
    }

    Ok(Json(SpawnResponse {
        instance: format!("{}:{}", req.process, req.id),
        socket: socket.display().to_string(),
        port,
    }))
}

/// Stop an instance: DELETE /api/instances/{process:id}
pub async fn delete_instance(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<crate::server::AuthIdentity>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let (process, instance_id) = parse_instance_id(&id)?;
    check_tenant_access(&auth, &instance_id)?;

    state
        .hypervisor
        .stop(&process, &instance_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to stop {}: {}", id, e);
            (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(e.to_string())),
            )
        })?;

    // Audit log
    if let Err(e) = state.deploy_log.log("stop", &process, &instance_id, None, true).await {
        tracing::error!("Audit log failed: {}", e);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Restart an instance: POST /api/instances/{process:id}/restart
pub async fn post_restart(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<crate::server::AuthIdentity>,
    Path(id): Path<String>,
) -> Result<Json<SpawnResponse>, (StatusCode, Json<ApiError>)> {
    let (process, instance_id) = parse_instance_id(&id)?;
    check_tenant_access(&auth, &instance_id)?;

    let socket = state
        .hypervisor
        .restart(&process, &instance_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to restart {}: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(e.to_string())),
            )
        })?;

    let port = state
        .hypervisor
        .get(&process, &instance_id)
        .await
        .and_then(|info| info.port);

    Ok(Json(SpawnResponse {
        instance: id,
        socket: socket.display().to_string(),
        port,
    }))
}

/// Set weight: PUT /api/instances/{process:id}/weight
pub async fn put_weight(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<crate::server::AuthIdentity>,
    Path(id): Path<String>,
    Json(req): Json<WeightRequest>,
) -> Result<Json<WeightResponse>, (StatusCode, Json<ApiError>)> {
    let (process, instance_id) = parse_instance_id(&id)?;
    check_tenant_access(&auth, &instance_id)?;

    state
        .hypervisor
        .set_weight(&process, &instance_id, req.weight)
        .await
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(e.to_string())),
            )
        })?;

    Ok(Json(WeightResponse {
        instance: id,
        weight: req.weight,
    }))
}

/// Check health: GET /api/instances/{process:id}/health
pub async fn get_health_check(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<crate::server::AuthIdentity>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let (process, instance_id) = parse_instance_id(&id)?;
    check_tenant_access(&auth, &instance_id)?;

    let status = state
        .hypervisor
        .check_health(&process, &instance_id)
        .await;

    Ok(Json(serde_json::json!({
        "instance": id,
        "health": status.to_string(),
    })))
}

/// Deploy: POST /api/deploy (admin only)
pub async fn post_deploy(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<crate::server::AuthIdentity>,
    Json(req): Json<DeployRequest>,
) -> Result<Json<DeployResponse>, (StatusCode, Json<ApiError>)> {
    if auth.tenant_id.is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new("Deploy requires admin token")),
        ));
    }
    let socket = state
        .hypervisor
        .deploy_and_wait_healthy(&req.process, &req.version, req.weight, req.timeout)
        .await
        .map_err(|e| {
            tracing::error!(
                "Deploy failed for {}:{}: {}",
                req.process,
                req.version,
                e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(e.to_string())),
            )
        })?;

    // Audit log
    if let Err(e) = state.deploy_log.log(
        "deploy",
        &req.process,
        &req.version,
        Some(&format!("weight={}", req.weight)),
        true,
    ).await {
        tracing::error!("Audit log failed: {}", e);
    }

    Ok(Json(DeployResponse {
        instance: format!("{}:{}", req.process, req.version),
        socket: socket.display().to_string(),
        weight: req.weight,
        status: "healthy".to_string(),
    }))
}

/// Route swap: POST /api/route (admin only)
pub async fn post_route(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<crate::server::AuthIdentity>,
    Json(req): Json<RouteRequest>,
) -> Result<Json<RouteResponse>, (StatusCode, Json<ApiError>)> {
    if auth.tenant_id.is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new("Route swap requires admin token")),
        ));
    }
    state
        .hypervisor
        .route_swap(&req.process, &req.from, &req.to)
        .await
        .map_err(|e| {
            tracing::error!(
                "Route swap failed for {} {} -> {}: {}",
                req.process,
                req.from,
                req.to,
                e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(e.to_string())),
            )
        })?;

    // Audit log
    if let Err(e) = state.deploy_log.log(
        "route",
        &req.process,
        &format!("{} -> {}", req.from, req.to),
        Some("weight swap: 0/100"),
        true,
    ).await {
        tracing::error!("Audit log failed: {}", e);
    }

    Ok(Json(RouteResponse {
        from_instance: format!("{}:{}", req.process, req.from),
        to_instance: format!("{}:{}", req.process, req.to),
        from_weight: 0,
        to_weight: 100,
    }))
}

// ===================
// Helpers
// ===================

/// Check that a tenant token is authorized to access the given instance ID.
/// Admin tokens (tenant_id = None) have full access.
/// Tenant tokens can only access instances where the instance ID matches their tenant_id.
fn check_tenant_access(
    auth: &crate::server::AuthIdentity,
    instance_id: &str,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    if let Some(ref tenant) = auth.tenant_id {
        if tenant != instance_id {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ApiError::new(format!(
                    "Tenant token can only access instance '{}'",
                    tenant
                ))),
            ));
        }
    }
    Ok(())
}

fn parse_instance_id(s: &str) -> Result<(String, String), (StatusCode, Json<ApiError>)> {
    let parts: Vec<&str> = s.splitn(2, ':').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(format!(
                "Invalid instance ID '{}'. Expected format: process:id",
                s
            ))),
        ));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_instance_id_valid() {
        let (process, id) = parse_instance_id("api:prod").unwrap();
        assert_eq!(process, "api");
        assert_eq!(id, "prod");
    }

    #[test]
    fn test_parse_instance_id_with_colons_in_id() {
        let (process, id) = parse_instance_id("api:user:with:colons").unwrap();
        assert_eq!(process, "api");
        assert_eq!(id, "user:with:colons");
    }

    #[test]
    fn test_parse_instance_id_invalid_no_colon() {
        assert!(parse_instance_id("invalid").is_err());
    }

    #[test]
    fn test_parse_instance_id_invalid_empty_process() {
        assert!(parse_instance_id(":id").is_err());
    }

    #[test]
    fn test_parse_instance_id_invalid_empty_id() {
        assert!(parse_instance_id("process:").is_err());
    }

    #[test]
    fn test_parse_instance_id_invalid_empty() {
        assert!(parse_instance_id("").is_err());
    }
}
