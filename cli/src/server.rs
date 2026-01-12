//! HTTP server with subdomain routing and reverse proxy

use anyhow::Result;
use axum::{
    body::Body,
    extract::{Host, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::get,
    Router,
};
use futures::stream::Stream;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use hyperlocal::UnixConnector;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use tenement::{ConfigStore, Hypervisor, LogLevel, LogQuery, TokenStore};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::trace::TraceLayer;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub hypervisor: Arc<Hypervisor>,
    pub domain: String,
    pub client: Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
    pub config_store: Arc<ConfigStore>,
}

/// Create the router (exposed for testing)
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Dashboard/API routes (root domain)
        .route("/", get(dashboard))
        .route("/health", get(health))
        .route("/metrics", get(metrics_endpoint))
        .route("/api/instances", get(list_instances))
        .route("/api/instances/{id}/storage", get(get_instance_storage))
        .route("/api/logs", get(query_logs))
        .route("/api/logs/stream", get(stream_logs))
        // Dashboard static assets
        .route("/assets/*path", get(dashboard_asset))
        // Fallback handles subdomain routing
        .fallback(handle_request)
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Auth middleware - requires Bearer token for API endpoints
async fn auth_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();

    // Skip auth for public endpoints
    if path == "/health" || path == "/metrics" || path == "/" || path.starts_with("/assets/") {
        return Ok(next.run(req).await);
    }

    // Skip auth for subdomain requests (they go through the fallback handler)
    // The fallback handler will handle any subdomain-specific auth if needed
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if parse_subdomain(host, &state.domain).is_some() {
        return Ok(next.run(req).await);
    }

    // Extract token from Authorization header
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.to_lowercase().starts_with("bearer ") => &h[7..],
        _ => {
            tracing::debug!("Missing or invalid Authorization header");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Verify token using TokenStore
    let token_store = TokenStore::new(&state.config_store);
    match token_store.verify(token).await {
        Ok(true) => Ok(next.run(req).await),
        Ok(false) => {
            tracing::debug!("Invalid token provided");
            Err(StatusCode::UNAUTHORIZED)
        }
        Err(e) => {
            tracing::error!("Token verification error: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Start the HTTP server
pub async fn serve(
    hypervisor: Arc<Hypervisor>,
    domain: String,
    port: u16,
    config_store: Arc<ConfigStore>,
) -> Result<()> {
    let client = Client::builder(TokioExecutor::new()).build_http();

    let state = AppState {
        hypervisor,
        domain: domain.clone(),
        client,
        config_store,
    };

    let app = create_router(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("tenement listening on http://{}", addr);
    tracing::info!("Dashboard at http://{}", domain);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Serve dashboard
async fn dashboard() -> impl IntoResponse {
    crate::dashboard::serve_asset("").await
}

/// Serve dashboard assets
async fn dashboard_asset(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    crate::dashboard::serve_asset(&path).await
}

/// Health check endpoint
async fn health() -> impl IntoResponse {
    Json(HealthResponse { status: "ok" })
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// Prometheus metrics endpoint
async fn metrics_endpoint(State(state): State<AppState>) -> impl IntoResponse {
    let metrics = state.hypervisor.metrics();
    let output = metrics.format_prometheus().await;
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        output,
    )
}

/// List all running instances
async fn list_instances(State(state): State<AppState>) -> impl IntoResponse {
    let instances = state.hypervisor.list().await;
    let response: Vec<InstanceInfo> = instances
        .into_iter()
        .map(|i| InstanceInfo {
            id: i.id.to_string(),
            socket: i.socket.display().to_string(),
            uptime_secs: i.uptime_secs,
            restarts: i.restarts,
            health: i.health.to_string(),
            storage_used_bytes: i.storage_used_bytes,
            storage_quota_bytes: i.storage_quota_bytes,
        })
        .collect();
    Json(response)
}

#[derive(Serialize)]
struct InstanceInfo {
    id: String,
    socket: String,
    uptime_secs: u64,
    restarts: u32,
    health: String,
    storage_used_bytes: u64,
    storage_quota_bytes: Option<u64>,
}

/// Get storage info for a specific instance
/// Instance ID format: process:instance (e.g., "api:prod")
async fn get_instance_storage(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<StorageInfoResponse>, StatusCode> {
    // Parse instance ID as "process:instance"
    let parts: Vec<&str> = id.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let process = parts[0];
    let instance_id = parts[1];

    // Get storage info from hypervisor
    match state.hypervisor.get_storage_info(process, instance_id).await {
        Some(info) => Ok(Json(StorageInfoResponse {
            used_bytes: info.used_bytes,
            quota_bytes: info.quota_bytes,
            usage_percent: info.usage_percent(),
            path: info.path.display().to_string(),
        })),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Serialize)]
struct StorageInfoResponse {
    used_bytes: u64,
    quota_bytes: Option<u64>,
    usage_percent: Option<f64>,
    path: String,
}

/// Query parameters for log endpoint
#[derive(Debug, Deserialize)]
struct LogQueryParams {
    process: Option<String>,
    id: Option<String>,
    level: Option<String>,
    search: Option<String>,
    limit: Option<usize>,
}

impl From<LogQueryParams> for LogQuery {
    fn from(params: LogQueryParams) -> Self {
        LogQuery {
            process: params.process,
            instance_id: params.id,
            level: params.level.and_then(|l| match l.as_str() {
                "stdout" => Some(LogLevel::Stdout),
                "stderr" => Some(LogLevel::Stderr),
                _ => None,
            }),
            search: params.search,
            limit: params.limit,
        }
    }
}

/// Query logs with filters
async fn query_logs(
    State(state): State<AppState>,
    Query(params): Query<LogQueryParams>,
) -> impl IntoResponse {
    let query: LogQuery = params.into();
    let log_buffer = state.hypervisor.log_buffer();
    let logs = log_buffer.query(&query).await;
    Json(logs)
}

/// Stream logs via SSE
async fn stream_logs(
    State(state): State<AppState>,
    Query(params): Query<LogQueryParams>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let log_buffer = state.hypervisor.log_buffer();
    let rx = log_buffer.subscribe();

    // Filter parameters
    let process_filter = params.process;
    let id_filter = params.id;
    let level_filter = params.level.and_then(|l| match l.as_str() {
        "stdout" => Some(LogLevel::Stdout),
        "stderr" => Some(LogLevel::Stderr),
        _ => None,
    });

    let stream = BroadcastStream::new(rx)
        // Filter out errors and apply filters
        .filter(move |result| {
            let process_filter = process_filter.clone();
            let id_filter = id_filter.clone();
            match result {
                Ok(entry) => {
                    // Apply filters
                    if let Some(ref p) = process_filter {
                        if &entry.process != p {
                            return false;
                        }
                    }
                    if let Some(ref id) = id_filter {
                        if &entry.instance_id != id {
                            return false;
                        }
                    }
                    if let Some(level) = level_filter {
                        if entry.level != level {
                            return false;
                        }
                    }
                    true
                }
                Err(_) => false,
            }
        })
        // Convert to SSE events
        .map(|result| {
            let entry = result.expect("filtered out errors above");
            let json = serde_json::to_string(&entry).unwrap_or_default();
            Ok(Event::default().data(json))
        });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Handle incoming requests - route to dashboard or proxy to process
async fn handle_request(
    Host(host): Host,
    State(state): State<AppState>,
    req: Request<Body>,
) -> Response {
    // Parse subdomain pattern: {id}.{process}.{domain}
    match parse_subdomain(&host, &state.domain) {
        Some((process, id)) => proxy_to_instance(&state, &process, &id, req).await,
        None => {
            // No subdomain or invalid pattern - serve dashboard
            // For now just return 404 for non-dashboard routes
            (StatusCode::NOT_FOUND, "Not found").into_response()
        }
    }
}

/// Parse subdomain pattern: {id}.{process}.{domain} -> Some((process, id))
fn parse_subdomain(host: &str, domain: &str) -> Option<(String, String)> {
    // Strip port if present
    let host = host.split(':').next().unwrap_or(host);

    // Check if host ends with domain
    if !host.ends_with(domain) {
        return None;
    }

    // Get subdomain part
    let subdomain = host.strip_suffix(domain)?.strip_suffix('.')?;

    // Split into parts: id.process
    let parts: Vec<&str> = subdomain.splitn(2, '.').collect();
    if parts.len() != 2 {
        return None;
    }

    let id = parts[0].to_string();
    let process = parts[1].to_string();

    if id.is_empty() || process.is_empty() {
        return None;
    }

    Some((process, id))
}

/// Proxy request to a process instance via unix socket
///
/// Implements wake-on-request: if the instance is not running but the process
/// is configured, it will spawn the instance and wait for it to be ready.
async fn proxy_to_instance(
    state: &AppState,
    process: &str,
    id: &str,
    req: Request<Body>,
) -> Response {
    // Use atomic get-and-touch to prevent race condition where instance
    // could be reaped between checking if running and touching activity.
    let socket_path = match state.hypervisor.get_and_touch(process, id).await {
        Some(info) => {
            // Instance is running - activity already touched
            info.socket
        }
        None => {
            // Instance not running - check if process is configured
            if !state.hypervisor.has_process(process) {
                return (
                    StatusCode::NOT_FOUND,
                    format!("Process '{}' not configured", process),
                )
                    .into_response();
            }

            // Wake-on-request: spawn and wait for instance to be ready
            tracing::info!("Waking instance {}:{}", process, id);
            match state.hypervisor.spawn_and_wait(process, id).await {
                Ok(socket) => socket,
                Err(e) => {
                    tracing::error!("Failed to wake instance {}:{}: {}", process, id, e);
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Failed to start instance: {}", e),
                    )
                        .into_response();
                }
            }
        }
    };

    // Proxy to Unix socket
    proxy_to_unix_socket(&socket_path, req).await
}

/// Proxy an HTTP request to a Unix socket
async fn proxy_to_unix_socket(socket_path: &Path, req: Request<Body>) -> Response {
    // Create Unix socket client
    let connector = UnixConnector;
    let client: Client<UnixConnector, Body> = Client::builder(TokioExecutor::new()).build(connector);

    // Build URI for Unix socket - hyperlocal requires a special URI format
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let socket_uri = hyperlocal::Uri::new(socket_path, path_and_query);

    // Build proxy request preserving method and headers
    let mut proxy_req = Request::builder()
        .method(req.method())
        .uri(socket_uri);

    // Copy headers from original request
    for (key, value) in req.headers() {
        proxy_req = proxy_req.header(key, value);
    }

    let proxy_req = match proxy_req.body(req.into_body()) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to build proxy request: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build proxy request: {}", e),
            )
                .into_response();
        }
    };

    // Forward request to Unix socket
    match client.request(proxy_req).await {
        Ok(response) => {
            // Convert hyper Response to axum Response
            let (parts, body) = response.into_parts();
            Response::from_parts(parts, Body::new(body))
        }
        Err(e) => {
            tracing::error!("Proxy error to {}: {}", socket_path.display(), e);
            (
                StatusCode::BAD_GATEWAY,
                format!("Proxy error: {}", e),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    use tempfile::TempDir;
    use tenement::{init_db, Config};

    #[test]
    fn test_parse_subdomain() {
        // Valid patterns
        assert_eq!(
            parse_subdomain("prod.api.example.com", "example.com"),
            Some(("api".to_string(), "prod".to_string()))
        );
        assert_eq!(
            parse_subdomain("user123.app.example.com", "example.com"),
            Some(("app".to_string(), "user123".to_string()))
        );
        assert_eq!(
            parse_subdomain("staging.web.example.com:8080", "example.com"),
            Some(("web".to_string(), "staging".to_string()))
        );

        // Invalid patterns
        assert_eq!(parse_subdomain("example.com", "example.com"), None);
        assert_eq!(parse_subdomain("api.example.com", "example.com"), None);
        assert_eq!(parse_subdomain("other.com", "example.com"), None);
        assert_eq!(parse_subdomain("", "example.com"), None);
    }

    /// Create test state with auth token
    /// Returns (state, token, temp_dir) - temp_dir must be kept alive during test
    async fn create_test_state() -> (AppState, String, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = init_db(&db_path).await.unwrap();
        let config_store = Arc::new(ConfigStore::new(pool));

        // Generate and store a test token
        let token_store = TokenStore::new(&config_store);
        let token = token_store.generate_and_store().await.unwrap();

        let config = Config::default();
        let hypervisor = Hypervisor::new(config);
        let client = Client::builder(TokioExecutor::new()).build_http();
        let state = AppState {
            hypervisor,
            domain: "example.com".to_string(),
            client,
            config_store,
        };
        (state, token, dir)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let (state, _token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server.get("/health").await;
        response.assert_status_ok();

        let json: serde_json::Value = response.json();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_instances_endpoint_empty() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/instances")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let json: Vec<serde_json::Value> = response.json();
        assert!(json.is_empty());
    }

    #[tokio::test]
    async fn test_dashboard_endpoint() {
        let (state, _token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server.get("/").await;
        response.assert_status_ok();
        response.assert_text_contains("tenement dashboard");
    }

    #[tokio::test]
    async fn test_unknown_subdomain_returns_404() {
        let (state, _token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Simulate request to subdomain for unconfigured process
        let response = server
            .get("/some-path")
            .add_header("Host", "prod.api.example.com")
            .await;

        response.assert_status_not_found();
        response.assert_text_contains("not configured");
    }

    #[tokio::test]
    async fn test_logs_endpoint_empty() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/logs")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let json: Vec<serde_json::Value> = response.json();
        assert!(json.is_empty());
    }

    #[tokio::test]
    async fn test_logs_endpoint_with_entries() {
        let (state, token, _dir) = create_test_state().await;
        let log_buffer = state.hypervisor.log_buffer();

        // Add some log entries
        log_buffer.push_stdout("api", "prod", "hello world".to_string()).await;
        log_buffer.push_stderr("api", "prod", "error message".to_string()).await;

        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/logs")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let json: Vec<serde_json::Value> = response.json();
        assert_eq!(json.len(), 2);
    }

    #[tokio::test]
    async fn test_logs_endpoint_filter_by_process() {
        let (state, token, _dir) = create_test_state().await;
        let log_buffer = state.hypervisor.log_buffer();

        // Add entries for different processes
        log_buffer.push_stdout("api", "prod", "api message".to_string()).await;
        log_buffer.push_stdout("web", "prod", "web message".to_string()).await;

        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/logs?process=api")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let json: Vec<serde_json::Value> = response.json();
        assert_eq!(json.len(), 1);
        assert_eq!(json[0]["process"], "api");
    }

    #[tokio::test]
    async fn test_logs_endpoint_filter_by_id() {
        let (state, token, _dir) = create_test_state().await;
        let log_buffer = state.hypervisor.log_buffer();

        // Add entries for different instances
        log_buffer.push_stdout("api", "prod", "prod message".to_string()).await;
        log_buffer.push_stdout("api", "staging", "staging message".to_string()).await;

        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/logs?id=prod")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let json: Vec<serde_json::Value> = response.json();
        assert_eq!(json.len(), 1);
        assert_eq!(json[0]["instance_id"], "prod");
    }

    #[tokio::test]
    async fn test_logs_endpoint_filter_by_level() {
        let (state, token, _dir) = create_test_state().await;
        let log_buffer = state.hypervisor.log_buffer();

        // Add stdout and stderr entries
        log_buffer.push_stdout("api", "prod", "stdout message".to_string()).await;
        log_buffer.push_stderr("api", "prod", "stderr message".to_string()).await;

        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/logs?level=stderr")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let json: Vec<serde_json::Value> = response.json();
        assert_eq!(json.len(), 1);
        assert_eq!(json[0]["level"], "stderr");
    }

    #[tokio::test]
    async fn test_logs_endpoint_with_limit() {
        let (state, token, _dir) = create_test_state().await;
        let log_buffer = state.hypervisor.log_buffer();

        // Add multiple entries
        for i in 0..5 {
            log_buffer.push_stdout("api", "prod", format!("msg{}", i)).await;
        }

        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/logs?limit=2")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let json: Vec<serde_json::Value> = response.json();
        assert_eq!(json.len(), 2);
    }

    #[tokio::test]
    async fn test_logs_endpoint_search() {
        let (state, token, _dir) = create_test_state().await;
        let log_buffer = state.hypervisor.log_buffer();

        // Add entries with different content
        log_buffer.push_stdout("api", "prod", "hello world".to_string()).await;
        log_buffer.push_stdout("api", "prod", "goodbye world".to_string()).await;
        log_buffer.push_stdout("api", "prod", "hello there".to_string()).await;

        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/logs?search=hello")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let json: Vec<serde_json::Value> = response.json();
        assert_eq!(json.len(), 2);
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let (state, _token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server.get("/metrics").await;
        response.assert_status_ok();

        let text = response.text();
        // Should contain Prometheus format headers
        assert!(text.contains("# HELP tenement_requests_total"));
        assert!(text.contains("# TYPE tenement_requests_total counter"));
        assert!(text.contains("# HELP tenement_instances_up"));
        assert!(text.contains("# TYPE tenement_instances_up gauge"));
        assert!(text.contains("tenement_instances_up 0"));
    }

    #[tokio::test]
    async fn test_api_requires_auth() {
        let (state, _token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // API endpoints should return 401 without token
        let response = server.get("/api/instances").await;
        response.assert_status_unauthorized();

        let response = server.get("/api/logs").await;
        response.assert_status_unauthorized();
    }

    #[tokio::test]
    async fn test_api_invalid_token() {
        let (state, _token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Invalid token should return 401
        let response = server
            .get("/api/instances")
            .add_header("Authorization", "Bearer invalid_token")
            .await;
        response.assert_status_unauthorized();
    }
}
