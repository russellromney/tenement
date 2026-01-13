//! Configuration parsing for tenement.toml

use crate::runtime::RuntimeType;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Raw config structure for TOML parsing (internal use)
#[derive(Debug, Clone, Deserialize)]
struct RawConfig {
    #[serde(default)]
    settings: Settings,
    #[serde(default)]
    service: HashMap<String, ProcessConfig>,
    #[serde(default)]
    process: HashMap<String, ProcessConfig>,
    #[serde(default)]
    routing: RoutingConfig,
    /// Instances to auto-spawn on boot
    #[serde(default)]
    instances: HashMap<String, Vec<String>>,
}

/// Main configuration structure
///
/// Supports both `[process.X]` (legacy) and `[service.X]` (preferred) section names.
/// Both are merged together during loading - `[process.X]` is an alias for `[service.X]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Global settings
    #[serde(default)]
    pub settings: Settings,

    /// Service definitions (templates)
    /// Both `[service.X]` and `[process.X]` sections are merged here
    #[serde(default)]
    pub service: HashMap<String, ProcessConfig>,

    /// Routing rules
    #[serde(default)]
    pub routing: RoutingConfig,

    /// Instances to auto-spawn on boot
    /// Maps service name to list of instance IDs
    /// Example: { "api": ["prod"], "worker": ["bg-1", "bg-2"] }
    #[serde(default)]
    pub instances: HashMap<String, Vec<String>>,
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

    /// Base delay for exponential backoff (in milliseconds)
    /// Delay = base * 2^(restart_count - 1), capped at backoff_max
    #[serde(default = "default_backoff_base_ms")]
    pub backoff_base_ms: u64,

    /// Maximum backoff delay (in milliseconds)
    #[serde(default = "default_backoff_max_ms")]
    pub backoff_max_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            health_check_interval: default_health_interval(),
            max_restarts: default_max_restarts(),
            restart_window: default_restart_window(),
            backoff_base_ms: default_backoff_base_ms(),
            backoff_max_ms: default_backoff_max_ms(),
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

fn default_backoff_base_ms() -> u64 {
    1000 // 1 second
}

fn default_backoff_max_ms() -> u64 {
    60000 // 60 seconds
}

/// Service template definition (also known as ProcessConfig for backwards compatibility)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    /// Isolation level: "namespace" (default), "process", "firecracker", or "qemu"
    /// Also accepts "runtime" as an alias for backwards compatibility
    #[serde(default, alias = "runtime")]
    pub isolation: RuntimeType,

    /// Command to run (supports {name}, {id}, {data_dir} interpolation)
    pub command: String,

    /// Arguments (optional)
    #[serde(default)]
    pub args: Vec<String>,

    /// Unix socket path pattern (supports {name}, {id})
    /// Used when `port` is not specified.
    #[serde(default = "default_socket")]
    pub socket: String,

    /// TCP port for the service to listen on (alternative to socket)
    /// When specified, the service listens on 127.0.0.1:{port} instead of a Unix socket.
    /// The PORT environment variable is set automatically.
    #[serde(default)]
    pub port: Option<u16>,

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

    /// Idle timeout in seconds before auto-stopping (0 = never stop)
    /// When set, instance will be stopped after this many seconds of inactivity.
    /// Health checks do NOT count as activity - only real requests do.
    #[serde(default)]
    pub idle_timeout: Option<u64>,

    /// Startup timeout in seconds (default: 10)
    /// How long to wait for a process to create its socket before giving up.
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout: u64,

    // --- Resource limits (cgroups v2 on Linux) ---

    /// Memory limit in MB (0 = unlimited)
    /// Applied via cgroups v2 on Linux for process/namespace/sandbox isolation.
    /// For Firecracker/QEMU VMs, this sets the VM memory.
    #[serde(default)]
    pub memory_limit_mb: Option<u32>,

    /// CPU weight (1-10000, default 100)
    /// Higher values get more CPU time relative to other services.
    /// Applied via cgroups v2 cpu.weight on Linux.
    /// 100 = normal priority, 1 = minimum, 10000 = maximum
    #[serde(default)]
    pub cpu_shares: Option<u32>,

    // --- Storage limits ---

    /// Storage quota in MB (None = unlimited)
    /// Soft limit: exceeding quota triggers warnings and metrics but doesn't kill the process.
    #[serde(default)]
    pub storage_quota_mb: Option<u32>,

    /// Keep data directory on stop (default: false)
    /// If false, the instance's data directory is deleted when stopped.
    /// If true, data is preserved for the next spawn.
    #[serde(default)]
    pub storage_persist: bool,

    // --- Firecracker/QEMU-specific fields ---

    /// Path to kernel image (required for firecracker runtime)
    #[serde(default)]
    pub kernel: Option<PathBuf>,

    /// Path to root filesystem image (required for firecracker runtime)
    #[serde(default)]
    pub rootfs: Option<PathBuf>,

    /// Memory in MB for VM (firecracker/qemu only, default 256)
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u32,

    /// Number of vCPUs (firecracker/qemu only)
    #[serde(default = "default_vcpus")]
    pub vcpus: u32,

    /// VSOCK port for guest communication (firecracker only)
    #[serde(default = "default_vsock_port")]
    pub vsock_port: u32,
}

fn default_memory_mb() -> u32 {
    256
}

fn default_vcpus() -> u32 {
    1
}

fn default_vsock_port() -> u32 {
    5000
}

fn default_socket() -> String {
    "/tmp/{name}-{id}.sock".to_string()
}

fn default_restart_policy() -> String {
    "on-failure".to_string()
}

fn default_startup_timeout() -> u64 {
    10
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
            service: HashMap::new(),
            routing: RoutingConfig::default(),
            instances: HashMap::new(),
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
    ///
    /// Supports both `[service.X]` (preferred) and `[process.X]` (legacy) sections.
    /// Both are merged into the `service` field.
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        Self::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))
    }

    /// Parse config from a TOML string
    ///
    /// Supports both `[service.X]` (preferred) and `[process.X]` (legacy) sections.
    pub fn from_str(content: &str) -> Result<Self> {
        let raw: RawConfig = toml::from_str(content)?;

        // Merge process (legacy) and service (preferred) sections
        let mut service = raw.service;
        for (name, config) in raw.process {
            if service.contains_key(&name) {
                anyhow::bail!(
                    "Service '{}' defined in both [service.{}] and [process.{}]. Use only one.",
                    name, name, name
                );
            }
            service.insert(name, config);
        }

        // Validate instances reference defined services
        for (service_name, _instance_ids) in &raw.instances {
            if !service.contains_key(service_name) {
                anyhow::bail!(
                    "Instance references undefined service '{}'. \
                    Define it in [service.{}] first.",
                    service_name, service_name
                );
            }
        }

        Ok(Config {
            settings: raw.settings,
            service,
            routing: raw.routing,
            instances: raw.instances,
        })
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

    /// Get a service config by name
    pub fn get_service(&self, name: &str) -> Option<&ProcessConfig> {
        self.service.get(name)
    }

    /// Get a process config by name (legacy alias for get_service)
    #[deprecated(since = "0.4.0", note = "Use get_service() instead")]
    pub fn get_process(&self, name: &str) -> Option<&ProcessConfig> {
        self.get_service(name)
    }

    /// Get all configured instances to spawn on boot
    /// Returns pairs of (service_name, instance_id)
    pub fn get_instances_to_spawn(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        for (service_name, instance_ids) in &self.instances {
            for instance_id in instance_ids {
                result.push((service_name.clone(), instance_id.clone()));
            }
        }
        result
    }

    /// Check if any instances are configured for auto-spawn
    pub fn has_instances_to_spawn(&self) -> bool {
        self.instances.values().any(|ids| !ids.is_empty())
    }
}

/// Listen address for a service - either a Unix socket path or a TCP address
#[derive(Debug, Clone)]
pub enum ListenAddr {
    /// Unix socket path
    Socket(PathBuf),
    /// TCP address (host:port)
    Tcp(String),
}

impl ListenAddr {
    /// Check if this is a TCP address
    pub fn is_tcp(&self) -> bool {
        matches!(self, ListenAddr::Tcp(_))
    }

    /// Check if this is a Unix socket
    pub fn is_socket(&self) -> bool {
        matches!(self, ListenAddr::Socket(_))
    }

    /// Get the TCP port if this is a TCP address
    pub fn port(&self) -> Option<u16> {
        match self {
            ListenAddr::Tcp(addr) => addr.split(':').last()?.parse().ok(),
            ListenAddr::Socket(_) => None,
        }
    }
}

impl ProcessConfig {
    /// Validate config for the specified isolation level
    pub fn validate(&self, name: &str) -> Result<()> {
        if self.isolation == RuntimeType::Firecracker {
            if self.kernel.is_none() {
                anyhow::bail!(
                    "Service '{}' uses firecracker isolation but 'kernel' is not specified",
                    name
                );
            }
            if self.rootfs.is_none() {
                anyhow::bail!(
                    "Service '{}' uses firecracker isolation but 'rootfs' is not specified",
                    name
                );
            }
        }
        Ok(())
    }

    /// Get the isolation level (preferred name)
    pub fn isolation(&self) -> RuntimeType {
        self.isolation
    }

    /// Get the runtime type (legacy alias for isolation)
    #[deprecated(since = "0.4.0", note = "Use isolation() instead")]
    pub fn runtime(&self) -> RuntimeType {
        self.isolation
    }

    /// Check if this service uses TCP port instead of Unix socket
    pub fn uses_port(&self) -> bool {
        self.port.is_some()
    }

    /// Get the listen address for an instance (socket path or TCP address)
    pub fn listen_addr(&self, name: &str, id: &str) -> ListenAddr {
        if let Some(port) = self.port {
            ListenAddr::Tcp(format!("127.0.0.1:{}", port))
        } else {
            ListenAddr::Socket(self.socket_path(name, id))
        }
    }

    /// Interpolate variables in a string
    /// Supports: {name}, {id}, {data_dir}, {socket}, {port}
    pub fn interpolate(&self, template: &str, name: &str, id: &str, data_dir: &Path) -> String {
        let socket = self.socket_path(name, id);
        let port_str = self.port.map(|p| p.to_string()).unwrap_or_default();
        template
            .replace("{name}", name)
            .replace("{id}", id)
            .replace("{data_dir}", &data_dir.to_string_lossy())
            .replace("{socket}", &socket.to_string_lossy())
            .replace("{port}", &port_str)
    }

    /// Get the socket path for an instance (used for Unix socket mode)
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
    fn test_parse_minimal_config_legacy_process() {
        // Test legacy [process.X] format still works
        let config_str = r#"
[process.api]
command = "./api-server"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert!(config.service.contains_key("api"));
        let api = config.get_service("api").unwrap();
        assert_eq!(api.command, "./api-server");
        assert_eq!(api.socket, "/tmp/{name}-{id}.sock");
    }

    #[test]
    fn test_parse_minimal_config_new_service() {
        // Test new [service.X] format
        let config_str = r#"
[service.api]
command = "./api-server"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert!(config.service.contains_key("api"));
        let api = config.get_service("api").unwrap();
        assert_eq!(api.command, "./api-server");
        assert_eq!(api.socket, "/tmp/{name}-{id}.sock");
    }

    #[test]
    fn test_parse_full_config() {
        let config_str = r#"
[settings]
data_dir = "/data/tenement"
health_check_interval = 30

[service.api]
command = "./api"
args = ["--port", "8080"]
socket = "/tmp/api-{id}.sock"
health = "/health"
restart = "always"

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
LOG_LEVEL = "info"

[routing]
default = "api"

[routing.subdomain]
"api.example.com" = "api"
"*.example.com" = "api"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.settings.data_dir, PathBuf::from("/data/tenement"));
        assert_eq!(config.settings.health_check_interval, 30);

        let api = config.get_service("api").unwrap();
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
[service.api]
command = "./api"
socket = "/tmp/{name}-{id}.sock"

[service.api.env]
DB = "{data_dir}/{id}/app.db"
SOCKET = "{socket}"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

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
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.settings.data_dir, PathBuf::from("/var/lib/tenement"));
        assert_eq!(config.settings.health_check_interval, 10);
        assert_eq!(config.settings.max_restarts, 3);
        assert_eq!(config.settings.restart_window, 300);
    }

    #[test]
    fn test_multiple_services() {
        let config_str = r#"
[service.api]
command = "./api"

[service.worker]
command = "./worker"
socket = "/tmp/worker-{id}.sock"

[service.scheduler]
command = "./scheduler"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.service.len(), 3);
        assert!(config.get_service("api").is_some());
        assert!(config.get_service("worker").is_some());
        assert!(config.get_service("scheduler").is_some());
        assert!(config.get_service("nonexistent").is_none());
    }

    #[test]
    fn test_service_with_workdir() {
        let config_str = r#"
[service.api]
command = "./api"
workdir = "/var/app"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();
        assert_eq!(api.workdir, Some(PathBuf::from("/var/app")));
    }

    #[test]
    fn test_service_restart_policies() {
        let config_str = r#"
[service.always]
command = "./always"
restart = "always"

[service.never]
command = "./never"
restart = "never"

[service.default]
command = "./default"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.get_service("always").unwrap().restart, "always");
        assert_eq!(config.get_service("never").unwrap().restart, "never");
        assert_eq!(config.get_service("default").unwrap().restart, "on-failure");
    }

    #[test]
    fn test_routing_config() {
        let config_str = r#"
[service.api]
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
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.routing.default, Some("api".to_string()));
        assert_eq!(config.routing.subdomain.len(), 2);
        assert_eq!(config.routing.path.len(), 2);
        assert_eq!(config.routing.path.get("/api"), Some(&"api".to_string()));
    }

    #[test]
    fn test_empty_routing() {
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert!(config.routing.default.is_none());
        assert!(config.routing.subdomain.is_empty());
        assert!(config.routing.path.is_empty());
    }

    #[test]
    fn test_command_interpolated() {
        let config_str = r#"
[service.api]
command = "./api --id {id} --name {name}"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();
        let data_dir = PathBuf::from("/data");

        let cmd = api.command_interpolated("api", "user123", &data_dir);
        assert_eq!(cmd, "./api --id user123 --name api");
    }

    #[test]
    fn test_args_interpolated() {
        let config_str = r#"
[service.api]
command = "./api"
args = ["--socket", "{socket}", "--data", "{data_dir}/{id}"]
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();
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
[service.api]
command = "./api"
"#;
        let mut file = std::fs::File::create(&config_path).unwrap();
        file.write_all(config_content.as_bytes()).unwrap();

        let config = Config::load_from_path(&config_path).unwrap();
        assert!(config.get_service("api").is_some());
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
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        let cloned = config.clone();
        assert_eq!(config.service.len(), cloned.service.len());
    }

    #[test]
    fn test_service_config_clone() {
        let config_str = r#"
[service.api]
command = "./api"
health = "/health"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();
        let cloned = api.clone();
        assert_eq!(api.command, cloned.command);
        assert_eq!(api.health, cloned.health);
    }

    #[test]
    fn test_firecracker_config_with_isolation() {
        // Test new 'isolation' field name
        let config_str = r#"
[service.secure]
isolation = "firecracker"
command = "./worker"
kernel = "/var/lib/tenement/vmlinux"
rootfs = "/var/lib/tenement/worker.ext4"
memory_mb = 512
vcpus = 2
vsock_port = 6000
"#;
        let config = Config::from_str(config_str).unwrap();
        let secure = config.get_service("secure").unwrap();

        assert_eq!(secure.isolation, RuntimeType::Firecracker);
        assert_eq!(secure.kernel, Some(PathBuf::from("/var/lib/tenement/vmlinux")));
        assert_eq!(secure.rootfs, Some(PathBuf::from("/var/lib/tenement/worker.ext4")));
        assert_eq!(secure.memory_mb, 512);
        assert_eq!(secure.vcpus, 2);
        assert_eq!(secure.vsock_port, 6000);

        // Validation should pass
        assert!(secure.validate("secure").is_ok());
    }

    #[test]
    fn test_firecracker_config_legacy_runtime() {
        // Test legacy 'runtime' field still works
        let config_str = r#"
[process.secure]
runtime = "firecracker"
command = "./worker"
kernel = "/var/lib/tenement/vmlinux"
rootfs = "/var/lib/tenement/worker.ext4"
"#;
        let config = Config::from_str(config_str).unwrap();
        let secure = config.get_service("secure").unwrap();

        assert_eq!(secure.isolation, RuntimeType::Firecracker);
    }

    #[test]
    fn test_firecracker_defaults() {
        let config_str = r#"
[service.secure]
isolation = "firecracker"
command = "./worker"
kernel = "/vmlinux"
rootfs = "/rootfs.ext4"
"#;
        let config = Config::from_str(config_str).unwrap();
        let secure = config.get_service("secure").unwrap();

        assert_eq!(secure.memory_mb, 256);
        assert_eq!(secure.vcpus, 1);
        assert_eq!(secure.vsock_port, 5000);
    }

    #[test]
    fn test_firecracker_validation_missing_kernel() {
        let config_str = r#"
[service.secure]
isolation = "firecracker"
command = "./worker"
rootfs = "/rootfs.ext4"
"#;
        let config = Config::from_str(config_str).unwrap();
        let secure = config.get_service("secure").unwrap();

        let result = secure.validate("secure");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("kernel"));
    }

    #[test]
    fn test_firecracker_validation_missing_rootfs() {
        let config_str = r#"
[service.secure]
isolation = "firecracker"
command = "./worker"
kernel = "/vmlinux"
"#;
        let config = Config::from_str(config_str).unwrap();
        let secure = config.get_service("secure").unwrap();

        let result = secure.validate("secure");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rootfs"));
    }

    #[test]
    fn test_namespace_isolation_default() {
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        // Default isolation is namespace (not process)
        assert_eq!(api.isolation, RuntimeType::Namespace);
        assert!(api.validate("api").is_ok());
    }

    #[test]
    fn test_explicit_process_isolation() {
        let config_str = r#"
[service.api]
isolation = "process"
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.isolation, RuntimeType::Process);
    }

    #[test]
    fn test_legacy_runtime_field_works() {
        // Test that the legacy 'runtime' field still works
        let config_str = r#"
[process.api]
runtime = "process"
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.isolation, RuntimeType::Process);
    }

    #[test]
    fn test_idle_timeout_config() {
        let config_str = r#"
[service.api]
command = "./api"
idle_timeout = 300
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.idle_timeout, Some(300));
    }

    #[test]
    fn test_idle_timeout_default() {
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.idle_timeout, None);
    }

    #[test]
    fn test_idle_timeout_zero_means_never() {
        let config_str = r#"
[service.api]
command = "./api"
idle_timeout = 0
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        // 0 is valid - means never auto-stop (explicitly disabled)
        assert_eq!(api.idle_timeout, Some(0));
    }

    #[test]
    fn test_startup_timeout_config() {
        let config_str = r#"
[service.api]
command = "./api"
startup_timeout = 30
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.startup_timeout, 30);
    }

    #[test]
    fn test_startup_timeout_default() {
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        // Default is 10 seconds
        assert_eq!(api.startup_timeout, 10);
    }

    #[test]
    fn test_backoff_settings() {
        let config_str = r#"
[settings]
backoff_base_ms = 2000
backoff_max_ms = 120000

[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.settings.backoff_base_ms, 2000);
        assert_eq!(config.settings.backoff_max_ms, 120000);
    }

    #[test]
    fn test_backoff_settings_default() {
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();

        // Default: 1s base, 60s max
        assert_eq!(config.settings.backoff_base_ms, 1000);
        assert_eq!(config.settings.backoff_max_ms, 60000);
    }

    #[test]
    fn test_mixed_service_and_process_sections() {
        // Test that both [service.X] and [process.X] can be used together
        let config_str = r#"
[service.api]
command = "./api"

[process.worker]
command = "./worker"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.service.len(), 2);
        assert!(config.get_service("api").is_some());
        assert!(config.get_service("worker").is_some());
    }

    #[test]
    fn test_duplicate_service_process_fails() {
        // Test that defining the same name in both [service] and [process] fails
        let config_str = r#"
[service.api]
command = "./api"

[process.api]
command = "./api-other"
"#;
        let result = Config::from_str(config_str);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("defined in both"));
    }

    #[test]
    fn test_resource_limits_memory() {
        let config_str = r#"
[service.api]
command = "./api"
memory_limit_mb = 256
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.memory_limit_mb, Some(256));
        assert_eq!(api.cpu_shares, None);
    }

    #[test]
    fn test_resource_limits_cpu() {
        let config_str = r#"
[service.api]
command = "./api"
cpu_shares = 500
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.memory_limit_mb, None);
        assert_eq!(api.cpu_shares, Some(500));
    }

    #[test]
    fn test_resource_limits_both() {
        let config_str = r#"
[service.api]
command = "./api"
memory_limit_mb = 512
cpu_shares = 200
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.memory_limit_mb, Some(512));
        assert_eq!(api.cpu_shares, Some(200));
    }

    #[test]
    fn test_resource_limits_default_none() {
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        // Both should default to None (unlimited)
        assert_eq!(api.memory_limit_mb, None);
        assert_eq!(api.cpu_shares, None);
    }

    // ===================
    // STORAGE QUOTA TESTS
    // ===================

    #[test]
    fn test_storage_quota_config() {
        let config_str = r#"
[service.api]
command = "./api"
storage_quota_mb = 512
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.storage_quota_mb, Some(512));
        assert!(!api.storage_persist); // Default false
    }

    #[test]
    fn test_storage_persist_config() {
        let config_str = r#"
[service.api]
command = "./api"
storage_persist = true
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert!(api.storage_persist);
        assert_eq!(api.storage_quota_mb, None); // Default None
    }

    #[test]
    fn test_storage_quota_and_persist() {
        let config_str = r#"
[service.api]
command = "./api"
storage_quota_mb = 256
storage_persist = true
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.storage_quota_mb, Some(256));
        assert!(api.storage_persist);
    }

    #[test]
    fn test_storage_defaults() {
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        // Both should have defaults
        assert_eq!(api.storage_quota_mb, None);
        assert!(!api.storage_persist);
    }

    #[test]
    fn test_storage_quota_zero() {
        // storage_quota_mb of 0 is valid (means no storage allowed)
        let config_str = r#"
[service.api]
command = "./api"
storage_quota_mb = 0
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.storage_quota_mb, Some(0));
    }

    #[test]
    fn test_storage_quota_large_value() {
        let config_str = r#"
[service.api]
command = "./api"
storage_quota_mb = 102400
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        // 100GB quota
        assert_eq!(api.storage_quota_mb, Some(102400));
    }

    // ===================
    // INSTANCE AUTO-START TESTS
    // ===================

    #[test]
    fn test_instances_section_basic() {
        let config_str = r#"
[service.api]
command = "./api"

[service.worker]
command = "./worker"

[instances]
api = ["prod"]
worker = ["bg-1", "bg-2"]
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.instances.len(), 2);
        assert_eq!(config.instances.get("api"), Some(&vec!["prod".to_string()]));
        assert_eq!(
            config.instances.get("worker"),
            Some(&vec!["bg-1".to_string(), "bg-2".to_string()])
        );
    }

    #[test]
    fn test_instances_section_empty() {
        let config_str = r#"
[service.api]
command = "./api"

[instances]
"#;
        let config = Config::from_str(config_str).unwrap();

        assert!(config.instances.is_empty());
        assert!(!config.has_instances_to_spawn());
    }

    #[test]
    fn test_instances_section_missing() {
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();

        assert!(config.instances.is_empty());
        assert!(!config.has_instances_to_spawn());
    }

    #[test]
    fn test_instances_references_undefined_service_fails() {
        let config_str = r#"
[service.api]
command = "./api"

[instances]
worker = ["bg-1"]
"#;
        let result = Config::from_str(config_str);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("undefined service"));
        assert!(err.contains("worker"));
    }

    #[test]
    fn test_get_instances_to_spawn() {
        let config_str = r#"
[service.api]
command = "./api"

[service.worker]
command = "./worker"

[instances]
api = ["prod", "staging"]
worker = ["bg-1"]
"#;
        let config = Config::from_str(config_str).unwrap();
        let instances = config.get_instances_to_spawn();

        assert_eq!(instances.len(), 3);

        // Check all expected instances are present (order may vary due to HashMap)
        assert!(instances.contains(&("api".to_string(), "prod".to_string())));
        assert!(instances.contains(&("api".to_string(), "staging".to_string())));
        assert!(instances.contains(&("worker".to_string(), "bg-1".to_string())));
    }

    #[test]
    fn test_has_instances_to_spawn() {
        // No instances section
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        assert!(!config.has_instances_to_spawn());

        // Empty instances
        let config_str = r#"
[service.api]
command = "./api"

[instances]
api = []
"#;
        let config = Config::from_str(config_str).unwrap();
        assert!(!config.has_instances_to_spawn());

        // With instances
        let config_str = r#"
[service.api]
command = "./api"

[instances]
api = ["prod"]
"#;
        let config = Config::from_str(config_str).unwrap();
        assert!(config.has_instances_to_spawn());
    }

    #[test]
    fn test_instances_with_single_id() {
        let config_str = r#"
[service.api]
command = "./api"

[instances]
api = ["prod"]
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.instances.len(), 1);
        let instances = config.get_instances_to_spawn();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0], ("api".to_string(), "prod".to_string()));
    }

    #[test]
    fn test_instances_empty_list() {
        let config_str = r#"
[service.api]
command = "./api"

[instances]
api = []
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.instances.len(), 1);
        assert_eq!(config.instances.get("api"), Some(&vec![]));

        let instances = config.get_instances_to_spawn();
        assert!(instances.is_empty());
    }

    #[test]
    fn test_instances_multiple_services_multiple_ids() {
        let config_str = r#"
[service.api]
command = "./api"

[service.web]
command = "./web"

[service.worker]
command = "./worker"

[instances]
api = ["prod"]
web = ["prod", "staging"]
worker = ["bg-1", "bg-2", "bg-3"]
"#;
        let config = Config::from_str(config_str).unwrap();

        assert_eq!(config.instances.len(), 3);

        let instances = config.get_instances_to_spawn();
        assert_eq!(instances.len(), 6); // 1 + 2 + 3

        // Verify all are present
        assert!(instances.contains(&("api".to_string(), "prod".to_string())));
        assert!(instances.contains(&("web".to_string(), "prod".to_string())));
        assert!(instances.contains(&("web".to_string(), "staging".to_string())));
        assert!(instances.contains(&("worker".to_string(), "bg-1".to_string())));
        assert!(instances.contains(&("worker".to_string(), "bg-2".to_string())));
        assert!(instances.contains(&("worker".to_string(), "bg-3".to_string())));
    }

    // ===================
    // TCP PORT CONFIG TESTS
    // ===================

    #[test]
    fn test_port_config() {
        let config_str = r#"
[service.api]
command = "./api"
port = 3000
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.port, Some(3000));
        assert!(api.uses_port());
    }

    #[test]
    fn test_port_default_none() {
        let config_str = r#"
[service.api]
command = "./api"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.port, None);
        assert!(!api.uses_port());
    }

    #[test]
    fn test_socket_with_no_port() {
        let config_str = r#"
[service.api]
command = "./api"
socket = "/tmp/api-{id}.sock"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.port, None);
        assert!(!api.uses_port());
        assert_eq!(api.socket_path("api", "test"), PathBuf::from("/tmp/api-test.sock"));
    }

    #[test]
    fn test_listen_addr_tcp() {
        let config_str = r#"
[service.api]
command = "./api"
port = 8080
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        let addr = api.listen_addr("api", "test");
        assert!(addr.is_tcp());
        assert!(!addr.is_socket());
        assert_eq!(addr.port(), Some(8080));
    }

    #[test]
    fn test_listen_addr_socket() {
        let config_str = r#"
[service.api]
command = "./api"
socket = "/tmp/api-{id}.sock"
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        let addr = api.listen_addr("api", "test");
        assert!(addr.is_socket());
        assert!(!addr.is_tcp());
        assert_eq!(addr.port(), None);
    }

    #[test]
    fn test_interpolate_with_port() {
        let config_str = r#"
[service.api]
command = "./api --port {port}"
port = 3000
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();
        let data_dir = PathBuf::from("/data");

        let cmd = api.command_interpolated("api", "test", &data_dir);
        assert_eq!(cmd, "./api --port 3000");
    }

    #[test]
    fn test_port_with_other_options() {
        let config_str = r#"
[service.api]
command = "./api"
port = 4000
health = "/health"
restart = "always"
idle_timeout = 300
memory_limit_mb = 256
"#;
        let config = Config::from_str(config_str).unwrap();
        let api = config.get_service("api").unwrap();

        assert_eq!(api.port, Some(4000));
        assert_eq!(api.health, Some("/health".to_string()));
        assert_eq!(api.restart, "always");
        assert_eq!(api.idle_timeout, Some(300));
        assert_eq!(api.memory_limit_mb, Some(256));
    }
}
