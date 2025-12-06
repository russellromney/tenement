//! Process instance management

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Child;
use std::time::Instant;

/// Unique identifier for an instance: "process_name:id"
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstanceId {
    pub process: String,
    pub id: String,
}

impl InstanceId {
    pub fn new(process: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            process: process.into(),
            id: id.into(),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        if parts.len() == 2 {
            Some(Self::new(parts[0], parts[1]))
        } else {
            None
        }
    }
}

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.process, self.id)
    }
}

/// Health status of an instance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Unknown,
    Healthy,
    Degraded,
    Unhealthy,
    Failed,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Unknown => write!(f, "unknown"),
            HealthStatus::Healthy => write!(f, "healthy"),
            HealthStatus::Degraded => write!(f, "degraded"),
            HealthStatus::Unhealthy => write!(f, "unhealthy"),
            HealthStatus::Failed => write!(f, "failed"),
        }
    }
}

/// Running status of an instance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstanceStatus {
    Running,
    Stopped,
    Starting,
    Stopping,
}

impl std::fmt::Display for InstanceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstanceStatus::Running => write!(f, "running"),
            InstanceStatus::Stopped => write!(f, "stopped"),
            InstanceStatus::Starting => write!(f, "starting"),
            InstanceStatus::Stopping => write!(f, "stopping"),
        }
    }
}

/// A running process instance
pub struct Instance {
    pub id: InstanceId,
    pub child: Child,
    pub socket: PathBuf,
    pub started_at: Instant,
    pub restarts: u32,
    pub consecutive_failures: u32,
    pub last_health_check: Option<Instant>,
    pub health_status: HealthStatus,
    pub restart_times: Vec<Instant>,
}

/// Instance info for display (serializable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub id: InstanceId,
    pub socket: PathBuf,
    pub uptime_secs: u64,
    pub restarts: u32,
    pub health: HealthStatus,
    pub status: InstanceStatus,
}

impl Instance {
    pub fn info(&self) -> InstanceInfo {
        InstanceInfo {
            id: self.id.clone(),
            socket: self.socket.clone(),
            uptime_secs: self.started_at.elapsed().as_secs(),
            restarts: self.restarts,
            health: self.health_status,
            status: InstanceStatus::Running,
        }
    }

    pub fn uptime_human(&self) -> String {
        let secs = self.started_at.elapsed().as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m", secs / 60)
        } else if secs < 86400 {
            format!("{}h", secs / 3600)
        } else {
            format!("{}d", secs / 86400)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_id_parse() {
        let id = InstanceId::parse("api:user123").unwrap();
        assert_eq!(id.process, "api");
        assert_eq!(id.id, "user123");
        assert_eq!(id.to_string(), "api:user123");
    }

    #[test]
    fn test_instance_id_parse_invalid() {
        assert!(InstanceId::parse("invalid").is_none());
    }

    #[test]
    fn test_instance_id_with_colons() {
        let id = InstanceId::parse("api:user:with:colons").unwrap();
        assert_eq!(id.process, "api");
        assert_eq!(id.id, "user:with:colons");
    }
}
