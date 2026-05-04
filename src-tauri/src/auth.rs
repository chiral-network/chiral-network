//! Owner-proof authentication for HTTP routes that previously trusted
//! the bare `X-Owner` header.
//!
//! Before FM-A03, any HTTP caller could put any wallet address in the
//! `X-Owner` header and the server would treat them as that wallet.
//! This module replaces that with a signed-challenge scheme.
//!
//! Wire format:
//!   X-Owner:     0x<40 lowercase hex>          (the claimed wallet)
//!   X-Owner-Sig: <unix_ts>:<hex_signature>     (timestamp + ECDSA sig)
//!
//! The signed bytes bind the caller's wallet, an issue timestamp, the
//! HTTP method, and the path-with-query — so a captured proof can only
//! be replayed against the same endpoint within a ±5-minute window.
//! The encoding is length-prefixed (same family as
//! `version::canonical_signing_payload`) so an attacker can't shift
//! bytes across field boundaries.
//!
//! Verification is stateless: the server consults only `now` and the
//! claimed wallet's recoverable signer. No nonce store, no session.

use axum::http::{HeaderMap, Method};

pub const PROOF_VALIDITY_SECS: i64 = 300;
const TAG: &[u8] = b"chiral-owner-proof-v1";

/// Canonical signing bytes for the owner-proof header.
pub fn owner_proof_payload(
    wallet_lowercased: &str,
    ts_unix: i64,
    method: &str,
    path_and_query: &str,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + path_and_query.len());
    out.extend_from_slice(TAG);
    for part in [
        wallet_lowercased.as_bytes(),
        method.as_bytes(),
        path_and_query.as_bytes(),
    ] {
        out.extend_from_slice(&(part.len() as u32).to_le_bytes());
        out.extend_from_slice(part);
    }
    out.extend_from_slice(&ts_unix.to_le_bytes());
    out
}

fn is_valid_wallet(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Verify the `X-Owner` + `X-Owner-Sig` headers against the given
/// method and `path_and_query`. On success returns the verified
/// lowercase wallet address; on failure returns a human-readable
/// reason (which middleware will return as the 401 body).
pub fn verify_owner_proof(
    headers: &HeaderMap,
    method: &Method,
    path_and_query: &str,
) -> Result<String, String> {
    let owner_raw = headers
        .get("x-owner")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if owner_raw.is_empty() {
        return Err("X-Owner header required".to_string());
    }
    let owner = owner_raw.to_lowercase();
    if !is_valid_wallet(&owner) {
        return Err("X-Owner is not a valid 0x-hex wallet address".to_string());
    }
    let sig_header = headers
        .get("x-owner-sig")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if sig_header.is_empty() {
        return Err("X-Owner-Sig header required".to_string());
    }
    let (ts_str, sig) = sig_header
        .split_once(':')
        .ok_or_else(|| "X-Owner-Sig must be \"<ts>:<hex>\"".to_string())?;
    let ts: i64 = ts_str
        .parse()
        .map_err(|_| "X-Owner-Sig timestamp is not an integer".to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if (now - ts).abs() > PROOF_VALIDITY_SECS {
        return Err(format!(
            "X-Owner-Sig timestamp outside ±{}s validity window (server={}, sig={})",
            PROOF_VALIDITY_SECS, now, ts
        ));
    }
    let payload = owner_proof_payload(&owner, ts, method.as_str(), path_and_query);
    if !crate::wallet::verify_signature(&payload, sig, &owner) {
        return Err("X-Owner-Sig did not verify against X-Owner wallet".to_string());
    }
    Ok(owner)
}

/// Axum middleware: require a valid owner-proof on every request that
/// passes through this layer. OPTIONS preflight is allowed through so
/// CORS still works. Failed proofs return `401 Unauthorized` with the
/// failure reason as the body.
pub async fn owner_proof_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    if req.method() == Method::OPTIONS {
        return next.run(req).await;
    }
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("");
    let method = req.method().clone();
    match verify_owner_proof(req.headers(), &method, path_and_query) {
        Ok(_) => next.run(req).await,
        Err(reason) => (StatusCode::UNAUTHORIZED, reason).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_is_injective_under_field_shift() {
        // Length-prefix protects the parse: shifting bytes between
        // wallet/method/path can't produce a colliding payload.
        let a = owner_proof_payload("0xabc:def", 1700000000, "GET", "/api/drive/items");
        let b = owner_proof_payload("0xabc", 1700000000, "GET:def", "/api/drive/items");
        assert_ne!(a, b);
    }

    #[test]
    fn payload_is_deterministic() {
        let p1 = owner_proof_payload("0x1234", 1700000000, "POST", "/api/x");
        let p2 = owner_proof_payload("0x1234", 1700000000, "POST", "/api/x");
        assert_eq!(p1, p2);
    }

    /// Tag prefix is the only thing that domain-separates this proof
    /// from any other length-prefixed signing payload in the project
    /// (file metadata, folder manifest, site directory, etc.). If the
    /// tag goes missing, a signature attesting to one purpose could be
    /// replayed against another. Lock the literal tag down.
    #[test]
    fn payload_starts_with_domain_tag() {
        let p = owner_proof_payload("0x1234", 1, "GET", "/api");
        assert!(
            p.starts_with(b"chiral-owner-proof-v1"),
            "payload must begin with the v1 domain tag"
        );
    }

    /// Each input field must change the payload independently. Without
    /// this, an attacker could capture a (wallet, ts, GET, /api/foo)
    /// proof and replay it as (wallet, ts, GET, /api/bar) — defeating
    /// the path binding.
    #[test]
    fn payload_distinct_per_field() {
        let base = owner_proof_payload("0xa", 100, "GET", "/api/x");
        // Wallet differs.
        assert_ne!(base, owner_proof_payload("0xb", 100, "GET", "/api/x"));
        // Timestamp differs.
        assert_ne!(base, owner_proof_payload("0xa", 101, "GET", "/api/x"));
        // Method differs.
        assert_ne!(base, owner_proof_payload("0xa", 100, "POST", "/api/x"));
        // Path differs.
        assert_ne!(base, owner_proof_payload("0xa", 100, "GET", "/api/y"));
    }

    /// Method and path are separately length-prefixed, so a path that
    /// "absorbs" a method suffix (e.g. method="GET" path="/api" vs.
    /// method="" path="GET/api") must not collide.
    #[test]
    fn payload_method_path_cannot_be_merged() {
        let a = owner_proof_payload("0x1234", 1700000000, "GET", "/api");
        let b = owner_proof_payload("0x1234", 1700000000, "", "GET/api");
        assert_ne!(a, b);
    }

    /// Empty fields are still length-prefixed (with a zero u32). An
    /// attacker who omits the wallet shouldn't get a payload that
    /// collides with a different non-empty wallet.
    #[test]
    fn payload_empty_fields_remain_distinct() {
        let a = owner_proof_payload("", 1, "GET", "/x");
        let b = owner_proof_payload("a", 1, "GET", "/x");
        assert_ne!(a, b);
    }

    #[test]
    fn invalid_wallet_format_rejected() {
        assert!(!is_valid_wallet(""));
        assert!(!is_valid_wallet("0x"));
        assert!(!is_valid_wallet("0x123")); // too short
        assert!(!is_valid_wallet(&format!("0x{}", "z".repeat(40)))); // non-hex
        assert!(!is_valid_wallet(&format!("0x{}1", "0".repeat(40)))); // too long
        assert!(!is_valid_wallet(&"a".repeat(42))); // missing 0x prefix
        // Real-shape wallet accepted regardless of case.
        assert!(is_valid_wallet(&format!("0x{}", "abcdef0123".repeat(4))));
        assert!(is_valid_wallet(&format!("0x{}", "ABCDEF0123".repeat(4))));
    }
}
