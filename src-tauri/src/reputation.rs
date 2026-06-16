//! Verifiable reputation issuer keys and transfer verdict signatures.
//!
//! Reputation events are admitted by the HTTP rating API, but the
//! per-event verdict is signed by an Ed25519 issuer key. The issuer key
//! itself is published through the DHT as a wallet-signed record so a
//! verifier can bind `issuer_wallet` to the Ed25519 verifying key before
//! trusting the verdict signature.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::dht::DhtService;

pub const ISSUER_KEY_DHT_PREFIX: &str = "chiral_reputation_issuer_v1_";
const ISSUER_KEY_TAG: &[u8] = b"chiral-reputation-issuer-key-v1";
const VERDICT_TAG: &[u8] = b"chiral-reputation-verdict-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReputationIssuerKeyRecord {
    pub issuer_wallet: String,
    /// Hex-encoded 32-byte Ed25519 verifying key.
    pub verifying_key: String,
    /// Wallet ECDSA signature over `issuer_key_binding_payload`.
    pub owner_signature: String,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReputationVerdictPayload {
    pub transfer_id: String,
    pub seeder_wallet: String,
    pub downloader_wallet: String,
    pub file_hash: String,
    pub amount_wei: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
}

fn push_part(out: &mut Vec<u8>, part: &[u8]) {
    out.extend_from_slice(&(part.len() as u32).to_le_bytes());
    out.extend_from_slice(part);
}

pub fn is_valid_wallet(addr: &str) -> bool {
    let trimmed = addr.trim();
    trimmed.len() == 42
        && trimmed.starts_with("0x")
        && trimmed[2..].chars().all(|c| c.is_ascii_hexdigit())
}

pub fn normalize_wallet(addr: &str) -> Result<String, String> {
    let wallet = addr.trim().to_lowercase();
    if !is_valid_wallet(&wallet) {
        return Err("issuer wallet must be a 0x-prefixed 20-byte hex address".to_string());
    }
    Ok(wallet)
}

fn normalize_hex_key(hex_key: &str) -> Result<String, String> {
    let key = hex_key.trim().trim_start_matches("0x").to_lowercase();
    let bytes = hex::decode(&key).map_err(|e| format!("issuer verifying key is not hex: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!(
            "issuer verifying key must be 32 bytes, got {}",
            bytes.len()
        ));
    }
    Ok(key)
}

fn decode_signature(signature_hex: &str) -> Result<Signature, String> {
    let bytes = hex::decode(signature_hex.trim().trim_start_matches("0x"))
        .map_err(|e| format!("verdict signature is not hex: {e}"))?;
    Signature::from_slice(&bytes).map_err(|e| format!("invalid Ed25519 signature: {e}"))
}

pub fn issuer_key_dht_key(issuer_wallet: &str) -> Result<String, String> {
    Ok(format!(
        "{}{}",
        ISSUER_KEY_DHT_PREFIX,
        normalize_wallet(issuer_wallet)?
    ))
}

pub fn issuer_key_binding_payload(issuer_wallet: &str, verifying_key: &str) -> Vec<u8> {
    let wallet = issuer_wallet.trim().to_lowercase();
    let key = verifying_key.trim().trim_start_matches("0x").to_lowercase();
    let mut out = Vec::with_capacity(ISSUER_KEY_TAG.len() + 96);
    out.extend_from_slice(ISSUER_KEY_TAG);
    push_part(&mut out, wallet.as_bytes());
    push_part(&mut out, key.as_bytes());
    out
}

pub fn verdict_signing_payload(verdict: &ReputationVerdictPayload) -> Vec<u8> {
    let mut out = Vec::with_capacity(VERDICT_TAG.len() + 256);
    out.extend_from_slice(VERDICT_TAG);
    let seeder_wallet = verdict.seeder_wallet.trim().to_lowercase();
    let downloader_wallet = verdict.downloader_wallet.trim().to_lowercase();
    let outcome = verdict.outcome.trim().to_lowercase();
    let tx_hash = verdict.tx_hash.as_deref().unwrap_or("").trim();
    for part in [
        verdict.transfer_id.trim().as_bytes(),
        seeder_wallet.as_bytes(),
        downloader_wallet.as_bytes(),
        verdict.file_hash.trim().as_bytes(),
        verdict.amount_wei.trim().as_bytes(),
        outcome.as_bytes(),
        tx_hash.as_bytes(),
    ] {
        push_part(&mut out, part);
    }
    out
}

pub fn validate_issuer_key_record(
    record: &ReputationIssuerKeyRecord,
) -> Result<VerifyingKey, String> {
    let issuer_wallet = normalize_wallet(&record.issuer_wallet)?;
    let verifying_key = normalize_hex_key(&record.verifying_key)?;
    let key_bytes =
        hex::decode(&verifying_key).map_err(|e| format!("issuer verifying key is not hex: {e}"))?;
    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| "issuer verifying key must be 32 bytes".to_string())?;
    let key = VerifyingKey::from_bytes(&key_array)
        .map_err(|e| format!("invalid Ed25519 verifying key: {e}"))?;
    let binding_payload = issuer_key_binding_payload(&issuer_wallet, &verifying_key);
    if !crate::wallet::verify_signature(&binding_payload, &record.owner_signature, &issuer_wallet) {
        return Err("issuer key owner signature does not verify against issuer wallet".to_string());
    }
    Ok(key)
}

pub fn validate_issuer_key_record_for_wallet(
    record: &ReputationIssuerKeyRecord,
    requested_wallet: &str,
) -> Result<VerifyingKey, String> {
    let requested_wallet = normalize_wallet(requested_wallet)?;
    let record_wallet = normalize_wallet(&record.issuer_wallet)?;
    if record_wallet != requested_wallet {
        return Err(format!(
            "issuer key record wallet {record_wallet} does not match requested wallet {requested_wallet}"
        ));
    }
    validate_issuer_key_record(record)
}

pub fn verify_reputation_verdict(
    issuer_record: &ReputationIssuerKeyRecord,
    verdict: &ReputationVerdictPayload,
    signature_hex: &str,
) -> Result<(), String> {
    let key = validate_issuer_key_record(issuer_record)?;
    let signature = decode_signature(signature_hex)?;
    key.verify(&verdict_signing_payload(verdict), &signature)
        .map_err(|_| "reputation verdict signature does not match issuer key".to_string())
}

pub fn verify_reputation_verdict_for_wallet(
    issuer_record: &ReputationIssuerKeyRecord,
    requested_wallet: &str,
    verdict: &ReputationVerdictPayload,
    signature_hex: &str,
) -> Result<(), String> {
    let key = validate_issuer_key_record_for_wallet(issuer_record, requested_wallet)?;
    let signature = decode_signature(signature_hex)?;
    key.verify(&verdict_signing_payload(verdict), &signature)
        .map_err(|_| "reputation verdict signature does not match issuer key".to_string())
}

pub async fn publish_issuer_key(
    dht: &Arc<DhtService>,
    record: ReputationIssuerKeyRecord,
) -> Result<(), String> {
    validate_issuer_key_record(&record)?;
    let key = issuer_key_dht_key(&record.issuer_wallet)?;
    let value = serde_json::to_string(&record)
        .map_err(|e| format!("failed to serialize reputation issuer key: {e}"))?;
    dht.put_dht_value(key, value).await
}

pub async fn fetch_issuer_key(
    dht: &Arc<DhtService>,
    issuer_wallet: &str,
) -> Result<Option<ReputationIssuerKeyRecord>, String> {
    let key = issuer_key_dht_key(issuer_wallet)?;
    let Some(raw) = dht.get_dht_value(key).await? else {
        return Ok(None);
    };
    let record: ReputationIssuerKeyRecord = serde_json::from_str(&raw)
        .map_err(|e| format!("invalid reputation issuer key record: {e}"))?;
    validate_issuer_key_record_for_wallet(&record, issuer_wallet)?;
    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    const TEST_PRIVATE_KEY: &str =
        "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const OTHER_PRIVATE_KEY: &str =
        "0x1111111111111111111111111111111111111111111111111111111111111111";

    fn signed_issuer_record(signing: &SigningKey) -> ReputationIssuerKeyRecord {
        signed_issuer_record_for_key(signing, TEST_PRIVATE_KEY)
    }

    fn signed_issuer_record_for_key(
        signing: &SigningKey,
        wallet_private_key: &str,
    ) -> ReputationIssuerKeyRecord {
        let issuer_wallet = wallet_from_private_key(wallet_private_key);
        let verifying_key = hex::encode(signing.verifying_key().to_bytes());
        let payload = issuer_key_binding_payload(&issuer_wallet, &verifying_key);
        let owner_signature = crate::wallet::sign_message(wallet_private_key, &payload).unwrap();
        ReputationIssuerKeyRecord {
            issuer_wallet,
            verifying_key,
            owner_signature,
            updated_at: 1_700_000_000,
        }
    }

    fn test_wallet() -> String {
        wallet_from_private_key(TEST_PRIVATE_KEY)
    }

    fn wallet_from_private_key(private_key: &str) -> String {
        let probe = b"wallet-probe";
        let sig = crate::wallet::sign_message(private_key, probe).unwrap();
        crate::wallet::recover_signer(probe, &sig).unwrap()
    }

    fn verdict() -> ReputationVerdictPayload {
        ReputationVerdictPayload {
            transfer_id: "transfer-1".to_string(),
            seeder_wallet: "0x1111111111111111111111111111111111111111".to_string(),
            downloader_wallet: test_wallet(),
            file_hash: "abc123".to_string(),
            amount_wei: "1000".to_string(),
            outcome: "completed".to_string(),
            tx_hash: Some("0xtx".to_string()),
        }
    }

    #[test]
    fn valid_issuer_key_verifies_verdict_signature() {
        let signing = SigningKey::generate(&mut OsRng);
        let record = signed_issuer_record(&signing);
        let payload = verdict();
        let sig = signing.sign(&verdict_signing_payload(&payload));

        verify_reputation_verdict(&record, &payload, &hex::encode(sig.to_bytes()))
            .expect("valid issuer verdict should verify");
        verify_reputation_verdict_for_wallet(
            &record,
            &test_wallet(),
            &payload,
            &hex::encode(sig.to_bytes()),
        )
        .expect("valid issuer verdict should verify for requested wallet");
    }

    #[test]
    fn mismatched_issuer_key_rejects_verdict_signature() {
        let signing = SigningKey::generate(&mut OsRng);
        let other_signing = SigningKey::generate(&mut OsRng);
        let record = signed_issuer_record(&other_signing);
        let payload = verdict();
        let sig = signing.sign(&verdict_signing_payload(&payload));

        let err = verify_reputation_verdict(&record, &payload, &hex::encode(sig.to_bytes()))
            .expect_err("mismatched issuer key must reject");
        assert!(err.contains("verdict signature"));
    }

    #[test]
    fn missing_or_unbound_issuer_key_rejects_before_verdict_trust() {
        let signing = SigningKey::generate(&mut OsRng);
        let mut record = signed_issuer_record(&signing);
        record.verifying_key = hex::encode([7u8; 32]);
        let payload = verdict();
        let sig = signing.sign(&verdict_signing_payload(&payload));

        let err = verify_reputation_verdict(&record, &payload, &hex::encode(sig.to_bytes()))
            .expect_err("unbound issuer key must reject");
        assert!(err.contains("owner signature"));

        let invalid_key = issuer_key_dht_key("not-a-wallet").expect_err("invalid wallet rejected");
        assert!(invalid_key.contains("issuer wallet"));
    }

    #[test]
    fn issuer_key_record_for_different_wallet_rejects_requested_wallet() {
        let signing = SigningKey::generate(&mut OsRng);
        let record_for_other_wallet = signed_issuer_record_for_key(&signing, OTHER_PRIVATE_KEY);
        let payload = verdict();
        let sig = signing.sign(&verdict_signing_payload(&payload));

        validate_issuer_key_record(&record_for_other_wallet)
            .expect("record should be valid for its own wallet");
        let err = verify_reputation_verdict_for_wallet(
            &record_for_other_wallet,
            &test_wallet(),
            &payload,
            &hex::encode(sig.to_bytes()),
        )
        .expect_err("wallet B record must not satisfy wallet A lookup");
        assert!(err.contains("does not match requested wallet"));
    }

    #[test]
    fn missing_issuer_key_material_rejects_verdict_signature() {
        let signing = SigningKey::generate(&mut OsRng);
        let record = ReputationIssuerKeyRecord {
            issuer_wallet: test_wallet(),
            verifying_key: String::new(),
            owner_signature: String::new(),
            updated_at: 1_700_000_000,
        };
        let payload = verdict();
        let sig = signing.sign(&verdict_signing_payload(&payload));

        let err = verify_reputation_verdict(&record, &payload, &hex::encode(sig.to_bytes()))
            .expect_err("missing issuer key material must reject");
        assert!(err.contains("verifying key"));
    }
}
