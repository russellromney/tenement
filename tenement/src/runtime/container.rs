//! Shared helpers for runtimes that run OCI containers via docker/containerd
//! (Quark, gVisor/runsc).
//!
//! Tenement does NOT hand-roll an OCI bundle for these. A minimal hand-rolled
//! spec fails on real apps (missing seccomp/caps/masked-paths/standard `/dev`/
//! networking); docker/containerd generate the complete spec the runtime needs.
//! We run the app's image with `docker run -d --runtime=<runtime> --network
//! host`: the app binds the allocated PORT directly (Tenement proxies to
//! `127.0.0.1:PORT`) and can reach host-local sidecars (e.g. the DB protocol
//! sidecar at `127.0.0.1:<pgport>`). The container is owned by the docker
//! daemon; Tenement tracks it by name and reaps it with `docker rm -f`.
//!
//! Per-app network isolation (per-app bridge + sidecar-as-container) is a
//! hardening follow-up.

/// docker present on PATH (or the usual location)?
pub fn docker_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var("PATH")
            .map(|p| {
                p.split(':')
                    .any(|d| std::path::Path::new(d).join("docker").exists())
            })
            .unwrap_or(false)
            || std::path::Path::new("/usr/bin/docker").exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn docker_run_args(
    runtime: &str,
    name: &str,
    image: &str,
    config: &crate::runtime::SpawnConfig,
) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--rm".to_string(),
        "--name".to_string(),
        name.to_string(),
        format!("--runtime={runtime}"),
        "--network".to_string(),
        "host".to_string(),
    ];

    if let Some(memory_mb) = config.memory_limit_mb {
        if memory_mb > 0 {
            args.push("--memory".to_string());
            args.push(format!("{memory_mb}m"));
        }
    }

    if let Some(cpu_shares) = config.cpu_shares {
        args.push("--cpu-shares".to_string());
        args.push(cpu_shares.clamp(2, 10000).to_string());
    }

    // Neutralize any image ENTRYPOINT (railpack bakes `/bin/bash -c`) so the
    // explicit command runs directly, not as args to the entrypoint.
    // Harmless for entrypoint-less images (e.g. `docker import`ed rootfs).
    if !config.command.is_empty() {
        args.push("--entrypoint".to_string());
        args.push(String::new());
    }

    for m in &config.mounts {
        let ro = if m.readonly { ":ro" } else { "" };
        args.push("-v".to_string());
        args.push(format!(
            "{}:{}{ro}",
            m.source.display(),
            m.destination.display()
        ));
    }

    for (k, v) in &config.env {
        args.push("-e".to_string());
        args.push(format!("{k}={v}"));
    }

    if let Some(wd) = &config.workdir {
        args.push("-w".to_string());
        args.push(wd.display().to_string());
    }

    args.push(image.to_string());
    if !config.command.is_empty() {
        args.push(config.command.clone());
        args.extend(config.args.clone());
    }

    args
}

#[cfg(target_os = "linux")]
pub mod linux {
    use crate::runtime::SpawnConfig;
    use anyhow::{bail, Context, Result};
    use tokio::process::Command;

    /// Run an OCI image via `docker run -d --runtime=<runtime> --network host`.
    /// Returns the docker container name.
    pub async fn run(runtime: &str, config: &SpawnConfig) -> Result<String> {
        let image = config.image.clone().with_context(|| {
            format!(
                "isolation `{runtime}` needs an `image` (OCI image ref); Tenement runs it via \
                 `docker run --runtime={runtime}`. Render `image = \"...\"` in the service config."
            )
        })?;
        let name = format!("ten-{}", uuid::Uuid::new_v4().simple());

        let mut cmd = Command::new("docker");
        cmd.args(super::docker_run_args(runtime, &name, &image, config));

        let out = cmd
            .output()
            .await
            .with_context(|| format!("invoking `docker run --runtime={runtime}`"))?;
        if !out.status.success() {
            bail!(
                "docker run --runtime={runtime} failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::docker_run_args;
    use crate::runtime::SpawnConfig;
    use std::path::PathBuf;

    #[test]
    fn docker_args_include_resource_limits() {
        let config = SpawnConfig {
            command: "python".into(),
            args: vec!["app.py".into()],
            workdir: Some(PathBuf::from("/app")),
            image: Some("tinyhost/app:abc".into()),
            memory_limit_mb: Some(256),
            cpu_shares: Some(500),
            ..Default::default()
        };

        let args = docker_run_args("quark", "ten-test", "tinyhost/app:abc", &config);
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--memory" && w[1] == "256m"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--cpu-shares" && w[1] == "500"));
        assert!(args.contains(&"--entrypoint".to_string()));
        assert_eq!(args.last(), Some(&"app.py".to_string()));
    }
}
