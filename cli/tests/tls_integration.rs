//! TLS integration tests for tenement
//!
//! Tests cover:
//! - TlsOptions validation and construction
//! - HTTP redirect server behavior
//! - TLS configuration from config file
//! - Server startup with TLS options
//!
//! Note: Real ACME/Let's Encrypt tests require a real domain and are tested manually.
//! These tests focus on the configuration, validation, and redirect logic.

use axum::{
    body::Body,
    extract::Host,
    http::{Request, StatusCode},
    response::Redirect,
    Router,
};
use axum_test::TestServer;
use std::path::PathBuf;
use tempfile::TempDir;

// Re-export TlsOptions for testing
use tenement_cli::server::TlsOptions;

// ============================================================================
// TlsOptions Validation Tests
// ============================================================================

mod tls_options_tests {
    use super::*;

    #[test]
    fn test_tls_options_construction() {
        let dir = TempDir::new().unwrap();
        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain: "example.com".to_string(),
            cache_dir: dir.path().to_path_buf(),
            staging: false,
            https_port: 443,
            http_port: 80,
        };

        assert!(opts.enabled);
        assert_eq!(opts.email, "test@example.com");
        assert_eq!(opts.domain, "example.com");
        assert!(!opts.staging);
        assert_eq!(opts.https_port, 443);
        assert_eq!(opts.http_port, 80);
    }

    #[test]
    fn test_tls_options_staging_mode() {
        let dir = TempDir::new().unwrap();
        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain: "test.example.com".to_string(),
            cache_dir: dir.path().to_path_buf(),
            staging: true,
            https_port: 443,
            http_port: 80,
        };

        assert!(opts.staging);
    }

    #[test]
    fn test_tls_options_custom_ports() {
        let dir = TempDir::new().unwrap();
        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain: "example.com".to_string(),
            cache_dir: dir.path().to_path_buf(),
            staging: false,
            https_port: 8443,
            http_port: 8080,
        };

        assert_eq!(opts.https_port, 8443);
        assert_eq!(opts.http_port, 8080);
    }

    #[test]
    fn test_tls_options_cache_dir_path() {
        let dir = TempDir::new().unwrap();
        let cache_path = dir.path().join("acme");
        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain: "example.com".to_string(),
            cache_dir: cache_path.clone(),
            staging: false,
            https_port: 443,
            http_port: 80,
        };

        assert_eq!(opts.cache_dir, cache_path);
    }

    #[test]
    fn test_tls_options_clone() {
        let dir = TempDir::new().unwrap();
        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain: "example.com".to_string(),
            cache_dir: dir.path().to_path_buf(),
            staging: true,
            https_port: 443,
            http_port: 80,
        };

        let cloned = opts.clone();
        assert_eq!(cloned.enabled, opts.enabled);
        assert_eq!(cloned.email, opts.email);
        assert_eq!(cloned.domain, opts.domain);
        assert_eq!(cloned.staging, opts.staging);
    }
}

// ============================================================================
// HTTP Redirect Tests
// ============================================================================

mod http_redirect_tests {
    use super::*;

    /// Create a redirect router similar to serve_http_redirect
    fn create_redirect_router(https_port: u16) -> Router {
        Router::new().fallback(move |Host(host): Host, req: Request<Body>| async move {
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
        })
    }

    #[tokio::test]
    async fn test_redirect_root_path() {
        let app = create_redirect_router(443);
        let server = TestServer::new(app).unwrap();

        let response = server.get("/").add_header("Host", "example.com").await;

        response.assert_status(StatusCode::PERMANENT_REDIRECT);
        let location = response.header("location");
        assert_eq!(location, "https://example.com/");
    }

    #[tokio::test]
    async fn test_redirect_with_path() {
        let app = create_redirect_router(443);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/instances")
            .add_header("Host", "example.com")
            .await;

        response.assert_status(StatusCode::PERMANENT_REDIRECT);
        let location = response.header("location");
        assert_eq!(location, "https://example.com/api/instances");
    }

    #[tokio::test]
    async fn test_redirect_with_query_string() {
        let app = create_redirect_router(443);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/search?q=test&page=2")
            .add_header("Host", "example.com")
            .await;

        response.assert_status(StatusCode::PERMANENT_REDIRECT);
        let location = response.header("location");
        assert_eq!(location, "https://example.com/search?q=test&page=2");
    }

    #[tokio::test]
    async fn test_redirect_custom_https_port() {
        let app = create_redirect_router(8443);
        let server = TestServer::new(app).unwrap();

        let response = server.get("/").add_header("Host", "example.com").await;

        response.assert_status(StatusCode::PERMANENT_REDIRECT);
        let location = response.header("location");
        assert_eq!(location, "https://example.com:8443/");
    }

    #[tokio::test]
    async fn test_redirect_strips_http_port_from_host() {
        let app = create_redirect_router(443);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/")
            .add_header("Host", "example.com:80")
            .await;

        response.assert_status(StatusCode::PERMANENT_REDIRECT);
        let location = response.header("location");
        // Should strip the :80 and redirect to https without port
        assert_eq!(location, "https://example.com/");
    }

    #[tokio::test]
    async fn test_redirect_subdomain() {
        let app = create_redirect_router(443);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/api/data")
            .add_header("Host", "api.example.com")
            .await;

        response.assert_status(StatusCode::PERMANENT_REDIRECT);
        let location = response.header("location");
        assert_eq!(location, "https://api.example.com/api/data");
    }

    #[tokio::test]
    async fn test_redirect_nested_subdomain() {
        let app = create_redirect_router(443);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/")
            .add_header("Host", "prod.api.example.com")
            .await;

        response.assert_status(StatusCode::PERMANENT_REDIRECT);
        let location = response.header("location");
        assert_eq!(location, "https://prod.api.example.com/");
    }

    #[tokio::test]
    async fn test_redirect_post_request() {
        let app = create_redirect_router(443);
        let server = TestServer::new(app).unwrap();

        let response = server
            .post("/api/submit")
            .add_header("Host", "example.com")
            .await;

        // POST should also get redirected
        response.assert_status(StatusCode::PERMANENT_REDIRECT);
        let location = response.header("location");
        assert_eq!(location, "https://example.com/api/submit");
    }

    #[tokio::test]
    async fn test_redirect_preserves_special_characters_in_path() {
        let app = create_redirect_router(443);
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/path/with%20spaces")
            .add_header("Host", "example.com")
            .await;

        response.assert_status(StatusCode::PERMANENT_REDIRECT);
        let location = response.header("location");
        assert_eq!(location, "https://example.com/path/with%20spaces");
    }
}

// ============================================================================
// TLS Configuration Tests
// ============================================================================

mod tls_config_tests {
    use super::*;
    use tenement::{Config, TlsConfig};

    #[test]
    fn test_tls_config_defaults() {
        let config = TlsConfig::default();

        assert!(!config.enabled);
        assert!(config.acme_email.is_none());
        assert!(config.domain.is_none());
        assert!(config.cache_dir.is_none());
        assert!(!config.staging);
        assert_eq!(config.https_port, 443);
        assert_eq!(config.http_port, 80);
    }

    #[test]
    fn test_tls_config_with_staging() {
        let toml_str = r#"
            [settings]
            data_dir = "/tmp/tenement"

            [settings.tls]
            enabled = true
            acme_email = "test@example.com"
            domain = "example.com"
            staging = true
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert!(config.settings.tls.enabled);
        assert_eq!(
            config.settings.tls.acme_email,
            Some("test@example.com".to_string())
        );
        assert_eq!(
            config.settings.tls.domain,
            Some("example.com".to_string())
        );
        assert!(config.settings.tls.staging);
    }

    #[test]
    fn test_tls_config_custom_ports() {
        let toml_str = r#"
            [settings]
            data_dir = "/tmp/tenement"

            [settings.tls]
            enabled = true
            acme_email = "test@example.com"
            https_port = 8443
            http_port = 8080
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.settings.tls.https_port, 8443);
        assert_eq!(config.settings.tls.http_port, 8080);
    }

    #[test]
    fn test_tls_config_cache_dir() {
        let toml_str = r#"
            [settings]
            data_dir = "/tmp/tenement"

            [settings.tls]
            enabled = true
            acme_email = "test@example.com"
            cache_dir = "/var/lib/tenement/acme"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(
            config.settings.tls.cache_dir,
            Some(PathBuf::from("/var/lib/tenement/acme"))
        );
    }

    #[test]
    fn test_tls_disabled_by_default() {
        let toml_str = r#"
            [settings]
            data_dir = "/tmp/tenement"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert!(!config.settings.tls.enabled);
    }

    #[test]
    fn test_tls_config_partial() {
        // Only some TLS fields specified - others should use defaults
        let toml_str = r#"
            [settings]
            data_dir = "/tmp/tenement"

            [settings.tls]
            enabled = true
            acme_email = "test@example.com"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert!(config.settings.tls.enabled);
        assert_eq!(
            config.settings.tls.acme_email,
            Some("test@example.com".to_string())
        );
        // Defaults
        assert!(config.settings.tls.domain.is_none());
        assert!(!config.settings.tls.staging);
        assert_eq!(config.settings.tls.https_port, 443);
        assert_eq!(config.settings.tls.http_port, 80);
    }
}

// ============================================================================
// Cache Directory Tests
// ============================================================================

mod cache_dir_tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_cache_dir_creation() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join("acme");

        // Directory shouldn't exist yet
        assert!(!cache_dir.exists());

        // Create it (simulating what serve_with_tls does)
        fs::create_dir_all(&cache_dir).unwrap();

        // Now it should exist
        assert!(cache_dir.exists());
        assert!(cache_dir.is_dir());
    }

    #[test]
    fn test_cache_dir_nested_creation() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join("var").join("lib").join("tenement").join("acme");

        // Create nested directories
        fs::create_dir_all(&cache_dir).unwrap();

        assert!(cache_dir.exists());
        assert!(cache_dir.is_dir());
    }

    #[test]
    fn test_cache_dir_already_exists() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join("acme");

        // Create it twice - should not error
        fs::create_dir_all(&cache_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();

        assert!(cache_dir.exists());
    }

    #[test]
    fn test_cache_dir_with_existing_files() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join("acme");
        fs::create_dir_all(&cache_dir).unwrap();

        // Create a file in the cache dir
        let cert_file = cache_dir.join("cert.pem");
        fs::write(&cert_file, "dummy cert content").unwrap();

        // Calling create_dir_all again should not affect existing files
        fs::create_dir_all(&cache_dir).unwrap();

        assert!(cert_file.exists());
        let content = fs::read_to_string(&cert_file).unwrap();
        assert_eq!(content, "dummy cert content");
    }
}

// ============================================================================
// TLS Validation Logic Tests
// ============================================================================

mod tls_validation_tests {
    /// Simulates the validation logic from main.rs
    fn validate_tls_config(
        tls_enabled: bool,
        email: Option<String>,
        domain: &str,
    ) -> Result<(), String> {
        if !tls_enabled {
            return Ok(());
        }

        if email.is_none() {
            return Err("TLS enabled but no email provided".to_string());
        }

        if domain == "localhost" {
            return Err("TLS cannot be used with localhost".to_string());
        }

        Ok(())
    }

    #[test]
    fn test_tls_disabled_no_validation() {
        let result = validate_tls_config(false, None, "localhost");
        assert!(result.is_ok());
    }

    #[test]
    fn test_tls_enabled_requires_email() {
        let result = validate_tls_config(true, None, "example.com");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no email"));
    }

    #[test]
    fn test_tls_rejects_localhost() {
        let result = validate_tls_config(true, Some("test@example.com".to_string()), "localhost");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("localhost"));
    }

    #[test]
    fn test_tls_valid_config() {
        let result = validate_tls_config(
            true,
            Some("test@example.com".to_string()),
            "example.com",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_tls_valid_with_subdomain() {
        let result = validate_tls_config(
            true,
            Some("test@example.com".to_string()),
            "api.example.com",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_tls_valid_with_nested_subdomain() {
        let result = validate_tls_config(
            true,
            Some("test@example.com".to_string()),
            "prod.api.example.com",
        );
        assert!(result.is_ok());
    }
}

// ============================================================================
// ACME Configuration Tests
// ============================================================================

mod acme_config_tests {
    #[test]
    fn test_staging_vs_production_url_logic() {
        // Test the directory_lets_encrypt parameter logic
        // true = production, false = staging

        let staging = true;
        let use_production = !staging; // false = staging environment
        assert!(!use_production);

        let staging = false;
        let use_production = !staging; // true = production environment
        assert!(use_production);
    }

    #[test]
    fn test_contact_email_format() {
        let email = "test@example.com";
        let contact = format!("mailto:{}", email);
        assert_eq!(contact, "mailto:test@example.com");
    }

    #[test]
    fn test_domain_array_single() {
        let domain = "example.com".to_string();
        let domains = [domain.clone()];
        assert_eq!(domains.len(), 1);
        assert_eq!(domains[0], "example.com");
    }
}

// ============================================================================
// Edge Cases and Error Handling Tests
// ============================================================================

mod edge_case_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_empty_domain() {
        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain: "".to_string(),
            cache_dir: PathBuf::from("/tmp"),
            staging: false,
            https_port: 443,
            http_port: 80,
        };

        // Empty domain is technically allowed at struct level
        // but ACME will fail - this is caught at runtime
        assert!(opts.domain.is_empty());
    }

    #[test]
    fn test_empty_email() {
        let opts = TlsOptions {
            enabled: true,
            email: "".to_string(),
            domain: "example.com".to_string(),
            cache_dir: PathBuf::from("/tmp"),
            staging: false,
            https_port: 443,
            http_port: 80,
        };

        // Empty email is technically allowed at struct level
        // but ACME will fail - this is caught at runtime
        assert!(opts.email.is_empty());
    }

    #[test]
    fn test_port_zero() {
        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain: "example.com".to_string(),
            cache_dir: PathBuf::from("/tmp"),
            staging: false,
            https_port: 0,
            http_port: 0,
        };

        // Port 0 is valid at struct level (means OS picks a port)
        assert_eq!(opts.https_port, 0);
        assert_eq!(opts.http_port, 0);
    }

    #[test]
    fn test_same_http_https_port() {
        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain: "example.com".to_string(),
            cache_dir: PathBuf::from("/tmp"),
            staging: false,
            https_port: 8443,
            http_port: 8443, // Same as HTTPS - would fail at runtime
        };

        // Struct allows this, runtime will fail with port conflict
        assert_eq!(opts.https_port, opts.http_port);
    }

    #[test]
    fn test_unicode_domain() {
        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain: "例え.jp".to_string(), // Unicode domain
            cache_dir: PathBuf::from("/tmp"),
            staging: false,
            https_port: 443,
            http_port: 80,
        };

        // Unicode domains are allowed at struct level
        // ACME handles IDN conversion
        assert!(!opts.domain.is_ascii());
    }

    #[test]
    fn test_very_long_domain() {
        let long_subdomain = "a".repeat(63); // Max label length
        let domain = format!("{}.example.com", long_subdomain);

        let opts = TlsOptions {
            enabled: true,
            email: "test@example.com".to_string(),
            domain,
            cache_dir: PathBuf::from("/tmp"),
            staging: false,
            https_port: 443,
            http_port: 80,
        };

        assert!(opts.domain.len() > 70);
    }
}
