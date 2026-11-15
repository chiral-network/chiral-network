//! Shared signed version-policy governance for native clients, HTTP services and DHT peers.
//! Production keys require authenticated policies; unsigned development policies cannot
//! tighten the bundled floor or obstruct a later signed policy.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[path = "policy_key_validation.rs"]
mod policy_key_validation;
use policy_key_validation::{is_placeholder_policy_key, parse_policy_public_key_hex};

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
/// component tuples. Malformed numeric prefixes now return an error
/// instead of silently becoming zero.
pub fn version_is_below(a: &str, b: &str) -> Result<bool, String> {
    version_is_below_named(a, "left", b, "right")
}

pub fn version_is_below_named(
    a: &str,
    a_label: &str,
    b: &str,
    b_label: &str,
) -> Result<bool, String> {
    let ap = parse_version_parts(a, a_label)?;
    let bp = parse_version_parts(b, b_label)?;
    let n = ap.len().max(bp.len());
    for i in 0..n {
        let av = ap.get(i).copied().unwrap_or(0);
        let bv = bp.get(i).copied().unwrap_or(0);
        if av < bv {
            return Ok(true);
        }
        if av > bv {
            return Ok(false);
        }
    }
    Ok(false)
}

fn parse_version_parts(raw: &str, label: &str) -> Result<Vec<u64>, String> {
    let trimmed = raw.trim();
    let without_prefix = trimmed.trim_start_matches('v');
    let core = without_prefix
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .next()
        .unwrap_or(without_prefix);
    if core.is_empty() {
        return Err(format!(
            "{label} version {raw:?} does not start with a numeric version"
        ));
    }

    let mut parts = Vec::new();
    for part in core.split('.') {
        if part.is_empty() {
            return Err(format!(
                "{label} version {raw:?} contains an empty numeric component"
            ));
        }
        let parsed = part.parse::<u64>().map_err(|e| {
            format!("{label} version {raw:?} contains an invalid numeric component {part:?}: {e}")
        })?;
        parts.push(parsed);
    }
    Ok(parts)
}

/// Three-way comparison of `current` against the policy thresholds.
/// Returns "ok" / "recommended" / "required".
pub fn compare_to_policy(current: &str, policy: &VersionPolicy) -> Result<&'static str, String> {
    ensure_policy_fresh(policy, policy_now()?)?;
    if version_is_below_named(
        current,
        "current",
        &policy.min_required,
        "policy min_required",
    )? {
        Ok("required")
    } else if version_is_below_named(
        current,
        "current",
        &policy.recommended,
        "policy recommended",
    )? {
        Ok("recommended")
    } else {
        Ok("ok")
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
// Debug/test builds without a configured key use 32 zero bytes as a
// placeholder. Release builds are blocked by build.rs unless
// CHIRAL_POLICY_PUBLIC_KEY is set to a nonzero 32-byte hex key at build
// time, which becomes the compiled fallback. The matching private key
// stays offline / in a CI secret and only feeds the `chiral-policy-sign`
// operator CLI.

// Compile-time fallback for the project policy-signing public key.
// Release builds require CHIRAL_POLICY_PUBLIC_KEY at build time; the matching
// private key remains offline.
include!(concat!(env!("OUT_DIR"), "/policy_public_key.rs"));

/// Resolve the active policy public key. Reads the
/// `CHIRAL_POLICY_PUBLIC_KEY` env var on first call and caches it; if
/// the env var is absent or malformed, falls back to the compile-time
/// `POLICY_PUBLIC_KEY` constant.
pub fn policy_public_key() -> [u8; 32] {
    use once_cell::sync::OnceCell;
    static RESOLVED: OnceCell<[u8; 32]> = OnceCell::new();
    *RESOLVED.get_or_init(|| {
        let raw = match std::env::var("CHIRAL_POLICY_PUBLIC_KEY") {
            Ok(v) => v,
            Err(_) => return POLICY_PUBLIC_KEY,
        };
        match parse_policy_public_key_hex(&raw) {
            Ok(key) if !is_placeholder_policy_key(&key) => {
                eprintln!(
                    "[VERSION] CHIRAL_POLICY_PUBLIC_KEY override active: {}",
                    raw.trim()
                );
                key
            }
            Ok(_) => {
                eprintln!(
                    "[VERSION] CHIRAL_POLICY_PUBLIC_KEY is the all-zero placeholder — falling back to compile-time policy key"
                );
                POLICY_PUBLIC_KEY
            }
            Err(e) => {
                eprintln!(
                    "[VERSION] CHIRAL_POLICY_PUBLIC_KEY invalid ({}) — falling back to compile-time policy key",
                    e
                );
                POLICY_PUBLIC_KEY
            }
        }
    })
}

/// Print a one-shot warning if the active policy public key is the
/// 32-byte zero placeholder. Call this once at process startup.
/// Activates `verify_policy` requires a real key; until then only
/// the unsigned-but-permissive transition path can promote a remote
/// policy.
pub fn log_policy_key_status() {
    use once_cell::sync::OnceCell;
    static LOGGED: OnceCell<()> = OnceCell::new();
    LOGGED.get_or_init(|| {
        let key = policy_public_key();
        if is_placeholder_policy_key(&key) {
            eprintln!(
                "[VERSION] WARNING: policy-signing public key is the placeholder zeros. \
                 Signed policies cannot verify until a real key is generated \
                 (chiral-policy-sign keygen) and wired in via the CHIRAL_POLICY_PUBLIC_KEY \
                 env var. Only the unsigned-but-permissive \
                 transition path is active."
            );
        } else {
            eprintln!("[VERSION] policy-signing public key configured (signed policies enabled)");
        }
    });
}

/// Canonical bytes that get fed to Ed25519 sign/verify. Each field is
/// length-prefixed with a little-endian `u32` byte length so that field
/// boundaries are unambiguous regardless of field contents — in
/// particular, embedded NUL bytes inside a field cannot shift the
/// parse, which would otherwise let one signature attest to multiple
/// distinct policies. Stable order so any other implementation can
/// reproduce the payload without a JSON canonicaliser.
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
    for part in parts.iter() {
        out.extend_from_slice(&(part.len() as u32).to_le_bytes());
        out.extend_from_slice(part);
    }
    out
}

/// Verify the policy's `signature` field against POLICY_PUBLIC_KEY.
/// Returns `false` for empty signatures, malformed hex, malformed
/// signatures, malformed public keys (e.g. while the placeholder zeros
/// stand in), or signature mismatches.
pub fn verify_policy(p: &VersionPolicy) -> bool {
    verify_policy_with_key(p, &policy_public_key())
}

fn verify_policy_with_key(p: &VersionPolicy, key_bytes: &[u8; 32]) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    if p.signature.is_empty() || is_placeholder_policy_key(key_bytes) {
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
    let key = match VerifyingKey::from_bytes(key_bytes) {
        Ok(k) => k,
        Err(_) => return false,
    };
    key.verify(&canonical_signing_payload(p), &signature)
        .is_ok()
}

/// Require fresh, authenticated production policies and monotonic signed updates.
/// Unsigned development policies may not tighten the bundled floor, replace a
/// signed policy, or establish a watermark that blocks authenticated governance.
pub fn is_acceptable_remote_policy(remote: &VersionPolicy, current: &VersionPolicy) -> bool {
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return false;
    };
    accept_policy(remote, current, &policy_public_key(), now.as_secs())
}

fn accept_policy(
    remote: &VersionPolicy,
    current: &VersionPolicy,
    key: &[u8; 32],
    now: u64,
) -> bool {
    if remote.issued_at > now.saturating_add(30)
        || (remote.valid_until != 0
            && (remote.valid_until <= now || remote.valid_until <= remote.issued_at))
        || parse_version_parts(&remote.min_required, "min_required").is_err()
        || parse_version_parts(&remote.recommended, "recommended").is_err()
    {
        return false;
    }
    if !remote.signature.is_empty() {
        if !verify_policy_with_key(remote, key) {
            return false;
        }
        // An unauthenticated development watermark cannot veto an authenticated policy.
        if current.signature.is_empty() {
            return true;
        }
        return remote.issued_at > current.issued_at
            || (remote.issued_at == current.issued_at
                && canonical_signing_payload(remote) == canonical_signing_payload(current));
    }
    if !is_placeholder_policy_key(key)
        || !current.signature.is_empty()
        || remote.issued_at < current.issued_at
    {
        return false;
    }
    version_is_below_named(
        &bundled_policy().min_required,
        "bundled min_required",
        &remote.min_required,
        "remote min_required",
    )
    .is_ok_and(|tighter| !tighter)
}

// ---------------------------------------------------------------------------
// Effective policy slot — global so any caller (Tauri command, fetch
// task, libp2p Identify handler) reads the same value with sync access.
// ---------------------------------------------------------------------------

use once_cell::sync::OnceCell;
use parking_lot::RwLock;

static EFFECTIVE_POLICY: OnceCell<RwLock<EffectivePolicy>> = OnceCell::new();

fn effective_slot() -> &'static RwLock<EffectivePolicy> {
    EFFECTIVE_POLICY.get_or_init(|| RwLock::new(EffectivePolicy::default()))
}

/// Snapshot the currently-effective policy (initialised to bundled if
/// no override has been promoted yet).
pub fn effective_policy() -> VersionPolicy {
    effective_slot()
        .read()
        .policy
        .clone()
        .unwrap_or_else(bundled_policy)
}

/// Restore the authenticated policy for a network-scoped directory and the active
/// authority. An expired policy still establishes the rollback watermark: callers
/// may fetch a newer policy, but admission fails until that refresh succeeds.
/// Corruption, insecure permissions and signature failures are fatal, never a
/// reason to fall back to the bundled policy.
pub fn restore_persisted_policy(directory: &Path) -> Result<(), String> {
    persisted_policy(
        directory,
        None,
        &policy_public_key(),
        policy_now()?,
        effective_slot(),
    )
    .map(|_| ())
}

/// Atomically accept and durably commit a policy before making it effective.
/// Every invocation rereads the authenticated watermark under a process lock.
/// Unsigned development metadata is memory-only and cannot establish that mark.
pub fn update_persisted_policy(directory: &Path, remote: VersionPolicy) -> Result<bool, String> {
    persisted_policy(
        directory,
        Some(remote),
        &policy_public_key(),
        policy_now()?,
        effective_slot(),
    )
}

fn policy_now() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| "system clock precedes the Unix epoch".into())
}

fn ensure_policy_fresh(policy: &VersionPolicy, now: u64) -> Result<(), String> {
    if policy.valid_until != 0 && policy.valid_until <= now {
        return Err("authenticated version policy is expired; fetch a newer signed policy".into());
    }
    Ok(())
}

#[derive(Default)]
struct EffectivePolicy {
    scope: Option<PathBuf>,
    policy: Option<VersionPolicy>,
}

// No unsigned timestamp or separate watermark is stored. The authenticated
// policy itself is the evidence for both the issue time and equal-time payload.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedPolicy<T = VersionPolicy> {
    policy: T,
}

const POLICY_CACHE_LIMIT: u64 = 64 * 1024;

fn private_policy_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => crate::wallet::secure_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or("policy directory has no parent")?;
            if !parent.exists() {
                private_policy_directory(parent)?;
            }
            crate::wallet::secure_directory(path)?;
            sync_policy_directory(parent)
        }
        Err(error) => Err(format!("cannot inspect policy directory: {error}")),
    }
}

fn sync_policy_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("cannot sync policy directory: {error}"))?;
    // Windows commits use MoveFileExW(MOVEFILE_WRITE_THROUGH) below; a Unix-style
    // directory FlushFileBuffers is not available to an unprivileged process.
    #[cfg(windows)]
    let _ = path;
    #[cfg(not(any(unix, windows)))]
    return Err("durable policy persistence is unsupported on this platform".into());
    Ok(())
}

fn commit_policy_file(temporary: &Path, directory: &Path) -> Result<(), String> {
    let destination = directory.join("policy.json");
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(format!(
                "cannot commit policy cache: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    #[cfg(not(windows))]
    fs::rename(temporary, destination)
        .map_err(|error| format!("cannot commit policy cache: {error}"))?;
    sync_policy_directory(directory)
}

fn read_persisted_policy(
    path: &Path,
    key: &[u8; 32],
    now: u64,
) -> Result<Option<VersionPolicy>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect policy cache: {error}")),
        Ok(_) => {}
    }
    let file = crate::wallet::open_secret_file(path)?;
    let mut bytes = Vec::new();
    file.take(POLICY_CACHE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read policy cache: {error}"))?;
    if bytes.len() as u64 > POLICY_CACHE_LIMIT {
        return Err("policy cache exceeds size limit".into());
    }
    let cached: PersistedPolicy =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid policy cache: {error}"))?;
    let policy = cached.policy;
    // Expiry is intentionally not checked here. It makes the policy unusable for
    // admission, not unauthenticated, and must never discard rollback evidence.
    if !verify_policy_with_key(&policy, key)
        || policy.issued_at > now.saturating_add(30)
        || (policy.valid_until != 0 && policy.valid_until <= policy.issued_at)
        || parse_version_parts(&policy.min_required, "min_required").is_err()
        || parse_version_parts(&policy.recommended, "recommended").is_err()
    {
        return Err("policy cache authenticity or signed fields rejected".into());
    }
    Ok(Some(policy))
}

fn persist_signed_policy(directory: &Path, policy: &VersionPolicy) -> Result<(), String> {
    let bytes = serde_json::to_vec(&PersistedPolicy { policy })
        .map_err(|error| format!("cannot encode policy cache: {error}"))?;
    if bytes.len() as u64 > POLICY_CACHE_LIMIT {
        return Err("policy cache exceeds size limit".into());
    }
    let temporary = directory.join(format!(".policy-{:032x}.tmp", rand::random::<u128>()));
    let result = (|| {
        let mut file = crate::wallet::create_secret_file(&temporary)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("cannot persist policy cache: {error}"))?;
        drop(file);
        commit_policy_file(&temporary, directory)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn persisted_policy(
    directory: &Path,
    remote: Option<VersionPolicy>,
    key: &[u8; 32],
    now: u64,
    slot: &RwLock<EffectivePolicy>,
) -> Result<bool, String> {
    // Serialize threads before acquiring the file lock, including profile switches.
    let mut effective = slot.write();
    let directory = if directory.is_absolute() {
        directory.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(directory)
    };
    private_policy_directory(&directory)?;
    let authority = directory.join(hex::encode(key));
    private_policy_directory(&authority)?;
    let authority = fs::canonicalize(authority).map_err(|error| error.to_string())?;
    let lock_path = authority.join("policy.lock");
    let lock = crate::wallet::create_secret_file(&lock_path)
        .or_else(|_| crate::wallet::open_secret_file(&lock_path))?;
    let _lock = crate::file_lock::ExclusiveFileLock::acquire(lock)
        .map_err(|error| format!("cannot lock policy cache: {error}"))?;
    let cached = read_persisted_policy(&authority.join("policy.json"), key, now)?;
    let previous = if effective.scope.as_ref() == Some(&authority) {
        effective.policy.as_ref()
    } else {
        None
    };
    if let Some(previous) = previous.filter(|policy| !policy.signature.is_empty()) {
        let cached = cached
            .as_ref()
            .ok_or("authenticated policy cache disappeared")?;
        if cached.issued_at < previous.issued_at
            || (cached.issued_at == previous.issued_at
                && canonical_signing_payload(cached) != canonical_signing_payload(previous))
        {
            return Err("authenticated policy cache rolled back or equivocated".into());
        }
    }
    let mut current = cached
        .or_else(|| previous.cloned())
        .unwrap_or_else(bundled_policy);
    let accepted = if let Some(remote) = remote {
        if accept_policy(&remote, &current, key, now) {
            if !remote.signature.is_empty() {
                persist_signed_policy(&authority, &remote)?;
            }
            current = remote;
            true
        } else {
            false
        }
    } else {
        false
    };
    effective.scope = Some(authority);
    effective.policy = Some(current);
    Ok(accepted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn policy(min: &str, rec: &str, issued: u64) -> VersionPolicy {
        VersionPolicy {
            min_required: min.into(),
            recommended: rec.into(),
            download_url: "https://x".into(),
            message: None,
            issued_at: issued,
            valid_until: 0,
            signature: String::new(),
        }
    }

    #[test]
    fn version_is_below_basic_ordering() {
        assert_eq!(version_is_below("1.0.0", "1.0.1"), Ok(true));
        assert_eq!(version_is_below("1.0.0", "1.1.0"), Ok(true));
        assert_eq!(version_is_below("1.0.0", "2.0.0"), Ok(true));
        assert_eq!(version_is_below("1.0.0", "1.0.0"), Ok(false));
        assert_eq!(version_is_below("2.0.0", "1.9.9"), Ok(false));
    }

    #[test]
    fn version_is_below_padding_and_v_prefix() {
        // "1.0" should equal "1.0.0", not be below it.
        assert_eq!(version_is_below("1.0", "1.0.0"), Ok(false));
        assert_eq!(version_is_below("1.0.0", "1.0"), Ok(false));
        // Leading 'v' tolerated on either side.
        assert_eq!(version_is_below("v1.0.0", "v1.0.1"), Ok(true));
        assert_eq!(version_is_below("1.0.0", "v1.0.1"), Ok(true));
    }

    #[test]
    fn version_is_below_strips_pre_release_suffix() {
        // "1.0.0-rc1" parses as 1.0.0, equal to "1.0.0".
        assert_eq!(version_is_below("1.0.0-rc1", "1.0.0"), Ok(false));
        assert_eq!(version_is_below("1.0.0+build", "1.0.1"), Ok(true));
    }

    #[test]
    fn version_is_below_rejects_empty_and_garbage() {
        let empty = version_is_below("", "0.0.0").expect_err("empty versions should be rejected");
        assert!(empty.contains("numeric version"));

        let garbage =
            version_is_below("garbage", "0.0.1").expect_err("garbage versions should be rejected");
        assert!(garbage.contains("numeric version"));
    }

    #[test]
    fn compare_to_policy_three_states() {
        let p = policy("1.0.0", "2.0.0", 0);
        assert_eq!(compare_to_policy("0.9.0", &p), Ok("required"));
        assert_eq!(compare_to_policy("1.0.0", &p), Ok("recommended"));
        assert_eq!(compare_to_policy("1.5.0", &p), Ok("recommended"));
        assert_eq!(compare_to_policy("2.0.0", &p), Ok("ok"));
        assert_eq!(compare_to_policy("3.0.0", &p), Ok("ok"));
    }

    #[test]
    fn compare_to_policy_misconfigured_min_above_recommended_fails_safe() {
        // Misconfigured policy where min_required > recommended must
        // still treat versions below min as "required" (fail-safe).
        let p = policy("2.0.0", "1.0.0", 0);
        assert_eq!(compare_to_policy("1.5.0", &p), Ok("required"));
    }

    #[test]
    fn compare_to_policy_rejects_malformed_current_version() {
        let p = policy("1.0.0", "2.0.0", 0);
        assert!(compare_to_policy("not-a-version", &p).is_err());
    }

    #[test]
    fn compare_to_policy_rejects_malformed_policy_version() {
        let p = policy("1.x.0", "2.0.0", 0);
        assert!(compare_to_policy("1.0.0", &p).is_err());
    }

    #[test]
    fn is_acceptable_unsigned_rejects_malformed_remote_minimum() {
        let bundled = bundled_policy();
        let mut remote = bundled.clone();
        remote.min_required = "not-a-version".into();
        remote.issued_at = bundled.issued_at + 1;

        assert!(!is_acceptable_remote_policy(&remote, &bundled));
    }

    #[test]
    fn canonical_payload_distinguishes_field_changes() {
        let a = policy("1.0.0", "2.0.0", 100);
        let mut b = a.clone();
        b.recommended = "2.0.1".into();
        assert_ne!(canonical_signing_payload(&a), canonical_signing_payload(&b));
    }

    #[test]
    fn canonical_payload_is_injective_under_nul_shift() {
        // Regression: BUG-002. Two policies that differ only by where a
        // NUL byte sits within their string fields used to produce
        // byte-equal canonical payloads under a separator-based
        // encoding. The length-prefixed encoding keeps them distinct.
        let mut a = policy("1.0.0\0relaxed", "9.9.9", 1700000000);
        a.download_url = String::new();
        let mut b = policy("1.0.0", "relaxed\09.9.9", 1700000000);
        b.download_url = String::new();
        assert_ne!(canonical_signing_payload(&a), canonical_signing_payload(&b));
    }

    #[test]
    fn verify_policy_rejects_empty_signature() {
        let p = policy("1.0.0", "2.0.0", 100);
        assert!(!verify_policy(&p));
    }

    #[test]
    fn verify_policy_rejects_malformed_hex() {
        let mut p = policy("1.0.0", "2.0.0", 100);
        p.signature = "not-hex!".into();
        assert!(!verify_policy(&p));
    }

    #[test]
    fn verify_policy_rejects_wrong_length() {
        let mut p = policy("1.0.0", "2.0.0", 100);
        p.signature = "deadbeef".into();
        assert!(!verify_policy(&p));
    }

    #[test]
    fn verify_policy_rejects_against_placeholder_zero_key() {
        // While POLICY_PUBLIC_KEY is all zeros, even a properly-shaped
        // 64-byte hex signature must not verify.
        let mut p = policy("1.0.0", "2.0.0", 100);
        p.signature = hex::encode([1u8; 64]);
        assert!(!verify_policy(&p));
    }

    #[test]
    fn placeholder_policy_key_detection() {
        assert!(is_placeholder_policy_key(&[0u8; 32]));
        assert!(!is_placeholder_policy_key(&[1u8; 32]));
    }

    #[test]
    fn release_builds_do_not_embed_placeholder_policy_key() {
        if !cfg!(debug_assertions) {
            assert!(!is_placeholder_policy_key(&POLICY_PUBLIC_KEY));
        }
    }

    #[test]
    fn is_acceptable_rejects_rollback_by_issued_at() {
        let current = policy("0.0.0", "1.0.0", 200);
        let older = policy("0.0.0", "1.0.0", 100);
        assert!(!is_acceptable_remote_policy(&older, &current));
    }

    #[test]
    fn is_acceptable_unsigned_at_or_below_bundled_floor() {
        let bundled = bundled_policy();
        let mut remote = bundled.clone();
        remote.recommended = "9.9.9".into();
        remote.issued_at = bundled.issued_at + 1;
        assert!(accept_policy(&remote, &bundled, &[0; 32], 100));
    }

    #[test]
    fn is_acceptable_unsigned_rejects_tightening() {
        // Unsigned policy raising min_required above bundled floor (0.0.0)
        // must be rejected.
        let bundled = bundled_policy();
        let mut remote = bundled.clone();
        remote.min_required = "9.9.9".into();
        remote.issued_at = bundled.issued_at + 1;
        assert!(!is_acceptable_remote_policy(&remote, &bundled));
    }

    #[test]
    fn is_acceptable_unsigned_cannot_replace_signed_current() {
        // Once a signed policy is in effect, an unsigned remote (even with
        // a fresher issued_at and no tightening) must not be accepted —
        // otherwise an attacker can disable enforcement by publishing
        // unsigned "0.0.0" updates.
        let mut current = policy("1.0.0", "1.0.0", 100);
        current.signature = hex::encode([1u8; 64]); // marker: signed
        let mut remote = policy("0.0.0", "0.0.0", 200);
        remote.signature = String::new();
        assert!(!is_acceptable_remote_policy(&remote, &current));
    }

    #[test]
    fn signed_policy_cannot_be_blocked_by_unsigned_watermark() {
        let signing = SigningKey::generate(&mut OsRng);
        let key = signing.verifying_key().to_bytes();
        let mut current = bundled_policy();
        current.issued_at = u64::MAX;
        let mut remote = policy("1.0.0", "2.0.0", 100);
        assert!(!accept_policy(&remote, &bundled_policy(), &key, 200));
        remote.signature =
            hex::encode(signing.sign(&canonical_signing_payload(&remote)).to_bytes());
        assert!(accept_policy(&remote, &current, &key, 200));
        let accepted = remote.clone();
        remote.min_required = "0.0.0".into();
        remote.signature =
            hex::encode(signing.sign(&canonical_signing_payload(&remote)).to_bytes());
        assert!(!accept_policy(&remote, &accepted, &key, 200));
    }

    #[test]
    fn future_and_expired_policies_are_rejected() {
        let current = bundled_policy();
        let mut future = policy("0.0.0", "1.0.0", u64::MAX);
        assert!(!accept_policy(&future, &current, &[0; 32], 200));
        future.issued_at = 100;
        future.valid_until = 200;
        assert!(!accept_policy(&future, &current, &[0; 32], 200));
    }

    fn signed_policy(signing: &SigningKey, min: &str, issued: u64, expires: u64) -> VersionPolicy {
        let mut policy = policy(min, min, issued);
        policy.valid_until = expires;
        policy.signature =
            hex::encode(signing.sign(&canonical_signing_payload(&policy)).to_bytes());
        policy
    }

    fn fresh_slot() -> RwLock<EffectivePolicy> {
        RwLock::new(EffectivePolicy::default())
    }

    fn cached_path(directory: &Path, key: &[u8; 32]) -> PathBuf {
        directory.join(hex::encode(key)).join("policy.json")
    }

    fn private_test_directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        crate::wallet::secure_directory(directory.path()).unwrap();
        directory
    }

    #[test]
    fn persisted_policy_restore_keeps_equal_time_payload_and_rejects_rollback() {
        let directory = private_test_directory();
        let signing = SigningKey::generate(&mut OsRng);
        let key = signing.verifying_key().to_bytes();
        let b = signed_policy(&signing, "2.0.0", 200, 400);
        assert!(
            persisted_policy(directory.path(), Some(b.clone()), &key, 250, &fresh_slot()).unwrap()
        );
        let restored = fresh_slot();
        persisted_policy(directory.path(), None, &key, 251, &restored).unwrap();
        assert_eq!(restored.read().policy.as_ref().unwrap().issued_at, 200);
        assert!(!persisted_policy(
            directory.path(),
            Some(signed_policy(&signing, "0.0.0", 100, 400)),
            &key,
            251,
            &restored
        )
        .unwrap());
        assert!(!persisted_policy(
            directory.path(),
            Some(signed_policy(&signing, "0.0.0", 200, 400)),
            &key,
            251,
            &restored
        )
        .unwrap());
        assert!(persisted_policy(directory.path(), Some(b), &key, 251, &restored).unwrap());
        assert_eq!(
            restored.read().policy.as_ref().unwrap().min_required,
            "2.0.0"
        );
    }

    #[test]
    fn persisted_policy_expiry_retains_watermark_but_allows_fresh_advance() {
        let directory = private_test_directory();
        let signing = SigningKey::generate(&mut OsRng);
        let key = signing.verifying_key().to_bytes();
        let b = signed_policy(&signing, "2.0.0", 200, 300);
        assert!(persisted_policy(directory.path(), Some(b), &key, 250, &fresh_slot()).unwrap());
        let restored = fresh_slot();
        persisted_policy(directory.path(), None, &key, 400, &restored).unwrap();
        {
            let effective = restored.read();
            let expired = effective.policy.as_ref().unwrap();
            assert_eq!(expired.issued_at, 200);
            assert!(ensure_policy_fresh(expired, 400).is_err());
            assert!(compare_to_policy("3.0.0", expired).is_err());
        }
        assert!(!persisted_policy(
            directory.path(),
            Some(signed_policy(&signing, "0.0.0", 100, 0)),
            &key,
            400,
            &restored
        )
        .unwrap());
        assert!(!persisted_policy(
            directory.path(),
            Some(signed_policy(&signing, "2.0.0", 200, 0)),
            &key,
            400,
            &restored
        )
        .unwrap());
        assert!(persisted_policy(
            directory.path(),
            Some(signed_policy(&signing, "3.0.0", 350, 500)),
            &key,
            400,
            &restored
        )
        .unwrap());
        assert_eq!(restored.read().policy.as_ref().unwrap().issued_at, 350);
    }

    #[test]
    fn persisted_policy_never_persists_unsigned_future_watermark() {
        let directory = private_test_directory();
        let slot = fresh_slot();
        assert!(!persisted_policy(
            directory.path(),
            Some(policy("0.0.0", "1.0.0", u64::MAX)),
            &[0; 32],
            200,
            &slot
        )
        .unwrap());
        assert!(persisted_policy(
            directory.path(),
            Some(policy("0.0.0", "1.0.0", 200)),
            &[0; 32],
            200,
            &slot
        )
        .unwrap());
        assert!(!cached_path(directory.path(), &[0; 32]).exists());
        let signing = SigningKey::generate(&mut OsRng);
        let key = signing.verifying_key().to_bytes();
        assert!(persisted_policy(
            directory.path(),
            Some(signed_policy(&signing, "1.0.0", 100, 0)),
            &key,
            200,
            &slot
        )
        .unwrap());
        let bytes = fs::read(cached_path(directory.path(), &key)).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["issuedAt"] = serde_json::json!(u64::MAX);
        fs::write(
            cached_path(directory.path(), &key),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(persisted_policy(directory.path(), None, &key, 200, &fresh_slot()).is_err());
    }

    #[test]
    fn persisted_policy_isolates_authorities_and_networks() {
        let directory = private_test_directory();
        let network_a = directory.path().join("network-a");
        let network_b = directory.path().join("network-b");
        let authority_a = SigningKey::generate(&mut OsRng);
        let authority_b = SigningKey::generate(&mut OsRng);
        let key_a = authority_a.verifying_key().to_bytes();
        let key_b = authority_b.verifying_key().to_bytes();
        let slot = fresh_slot();
        assert!(persisted_policy(
            &network_a,
            Some(signed_policy(&authority_a, "2.0.0", 200, 0)),
            &key_a,
            250,
            &slot
        )
        .unwrap());
        persisted_policy(&network_b, None, &key_a, 250, &slot).unwrap();
        assert_eq!(slot.read().policy.as_ref().unwrap().issued_at, 0);
        assert!(persisted_policy(
            &network_b,
            Some(signed_policy(&authority_a, "1.0.0", 100, 0)),
            &key_a,
            250,
            &slot
        )
        .unwrap());
        persisted_policy(&network_a, None, &key_b, 250, &slot).unwrap();
        assert_eq!(slot.read().policy.as_ref().unwrap().issued_at, 0);
        assert!(persisted_policy(
            &network_a,
            Some(signed_policy(&authority_b, "1.0.0", 100, 0)),
            &key_b,
            250,
            &slot
        )
        .unwrap());
        persisted_policy(&network_a, None, &key_a, 250, &slot).unwrap();
        assert_eq!(slot.read().policy.as_ref().unwrap().issued_at, 200);
        // A copied policy in the other authority's private cache must not verify.
        fs::copy(
            cached_path(&network_a, &key_a),
            cached_path(&network_a, &key_b),
        )
        .unwrap();
        assert!(persisted_policy(&network_a, None, &key_b, 250, &fresh_slot()).is_err());
    }

    #[test]
    fn persisted_policy_corruption_never_falls_back_or_overwrites_evidence() {
        let directory = private_test_directory();
        let signing = SigningKey::generate(&mut OsRng);
        let key = signing.verifying_key().to_bytes();
        let b = signed_policy(&signing, "2.0.0", 200, 0);
        assert!(
            persisted_policy(directory.path(), Some(b.clone()), &key, 250, &fresh_slot()).unwrap()
        );
        let path = cached_path(directory.path(), &key);
        let mut forged = b.clone();
        forged.min_required = "0.0.0".into();
        let unsigned = policy("0.0.0", "1.0.0", u64::MAX);
        for corrupt in [
            b"{".to_vec(),
            serde_json::to_vec(&PersistedPolicy { policy: forged }).unwrap(),
            serde_json::to_vec(&PersistedPolicy { policy: unsigned }).unwrap(),
        ] {
            fs::write(&path, &corrupt).unwrap();
            let slot = fresh_slot();
            assert!(persisted_policy(directory.path(), None, &key, 250, &slot).is_err());
            assert!(persisted_policy(directory.path(), Some(b.clone()), &key, 250, &slot).is_err());
            assert_eq!(fs::read(&path).unwrap(), corrupt);
            assert!(slot.read().policy.is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn persisted_policy_requires_owner_private_files_and_lock() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = private_test_directory();
        let signing = SigningKey::generate(&mut OsRng);
        let key = signing.verifying_key().to_bytes();
        assert!(persisted_policy(
            directory.path(),
            Some(signed_policy(&signing, "1.0.0", 100, 0)),
            &key,
            200,
            &fresh_slot()
        )
        .unwrap());
        let cache = cached_path(directory.path(), &key);
        let lock = cache.parent().unwrap().join("policy.lock");
        for path in [&cache, &lock] {
            let metadata = fs::metadata(path).unwrap();
            assert_eq!(metadata.mode() & 0o777, 0o600);
            assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
            fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(persisted_policy(directory.path(), None, &key, 200, &fresh_slot()).is_err());
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let saved = cache.with_extension("saved");
        fs::rename(&cache, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, &cache).unwrap();
        assert!(persisted_policy(directory.path(), None, &key, 200, &fresh_slot()).is_err());
    }

    fn policy_process(
        directory: &Path,
        key: &[u8; 32],
        remote: Option<VersionPolicy>,
        accepted: Option<bool>,
        issued: Option<u64>,
    ) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "version::tests::persisted_policy_process_worker", "--nocapture"])
            .env("CHIRAL_POLICY_PUBLIC_KEY", hex::encode(key))
            .env("CHIRAL_POLICY_TEST_PROCESS", serde_json::json!({
                "directory": directory, "remote": remote, "accepted": accepted, "issued": issued,
            }).to_string())
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .spawn().unwrap()
    }

    fn finish_policy_process(child: std::process::Child) {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "policy child failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "child-process entrypoint for durable policy regression tests"]
    fn persisted_policy_process_worker() {
        let Ok(input) = std::env::var("CHIRAL_POLICY_TEST_PROCESS") else {
            return;
        };
        let input: serde_json::Value = serde_json::from_str(&input).unwrap();
        let directory = Path::new(input["directory"].as_str().unwrap());
        restore_persisted_policy(directory).unwrap();
        if !input["remote"].is_null() {
            let accepted = update_persisted_policy(
                directory,
                serde_json::from_value(input["remote"].clone()).unwrap(),
            )
            .unwrap();
            if let Some(expected) = input["accepted"].as_bool() {
                assert_eq!(accepted, expected);
            }
        }
        if let Some(issued) = input["issued"].as_u64() {
            assert_eq!(effective_policy().issued_at, issued);
        }
    }

    #[test]
    fn persisted_policy_fresh_process_rejects_older_still_valid_policy() {
        let directory = private_test_directory();
        let signing = SigningKey::generate(&mut OsRng);
        let key = signing.verifying_key().to_bytes();
        let now = policy_now().unwrap();
        finish_policy_process(policy_process(
            directory.path(),
            &key,
            Some(signed_policy(&signing, "2.0.0", now - 10, now + 300)),
            Some(true),
            Some(now - 10),
        ));
        finish_policy_process(policy_process(
            directory.path(),
            &key,
            None,
            None,
            Some(now - 10),
        ));
        finish_policy_process(policy_process(
            directory.path(),
            &key,
            Some(signed_policy(&signing, "0.0.0", now - 20, now + 300)),
            Some(false),
            Some(now - 10),
        ));
    }

    #[test]
    fn persisted_policy_concurrent_process_writers_keep_highest_watermark() {
        let directory = private_test_directory();
        let signing = SigningKey::generate(&mut OsRng);
        let key = signing.verifying_key().to_bytes();
        let now = policy_now().unwrap();
        let mut children = Vec::new();
        for age in 1..=8 {
            children.push(policy_process(
                directory.path(),
                &key,
                Some(signed_policy(&signing, "1.0.0", now - age, 0)),
                None,
                None,
            ));
        }
        for child in children {
            finish_policy_process(child);
        }
        finish_policy_process(policy_process(
            directory.path(),
            &key,
            None,
            None,
            Some(now - 1),
        ));
    }
}
