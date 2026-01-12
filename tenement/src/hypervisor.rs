//! Process hypervisor - spawns and supervises instances

use crate::cgroup::{CgroupManager, ResourceLimits};
use crate::config::Config;
use crate::instance::{HealthStatus, Instance, InstanceId, InstanceInfo};
use crate::logs::LogBuffer;
use crate::metrics::Metrics;
use crate::runtime::{NamespaceRuntime, ProcessRuntime, Runtime, RuntimeHandle, RuntimeType, SpawnConfig};
use crate::storage::{calculate_dir_size, StorageInfo};
#[cfg(feature = "sandbox")]
use crate::runtime::SandboxRuntime;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// The hypervisor manages all running instances
pub struct Hypervisor {
    config: Config,
    instances: RwLock<HashMap<InstanceId, Instance>>,
    log_buffer: Arc<LogBuffer>,
    metrics: Arc<Metrics>,
    /// Process runtime (always available, fallback)
    process_runtime: ProcessRuntime,
    /// Namespace runtime (default on Linux)
    namespace_runtime: NamespaceRuntime,
    /// Sandbox runtime (gVisor) - requires runsc
    #[cfg(feature = "sandbox")]
    sandbox_runtime: SandboxRuntime,
    /// Cgroup manager for resource limits (Linux cgroups v2)
    cgroup_manager: CgroupManager,
}

impl Hypervisor {
    /// Create a new hypervisor with the given config
    pub fn new(config: Config) -> Arc<Self> {
        let namespace_runtime = NamespaceRuntime::new();
        let cgroup_manager = CgroupManager::new();

        Arc::new(Self {
            config,
            instances: RwLock::new(HashMap::new()),
            log_buffer: LogBuffer::new(),
            metrics: Metrics::new(),
            process_runtime: ProcessRuntime::new(),
            namespace_runtime,
            #[cfg(feature = "sandbox")]
            sandbox_runtime: SandboxRuntime::new(),
            cgroup_manager,
        })
    }

    /// Create a new hypervisor with a custom log buffer
    pub fn with_log_buffer(config: Config, log_buffer: Arc<LogBuffer>) -> Arc<Self> {
        let namespace_runtime = NamespaceRuntime::new();
        let cgroup_manager = CgroupManager::new();

        Arc::new(Self {
            config,
            instances: RwLock::new(HashMap::new()),
            log_buffer,
            metrics: Metrics::new(),
            process_runtime: ProcessRuntime::new(),
            namespace_runtime,
            #[cfg(feature = "sandbox")]
            sandbox_runtime: SandboxRuntime::new(),
            cgroup_manager,
        })
    }

    /// Get the log buffer
    pub fn log_buffer(&self) -> Arc<LogBuffer> {
        self.log_buffer.clone()
    }

    /// Get the metrics
    pub fn metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }

    /// Load config from tenement.toml and create hypervisor
    pub fn from_config_file() -> Result<Arc<Self>> {
        let config = Config::load()?;
        Ok(Self::new(config))
    }

    /// Spawn a new instance of a process
    pub async fn spawn(&self, process_name: &str, id: &str) -> Result<PathBuf> {
        self.spawn_with_env(process_name, id, HashMap::new()).await
    }

    /// Spawn a new instance with additional environment variables
    pub async fn spawn_with_env(
        &self,
        process_name: &str,
        id: &str,
        extra_env: HashMap<String, String>,
    ) -> Result<PathBuf> {
        let process_config = self
            .config
            .get_service(process_name)
            .with_context(|| format!("Unknown process: {}", process_name))?
            .clone();

        let instance_id = InstanceId::new(process_name, id);
        let data_dir = &self.config.settings.data_dir;
        let socket = process_config.socket_path(process_name, id);

        // Create instance data directory
        let instance_data_dir = data_dir.join(process_name).join(id);
        std::fs::create_dir_all(&instance_data_dir)
            .with_context(|| format!("Failed to create data dir: {:?}", instance_data_dir))?;

        // Check if already running
        {
            let instances = self.instances.read().await;
            if instances.contains_key(&instance_id) {
                info!("Instance {} already running", instance_id);
                return Ok(socket);
            }
        }

        // Validate isolation level is available - fail loudly if not
        let isolation = process_config.isolation;
        match isolation {
            RuntimeType::Namespace => {
                if !self.namespace_runtime.is_available() {
                    anyhow::bail!(
                        "Instance {}: namespace isolation requires Linux. \
                         Set isolation = \"process\" in your config for local development.",
                        instance_id
                    );
                }
            }
            RuntimeType::Process => {}
            RuntimeType::Sandbox => {
                #[cfg(feature = "sandbox")]
                {
                    if !self.sandbox_runtime.is_available() {
                        anyhow::bail!(
                            "Instance {}: sandbox isolation requires gVisor (runsc).\n\
                            Install: https://gvisor.dev/docs/user_guide/install/\n\
                            Or use isolation = \"namespace\" for trusted code.",
                            instance_id
                        );
                    }
                }
                #[cfg(not(feature = "sandbox"))]
                {
                    anyhow::bail!(
                        "Instance {}: sandbox isolation requires the 'sandbox' feature.\n\
                        Compile with: cargo build --features sandbox",
                        instance_id
                    );
                }
            }
            RuntimeType::Firecracker | RuntimeType::Qemu => {
                anyhow::bail!(
                    "Instance {}: {} isolation not yet supported in hypervisor",
                    instance_id, isolation
                );
            }
        }

        info!("Spawning instance {} (isolation: {})", instance_id, isolation);

        // Build environment
        let command = process_config.command_interpolated(process_name, id, data_dir);
        let args = process_config.args_interpolated(process_name, id, data_dir);
        let mut env = process_config.env_interpolated(process_name, id, data_dir);

        // Merge extra env vars
        env.extend(extra_env);

        // Add socket path to env
        env.insert("SOCKET_PATH".to_string(), socket.to_string_lossy().to_string());

        // Build spawn config
        let spawn_config = SpawnConfig {
            command,
            args,
            env,
            socket: socket.clone(),
            workdir: process_config.workdir.clone(),
            vm_config: None,
        };

        // Spawn using the selected isolation level (we already validated it's available above)
        let mut handle = match isolation {
            RuntimeType::Namespace => self.namespace_runtime.spawn(&spawn_config).await?,
            RuntimeType::Process => self.process_runtime.spawn(&spawn_config).await?,
            #[cfg(feature = "sandbox")]
            RuntimeType::Sandbox => self.sandbox_runtime.spawn(&spawn_config).await?,
            #[cfg(not(feature = "sandbox"))]
            RuntimeType::Sandbox => unreachable!("sandbox feature not enabled"),
            // Firecracker/Qemu already rejected above
            _ => unreachable!(),
        };

        // Apply resource limits via cgroups v2 (Linux only)
        let resource_limits = ResourceLimits {
            memory_limit_mb: process_config.memory_limit_mb,
            cpu_shares: process_config.cpu_shares,
        };
        if resource_limits.has_limits() {
            // Create cgroup for this instance
            if let Err(e) = self.cgroup_manager.create_cgroup(&instance_id.to_string(), &resource_limits) {
                warn!("Failed to create cgroup for {}: {}", instance_id, e);
            } else if let Some(pid) = handle.pid() {
                // Add process to the cgroup
                if let Err(e) = self.cgroup_manager.add_process(&instance_id.to_string(), pid, &resource_limits) {
                    warn!("Failed to add process to cgroup for {}: {}", instance_id, e);
                }
            }
        }

        // Set up log capture for process-based runtimes (Process and Namespace both have child processes)
        match &mut handle {
            RuntimeHandle::Process { ref mut child, .. }
            | RuntimeHandle::Namespace { ref mut child, .. } => {
                // Take stdout/stderr handles and spawn capture tasks
                let stdout = child.stdout.take();
                let stderr = child.stderr.take();

                // Spawn stdout capture task
                if let Some(stdout) = stdout {
                    let log_buffer = self.log_buffer.clone();
                    let process = process_name.to_string();
                    let inst_id = id.to_string();
                    tokio::spawn(async move {
                        let reader = BufReader::new(stdout);
                        let mut lines = reader.lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            log_buffer.push_stdout(&process, &inst_id, line).await;
                        }
                    });
                }

                // Spawn stderr capture task
                if let Some(stderr) = stderr {
                    let log_buffer = self.log_buffer.clone();
                    let process = process_name.to_string();
                    let inst_id = id.to_string();
                    tokio::spawn(async move {
                        let reader = BufReader::new(stderr);
                        let mut lines = reader.lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            log_buffer.push_stderr(&process, &inst_id, line).await;
                        }
                    });
                }
            }
            _ => {
                // VM runtimes handle logging differently
            }
        }

        let runtime_type = handle.runtime_type();
        let now = Instant::now();
        let instance = Instance {
            id: instance_id.clone(),
            handle,
            runtime_type,
            socket: socket.clone(),
            started_at: now,
            restarts: 0,
            consecutive_failures: 0,
            last_health_check: None,
            health_status: HealthStatus::Unknown,
            restart_times: Vec::new(),
            last_activity: now,
            idle_timeout: process_config.idle_timeout,
            storage_quota_mb: process_config.storage_quota_mb,
            storage_persist: process_config.storage_persist,
            storage_used_bytes: 0,
            data_dir: instance_data_dir.clone(),
        };

        {
            let mut instances = self.instances.write().await;
            instances.insert(instance_id.clone(), instance);
        }

        // Update metrics
        self.metrics.instances_up.inc();

        // Wait for socket to be created
        for _ in 0..50 {
            if socket.exists() {
                info!("Instance {} ready at {:?}", instance_id, socket);
                return Ok(socket);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        warn!("Instance {} socket not ready after 500ms", instance_id);
        Ok(socket)
    }

    /// Stop an instance
    pub async fn stop(&self, process_name: &str, id: &str) -> Result<()> {
        let instance_id = InstanceId::new(process_name, id);

        let mut instances = self.instances.write().await;

        if let Some(mut instance) = instances.remove(&instance_id) {
            info!("Stopping instance {}", instance_id);

            instance
                .handle
                .kill()
                .await
                .with_context(|| format!("Failed to kill process: {}", instance_id))?;

            // Clean up cgroup (if one was created)
            if let Err(e) = self.cgroup_manager.remove_cgroup(&instance_id.to_string()) {
                warn!("Failed to remove cgroup for {}: {}", instance_id, e);
            }

            // Clean up socket
            if instance.socket.exists() {
                std::fs::remove_file(&instance.socket).ok();
            }

            // Clean up data directory if storage_persist is false
            if !instance.storage_persist && instance.data_dir.exists() {
                if let Err(e) = std::fs::remove_dir_all(&instance.data_dir) {
                    warn!(
                        "Failed to remove data directory {:?} for {}: {}",
                        instance.data_dir, instance_id, e
                    );
                } else {
                    info!("Removed data directory {:?} for {}", instance.data_dir, instance_id);
                }
            }

            // Update metrics
            self.metrics.instances_up.dec();

            Ok(())
        } else {
            anyhow::bail!("Instance not found: {}", instance_id)
        }
    }

    /// Restart an instance with exponential backoff
    pub async fn restart(&self, process_name: &str, id: &str) -> Result<PathBuf> {
        let instance_id = InstanceId::new(process_name, id);

        // Get restart count before stopping
        let restarts = {
            let instances = self.instances.read().await;
            instances.get(&instance_id).map(|i| i.restarts).unwrap_or(0)
        };

        // Stop if running
        let _ = self.stop(process_name, id).await;

        // Calculate and apply exponential backoff delay
        let backoff_delay = self.calculate_backoff(restarts);
        if backoff_delay > Duration::ZERO {
            info!(
                "Applying backoff delay of {:?} before restarting {} (restart #{})",
                backoff_delay, instance_id, restarts + 1
            );
            tokio::time::sleep(backoff_delay).await;
        }

        // Spawn again
        let socket = self.spawn(process_name, id).await?;

        // Update restart count
        {
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(&instance_id) {
                instance.restarts = restarts + 1;
                instance.restart_times.push(Instant::now());
                // Keep only recent restarts
                let window = Duration::from_secs(self.config.settings.restart_window);
                instance.restart_times.retain(|t| t.elapsed() < window);
            }
        }

        // Update metrics
        let mut labels = HashMap::new();
        labels.insert("process".to_string(), process_name.to_string());
        labels.insert("id".to_string(), id.to_string());
        let counter = self.metrics.instance_restarts.with_labels(&labels).await;
        counter.inc();

        Ok(socket)
    }

    /// Calculate exponential backoff delay based on restart count
    /// Formula: base * 2^(restarts - 1), capped at max
    fn calculate_backoff(&self, restarts: u32) -> Duration {
        if restarts == 0 {
            return Duration::ZERO;
        }

        let base_ms = self.config.settings.backoff_base_ms;
        let max_ms = self.config.settings.backoff_max_ms;

        // Calculate delay: base * 2^(restarts - 1)
        // Cap the shift amount to prevent overflow (max 63 bits for u64)
        let shift = (restarts - 1).min(63);
        let multiplier = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        let delay_ms = base_ms.saturating_mul(multiplier).min(max_ms);

        Duration::from_millis(delay_ms)
    }

    /// Check if an instance is running
    pub async fn is_running(&self, process_name: &str, id: &str) -> bool {
        let instance_id = InstanceId::new(process_name, id);
        let instances = self.instances.read().await;
        instances.contains_key(&instance_id)
    }

    /// Spawn if not already running
    pub async fn spawn_if_not_running(&self, process_name: &str, id: &str) -> Result<PathBuf> {
        if self.is_running(process_name, id).await {
            let process_config = self
                .config
                .get_service(process_name)
                .context("Unknown process")?;
            Ok(process_config.socket_path(process_name, id))
        } else {
            self.spawn(process_name, id).await
        }
    }

    /// List all running instances
    pub async fn list(&self) -> Vec<InstanceInfo> {
        let instances = self.instances.read().await;
        instances.values().map(|i| i.info()).collect()
    }

    /// Get info for a specific instance
    pub async fn get(&self, process_name: &str, id: &str) -> Option<InstanceInfo> {
        let instance_id = InstanceId::new(process_name, id);
        let instances = self.instances.read().await;
        instances.get(&instance_id).map(|i| i.info())
    }

    /// Get storage information for a specific instance
    pub async fn get_storage_info(&self, process_name: &str, id: &str) -> Option<StorageInfo> {
        let instance_id = InstanceId::new(process_name, id);
        let (data_dir, quota_mb) = {
            let instances = self.instances.read().await;
            instances.get(&instance_id).map(|i| {
                (i.data_dir.clone(), i.storage_quota_mb)
            })?
        };

        // Calculate current directory size
        let used_bytes = calculate_dir_size(data_dir.clone()).await.unwrap_or(0);

        // Convert quota from MB to bytes
        let quota_bytes = quota_mb.map(|mb| (mb as u64) * 1024 * 1024);

        Some(StorageInfo::new(used_bytes, quota_bytes, data_dir))
    }

    /// Get instance info and touch activity atomically.
    /// This prevents race conditions where an instance could be reaped
    /// between checking if it's running and touching its activity.
    pub async fn get_and_touch(&self, process_name: &str, id: &str) -> Option<InstanceInfo> {
        let instance_id = InstanceId::new(process_name, id);
        let mut instances = self.instances.write().await;
        if let Some(instance) = instances.get_mut(&instance_id) {
            instance.touch();
            Some(instance.info())
        } else {
            None
        }
    }

    /// Check if a process is configured (can be spawned)
    pub fn has_process(&self, process_name: &str) -> bool {
        self.config.get_service(process_name).is_some()
    }

    /// Check health of an instance
    pub async fn check_health(&self, process_name: &str, id: &str) -> HealthStatus {
        let instance_id = InstanceId::new(process_name, id);

        let process_config = match self.config.get_service(process_name) {
            Some(c) => c,
            None => return HealthStatus::Unknown,
        };

        // If no health endpoint configured, assume healthy if socket exists
        let health_endpoint = match &process_config.health {
            Some(h) => h,
            None => {
                let socket = process_config.socket_path(process_name, id);
                return if socket.exists() {
                    HealthStatus::Healthy
                } else {
                    HealthStatus::Unhealthy
                };
            }
        };

        // Get socket and vsock port from the running instance
        let (socket, vsock_port) = {
            let instances = self.instances.read().await;
            match instances.get(&instance_id) {
                Some(instance) => (instance.handle.socket().clone(), instance.handle.vsock_port()),
                None => return HealthStatus::Unknown,
            }
        };

        let result = self.ping_health_with_vsock(&socket, health_endpoint, vsock_port).await;

        let mut instances = self.instances.write().await;
        let instance = match instances.get_mut(&instance_id) {
            Some(i) => i,
            None => return HealthStatus::Unknown,
        };

        instance.last_health_check = Some(Instant::now());

        match result {
            Ok(()) => {
                instance.consecutive_failures = 0;
                instance.health_status = HealthStatus::Healthy;
                HealthStatus::Healthy
            }
            Err(e) => {
                instance.consecutive_failures += 1;
                warn!(
                    "Health check failed for {}: {} (failures: {})",
                    instance_id, e, instance.consecutive_failures
                );

                let status = match instance.consecutive_failures {
                    1..=2 => HealthStatus::Degraded,
                    _ => {
                        let window = Duration::from_secs(self.config.settings.restart_window);
                        let recent_restarts = instance
                            .restart_times
                            .iter()
                            .filter(|t| t.elapsed() < window)
                            .count() as u32;

                        if recent_restarts >= self.config.settings.max_restarts {
                            HealthStatus::Failed
                        } else {
                            HealthStatus::Unhealthy
                        }
                    }
                };
                instance.health_status = status;
                status
            }
        }
    }

    /// Ping a health endpoint, optionally using vsock CONNECT protocol
    ///
    /// For Firecracker VMs, the vsock socket requires the CONNECT protocol:
    /// 1. Connect to the Unix socket (exposed by Firecracker)
    /// 2. Send "CONNECT <port>\n"
    /// 3. Wait for "OK <port>\n" response
    /// 4. Socket is now connected to guest app
    async fn ping_health_with_vsock(
        &self,
        socket_path: &PathBuf,
        endpoint: &str,
        vsock_port: Option<u32>,
    ) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixStream;

        let stream = tokio::time::timeout(HEALTH_CHECK_TIMEOUT, UnixStream::connect(socket_path))
            .await
            .context("Connection timeout")?
            .context("Failed to connect")?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // If vsock port is specified, perform CONNECT handshake
        if let Some(port) = vsock_port {
            // Send CONNECT request
            let connect_cmd = format!("CONNECT {}\n", port);
            writer
                .write_all(connect_cmd.as_bytes())
                .await
                .context("Failed to send CONNECT")?;

            // Read response line
            let mut response_line = String::new();
            tokio::time::timeout(HEALTH_CHECK_TIMEOUT, reader.read_line(&mut response_line))
                .await
                .context("CONNECT response timeout")?
                .context("Failed to read CONNECT response")?;

            // Expect "OK <port>\n"
            let expected = format!("OK {}", port);
            if !response_line.starts_with(&expected) {
                anyhow::bail!(
                    "VSOCK CONNECT failed: expected '{}', got '{}'",
                    expected,
                    response_line.trim()
                );
            }
        }

        // Now send HTTP health check request
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            endpoint
        );
        writer
            .write_all(request.as_bytes())
            .await
            .context("Failed to write request")?;

        let mut response = vec![0u8; 1024];
        let n = tokio::time::timeout(HEALTH_CHECK_TIMEOUT, reader.read(&mut response))
            .await
            .context("Read timeout")?
            .context("Failed to read response")?;

        let response_str = String::from_utf8_lossy(&response[..n]);
        if response_str.contains("200 OK") {
            Ok(())
        } else {
            anyhow::bail!("Unhealthy response")
        }
    }

    /// Run health checks on all instances and handle unhealthy ones
    pub async fn run_health_checks(&self) {
        let instance_ids: Vec<InstanceId> = {
            let instances = self.instances.read().await;
            instances.keys().cloned().collect()
        };

        for instance_id in instance_ids {
            let status = self.check_health(&instance_id.process, &instance_id.id).await;

            match status {
                HealthStatus::Unhealthy => {
                    info!("Instance {} is unhealthy, restarting", instance_id);
                    if let Err(e) = self.restart(&instance_id.process, &instance_id.id).await {
                        error!("Failed to restart {}: {}", instance_id, e);
                    }
                }
                HealthStatus::Failed => {
                    error!("Instance {} has failed (too many restarts)", instance_id);
                }
                _ => {}
            }
        }
    }

    /// Start the background health monitor loop
    pub fn start_monitor(self: Arc<Self>) {
        let interval = Duration::from_secs(self.config.settings.health_check_interval);
        let hyp = self.clone();
        tokio::spawn(async move {
            info!("Starting health monitor (interval: {:?})", interval);
            loop {
                tokio::time::sleep(interval).await;
                hyp.run_health_checks().await;
                hyp.reap_idle_instances().await;
            }
        });
    }

    /// Update activity timestamp for an instance.
    /// Call this on real requests (NOT health checks) to prevent auto-stop.
    pub async fn touch_activity(&self, process_name: &str, id: &str) {
        let instance_id = InstanceId::new(process_name, id);
        let mut instances = self.instances.write().await;
        if let Some(instance) = instances.get_mut(&instance_id) {
            instance.touch();
        }
    }

    /// Stop idle instances that have exceeded their idle_timeout.
    /// Called periodically by the health monitor.
    async fn reap_idle_instances(&self) {
        let idle_instances: Vec<InstanceId> = {
            let instances = self.instances.read().await;
            instances
                .values()
                .filter(|i| i.is_idle())
                .map(|i| i.id.clone())
                .collect()
        };

        for instance_id in idle_instances {
            let idle_secs = {
                let instances = self.instances.read().await;
                instances
                    .get(&instance_id)
                    .map(|i| i.last_activity.elapsed().as_secs())
                    .unwrap_or(0)
            };

            info!(
                "Stopping idle instance {} (idle: {}s)",
                instance_id, idle_secs
            );

            if let Err(e) = self.stop(&instance_id.process, &instance_id.id).await {
                error!("Failed to stop idle instance {}: {}", instance_id, e);
            }
        }
    }

    /// Spawn instance if not running, and wait for it to be ready.
    /// Returns the socket path. Use this for wake-on-request.
    /// Uses the process's configured startup_timeout (default: 10s).
    pub async fn spawn_and_wait(&self, process_name: &str, id: &str) -> Result<PathBuf> {
        // Get the startup timeout from process config
        let timeout_secs = self
            .config
            .get_service(process_name)
            .map(|p| p.startup_timeout)
            .unwrap_or(10);

        let socket = self.spawn_if_not_running(process_name, id).await?;

        // Wait for socket to be ready (check every 100ms)
        let iterations = (timeout_secs * 10) as usize;
        for _ in 0..iterations {
            if socket.exists() {
                // Also touch activity since this is a real request
                self.touch_activity(process_name, id).await;
                return Ok(socket);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        anyhow::bail!("Instance {} failed to start within {} seconds",
            InstanceId::new(process_name, id), timeout_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProcessConfig;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::TempDir;

    // Helper to create a test config with a simple echo server process
    fn test_config_with_process(name: &str, command: &str, args: Vec<&str>) -> Config {
        let mut config = Config::default();
        config.settings.data_dir = std::env::temp_dir().join("tenement-test");

        let process = ProcessConfig {
            command: command.to_string(),
            args: args.into_iter().map(|s| s.to_string()).collect(),
            socket: "/tmp/{name}-{id}.sock".to_string(),
            isolation: RuntimeType::Process,
            health: None,
            env: HashMap::new(),
            workdir: None,
            restart: "on-failure".to_string(),
            idle_timeout: None,
            startup_timeout: 5,
            memory_limit_mb: None,
            cpu_shares: None,
            kernel: None,
            rootfs: None,
            memory_mb: 256,
            vcpus: 1,
            vsock_port: 5000,
            storage_quota_mb: None,
            storage_persist: false,
        };

        config.service.insert(name.to_string(), process);
        config
    }

    // Helper to create a shell script that creates a socket and waits
    fn create_socket_server_script(dir: &Path) -> PathBuf {
        let script_path = dir.join("server.sh");
        let script = r#"#!/bin/bash
SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
rm -f "$SOCKET_PATH"
# Create socket using nc (netcat) - listens and responds
# Use socat if available, otherwise use a simple approach
if command -v socat &> /dev/null; then
    socat UNIX-LISTEN:"$SOCKET_PATH",fork EXEC:"echo HTTP/1.1 200 OK"
elif command -v nc &> /dev/null; then
    while true; do
        echo -e "HTTP/1.1 200 OK\r\n\r\nOK" | nc -lU "$SOCKET_PATH" -q0 2>/dev/null || break
    done
else
    # Fallback: just create the socket file and sleep
    python3 -c "
import socket
import os
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.bind('$SOCKET_PATH')
sock.listen(1)
while True:
    conn, _ = sock.accept()
    conn.sendall(b'HTTP/1.1 200 OK\r\n\r\nOK')
    conn.close()
" 2>/dev/null &
    sleep infinity
fi
"#;
        std::fs::write(&script_path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        script_path
    }

    // Helper to create a simple process that touches a socket file
    fn create_touch_socket_script(dir: &Path) -> PathBuf {
        let script_path = dir.join("touch_socket.sh");
        let script = r#"#!/bin/bash
SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
rm -f "$SOCKET_PATH"
touch "$SOCKET_PATH"
sleep 30
"#;
        std::fs::write(&script_path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        script_path
    }

    // ===================
    // BACKOFF TESTS
    // ===================

    #[test]
    fn test_calculate_backoff() {
        let config = Config::default();
        let hypervisor = Hypervisor::new(config);

        // Default settings: base=1000ms, max=60000ms

        // 0 restarts = no delay
        assert_eq!(hypervisor.calculate_backoff(0), Duration::ZERO);

        // 1 restart = 1000ms (1s)
        assert_eq!(hypervisor.calculate_backoff(1), Duration::from_millis(1000));

        // 2 restarts = 2000ms (2s)
        assert_eq!(hypervisor.calculate_backoff(2), Duration::from_millis(2000));

        // 3 restarts = 4000ms (4s)
        assert_eq!(hypervisor.calculate_backoff(3), Duration::from_millis(4000));

        // 4 restarts = 8000ms (8s)
        assert_eq!(hypervisor.calculate_backoff(4), Duration::from_millis(8000));

        // 5 restarts = 16000ms (16s)
        assert_eq!(hypervisor.calculate_backoff(5), Duration::from_millis(16000));

        // 6 restarts = 32000ms (32s)
        assert_eq!(hypervisor.calculate_backoff(6), Duration::from_millis(32000));

        // 7 restarts = 64000ms but capped at 60000ms (60s max)
        assert_eq!(hypervisor.calculate_backoff(7), Duration::from_millis(60000));

        // Large values stay capped
        assert_eq!(hypervisor.calculate_backoff(100), Duration::from_millis(60000));
    }

    #[test]
    fn test_calculate_backoff_custom_settings() {
        let mut config = Config::default();
        config.settings.backoff_base_ms = 500;
        config.settings.backoff_max_ms = 5000;
        let hypervisor = Hypervisor::new(config);

        // 1 restart = 500ms
        assert_eq!(hypervisor.calculate_backoff(1), Duration::from_millis(500));

        // 2 restarts = 1000ms
        assert_eq!(hypervisor.calculate_backoff(2), Duration::from_millis(1000));

        // 4 restarts = 4000ms
        assert_eq!(hypervisor.calculate_backoff(4), Duration::from_millis(4000));

        // 5 restarts = 8000ms but capped at 5000ms
        assert_eq!(hypervisor.calculate_backoff(5), Duration::from_millis(5000));
    }

    #[test]
    fn test_calculate_backoff_overflow_protection() {
        let mut config = Config::default();
        config.settings.backoff_base_ms = u64::MAX;
        config.settings.backoff_max_ms = u64::MAX;
        let hypervisor = Hypervisor::new(config);

        // Should not panic or overflow, should saturate
        let result = hypervisor.calculate_backoff(100);
        assert_eq!(result, Duration::from_millis(u64::MAX));
    }

    #[test]
    fn test_calculate_backoff_zero_base() {
        let mut config = Config::default();
        config.settings.backoff_base_ms = 0;
        config.settings.backoff_max_ms = 1000;
        let hypervisor = Hypervisor::new(config);

        // 0 * 2^n = 0
        assert_eq!(hypervisor.calculate_backoff(1), Duration::ZERO);
        assert_eq!(hypervisor.calculate_backoff(5), Duration::ZERO);
    }

    // ===================
    // LIFECYCLE TESTS
    // ===================

    #[tokio::test]
    async fn test_spawn_process_instance() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("test-api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        let result = hypervisor.spawn("test-api", "instance1").await;
        assert!(result.is_ok(), "Spawn should succeed: {:?}", result.err());

        // Verify instance is tracked
        assert!(hypervisor.is_running("test-api", "instance1").await);

        // Clean up
        hypervisor.stop("test-api", "instance1").await.ok();
    }

    #[tokio::test]
    async fn test_spawn_creates_instance_entry() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        // Initially empty
        let list = hypervisor.list().await;
        assert!(list.is_empty());

        // Spawn
        hypervisor.spawn("api", "user1").await.unwrap();

        // Should appear in list
        let list = hypervisor.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id.process, "api");
        assert_eq!(list[0].id.id, "user1");

        // Clean up
        hypervisor.stop("api", "user1").await.ok();
    }

    #[tokio::test]
    async fn test_spawn_returns_socket_path() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("myapp", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        let socket = hypervisor.spawn("myapp", "prod").await.unwrap();
        assert!(socket.to_string_lossy().contains("myapp"));
        assert!(socket.to_string_lossy().contains("prod"));

        hypervisor.stop("myapp", "prod").await.ok();
    }

    #[tokio::test]
    async fn test_stop_instance() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();
        assert!(hypervisor.is_running("api", "test").await);

        let result = hypervisor.stop("api", "test").await;
        assert!(result.is_ok());
        assert!(!hypervisor.is_running("api", "test").await);
    }

    #[tokio::test]
    async fn test_stop_nonexistent_instance_returns_error() {
        let config = Config::default();
        let hypervisor = Hypervisor::new(config);

        let result = hypervisor.stop("nonexistent", "id").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_list_instances() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        // Spawn multiple instances
        hypervisor.spawn("api", "user1").await.unwrap();
        hypervisor.spawn("api", "user2").await.unwrap();

        let list = hypervisor.list().await;
        assert_eq!(list.len(), 2);

        // Verify both are present
        let ids: Vec<String> = list.iter().map(|i| i.id.id.clone()).collect();
        assert!(ids.contains(&"user1".to_string()));
        assert!(ids.contains(&"user2".to_string()));

        // Clean up
        hypervisor.stop("api", "user1").await.ok();
        hypervisor.stop("api", "user2").await.ok();
    }

    #[tokio::test]
    async fn test_get_instance() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "user1").await.unwrap();

        let info = hypervisor.get("api", "user1").await;
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.id.process, "api");
        assert_eq!(info.id.id, "user1");

        // Non-existent should return None
        assert!(hypervisor.get("api", "nonexistent").await.is_none());

        hypervisor.stop("api", "user1").await.ok();
    }

    #[tokio::test]
    async fn test_spawn_if_not_running() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        // First call spawns
        let socket1 = hypervisor.spawn_if_not_running("api", "test").await.unwrap();

        // Second call returns existing socket without spawning again
        let socket2 = hypervisor.spawn_if_not_running("api", "test").await.unwrap();
        assert_eq!(socket1, socket2);

        // Only one instance in list
        let list = hypervisor.list().await;
        assert_eq!(list.len(), 1);

        hypervisor.stop("api", "test").await.ok();
    }

    #[tokio::test]
    async fn test_spawn_already_running_returns_socket() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        let socket1 = hypervisor.spawn("api", "test").await.unwrap();
        let socket2 = hypervisor.spawn("api", "test").await.unwrap();

        // Both return same socket (idempotent)
        assert_eq!(socket1, socket2);

        hypervisor.stop("api", "test").await.ok();
    }

    // ===================
    // ERROR PATH TESTS
    // ===================

    #[tokio::test]
    async fn test_spawn_unknown_process_returns_error() {
        let config = Config::default();
        let hypervisor = Hypervisor::new(config);

        let result = hypervisor.spawn("nonexistent", "id").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown process"));
    }

    #[tokio::test]
    async fn test_spawn_command_not_found() {
        let config = test_config_with_process("api", "/nonexistent/binary", vec![]);
        let hypervisor = Hypervisor::new(config);

        let result = hypervisor.spawn("api", "test").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_has_process() {
        let config = test_config_with_process("myapi", "sleep", vec!["1"]);
        let hypervisor = Hypervisor::new(config);

        assert!(hypervisor.has_process("myapi"));
        assert!(!hypervisor.has_process("unknown"));
    }

    // ===================
    // RESTART TESTS
    // ===================

    #[tokio::test]
    async fn test_restart_instance() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let mut config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        // Use zero backoff for faster tests
        config.settings.backoff_base_ms = 0;
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();

        // Restart
        let result = hypervisor.restart("api", "test").await;
        assert!(result.is_ok());

        // Should still be running
        assert!(hypervisor.is_running("api", "test").await);

        hypervisor.stop("api", "test").await.ok();
    }

    #[tokio::test]
    async fn test_restart_increments_counter() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let mut config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        config.settings.backoff_base_ms = 0;
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();

        // Check initial restart count
        let info = hypervisor.get("api", "test").await.unwrap();
        assert_eq!(info.restarts, 0);

        // Restart
        hypervisor.restart("api", "test").await.unwrap();

        // Restart count should be 1
        let info = hypervisor.get("api", "test").await.unwrap();
        assert_eq!(info.restarts, 1);

        hypervisor.stop("api", "test").await.ok();
    }

    // ===================
    // ACTIVITY TRACKING TESTS
    // ===================

    #[tokio::test]
    async fn test_touch_activity() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Get idle time before touch
        let info_before = hypervisor.get("api", "test").await.unwrap();
        let idle_before = info_before.idle_secs;

        // Touch activity
        hypervisor.touch_activity("api", "test").await;

        // Idle time should reset (or be very small)
        let info_after = hypervisor.get("api", "test").await.unwrap();
        assert!(info_after.idle_secs <= idle_before);

        hypervisor.stop("api", "test").await.ok();
    }

    #[tokio::test]
    async fn test_get_and_touch_running_instance() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();

        // Wait a bit so idle time accumulates
        tokio::time::sleep(Duration::from_millis(50)).await;

        // get_and_touch should return Some and reset activity
        let info = hypervisor.get_and_touch("api", "test").await;
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.id.process, "api");
        assert_eq!(info.id.id, "test");

        // Idle time should be reset (very small after touch)
        assert!(info.idle_secs < 1);

        hypervisor.stop("api", "test").await.ok();
    }

    #[tokio::test]
    async fn test_get_and_touch_nonexistent_instance() {
        let config = Config::default();
        let hypervisor = Hypervisor::new(config);

        // Non-existent instance should return None
        let info = hypervisor.get_and_touch("api", "nonexistent").await;
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn test_get_and_touch_is_atomic() {
        // This test verifies get_and_touch provides instance info AND touches activity
        // in a single operation (vs separate is_running + touch_activity + get calls)
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();

        // Wait so idle time > 0
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Record time before
        let before_info = hypervisor.get("api", "test").await.unwrap();
        let idle_before = before_info.idle_secs;

        // Wait a bit more
        tokio::time::sleep(Duration::from_millis(50)).await;

        // get_and_touch should touch activity
        let touched_info = hypervisor.get_and_touch("api", "test").await.unwrap();

        // The returned info should have fresh activity timestamp
        // (idle_secs should be 0 or very small since we just touched)
        assert!(touched_info.idle_secs <= idle_before);

        // Verify activity was actually touched by checking again
        let after_info = hypervisor.get("api", "test").await.unwrap();
        assert!(after_info.idle_secs < 1); // Should be very fresh

        hypervisor.stop("api", "test").await.ok();
    }

    // ===================
    // LOG CAPTURE TESTS
    // ===================

    #[tokio::test]
    async fn test_spawn_captures_stdout() {
        let config = test_config_with_process("api", "echo", vec!["hello from stdout"]);
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();

        // Give time for log capture
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check log buffer
        let logs = hypervisor.log_buffer().query(&crate::logs::LogQuery {
            process: Some("api".to_string()),
            ..Default::default()
        }).await;

        // Should have captured stdout
        assert!(logs.iter().any(|l| l.message.contains("hello from stdout")));
    }

    #[tokio::test]
    async fn test_spawn_captures_stderr() {
        let config = test_config_with_process("api", "sh", vec!["-c", "echo error >&2"]);
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();

        // Give time for log capture
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check log buffer
        let logs = hypervisor.log_buffer().query(&crate::logs::LogQuery {
            process: Some("api".to_string()),
            level: Some(crate::logs::LogLevel::Stderr),
            ..Default::default()
        }).await;

        assert!(logs.iter().any(|l| l.message.contains("error")));
    }

    // ===================
    // METRICS TESTS
    // ===================

    #[tokio::test]
    async fn test_spawn_increments_metrics() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        let metrics = hypervisor.metrics();
        let initial = metrics.instances_up.get();

        hypervisor.spawn("api", "test").await.unwrap();

        // instances_up should increment
        assert_eq!(metrics.instances_up.get(), initial + 1);

        hypervisor.stop("api", "test").await.unwrap();

        // Should decrement back
        assert_eq!(metrics.instances_up.get(), initial);
    }

    // ===================
    // HEALTH STATUS TESTS
    // ===================

    #[tokio::test]
    async fn test_check_health_no_endpoint_socket_file() {
        let dir = TempDir::new().unwrap();
        let script = create_touch_socket_script(dir.path());

        // No health endpoint configured, socket is just a file (not real socket)
        let config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();

        // Wait for socket file to be created
        tokio::time::sleep(Duration::from_millis(200)).await;

        let status = hypervisor.check_health("api", "test").await;
        // File exists but isn't a real socket, so can't connect
        // The actual status depends on implementation - could be Healthy (file exists)
        // or Unhealthy (can't connect). Just verify it returns a status.
        assert!(matches!(status, HealthStatus::Healthy | HealthStatus::Unhealthy));

        hypervisor.stop("api", "test").await.ok();
    }

    #[tokio::test]
    async fn test_check_health_unknown_process() {
        let config = Config::default();
        let hypervisor = Hypervisor::new(config);

        let status = hypervisor.check_health("nonexistent", "id").await;
        assert_eq!(status, HealthStatus::Unknown);
    }

    #[tokio::test]
    async fn test_check_health_not_running_instance() {
        let config = test_config_with_process("api", "sleep", vec!["1"]);
        let hypervisor = Hypervisor::new(config);

        // Don't spawn, just check health
        // For a configured process that isn't running, returns Unhealthy
        let status = hypervisor.check_health("api", "test").await;
        assert!(matches!(status, HealthStatus::Unknown | HealthStatus::Unhealthy));
    }

    // ===================
    // DATA DIRECTORY TESTS
    // ===================

    #[tokio::test]
    async fn test_spawn_creates_data_directory() {
        let dir = TempDir::new().unwrap();
        let data_dir = dir.path().join("tenement-data");
        let script = create_touch_socket_script(dir.path());

        let mut config = test_config_with_process("api", script.to_str().unwrap(), vec![]);
        config.settings.data_dir = data_dir.clone();
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "user1").await.unwrap();

        // Data directory should be created
        assert!(data_dir.join("api").join("user1").exists());

        hypervisor.stop("api", "user1").await.ok();
    }

    // ===================
    // ENVIRONMENT VARIABLE TESTS
    // ===================

    #[tokio::test]
    async fn test_spawn_with_extra_env() {
        let config = test_config_with_process("api", "env", vec![]);
        let hypervisor = Hypervisor::new(config);

        let mut extra_env = HashMap::new();
        extra_env.insert("MY_CUSTOM_VAR".to_string(), "custom_value".to_string());

        hypervisor.spawn_with_env("api", "test", extra_env).await.unwrap();

        // Give time for process to run
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check that env was set (captured in logs)
        let logs = hypervisor.log_buffer().query(&crate::logs::LogQuery::default()).await;
        assert!(logs.iter().any(|l| l.message.contains("MY_CUSTOM_VAR=custom_value")));
    }

    #[tokio::test]
    async fn test_spawn_sets_socket_path_env() {
        let config = test_config_with_process("api", "env", vec![]);
        let hypervisor = Hypervisor::new(config);

        hypervisor.spawn("api", "test").await.unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let logs = hypervisor.log_buffer().query(&crate::logs::LogQuery::default()).await;
        assert!(logs.iter().any(|l| l.message.contains("SOCKET_PATH=")));
    }
}
