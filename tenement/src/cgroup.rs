//! Cgroup v2 resource limits for Linux
//!
//! Provides memory and CPU limits via cgroups v2 unified hierarchy.
//! Requires Linux kernel 4.5+ with cgroups v2 enabled.
//!
//! **Linux only** - on other platforms, returns Ok() (no-op).

#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;
use std::path::PathBuf;

/// Base path for cgroups v2 unified hierarchy (Linux only)
#[cfg(target_os = "linux")]
const CGROUP_BASE: &str = "/sys/fs/cgroup";

/// Tenement cgroup subtree
const TENEMENT_CGROUP: &str = "/sys/fs/cgroup/tenement";

/// Resource limits for a service instance
#[derive(Debug, Clone, Default)]
pub struct ResourceLimits {
    /// Memory limit in MB (None = unlimited)
    pub memory_limit_mb: Option<u32>,
    /// CPU weight (1-10000, None = default 100)
    pub cpu_shares: Option<u32>,
}

impl ResourceLimits {
    /// Check if any limits are configured
    pub fn has_limits(&self) -> bool {
        self.memory_limit_mb.is_some() || self.cpu_shares.is_some()
    }
}

/// Manages cgroup v2 resource limits for tenement instances
pub struct CgroupManager {
    /// Base path for tenement cgroups
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    base_path: PathBuf,
}

impl CgroupManager {
    /// Create a new cgroup manager
    pub fn new() -> Self {
        Self {
            base_path: PathBuf::from(TENEMENT_CGROUP),
        }
    }

    /// Check if cgroups v2 are available on this system
    #[cfg(target_os = "linux")]
    pub fn is_available(&self) -> bool {
        // Check for cgroup2 filesystem
        let cgroup2_path = PathBuf::from(CGROUP_BASE);
        if !cgroup2_path.exists() {
            return false;
        }

        // Check for cgroup.controllers file (indicates v2)
        let controllers_path = cgroup2_path.join("cgroup.controllers");
        controllers_path.exists()
    }

    #[cfg(not(target_os = "linux"))]
    pub fn is_available(&self) -> bool {
        false
    }

    /// Get the cgroup path for an instance
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn cgroup_path(&self, instance_id: &str) -> PathBuf {
        self.base_path.join(instance_id)
    }

    /// Create a cgroup for an instance and apply resource limits
    #[cfg(target_os = "linux")]
    pub fn create_cgroup(&self, instance_id: &str, limits: &ResourceLimits) -> Result<()> {
        // Check availability first
        if !self.is_available() {
            if limits.has_limits() {
                tracing::warn!(
                    "cgroups v2 not available, resource limits will not be enforced for {}",
                    instance_id
                );
            }
            return Ok(());
        }

        // Then check if limits are needed
        if !limits.has_limits() {
            return Ok(());
        }

        // Ensure base tenement cgroup exists
        self.ensure_base_cgroup()?;

        // Create instance cgroup
        let cgroup_path = self.cgroup_path(instance_id);
        std::fs::create_dir_all(&cgroup_path).with_context(|| {
            format!(
                "Failed to create cgroup directory: {}\n\
                Try: sudo mkdir -p {} && sudo chown $(id -u):$(id -g) {}",
                cgroup_path.display(),
                TENEMENT_CGROUP,
                TENEMENT_CGROUP
            )
        })?;

        // Apply memory limit
        if let Some(memory_mb) = limits.memory_limit_mb {
            if memory_mb > 0 {
                let memory_bytes = (memory_mb as u64) * 1024 * 1024;
                let memory_max_path = cgroup_path.join("memory.max");
                std::fs::write(&memory_max_path, memory_bytes.to_string()).with_context(|| {
                    format!(
                        "Failed to set memory limit: {}\n\
                        Ensure memory controller is enabled in parent cgroup",
                        memory_max_path.display()
                    )
                })?;
                tracing::debug!(
                    "Set memory limit for {}: {}MB",
                    instance_id,
                    memory_mb
                );
            }
        }

        // Apply CPU weight
        if let Some(cpu_weight) = limits.cpu_shares {
            // Clamp to valid range (1-10000)
            let weight = cpu_weight.clamp(1, 10000);
            if weight != cpu_weight {
                tracing::info!(
                    "CPU weight {} clamped to {} for '{}'",
                    cpu_weight,
                    weight,
                    instance_id
                );
            }
            let cpu_weight_path = cgroup_path.join("cpu.weight");
            std::fs::write(&cpu_weight_path, weight.to_string()).with_context(|| {
                format!(
                    "Failed to set CPU weight: {}\n\
                    Ensure cpu controller is enabled in parent cgroup",
                    cpu_weight_path.display()
                )
            })?;
            tracing::debug!("Set CPU weight for {}: {}", instance_id, weight);
        }

        tracing::info!(
            "Created cgroup for {} with limits: memory={}MB, cpu_weight={}",
            instance_id,
            limits.memory_limit_mb.unwrap_or(0),
            limits.cpu_shares.unwrap_or(100)
        );

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn create_cgroup(&self, _instance_id: &str, _limits: &ResourceLimits) -> Result<()> {
        // No-op on non-Linux
        Ok(())
    }

    /// Add a process to the instance's cgroup
    #[cfg(target_os = "linux")]
    pub fn add_process(&self, instance_id: &str, pid: u32, limits: &ResourceLimits) -> Result<()> {
        if !limits.has_limits() {
            return Ok(());
        }

        if !self.is_available() {
            return Ok(());
        }

        let cgroup_path = self.cgroup_path(instance_id);
        if !cgroup_path.exists() {
            // Cgroup doesn't exist, might not have been created
            return Ok(());
        }

        let procs_path = cgroup_path.join("cgroup.procs");
        std::fs::write(&procs_path, pid.to_string()).with_context(|| {
            format!(
                "Failed to add PID {} to cgroup: {}",
                pid,
                procs_path.display()
            )
        })?;

        tracing::debug!("Added PID {} to cgroup {}", pid, instance_id);
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn add_process(&self, _instance_id: &str, _pid: u32, _limits: &ResourceLimits) -> Result<()> {
        Ok(())
    }

    /// Remove the cgroup for an instance
    #[cfg(target_os = "linux")]
    pub fn remove_cgroup(&self, instance_id: &str) -> Result<()> {
        let cgroup_path = self.cgroup_path(instance_id);
        if cgroup_path.exists() {
            // Move any remaining processes to parent before removing
            // (kernel requires cgroup to be empty before removal)
            let procs_path = cgroup_path.join("cgroup.procs");
            if procs_path.exists() {
                if let Ok(contents) = std::fs::read_to_string(&procs_path) {
                    let parent_procs = self.base_path.join("cgroup.procs");
                    for line in contents.lines() {
                        if let Ok(pid) = line.trim().parse::<u32>() {
                            // Move to parent (or init cgroup)
                            if let Err(e) = std::fs::write(&parent_procs, pid.to_string()) {
                                // Process may have already exited, log but continue
                                tracing::warn!(
                                    "Failed to move PID {} to parent cgroup for {}: {}",
                                    pid,
                                    instance_id,
                                    e
                                );
                            }
                        }
                    }
                }
            }

            // Now remove the cgroup directory
            if let Err(e) = std::fs::remove_dir(&cgroup_path) {
                tracing::warn!(
                    "Failed to remove cgroup directory for {}: {}",
                    instance_id,
                    e
                );
            } else {
                tracing::debug!("Removed cgroup for {}", instance_id);
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn remove_cgroup(&self, _instance_id: &str) -> Result<()> {
        Ok(())
    }

    /// Ensure the base tenement cgroup exists with proper controllers enabled
    #[cfg(target_os = "linux")]
    fn ensure_base_cgroup(&self) -> Result<()> {
        if self.base_path.exists() {
            return Ok(());
        }

        // Create base tenement cgroup
        std::fs::create_dir_all(&self.base_path).with_context(|| {
            format!(
                "Failed to create tenement cgroup: {}\n\
                Try: sudo mkdir -p {} && sudo chown $(id -u):$(id -g) {}",
                self.base_path.display(),
                TENEMENT_CGROUP,
                TENEMENT_CGROUP
            )
        })?;

        // Enable controllers for child cgroups
        // We need memory and cpu controllers
        let subtree_control = self.base_path.join("cgroup.subtree_control");
        if subtree_control.exists() {
            // Try to enable controllers (may fail if not available in parent)
            std::fs::write(&subtree_control, "+memory +cpu").ok();
        }

        Ok(())
    }
}

impl Default for CgroupManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================
    // RESOURCE LIMITS TESTS
    // ===================

    #[test]
    fn test_resource_limits_has_limits() {
        let empty = ResourceLimits::default();
        assert!(!empty.has_limits());

        let with_memory = ResourceLimits {
            memory_limit_mb: Some(256),
            cpu_shares: None,
        };
        assert!(with_memory.has_limits());

        let with_cpu = ResourceLimits {
            memory_limit_mb: None,
            cpu_shares: Some(200),
        };
        assert!(with_cpu.has_limits());

        let with_both = ResourceLimits {
            memory_limit_mb: Some(512),
            cpu_shares: Some(500),
        };
        assert!(with_both.has_limits());
    }

    #[test]
    fn test_resource_limits_default() {
        let limits = ResourceLimits::default();
        assert!(limits.memory_limit_mb.is_none());
        assert!(limits.cpu_shares.is_none());
        assert!(!limits.has_limits());
    }

    #[test]
    fn test_resource_limits_clone() {
        let limits = ResourceLimits {
            memory_limit_mb: Some(512),
            cpu_shares: Some(200),
        };
        let cloned = limits.clone();
        assert_eq!(limits.memory_limit_mb, cloned.memory_limit_mb);
        assert_eq!(limits.cpu_shares, cloned.cpu_shares);
    }

    #[test]
    fn test_resource_limits_debug() {
        let limits = ResourceLimits {
            memory_limit_mb: Some(256),
            cpu_shares: Some(100),
        };
        let debug = format!("{:?}", limits);
        assert!(debug.contains("256"));
        assert!(debug.contains("100"));
    }

    #[test]
    fn test_resource_limits_memory_only() {
        let limits = ResourceLimits {
            memory_limit_mb: Some(1024),
            cpu_shares: None,
        };
        assert!(limits.has_limits());
        assert_eq!(limits.memory_limit_mb, Some(1024));
    }

    #[test]
    fn test_resource_limits_cpu_only() {
        let limits = ResourceLimits {
            memory_limit_mb: None,
            cpu_shares: Some(500),
        };
        assert!(limits.has_limits());
        assert_eq!(limits.cpu_shares, Some(500));
    }

    #[test]
    fn test_resource_limits_zero_memory() {
        // memory_limit_mb of Some(0) still counts as "has limits"
        // The behavior is handled at application time
        let limits = ResourceLimits {
            memory_limit_mb: Some(0),
            cpu_shares: None,
        };
        assert!(limits.has_limits());
    }

    #[test]
    fn test_resource_limits_zero_cpu() {
        let limits = ResourceLimits {
            memory_limit_mb: None,
            cpu_shares: Some(0),
        };
        assert!(limits.has_limits());
    }

    #[test]
    fn test_resource_limits_large_values() {
        let limits = ResourceLimits {
            memory_limit_mb: Some(u32::MAX),
            cpu_shares: Some(10000),
        };
        assert!(limits.has_limits());
        assert_eq!(limits.memory_limit_mb, Some(u32::MAX));
        assert_eq!(limits.cpu_shares, Some(10000));
    }

    // ===================
    // CGROUP PATH TESTS
    // ===================

    #[test]
    fn test_cgroup_path() {
        let manager = CgroupManager::new();
        let path = manager.cgroup_path("api:user123");
        assert_eq!(
            path,
            PathBuf::from("/sys/fs/cgroup/tenement/api:user123")
        );
    }

    #[test]
    fn test_cgroup_path_with_special_chars() {
        let manager = CgroupManager::new();
        let path = manager.cgroup_path("api-v2:user_123");
        assert_eq!(
            path,
            PathBuf::from("/sys/fs/cgroup/tenement/api-v2:user_123")
        );
    }

    #[test]
    fn test_cgroup_path_simple_id() {
        let manager = CgroupManager::new();
        let path = manager.cgroup_path("simple");
        assert_eq!(
            path,
            PathBuf::from("/sys/fs/cgroup/tenement/simple")
        );
    }

    #[test]
    fn test_cgroup_manager_new() {
        let manager = CgroupManager::new();
        assert_eq!(manager.base_path, PathBuf::from(TENEMENT_CGROUP));
    }

    #[test]
    fn test_cgroup_manager_default() {
        let manager = CgroupManager::default();
        assert_eq!(manager.base_path, PathBuf::from(TENEMENT_CGROUP));
    }

    // ===================
    // NON-LINUX TESTS
    // ===================

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_cgroup_not_available_on_non_linux() {
        let manager = CgroupManager::new();
        assert!(!manager.is_available());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_cgroup_operations_noop_on_non_linux() {
        let manager = CgroupManager::new();
        let limits = ResourceLimits {
            memory_limit_mb: Some(256),
            cpu_shares: Some(100),
        };

        // All operations should succeed as no-ops
        assert!(manager.create_cgroup("test", &limits).is_ok());
        assert!(manager.add_process("test", 1234, &limits).is_ok());
        assert!(manager.remove_cgroup("test").is_ok());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_cgroup_create_no_limits_noop() {
        let manager = CgroupManager::new();
        let limits = ResourceLimits::default();

        // No limits means no-op even on Linux
        assert!(manager.create_cgroup("test", &limits).is_ok());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_cgroup_add_process_no_limits_noop() {
        let manager = CgroupManager::new();
        let limits = ResourceLimits::default();

        // No limits means no-op
        assert!(manager.add_process("test", 12345, &limits).is_ok());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_cgroup_remove_nonexistent() {
        let manager = CgroupManager::new();

        // Should succeed (no-op) even if cgroup doesn't exist
        assert!(manager.remove_cgroup("nonexistent").is_ok());
    }

    // ===================
    // LINUX-SPECIFIC TESTS
    // ===================

    #[cfg(target_os = "linux")]
    mod linux_tests {
        use super::*;
        use tempfile::TempDir;

        // Note: These tests may require elevated privileges to actually create cgroups.
        // They're marked #[ignore] to prevent failures in CI without proper permissions.

        #[test]
        fn test_cgroup_is_available() {
            let manager = CgroupManager::new();
            // This will return true if /sys/fs/cgroup/cgroup.controllers exists
            // The result depends on the system configuration
            let _available = manager.is_available();
            // We can't assert a specific value as it depends on the system
        }

        #[test]
        #[ignore = "requires root/cgroup privileges"]
        fn test_create_and_remove_cgroup() {
            let manager = CgroupManager::new();
            let limits = ResourceLimits {
                memory_limit_mb: Some(256),
                cpu_shares: Some(100),
            };

            let instance_id = format!("test-{}", std::process::id());

            // Create cgroup
            manager.create_cgroup(&instance_id, &limits).unwrap();

            // Verify it exists
            let cgroup_path = manager.cgroup_path(&instance_id);
            assert!(cgroup_path.exists());

            // Remove cgroup
            manager.remove_cgroup(&instance_id).unwrap();

            // Verify it's gone
            assert!(!cgroup_path.exists());
        }

        #[test]
        #[ignore = "requires root/cgroup privileges"]
        fn test_create_cgroup_sets_memory_limit() {
            let manager = CgroupManager::new();
            let limits = ResourceLimits {
                memory_limit_mb: Some(256),
                cpu_shares: None,
            };

            let instance_id = format!("test-mem-{}", std::process::id());

            manager.create_cgroup(&instance_id, &limits).unwrap();

            // Verify memory.max was written
            let memory_max_path = manager.cgroup_path(&instance_id).join("memory.max");
            if memory_max_path.exists() {
                let content = std::fs::read_to_string(&memory_max_path).unwrap();
                let expected_bytes = 256u64 * 1024 * 1024;
                assert_eq!(content.trim(), expected_bytes.to_string());
            }

            // Cleanup
            manager.remove_cgroup(&instance_id).ok();
        }

        #[test]
        #[ignore = "requires root/cgroup privileges"]
        fn test_create_cgroup_sets_cpu_weight() {
            let manager = CgroupManager::new();
            let limits = ResourceLimits {
                memory_limit_mb: None,
                cpu_shares: Some(500),
            };

            let instance_id = format!("test-cpu-{}", std::process::id());

            manager.create_cgroup(&instance_id, &limits).unwrap();

            // Verify cpu.weight was written
            let cpu_weight_path = manager.cgroup_path(&instance_id).join("cpu.weight");
            if cpu_weight_path.exists() {
                let content = std::fs::read_to_string(&cpu_weight_path).unwrap();
                assert_eq!(content.trim(), "500");
            }

            // Cleanup
            manager.remove_cgroup(&instance_id).ok();
        }

        #[test]
        #[ignore = "requires root/cgroup privileges"]
        fn test_cpu_weight_clamped_minimum() {
            let manager = CgroupManager::new();
            let limits = ResourceLimits {
                memory_limit_mb: None,
                cpu_shares: Some(0), // Below minimum, should clamp to 1
            };

            let instance_id = format!("test-cpu-min-{}", std::process::id());

            manager.create_cgroup(&instance_id, &limits).unwrap();

            let cpu_weight_path = manager.cgroup_path(&instance_id).join("cpu.weight");
            if cpu_weight_path.exists() {
                let content = std::fs::read_to_string(&cpu_weight_path).unwrap();
                assert_eq!(content.trim(), "1"); // Clamped to minimum
            }

            manager.remove_cgroup(&instance_id).ok();
        }

        #[test]
        #[ignore = "requires root/cgroup privileges"]
        fn test_cpu_weight_clamped_maximum() {
            let manager = CgroupManager::new();
            let limits = ResourceLimits {
                memory_limit_mb: None,
                cpu_shares: Some(50000), // Above maximum, should clamp to 10000
            };

            let instance_id = format!("test-cpu-max-{}", std::process::id());

            manager.create_cgroup(&instance_id, &limits).unwrap();

            let cpu_weight_path = manager.cgroup_path(&instance_id).join("cpu.weight");
            if cpu_weight_path.exists() {
                let content = std::fs::read_to_string(&cpu_weight_path).unwrap();
                assert_eq!(content.trim(), "10000"); // Clamped to maximum
            }

            manager.remove_cgroup(&instance_id).ok();
        }

        #[test]
        fn test_create_cgroup_no_limits_skips() {
            let manager = CgroupManager::new();
            let limits = ResourceLimits::default();

            // Should return Ok immediately without creating anything
            assert!(manager.create_cgroup("test-no-limits", &limits).is_ok());

            // Cgroup should not exist
            let cgroup_path = manager.cgroup_path("test-no-limits");
            assert!(!cgroup_path.exists());
        }

        #[test]
        fn test_add_process_no_limits_skips() {
            let manager = CgroupManager::new();
            let limits = ResourceLimits::default();

            // Should return Ok immediately
            assert!(manager.add_process("test", 12345, &limits).is_ok());
        }

        #[test]
        fn test_remove_nonexistent_cgroup() {
            let manager = CgroupManager::new();

            // Should succeed even if cgroup doesn't exist
            assert!(manager.remove_cgroup("nonexistent-cgroup-12345").is_ok());
        }
    }

    // ===================
    // CPU WEIGHT CLAMPING LOGIC TESTS
    // ===================

    #[test]
    fn test_cpu_weight_clamp_logic() {
        // Test the clamping logic used in create_cgroup
        fn clamp_cpu_weight(weight: u32) -> u32 {
            weight.clamp(1, 10000)
        }

        assert_eq!(clamp_cpu_weight(0), 1);      // Below min
        assert_eq!(clamp_cpu_weight(1), 1);      // At min
        assert_eq!(clamp_cpu_weight(100), 100);  // Default
        assert_eq!(clamp_cpu_weight(500), 500);  // Normal
        assert_eq!(clamp_cpu_weight(10000), 10000); // At max
        assert_eq!(clamp_cpu_weight(10001), 10000); // Above max
        assert_eq!(clamp_cpu_weight(u32::MAX), 10000); // Way above max
    }

    // ===================
    // MEMORY BYTES CALCULATION TESTS
    // ===================

    #[test]
    fn test_memory_bytes_calculation() {
        fn memory_mb_to_bytes(memory_mb: u32) -> u64 {
            (memory_mb as u64) * 1024 * 1024
        }

        assert_eq!(memory_mb_to_bytes(1), 1048576);       // 1 MB
        assert_eq!(memory_mb_to_bytes(256), 268435456);   // 256 MB
        assert_eq!(memory_mb_to_bytes(1024), 1073741824); // 1 GB
        assert_eq!(memory_mb_to_bytes(4096), 4294967296); // 4 GB
    }

    #[test]
    fn test_memory_bytes_large_values() {
        fn memory_mb_to_bytes(memory_mb: u32) -> u64 {
            (memory_mb as u64) * 1024 * 1024
        }

        // Test that u32::MAX MB doesn't overflow when converted to u64 bytes
        let max_mb = u32::MAX;
        let bytes = memory_mb_to_bytes(max_mb);
        assert!(bytes > 0);
        assert_eq!(bytes, (u32::MAX as u64) * 1024 * 1024);
    }
}
