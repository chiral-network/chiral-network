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

// ---------------------------------------------------------------------------
// Phase 5 — Ed25519 signing + verification
// ---------------------------------------------------------------------------
//
// Goal: a runtime policy update must come signed by a long-lived project
// key whose public half is hardcoded here. Servers ship the bundled
// policy as a known-good floor; clients only accept network-fetched
// policies that verify against POLICY_PUBLIC_KEY (or, transitionally,
// unsigned policies that don't tighten beyond what's bundled — see
// [`is_acceptable_remote_policy`]).
//
// The placeholder below is 32 zero bytes — *not* a real key. Replace it
// with the project's real Ed25519 public key (32 bytes, hex-encoded
// inline) when the operator generates one. The matching private key
// stays offline / in a CI secret and only feeds the
// `chiral-policy-sign` operator CLI.

/// Project policy-signing public key. **Placeholder zeros until the real
/// key is generated.** No signature can verify against an all-zero key,
/// so signed policies are always rejected until a real key is wired in
/// — and the unsigned-but-permissive transition path (see below) is the
/// only way for a relay-served policy to take effect today.
pub const POLICY_PUBLIC_KEY: [u8; 32] = [0u8; 32];

/// Canonical bytes that get fed to Ed25519 sign/verify. Stable order +
/// NUL separators so any other implementation can reproduce the payload
/// without a JSON canonicaliser.
pub fn canonical_signing_payload(p: &VersionPolicy) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    let issued = p.issued_at.to_string();
    let valid = p.valid_until.to_string();
    let parts: [&[u8]; 6] = [
        p.min_required.as_bytes(),
        p.recommended.as_bytes(),
        p.download_url.as_bytes(),
        p.message.as_deref().unwrap_or("").as_bytes(),
        issued.as_bytes(),
        valid.as_bytes(),
    ];
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push(0);
        }
        out.extend_from_slice(part);
    }
    out
}

/// Verify the policy's `signature` field against POLICY_PUBLIC_KEY.
/// Returns `false` for empty signatures, malformed hex, malformed
/// signatures, malformed public keys (e.g. while the placeholder zeros
/// stand in), or signature mismatches.
pub fn verify_policy(p: &VersionPolicy) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    if p.signature.is_empty() {
        return false;
    }
    let sig_bytes = match hex::decode(&p.signature) {
        Ok(b) if b.len() == 64 => b,
        _ => return false,
    };
    let signature = match Signature::from_slice(&sig_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let key = match VerifyingKey::from_bytes(&POLICY_PUBLIC_KEY) {
        Ok(k) => k,
        Err(_) => return false,
    };
    key.verify(&canonical_signing_payload(p), &signature).is_ok()
}

/// Decide whether a policy fetched from the network should replace the
/// current effective policy. Three rules, in priority order:
///
/// 1. **Rollback protection.** Never accept a policy whose `issuedAt` is
///    older than the currently-stored one.
/// 2. **Signature.** A valid signature is sufficient.
/// 3. **Unsigned-but-permissive (transitional).** While the public key
///    is still the placeholder, real signatures don't exist. Accept
///    unsigned policies *only* if they don't tighten beyond the bundled
///    floor — i.e. their `min_required` is at or below bundled. This
///    lets relays advertise "update available" nudges (recommended /
///    message / downloadUrl) without ever pushing a more aggressive
///    `min_required` than what the binary itself ships with.
pub fn is_acceptable_remote_policy(remote: &VersionPolicy, current: &VersionPolicy) -> bool {
    if remote.issued_at < current.issued_at {
        return false;
    }
    if !remote.signature.is_empty() {
        return verify_policy(remote);
    }
    // Unsigned: accept only if it doesn't tighten the floor.
    let bundled = bundled_policy();
    !version_is_below(&bundled.min_required, &remote.min_required)
}

// ---------------------------------------------------------------------------
// Effective policy slot — global so any caller (Tauri command, fetch
// task, libp2p Identify handler) reads the same value with sync access.
// ---------------------------------------------------------------------------

use once_cell::sync::OnceCell;
use parking_lot::RwLock;

static EFFECTIVE_POLICY: OnceCell<RwLock<VersionPolicy>> = OnceCell::new();

fn effective_slot() -> &'static RwLock<VersionPolicy> {
    EFFECTIVE_POLICY.get_or_init(|| RwLock::new(bundled_policy()))
}

/// Snapshot the currently-effective policy (initialised to bundled if
/// no override has been promoted yet).
pub fn effective_policy() -> VersionPolicy {
    effective_slot().read().clone()
}

/// Replace the effective policy. Returns `true` if accepted, `false` if
/// the supplied policy was rejected by `is_acceptable_remote_policy`.
pub fn update_effective_policy(new: VersionPolicy) -> bool {
    let slot = effective_slot();
    let current = slot.read().clone();
    if !is_acceptable_remote_policy(&new, &current) {
        return false;
    }
    *slot.write() = new;
    true
}
