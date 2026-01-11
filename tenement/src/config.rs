//! Configuration parsing for tenement.toml

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Global settings
    #[serde(default)]
    pub settings: Settings,

    /// Process definitions (templates)
    #[serde(default)]
    pub process: HashMap<String, ProcessConfig>,

    /// Routing rules
    #[serde(default)]
    pub routing: RoutingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Data directory for instance state
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    /// Health check interval in seconds
    #[serde(default = "default_health_interval")]
    pub health_check_interval: u64,

    /// Max restart attempts within window
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,

    /// Restart window in seconds
    #[serde(default = "default_restart_window")]
    pub restart_window: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            health_check_interval: default_health_interval(),
            max_restarts: default_max_restarts(),
            restart_window: default_restart_window(),
        }
    }
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/tenement")
}

fn default_health_interval() -> u64 {
    10
}

fn default_max_restarts() -> u32 {
    3
}

fn default_restart_window() -> u64 {
    300
}

/// Process template definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    /// Command to run (supports {name}, {id}, {data_dir} interpolation)
    pub command: String,

    /// Arguments (optional)
    #[serde(default)]
    pub args: Vec<String>,

    /// Unix socket path pattern (supports {name}, {id})
    #[serde(default = "default_socket")]
    pub socket: String,

    /// Health check endpoint (e.g., "/health")
    #[serde(default)]
    pub health: Option<String>,

    /// Environment variables (supports {name}, {id}, {data_dir}, {socket})
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Working directory
    #[serde(default)]
    pub workdir: Option<PathBuf>,

    /// Restart policy: "always", "on-failure", "never"
    #[serde(default = "default_restart_policy")]
    pub restart: String,
}

fn default_socket() -> String {
    "/tmp/{name}-{id}.sock".to_string()
}

fn default_restart_policy() -> String {
    "on-failure".to_string()
}

/// Routing configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutingConfig {
    /// Default process to route to
    pub default: Option<String>,

    /// Route by subdomain: "*.example.com" -> "process-name"
    #[serde(default)]
    pub subdomain: HashMap<String, String>,

    /// Route by path prefix: "/api" -> "process-name"
    #[serde(default)]
    pub path: HashMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            process: HashMap::new(),
            routing: RoutingConfig::default(),
        }
    }
}

impl Config {
    /// Load config from tenement.toml in current directory or parents
    pub fn load() -> Result<Self> {
        let config_path = Self::find_config_file()?;
        Self::load_from_path(&config_path)
    }

    /// Load config from a specific path
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    /// Find tenement.toml by walking up from current directory
    fn find_config_file() -> Result<PathBuf> {
        let mut current = std::env::current_dir()?;

        loop {
            let config_path = current.join("tenement.toml");
            if config_path.exists() {
                return Ok(config_path);
            }

            if !current.pop() {
                anyhow::bail!(
                    "No tenement.toml found. Create one with:\n\n\
                    [process.myapp]\n\
                    command = \"./my-app\"\n\
                    socket = \"/tmp/myapp-{{id}}.sock\"\n"
                );
            }
        }
    }

    /// Get a process config by name
    pub fn get_process(&self, name: &str) -> Option<&ProcessConfig> {
        self.process.get(name)
    }
}

impl ProcessConfig {
    /// Interpolate variables in a string
    /// Supports: {name}, {id}, {data_dir}, {socket}
    pub fn interpolate(&self, template: &str, name: &str, id: &str, data_dir: &Path) -> String {
        let socket = self.socket_path(name, id);
        template
            .replace("{name}", name)
            .replace("{id}", id)
            .replace("{data_dir}", &data_dir.to_string_lossy())
            .replace("{socket}", &socket.to_string_lossy())
    }

    /// Get the socket path for an instance
    pub fn socket_path(&self, name: &str, id: &str) -> PathBuf {
        let path = self.socket
            .replace("{name}", name)
            .replace("{id}", id);
        PathBuf::from(path)
    }

    /// Get interpolated command
    pub fn command_interpolated(&self, name: &str, id: &str, data_dir: &Path) -> String {
        self.interpolate(&self.command, name, id, data_dir)
    }

    /// Get interpolated args
    pub fn args_interpolated(&self, name: &str, id: &str, data_dir: &Path) -> Vec<String> {
        self.args
            .iter()
            .map(|arg| self.interpolate(arg, name, id, data_dir))
            .collect()
    }

    /// Get interpolated environment variables
    pub fn env_interpolated(&self, name: &str, id: &str, data_dir: &Path) -> HashMap<String, String> {
        self.env
            .iter()
            .map(|(k, v)| (k.clone(), self.interpolate(v, name, id, data_dir)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let config_str = r#"
[process.api]
command = "./api-server"
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        assert!(config.process.contains_key("api"));
        let api = config.get_process("api").unwrap();
        assert_eq!(api.command, "./api-server");
        assert_eq!(api.socket, "/tmp/{name}-{id}.sock");
    }

    #[test]
    fn test_parse_full_config() {
        let config_str = r#"
[settings]
data_dir = "/data/tenement"
health_check_interval = 30

[process.api]
command = "./api"
args = ["--port", "8080"]
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "always"

[process.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"

[routing]
default = "api"

[routing.subdomain]
"api.example.com" = "api"
"*.example.com" = "api"
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        assert_eq!(config.settings.data_dir, PathBuf::from("/data/tenement"));
        assert_eq!(config.settings.health_check_interval, 30);

        let api = config.get_process("api").unwrap();
        assert_eq!(api.command, "./api");
        assert_eq!(api.args, vec!["--port", "8080"]);
        assert_eq!(api.health, Some("/health".to_string()));
        assert_eq!(api.restart, "always");
        assert_eq!(api.env.get("LOG_LEVEL"), Some(&"info".to_string()));

        assert_eq!(config.routing.default, Some("api".to_string()));
    }

    #[test]
    fn test_interpolation() {
        let config_str = r#"
[process.api]
command = "./api"
socket = "/tmp/{name}-{id}.sock"

[process.api.env]
DB = "{data_dir}/{id}/app.db"
SOCKET = "{socket}"
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        let api = config.get_process("api").unwrap();

        let data_dir = PathBuf::from("/var/lib/tenement");

        let socket = api.socket_path("api", "user123");
        assert_eq!(socket, PathBuf::from("/tmp/api-user123.sock"));

        let env = api.env_interpolated("api", "user123", &data_dir);
        assert_eq!(env.get("DB"), Some(&"/var/lib/tenement/user123/app.db".to_string()));
        assert_eq!(env.get("SOCKET"), Some(&"/tmp/api-user123.sock".to_string()));
    }

    #[test]
    fn test_default_settings() {
        let config_str = r#"
[process.api]
command = "./api"
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        assert_eq!(config.settings.data_dir, PathBuf::from("/var/lib/tenement"));
        assert_eq!(config.settings.health_check_interval, 10);
        assert_eq!(config.settings.max_restarts, 3);
        assert_eq!(config.settings.restart_window, 300);
    }

    #[test]
    fn test_multiple_processes() {
        let config_str = r#"
[process.api]
command = "./api"

[process.worker]
command = "./worker"
socket = "/tmp/worker-{id}.sock"

[process.scheduler]
command = "./scheduler"
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        assert_eq!(config.process.len(), 3);
        assert!(config.get_process("api").is_some());
        assert!(config.get_process("worker").is_some());
        assert!(config.get_process("scheduler").is_some());
        assert!(config.get_process("nonexistent").is_none());
    }

    #[test]
    fn test_process_with_workdir() {
        let config_str = r#"
[process.api]
command = "./api"
workdir = "/var/app"
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        let api = config.get_process("api").unwrap();
        assert_eq!(api.workdir, Some(PathBuf::from("/var/app")));
    }

    #[test]
    fn test_process_restart_policies() {
        let config_str = r#"
[process.always]
command = "./always"
restart = "always"

[process.never]
command = "./never"
restart = "never"

[process.default]
command = "./default"
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        assert_eq!(config.get_process("always").unwrap().restart, "always");
        assert_eq!(config.get_process("never").unwrap().restart, "never");
        assert_eq!(config.get_process("default").unwrap().restart, "on-failure");
    }

    #[test]
    fn test_routing_config() {
        let config_str = r#"
[process.api]
command = "./api"

[routing]
default = "api"

[routing.subdomain]
"api.example.com" = "api"
"*.tenant.example.com" = "api"

[routing.path]
"/api" = "api"
"/health" = "api"
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        assert_eq!(config.routing.default, Some("api".to_string()));
        assert_eq!(config.routing.subdomain.len(), 2);
        assert_eq!(config.routing.path.len(), 2);
        assert_eq!(config.routing.path.get("/api"), Some(&"api".to_string()));
    }

    #[test]
    fn test_empty_routing() {
        let config_str = r#"
[process.api]
command = "./api"
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        assert!(config.routing.default.is_none());
        assert!(config.routing.subdomain.is_empty());
        assert!(config.routing.path.is_empty());
    }

    #[test]
    fn test_command_interpolated() {
        let config_str = r#"
[process.api]
command = "./api --id {id} --name {name}"
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        let api = config.get_process("api").unwrap();
        let data_dir = PathBuf::from("/data");

        let cmd = api.command_interpolated("api", "user123", &data_dir);
        assert_eq!(cmd, "./api --id user123 --name api");
    }

    #[test]
    fn test_args_interpolated() {
        let config_str = r#"
[process.api]
command = "./api"
args = ["--socket", "{socket}", "--data", "{data_dir}/{id}"]
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        let api = config.get_process("api").unwrap();
        let data_dir = PathBuf::from("/data");

        let args = api.args_interpolated("api", "user123", &data_dir);
        assert_eq!(args.len(), 4);
        assert_eq!(args[0], "--socket");
        assert_eq!(args[1], "/tmp/api-user123.sock");
        assert_eq!(args[2], "--data");
        assert_eq!(args[3], "/data/user123");
    }

    #[test]
    fn test_load_from_path() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("tenement.toml");

        let config_content = r#"
[process.api]
command = "./api"
"#;
        let mut file = std::fs::File::create(&config_path).unwrap();
        file.write_all(config_content.as_bytes()).unwrap();

        let config = Config::load_from_path(&config_path).unwrap();
        assert!(config.get_process("api").is_some());
    }

    #[test]
    fn test_load_from_nonexistent_path() {
        let result = Config::load_from_path(std::path::Path::new("/nonexistent/tenement.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_toml() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("tenement.toml");

        let config_content = "this is not valid toml [[[";
        let mut file = std::fs::File::create(&config_path).unwrap();
        file.write_all(config_content.as_bytes()).unwrap();

        let result = Config::load_from_path(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_settings_clone() {
        let settings = Settings::default();
        let cloned = settings.clone();
        assert_eq!(settings.data_dir, cloned.data_dir);
        assert_eq!(settings.health_check_interval, cloned.health_check_interval);
    }

    #[test]
    fn test_config_clone() {
        let config_str = r#"
[process.api]
command = "./api"
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        let cloned = config.clone();
        assert_eq!(config.process.len(), cloned.process.len());
    }

    #[test]
    fn test_process_config_clone() {
        let config_str = r#"
[process.api]
command = "./api"
health = "/health"
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        let api = config.get_process("api").unwrap();
        let cloned = api.clone();
        assert_eq!(api.command, cloned.command);
        assert_eq!(api.health, cloned.health);
    }
}
