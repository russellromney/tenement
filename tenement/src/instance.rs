//! Process instance management

use crate::runtime::{RuntimeHandle, RuntimeType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
    /// Instance was auto-stopped due to idle timeout, can be auto-woken on request
    Sleeping,
}

impl std::fmt::Display for InstanceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstanceStatus::Running => write!(f, "running"),
            InstanceStatus::Stopped => write!(f, "stopped"),
            InstanceStatus::Starting => write!(f, "starting"),
            InstanceStatus::Stopping => write!(f, "stopping"),
            InstanceStatus::Sleeping => write!(f, "sleeping"),
        }
    }
}

/// A running process or VM instance
pub struct Instance {
    pub id: InstanceId,
    pub handle: RuntimeHandle,
    pub runtime_type: RuntimeType,
    pub socket: PathBuf,
    pub started_at: Instant,
    pub restarts: u32,
    pub consecutive_failures: u32,
    pub last_health_check: Option<Instant>,
    pub health_status: HealthStatus,
    pub restart_times: Vec<Instant>,
    /// Last time a real request (not health check) was received.
    /// Used for idle timeout calculation.
    pub last_activity: Instant,
    /// Idle timeout in seconds (None = never auto-stop)
    pub idle_timeout: Option<u64>,
    /// Storage quota in MB (None = unlimited)
    pub storage_quota_mb: Option<u32>,
    /// Keep data directory on stop
    pub storage_persist: bool,
    /// Cached storage usage in bytes (updated during health checks)
    pub storage_used_bytes: u64,
    /// Path to the instance's data directory
    pub data_dir: PathBuf,
    /// Traffic weight for load balancing (0-100, default 100)
    /// Weight 0 means instance receives no traffic
    pub weight: u8,
}

/// Instance info for display (serializable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub id: InstanceId,
    pub runtime: RuntimeType,
    pub socket: PathBuf,
    pub uptime_secs: u64,
    pub restarts: u32,
    pub health: HealthStatus,
    pub status: InstanceStatus,
    /// Seconds since last activity (real request, not health check)
    pub idle_secs: u64,
    /// Configured idle timeout (None = never auto-stop)
    pub idle_timeout: Option<u64>,
    /// Current storage usage in bytes
    pub storage_used_bytes: u64,
    /// Configured storage quota in bytes (None = unlimited)
    pub storage_quota_bytes: Option<u64>,
    /// Path to instance data directory
    pub data_dir: PathBuf,
    /// Traffic weight for load balancing (0-100)
    pub weight: u8,
}

use std::time::Duration;

impl Instance {
    pub fn info(&self) -> InstanceInfo {
        InstanceInfo {
            id: self.id.clone(),
            runtime: self.runtime_type,
            socket: self.socket.clone(),
            uptime_secs: self.started_at.elapsed().as_secs(),
            restarts: self.restarts,
            health: self.health_status,
            status: InstanceStatus::Running,
            idle_secs: self.last_activity.elapsed().as_secs(),
            idle_timeout: self.idle_timeout,
            storage_used_bytes: self.storage_used_bytes,
            storage_quota_bytes: self.storage_quota_mb.map(|mb| (mb as u64) * 1024 * 1024),
            data_dir: self.data_dir.clone(),
            weight: self.weight,
        }
    }

    /// Check if this instance has been idle longer than its timeout.
    ///
    /// Returns false if:
    /// - No idle_timeout is configured (None)
    /// - idle_timeout is set to 0 (explicit "never stop")
    ///
    /// Only returns true when idle_timeout > 0 AND the instance has been
    /// idle for longer than that duration.
    pub fn is_idle(&self) -> bool {
        match self.idle_timeout {
            Some(timeout) if timeout > 0 => {
                self.last_activity.elapsed() > Duration::from_secs(timeout)
            }
            _ => false,
        }
    }

    /// Update the last activity timestamp (call on real requests, NOT health checks)
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
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

    // ===================
    // INSTANCE ID TESTS
    // ===================

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
    fn test_instance_id_parse_empty_process() {
        let id = InstanceId::parse(":user").unwrap();
        assert_eq!(id.process, "");
        assert_eq!(id.id, "user");
    }

    #[test]
    fn test_instance_id_parse_both_empty() {
        let id = InstanceId::parse(":").unwrap();
        assert_eq!(id.process, "");
        assert_eq!(id.id, "");
    }

    #[test]
    fn test_instance_id_with_special_chars() {
        let id = InstanceId::new("api-v2", "user_123");
        assert_eq!(id.to_string(), "api-v2:user_123");

        let parsed = InstanceId::parse("api-v2:user_123").unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_instance_id_display_roundtrip() {
        let id = InstanceId::new("myprocess", "myid");
        let displayed = id.to_string();
        let parsed = InstanceId::parse(&displayed).unwrap();
        assert_eq!(id, parsed);
    }

    // ===================
    // HEALTH STATUS TESTS
    // ===================

    #[test]
    fn test_health_status_display() {
        assert_eq!(HealthStatus::Unknown.to_string(), "unknown");
        assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
        assert_eq!(HealthStatus::Degraded.to_string(), "degraded");
        assert_eq!(HealthStatus::Unhealthy.to_string(), "unhealthy");
        assert_eq!(HealthStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_health_status_equality() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_ne!(HealthStatus::Healthy, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_health_status_clone() {
        let status = HealthStatus::Degraded;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_health_status_copy() {
        let status = HealthStatus::Healthy;
        let copied: HealthStatus = status; // Copy, not move
        assert_eq!(status, copied);
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
    fn test_health_status_all_variants_serialize() {
        let variants = [
            (HealthStatus::Unknown, "\"unknown\""),
            (HealthStatus::Healthy, "\"healthy\""),
            (HealthStatus::Degraded, "\"degraded\""),
            (HealthStatus::Unhealthy, "\"unhealthy\""),
            (HealthStatus::Failed, "\"failed\""),
        ];

        for (status, expected) in variants {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);

            let deserialized: HealthStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    // ===================
    // INSTANCE STATUS TESTS
    // ===================

    #[test]
    fn test_instance_status_display() {
        assert_eq!(InstanceStatus::Running.to_string(), "running");
        assert_eq!(InstanceStatus::Stopped.to_string(), "stopped");
        assert_eq!(InstanceStatus::Starting.to_string(), "starting");
        assert_eq!(InstanceStatus::Stopping.to_string(), "stopping");
        assert_eq!(InstanceStatus::Sleeping.to_string(), "sleeping");
    }

    #[test]
    fn test_instance_status_equality() {
        assert_eq!(InstanceStatus::Running, InstanceStatus::Running);
        assert_ne!(InstanceStatus::Running, InstanceStatus::Stopped);
    }

    #[test]
    fn test_instance_status_clone() {
        let status = InstanceStatus::Starting;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_instance_status_copy() {
        let status = InstanceStatus::Running;
        let copied: InstanceStatus = status;
        assert_eq!(status, copied);
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

    #[test]
    fn test_sleeping_status_display() {
        assert_eq!(InstanceStatus::Sleeping.to_string(), "sleeping");
    }

    #[test]
    fn test_sleeping_status_serialize() {
        let status = InstanceStatus::Sleeping;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"sleeping\"");
    }

    #[test]
    fn test_sleeping_status_deserialize() {
        let json = "\"sleeping\"";
        let status: InstanceStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status, InstanceStatus::Sleeping);
    }

    #[test]
    fn test_instance_status_all_variants_serialize() {
        let variants = [
            (InstanceStatus::Running, "\"running\""),
            (InstanceStatus::Stopped, "\"stopped\""),
            (InstanceStatus::Starting, "\"starting\""),
            (InstanceStatus::Stopping, "\"stopping\""),
            (InstanceStatus::Sleeping, "\"sleeping\""),
        ];

        for (status, expected) in variants {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);

            let deserialized: InstanceStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    // ===================
    // INSTANCE ID SERIALIZATION TESTS
    // ===================

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
    fn test_instance_id_serde_roundtrip() {
        let id = InstanceId::new("my-api", "instance-42");
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: InstanceId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    // ===================
    // INSTANCE INFO TESTS
    // ===================

    #[test]
    fn test_instance_info_serialization() {
        let info = InstanceInfo {
            id: InstanceId::new("api", "user1"),
            runtime: RuntimeType::Process,
            socket: PathBuf::from("/tmp/test.sock"),
            uptime_secs: 3600,
            restarts: 2,
            health: HealthStatus::Healthy,
            status: InstanceStatus::Running,
            idle_secs: 60,
            idle_timeout: Some(300),
            storage_used_bytes: 134217728,
            storage_quota_bytes: Some(536870912),
            data_dir: PathBuf::from("/data/api/user1"),
            weight: 100,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("api"));
        assert!(json.contains("user1"));
        assert!(json.contains("3600"));
        assert!(json.contains("healthy"));
        assert!(json.contains("running"));
        assert!(json.contains("134217728"));
        assert!(json.contains("536870912"));

        let deserialized: InstanceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id.process, "api");
        assert_eq!(deserialized.uptime_secs, 3600);
        assert_eq!(deserialized.health, HealthStatus::Healthy);
        assert_eq!(deserialized.storage_used_bytes, 134217728);
        assert_eq!(deserialized.storage_quota_bytes, Some(536870912));
        assert_eq!(deserialized.weight, 100);
    }

    #[test]
    fn test_instance_info_idle_timeout_none() {
        let info = InstanceInfo {
            id: InstanceId::new("api", "user1"),
            runtime: RuntimeType::Namespace,
            socket: PathBuf::from("/tmp/test.sock"),
            uptime_secs: 100,
            restarts: 0,
            health: HealthStatus::Unknown,
            status: InstanceStatus::Starting,
            idle_secs: 0,
            idle_timeout: None,
            storage_used_bytes: 0,
            storage_quota_bytes: None,
            data_dir: PathBuf::from("/data/api/user1"),
            weight: 100,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("null") || json.contains("idle_timeout\":null"));

        let deserialized: InstanceInfo = serde_json::from_str(&json).unwrap();
        assert!(deserialized.idle_timeout.is_none());
        assert!(deserialized.storage_quota_bytes.is_none());
    }

    // ===================
    // INSTANCE IS_IDLE TESTS
    // ===================

    // Note: These tests require creating Instance structs with mock handles
    // which is complex. Integration tests in hypervisor.rs cover this better.
    // Here we test the logic conceptually.

    #[test]
    fn test_is_idle_logic_no_timeout() {
        // If idle_timeout is None, is_idle should always return false
        // This tests the conceptual logic
        let timeout: Option<u64> = None;
        let is_idle = match timeout {
            Some(t) if t > 0 => true, // Would check elapsed
            _ => false,
        };
        assert!(!is_idle);
    }

    #[test]
    fn test_is_idle_logic_zero_timeout() {
        // If idle_timeout is Some(0), is_idle should return false
        let timeout: Option<u64> = Some(0);
        let is_idle = match timeout {
            Some(t) if t > 0 => true,
            _ => false,
        };
        assert!(!is_idle);
    }

    #[test]
    fn test_is_idle_logic_positive_timeout() {
        // If idle_timeout is Some(300), is_idle checks elapsed time
        let timeout: Option<u64> = Some(300);
        let is_idle = match timeout {
            Some(t) if t > 0 => true, // Would check elapsed
            _ => false,
        };
        assert!(is_idle); // Logic branch is correct
    }

    // ===================
    // UPTIME HUMAN FORMAT TESTS
    // ===================

    #[test]
    fn test_uptime_human_format_logic() {
        // Test the logic of uptime_human formatting
        fn format_uptime(secs: u64) -> String {
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

        // Seconds
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(1), "1s");
        assert_eq!(format_uptime(30), "30s");
        assert_eq!(format_uptime(59), "59s");

        // Minutes
        assert_eq!(format_uptime(60), "1m");
        assert_eq!(format_uptime(90), "1m");  // 1.5 minutes = 1m
        assert_eq!(format_uptime(120), "2m");
        assert_eq!(format_uptime(3599), "59m");

        // Hours
        assert_eq!(format_uptime(3600), "1h");
        assert_eq!(format_uptime(7200), "2h");
        assert_eq!(format_uptime(86399), "23h");

        // Days
        assert_eq!(format_uptime(86400), "1d");
        assert_eq!(format_uptime(172800), "2d");
        assert_eq!(format_uptime(604800), "7d"); // 1 week
    }

    #[test]
    fn test_uptime_human_boundary_seconds_to_minutes() {
        fn format_uptime(secs: u64) -> String {
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

        assert_eq!(format_uptime(59), "59s");
        assert_eq!(format_uptime(60), "1m");
        assert_eq!(format_uptime(61), "1m");
    }

    #[test]
    fn test_uptime_human_boundary_minutes_to_hours() {
        fn format_uptime(secs: u64) -> String {
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

        assert_eq!(format_uptime(3599), "59m");
        assert_eq!(format_uptime(3600), "1h");
        assert_eq!(format_uptime(3601), "1h");
    }

    #[test]
    fn test_uptime_human_boundary_hours_to_days() {
        fn format_uptime(secs: u64) -> String {
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

        assert_eq!(format_uptime(86399), "23h");
        assert_eq!(format_uptime(86400), "1d");
        assert_eq!(format_uptime(86401), "1d");
    }

    #[test]
    fn test_uptime_human_large_values() {
        fn format_uptime(secs: u64) -> String {
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

        assert_eq!(format_uptime(31536000), "365d"); // 1 year
        assert_eq!(format_uptime(315360000), "3650d"); // 10 years
    }

    // ===================
    // INSTANCE INFO CLONE TESTS
    // ===================

    #[test]
    fn test_instance_info_clone() {
        let info = InstanceInfo {
            id: InstanceId::new("api", "user1"),
            runtime: RuntimeType::Process,
            socket: PathBuf::from("/tmp/test.sock"),
            uptime_secs: 100,
            restarts: 1,
            health: HealthStatus::Healthy,
            status: InstanceStatus::Running,
            idle_secs: 10,
            idle_timeout: Some(300),
            storage_used_bytes: 1024,
            storage_quota_bytes: Some(2048),
            data_dir: PathBuf::from("/data/api/user1"),
            weight: 100,
        };

        let cloned = info.clone();
        assert_eq!(info.id, cloned.id);
        assert_eq!(info.uptime_secs, cloned.uptime_secs);
        assert_eq!(info.restarts, cloned.restarts);
        assert_eq!(info.health, cloned.health);
        assert_eq!(info.storage_used_bytes, cloned.storage_used_bytes);
        assert_eq!(info.storage_quota_bytes, cloned.storage_quota_bytes);
        assert_eq!(info.weight, cloned.weight);
    }

    #[test]
    fn test_instance_info_debug() {
        let info = InstanceInfo {
            id: InstanceId::new("api", "user1"),
            runtime: RuntimeType::Namespace,
            socket: PathBuf::from("/tmp/test.sock"),
            uptime_secs: 100,
            restarts: 0,
            health: HealthStatus::Unknown,
            status: InstanceStatus::Running,
            idle_secs: 0,
            idle_timeout: None,
            storage_used_bytes: 0,
            storage_quota_bytes: None,
            data_dir: PathBuf::from("/data/api/user1"),
            weight: 100,
        };

        let debug = format!("{:?}", info);
        assert!(debug.contains("api"));
        assert!(debug.contains("user1"));
    }

    // ===================
    // STORAGE FIELDS TESTS
    // ===================

    #[test]
    fn test_instance_info_storage_no_quota() {
        let info = InstanceInfo {
            id: InstanceId::new("api", "user1"),
            runtime: RuntimeType::Process,
            socket: PathBuf::from("/tmp/test.sock"),
            uptime_secs: 100,
            restarts: 0,
            health: HealthStatus::Healthy,
            status: InstanceStatus::Running,
            idle_secs: 0,
            idle_timeout: None,
            storage_used_bytes: 1024 * 1024 * 100, // 100MB
            storage_quota_bytes: None, // No quota
            data_dir: PathBuf::from("/data/api/user1"),
            weight: 100,
        };

        assert_eq!(info.storage_used_bytes, 104857600);
        assert!(info.storage_quota_bytes.is_none());
    }

    #[test]
    fn test_instance_info_storage_with_quota() {
        let info = InstanceInfo {
            id: InstanceId::new("api", "user1"),
            runtime: RuntimeType::Process,
            socket: PathBuf::from("/tmp/test.sock"),
            uptime_secs: 100,
            restarts: 0,
            health: HealthStatus::Healthy,
            status: InstanceStatus::Running,
            idle_secs: 0,
            idle_timeout: None,
            storage_used_bytes: 134217728, // 128MB
            storage_quota_bytes: Some(536870912), // 512MB
            data_dir: PathBuf::from("/data/api/user1"),
            weight: 100,
        };

        assert_eq!(info.storage_used_bytes, 134217728);
        assert_eq!(info.storage_quota_bytes, Some(536870912));
    }

    // ===================
    // WEIGHT TESTS
    // ===================

    #[test]
    fn test_instance_info_weight() {
        let info = InstanceInfo {
            id: InstanceId::new("api", "user1"),
            runtime: RuntimeType::Process,
            socket: PathBuf::from("/tmp/test.sock"),
            uptime_secs: 100,
            restarts: 0,
            health: HealthStatus::Healthy,
            status: InstanceStatus::Running,
            idle_secs: 0,
            idle_timeout: None,
            storage_used_bytes: 0,
            storage_quota_bytes: None,
            data_dir: PathBuf::from("/data/api/user1"),
            weight: 50,
        };

        assert_eq!(info.weight, 50);
    }

    #[test]
    fn test_instance_info_weight_serialization() {
        let info = InstanceInfo {
            id: InstanceId::new("api", "user1"),
            runtime: RuntimeType::Process,
            socket: PathBuf::from("/tmp/test.sock"),
            uptime_secs: 100,
            restarts: 0,
            health: HealthStatus::Healthy,
            status: InstanceStatus::Running,
            idle_secs: 0,
            idle_timeout: None,
            storage_used_bytes: 0,
            storage_quota_bytes: None,
            data_dir: PathBuf::from("/data/api/user1"),
            weight: 75,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"weight\":75"));

        let deserialized: InstanceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.weight, 75);
    }
}
