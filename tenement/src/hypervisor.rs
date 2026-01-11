//! Process hypervisor - spawns and supervises instances

use crate::config::Config;
use crate::instance::{HealthStatus, Instance, InstanceId, InstanceInfo};
use crate::logs::LogBuffer;
use crate::metrics::Metrics;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// The hypervisor manages all running instances
pub struct Hypervisor {
    config: Config,
    instances: RwLock<HashMap<InstanceId, Instance>>,
    log_buffer: Arc<LogBuffer>,
    metrics: Arc<Metrics>,
}

impl Hypervisor {
    /// Create a new hypervisor with the given config
    pub fn new(config: Config) -> Arc<Self> {
        Arc::new(Self {
            config,
            instances: RwLock::new(HashMap::new()),
            log_buffer: LogBuffer::new(),
            metrics: Metrics::new(),
        })
    }

    /// Create a new hypervisor with a custom log buffer
    pub fn with_log_buffer(config: Config, log_buffer: Arc<LogBuffer>) -> Arc<Self> {
        Arc::new(Self {
            config,
            instances: RwLock::new(HashMap::new()),
            log_buffer,
            metrics: Metrics::new(),
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
            .get_process(process_name)
            .with_context(|| format!("Unknown process: {}", process_name))?
            .clone();

        let instance_id = InstanceId::new(process_name, id);
        let data_dir = &self.config.settings.data_dir;
        let socket = process_config.socket_path(process_name, id);

        // Create instance data directory
        let instance_data_dir = data_dir.join(process_name).join(id);
        std::fs::create_dir_all(&instance_data_dir)
            .with_context(|| format!("Failed to create data dir: {:?}", instance_data_dir))?;

        // Remove old socket if exists
        if socket.exists() {
            std::fs::remove_file(&socket).ok();
        }

        // Check if already running
        {
            let instances = self.instances.read().await;
            if instances.contains_key(&instance_id) {
                info!("Instance {} already running", instance_id);
                return Ok(socket);
            }
        }

        info!("Spawning instance {}", instance_id);

        // Build command
        let command = process_config.command_interpolated(process_name, id, data_dir);
        let args = process_config.args_interpolated(process_name, id, data_dir);
        let mut env = process_config.env_interpolated(process_name, id, data_dir);

        // Merge extra env vars
        env.extend(extra_env);

        // Add socket path to env
        env.insert("SOCKET_PATH".to_string(), socket.to_string_lossy().to_string());

        let mut cmd = Command::new(&command);
        cmd.args(&args)
            .envs(&env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(workdir) = &process_config.workdir {
            cmd.current_dir(workdir);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn process: {}", command))?;

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

        let instance = Instance {
            id: instance_id.clone(),
            child,
            socket: socket.clone(),
            started_at: Instant::now(),
            restarts: 0,
            consecutive_failures: 0,
            last_health_check: None,
            health_status: HealthStatus::Unknown,
            restart_times: Vec::new(),
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
                .child
                .kill()
                .await
                .with_context(|| format!("Failed to kill process: {}", instance_id))?;

            // Clean up socket
            if instance.socket.exists() {
                std::fs::remove_file(&instance.socket).ok();
            }

            // Update metrics
            self.metrics.instances_up.dec();

            Ok(())
        } else {
            anyhow::bail!("Instance not found: {}", instance_id)
        }
    }

    /// Restart an instance
    pub async fn restart(&self, process_name: &str, id: &str) -> Result<PathBuf> {
        let instance_id = InstanceId::new(process_name, id);

        // Get restart count before stopping
        let restarts = {
            let instances = self.instances.read().await;
            instances.get(&instance_id).map(|i| i.restarts).unwrap_or(0)
        };

        // Stop if running
        let _ = self.stop(process_name, id).await;

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
                .get_process(process_name)
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

    /// Check health of an instance
    pub async fn check_health(&self, process_name: &str, id: &str) -> HealthStatus {
        let instance_id = InstanceId::new(process_name, id);

        let process_config = match self.config.get_process(process_name) {
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

        let socket = process_config.socket_path(process_name, id);
        let result = self.ping_health(&socket, health_endpoint).await;

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

    /// Ping a health endpoint via Unix socket
    async fn ping_health(&self, socket_path: &PathBuf, endpoint: &str) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::UnixStream;

        let stream = tokio::time::timeout(HEALTH_CHECK_TIMEOUT, UnixStream::connect(socket_path))
            .await
            .context("Connection timeout")?
            .context("Failed to connect")?;

        let (mut reader, mut writer) = stream.into_split();

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            endpoint
        );
        writer
            .write_all(request.as_bytes())
            .await
            .context("Failed to write")?;

        let mut response = vec![0u8; 1024];
        let n = tokio::time::timeout(HEALTH_CHECK_TIMEOUT, reader.read(&mut response))
            .await
            .context("Read timeout")?
            .context("Failed to read")?;

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
        tokio::spawn(async move {
            info!("Starting health monitor (interval: {:?})", interval);
            loop {
                tokio::time::sleep(interval).await;
                self.run_health_checks().await;
            }
        });
    }
}
