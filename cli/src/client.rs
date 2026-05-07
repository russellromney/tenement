//! HTTP client for CLI commands to talk to the running tenement server.
//!
//! All CLI commands except `serve` and `init` use this client
//! to send requests to the running server process.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::api_routes::{
    ApiError, DeployRequest, DeployResponse, RouteRequest, RouteResponse, SpawnRequest,
    SpawnResponse, WeightRequest, WeightResponse,
};

/// Token file name stored in data_dir alongside tenement.db
const TOKEN_FILE: &str = "api_token";

/// HTTP client for the tenement API
pub struct ApiClient {
    server_url: String,
    token: String,
    client: reqwest::Client,
}

impl ApiClient {
    /// Create a new API client with explicit server URL and token
    pub fn new(server_url: &str, token: String) -> Self {
        let server_url = server_url.trim_end_matches('/').to_string();
        Self {
            server_url,
            token,
            client: reqwest::Client::new(),
        }
    }

    /// Create an API client by auto-detecting the token.
    ///
    /// Token resolution order:
    /// 1. Explicit token passed via --token flag
    /// 2. TENEMENT_TOKEN environment variable
    /// 3. Token file at {data_dir}/api_token  (data_dir_override takes precedence over config)
    pub fn from_args(server_url: &str, explicit_token: Option<String>, data_dir_override: Option<&std::path::Path>) -> Result<Self> {
        let token = if let Some(t) = explicit_token {
            t
        } else if let Ok(t) = std::env::var("TENEMENT_TOKEN") {
            t
        } else {
            // Try to read from data_dir/api_token
            let config = tenement::Config::load_with_override(
                data_dir_override.map(|p| p.to_path_buf()),
            )
            .context("Could not load tenement.toml. Are you in a tenement project directory?")?;
            let token_path = config.settings.data_dir.join(TOKEN_FILE);
            std::fs::read_to_string(&token_path).with_context(|| {
                format!(
                    "No API token found. Tried:\n\
                     - --token flag\n\
                     - TENEMENT_TOKEN env var\n\
                     - {}\n\n\
                     Run `ten token-gen` first to create a token.",
                    token_path.display()
                )
            })?
        };

        let token = token.trim().to_string();
        if token.is_empty() {
            anyhow::bail!("API token is empty. Run `ten token-gen` to create a new token.");
        }

        Ok(Self::new(server_url, token))
    }

    // ===================
    // Instance operations
    // ===================

    /// Spawn a new instance
    pub async fn spawn(&self, process: &str, id: &str) -> Result<SpawnResponse> {
        let req = SpawnRequest {
            process: process.to_string(),
            id: id.to_string(),
        };
        self.post("/api/instances/spawn", &req).await
    }

    /// Stop an instance
    pub async fn stop(&self, instance: &str) -> Result<()> {
        let url = format!("{}/api/instances/{}", self.server_url, instance);
        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("Failed to connect to server at {}", self.server_url))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let err = self.parse_error(resp).await;
            anyhow::bail!("{}", err)
        }
    }

    /// Restart an instance
    pub async fn restart(&self, instance: &str) -> Result<SpawnResponse> {
        let url = format!("{}/api/instances/{}/restart", self.server_url, instance);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("Failed to connect to server at {}", self.server_url))?;

        self.handle_response(resp).await
    }

    /// Set traffic weight
    pub async fn set_weight(&self, instance: &str, weight: u8) -> Result<WeightResponse> {
        let url = format!("{}/api/instances/{}/weight", self.server_url, instance);
        let req = WeightRequest { weight };
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.token)
            .json(&req)
            .send()
            .await
            .with_context(|| format!("Failed to connect to server at {}", self.server_url))?;

        self.handle_response(resp).await
    }

    /// Check instance health
    pub async fn health(&self, instance: &str) -> Result<serde_json::Value> {
        let url = format!("{}/api/instances/{}/health", self.server_url, instance);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("Failed to connect to server at {}", self.server_url))?;

        self.handle_response(resp).await
    }

    /// Deploy a new version
    pub async fn deploy(
        &self,
        process: &str,
        version: &str,
        weight: u8,
        timeout: u64,
    ) -> Result<DeployResponse> {
        let req = DeployRequest {
            process: process.to_string(),
            version: version.to_string(),
            weight,
            timeout,
        };

        let url = format!("{}/api/deploy", self.server_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&req)
            .timeout(std::time::Duration::from_secs(timeout + 10))
            .send()
            .await
            .with_context(|| format!("Failed to connect to server at {}", self.server_url))?;

        self.handle_response(resp).await
    }

    /// Atomic traffic swap between versions
    pub async fn route(&self, process: &str, from: &str, to: &str) -> Result<RouteResponse> {
        let req = RouteRequest {
            process: process.to_string(),
            from: from.to_string(),
            to: to.to_string(),
        };
        self.post("/api/route", &req).await
    }

    /// List all running instances
    pub async fn list(&self) -> Result<Vec<serde_json::Value>> {
        self.get("/api/instances").await
    }

    // ===================
    // Log operations
    // ===================

    /// Query logs with filters
    pub async fn query_logs(
        &self,
        process: Option<&str>,
        id: Option<&str>,
        level: Option<&str>,
        search: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        let mut params = vec![format!("limit={}", limit)];
        if let Some(p) = process {
            params.push(format!("process={}", p));
        }
        if let Some(i) = id {
            params.push(format!("id={}", i));
        }
        if let Some(l) = level {
            params.push(format!("level={}", l));
        }
        if let Some(s) = search {
            params.push(format!("search={}", urlencoding::encode(s)));
        }
        let query = params.join("&");
        self.get(&format!("/api/logs?{}", query)).await
    }

    /// Stream logs via SSE (prints to stdout, blocks until interrupted)
    pub async fn stream_logs(
        &self,
        process: Option<&str>,
        id: Option<&str>,
        level: Option<&str>,
    ) -> Result<()> {
        let mut params = Vec::new();
        if let Some(p) = process {
            params.push(format!("process={}", p));
        }
        if let Some(i) = id {
            params.push(format!("id={}", i));
        }
        if let Some(l) = level {
            params.push(format!("level={}", l));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };

        let url = format!("{}/api/logs/stream{}", self.server_url, query);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("Failed to connect to server at {}", self.server_url))?;

        if !resp.status().is_success() {
            let err = self.parse_error(resp).await;
            anyhow::bail!("{}", err);
        }

        // Read SSE stream line by line
        use futures::StreamExt;
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Stream error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE events (separated by double newlines)
            while let Some(pos) = buffer.find("\n\n") {
                let event = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                // Parse SSE data lines
                for line in event.lines() {
                    if let Some(data) = line.strip_prefix("data:") {
                        let data = data.trim();
                        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(data) {
                            let lvl = entry["level"].as_str().unwrap_or("?");
                            let proc = entry["process"].as_str().unwrap_or("?");
                            let inst = entry["instance_id"].as_str().unwrap_or("?");
                            let msg = entry["message"].as_str().unwrap_or("");
                            let level_marker = if lvl == "stderr" { "ERR" } else { "OUT" };
                            println!("[{}] {}:{} {}", level_marker, proc, inst, msg);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    // ===================
    // HTTP helpers
    // ===================

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.server_url, path);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("Failed to connect to server at {}", self.server_url))?;

        self.handle_response(resp).await
    }

    async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.server_url, path);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .with_context(|| format!("Failed to connect to server at {}", self.server_url))?;

        self.handle_response(resp).await
    }

    async fn handle_response<T: DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<T> {
        let status = resp.status();
        if status.is_success() {
            resp.json::<T>()
                .await
                .context("Failed to parse server response")
        } else {
            let err = self.parse_error(resp).await;
            anyhow::bail!("{}", err)
        }
    }

    async fn parse_error(&self, resp: reqwest::Response) -> String {
        let status = resp.status();
        match resp.json::<ApiError>().await {
            Ok(err) => err.error,
            Err(_) => format!("Server returned {}", status),
        }
    }
}

/// Save a token to the data_dir for future CLI use.
/// Called by `ten token-gen` after generating a new token.
pub fn save_token_file(data_dir: &std::path::Path, token: &str) -> Result<()> {
    let token_path = data_dir.join(TOKEN_FILE);

    // Ensure data_dir exists
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("Failed to create data dir: {}", data_dir.display()))?;

    std::fs::write(&token_path, token)
        .with_context(|| format!("Failed to write token file: {}", token_path.display()))?;

    // Set restrictive permissions (owner read/write only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&token_path, perms).with_context(|| {
            format!(
                "Failed to set permissions on token file: {}",
                token_path.display()
            )
        })?;
    }

    Ok(())
}
