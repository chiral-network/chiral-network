//! Fixed warm-generation SLA. Evidence is an operator declaration, not execution attestation.
use crate::protocol::{self, EngineDevice, ModelSpec, PerformanceOutcome, ServingProfile, TokenUsage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const POLICY: &str = "warm-generation-v1";
pub const TTFT_US: u64 = 2_000_000;

pub fn validate_profile(model: &ModelSpec, p: &ServingProfile) -> Result<(), String> {
    let valid_text = |s: &str| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control);
    for text in [&p.hf_repo, &p.tokenizer_repo, &p.engine_version, &p.region] {
        if !valid_text(text) {
            return Err("invalid serving deployment identity".into());
        }
    }
    for digest in [&p.artifact_manifest_sha256, &p.runtime_sha256, &p.qualification.evidence_sha256] {
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err("serving evidence, runtime and artifact require lowercase SHA256 digests".into());
        }
    }
    let capacity = match p.capacity_tier.as_str() {
        "8gb" => 7500, "16gb" => 15000, "32gb" => 30000, "higher" => 45000,
        _ => return Err("unknown capacity tier".into()),
    };
    let platform_backend = match p.platform.as_str() {
        "linux" | "windows" => matches!(p.backend.as_str(), "cuda" | "vulkan"),
        "macos" => p.backend == "metal",
        _ => false,
    };
    let unified = p.memory_kind == "unified";
    if !platform_backend
        || !matches!(p.memory_kind.as_str(), "dedicated" | "unified")
        || (p.backend == "metal" && !unified)
        || (p.backend == "cuda" && unified)
        || p.devices.is_empty() || p.devices.len() > 64
        || (unified && p.devices.len() != 1)
        || p.memory_reserve_mib < if unified { 4096 } else { 256 } {
        return Err("unsupported platform/backend or memory headroom policy".into());
    }
    let mut ids = std::collections::HashSet::new();
    let mut selectors = std::collections::HashSet::new();
    for device in &p.devices {
        if [&device.id, &device.name, &device.driver_version, &device.engine_device]
            .iter().any(|text| !valid_text(text))
            || !ids.insert(&device.id) || !selectors.insert(&device.engine_device)
            || device.memory_mib <= p.memory_reserve_mib || device.memory_mib > (1 << 30) {
            return Err("physical device inventory or available memory is invalid".into());
        }
    }
    let capacity_memory = if p.capacity_tier == "higher" {
        p.devices.iter().map(|d| d.memory_mib).sum()
    } else {
        p.devices.iter().map(|d| d.memory_mib).min().unwrap_or(0)
    };
    let cache_type = |s: &str| matches!(s,
        "f32" | "f16" | "bf16" | "q8_0" | "q4_0" | "q4_1" | "iq4_nl" | "q5_0" | "q5_1");
    if p.engine != "llama.cpp" || !(1..=999).contains(&p.gpu_layers)
        || !matches!(p.split_mode.as_str(), "none" | "layer" | "row")
        || (p.devices.len() == 1 && (p.split_mode != "none" || !p.tensor_split.is_empty()))
        || (p.devices.len() > 1 && (p.split_mode == "none" || p.tensor_split.len() != p.devices.len()
            || p.tensor_split.iter().any(|weight| *weight == 0 || *weight > 1_000_000)))
        || !cache_type(&p.cache_type_k) || !cache_type(&p.cache_type_v)
        || (!matches!(p.cache_type_v.as_str(), "f32" | "f16" | "bf16") && !p.flash_attention)
        || !(1..=8192).contains(&p.batch_size) || p.ubatch_size == 0 || p.ubatch_size > p.batch_size
        || capacity_memory < capacity || p.max_concurrency == 0 || p.max_concurrency > 256
        || p.arrival_milli_rps == 0 || p.arrival_milli_rps > 1_000_000
        || !matches!(p.output_tokens_per_second, 20 | 50 | 100) || p.policy != POLICY
        || p.qualification.measured_at == 0 || model.context_length < 4096
        || model.max_output_tokens < 256
        || !model.capabilities.iter().any(|c| matches!(c.as_str(), "chat" | "completions"))
        || model.capabilities.iter().any(|c| c == "embeddings") {
        return Err("unsupported serving settings, capacity, workload or performance policy".into());
    }
    Ok(())
}

/// Qualification identity is omitted to avoid a circular evidence hash.
pub fn profile_sha256(model: &ModelSpec) -> Result<String, String> {
    let mut value = serde_json::to_value(model).map_err(|e| e.to_string())?;
    if let Some(profile) = value.get_mut("serving").and_then(|v| v.as_object_mut()) {
        profile.remove("qualification");
    }
    Ok(hex::encode(Sha256::digest(protocol::canonical_json(&value)?)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub schema: String,
    pub profile_sha256: String,
    pub runtime_identity_sha256: String,
    pub runtime_observation: RuntimeObservation,
    pub measured_at: u64,
    pub probe_location: String,
    pub transport: String,
    pub sweeps: Vec<Sweep>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeObservation {
    pub pid: u32,
    pub engine_url: String,
    pub model_id: String,
    pub context_length: u64,
    pub max_concurrency: u32,
    pub artifact_manifest_sha256: String,
    pub runtime_sha256: String,
    pub engine_version: String,
    pub platform: String,
    pub backend: String,
    pub memory_kind: String,
    pub devices: Vec<EngineDevice>,
    pub memory_reserve_mib: u64,
    pub gpu_layers: u32,
    pub split_mode: String,
    pub tensor_split: Vec<u32>,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub flash_attention: bool,
    pub batch_size: u32,
    pub ubatch_size: u32,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sweep {
    pub path: String,
    pub concurrency: u32,
    pub observed_concurrency: u32,
    pub arrival_milli_rps: u32,
    pub duration_ms: u64,
    pub memory_metric: String,
    pub peak_memory_mib: u64,
    pub min_available_memory_mib: u64,
    pub requests: Vec<Sample>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    pub prompt_tokens: u64,
    pub output_tokens: u64,
    pub ttft_us: u64,
    pub decode_us: u64,
    pub elapsed_us: u64,
    pub error: bool,
    pub usage_known: bool,
    pub endpoint: String,
    pub client_stream: bool,
    pub prompt_sha256: String,
    pub external_ttft_us: Option<u64>,
    pub external_decode_us: Option<u64>,
    pub external_elapsed_us: u64,
}
#[derive(Debug, Serialize)]
pub struct Percentiles { pub p50_us: u64, pub p95_us: u64, pub p99_us: u64 }
fn percentiles(mut values: Vec<u64>) -> Percentiles {
    values.sort_unstable();
    let at = |p: usize| values[(values.len() * p).div_ceil(100).saturating_sub(1)];
    Percentiles { p50_us: at(50), p95_us: at(95), p99_us: at(99) }
}
#[derive(Debug, Serialize)]
pub struct SweepSummary {
    pub path: String, pub concurrency: u32, pub requests: usize, pub errors: usize,
    pub jointly_attained: usize, pub goodput_milli_rps: u64,
    pub ttft: Percentiles, pub tpot: Percentiles, pub latency: Percentiles,
    pub external_ttft: Option<Percentiles>,
    pub external_tpot: Option<Percentiles>,
    pub external_latency: Percentiles,
}
pub fn validate_evidence(model: &ModelSpec, path: &Path, now: u64) -> Result<Vec<SweepSummary>, String> {
    let p = model.serving.as_ref().ok_or("missing serving profile")?;
    validate_profile(model, p)?;
    let file = std::fs::File::open(path).map_err(|_| "qualification evidence unavailable")?;
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| "cannot read qualification evidence")?;
    if bytes.len() > 64 * 1024 * 1024 || hex::encode(Sha256::digest(&bytes)) != p.qualification.evidence_sha256 {
        return Err("qualification evidence size/hash mismatch".into());
    }
    let report: Evidence = serde_json::from_slice(&bytes).map_err(|_| "invalid qualification evidence")?;
    let identity = profile_sha256(model)?;
    if report.schema != "chiral-performance-evidence-v2" || report.profile_sha256 != identity
        || report.measured_at != p.qualification.measured_at
        || report.measured_at > now
        || report.probe_location.is_empty() || report.probe_location.len() > 256
        || report.transport.is_empty() || report.transport.len() > 256
        || report.sweeps.len() != 2 * p.max_concurrency as usize {
        return Err("qualification identity, timestamp or sweep coverage mismatch".into());
    }
    let runtime = &report.runtime_observation;
    let runtime_digest = hex::encode(Sha256::digest(protocol::canonical_json(runtime)?));
    let runtime_origin = crate::provider_config::parse_origin(&runtime.engine_url)?;
    let loopback = match runtime_origin.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if runtime.pid == 0 || !loopback || runtime_origin.scheme() != "http"
        || report.runtime_identity_sha256 != runtime_digest
        || runtime.model_id != model.id || runtime.context_length != model.context_length
        || runtime.max_concurrency != p.max_concurrency
        || runtime.runtime_sha256 != p.runtime_sha256
        || runtime.artifact_manifest_sha256 != p.artifact_manifest_sha256
        || runtime.engine_version != p.engine_version
        || runtime.platform != p.platform || runtime.backend != p.backend
        || runtime.memory_kind != p.memory_kind || runtime.devices != p.devices
        || runtime.memory_reserve_mib != p.memory_reserve_mib
        || runtime.gpu_layers != p.gpu_layers || runtime.split_mode != p.split_mode
        || runtime.tensor_split != p.tensor_split || runtime.cache_type_k != p.cache_type_k
        || runtime.cache_type_v != p.cache_type_v || runtime.flash_attention != p.flash_attention
        || runtime.batch_size != p.batch_size || runtime.ubatch_size != p.ubatch_size {
        return Err("observed qualification runtime differs from signed deployment".into());
    }
    let total_memory: u64 = p.devices.iter().map(|device| device.memory_mib).sum();
    let memory_budget = total_memory - p.memory_reserve_mib * p.devices.len() as u64;
    let memory_metric = if p.memory_kind == "unified" { "system_used" } else { "device_used" };
    let mut seen = std::collections::HashSet::new();
    let mut prompts = std::collections::HashSet::new();
    let mut summaries = Vec::with_capacity(report.sweeps.len());
    for s in report.sweeps {
        if !matches!(s.path.as_str(), "engine" | "gateway") || s.concurrency == 0
            || s.concurrency > p.max_concurrency || !seen.insert((s.path.clone(), s.concurrency))
            || s.observed_concurrency != s.concurrency
            || s.arrival_milli_rps != p.arrival_milli_rps || !(100..=10000).contains(&s.requests.len())
            || s.duration_ms == 0 || s.peak_memory_mib == 0 || s.memory_metric != memory_metric
            || s.peak_memory_mib > memory_budget
            || s.min_available_memory_mib < p.memory_reserve_mib
            || s.min_available_memory_mib > p.devices.iter().map(|d| d.memory_mib).min().unwrap_or(0) {
            return Err("invalid qualification workload coverage".into());
        }
        // A declared rate without sustained offered load is not qualification.
        let offered = (s.requests.len() as u128) * 1_000_000;
        let declared = (s.arrival_milli_rps as u128) * s.duration_ms as u128;
        if offered * 100 < declared * 95 || offered * 100 > declared * 105 {
            return Err("qualification did not exercise declared sustained arrival rate".into());
        }
        for r in &s.requests {
            if !matches!(r.endpoint.as_str(), "chat" | "completions")
                || !model.capabilities.contains(&r.endpoint)
                || r.prompt_sha256.len() != 64
                || !r.prompt_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || !prompts.insert(r.prompt_sha256.clone())
                || (s.path == "engine" && !r.client_stream)
                || (!r.error && r.client_stream && (r.external_ttft_us.is_none() || r.external_decode_us.is_none())) {
                return Err("qualification endpoint, prompt uniqueness or external timing missing".into());
            }
            // Every qualification prompt sits at a boundary, at most eight tokens below it.
            if ![128u64, 512, 1024, 2048].iter().any(|target| (target - 8..=*target).contains(&r.prompt_tokens)) {
                return Err("qualification prompt is outside every prompt-size boundary".into());
            }
        }
        let passes = |r: &&Sample| !r.error && r.usage_known && r.prompt_tokens > 0 && r.prompt_tokens <= 2048
            && r.output_tokens == 256 && r.ttft_us > 0 && r.ttft_us <= TTFT_US && r.decode_us > 0
            && r.decode_us as u128 * p.output_tokens_per_second as u128 <= 255_000_000
            && r.elapsed_us >= r.ttft_us.saturating_add(r.decode_us);
        for endpoint in model.capabilities.iter().filter(|c| matches!(c.as_str(), "chat" | "completions")) {
            for streaming in [true, false] {
                if s.path == "engine" && !streaming { continue; }
                let matching = || s.requests.iter().filter(move |r| &r.endpoint == endpoint && r.client_stream == streaming);
                let count = matching().count();
                if count < 100 || matching().filter(passes).count() * 100 < count * 95 {
                    return Err("qualification must attain class on every advertised endpoint and response mode".into());
                }
                for target in [128u64, 512, 1024, 2048] {
                    let bucket = || matching().filter(move |r| (target - 8..=target).contains(&r.prompt_tokens));
                    let count = bucket().count();
                    if count < 20 || bucket().filter(passes).count() * 100 < count * 95 {
                        return Err("qualification must attain every prompt-size boundary through 2048 tokens".into());
                    }
                }
            }
        }
        let optional_percentiles = |values: Vec<u64>| if values.is_empty() { None } else { Some(percentiles(values)) };
        let joint = s.requests.iter().filter(passes).count();
        if joint * 100 < s.requests.len() * 95 { return Err("requested speed class failed joint 95% attainment".into()); }
        summaries.push(SweepSummary {
            path: s.path, concurrency: s.concurrency, requests: s.requests.len(),
            errors: s.requests.iter().filter(|r| r.error).count(), jointly_attained: joint,
            goodput_milli_rps: (joint as u64).saturating_mul(1_000_000) / s.duration_ms,
            ttft: percentiles(s.requests.iter().filter(|r| !r.error).map(|r| r.ttft_us).collect()),
            tpot: percentiles(s.requests.iter().filter(|r| !r.error).map(|r| r.decode_us / r.output_tokens.saturating_sub(1).max(1)).collect()),
            latency: percentiles(s.requests.iter().filter(|r| !r.error).map(|r| r.elapsed_us).collect()),
            external_ttft: optional_percentiles(s.requests.iter().filter(|r| !r.error).filter_map(|r| r.external_ttft_us).collect()),
            external_tpot: optional_percentiles(s.requests.iter().filter(|r| !r.error && r.output_tokens > 1).filter_map(|r| r.external_decode_us.map(|t| t / (r.output_tokens - 1))).collect()),
            external_latency: percentiles(s.requests.iter().filter(|r| !r.error).map(|r| r.external_elapsed_us).collect()),
        });
    }
    Ok(summaries)
}

pub fn outcome(p: &ServingProfile, in_envelope: bool, ttft_us: Option<u64>, decode_us: Option<u64>,
    usage: Option<&TokenUsage>, status: &str, client_stalled: bool, elapsed_us: Option<u64>,
    evidence_sha256: &str) -> PerformanceOutcome {
    let tokens = usage.map(|u| u.completion_tokens);
    // A nonpositive TTFT is no evidence of attainment: it counts as a missing first token.
    let first_token = ttft_us.filter(|t| *t > 0);
    let ttft_missed = first_token.is_some_and(|t| t > TTFT_US)
        || (first_token.is_none() && elapsed_us.is_some_and(|t| t > TTFT_US));
    let client = client_stalled || matches!(status, "cancelled" | "disconnected");
    let short = tokens.is_some_and(|n| n < 2);
    let decode_ok = match (decode_us, tokens) {
        (Some(t), Some(n)) if n >= 2 => Some(t > 0 && t as u128 * p.output_tokens_per_second as u128 <= (n - 1) as u128 * 1_000_000),
        _ => None,
    };
    let measured_miss = ttft_missed || decode_ok == Some(false);
    // The gateway settles every client cancellation, disconnect or stall it can
    // observe as such. `interrupted` remains only for what it cannot attribute
    // (a task panic, runtime shutdown), which is provider-incomplete.
    let (eligibility, attained) = if !in_envelope { ("outside_workload", None) }
        else if measured_miss { (if short { "short_output_ttft_only" } else { "eligible" }, Some(false)) }
        else if client { ("client_interrupted", None) }
        else if status != "completed" || usage.is_none() || first_token.is_none() || (!short && decode_ok.is_none()) { ("provider_incomplete", Some(false)) }
        else if short { ("short_output_ttft_only", Some(true)) }
        else { ("eligible", Some(true)) };
    PerformanceOutcome { evidence_sha256: evidence_sha256.into(), policy: p.policy.clone(),
        eligibility: eligibility.into(), ttft_us, decode_us, elapsed_us, output_tokens: tokens, attained,
        remedy: if attained == Some(false) { "inference_waiver" } else { "none" }.into() }
}

#[cfg(test)]
pub(crate) fn test_profile() -> ServingProfile {
    ServingProfile {
        hf_repo: "test/model".into(), tokenizer_repo: "test/tokenizer".into(),
        artifact_manifest_sha256: "a".repeat(64),
        engine: "llama.cpp".into(), engine_version: "test-only".into(),
        runtime_sha256: "b".repeat(64), platform: "linux".into(),
        backend: "cuda".into(), memory_kind: "dedicated".into(),
        devices: vec![EngineDevice { id: "GPU-unit-test".into(), name: "synthetic-unit-test-device".into(),
            memory_mib: 32000, driver_version: "test".into(), engine_device: "CUDA0".into() }],
        memory_reserve_mib: 256, gpu_layers: 999, split_mode: "none".into(), tensor_split: vec![],
        cache_type_k: "f16".into(), cache_type_v: "f16".into(), flash_attention: true,
        batch_size: 512, ubatch_size: 128,
        capacity_tier: "32gb".into(), region: "test".into(), max_concurrency: 10,
        arrival_milli_rps: 1000, output_tokens_per_second: 20, policy: POLICY.into(),
        qualification: protocol::QualificationIdentity { evidence_sha256: "c".repeat(64), measured_at: 100 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_capacity_tier_requires_enough_vram_on_each_device() {
        let model = crate::offer_registry::tests::offer().payload.model;
        let mut profile = test_profile();
        profile.capacity_tier = "8gb".into();
        profile.devices[0].memory_mib = 7500;
        assert!(validate_profile(&model, &profile).is_ok());
        profile.devices[0].memory_mib = 7499;
        assert!(validate_profile(&model, &profile).is_err());
        profile.devices[0].memory_mib = 4096;
        let mut second = profile.devices[0].clone();
        second.id = "GPU-second-test".into();
        second.engine_device = "CUDA1".into();
        profile.devices.push(second);
        profile.split_mode = "layer".into();
        profile.tensor_split = vec![1, 1];
        assert!(validate_profile(&model, &profile).is_err());
    }

    #[test]
    fn unified_capacity_counts_shared_memory_once_and_keeps_os_headroom() {
        let model = crate::offer_registry::tests::offer().payload.model;
        let mut profile = test_profile();
        profile.platform = "macos".into();
        profile.backend = "metal".into();
        profile.memory_kind = "unified".into();
        profile.capacity_tier = "16gb".into();
        profile.devices[0].memory_mib = 16384;
        profile.devices[0].id = "apple-platform-identity".into();
        profile.devices[0].engine_device = "MTL0".into();
        profile.memory_reserve_mib = 4096;
        assert!(validate_profile(&model, &profile).is_ok());
        profile.memory_reserve_mib = 4095;
        assert!(validate_profile(&model, &profile).is_err());
        profile.memory_reserve_mib = 4096;
        let mut duplicate_memory = profile.devices[0].clone();
        duplicate_memory.id.push('2');
        duplicate_memory.engine_device = "MTL1".into();
        profile.devices.push(duplicate_memory);
        profile.split_mode = "layer".into();
        profile.tensor_split = vec![1, 1];
        assert!(validate_profile(&model, &profile).is_err());
    }

    #[test]
    fn inclusive_joint_boundaries_use_authoritative_tokens_not_events() {
        for rate in [20, 50, 100] {
            let mut profile = test_profile();
            profile.output_tokens_per_second = rate;
            let usage = TokenUsage { prompt_tokens: 2048, completion_tokens: 256 };
            let decode = 255_000_000 / u64::from(rate);
            let result = outcome(&profile, true, Some(TTFT_US), Some(decode), Some(&usage), "completed", false, Some(TTFT_US + decode), &(&profile).qualification.evidence_sha256);
            assert_eq!(result.attained, Some(true));
            assert_eq!(result.remedy, "none");
            for (ttft, elapsed) in [(TTFT_US + 1, decode), (TTFT_US, decode + 1), (1, 0)] {
                let missed = outcome(&profile, true, Some(ttft), Some(elapsed), Some(&usage), "completed", false, None, &(&profile).qualification.evidence_sha256);
                assert_eq!(missed.attained, Some(false));
                assert_eq!(missed.remedy, "inference_waiver");
            }
        }
    }

    #[test]
    fn zero_ttft_is_no_evidence_of_attainment() {
        let p = test_profile();
        let usage = TokenUsage { prompt_tokens: 128, completion_tokens: 256 };
        let zero = outcome(&p, true, Some(0), Some(1_000_000), Some(&usage), "completed", false, Some(1_000_000), &p.qualification.evidence_sha256);
        assert_eq!(zero.eligibility, "provider_incomplete");
        assert_eq!(zero.attained, Some(false));
        assert_eq!(zero.remedy, "inference_waiver");
        assert_eq!(zero.ttft_us, Some(0));
        let short = TokenUsage { prompt_tokens: 128, completion_tokens: 1 };
        assert_eq!(outcome(&p, true, Some(0), None, Some(&short), "completed", false, Some(1), &p.qualification.evidence_sha256).attained, Some(false));
        // A client interruption before any measured miss is still not counted.
        assert_eq!(outcome(&p, true, Some(0), None, None, "disconnected", false, Some(1), &p.qualification.evidence_sha256).attained, None);
    }

    #[test]
    fn exclusions_do_not_erase_a_previously_observed_miss() {
        let p = test_profile();
        let one = TokenUsage { prompt_tokens: 1, completion_tokens: 1 };
        let short = outcome(&p, true, Some(TTFT_US), None, Some(&one), "completed", false, None, &(&p).qualification.evidence_sha256);
        assert_eq!(short.eligibility, "short_output_ttft_only");
        assert_eq!(short.attained, Some(true));
        assert_eq!(outcome(&p, true, None, None, None, "cancelled", false, Some(1), &p.qualification.evidence_sha256).attained, None);
        assert_eq!(outcome(&p, true, None, None, None, "cancelled", false, Some(TTFT_US + 1), &p.qualification.evidence_sha256).remedy, "inference_waiver");
        assert_eq!(outcome(&p, true, Some(TTFT_US + 1), None, Some(&one), "cancelled", true, None, &p.qualification.evidence_sha256).remedy, "inference_waiver");
        let outside = outcome(&p, false, None, None, None, "timed_out", false, None, &(&p).qualification.evidence_sha256);
        assert_eq!(outside.eligibility, "outside_workload");
        assert_eq!(outside.remedy, "none");
        let unknown = outcome(&p, true, Some(1), None, None, "failed", false, None, &(&p).qualification.evidence_sha256);
        assert_eq!(unknown.output_tokens, None);
        assert_eq!(unknown.remedy, "inference_waiver");
        // An interruption the gateway cannot attribute is provider-incomplete,
        // never a separate eligibility class.
        let interrupted = outcome(&p, true, None, None, None, "interrupted", false, Some(1), &(&p).qualification.evidence_sha256);
        assert_eq!(interrupted.eligibility, "provider_incomplete");
        assert_eq!(interrupted.attained, Some(false));
        assert_eq!(interrupted.remedy, "inference_waiver");
        // A disconnect before a measured miss is the client's; after one, the miss stays.
        let disconnected = outcome(&p, true, None, None, None, "disconnected", false, Some(500_000), &p.qualification.evidence_sha256);
        assert_eq!(disconnected.eligibility, "client_interrupted");
        assert_eq!(disconnected.attained, None);
        assert_eq!(disconnected.remedy, "none");
        let late = outcome(&p, true, Some(2_100_000), None, None, "disconnected", false, Some(2_400_000), &p.qualification.evidence_sha256);
        assert_eq!(late.eligibility, "eligible");
        assert_eq!(late.attained, Some(false));
        assert_eq!(late.remedy, "inference_waiver");
    }

    #[test]
    fn evidence_renewal_cannot_change_any_agreed_model_or_serving_term() {
        let mut original = crate::offer_registry::tests::offer().payload.model;
        original.serving = Some(test_profile());
        let mut renewed = original.clone();
        renewed.serving.as_mut().unwrap().qualification.evidence_sha256 = "d".repeat(64);
        assert!(!original.accepts_execution(&renewed));
        renewed.serving.as_mut().unwrap().qualification.measured_at += 1;
        assert!(original.accepts_execution(&renewed));
        assert!(!renewed.accepts_execution(&original));
        fn mutate(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::String(text) => text.push('x'),
                serde_json::Value::Number(number) => *number = (number.as_u64().unwrap() + 1).into(),
                serde_json::Value::Bool(value) => *value = !*value,
                serde_json::Value::Array(values) if values.is_empty() => values.push(serde_json::json!(1)),
                serde_json::Value::Array(values) => mutate(&mut values[0]),
                serde_json::Value::Object(values) => mutate(values.values_mut().next().unwrap()),
                serde_json::Value::Null => *value = serde_json::json!("Q4_K_M"),
            }
        }
        let document = serde_json::to_value(&renewed).unwrap();
        for field in document.as_object().unwrap().keys().filter(|key| *key != "serving") {
            let mut changed = document.clone();
            mutate(&mut changed[field]);
            let changed: ModelSpec = serde_json::from_value(changed).unwrap();
            assert!(!original.accepts_execution(&changed), "changed model term: {field}");
        }
        for field in document["serving"].as_object().unwrap().keys().filter(|key| *key != "qualification") {
            let mut changed = document.clone();
            mutate(&mut changed["serving"][field]);
            let changed: ModelSpec = serde_json::from_value(changed).unwrap();
            assert!(!original.accepts_execution(&changed), "changed serving term: {field}");
        }
        renewed.serving = None;
        assert!(!original.accepts_execution(&renewed));
        assert!(!renewed.accepts_execution(&original));
    }

    #[test]
    fn capacity_is_not_speed_and_qualification_cannot_survive_setting_changes() {
        let mut model = crate::offer_registry::tests::offer().payload.model;
        model.serving = Some(test_profile());
        assert!(validate_profile(&model, model.serving.as_ref().unwrap()).is_ok());
        model.serving.as_mut().unwrap().engine = "sglang".into();
        assert!(validate_profile(&model, model.serving.as_ref().unwrap()).is_err());
        model.serving.as_mut().unwrap().engine = "llama.cpp".into();
        let before = profile_sha256(&model).unwrap();
        model.serving.as_mut().unwrap().qualification.measured_at += 1;
        assert_eq!(before, profile_sha256(&model).unwrap());
        model.serving.as_mut().unwrap().output_tokens_per_second = 100;
        assert_ne!(before, profile_sha256(&model).unwrap());
        model.serving.as_mut().unwrap().devices[0].memory_mib = 8000;
        assert!(validate_profile(&model, model.serving.as_ref().unwrap()).is_err());
        model.serving = Some(test_profile());
        for capabilities in [vec!["chat"], vec!["completions"], vec!["chat", "completions"]] {
            model.capabilities = capabilities.into_iter().map(str::to_owned).collect();
            assert!(validate_profile(&model, model.serving.as_ref().unwrap()).is_ok());
        }
        for capabilities in [vec![], vec!["tools", "structured_output"], vec!["streaming"], vec!["chat", "embeddings"]] {
            model.capabilities = capabilities.into_iter().map(str::to_owned).collect();
            assert!(validate_profile(&model, model.serving.as_ref().unwrap()).is_err());
        }
    }
}

#[cfg(test)]
pub(crate) mod evidence_tests {
    use super::*;
    use serde_json::{json, Value};

    // Synthetic parser/accounting inputs only, never a hardware qualification.
    pub(crate) fn fixture() -> (ModelSpec, Value) {
        let mut model = crate::offer_registry::tests::offer().payload.model;
        model.capabilities = vec!["chat".into()];
        let mut profile = test_profile();
        profile.max_concurrency = 1;
        profile.arrival_milli_rps = 50;
        model.serving = Some(profile.clone());
        let mut sweeps = Vec::new();
        for path in ["engine", "gateway"] {
            let mut requests = Vec::new();
            for streaming in [true, false] {
                if path == "engine" && !streaming { continue; }
                for index in 0..100 {
                    requests.push(json!({
                        "prompt_tokens": ([128,512,1024,2048][index % 4]),
                        "output_tokens":256,"ttft_us":1_000_000,"decode_us":12_750_000,
                        "elapsed_us":13_750_000,"error":false,"usage_known":true,
                        "endpoint":"chat","client_stream":streaming,
                        "prompt_sha256":hex::encode(Sha256::digest(format!("{path}-{streaming}-{index}").as_bytes())),
                        "external_ttft_us":if streaming {Some(1_001_000u64)} else {None},
                        "external_decode_us":if streaming {Some(12_750_000u64)} else {None},
                        "external_elapsed_us":13_751_000
                    }));
                }
            }
            sweeps.push(json!({"path":path,"concurrency":1,"observed_concurrency":1,
                "arrival_milli_rps":50,"duration_ms":requests.len() * 20_000,
                "memory_metric":"device_used","peak_memory_mib":16000,
                "min_available_memory_mib":16000,"requests":requests}));
        }
        let report = json!({
            "schema":"chiral-performance-evidence-v2","profile_sha256":"","runtime_identity_sha256":"",
            "measured_at":100,"probe_location":"synthetic unit fixture","transport":"not executed",
            "runtime_observation":{
                "pid":1234,"engine_url":"http://127.0.0.1:8000","model_id":model.id,
                "context_length":model.context_length,"max_concurrency":profile.max_concurrency,
                "runtime_sha256":profile.runtime_sha256,"devices":profile.devices,
                "platform":profile.platform,"backend":profile.backend,"memory_kind":profile.memory_kind,
                "memory_reserve_mib":profile.memory_reserve_mib,"gpu_layers":profile.gpu_layers,
                "split_mode":profile.split_mode,"tensor_split":profile.tensor_split,
                "cache_type_k":profile.cache_type_k,"cache_type_v":profile.cache_type_v,
                "flash_attention":profile.flash_attention,"batch_size":profile.batch_size,
                "ubatch_size":profile.ubatch_size,"artifact_manifest_sha256":profile.artifact_manifest_sha256,
                "engine_version":profile.engine_version
            },
            "sweeps":sweeps
        });
        (model, report)
    }

    pub(crate) fn persist(model: &mut ModelSpec, report: &mut Value, path: &Path) {
        let identity = profile_sha256(model).unwrap();
        report["profile_sha256"] = json!(identity);
        report["runtime_identity_sha256"] = json!(hex::encode(Sha256::digest(
            protocol::canonical_json(&report["runtime_observation"]).unwrap())));
        let bytes = protocol::canonical_json(report).unwrap();
        model.serving.as_mut().unwrap().qualification.evidence_sha256 = hex::encode(Sha256::digest(&bytes));
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn qualification_checks_joint_boundary_each_workload_and_actual_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-evidence.json");
        let (mut model, mut report) = fixture();
        persist(&mut model, &mut report, &path);
        assert!(validate_evidence(&model, &path, 100).is_ok());
        report["sweeps"][1]["requests"][0]["ttft_us"] = json!(2_000_001);
        persist(&mut model, &mut report, &path);
        assert!(validate_evidence(&model, &path, 100).is_ok());
        // Two misses in the same 25-request prompt bucket cannot hide in 200 samples.
        report["sweeps"][1]["requests"][4]["ttft_us"] = json!(2_000_001);
        persist(&mut model, &mut report, &path);
        assert!(validate_evidence(&model, &path, 100).is_err());
        let (mut model, mut report) = fixture();
        model.serving.as_mut().unwrap().output_tokens_per_second = 100;
        persist(&mut model, &mut report, &path);
        assert!(validate_evidence(&model, &path, 100).is_err());
        let (mut model, mut report) = fixture();
        report["runtime_observation"]["devices"][0]["memory_mib"] = json!(8000);
        persist(&mut model, &mut report, &path);
        assert!(validate_evidence(&model, &path, 100).is_err());
        let (mut model, mut report) = fixture();
        persist(&mut model, &mut report, &path);
        model.context_length = 8192;
        assert!(validate_evidence(&model, &path, 100).is_err());
    }

    #[test]
    fn zero_ttft_samples_do_not_attain() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-evidence.json");
        // Requests 0 and 4 of the gateway sweep share the 128-token bucket of 25 samples.
        let (mut model, mut report) = fixture();
        report["sweeps"][1]["requests"][0]["ttft_us"] = json!(0);
        persist(&mut model, &mut report, &path);
        assert!(validate_evidence(&model, &path, 100).is_ok());
        report["sweeps"][1]["requests"][4]["ttft_us"] = json!(0);
        persist(&mut model, &mut report, &path);
        assert!(validate_evidence(&model, &path, 100).is_err());
    }

    #[test]
    fn samples_outside_every_prompt_boundary_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-evidence.json");
        let (mut model, mut report) = fixture();
        let mut extra = report["sweeps"][1]["requests"][0].clone();
        extra["prompt_tokens"] = json!(300);
        extra["prompt_sha256"] = json!(hex::encode(Sha256::digest(b"outside-every-boundary")));
        let requests = report["sweeps"][1]["requests"].as_array_mut().unwrap();
        requests.push(extra);
        // Keep the offered rate exactly at the declared rate for the longer sweep.
        let count = requests.len() as u64;
        report["sweeps"][1]["duration_ms"] = json!(count * 20_000);
        persist(&mut model, &mut report, &path);
        let error = validate_evidence(&model, &path, 100).unwrap_err();
        assert!(error.contains("outside every prompt-size boundary"), "{error}");
        for boundary in [120u64, 128, 504, 2048] {
            let (mut model, mut report) = fixture();
            report["sweeps"][1]["requests"][0]["prompt_tokens"] = json!(boundary);
            persist(&mut model, &mut report, &path);
            assert!(validate_evidence(&model, &path, 100).is_ok(), "{boundary}");
        }
    }
}
