//! HTTP server with subdomain routing, reverse proxy, and automatic TLS

use anyhow::Result;
use axum::{
    body::Body,
    extract::{Host, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Redirect, Response,
    },
    routing::get,
    Router,
};
use futures::stream::Stream;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use hyperlocal::UnixConnector;
use rustls_acme::{caches::DirCache, AcmeConfig};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tenement::{ConfigStore, Hypervisor, LogLevel, LogQuery, TokenStore};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::trace::TraceLayer;

/// TLS configuration for the server
#[derive(Debug, Clone)]
pub struct TlsOptions {
    pub enabled: bool,
    pub email: String,
    pub domain: String,
    pub cache_dir: PathBuf,
    pub staging: bool,
    pub https_port: u16,
    pub http_port: u16,
}

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
        // Fallback handles subdomain routing (for non-subdomain 404s)
        .fallback(handle_request)
        // Middleware layers are applied inside-out:
        // - TraceLayer runs first (outermost)
        // - subdomain_middleware runs second (intercepts subdomains before auth)
        // - auth_middleware runs last for non-subdomain requests
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .layer(middleware::from_fn_with_state(state.clone(), subdomain_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Subdomain routing middleware - intercepts subdomain requests before routes match
///
/// This middleware runs first (outermost layer) and handles subdomain routing
/// by proxying requests directly to the appropriate process instance.
/// Non-subdomain requests continue to the normal route handlers.
async fn subdomain_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    // Check if this is a subdomain request
    match parse_subdomain(host, &state.domain) {
        Some(SubdomainRoute::Direct { process, id }) => {
            // Direct route to specific instance: {id}.{process}.{domain}
            proxy_to_instance(&state, &process, Some(&id), req).await
        }
        Some(SubdomainRoute::Weighted { process }) => {
            // Weighted route across instances: {process}.{domain}
            proxy_to_instance(&state, &process, None, req).await
        }
        None => {
            // Not a subdomain request - continue to normal routes
            next.run(req).await
        }
    }
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

    // Subdomain requests are handled by subdomain_middleware before reaching here
    // so we don't need to check for subdomains in auth

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

/// Start the HTTP server (with optional TLS)
pub async fn serve(
    hypervisor: Arc<Hypervisor>,
    domain: String,
    port: u16,
    config_store: Arc<ConfigStore>,
    tls_options: Option<TlsOptions>,
) -> Result<()> {
    // Spawn configured instances before accepting connections
    let (success, failed) = hypervisor.spawn_configured_instances().await;
    if failed > 0 {
        tracing::warn!(
            "Auto-spawn: {} succeeded, {} failed - check logs for details",
            success, failed
        );
    } else if success > 0 {
        tracing::info!("Auto-spawn: {} instance(s) started", success);
    }

    // Start health monitor
    hypervisor.clone().start_monitor();

    let client = Client::builder(TokioExecutor::new()).build_http();

    let state = AppState {
        hypervisor,
        domain: domain.clone(),
        client,
        config_store,
    };

    match tls_options {
        Some(tls) if tls.enabled => {
            serve_with_tls(state, tls).await
        }
        _ => {
            serve_http_only(state, port).await
        }
    }
}

/// HTTP-only server (no TLS)
async fn serve_http_only(state: AppState, port: u16) -> Result<()> {
    let app = create_router(state.clone());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("tenement listening on http://{}", addr);
    tracing::info!("Dashboard at http://{}", state.domain);

    axum::serve(listener, app).await?;
    Ok(())
}

/// HTTPS server with automatic Let's Encrypt certificates
/// Uses TLS-ALPN-01 challenge (default in rustls-acme) - handles everything on port 443
async fn serve_with_tls(state: AppState, tls: TlsOptions) -> Result<()> {
    // Ensure cache directory exists
    std::fs::create_dir_all(&tls.cache_dir)?;

    // Create ACME configuration - uses TLS-ALPN-01 by default
    // TLS-ALPN-01 handles challenges on port 443, no separate port 80 listener needed
    let cache_dir = tls.cache_dir.clone();
    let mut acme_state = AcmeConfig::new([tls.domain.clone()])
        .contact([format!("mailto:{}", tls.email)])
        .cache(DirCache::new(cache_dir))
        .directory_lets_encrypt(!tls.staging) // true = production, false = staging
        .state();

    // Get acceptor for TLS connections (includes ACME challenge handling)
    let acceptor = acme_state.axum_acceptor(acme_state.default_rustls_config());

    // Spawn ACME event handler (handles cert acquisition/renewal)
    tokio::spawn(async move {
        loop {
            match acme_state.next().await {
                Some(Ok(event)) => {
                    tracing::info!("ACME event: {:?}", event);
                }
                Some(Err(err)) => {
                    tracing::error!("ACME error: {:?}", err);
                }
                None => break,
            }
        }
    });

    // Spawn HTTP redirect server on port 80
    let https_port = tls.https_port;
    let http_port = tls.http_port;

    let http_server = tokio::spawn(async move {
        if let Err(e) = serve_http_redirect(http_port, https_port).await {
            tracing::error!("HTTP redirect server error: {}", e);
        }
    });

    // Create HTTPS server
    let app = create_router(state.clone());
    let https_addr = SocketAddr::from(([0, 0, 0, 0], tls.https_port));

    tracing::info!("tenement listening on https://{}:{}", tls.domain, tls.https_port);
    tracing::info!("HTTP redirect on port {}", tls.http_port);
    if tls.staging {
        tracing::warn!("Using Let's Encrypt STAGING environment (certs not trusted by browsers)");
    }

    // Bind and serve HTTPS
    axum_server::bind(https_addr)
        .acceptor(acceptor)
        .serve(app.into_make_service())
        .await?;

    http_server.abort();
    Ok(())
}

/// HTTP server on port 80 - redirects all traffic to HTTPS
/// (TLS-ALPN-01 handles ACME challenges on port 443, so no challenge handling needed here)
async fn serve_http_redirect(http_port: u16, https_port: u16) -> Result<()> {
    let redirect_app = Router::new().fallback(move |Host(host): Host, req: Request<Body>| {
        async move {
            // Strip port from host if present
            let host = host.split(':').next().unwrap_or(&host);
            let path = req
                .uri()
                .path_and_query()
                .map(|pq| pq.as_str())
                .unwrap_or("/");

            let redirect_url = if https_port == 443 {
                format!("https://{}{}", host, path)
            } else {
                format!("https://{}:{}{}", host, https_port, path)
            };

            Redirect::permanent(&redirect_url)
        }
    });

    let addr = SocketAddr::from(([0, 0, 0, 0], http_port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::debug!("HTTP redirect server listening on port {}", http_port);

    axum::serve(listener, redirect_app).await?;
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
            weight: i.weight,
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
    weight: u8,
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

/// Instance connection info for proxying
struct ProxyTarget {
    socket: std::path::PathBuf,
    port: Option<u16>,
}

impl ProxyTarget {
    fn uses_tcp(&self) -> bool {
        self.port.is_some()
    }

    fn tcp_addr(&self) -> Option<String> {
        self.port.map(|p| format!("127.0.0.1:{}", p))
    }
}

/// Handle incoming requests - route to dashboard or proxy to process
async fn handle_request(
    Host(host): Host,
    State(state): State<AppState>,
    req: Request<Body>,
) -> Response {
    // Parse subdomain pattern
    match parse_subdomain(&host, &state.domain) {
        Some(SubdomainRoute::Direct { process, id }) => {
            // Direct route to specific instance: {id}.{process}.{domain}
            proxy_to_instance(&state, &process, Some(&id), req).await
        }
        Some(SubdomainRoute::Weighted { process }) => {
            // Weighted route across instances: {process}.{domain}
            proxy_to_instance(&state, &process, None, req).await
        }
        None => {
            // No subdomain or invalid pattern - serve dashboard
            // For now just return 404 for non-dashboard routes
            (StatusCode::NOT_FOUND, "Not found").into_response()
        }
    }
}

/// Subdomain routing types
enum SubdomainRoute {
    /// Direct route to a specific instance: {id}.{process}.{domain}
    Direct { process: String, id: String },
    /// Weighted route across all instances of a process: {process}.{domain}
    Weighted { process: String },
}

/// Parse subdomain pattern:
/// - {id}.{process}.{domain} -> Direct route to specific instance
/// - {process}.{domain} -> Weighted route across all instances
fn parse_subdomain(host: &str, domain: &str) -> Option<SubdomainRoute> {
    // Strip port if present
    let host = host.split(':').next().unwrap_or(host);

    // Check if host ends with domain
    if !host.ends_with(domain) {
        return None;
    }

    // Get subdomain part
    let subdomain = host.strip_suffix(domain)?.strip_suffix('.')?;
    if subdomain.is_empty() {
        return None;
    }

    // Split into parts: could be "id.process" or just "process"
    let parts: Vec<&str> = subdomain.splitn(2, '.').collect();

    match parts.len() {
        1 => {
            // Single part: {process}.{domain} -> weighted routing
            let process = parts[0].to_string();
            if process.is_empty() {
                return None;
            }
            Some(SubdomainRoute::Weighted { process })
        }
        2 => {
            // Two parts: {id}.{process}.{domain} -> direct routing
            let id = parts[0].to_string();
            let process = parts[1].to_string();
            if id.is_empty() || process.is_empty() {
                return None;
            }
            Some(SubdomainRoute::Direct { process, id })
        }
        _ => None,
    }
}

/// Proxy request to a process instance via unix socket
///
/// If `id` is Some, routes directly to that specific instance.
/// If `id` is None, uses weighted random selection across all instances.
///
/// Implements wake-on-request: if the instance is not running but the process
/// is configured, it will spawn the instance and wait for it to be ready.
async fn proxy_to_instance(
    state: &AppState,
    process: &str,
    id: Option<&str>,
    req: Request<Body>,
) -> Response {
    // Check if process is configured first
    if !state.hypervisor.has_process(process) {
        return (
            StatusCode::NOT_FOUND,
            format!("Process '{}' not configured", process),
        )
            .into_response();
    }

    let target = match id {
        Some(instance_id) => {
            // Direct routing to specific instance
            match state.hypervisor.get_and_touch(process, instance_id).await {
                Some(info) => ProxyTarget {
                    socket: info.socket,
                    port: info.port,
                },
                None => {
                    // Wake-on-request: spawn and wait for instance to be ready
                    tracing::info!("Waking instance {}:{}", process, instance_id);
                    match state.hypervisor.spawn_and_wait(process, instance_id).await {
                        Ok(socket) => {
                            // Get port info from the now-running instance
                            let port = state
                                .hypervisor
                                .get(process, instance_id)
                                .await
                                .and_then(|info| info.port);
                            ProxyTarget { socket, port }
                        }
                        Err(e) => {
                            tracing::error!("Failed to wake instance {}:{}: {}", process, instance_id, e);
                            return (
                                StatusCode::SERVICE_UNAVAILABLE,
                                format!("Failed to start instance: {}", e),
                            )
                                .into_response();
                        }
                    }
                }
            }
        }
        None => {
            // Weighted routing across all instances
            match state.hypervisor.select_weighted(process).await {
                Some(info) => {
                    // Touch activity for the selected instance
                    state.hypervisor.touch_activity(process, &info.id.id).await;
                    ProxyTarget {
                        socket: info.socket,
                        port: info.port,
                    }
                }
                None => {
                    // No instances available - return 503
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("No instances available for process '{}'", process),
                    )
                        .into_response();
                }
            }
        }
    };

    // Proxy based on connection type
    if let Some(addr) = target.tcp_addr() {
        proxy_to_tcp(&state.client, &addr, req).await
    } else {
        proxy_to_unix_socket(&target.socket, req).await
    }
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

/// Proxy an HTTP request to a TCP address
async fn proxy_to_tcp(
    client: &Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
    addr: &str,
    req: Request<Body>,
) -> Response {
    // Build URI for TCP connection
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let uri = format!("http://{}{}", addr, path_and_query);

    // Build proxy request preserving method and headers
    let mut proxy_req = Request::builder()
        .method(req.method())
        .uri(&uri);

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

    // Forward request to TCP address
    match client.request(proxy_req).await {
        Ok(response) => {
            // Convert hyper Response to axum Response
            let (parts, body) = response.into_parts();
            Response::from_parts(parts, Body::new(body))
        }
        Err(e) => {
            tracing::error!("Proxy error to {}: {}", addr, e);
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
        // Direct routing patterns: {id}.{process}.{domain}
        match parse_subdomain("prod.api.example.com", "example.com") {
            Some(SubdomainRoute::Direct { process, id }) => {
                assert_eq!(process, "api");
                assert_eq!(id, "prod");
            }
            _ => panic!("Expected Direct route"),
        }
        match parse_subdomain("user123.app.example.com", "example.com") {
            Some(SubdomainRoute::Direct { process, id }) => {
                assert_eq!(process, "app");
                assert_eq!(id, "user123");
            }
            _ => panic!("Expected Direct route"),
        }
        match parse_subdomain("staging.web.example.com:8080", "example.com") {
            Some(SubdomainRoute::Direct { process, id }) => {
                assert_eq!(process, "web");
                assert_eq!(id, "staging");
            }
            _ => panic!("Expected Direct route"),
        }

        // Weighted routing patterns: {process}.{domain}
        match parse_subdomain("api.example.com", "example.com") {
            Some(SubdomainRoute::Weighted { process }) => {
                assert_eq!(process, "api");
            }
            _ => panic!("Expected Weighted route"),
        }
        match parse_subdomain("web.example.com:8080", "example.com") {
            Some(SubdomainRoute::Weighted { process }) => {
                assert_eq!(process, "web");
            }
            _ => panic!("Expected Weighted route"),
        }

        // Invalid patterns
        assert!(parse_subdomain("example.com", "example.com").is_none());
        assert!(parse_subdomain("other.com", "example.com").is_none());
        assert!(parse_subdomain("", "example.com").is_none());
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
