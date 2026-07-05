//! Container provider — the compute data-plane's admission (resource-envelope
//! enforcement), runtime metering, and lifecycle bookkeeping. The actual OCI
//! runtime (Docker/Podman via `bollard`) is driven through the
//! [`ContainerRuntime`] trait at the wire layer; this module owns the billing
//! and admission logic, which is what must be exact.
//!
//! Design: `docs/chiral-book.md` → "Data-Plane API: Compute (Containers)" and
//! "Provider Implementation" → Container. A contract carries a resource
//! envelope (max vcpu / mem / gpu); a contract may run several containers so
//! long as their sum fits, and runtime is billed per second against the ledger.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Per-hour prices for the compute dimensions (wei).
#[derive(Debug, Clone, Copy)]
pub struct ContainerRates {
    pub per_vcpu_hour_wei: u128,
    pub per_gb_mem_hour_wei: u128,
    pub per_gpu_hour_wei: u128,
}

/// The resource shape a contract may consume (its `terms.params` envelope).
#[derive(Debug, Clone, Copy)]
pub struct ResourceEnvelope {
    pub max_vcpu: u32,
    pub max_mem_gb: u32,
    pub max_gpu: u32,
}

/// A container's requested resources.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ResourceRequest {
    pub vcpu: u32,
    pub mem_gb: u32,
    #[serde(default)]
    pub gpu: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerError {
    EnvelopeExceeded,
    NotFound,
}

/// The runtime driver the wire layer implements with `bollard` (create/start/
/// stop/logs/stats). Kept as a trait so the billing core is testable without a
/// Docker host.
pub trait ContainerRuntime {
    fn start(&self, id: &str, image: &str, req: ResourceRequest) -> Result<String, String>; // -> endpoint
    fn stop(&self, id: &str) -> Result<(), String>;
}

#[derive(Debug, Clone)]
struct Running {
    contract_id: String,
    req: ResourceRequest,
    last_metered_at: u64,
}

/// Tracks a provider's running containers, enforces per-contract envelopes, and
/// meters runtime.
#[derive(Debug)]
pub struct ContainerProvider {
    running: HashMap<String, Running>,
    pub rates: ContainerRates,
}

impl ContainerProvider {
    pub fn new(rates: ContainerRates) -> Self {
        ContainerProvider {
            running: HashMap::new(),
            rates,
        }
    }

    /// Admit a container for `contract_id` if it fits the envelope alongside the
    /// contract's already-running containers. `id` is supplied by the caller
    /// (the wire layer generates it); the caller starts the real container only
    /// after admission succeeds.
    pub fn admit(
        &mut self,
        contract_id: &str,
        id: &str,
        req: ResourceRequest,
        envelope: ResourceEnvelope,
        now_unix: u64,
    ) -> Result<(), ContainerError> {
        let (mut vcpu, mut mem, mut gpu) = self.contract_usage(contract_id);
        vcpu += req.vcpu;
        mem += req.mem_gb;
        gpu += req.gpu;
        if vcpu > envelope.max_vcpu || mem > envelope.max_mem_gb || gpu > envelope.max_gpu {
            return Err(ContainerError::EnvelopeExceeded);
        }
        self.running.insert(
            id.to_string(),
            Running {
                contract_id: contract_id.to_lowercase(),
                req,
                last_metered_at: now_unix,
            },
        );
        Ok(())
    }

    /// Cost of running `req` for `seconds`, ceil-rounded per dimension.
    pub fn cost_wei(&self, req: ResourceRequest, seconds: u64) -> u128 {
        let s = seconds as u128;
        ceil_div(req.vcpu as u128 * self.rates.per_vcpu_hour_wei * s, 3600)
            + ceil_div(req.mem_gb as u128 * self.rates.per_gb_mem_hour_wei * s, 3600)
            + ceil_div(req.gpu as u128 * self.rates.per_gpu_hour_wei * s, 3600)
    }

    /// Charge the runtime accrued for one container since it was last metered,
    /// advancing its meter clock. Returns `(contract_id, cost_wei)`.
    pub fn charge_since(&mut self, id: &str, now_unix: u64) -> Result<(String, u128), ContainerError> {
        let c = self.running.get_mut(id).ok_or(ContainerError::NotFound)?;
        let seconds = now_unix.saturating_sub(c.last_metered_at);
        let cost = cost_of(&self.rates, c.req, seconds);
        c.last_metered_at = now_unix;
        Ok((c.contract_id.clone(), cost))
    }

    /// Meter every running container, returning per-container charges for the
    /// caller to apply via the ledger.
    pub fn meter_all(&mut self, now_unix: u64) -> Vec<(String, String, u128)> {
        let ids: Vec<String> = self.running.keys().cloned().collect();
        let mut out = Vec::new();
        for id in ids {
            if let Ok((contract, cost)) = self.charge_since(&id, now_unix) {
                out.push((id, contract, cost));
            }
        }
        out
    }

    /// Stop a container, returning its `(contract_id, final_cost_wei)` (the
    /// runtime accrued since it was last metered).
    pub fn stop(&mut self, id: &str, now_unix: u64) -> Result<(String, u128), ContainerError> {
        let c = self.running.remove(id).ok_or(ContainerError::NotFound)?;
        let seconds = now_unix.saturating_sub(c.last_metered_at);
        Ok((c.contract_id, cost_of(&self.rates, c.req, seconds)))
    }

    pub fn running_count(&self, contract_id: &str) -> usize {
        let id = contract_id.to_lowercase();
        self.running.values().filter(|c| c.contract_id == id).count()
    }

    fn contract_usage(&self, contract_id: &str) -> (u32, u32, u32) {
        let id = contract_id.to_lowercase();
        self.running
            .values()
            .filter(|c| c.contract_id == id)
            .fold((0, 0, 0), |(v, m, g), c| {
                (v + c.req.vcpu, m + c.req.mem_gb, g + c.req.gpu)
            })
    }
}

fn cost_of(rates: &ContainerRates, req: ResourceRequest, seconds: u64) -> u128 {
    let s = seconds as u128;
    ceil_div(req.vcpu as u128 * rates.per_vcpu_hour_wei * s, 3600)
        + ceil_div(req.mem_gb as u128 * rates.per_gb_mem_hour_wei * s, 3600)
        + ceil_div(req.gpu as u128 * rates.per_gpu_hour_wei * s, 3600)
}

fn ceil_div(n: u128, d: u128) -> u128 {
    if d == 0 {
        return 0;
    }
    n.div_ceil(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rates() -> ContainerRates {
        ContainerRates {
            per_vcpu_hour_wei: 3_600_000, // 1000 wei/sec per vcpu
            per_gb_mem_hour_wei: 360_000, // 100 wei/sec per GB
            per_gpu_hour_wei: 36_000_000, // 10000 wei/sec per gpu
        }
    }
    fn envelope() -> ResourceEnvelope {
        ResourceEnvelope { max_vcpu: 4, max_mem_gb: 8, max_gpu: 1 }
    }
    fn req(vcpu: u32, mem: u32, gpu: u32) -> ResourceRequest {
        ResourceRequest { vcpu, mem_gb: mem, gpu }
    }

    #[test]
    fn admit_within_envelope_and_reject_over() {
        let mut c = ContainerProvider::new(rates());
        c.admit("k", "ctr1", req(2, 4, 0), envelope(), 0).unwrap();
        // Second container fits (2+2<=4, 4+4<=8).
        c.admit("k", "ctr2", req(2, 4, 0), envelope(), 0).unwrap();
        assert_eq!(c.running_count("k"), 2);
        // Third would exceed vcpu.
        assert_eq!(
            c.admit("k", "ctr3", req(1, 0, 0), envelope(), 0).unwrap_err(),
            ContainerError::EnvelopeExceeded
        );
    }

    #[test]
    fn gpu_envelope_enforced() {
        let mut c = ContainerProvider::new(rates());
        c.admit("k", "g1", req(1, 1, 1), envelope(), 0).unwrap();
        assert_eq!(
            c.admit("k", "g2", req(1, 1, 1), envelope(), 0).unwrap_err(),
            ContainerError::EnvelopeExceeded // only 1 gpu in envelope
        );
    }

    #[test]
    fn runtime_cost_math() {
        let c = ContainerProvider::new(rates());
        // 2 vcpu for 3600s = 2 * per_vcpu_hour; 4 GB likewise.
        assert_eq!(
            c.cost_wei(req(2, 4, 0), 3600),
            2 * 3_600_000 + 4 * 360_000
        );
        // Sub-second-rate ceilings never undercharge.
        assert!(c.cost_wei(req(1, 0, 0), 1) >= 1);
    }

    #[test]
    fn charge_since_advances_meter() {
        let mut c = ContainerProvider::new(rates());
        c.admit("k", "ctr", req(1, 0, 0), envelope(), 0).unwrap();
        let (contract, cost1) = c.charge_since("ctr", 3600).unwrap();
        assert_eq!(contract, "k");
        assert_eq!(cost1, 3_600_000); // 1 vcpu * 1hr
        // Immediately metering again charges nothing (clock advanced).
        let (_, cost2) = c.charge_since("ctr", 3600).unwrap();
        assert_eq!(cost2, 0);
    }

    #[test]
    fn stop_charges_tail_and_removes() {
        let mut c = ContainerProvider::new(rates());
        c.admit("k", "ctr", req(1, 0, 0), envelope(), 0).unwrap();
        c.charge_since("ctr", 1800).unwrap(); // meter to 1800s
        let (contract, tail) = c.stop("ctr", 3600).unwrap(); // 1800s remain
        assert_eq!(contract, "k");
        assert_eq!(tail, 1_800_000); // 1 vcpu * 1800s * 1000wei/s
        assert_eq!(c.running_count("k"), 0);
        assert_eq!(c.stop("ctr", 4000).unwrap_err(), ContainerError::NotFound);
    }

    #[test]
    fn meter_all_covers_every_container() {
        let mut c = ContainerProvider::new(rates());
        c.admit("k", "a", req(1, 0, 0), envelope(), 0).unwrap();
        c.admit("k", "b", req(1, 0, 0), envelope(), 0).unwrap();
        let charges = c.meter_all(3600);
        assert_eq!(charges.len(), 2);
        assert!(charges.iter().all(|(_, contract, cost)| contract == "k" && *cost == 3_600_000));
    }
}
