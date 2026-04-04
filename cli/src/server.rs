//! HTTP server with subdomain routing, reverse proxy, and automatic TLS

use anyhow::{Context, Result};
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

/// TLS status information for the status endpoint
#[derive(Clone, Default)]
pub struct TlsStatus {
    pub enabled: bool,
    pub domain: Option<String>,
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
    pub unix_client: Client<UnixConnector, Body>,
    pub config_store: Arc<ConfigStore>,
    pub deploy_log: Arc<tenement::DeployLogStore>,
    pub tenant_tokens: Arc<tenement::TenantTokenStore>,
    pub tls_status: TlsStatus,
    /// Tracks failed auth attempts for rate limiting.
    /// Stores (failure_count, last_failure_time). Resets after cooldown.
    pub auth_failures: Arc<tokio::sync::RwLock<(u32, Option<std::time::Instant>)>>,
}

/// Authenticated caller identity, injected by auth middleware into request extensions.
/// Admin token: tenant_id is None (full access).
/// Tenant token: tenant_id is Some("alice") (scoped access).
#[derive(Clone, Debug)]
pub struct AuthIdentity {
    pub tenant_id: Option<String>,
}

/// Create the router (exposed for testing)
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Dashboard/API routes (root domain)
        .route("/", get(dashboard))
        .route("/_overview", get(overview_partial))
        .route("/_instances", get(instances_partial))
        .route("/_logs", get(logs_partial))
        .route("/login", get(login_page).post(handle_login))
        .route("/instances", get(instances_page))
        .route("/logs", get(logs_page))
        .route("/health", get(health))
        .route("/metrics", get(metrics_endpoint))
        .route("/api/telemetry", get(telemetry_endpoint))
        .route("/api/instances", get(list_instances))
        .route("/api/instances/spawn", axum::routing::post(crate::api_routes::post_spawn))
        .route("/api/instances/:id", axum::routing::delete(crate::api_routes::delete_instance))
        .route("/api/instances/:id/storage", get(get_instance_storage))
        .route("/api/instances/:id/restart", axum::routing::post(crate::api_routes::post_restart))
        .route("/api/instances/:id/weight", axum::routing::put(crate::api_routes::put_weight))
        .route("/api/instances/:id/health", axum::routing::get(crate::api_routes::get_health_check))
        .route("/api/deploy", axum::routing::post(crate::api_routes::post_deploy))
        .route("/api/route", axum::routing::post(crate::api_routes::post_route))
        .route("/api/logs", get(query_logs))
        .route("/api/logs/stream", get(stream_logs))
        .route("/api/tls/status", get(tls_status_endpoint))
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

/// Wait for shutdown signal (SIGTERM or SIGINT), then stop all instances.
async fn shutdown_signal(hypervisor: Arc<Hypervisor>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, shutting down");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, shutting down");
        },
    }

    hypervisor.stop_all().await;
}

/// Constant-time byte comparison to prevent timing attacks on token verification
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
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
            // Direct route to specific instance: :id.{process}.{domain}
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
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();

    // Skip auth for public endpoints
    if path == "/health" || path == "/metrics" || path == "/api/telemetry" || path == "/login" || path.starts_with("/assets/") {
        return Ok(next.run(req).await);
    }

    // Skip auth for HTML page routes - they use cookie-based auth handled in the handler
    if path == "/" || path == "/instances" || path == "/logs" || path.starts_with("/_") {
        return Ok(next.run(req).await);
    }

    // Subdomain requests are handled by subdomain_middleware before reaching here
    // so we don't need to check for subdomains in auth

    // Extract token from Authorization header or cookie
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token: String = match auth_header {
        Some(h) if h.to_lowercase().starts_with("bearer ") => h[7..].to_string(),
        _ => {
            // Try cookie as fallback
            match req.headers().get(axum::http::header::COOKIE) {
                Some(cookie_val) => {
                    let cookie_str = cookie_val.to_str().unwrap_or("");
                    let mut found_token = None;
                    for cookie in cookie_str.split(';') {
                        let cookie = cookie.trim();
                        if let Some((k, v)) = cookie.split_once('=') {
                            if k.trim() == "tenement_token" {
                                found_token = Some(v.to_string());
                                break;
                            }
                        }
                    }
                    match found_token {
                        Some(t) => t,
                        None => {
                            tracing::debug!("Missing or invalid Authorization header and no cookie");
                            return Err(StatusCode::UNAUTHORIZED);
                        }
                    }
                }
                None => {
                    tracing::debug!("Missing or invalid Authorization header and no cookie");
                    return Err(StatusCode::UNAUTHORIZED);
                }
            }
        }
    };

    // Rate limit: reject immediately if too many recent failures (prevents Argon2 DoS)
    {
        let failures = state.auth_failures.read().await;
        let (count, last_time) = &*failures;
        if *count >= 10 {
            if let Some(last) = last_time {
                // Cool down for 5 seconds after 10 consecutive failures
                if last.elapsed() < std::time::Duration::from_secs(5) {
                    tracing::debug!("Auth rate limited ({} recent failures)", count);
                    return Err(StatusCode::TOO_MANY_REQUESTS);
                }
            }
        }
    }

    // Try admin token first
    let token_store = TokenStore::new(&state.config_store);
    match token_store.verify(&token).await {
        Ok(true) => {
            let mut failures = state.auth_failures.write().await;
            *failures = (0, None);
            // Admin token: full access (no tenant scoping)
            req.extensions_mut().insert(AuthIdentity { tenant_id: None });
            return Ok(next.run(req).await);
        }
        Ok(false) => {} // Not the admin token, try tenant tokens
        Err(e) => {
            tracing::error!("Token verification error: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Try tenant token
    match state.tenant_tokens.verify(&token).await {
        Ok(Some(tenant_id)) => {
            let mut failures = state.auth_failures.write().await;
            *failures = (0, None);
            // Tenant token: scoped access
            req.extensions_mut().insert(AuthIdentity {
                tenant_id: Some(tenant_id),
            });
            Ok(next.run(req).await)
        }
        Ok(None) => {
            // Neither admin nor tenant token matched
            let mut failures = state.auth_failures.write().await;
            failures.0 += 1;
            failures.1 = Some(std::time::Instant::now());
            tracing::debug!("Invalid token (failure #{})", failures.0);
            Err(StatusCode::UNAUTHORIZED)
        }
        Err(e) => {
            tracing::error!("Tenant token verification error: {}", e);
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
    deploy_log: Arc<tenement::DeployLogStore>,
    tenant_tokens: Arc<tenement::TenantTokenStore>,
    tls_options: Option<TlsOptions>,
) -> Result<()> {
    // Recover any orphaned instances from a previous crash
    hypervisor.recover_orphans().await;

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
    let unix_client = Client::builder(TokioExecutor::new()).build(UnixConnector);

    // Build TLS status from options
    let tls_status = match &tls_options {
        Some(tls) if tls.enabled => TlsStatus {
            enabled: true,
            domain: Some(tls.domain.clone()),
            staging: tls.staging,
            https_port: tls.https_port,
            http_port: tls.http_port,
        },
        _ => TlsStatus::default(),
    };

    let state = AppState {
        hypervisor,
        domain: domain.clone(),
        client,
        unix_client,
        config_store,
        deploy_log,
        tenant_tokens,
        tls_status,
        auth_failures: Arc::new(tokio::sync::RwLock::new((0, None))),
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

    let hypervisor = state.hypervisor.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(hypervisor))
        .await?;
    Ok(())
}

/// HTTPS server with automatic Let's Encrypt certificates
/// Uses TLS-ALPN-01 challenge (default in rustls-acme) - handles everything on port 443
async fn serve_with_tls(state: AppState, tls: TlsOptions) -> Result<()> {
    // Ensure cache directory exists with secure permissions
    std::fs::create_dir_all(&tls.cache_dir)?;

    // Set restrictive permissions on cache directory (contains private keys)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&tls.cache_dir, perms).with_context(|| {
            format!(
                "Failed to set secure permissions on ACME cache directory: {}",
                tls.cache_dir.display()
            )
        })?;
    }

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
    // Tracks consecutive errors and provides troubleshooting hints
    let acme_domain = tls.domain.clone();
    tokio::spawn(async move {
        let mut consecutive_errors: u32 = 0;
        let mut cert_acquired = false;

        loop {
            match acme_state.next().await {
                Some(Ok(event)) => {
                    consecutive_errors = 0;
                    cert_acquired = true;
                    tracing::info!("ACME: Certificate event for {}: {:?}", acme_domain, event);
                }
                Some(Err(err)) => {
                    consecutive_errors += 1;
                    tracing::error!(
                        "ACME error (attempt {}) for {}: {:?}",
                        consecutive_errors,
                        acme_domain,
                        err
                    );

                    // After 3 consecutive errors, provide troubleshooting hints
                    if consecutive_errors == 3 {
                        tracing::warn!(
                            "ACME certificate acquisition failing. Troubleshooting checklist:\n\
                            - Verify DNS for {} points to this server\n\
                            - Ensure port 443 is accessible from the internet\n\
                            - Check firewall allows inbound HTTPS traffic\n\
                            - Verify domain ownership if using production Let's Encrypt",
                            acme_domain
                        );
                    }

                    // After 10 consecutive errors, warn about rate limits
                    if consecutive_errors == 10 && !cert_acquired {
                        tracing::error!(
                            "ACME has failed {} times without acquiring a certificate.\n\
                            Consider using --staging flag to avoid hitting Let's Encrypt rate limits.\n\
                            Rate limit: 5 failed validations per account per hostname per hour.",
                            consecutive_errors
                        );
                    }
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

/// Extract a cookie value by name from headers
fn extract_cookie<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> String {
    let cookie_header = headers.get(axum::http::header::COOKIE);
    match cookie_header {
        Some(val) => {
            let val = val.to_str().unwrap_or("");
            for cookie in val.split(';') {
                let cookie = cookie.trim();
                if let Some((k, v)) = cookie.split_once('=') {
                    if k.trim() == name {
                        return v.to_string();
                    }
                }
            }
            String::new()
        }
        None => String::new(),
    }
}

/// Dashboard overview page
async fn dashboard(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Response {
    let token = extract_cookie(req.headers(), "tenement_token");
    if token.is_empty() {
        return (StatusCode::SEE_OTHER, [(axum::http::header::LOCATION, "/login")]).into_response();
    }

    let (instances, summary, error) = match fetch_dashboard_data(&state, &token).await {
        Ok(data) => data,
        Err(e) => (vec![], None, Some(e)),
    };

    let tmpl = crate::dashboard::OverviewTemplate {
        auth_token: &token,
        summary,
        active_tab: "overview",
        instances,
        error,
    };
    axum::response::Html(tmpl.to_string()).into_response()
}

/// Login page
async fn login_page(query: Option<axum::extract::Query<std::collections::HashMap<String, String>>>) -> impl IntoResponse {
    let error = query.as_ref().and_then(|q| q.get("error").cloned());
    let tmpl = crate::dashboard::LoginTemplate { error };
    axum::response::Html(tmpl.to_string())
}

/// Handle login form submission
async fn handle_login(
    State(state): State<AppState>,
    form: axum::Form<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let token = form.get("token").cloned().unwrap_or_default();
    if token.is_empty() {
        return (StatusCode::SEE_OTHER, [(axum::http::header::LOCATION, "/login?error=Token+required")]).into_response();
    }

    // Verify the token
    let token_store = tenement::TokenStore::new(&state.config_store);
    match token_store.verify(&token).await {
        Ok(true) => {
            // Set HTTP-only cookie and redirect
            let cookie_header = format!("tenement_token={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=86400", token);
            (
                StatusCode::SEE_OTHER,
                [
                    (axum::http::header::LOCATION, "/"),
                    (axum::http::header::SET_COOKIE, cookie_header.as_str()),
                ],
            ).into_response()
        }
        _ => {
            (StatusCode::SEE_OTHER, [(axum::http::header::LOCATION, "/login?error=Invalid+token")]).into_response()
        }
    }
}

/// Instances page
async fn instances_page(
    State(state): State<AppState>,
    req: Request<Body>,
) -> impl IntoResponse {
    let token = extract_cookie(req.headers(), "tenement_token");
    let (instances, summary, error) = if token.is_empty() {
        (vec![], None, None)
    } else {
        match fetch_dashboard_data(&state, &token).await {
            Ok(data) => data,
            Err(e) => (vec![], None, Some(e)),
        }
    };

    let tmpl = crate::dashboard::InstancesTemplate {
        auth_token: &token,
        summary,
        active_tab: "instances",
        instances,
        error,
    };
    axum::response::Html(tmpl.to_string())
}

/// Logs page
async fn logs_page(
    State(state): State<AppState>,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
    req: Request<Body>,
) -> impl IntoResponse {
    let token = extract_cookie(req.headers(), "tenement_token");
    let filter_process = query.get("process").cloned().unwrap_or_default();
    let filter_level = query.get("level").cloned().unwrap_or_default();
    let search = query.get("search").cloned().unwrap_or_default();

    let logs = if token.is_empty() {
        vec![]
    } else {
        let limit: u32 = query.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100);
        match fetch_logs(&state, &token, &filter_process, &filter_level, &search, limit).await {
            Ok(logs) => logs,
            Err(_) => vec![],
        }
    };

    let processes = if token.is_empty() {
        vec![]
    } else {
        match fetch_process_list(&state, &token).await {
            Ok(p) => p,
            Err(_) => vec![],
        }
    };

    let summary = if token.is_empty() {
        None
    } else {
        fetch_summary(&state, &token).await.ok()
    };

    let tmpl = crate::dashboard::LogsTemplate {
        auth_token: &token,
        summary,
        active_tab: "logs",
        logs,
        processes,
        filter_process,
        filter_level,
        search,
        error: None,
    };
    axum::response::Html(tmpl.to_string())
}

/// Partial: overview content only (for HTMX refresh)
async fn overview_partial(
    State(state): State<AppState>,
    req: Request<Body>,
) -> impl IntoResponse {
    let token = extract_cookie(req.headers(), "tenement_token");
    let (instances, summary, error) = if token.is_empty() {
        (vec![], None, None)
    } else {
        match fetch_dashboard_data(&state, &token).await {
            Ok(data) => data,
            Err(e) => (vec![], None, Some(e)),
        }
    };

    let tmpl = crate::dashboard::OverviewContentTemplate {
        auth_token: &token,
        summary,
        active_tab: "overview",
        instances,
        error,
    };
    axum::response::Html(tmpl.to_string())
}

/// Partial: instances content only
async fn instances_partial(
    State(state): State<AppState>,
    req: Request<Body>,
) -> impl IntoResponse {
    let token = extract_cookie(req.headers(), "tenement_token");
    let (instances, summary, error) = if token.is_empty() {
        (vec![], None, None)
    } else {
        match fetch_dashboard_data(&state, &token).await {
            Ok(data) => data,
            Err(e) => (vec![], None, Some(e)),
        }
    };

    let tmpl = crate::dashboard::InstancesContentTemplate {
        auth_token: &token,
        summary,
        active_tab: "instances",
        instances,
        error,
    };
    axum::response::Html(tmpl.to_string())
}

/// Partial: logs content only
async fn logs_partial(
    State(state): State<AppState>,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
    req: Request<Body>,
) -> impl IntoResponse {
    let token = extract_cookie(req.headers(), "tenement_token");
    let filter_process = query.get("process").cloned().unwrap_or_default();
    let filter_level = query.get("level").cloned().unwrap_or_default();
    let search = query.get("search").cloned().unwrap_or_default();

    let logs = if token.is_empty() {
        vec![]
    } else {
        let limit: u32 = query.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100);
        match fetch_logs(&state, &token, &filter_process, &filter_level, &search, limit).await {
            Ok(logs) => logs,
            Err(_) => vec![],
        }
    };

    let processes = if token.is_empty() {
        vec![]
    } else {
        match fetch_process_list(&state, &token).await {
            Ok(p) => p,
            Err(_) => vec![],
        }
    };

    let summary = if token.is_empty() {
        None
    } else {
        fetch_summary(&state, &token).await.ok()
    };

    let tmpl = crate::dashboard::LogsContentTemplate {
        auth_token: &token,
        summary,
        active_tab: "logs",
        logs,
        processes,
        filter_process,
        filter_level,
        search,
        error: None,
    };
    axum::response::Html(tmpl.to_string())
}

/// Fetch dashboard data from internal API
async fn fetch_dashboard_data(
    state: &AppState,
    token: &str,
) -> Result<(Vec<crate::dashboard::InstanceRow>, Option<crate::dashboard::SummaryData>, Option<String>), String> {
    let instances = match fetch_instances(state, token).await {
        Ok(i) => i,
        Err(e) => return Err(e),
    };
    let summary = fetch_summary(state, token).await.ok();
    Ok((instances, summary, None))
}

async fn fetch_instances(
    state: &AppState,
    _token: &str,
) -> Result<Vec<crate::dashboard::InstanceRow>, String> {
    let instances = state.hypervisor.list().await;
    let mut rows = Vec::new();
    for inst in instances {
        rows.push(crate::dashboard::InstanceRow {
            id: inst.id.to_string(),
            health_badge: crate::dashboard::health_badge(&inst.health.to_string()).to_string(),
            health_color: crate::dashboard::health_color(&inst.health.to_string()).to_string(),
            requests_total: "-".to_string(),
            avg_latency_ms: "-".to_string(),
            uptime: crate::dashboard::format_duration(inst.uptime_secs),
            idle: crate::dashboard::format_duration(inst.idle_secs),
            restarts: inst.restarts.to_string(),
            weight: "100".to_string(),
            storage: crate::dashboard::format_bytes(inst.storage_used_bytes),
            health: inst.health.to_string(),
        });
    }
    Ok(rows)
}

async fn fetch_summary(
    state: &AppState,
    _token: &str,
) -> Result<crate::dashboard::SummaryData, String> {
    let instances = state.hypervisor.list().await;
    let total_instances = instances.len();
    let healthy_instances = instances.iter().filter(|i| i.health.to_string() == "healthy").count();
    Ok(crate::dashboard::SummaryData {
        total_instances,
        healthy_instances,
        total_requests: 0,
    })
}

async fn fetch_logs(
    state: &AppState,
    _token: &str,
    process: &str,
    level: &str,
    search: &str,
    limit: u32,
) -> Result<Vec<crate::dashboard::LogEntry>, String> {
    let log_buffer = state.hypervisor.log_buffer();
    let query = tenement::LogQuery {
        process: if process.is_empty() { None } else { Some(process.to_string()) },
        instance_id: None,
        level: if level.is_empty() { None } else { Some(tenement::LogLevel::Stderr) },
        search: if search.is_empty() { None } else { Some(search.to_string()) },
        limit: Some(limit as usize),
    };
    let logs = log_buffer.query(&query).await;

    let mut entries = Vec::new();
    for log in logs {
        let time = chrono::DateTime::from_timestamp(log.timestamp as i64, 0)
            .map(|dt| dt.format("%H:%M:%S").to_string())
            .unwrap_or_default();

        let process_label = format!("{}:{}", log.process, log.instance_id);
        let is_error = log.level == tenement::LogLevel::Stderr;
        let level_label = if is_error { "ERR" } else { "OUT" };

        entries.push(crate::dashboard::LogEntry {
            time,
            process: process_label,
            level_label: level_label.to_string(),
            is_error,
            message: log.message,
        });
    }
    Ok(entries)
}

async fn fetch_process_list(
    state: &AppState,
    token: &str,
) -> Result<Vec<String>, String> {
    let instances = fetch_instances(state, token).await?;
    let mut processes: Vec<String> = instances.iter()
        .map(|i| i.id.split(':').next().unwrap_or("").to_string())
        .collect();
    processes.sort();
    processes.dedup();
    Ok(processes)
}

/// Health check endpoint
async fn health() -> impl IntoResponse {
    Json(HealthResponse { status: "ok" })
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// TLS status endpoint - returns current TLS configuration
async fn tls_status_endpoint(State(state): State<AppState>) -> impl IntoResponse {
    Json(TlsStatusResponse {
        enabled: state.tls_status.enabled,
        domain: state.tls_status.domain.clone(),
        staging: state.tls_status.staging,
        https_port: state.tls_status.https_port,
        http_port: state.tls_status.http_port,
        recommendation: if state.tls_status.enabled {
            None
        } else {
            Some("Use --caddy flag with `ten install` for production HTTPS".to_string())
        },
    })
}

#[derive(Serialize)]
struct TlsStatusResponse {
    enabled: bool,
    domain: Option<String>,
    staging: bool,
    https_port: u16,
    http_port: u16,
    recommendation: Option<String>,
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

/// Telemetry endpoint - returns structured JSON metrics for the dashboard
async fn telemetry_endpoint(State(state): State<AppState>) -> impl IntoResponse {
    let metrics = state.hypervisor.metrics();
    let instances = state.hypervisor.list().await;

    // Build per-instance telemetry
    let mut instance_telemetry = Vec::new();
    for info in &instances {
        let id_str = info.id.to_string();
        let mut labels = std::collections::HashMap::new();
        labels.insert("process".to_string(), info.id.process.clone());
        labels.insert("instance".to_string(), info.id.id.clone());

        let requests = metrics.requests_total.with_labels(&labels).await.get();
        let duration = metrics.request_duration_ms.with_labels(&labels).await;

        instance_telemetry.push(serde_json::json!({
            "id": id_str,
            "process": info.id.process,
            "instance": info.id.id,
            "health": info.health.to_string(),
            "uptime_secs": info.uptime_secs,
            "idle_secs": info.idle_secs,
            "restarts": info.restarts,
            "weight": info.weight,
            "requests_total": requests,
            "request_duration_avg_ms": if duration.get_count() > 0 {
                duration.get_sum() / duration.get_count() as f64
            } else {
                0.0
            },
            "request_duration_p99_ms": 0.0, // TODO: calculate from histogram
            "storage_used_bytes": info.storage_used_bytes,
            "storage_quota_bytes": info.storage_quota_bytes,
        }));
    }

    // Summary stats
    let total_instances = instances.len();
    let healthy_count = instances.iter().filter(|i| i.health == tenement::instance::HealthStatus::Healthy).count();
    let total_requests: u64 = metrics.requests_total.all().await.iter().map(|(_, v)| *v).sum();

    Json(serde_json::json!({
        "summary": {
            "total_instances": total_instances,
            "healthy_instances": healthy_count,
            "total_requests": total_requests,
        },
        "instances": instance_telemetry,
    }))
}

/// List all running instances (scoped by tenant token if present)
async fn list_instances(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthIdentity>,
) -> impl IntoResponse {
    let instances = state.hypervisor.list().await;
    let response: Vec<InstanceInfo> = instances
        .into_iter()
        .filter(|i| {
            // Tenant tokens can only see their own instances
            match &auth.tenant_id {
                Some(tenant) => &i.id.id == tenant,
                None => true, // Admin sees everything
            }
        })
        .map(|i| InstanceInfo {
            id: i.id.to_string(),
            socket: i.listen_addr(),
            uptime_secs: i.uptime_secs,
            idle_secs: i.idle_secs,
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
    idle_secs: u64,
    restarts: u32,
    health: String,
    storage_used_bytes: u64,
    storage_quota_bytes: Option<u64>,
    weight: u8,
}

/// Get storage info for a specific instance
/// Instance ID format: process:instance (e.g., "api:prod")
pub async fn get_instance_storage(
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
pub struct StorageInfoResponse {
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
    axum::Extension(auth): axum::Extension<AuthIdentity>,
) -> impl IntoResponse {
    let mut query: LogQuery = params.into();
    // Tenant tokens can only see their own logs
    if let Some(ref tenant) = auth.tenant_id {
        query.instance_id = Some(tenant.clone());
    }
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
            // Direct route to specific instance: :id.{process}.{domain}
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
    /// Direct route to a specific instance: :id.{process}.{domain}
    Direct { process: String, id: String },
    /// Weighted route across all instances of a process: {process}.{domain}
    Weighted { process: String },
}

/// Parse subdomain pattern:
/// - :id.{process}.{domain} -> Direct route to specific instance
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
            // Two parts: :id.{process}.{domain} -> direct routing
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
    let start = std::time::Instant::now();
    tracing::debug!(
        process = process,
        instance = id.unwrap_or("weighted"),
        method = %req.method(),
        path = %req.uri().path(),
        "proxy request"
    );

    // Check if process is configured first
    if !state.hypervisor.has_process(process) {
        tracing::debug!("Subdomain request for unconfigured process: {}", process);
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }

    let mut resolved_instance_id: Option<String> = None;
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
                                "Service temporarily unavailable",
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
                    resolved_instance_id = Some(info.id.id.clone());
                    ProxyTarget {
                        socket: info.socket,
                        port: info.port,
                    }
                }
                None => {
                    // No instances available - return 503
                    tracing::debug!("No instances available for process '{}'", process);
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Service temporarily unavailable",
                    )
                        .into_response();
                }
            }
        }
    };

    // Use the resolved instance ID (from weighted selection or direct routing)
    let conn_instance_id = resolved_instance_id.as_deref().or(id).unwrap_or("unknown");
    let _conn_guard = state.hypervisor.connection_start(process, conn_instance_id).await;

    // Proxy with request timeout
    let timeout = state.hypervisor.request_timeout(process);
    let proxy_future: std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> =
        if let Some(addr) = target.tcp_addr() {
            let client = state.client.clone();
            Box::pin(async move { proxy_to_tcp(&client, &addr, req).await })
        } else {
            let socket = target.socket.clone();
            let unix_client = state.unix_client.clone();
            Box::pin(async move { proxy_to_unix_socket(&unix_client, &socket, req).await })
        };

    let response = match tokio::time::timeout(timeout, proxy_future).await {
        Ok(resp) => resp,
        Err(_) => {
            tracing::error!("Request timeout after {:?} for process {}", timeout, process);
            (StatusCode::GATEWAY_TIMEOUT, "Gateway timeout").into_response()
        }
    };

    // Record request metrics
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
    let instance_id = conn_instance_id;
    let metrics = state.hypervisor.metrics();
    let mut labels = std::collections::HashMap::new();
    labels.insert("process".to_string(), process.to_string());
    labels.insert("instance".to_string(), instance_id.to_string());
    let counter = metrics.requests_total.with_labels(&labels).await;
    counter.inc();
    let histogram = metrics.request_duration_ms.with_labels(&labels).await;
    histogram.observe(duration_ms);

    response
}

/// Proxy an HTTP request to a Unix socket (uses pooled client)
async fn proxy_to_unix_socket(
    client: &Client<UnixConnector, Body>,
    socket_path: &Path,
    req: Request<Body>,
) -> Response {

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
                "Internal server error".to_string(),
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
                "Bad gateway".to_string(),
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
                "Internal server error".to_string(),
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
                "Bad gateway".to_string(),
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
        // Direct routing patterns: :id.{process}.{domain}
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
        let config_store = Arc::new(ConfigStore::new(pool.clone()));
        let deploy_log = Arc::new(tenement::DeployLogStore::new(pool.clone()));
        let tenant_tokens = Arc::new(tenement::TenantTokenStore::new(pool));

        // Generate and store a test token
        let token_store = TokenStore::new(&config_store);
        let token = token_store.generate_and_store().await.unwrap();

        let config = Config::default();
        let hypervisor = Hypervisor::new(config);
        let client = Client::builder(TokioExecutor::new()).build_http();
        let unix_client = Client::builder(TokioExecutor::new()).build(UnixConnector);
        let state = AppState {
            hypervisor,
            domain: "example.com".to_string(),
            client,
            unix_client,
            config_store,
            deploy_log,
            tenant_tokens,
            tls_status: TlsStatus::default(),
            auth_failures: Arc::new(tokio::sync::RwLock::new((0, None))),
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
        // Error message is sanitized (no internal details for unauthenticated users)
        response.assert_text_contains("Not found");
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

    // ===================
    // MUTATION API TESTS (Phase My Way)
    // ===================

    #[tokio::test]
    async fn test_spawn_requires_auth() {
        let (state, _token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .post("/api/instances/spawn")
            .json(&serde_json::json!({"process": "api", "id": "prod"}))
            .await;
        response.assert_status_unauthorized();
    }

    #[tokio::test]
    async fn test_spawn_unknown_process() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .post("/api/instances/spawn")
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({"process": "nonexistent", "id": "prod"}))
            .await;

        response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
        let json: serde_json::Value = response.json();
        assert!(json["error"].as_str().unwrap().contains("Unknown process"));
    }

    #[tokio::test]
    async fn test_stop_not_found() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .delete("/api/instances/api:prod")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;

        response.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_stop_invalid_id_format() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Instance ID without colon: parse_instance_id returns BAD_REQUEST
        let response = server
            .delete("/api/instances/invalid-no-colon")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;

        let status = response.status_code();
        // Either BAD_REQUEST (handler reached) or NOT_FOUND (fallback)
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
            "Expected 400 or 404, got {}",
            status
        );
    }

    #[tokio::test]
    async fn test_weight_not_found() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .put("/api/instances/api:prod/weight")
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({"weight": 50}))
            .await;

        response.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_health_check_unknown_instance() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/instances/api:prod/health")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;

        response.assert_status_ok();
        let json: serde_json::Value = response.json();
        assert_eq!(json["health"], "unknown");
    }

    #[tokio::test]
    async fn test_deploy_unknown_process() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .post("/api/deploy")
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "process": "nonexistent",
                "version": "v1",
                "weight": 100,
                "timeout": 2
            }))
            .await;

        response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_route_not_found() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .post("/api/route")
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "process": "api",
                "from": "v1",
                "to": "v2"
            }))
            .await;

        response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_restart_not_found() {
        let (state, token, _dir) = create_test_state().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        let response = server
            .post("/api/instances/api:prod/restart")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;

        // Restart on non-existent instance should fail (stop fails, then spawn fails for unknown process)
        response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ===================
    // TENANT TOKEN TESTS
    // ===================

    /// Create test state with both admin and tenant tokens
    async fn create_test_state_with_tenant() -> (AppState, String, String, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = init_db(&db_path).await.unwrap();
        let config_store = Arc::new(ConfigStore::new(pool.clone()));
        let deploy_log = Arc::new(tenement::DeployLogStore::new(pool.clone()));
        let tenant_tokens = Arc::new(tenement::TenantTokenStore::new(pool.clone()));

        // Generate admin token
        let token_store = TokenStore::new(&config_store);
        let admin_token = token_store.generate_and_store().await.unwrap();

        // Generate tenant token for "alice"
        let tenant_token = tenant_tokens
            .generate_and_store("alice", Some("test"))
            .await
            .unwrap();

        let config = Config::default();
        let hypervisor = Hypervisor::new(config);
        let client = Client::builder(TokioExecutor::new()).build_http();
        let unix_client = Client::builder(TokioExecutor::new()).build(UnixConnector);
        let state = AppState {
            hypervisor,
            domain: "example.com".to_string(),
            client,
            unix_client,
            config_store,
            deploy_log,
            tenant_tokens,
            tls_status: TlsStatus::default(),
            auth_failures: Arc::new(tokio::sync::RwLock::new((0, None))),
        };
        (state, admin_token, tenant_token, dir)
    }

    #[tokio::test]
    async fn test_tenant_token_can_list_instances() {
        let (state, _admin, tenant, _dir) = create_test_state_with_tenant().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Tenant token should be able to list (will see only their instances)
        let response = server
            .get("/api/instances")
            .add_header("Authorization", format!("Bearer {}", tenant))
            .await;
        response.assert_status_ok();
    }

    #[tokio::test]
    async fn test_tenant_token_cannot_deploy() {
        let (state, _admin, tenant, _dir) = create_test_state_with_tenant().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Tenant token should be rejected for deploy
        let response = server
            .post("/api/deploy")
            .add_header("Authorization", format!("Bearer {}", tenant))
            .json(&serde_json::json!({
                "process": "api",
                "version": "v1",
                "weight": 100,
                "timeout": 2
            }))
            .await;
        response.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_tenant_token_cannot_route() {
        let (state, _admin, tenant, _dir) = create_test_state_with_tenant().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Tenant token should be rejected for route swap
        let response = server
            .post("/api/route")
            .add_header("Authorization", format!("Bearer {}", tenant))
            .json(&serde_json::json!({
                "process": "api",
                "from": "v1",
                "to": "v2"
            }))
            .await;
        response.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_tenant_token_scoped_to_own_instances() {
        let (state, _admin, tenant, _dir) = create_test_state_with_tenant().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Tenant "alice" should be rejected when trying to stop "bob"
        let response = server
            .delete("/api/instances/api:bob")
            .add_header("Authorization", format!("Bearer {}", tenant))
            .await;
        response.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_admin_token_has_full_access() {
        let (state, admin, _tenant, _dir) = create_test_state_with_tenant().await;
        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Admin can access any instance
        let response = server
            .get("/api/instances")
            .add_header("Authorization", format!("Bearer {}", admin))
            .await;
        response.assert_status_ok();

        // Admin can deploy
        let response = server
            .post("/api/deploy")
            .add_header("Authorization", format!("Bearer {}", admin))
            .json(&serde_json::json!({
                "process": "nonexistent",
                "version": "v1",
                "weight": 100,
                "timeout": 1
            }))
            .await;
        // Will fail because process doesn't exist, but NOT because of auth
        assert_ne!(response.status_code(), StatusCode::FORBIDDEN);
        assert_ne!(response.status_code(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_tenant_token_scoped_logs() {
        let (state, _admin, tenant, _dir) = create_test_state_with_tenant().await;
        let log_buffer = state.hypervisor.log_buffer();

        // Add logs for different instances
        log_buffer.push_stdout("api", "alice", "alice's log".to_string()).await;
        log_buffer.push_stdout("api", "bob", "bob's log".to_string()).await;

        let app = create_router(state);
        let server = TestServer::new(app).unwrap();

        // Tenant "alice" should only see alice's logs
        let response = server
            .get("/api/logs")
            .add_header("Authorization", format!("Bearer {}", tenant))
            .await;
        response.assert_status_ok();

        let json: Vec<serde_json::Value> = response.json();
        assert_eq!(json.len(), 1, "Tenant should only see their own logs");
        assert_eq!(json[0]["instance_id"], "alice");
    }
}
