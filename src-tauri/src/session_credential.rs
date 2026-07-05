//! Session credentials — the contract-scoped handle a provider issues at
//! `open` so off-the-shelf S3 / OpenAI tooling can authenticate against a
//! contract (resolved Design Decision #1). A credential is a bearer token plus
//! an S3 access-key/secret, both bound to one `contract_id`; it is proof of
//! contract control, not an independent grant of authority.
//!
//! Design: `docs/chiral-book.md` — "Wire Protocol & API Reference" → Session
//! credential.
//!
//! Token entropy is injected by the caller (`derive`) so this module stays
//! deterministic and unit-testable; the wiring layer supplies OS randomness.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The credential handed back in the `open` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCredential {
    pub contract_id: String,
    /// Opaque bearer, presented as `Authorization: Bearer <bearer>`.
    pub bearer: String,
    pub s3_access_key_id: String,
    pub s3_secret_access_key: String,
    /// The single bucket the contract owns.
    pub bucket: String,
    /// Unix seconds after which the credential no longer resolves.
    pub expires_at: u64,
}

impl SessionCredential {
    /// Derive a credential for `contract_id` from caller-supplied entropy
    /// (needs ≥ 64 bytes). Deterministic in the entropy so it is testable; the
    /// wiring layer passes cryptographically-random bytes.
    pub fn derive(contract_id: &str, expires_at: u64, entropy: &[u8]) -> Result<Self, String> {
        if entropy.len() < 64 {
            return Err("need at least 64 bytes of entropy".to_string());
        }
        let id = contract_id.to_lowercase();
        let h = id.trim_start_matches("0x");
        let bucket = format!("c-{}", &h[..h.len().min(16)]);
        Ok(SessionCredential {
            contract_id: id,
            bearer: format!("chi_sess_{}", hex::encode(&entropy[0..32])),
            s3_access_key_id: format!("AKIA{}", hex::encode(&entropy[32..40]).to_uppercase()),
            s3_secret_access_key: hex::encode(&entropy[40..64]),
            bucket,
            expires_at,
        })
    }

    pub fn is_expired(&self, now_unix: u64) -> bool {
        self.expires_at != 0 && now_unix > self.expires_at
    }
}

/// Server-side index resolving a presented credential back to its contract.
#[derive(Debug, Default)]
pub struct SessionStore {
    by_bearer: HashMap<String, SessionCredential>,
    /// access_key_id -> bearer (indirection so both keys share one record).
    access_key_to_bearer: HashMap<String, String>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a freshly minted credential.
    pub fn insert(&mut self, cred: SessionCredential) {
        self.access_key_to_bearer
            .insert(cred.s3_access_key_id.clone(), cred.bearer.clone());
        self.by_bearer.insert(cred.bearer.clone(), cred);
    }

    /// Resolve a bearer to its contract id, honoring expiry.
    pub fn resolve_bearer(&self, bearer: &str, now_unix: u64) -> Option<&str> {
        self.by_bearer
            .get(bearer)
            .filter(|c| !c.is_expired(now_unix))
            .map(|c| c.contract_id.as_str())
    }

    /// Resolve an S3 access-key id to its contract id, honoring expiry.
    pub fn resolve_access_key(&self, access_key_id: &str, now_unix: u64) -> Option<&str> {
        let bearer = self.access_key_to_bearer.get(access_key_id)?;
        self.resolve_bearer(bearer, now_unix)
    }

    /// Look up the secret for an access-key id (for server-side SigV4 checking).
    pub fn secret_for_access_key(&self, access_key_id: &str) -> Option<&str> {
        let bearer = self.access_key_to_bearer.get(access_key_id)?;
        self.by_bearer
            .get(bearer)
            .map(|c| c.s3_secret_access_key.as_str())
    }

    /// The live credential for a contract, if any (used for idempotent re-open).
    pub fn credential_for_contract(&self, contract_id: &str) -> Option<&SessionCredential> {
        let id = contract_id.to_lowercase();
        self.by_bearer.values().find(|c| c.contract_id == id)
    }

    /// Revoke every credential for a contract (e.g. on close or rotation).
    pub fn revoke_contract(&mut self, contract_id: &str) {
        let id = contract_id.to_lowercase();
        let bearers: Vec<String> = self
            .by_bearer
            .iter()
            .filter(|(_, c)| c.contract_id == id)
            .map(|(b, _)| b.clone())
            .collect();
        for b in bearers {
            if let Some(cred) = self.by_bearer.remove(&b) {
                self.access_key_to_bearer.remove(&cred.s3_access_key_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTRACT: &str = "0xABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789";

    fn entropy(seed: u8) -> Vec<u8> {
        (0..64u8).map(|i| i.wrapping_mul(seed).wrapping_add(seed)).collect()
    }

    #[test]
    fn derive_shapes_tokens_and_bucket() {
        let c = SessionCredential::derive(CONTRACT, 100, &entropy(1)).unwrap();
        assert!(c.bearer.starts_with("chi_sess_"));
        assert_eq!(c.bearer.len(), "chi_sess_".len() + 64); // 32 bytes hex
        assert!(c.s3_access_key_id.starts_with("AKIA"));
        assert_eq!(c.s3_secret_access_key.len(), 48); // 24 bytes hex
        assert_eq!(c.bucket, "c-abcdef0123456789");
        assert_eq!(c.contract_id, CONTRACT.to_lowercase());
    }

    #[test]
    fn derive_requires_entropy() {
        assert!(SessionCredential::derive(CONTRACT, 100, &[0u8; 32]).is_err());
    }

    #[test]
    fn distinct_entropy_gives_distinct_tokens() {
        let a = SessionCredential::derive(CONTRACT, 100, &entropy(1)).unwrap();
        let b = SessionCredential::derive(CONTRACT, 100, &entropy(7)).unwrap();
        assert_ne!(a.bearer, b.bearer);
        assert_ne!(a.s3_access_key_id, b.s3_access_key_id);
    }

    #[test]
    fn store_resolves_both_keys() {
        let mut s = SessionStore::new();
        let c = SessionCredential::derive(CONTRACT, 100, &entropy(3)).unwrap();
        let (bearer, akid) = (c.bearer.clone(), c.s3_access_key_id.clone());
        let secret = c.s3_secret_access_key.clone();
        s.insert(c);
        assert_eq!(s.resolve_bearer(&bearer, 50), Some(CONTRACT.to_lowercase().as_str()));
        assert_eq!(s.resolve_access_key(&akid, 50), Some(CONTRACT.to_lowercase().as_str()));
        assert_eq!(s.secret_for_access_key(&akid), Some(secret.as_str()));
        assert_eq!(s.resolve_bearer("chi_sess_bogus", 50), None);
    }

    #[test]
    fn expired_credential_does_not_resolve() {
        let mut s = SessionStore::new();
        let c = SessionCredential::derive(CONTRACT, 100, &entropy(5)).unwrap();
        let bearer = c.bearer.clone();
        s.insert(c);
        assert!(s.resolve_bearer(&bearer, 99).is_some());
        assert!(s.resolve_bearer(&bearer, 101).is_none());
    }

    #[test]
    fn revoke_removes_both_indexes() {
        let mut s = SessionStore::new();
        let c = SessionCredential::derive(CONTRACT, 100, &entropy(9)).unwrap();
        let (bearer, akid) = (c.bearer.clone(), c.s3_access_key_id.clone());
        s.insert(c);
        s.revoke_contract(CONTRACT);
        assert!(s.resolve_bearer(&bearer, 50).is_none());
        assert!(s.resolve_access_key(&akid, 50).is_none());
    }
}
