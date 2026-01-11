//! Process instance management

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use tokio::process::Child;

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

    #[test]
    fn test_instance_id_new() {
        let id = InstanceId::new("myprocess", "myid");
        assert_eq!(id.process, "myprocess");
        assert_eq!(id.id, "myid");
    }

    #[test]
    fn test_instance_id_equality() {
        let id1 = InstanceId::new("api", "user1");
        let id2 = InstanceId::new("api", "user1");
        let id3 = InstanceId::new("api", "user2");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_instance_id_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(InstanceId::new("api", "user1"));
        set.insert(InstanceId::new("api", "user1")); // duplicate
        set.insert(InstanceId::new("api", "user2"));

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_instance_id_parse_empty_id() {
        let id = InstanceId::parse("api:").unwrap();
        assert_eq!(id.process, "api");
        assert_eq!(id.id, "");
    }

    #[test]
    fn test_health_status_display() {
        assert_eq!(HealthStatus::Unknown.to_string(), "unknown");
        assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
        assert_eq!(HealthStatus::Degraded.to_string(), "degraded");
        assert_eq!(HealthStatus::Unhealthy.to_string(), "unhealthy");
        assert_eq!(HealthStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_instance_status_display() {
        assert_eq!(InstanceStatus::Running.to_string(), "running");
        assert_eq!(InstanceStatus::Stopped.to_string(), "stopped");
        assert_eq!(InstanceStatus::Starting.to_string(), "starting");
        assert_eq!(InstanceStatus::Stopping.to_string(), "stopping");
    }

    #[test]
    fn test_health_status_equality() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_ne!(HealthStatus::Healthy, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_instance_status_equality() {
        assert_eq!(InstanceStatus::Running, InstanceStatus::Running);
        assert_ne!(InstanceStatus::Running, InstanceStatus::Stopped);
    }

    #[test]
    fn test_health_status_clone() {
        let status = HealthStatus::Degraded;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_instance_status_clone() {
        let status = InstanceStatus::Starting;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_instance_id_serialize() {
        let id = InstanceId::new("api", "user123");
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.contains("api"));
        assert!(json.contains("user123"));
    }

    #[test]
    fn test_instance_id_deserialize() {
        let json = r#"{"process":"api","id":"user123"}"#;
        let id: InstanceId = serde_json::from_str(json).unwrap();
        assert_eq!(id.process, "api");
        assert_eq!(id.id, "user123");
    }

    #[test]
    fn test_health_status_serialize() {
        let status = HealthStatus::Healthy;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"healthy\"");
    }

    #[test]
    fn test_health_status_deserialize() {
        let json = "\"unhealthy\"";
        let status: HealthStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_instance_status_serialize() {
        let status = InstanceStatus::Running;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"running\"");
    }

    #[test]
    fn test_instance_status_deserialize() {
        let json = "\"stopped\"";
        let status: InstanceStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status, InstanceStatus::Stopped);
    }
}
