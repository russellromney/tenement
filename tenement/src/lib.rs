//! tenement - Hyperlightweight process hypervisor for single-server deployments
//!
//! Spawn and supervise processes with Unix socket communication,
//! health checks, and automatic restarts.

pub mod auth;
pub mod cgroup;
pub mod config;
pub mod hypervisor;
pub mod instance;
pub mod logs;
pub mod metrics;
pub mod port_allocator;
pub mod runtime;
pub mod storage;
pub mod store;

pub use auth::{generate_token, hash_token, verify_token, TokenStore};
pub use cgroup::{CgroupManager, ResourceLimits};
pub use config::{Config, TlsConfig};
pub use hypervisor::Hypervisor;
pub use instance::{Instance, InstanceId, InstanceStatus};
pub use logs::{LogBuffer, LogEntry, LogLevel, LogQuery};
pub use metrics::Metrics;
pub use port_allocator::PortAllocator;
pub use runtime::{ProcessRuntime, Runtime, RuntimeHandle, RuntimeType, SpawnConfig, VmConfig};
#[cfg(feature = "sandbox")]
pub use runtime::SandboxRuntime;
pub use storage::{calculate_dir_size, format_bytes, StorageInfo};
pub use store::{init_db, ConfigStore, DbPool, LogStore};
