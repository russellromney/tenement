//! tenement - Hyperlightweight process hypervisor for single-server deployments
//!
//! Spawn and supervise processes with Unix socket communication,
//! health checks, and automatic restarts.

pub mod auth;
pub mod config;
pub mod hypervisor;
pub mod instance;
pub mod logs;
pub mod metrics;
pub mod store;

pub use auth::{generate_token, hash_token, verify_token, TokenStore};
pub use config::Config;
pub use hypervisor::Hypervisor;
pub use instance::{Instance, InstanceId, InstanceStatus};
pub use logs::{LogBuffer, LogEntry, LogLevel, LogQuery};
pub use metrics::Metrics;
pub use store::{init_db, ConfigStore, DbPool, LogStore};
