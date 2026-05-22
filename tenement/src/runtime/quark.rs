//! Quark runtime, via docker/containerd.
//!
//! Runs the app image with `docker run -d --runtime=quark --network host`
//! (shared logic in [`super::container`]). Tenement does not hand-roll an OCI
//! bundle — docker/containerd generate the complete spec Quark needs. The
//! container is tracked by name and reaped with `docker rm -f`
//! ([`RuntimeHandle::Quark`]).
//!
//! Linux only; requires docker with the `quark` runtime registered + `/dev/kvm`.

use super::{Runtime, RuntimeHandle, RuntimeType, SpawnConfig};
use anyhow::Result;
use async_trait::async_trait;

/// Runtime that runs containers via `docker run --runtime=quark`.
pub struct QuarkRuntime;

impl QuarkRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for QuarkRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Runtime for QuarkRuntime {
    async fn spawn(&self, config: &SpawnConfig) -> Result<RuntimeHandle> {
        #[cfg(target_os = "linux")]
        {
            let name = super::container::linux::run("quark", config).await?;
            Ok(RuntimeHandle::Quark {
                name,
                socket: config.socket.clone(),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            anyhow::bail!(
                "Quark runtime requires Linux + docker (with the `quark` runtime \
                 registered) + /dev/kvm. Use isolation = \"process\" for local dev."
            )
        }
    }

    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Quark
    }

    fn is_available(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            super::container::docker_available() && std::path::Path::new("/dev/kvm").exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    fn name(&self) -> &'static str {
        "quark"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quark_runtime_type_and_name() {
        let rt = QuarkRuntime::new();
        assert_eq!(rt.runtime_type(), RuntimeType::Quark);
        assert_eq!(rt.name(), "quark");
    }
}
