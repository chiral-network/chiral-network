//! Service contracts — the on-chain agreement between a consumer and a provider
//! in the resource exchange. A contract is committed as a single signed
//! transaction whose `value` prepays the balance and whose `data` carries a
//! compact commitment to the agreed terms. This module owns the wire codec for
//! that `data` field and the terms-hash computation; the HTTP handshake and the
//! provider-side balance ledger build on top of it.
//!
//! Design: `docs/chiral-book.md` — "Service Contracts and the Handshake" and
//! the contract-transaction `data` layout in the wire-protocol section.
//!
//! `data` layouts:
//!   open  (`CHR1`): magic(4) ‖ offer_ref(32) ‖ terms_hash(32) ‖ nonce(16) = 84
//!   topup (`CHR2`): magic(4) ‖ contract_id(32)                            = 36

use serde::{Deserialize, Serialize};

use crate::codec::{canonical_json, keccak256};
use crate::resource_offer::ResourceClass;

/// Magic tagging a contract-opening tx: `CHR1`.
pub const MAGIC_OPEN: [u8; 4] = *b"CHR1";
/// Magic tagging a top-up tx: `CHR2`.
pub const MAGIC_TOPUP: [u8; 4] = *b"CHR2";

pub const OPEN_DATA_LEN: usize = 4 + 32 + 32 + 16; // 84
pub const TOPUP_DATA_LEN: usize = 4 + 32; // 36

/// Parsed contract-transaction `data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractData {
    Open {
        offer_ref: [u8; 32],
        terms_hash: [u8; 32],
        contract_nonce: [u8; 16],
    },
    TopUp {
        contract_id: [u8; 32],
    },
}

/// Encode the `data` field for a contract-opening transaction.
pub fn encode_open(
    offer_ref: &[u8; 32],
    terms_hash: &[u8; 32],
    contract_nonce: &[u8; 16],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(OPEN_DATA_LEN);
    out.extend_from_slice(&MAGIC_OPEN);
    out.extend_from_slice(offer_ref);
    out.extend_from_slice(terms_hash);
    out.extend_from_slice(contract_nonce);
    out
}

/// Encode the `data` field for a top-up transaction referencing a contract.
pub fn encode_topup(contract_id: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(TOPUP_DATA_LEN);
    out.extend_from_slice(&MAGIC_TOPUP);
    out.extend_from_slice(contract_id);
    out
}

/// Parse a contract-transaction `data` field into its typed form. Rejects an
/// unknown magic or a wrong length rather than silently truncating.
pub fn parse(data: &[u8]) -> Result<ContractData, String> {
    if data.len() < 4 {
        return Err("contract data too short for magic".to_string());
    }
    match &data[..4] {
        m if m == MAGIC_OPEN => {
            if data.len() != OPEN_DATA_LEN {
                return Err(format!(
                    "CHR1 data must be {} bytes, got {}",
                    OPEN_DATA_LEN,
                    data.len()
                ));
            }
            let mut offer_ref = [0u8; 32];
            let mut terms_hash = [0u8; 32];
            let mut contract_nonce = [0u8; 16];
            offer_ref.copy_from_slice(&data[4..36]);
            terms_hash.copy_from_slice(&data[36..68]);
            contract_nonce.copy_from_slice(&data[68..84]);
            Ok(ContractData::Open {
                offer_ref,
                terms_hash,
                contract_nonce,
            })
        }
        m if m == MAGIC_TOPUP => {
            if data.len() != TOPUP_DATA_LEN {
                return Err(format!(
                    "CHR2 data must be {} bytes, got {}",
                    TOPUP_DATA_LEN,
                    data.len()
                ));
            }
            let mut contract_id = [0u8; 32];
            contract_id.copy_from_slice(&data[4..36]);
            Ok(ContractData::TopUp { contract_id })
        }
        _ => Err("unrecognized contract data magic".to_string()),
    }
}

/// The full negotiated terms of a contract. The provider quotes these in the
/// handshake; the consumer commits `terms_hash` on-chain; the provider
/// recomputes and checks equality at `open`. `params`/`rates` are class-specific
/// (see the data-plane sections).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractTerms {
    pub offer_ref: String,
    pub provider_wallet: String,
    pub consumer_wallet: String,
    pub resource_class: ResourceClass,
    pub funding_amount_wei: String,
    pub rates: serde_json::Value,
    pub params: serde_json::Value,
    pub contract_nonce: String,
    pub expiry: u64,
}

impl ContractTerms {
    /// `keccak256(canonical_json(terms))` — the on-chain commitment. Both sides
    /// compute it identically.
    pub fn terms_hash(&self) -> Result<[u8; 32], String> {
        let value = serde_json::to_value(self).map_err(|e| format!("terms serialize: {}", e))?;
        Ok(keccak256(canonical_json(&value).as_bytes()))
    }

    pub fn terms_hash_hex(&self) -> Result<String, String> {
        Ok(format!("0x{}", hex::encode(self.terms_hash()?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_terms() -> ContractTerms {
        ContractTerms {
            offer_ref: "0x".to_string() + &"11".repeat(32),
            provider_wallet: "0xprovider".to_string(),
            consumer_wallet: "0xconsumer".to_string(),
            resource_class: ResourceClass::Storage,
            funding_amount_wei: "1000000000000000000".to_string(),
            rates: json!({ "per_gb_month": "10000000000000000" }),
            params: json!({ "bucket": "c-abc" }),
            contract_nonce: "0x".to_string() + &"22".repeat(16),
            expiry: 1_900_000_000,
        }
    }

    #[test]
    fn open_data_roundtrips() {
        let offer_ref = [0xAAu8; 32];
        let terms_hash = [0xBBu8; 32];
        let nonce = [0xCCu8; 16];
        let data = encode_open(&offer_ref, &terms_hash, &nonce);
        assert_eq!(data.len(), OPEN_DATA_LEN);
        assert_eq!(&data[..4], &MAGIC_OPEN);
        match parse(&data).unwrap() {
            ContractData::Open {
                offer_ref: o,
                terms_hash: t,
                contract_nonce: n,
            } => {
                assert_eq!(o, offer_ref);
                assert_eq!(t, terms_hash);
                assert_eq!(n, nonce);
            }
            other => panic!("expected Open, got {:?}", other),
        }
    }

    #[test]
    fn topup_data_roundtrips() {
        let contract_id = [0x42u8; 32];
        let data = encode_topup(&contract_id);
        assert_eq!(data.len(), TOPUP_DATA_LEN);
        assert_eq!(parse(&data).unwrap(), ContractData::TopUp { contract_id });
    }

    #[test]
    fn parse_rejects_bad_magic_and_length() {
        assert!(parse(b"XXXX").is_err()); // unknown magic (and wrong len)
        assert!(parse(&[]).is_err()); // too short
        let mut short_open = encode_open(&[0u8; 32], &[0u8; 32], &[0u8; 16]);
        short_open.pop(); // 83 bytes
        assert!(parse(&short_open).is_err());
        let mut long_topup = encode_topup(&[0u8; 32]);
        long_topup.push(0); // 37 bytes
        assert!(parse(&long_topup).is_err());
    }

    #[test]
    fn magics_and_lengths_are_stable() {
        assert_eq!(&MAGIC_OPEN, b"CHR1");
        assert_eq!(&MAGIC_TOPUP, b"CHR2");
        assert_eq!(OPEN_DATA_LEN, 84);
        assert_eq!(TOPUP_DATA_LEN, 36);
    }

    #[test]
    fn terms_hash_is_deterministic_and_binds_fields() {
        let a = sample_terms();
        let b = sample_terms();
        assert_eq!(a.terms_hash().unwrap(), b.terms_hash().unwrap());
        assert!(a.terms_hash_hex().unwrap().starts_with("0x"));
        assert_eq!(a.terms_hash_hex().unwrap().len(), 66);

        let mut c = sample_terms();
        c.funding_amount_wei = "1".to_string();
        assert_ne!(a.terms_hash().unwrap(), c.terms_hash().unwrap());
    }
}
