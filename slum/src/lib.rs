//! Slum - Fleet orchestration for tenement
//!
//! Manages multiple tenement servers across a fleet.
//! Provides unified routing, metrics aggregation, and log collection.

pub mod db;
pub mod server;

pub use db::{Server, SlumDb, Tenant};
pub use server::SlumState;
