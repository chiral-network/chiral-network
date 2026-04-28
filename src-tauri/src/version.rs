//! Version policy plumbing — Phase 1 of the version-enforcement plan.
//!
//! At this stage the policy is purely informational: the binary embeds its
//! own `CARGO_PKG_VERSION` at compile time, every server exposes a
//! `GET /api/version-policy` endpoint that returns a bundled
//! `VersionPolicy`, and clients fetch + log it on startup. No enforcement
//! happens yet — Phases 2–5 will add the soft/hard UI, HTTP rejection,
//! libp2p Identify rejection, and signature-based governance.
//!
//! Bundling the policy with the binary (rather than reading from a config
//! file) keeps Phase 1 self-contained and deterministic. The `signature`
//! and `valid_until` fields are placeholders until Phase 5 wires Ed25519.

use serde::{Deserialize, Serialize};

/// The compile-time semantic version of this build, sourced from the
/// `version = "..."` field in `src-tauri/Cargo.toml`. Used everywhere the
/// client/daemon needs to identify itself (HTTP header in Phase 3,
/// libp2p `agent_version` in Phase 4, the bundled policy below).
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Format used in the libp2p Identify `agent_version` field and the
/// `X-Chiral-Client-Version` HTTP header in later phases. Kept here so
/// every server / client agrees on the spelling.
pub fn agent_version_string() -> String {
    format!("chiral/{}", CURRENT_VERSION)
}

/// What the network thinks about which client versions are allowed in.
///
/// Three states drive UI / network behaviour:
///   - `version >= recommended`  → no UI, fully participating.
///   - `recommended > version >= min_required` → soft "update available".
///   - `version < min_required`  → hard block (Phase 2 onwards).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionPolicy {
    /// Versions strictly below this will eventually be blocked from
    /// joining the network.
    pub min_required: String,
    /// Versions below this trigger a non-blocking "update available"
    /// nudge.
    pub recommended: String,
    /// Where to point users for an update.
    pub download_url: String,
    /// Optional human-readable explanation shown alongside the
    /// blocking modal — e.g. "Fixes payment-verification bug X".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Unix seconds at which this policy was issued. Phase 5 uses this
    /// for rollback protection (don't accept policies older than the
    /// last one we trusted).
    pub issued_at: u64,
    /// Unix seconds after which clients should treat this policy as
    /// stale and re-fetch. `0` means "no expiry" (Phase 1 default).
    pub valid_until: u64,
    /// Hex-encoded Ed25519 signature over the canonical JSON encoding
    /// of every other field. Empty in Phase 1; populated in Phase 5.
    #[serde(default)]
    pub signature: String,
}

/// The policy compiled into this build. Phase 1 keeps things permissive:
/// `min_required = "0.0.0"` means nobody gets blocked yet, while
/// `recommended` matches the current build so freshly-installed clients
/// won't get a (yet to be implemented) "update available" nudge.
pub fn bundled_policy() -> VersionPolicy {
    VersionPolicy {
        min_required: "0.0.0".to_string(),
        recommended: CURRENT_VERSION.to_string(),
        download_url: "https://github.com/chiral-network/chiral-network/releases/latest"
            .to_string(),
        message: None,
        issued_at: 0,
        valid_until: 0,
        signature: String::new(),
    }
}

/// Lightweight semver-ish comparator: parses each side as
/// `[0-9]+ ('.' [0-9]+)*` (ignoring any pre-release / build suffix after
/// the first non-digit/dot) and lexicographically compares the integer
/// component tuples. Phase 5 will swap this for a real semver crate
/// once the policy comes signed and we care about pre-release ordering.
pub fn version_is_below(a: &str, b: &str) -> bool {
    fn parts(s: &str) -> Vec<u64> {
        s.trim_start_matches('v')
            .split(|c: char| !(c.is_ascii_digit() || c == '.'))
            .next()
            .unwrap_or(s)
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    }
    let ap = parts(a);
    let bp = parts(b);
    let n = ap.len().max(bp.len());
    for i in 0..n {
        let av = ap.get(i).copied().unwrap_or(0);
        let bv = bp.get(i).copied().unwrap_or(0);
        if av < bv {
            return true;
        }
        if av > bv {
            return false;
        }
    }
    false
}

/// Three-way comparison of `current` against the policy thresholds.
/// Returns "ok" / "recommended" / "required".
pub fn compare_to_policy(current: &str, policy: &VersionPolicy) -> &'static str {
    if version_is_below(current, &policy.min_required) {
        "required"
    } else if version_is_below(current, &policy.recommended) {
        "recommended"
    } else {
        "ok"
    }
}
