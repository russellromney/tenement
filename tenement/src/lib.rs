//! tenement - Hyperlightweight process hypervisor for single-server deployments
//!
//! Spawn and supervise processes with Unix socket communication,
//! health checks, and automatic restarts.

pub mod config;
pub mod hypervisor;
pub mod instance;

pub use config::Config;
pub use hypervisor::Hypervisor;
pub use instance::{Instance, InstanceId, InstanceStatus};
