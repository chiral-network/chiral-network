//! Usage receipts — a provider-wallet-signed statement of what a contract was
//! charged, so a consumer holds portable, attributable evidence for disputes
//! and reputation ratings.
//!
//! Design: `docs/chiral-book.md` — resolved Design Decision #5 and the
//! `GET /v1/contracts/:id/receipt` wire format. The receipt is signed by the
//! provider wallet over a canonical, length-prefixed, domain-tagged
//! (`chiral-receipt-v1`) payload, and verified with the same
//! `wallet::sign_message` / `recover_signer` primitives as every other signed
//! record.

use serde::{Deserialize, Serialize};

use crate::codec::{canonical_json, put_lp};
use crate::wallet;

pub const RECEIPT_DOMAIN_TAG: &[u8] = b"chiral-receipt-v1";

/// A signed usage statement for a single contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageReceipt {
    pub contract_id: String,
    pub provider_wallet: String,
    /// Gross funded, net-of-fee spent, and remaining balance — decimal wei.
    pub funded_wei: String,
    pub spent_wei: String,
    pub balance_wei: String,
    /// Class-specific meters (free-form JSON; canonicalized when signed).
    pub usage: serde_json::Value,
    /// Unix seconds the receipt was issued.
    pub as_of: u64,
    #[serde(default)]
    pub signature: String,
}

impl UsageReceipt {
    /// Canonical bytes the provider signs (excludes `provider_wallet`, which is
    /// recovered from the signature and checked in [`verify`], and `signature`).
    pub fn signing_payload(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(160);
        put_lp(&mut out, RECEIPT_DOMAIN_TAG);
        put_lp(&mut out, self.contract_id.to_lowercase().as_bytes());
        put_lp(&mut out, self.funded_wei.as_bytes());
        put_lp(&mut out, self.spent_wei.as_bytes());
        put_lp(&mut out, self.balance_wei.as_bytes());
        put_lp(&mut out, canonical_json(&self.usage).as_bytes());
        out.extend_from_slice(&self.as_of.to_le_bytes());
        out
    }

    /// Sign the receipt in place with the provider's private key.
    pub fn sign(&mut self, provider_private_key_hex: &str) -> Result<(), String> {
        self.signature = wallet::sign_message(provider_private_key_hex, &self.signing_payload())?;
        Ok(())
    }

    /// Verify the receipt is signed and the recovered signer equals
    /// `provider_wallet`.
    pub fn verify(&self) -> Result<(), String> {
        if self.signature.trim().is_empty() {
            return Err("receipt is unsigned".to_string());
        }
        let signer = wallet::recover_signer(&self.signing_payload(), &self.signature)?;
        if signer.to_lowercase() != self.provider_wallet.to_lowercase() {
            return Err(format!(
                "receipt signer {} does not match provider_wallet {}",
                signer, self.provider_wallet
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEST_KEY: &str = "0x2222222222222222222222222222222222222222222222222222222222222222";

    fn test_address() -> String {
        let probe = b"chiral-test-address-probe";
        let sig = wallet::sign_message(TEST_KEY, probe).expect("probe sign");
        wallet::recover_signer(probe, &sig).expect("probe recover")
    }

    fn sample() -> UsageReceipt {
        UsageReceipt {
            contract_id: "0x".to_string() + &"ab".repeat(32),
            provider_wallet: String::new(),
            funded_wei: "1000000000000000000".to_string(),
            spent_wei: "12500000000000".to_string(),
            balance_wei: "994987500000000000".to_string(),
            usage: json!({ "gb_month": 0.5, "egress_gb": 1.2 }),
            as_of: 1_712_349_278,
            signature: String::new(),
        }
    }

    fn signed_sample() -> UsageReceipt {
        let mut r = sample();
        r.provider_wallet = test_address();
        r.sign(TEST_KEY).expect("sign");
        r
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let r = signed_sample();
        assert!(r.verify().is_ok());
    }

    #[test]
    fn unsigned_is_rejected() {
        assert!(sample().verify().is_err());
    }

    #[test]
    fn tampered_fields_break_verification() {
        for mutate in [
            |r: &mut UsageReceipt| r.spent_wei = "1".into(),
            |r: &mut UsageReceipt| r.balance_wei = "0".into(),
            |r: &mut UsageReceipt| r.usage = json!({ "gb_month": 99 }),
            |r: &mut UsageReceipt| r.as_of = 0,
            |r: &mut UsageReceipt| r.contract_id = "0xdead".into(),
        ] {
            let mut r = signed_sample();
            mutate(&mut r);
            assert!(r.verify().is_err());
        }
    }

    #[test]
    fn wrong_provider_wallet_is_rejected() {
        let mut r = signed_sample();
        r.provider_wallet = "0x0000000000000000000000000000000000000000".into();
        assert!(r.verify().is_err());
    }
}
