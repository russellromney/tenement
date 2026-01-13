//! Auth Integration Tests
//!
//! Comprehensive tests for authentication middleware integration with API endpoints.
//! Part of Session 2 of the E2E Testing Plan.

use axum_test::TestServer;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::sync::Arc;
use tempfile::TempDir;
use tenement::{generate_token, init_db, Config, ConfigStore, Hypervisor, TokenStore};
use tenement_cli::server::{create_router, AppState};

/// Create test state with auth token.
/// Returns (TestServer, token, config_store, temp_dir) - temp_dir must be kept alive during test.
async fn setup_test_server() -> (TestServer, String, Arc<ConfigStore>, TempDir) {
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
        config_store: config_store.clone(),
    };

    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    (server, token, config_store, dir)
}

/// Setup helper that returns just server and token (for simpler tests)
async fn setup_simple() -> (TestServer, String, TempDir) {
    let (server, token, _config_store, dir) = setup_test_server().await;
    (server, token, dir)
}

// =============================================================================
// CORE API ENDPOINT AUTH TESTS
// =============================================================================

#[tokio::test]
async fn test_api_instances_requires_auth() {
    let (server, _token, _dir) = setup_simple().await;
    let response = server.get("/api/instances").await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_api_instances_with_valid_token() {
    let (server, token, _dir) = setup_simple().await;
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_ok();
    let json: Vec<serde_json::Value> = response.json();
    assert!(json.is_empty());
}

#[tokio::test]
async fn test_api_instances_with_invalid_token() {
    let (server, _token, _dir) = setup_simple().await;
    let response = server
        .get("/api/instances")
        .add_header("Authorization", "Bearer invalid_token_12345")
        .await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_api_logs_requires_auth() {
    let (server, _token, _dir) = setup_simple().await;
    let response = server.get("/api/logs").await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_api_logs_with_valid_token() {
    let (server, token, _dir) = setup_simple().await;
    let response = server
        .get("/api/logs")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_ok();
}

#[tokio::test]
async fn test_api_logs_stream_requires_auth() {
    let (server, _token, _dir) = setup_simple().await;
    let response = server.get("/api/logs/stream").await;
    response.assert_status_unauthorized();
}

// =============================================================================
// PUBLIC ENDPOINT TESTS (NO AUTH REQUIRED)
// =============================================================================

#[tokio::test]
async fn test_health_no_auth_required() {
    let (server, _token, _dir) = setup_simple().await;
    let response = server.get("/health").await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn test_metrics_no_auth_required() {
    let (server, _token, _dir) = setup_simple().await;
    let response = server.get("/metrics").await;
    response.assert_status_ok();
    let text = response.text();
    assert!(text.contains("tenement_"));
}

#[tokio::test]
async fn test_dashboard_no_auth_required() {
    let (server, _token, _dir) = setup_simple().await;
    let response = server.get("/").await;
    response.assert_status_ok();
    let text = response.text();
    assert!(text.contains("tenement"));
}

#[tokio::test]
async fn test_public_endpoints_accept_token_too() {
    let (server, token, _dir) = setup_simple().await;

    // Public endpoints should work WITH a token too (not reject it)
    let response = server
        .get("/health")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_ok();

    let response = server
        .get("/metrics")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_ok();

    let response = server
        .get("/")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_ok();
}

#[tokio::test]
async fn test_public_endpoints_ignore_invalid_token() {
    let (server, _token, _dir) = setup_simple().await;

    // Public endpoints should work even with invalid token (they skip auth)
    let response = server
        .get("/health")
        .add_header("Authorization", "Bearer totally_invalid")
        .await;
    response.assert_status_ok();

    let response = server
        .get("/metrics")
        .add_header("Authorization", "Bearer totally_invalid")
        .await;
    response.assert_status_ok();
}

// =============================================================================
// BEARER TOKEN FORMAT TESTS
// =============================================================================

#[tokio::test]
async fn test_token_in_header_format() {
    let (server, token, _dir) = setup_simple().await;

    // Standard Bearer token format should work
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_ok();

    // Token directly without "Bearer " prefix should fail
    let response = server
        .get("/api/instances")
        .add_header("Authorization", token.clone())
        .await;
    response.assert_status_unauthorized();

    // Empty Authorization header should fail
    let response = server
        .get("/api/instances")
        .add_header("Authorization", "")
        .await;
    response.assert_status_unauthorized();

    // "Bearer" without token should fail
    let response = server
        .get("/api/instances")
        .add_header("Authorization", "Bearer ")
        .await;
    response.assert_status_unauthorized();

    // Just "Bearer" (no space, no token) should fail
    let response = server
        .get("/api/instances")
        .add_header("Authorization", "Bearer")
        .await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_token_case_insensitive() {
    let (server, token, _dir) = setup_simple().await;

    // "Bearer" (standard case)
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_ok();

    // "bearer" (lowercase)
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("bearer {}", token))
        .await;
    response.assert_status_ok();

    // "BEARER" (uppercase)
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("BEARER {}", token))
        .await;
    response.assert_status_ok();

    // Mixed case
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("BeArEr {}", token))
        .await;
    response.assert_status_ok();
}

// =============================================================================
// WRONG AUTH SCHEME TESTS
// =============================================================================

#[tokio::test]
async fn test_basic_auth_rejected() {
    let (server, token, _dir) = setup_simple().await;

    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Basic {}", token))
        .await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_digest_auth_rejected() {
    let (server, token, _dir) = setup_simple().await;

    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Digest {}", token))
        .await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_unknown_auth_schemes_rejected() {
    let (server, token, _dir) = setup_simple().await;

    let schemes = ["Token", "JWT", "OAuth", "ApiKey", "Custom", "X-Auth"];
    for scheme in schemes {
        let response = server
            .get("/api/instances")
            .add_header("Authorization", format!("{} {}", scheme, token))
            .await;
        response.assert_status_unauthorized();
    }
}

// =============================================================================
// MALFORMED HEADER TESTS
// =============================================================================

#[tokio::test]
async fn test_malformed_authorization_headers() {
    let (server, _token, _dir) = setup_simple().await;

    // Note: HTTP headers can't contain control chars like \n, \r, \t
    // These are valid HTTP header values but malformed Bearer tokens
    let malformed = [
        "Bearer",                    // No space, no token
        "Bearer  ",                  // Double space, no token
        " Bearer token",             // Leading space
        "Bearer  token",             // Double space before token
        "bearer",                    // Just keyword, no token
        "   ",                       // Just whitespace
        "BearerToken",               // No space between scheme and token
        "Bearer:",                   // Colon instead of space
        "Bearer=token",              // Equals sign
    ];

    for header in malformed {
        let response = server
            .get("/api/instances")
            .add_header("Authorization", header)
            .await;
        response.assert_status_unauthorized();
    }
}

#[tokio::test]
async fn test_token_with_extra_spaces() {
    let (server, token, _dir) = setup_simple().await;

    // Leading space in token (after "Bearer ") - token won't match
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer  {}", token))
        .await;
    response.assert_status_unauthorized();

    // Trailing space in token - token won't match
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {} ", token))
        .await;
    response.assert_status_unauthorized();
}

// =============================================================================
// TOKEN ROTATION/REPLACEMENT TESTS
// =============================================================================

#[tokio::test]
async fn test_token_replacement_invalidates_old() {
    let (server, token1, config_store, _dir) = setup_test_server().await;

    // First token should work
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", token1))
        .await;
    response.assert_status_ok();

    // Generate new token (replaces old one)
    let token_store = TokenStore::new(&config_store);
    let token2 = token_store.generate_and_store().await.unwrap();

    // Old token should now fail
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", token1))
        .await;
    response.assert_status_unauthorized();

    // New token should work
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", token2))
        .await;
    response.assert_status_ok();
}

#[tokio::test]
async fn test_token_cleared_requires_new_token() {
    let (server, token, config_store, _dir) = setup_test_server().await;

    // Token works initially
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_ok();

    // Clear the token
    let token_store = TokenStore::new(&config_store);
    token_store.clear().await.unwrap();

    // Old token should fail
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", token))
        .await;
    response.assert_status_unauthorized();

    // Any token should fail (no valid token exists)
    let response = server
        .get("/api/instances")
        .add_header("Authorization", "Bearer any_random_token")
        .await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_multiple_token_rotations() {
    let (server, _token, config_store, _dir) = setup_test_server().await;
    let token_store = TokenStore::new(&config_store);

    let mut prev_token = String::new();
    for i in 0..5 {
        let new_token = token_store.generate_and_store().await.unwrap();

        // New token works
        let response = server
            .get("/api/instances")
            .add_header("Authorization", format!("Bearer {}", new_token))
            .await;
        response.assert_status_ok();

        // Previous token (if any) should not work
        if i > 0 {
            let response = server
                .get("/api/instances")
                .add_header("Authorization", format!("Bearer {}", prev_token))
                .await;
            response.assert_status_unauthorized();
        }

        prev_token = new_token;
    }
}

// =============================================================================
// SUBDOMAIN ROUTING TESTS
// =============================================================================

#[tokio::test]
async fn test_subdomain_bypasses_auth() {
    let (server, _token, _dir) = setup_simple().await;

    // Requests with subdomain host header bypass auth middleware
    // Returns 404 (process not configured), NOT 401
    let response = server
        .get("/some-path")
        .add_header("Host", "prod.api.example.com")
        .await;
    response.assert_status_not_found();
}

#[tokio::test]
async fn test_subdomain_with_different_patterns() {
    let (server, _token, _dir) = setup_simple().await;

    // Various subdomain patterns should bypass auth (get 404, not 401)
    let subdomains = [
        "user1.app.example.com",
        "staging.web.example.com",
        "123.api.example.com",
        "test-user.service.example.com",
    ];

    for subdomain in subdomains {
        let response = server
            .get("/test")
            .add_header("Host", subdomain)
            .await;
        // Should be 404 (not found), not 401 (unauthorized)
        response.assert_status_not_found();
    }
}

#[tokio::test]
async fn test_root_domain_requires_auth() {
    let (server, _token, _dir) = setup_simple().await;

    // Single-level subdomain is now valid for weighted routing (bypasses auth)
    // api.example.com routes to process "api" via weighted selection
    let response = server
        .get("/api/instances")
        .add_header("Host", "api.example.com")
        .await;
    // Weighted routing to unconfigured process returns 404, not 401
    response.assert_status_not_found();

    // Root domain requires auth for API paths
    let response = server
        .get("/api/instances")
        .add_header("Host", "example.com")
        .await;
    response.assert_status_unauthorized();
}

// =============================================================================
// ALL PROTECTED ENDPOINTS TESTS
// =============================================================================

#[tokio::test]
async fn test_all_api_endpoints_require_auth() {
    let (server, _token, _dir) = setup_simple().await;

    let protected = [
        "/api/instances",
        "/api/logs",
        "/api/logs/stream",
        "/api/logs?process=api",
        "/api/logs?id=prod",
        "/api/logs?level=stdout",
        "/api/logs?limit=10",
        "/api/logs?search=error",
        "/api/logs?process=api&id=prod&limit=5",
    ];

    for endpoint in protected {
        let response = server.get(endpoint).await;
        response.assert_status_unauthorized();
    }
}

#[tokio::test]
async fn test_all_api_endpoints_work_with_auth() {
    let (server, token, _dir) = setup_simple().await;

    let endpoints = [
        "/api/instances",
        "/api/logs",
        "/api/logs?process=api",
        "/api/logs?limit=10",
    ];

    for endpoint in endpoints {
        let response = server
            .get(endpoint)
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();
    }
}

// =============================================================================
// EDGE CASE TOKEN TESTS
// =============================================================================

#[tokio::test]
async fn test_similar_but_wrong_tokens() {
    let (server, token, _dir) = setup_simple().await;

    // Token with one char different
    let mut wrong = token.clone();
    wrong.push('x');
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", wrong))
        .await;
    response.assert_status_unauthorized();

    // Token truncated
    let truncated = &token[..token.len() - 1];
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", truncated))
        .await;
    response.assert_status_unauthorized();

    // Token with prefix
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer x{}", token))
        .await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_empty_and_whitespace_tokens() {
    let (server, _token, _dir) = setup_simple().await;

    // Note: HTTP headers can't contain control chars like \n, \t
    let invalid = [
        "Bearer ",
        "Bearer  ",
        "Bearer   ",
        "Bearer     ",
    ];

    for header in invalid {
        let response = server
            .get("/api/instances")
            .add_header("Authorization", header)
            .await;
        response.assert_status_unauthorized();
    }
}

#[tokio::test]
async fn test_very_long_invalid_token() {
    let (server, _token, _dir) = setup_simple().await;

    // Very long token should be handled gracefully
    let long_token = "x".repeat(10000);
    let response = server
        .get("/api/instances")
        .add_header("Authorization", format!("Bearer {}", long_token))
        .await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn test_token_uniqueness() {
    // Verify generated tokens are unique (security property)
    let mut tokens = std::collections::HashSet::new();
    for _ in 0..20 {
        let token = generate_token();
        assert!(tokens.insert(token), "Token collision detected!");
    }
}

// =============================================================================
// CONCURRENT AUTH TESTS
// =============================================================================

#[tokio::test]
async fn test_rapid_authenticated_requests() {
    let (server, token, _dir) = setup_simple().await;

    // Send many rapid sequential requests to test auth under load
    for _ in 0..50 {
        let response = server
            .get("/api/instances")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();
    }
}

#[tokio::test]
async fn test_rapid_unauthenticated_requests() {
    let (server, _token, _dir) = setup_simple().await;

    // Send many rapid sequential unauthenticated requests
    for _ in 0..50 {
        let response = server.get("/api/instances").await;
        response.assert_status_unauthorized();
    }
}

#[tokio::test]
async fn test_mixed_auth_rapid_requests() {
    let (server, token, _dir) = setup_simple().await;

    // Alternating authenticated and unauthenticated requests
    for i in 0..50 {
        if i % 2 == 0 {
            let response = server
                .get("/api/instances")
                .add_header("Authorization", format!("Bearer {}", token))
                .await;
            response.assert_status_ok();
        } else {
            let response = server.get("/api/instances").await;
            response.assert_status_unauthorized();
        }
    }
}

#[tokio::test]
async fn test_rapid_different_endpoints() {
    let (server, token, _dir) = setup_simple().await;

    // Rapidly hit different endpoints with auth
    for _ in 0..20 {
        let response = server
            .get("/api/instances")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let response = server
            .get("/api/logs")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        response.assert_status_ok();

        let response = server.get("/health").await;
        response.assert_status_ok();

        let response = server.get("/metrics").await;
        response.assert_status_ok();
    }
}

// =============================================================================
// STATIC ASSETS AUTH TESTS
// =============================================================================

#[tokio::test]
async fn test_assets_no_auth_required() {
    let (server, _token, _dir) = setup_simple().await;

    // Dashboard assets should not require auth
    let response = server.get("/assets/index.js").await;
    // May be 200 or 404 depending on if asset exists, but NOT 401
    assert_ne!(response.status_code(), 401);

    let response = server.get("/assets/style.css").await;
    assert_ne!(response.status_code(), 401);
}

// =============================================================================
// SPECIAL CHARACTER TESTS
// =============================================================================

#[tokio::test]
async fn test_token_with_special_chars_in_request() {
    let (server, _token, _dir) = setup_simple().await;

    // Tokens with special characters should be rejected (real tokens are base64)
    let special_tokens = [
        "token with spaces",
        "token\twith\ttabs",
        "token;with;semicolons",
        "token=with=equals",
        "token&with&ampersands",
        "token<with>brackets",
        "token\"with\"quotes",
    ];

    for special in special_tokens {
        let response = server
            .get("/api/instances")
            .add_header("Authorization", format!("Bearer {}", special))
            .await;
        response.assert_status_unauthorized();
    }
}

// =============================================================================
// QUERY PARAM TOKEN REJECTION
// =============================================================================

#[tokio::test]
async fn test_token_in_query_param_rejected() {
    let (server, token, _dir) = setup_simple().await;

    // Token in query param should NOT work (only header auth supported)
    let response = server
        .get(&format!("/api/instances?token={}", token))
        .await;
    response.assert_status_unauthorized();

    let response = server
        .get(&format!("/api/instances?access_token={}", token))
        .await;
    response.assert_status_unauthorized();

    let response = server
        .get(&format!("/api/instances?bearer={}", token))
        .await;
    response.assert_status_unauthorized();
}

// =============================================================================
// NO TOKEN SET TESTS
// =============================================================================

#[tokio::test]
async fn test_no_token_configured() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = init_db(&db_path).await.unwrap();
    let config_store = Arc::new(ConfigStore::new(pool));

    // Don't generate a token - leave it empty
    let config = Config::default();
    let hypervisor = Hypervisor::new(config);
    let client = Client::builder(TokioExecutor::new()).build_http();
    let state = AppState {
        hypervisor,
        domain: "example.com".to_string(),
        client,
        config_store,
    };

    let app = create_router(state);
    let server = TestServer::new(app).unwrap();

    // With no token configured, any token should fail
    let response = server
        .get("/api/instances")
        .add_header("Authorization", "Bearer any_token_here")
        .await;
    response.assert_status_unauthorized();

    // No auth should also fail
    let response = server.get("/api/instances").await;
    response.assert_status_unauthorized();

    // Public endpoints should still work
    let response = server.get("/health").await;
    response.assert_status_ok();
}
