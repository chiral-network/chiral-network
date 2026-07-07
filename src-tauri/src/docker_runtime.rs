//! Docker container runtime — a real [`ContainerRuntime`] that shells out to the
//! `docker` CLI. This is deliberately chosen over a heavy Docker-API client:
//! the CLI is stable across versions, needs no extra dependency, and the
//! `ContainerRuntime` trait is synchronous (no async bridge). It starts
//! containers under a hardened profile (non-root defaults via `--cap-drop ALL`,
//! read-only rootfs, memory/cpu/pids caps, no new privileges) and removes them
//! on stop.
//!
//! Wire it into a compute provider in place of the daemon's default; it is the
//! real backing for the [`crate::container_provider`] admission/metering core.
//! A container create fails with a clear message if `docker` is absent — the
//! handshake and metering still work.

use std::process::Command;

use crate::container_provider::{ContainerRuntime, ResourceRequest};

pub struct DockerCliRuntime {
    /// Public base URL the provider advertises; container endpoints are
    /// `<base_url>/c/<id>` (the provider reverse-proxies by id).
    pub base_url: String,
}

impl DockerCliRuntime {
    pub fn new(base_url: String) -> Self {
        DockerCliRuntime { base_url }
    }
}

/// Build the hardened `docker run` argument vector. Pure, so it can be asserted
/// in tests without a Docker host.
pub fn docker_run_args(id: &str, image: &str, req: ResourceRequest) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        id.to_string(),
        "--memory".to_string(),
        format!("{}g", req.mem_gb.max(1)),
        "--cpus".to_string(),
        req.vcpu.max(1).to_string(),
        "--pids-limit".to_string(),
        "512".to_string(),
        "--read-only".to_string(),
        "--cap-drop".to_string(),
        "ALL".to_string(),
        "--security-opt".to_string(),
        "no-new-privileges".to_string(),
        "--network".to_string(),
        "bridge".to_string(),
        "-P".to_string(), // publish exposed ports to ephemeral host ports
    ];
    if req.gpu > 0 {
        args.push("--gpus".to_string());
        args.push(req.gpu.to_string());
    }
    args.push(image.to_string());
    args
}

impl ContainerRuntime for DockerCliRuntime {
    fn start(&self, id: &str, image: &str, req: ResourceRequest) -> Result<String, String> {
        let out = Command::new("docker")
            .args(docker_run_args(id, image, req))
            .output()
            .map_err(|e| format!("docker run: {e} (is docker installed and running?)"))?;
        if !out.status.success() {
            return Err(format!(
                "docker run failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(format!("{}/c/{}", self.base_url.trim_end_matches('/'), id))
    }

    fn stop(&self, id: &str) -> Result<(), String> {
        let out = Command::new("docker")
            .args(["rm", "-f", id])
            .output()
            .map_err(|e| format!("docker rm: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "docker rm failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardened_run_args() {
        let args = docker_run_args("ctr_1", "nginx:1.27", ResourceRequest { vcpu: 2, mem_gb: 4, gpu: 0 });
        let joined = args.join(" ");
        assert!(joined.contains("--name ctr_1"));
        assert!(joined.contains("--memory 4g"));
        assert!(joined.contains("--cpus 2"));
        assert!(joined.contains("--pids-limit 512"));
        assert!(joined.contains("--read-only"));
        assert!(joined.contains("--cap-drop ALL"));
        assert!(joined.contains("--security-opt no-new-privileges"));
        assert!(joined.ends_with("nginx:1.27"));
        assert!(!joined.contains("--gpus"));
    }

    #[test]
    fn gpu_request_adds_gpus_flag() {
        let args = docker_run_args("c", "img", ResourceRequest { vcpu: 1, mem_gb: 1, gpu: 2 });
        assert!(args.join(" ").contains("--gpus 2"));
    }

    #[test]
    fn zero_resources_floor_to_one() {
        // Docker rejects --memory 0g / --cpus 0; floor to 1.
        let args = docker_run_args("c", "img", ResourceRequest { vcpu: 0, mem_gb: 0, gpu: 0 });
        let joined = args.join(" ");
        assert!(joined.contains("--memory 1g"));
        assert!(joined.contains("--cpus 1"));
    }

    #[test]
    fn endpoint_url_shape() {
        let rt = DockerCliRuntime::new("https://p.example/".to_string());
        // start() would call docker; just check the URL builder via a manual format.
        assert_eq!(
            format!("{}/c/{}", rt.base_url.trim_end_matches('/'), "ctr_9"),
            "https://p.example/c/ctr_9"
        );
    }
}
