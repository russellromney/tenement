//! Sandbox runtime - gVisor (runsc), via docker/containerd.
//!
//! Runs the app image with `docker run -d --runtime=runsc --network host`
//! (shared logic in [`super::container`]), the same way the Quark runtime works
//! - Tenement does not hand-roll an OCI bundle. gVisor is the no-KVM fallback,
//!   so unlike Quark this does not require `/dev/kvm`. The container is tracked
//!   by name and reaped with `docker rm -f` ([`RuntimeHandle::Sandbox`]).
//!
//! Linux only; requires docker with the `runsc` runtime registered.

use super::{Runtime, RuntimeHandle, RuntimeType, SpawnConfig};
use anyhow::Result;
use async_trait::async_trait;

/// Runtime that runs containers via `docker run --runtime=runsc` (gVisor).
pub struct SandboxRuntime;

impl SandboxRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SandboxRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Runtime for SandboxRuntime {
    async fn spawn(&self, config: &SpawnConfig) -> Result<RuntimeHandle> {
        #[cfg(target_os = "linux")]
        {
            let name = super::container::linux::run("runsc", config).await?;
            Ok(RuntimeHandle::Sandbox {
                name,
                socket: config.socket.clone(),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            anyhow::bail!(
                "Sandbox (gVisor) runtime requires Linux + docker with the `runsc` \
                 runtime registered. Use isolation = \"process\" for local dev."
            )
        }
    }

    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Sandbox
    }

    fn is_available(&self) -> bool {
        // gVisor is the no-KVM fallback, so (unlike Quark) no /dev/kvm check.
        super::container::docker_available()
    }

    fn name(&self) -> &'static str {
        "sandbox"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_runtime_type() {
        assert_eq!(SandboxRuntime::new().runtime_type(), RuntimeType::Sandbox);
    }

    #[test]
    fn test_sandbox_runtime_name() {
        assert_eq!(SandboxRuntime::new().name(), "sandbox");
    }
}
