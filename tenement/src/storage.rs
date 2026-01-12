//! Storage quota management for tenement instances
//!
//! Provides utilities for calculating directory sizes and tracking
//! storage usage against configured quotas.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Storage information for an instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    /// Current storage usage in bytes
    pub used_bytes: u64,
    /// Configured quota in bytes (None = unlimited)
    pub quota_bytes: Option<u64>,
    /// Path to the data directory
    pub path: PathBuf,
}

impl StorageInfo {
    /// Create a new StorageInfo
    pub fn new(used_bytes: u64, quota_bytes: Option<u64>, path: PathBuf) -> Self {
        Self {
            used_bytes,
            quota_bytes,
            path,
        }
    }

    /// Calculate usage percentage (0.0-100.0)
    /// Returns None if no quota is configured
    pub fn usage_percent(&self) -> Option<f64> {
        self.quota_bytes.map(|quota| {
            if quota == 0 {
                if self.used_bytes > 0 {
                    100.0 // Any usage is over 0 quota
                } else {
                    0.0
                }
            } else {
                (self.used_bytes as f64 / quota as f64) * 100.0
            }
        })
    }

    /// Calculate usage ratio (0.0-1.0+)
    /// Returns None if no quota is configured
    pub fn usage_ratio(&self) -> Option<f64> {
        self.quota_bytes.map(|quota| {
            if quota == 0 {
                if self.used_bytes > 0 {
                    1.0 // Over quota
                } else {
                    0.0
                }
            } else {
                self.used_bytes as f64 / quota as f64
            }
        })
    }

    /// Check if storage usage exceeds quota
    pub fn is_over_quota(&self) -> bool {
        match self.quota_bytes {
            Some(quota) => self.used_bytes > quota,
            None => false, // No quota = never over
        }
    }

    /// Format usage as human-readable string (e.g., "134MB / 512MB")
    pub fn format_usage(&self) -> String {
        let used = format_bytes(self.used_bytes);
        match self.quota_bytes {
            Some(quota) => format!("{} / {}", used, format_bytes(quota)),
            None => used,
        }
    }
}

/// Format bytes as human-readable string (e.g., "134MB", "1.2GB")
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{}MB", bytes / MB)
    } else if bytes >= KB {
        format!("{}KB", bytes / KB)
    } else {
        format!("{}B", bytes)
    }
}

/// Calculate the total size of a directory recursively
///
/// This is synchronous and should be called from a blocking context.
/// For async usage, wrap in `tokio::task::spawn_blocking`.
pub fn calculate_dir_size_sync(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    if !path.is_dir() {
        // Single file
        let metadata = std::fs::metadata(path)?;
        return Ok(metadata.len());
    }

    let mut total = 0u64;
    calculate_dir_size_recursive(path, &mut total)?;
    Ok(total)
}

/// Recursive helper for directory size calculation
fn calculate_dir_size_recursive(path: &Path, total: &mut u64) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            calculate_dir_size_recursive(&entry.path(), total)?;
        } else {
            *total += metadata.len();
        }
    }
    Ok(())
}

/// Calculate directory size asynchronously
///
/// Uses spawn_blocking to avoid blocking the async runtime.
pub async fn calculate_dir_size(path: PathBuf) -> Result<u64> {
    tokio::task::spawn_blocking(move || calculate_dir_size_sync(&path))
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    // ===================
    // STORAGE INFO TESTS
    // ===================

    #[test]
    fn test_storage_info_new() {
        let info = StorageInfo::new(
            1024 * 1024,
            Some(10 * 1024 * 1024),
            PathBuf::from("/data/api/prod"),
        );
        assert_eq!(info.used_bytes, 1024 * 1024);
        assert_eq!(info.quota_bytes, Some(10 * 1024 * 1024));
        assert_eq!(info.path, PathBuf::from("/data/api/prod"));
    }

    #[test]
    fn test_storage_info_no_quota() {
        let info = StorageInfo::new(
            5 * 1024 * 1024,
            None,
            PathBuf::from("/data/api/prod"),
        );
        assert_eq!(info.quota_bytes, None);
        assert!(!info.is_over_quota());
        assert_eq!(info.usage_percent(), None);
        assert_eq!(info.usage_ratio(), None);
    }

    #[test]
    fn test_usage_percent() {
        let info = StorageInfo::new(
            25 * 1024 * 1024,  // 25MB used
            Some(100 * 1024 * 1024),  // 100MB quota
            PathBuf::from("/data"),
        );

        let percent = info.usage_percent().unwrap();
        assert!((percent - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_usage_percent_zero_quota() {
        let info = StorageInfo::new(
            1024,  // 1KB used
            Some(0),  // 0 quota
            PathBuf::from("/data"),
        );

        let percent = info.usage_percent().unwrap();
        assert_eq!(percent, 100.0); // Any usage is 100% of 0
    }

    #[test]
    fn test_usage_percent_zero_used_zero_quota() {
        let info = StorageInfo::new(
            0,  // 0 used
            Some(0),  // 0 quota
            PathBuf::from("/data"),
        );

        let percent = info.usage_percent().unwrap();
        assert_eq!(percent, 0.0);
    }

    #[test]
    fn test_usage_ratio() {
        let info = StorageInfo::new(
            50 * 1024 * 1024,  // 50MB used
            Some(100 * 1024 * 1024),  // 100MB quota
            PathBuf::from("/data"),
        );

        let ratio = info.usage_ratio().unwrap();
        assert!((ratio - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_is_over_quota() {
        // Under quota
        let under = StorageInfo::new(50, Some(100), PathBuf::from("/data"));
        assert!(!under.is_over_quota());

        // At quota
        let at = StorageInfo::new(100, Some(100), PathBuf::from("/data"));
        assert!(!at.is_over_quota());

        // Over quota
        let over = StorageInfo::new(150, Some(100), PathBuf::from("/data"));
        assert!(over.is_over_quota());

        // No quota
        let unlimited = StorageInfo::new(u64::MAX, None, PathBuf::from("/data"));
        assert!(!unlimited.is_over_quota());
    }

    #[test]
    fn test_format_usage() {
        let with_quota = StorageInfo::new(
            134 * 1024 * 1024,
            Some(512 * 1024 * 1024),
            PathBuf::from("/data"),
        );
        assert_eq!(with_quota.format_usage(), "134MB / 512MB");

        let without_quota = StorageInfo::new(
            256 * 1024 * 1024,
            None,
            PathBuf::from("/data"),
        );
        assert_eq!(without_quota.format_usage(), "256MB");
    }

    // ===================
    // FORMAT BYTES TESTS
    // ===================

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1023), "1023B");
        assert_eq!(format_bytes(1024), "1KB");
        assert_eq!(format_bytes(1536), "1KB");  // Truncates, not rounds
        assert_eq!(format_bytes(1024 * 1024), "1MB");
        assert_eq!(format_bytes(134 * 1024 * 1024), "134MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0GB");
        assert_eq!(format_bytes(1536 * 1024 * 1024), "1.5GB");
    }

    // ===================
    // DIRECTORY SIZE TESTS
    // ===================

    #[test]
    fn test_calculate_dir_size_empty() {
        let dir = TempDir::new().unwrap();
        let size = calculate_dir_size_sync(dir.path()).unwrap();
        assert_eq!(size, 0);
    }

    #[test]
    fn test_calculate_dir_size_single_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        let size = calculate_dir_size_sync(dir.path()).unwrap();
        assert_eq!(size, 11); // "hello world" = 11 bytes
    }

    #[test]
    fn test_calculate_dir_size_multiple_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "aaa").unwrap();  // 3 bytes
        fs::write(dir.path().join("b.txt"), "bbbbb").unwrap();  // 5 bytes

        let size = calculate_dir_size_sync(dir.path()).unwrap();
        assert_eq!(size, 8);
    }

    #[test]
    fn test_calculate_dir_size_nested() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("subdir");
        fs::create_dir(&sub).unwrap();

        fs::write(dir.path().join("root.txt"), "root").unwrap();  // 4 bytes
        fs::write(sub.join("nested.txt"), "nested").unwrap();  // 6 bytes

        let size = calculate_dir_size_sync(dir.path()).unwrap();
        assert_eq!(size, 10);
    }

    #[test]
    fn test_calculate_dir_size_deeply_nested() {
        let dir = TempDir::new().unwrap();
        let mut current = dir.path().to_path_buf();

        for i in 0..5 {
            current = current.join(format!("level{}", i));
            fs::create_dir(&current).unwrap();
            fs::write(current.join("file.txt"), format!("level{}", i)).unwrap();
        }

        let size = calculate_dir_size_sync(dir.path()).unwrap();
        // Each "level{i}" is 6 bytes * 5 levels = 30 bytes
        assert_eq!(size, 30);
    }

    #[test]
    fn test_calculate_dir_size_nonexistent() {
        let path = PathBuf::from("/nonexistent/path/12345");
        let size = calculate_dir_size_sync(&path).unwrap();
        assert_eq!(size, 0);
    }

    #[test]
    fn test_calculate_dir_size_single_file_path() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello").unwrap();

        let size = calculate_dir_size_sync(&file_path).unwrap();
        assert_eq!(size, 5);
    }

    #[tokio::test]
    async fn test_calculate_dir_size_async() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.txt"), "async test").unwrap();

        let size = calculate_dir_size(dir.path().to_path_buf()).await.unwrap();
        assert_eq!(size, 10);
    }

    // ===================
    // STORAGE INFO SERIALIZATION
    // ===================

    #[test]
    fn test_storage_info_serialize() {
        let info = StorageInfo::new(
            134217728,
            Some(536870912),
            PathBuf::from("/data/api/prod"),
        );

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("134217728"));
        assert!(json.contains("536870912"));
        assert!(json.contains("/data/api/prod"));
    }

    #[test]
    fn test_storage_info_deserialize() {
        let json = r#"{
            "used_bytes": 134217728,
            "quota_bytes": 536870912,
            "path": "/data/api/prod"
        }"#;

        let info: StorageInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.used_bytes, 134217728);
        assert_eq!(info.quota_bytes, Some(536870912));
        assert_eq!(info.path, PathBuf::from("/data/api/prod"));
    }

    #[test]
    fn test_storage_info_deserialize_null_quota() {
        let json = r#"{
            "used_bytes": 134217728,
            "quota_bytes": null,
            "path": "/data/api/prod"
        }"#;

        let info: StorageInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.quota_bytes, None);
    }

    // ===================
    // CLONE AND DEBUG TESTS
    // ===================

    #[test]
    fn test_storage_info_clone() {
        let info = StorageInfo::new(1000, Some(2000), PathBuf::from("/data"));
        let cloned = info.clone();
        assert_eq!(info.used_bytes, cloned.used_bytes);
        assert_eq!(info.quota_bytes, cloned.quota_bytes);
        assert_eq!(info.path, cloned.path);
    }

    #[test]
    fn test_storage_info_debug() {
        let info = StorageInfo::new(1000, Some(2000), PathBuf::from("/data"));
        let debug = format!("{:?}", info);
        assert!(debug.contains("1000"));
        assert!(debug.contains("2000"));
        assert!(debug.contains("/data"));
    }
}
