//! One-use, body-bound wallet authentication for inference control operations.
use crate::{
    billing_store::BillingStore,
    protocol::{self, NetworkIdentity},
};
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

pub const CHALLENGE_TTL_SECS: u64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthPayload {
    pub network: NetworkIdentity,
    pub provider_wallet: String,
    pub wallet: String,
    pub method: String,
    pub path: String,
    pub body_hash: String,
    pub nonce: String,
    pub expires_at: u64,
}

pub fn challenge_payload(payload: &AuthPayload) -> Result<Vec<u8>, String> {
    protocol::signing_payload(protocol::AUTH_DOMAIN, payload)
}

pub fn canonical_wallet(wallet: &str) -> Result<String, String> {
    if wallet.len() != 42
        || !wallet.starts_with("0x")
        || !wallet[2..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid_wallet: expected a 20-byte hex wallet address".into());
    }
    Ok(wallet.to_ascii_lowercase())
}

pub fn validate_challenge_request(request: &protocol::ChallengeRequest) -> Result<(), String> {
    canonical_wallet(&request.wallet)?;
    if !matches!(request.method.as_str(), "GET" | "POST" | "DELETE")
        || !request.path.starts_with("/v1/contracts/")
        || request.path.len() > 512
        || request
            .path
            .bytes()
            .any(|b| b.is_ascii_control() || b == b'#')
        || request.body_hash.len() != 66
        || !request.body_hash.starts_with("0x")
        || !request.body_hash[2..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid_challenge: invalid method, path, or body hash".into());
    }
    Ok(())
}

/// A verified signature never leaves a reusable nonce behind. Consumption and
/// challenge matching happen under the ledger's BEGIN IMMEDIATE transaction.
pub fn authenticate_owner(
    ledger: &BillingStore,
    headers: &HeaderMap,
    network: &NetworkIdentity,
    provider: &str,
    method: &str,
    path: &str,
    raw_body: &[u8],
    now: u64,
) -> Result<String, String> {
    fn header<'a>(headers: &'a HeaderMap, name: &str, limit: usize) -> Result<&'a str, String> {
        let value = headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .ok_or("unauthorized: owner proof required")?;
        if value.len() > limit {
            return Err("unauthorized: invalid owner proof".into());
        }
        Ok(value)
    }
    let owner = canonical_wallet(header(headers, "x-chiral-owner", 42)?)?;
    let nonce = header(headers, "x-chiral-nonce", 66)?;
    let signature = header(headers, "x-chiral-signature", 132)?;
    let payload = ledger.challenge(nonce)?;
    if payload.network != *network
        || payload.provider_wallet != provider
        || payload.wallet != owner
        || payload.method != method
        || payload.path != path
        || payload.body_hash != protocol::hash_hex(raw_body)
        || payload.expires_at <= now
    {
        return Err("unauthorized: owner proof does not bind this request".into());
    }
    if crate::wallet::recover_signer(&challenge_payload(&payload)?, signature)? != owner {
        return Err("unauthorized: invalid owner signature".into());
    }
    ledger.consume_challenge(nonce, &payload, now)?;
    Ok(owner)
}

pub fn bearer(headers: &HeaderMap) -> Result<&str, String> {
    let value = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or("unauthorized: bearer credential required")?;
    if value.len() != 71 || !value.starts_with("chiral_") {
        return Err("unauthorized: invalid bearer credential".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload() -> AuthPayload {
        AuthPayload {
            network: NetworkIdentity {
                chain_id: 1,
                genesis_hash: "g".into(),
                deployment_id: "d".into(),
            },
            provider_wallet: "p".into(),
            wallet: "w".into(),
            method: "POST".into(),
            path: "/v1/contracts/open".into(),
            body_hash: protocol::hash_hex(b"{}"),
            nonce: "n".into(),
            expires_at: 120,
        }
    }
    #[test]
    fn signature_binds_every_control_field_and_raw_body() {
        let p = payload();
        let original = challenge_payload(&p).unwrap();
        let mut variants = Vec::new();
        let mut v = p.clone();
        v.network.chain_id += 1;
        variants.push(v);
        let mut v = p.clone();
        v.network.genesis_hash.push('x');
        variants.push(v);
        let mut v = p.clone();
        v.network.deployment_id.push('x');
        variants.push(v);
        let mut v = p.clone();
        v.provider_wallet.push('x');
        variants.push(v);
        let mut v = p.clone();
        v.wallet.push('x');
        variants.push(v);
        let mut v = p.clone();
        v.method = "GET".into();
        variants.push(v);
        let mut v = p.clone();
        v.path.push('x');
        variants.push(v);
        let mut v = p.clone();
        v.body_hash = protocol::hash_hex(b"{ }");
        variants.push(v);
        let mut v = p.clone();
        v.nonce.push('x');
        variants.push(v);
        let mut v = p;
        v.expires_at += 1;
        variants.push(v);
        for variant in variants {
            assert_ne!(original, challenge_payload(&variant).unwrap());
        }
    }
}
