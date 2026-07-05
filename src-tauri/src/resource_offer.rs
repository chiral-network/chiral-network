//! Resource offers — the signed DHT records that advertise a provider's
//! sellable resources (storage / container / inference) in the Chiral resource
//! exchange. This generalizes the legacy single-purpose host advertisement
//! (`host_advertisement_payload` in `lib.rs`) into a typed, multi-class offer.
//!
//! Design: `docs/chiral-book.md` — "Resource Offers (Discovery)" and the
//! "Wire Protocol & API Reference (Contract Spine)" section.
//!
//! Trust contract: every offer is ECDSA-signed by the provider wallet over a
//! canonical, length-prefixed, domain-tagged payload (`chiral-offer-v1`).
//! Readers verify before acting and drop unsigned/invalid records; writers
//! refuse to publish without a key. Signing reuses `wallet::sign_message` /
//! `wallet::recover_signer` (keccak256 + secp256k1, low-`s` enforced), so the
//! offer shares the exact signature discipline of every other signed record.
//!
//! Note on encoding: length prefixes are little-endian `u32`, matching the
//! established codebase convention (`dht::file_info_sign_payload`). The book's
//! wire spec labels them `uint32_be`; that is a doc nit to reconcile — only
//! determinism matters for the signature, and both sides use this module.

use serde::{Deserialize, Serialize};

use crate::codec::{canonical_json, keccak256, put_lp};
use crate::wallet;

/// Domain tag mixed into the signed payload so an offer signature can never be
/// replayed as a different record type.
pub const OFFER_DOMAIN_TAG: &[u8] = b"chiral-offer-v1";

/// The three resource classes v1 supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceClass {
    Storage,
    Container,
    Inference,
}

impl ResourceClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceClass::Storage => "storage",
            ResourceClass::Container => "container",
            ResourceClass::Inference => "inference",
        }
    }

    /// Compact wire discriminant used inside the signed payload.
    pub fn wire_byte(&self) -> u8 {
        match self {
            ResourceClass::Storage => 1,
            ResourceClass::Container => 2,
            ResourceClass::Inference => 3,
        }
    }
}

/// A signed advertisement that a provider sells a class of resource at a price,
/// reachable at a public endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceOffer {
    /// 0x-prefixed wallet address; the payee, reputation subject, and signer.
    pub provider_wallet: String,
    pub resource_class: ResourceClass,
    /// Class-specific capacity descriptor (free-form JSON; canonicalized when
    /// signed so key order cannot change the signature).
    pub capacity: serde_json::Value,
    /// Class-specific CHI-per-unit price schedule (free-form JSON).
    pub price_schedule: serde_json::Value,
    /// Public HTTPS base URL of the provider's API.
    pub endpoint: String,
    #[serde(default)]
    pub region: String,
    /// Smallest contract (in wei, decimal string) the provider will open.
    pub min_funding_wei: String,
    /// Monotonic; lets the provider supersede a prior offer under first-claim.
    pub offer_nonce: u64,
    /// Unix seconds after which readers must ignore the offer (0 = no expiry).
    pub valid_until: u64,
    /// Hex ECDSA signature; empty until signed.
    #[serde(default)]
    pub signature: String,
}

impl ResourceOffer {
    /// Canonical, length-prefixed, domain-tagged bytes the signature covers.
    ///
    /// Layout: `tag ‖ lp(provider_wallet) ‖ class_byte ‖ lp(capacity_json)
    /// ‖ lp(price_json) ‖ lp(endpoint) ‖ lp(region) ‖ lp(min_funding_wei)
    /// ‖ offer_nonce(le u64) ‖ valid_until(le u64)`.
    pub fn signing_payload(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        put_lp(&mut out, OFFER_DOMAIN_TAG);
        put_lp(&mut out, self.provider_wallet.to_lowercase().as_bytes());
        out.push(self.resource_class.wire_byte());
        put_lp(&mut out, canonical_json(&self.capacity).as_bytes());
        put_lp(&mut out, canonical_json(&self.price_schedule).as_bytes());
        put_lp(&mut out, self.endpoint.as_bytes());
        put_lp(&mut out, self.region.as_bytes());
        put_lp(&mut out, self.min_funding_wei.as_bytes());
        out.extend_from_slice(&self.offer_nonce.to_le_bytes());
        out.extend_from_slice(&self.valid_until.to_le_bytes());
        out
    }

    /// `keccak256(signing_payload)` — the 32-byte `offer_ref` a consumer commits
    /// to inside a contract transaction. 0x-prefixed hex. Any change to the
    /// offer's terms (price, endpoint, capacity, …) changes the `offer_ref`.
    pub fn offer_ref(&self) -> String {
        format!("0x{}", hex::encode(keccak256(&self.signing_payload())))
    }

    /// DHT key under which the offer is stored (`chiral_offer_<class>_<wallet>`).
    pub fn dht_key(&self) -> String {
        format!(
            "chiral_offer_{}_{}",
            self.resource_class.as_str(),
            self.provider_wallet.to_lowercase()
        )
    }

    /// Per-class Kademlia provider-index key so consumers can enumerate sellers.
    pub fn class_index_key(class: ResourceClass) -> String {
        format!("chiral_offers_{}", class.as_str())
    }

    /// Sign the offer in place with the provider's private key.
    pub fn sign(&mut self, private_key_hex: &str) -> Result<(), String> {
        let payload = self.signing_payload();
        self.signature = wallet::sign_message(private_key_hex, &payload)?;
        Ok(())
    }

    /// Verify the offer: it must be signed, and the recovered signer must equal
    /// `provider_wallet`. Returns `Ok(())` only for a correctly-signed offer.
    /// (Expiry is checked separately via [`is_expired`], since a reader may want
    /// to distinguish "forged" from merely "stale".)
    pub fn verify(&self) -> Result<(), String> {
        if self.signature.trim().is_empty() {
            return Err("offer is unsigned".to_string());
        }
        let payload = self.signing_payload();
        let signer = wallet::recover_signer(&payload, &self.signature)?;
        if signer.to_lowercase() != self.provider_wallet.to_lowercase() {
            return Err(format!(
                "offer signer {} does not match provider_wallet {}",
                signer, self.provider_wallet
            ));
        }
        Ok(())
    }

    /// Whether the offer is past its `valid_until` (0 means never expires).
    pub fn is_expired(&self, now_unix: u64) -> bool {
        self.valid_until != 0 && now_unix > self.valid_until
    }
}

// `put_lp` / `canonical_json` / `keccak256` now live in `crate::codec`.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // A fixed, valid secp256k1 secret key for tests (32 bytes, < curve order).
    const TEST_KEY: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";

    fn sample_offer() -> ResourceOffer {
        ResourceOffer {
            provider_wallet: String::new(),
            resource_class: ResourceClass::Storage,
            capacity: json!({ "gb_available": 500, "max_object_bytes": 5_000_000_000u64 }),
            price_schedule: json!({ "per_gb_month": "10000000000000000", "per_gb_egress": "1000000000000000" }),
            endpoint: "https://provider.example:8443".to_string(),
            region: "us-east".to_string(),
            min_funding_wei: "1000000000000000000".to_string(),
            offer_nonce: 7,
            valid_until: 1_900_000_000,
            signature: String::new(),
        }
    }

    /// The Ethereum address controlled by `TEST_KEY`, derived independently of
    /// any offer payload (sign a fixed probe, recover the signer).
    fn test_address() -> String {
        let probe = b"chiral-test-address-probe";
        let sig = wallet::sign_message(TEST_KEY, probe).expect("probe sign");
        wallet::recover_signer(probe, &sig).expect("probe recover")
    }

    /// A sample offer with `provider_wallet` set to `TEST_KEY`'s address, then
    /// signed — the correct order (the provider knows its address before
    /// signing, and `provider_wallet` is part of the signed payload).
    fn signed_sample() -> ResourceOffer {
        let mut o = sample_offer();
        o.provider_wallet = test_address();
        o.sign(TEST_KEY).expect("sign");
        o
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let o = signed_sample();
        assert!(o.verify().is_ok(), "freshly signed offer must verify");
        assert!(o.provider_wallet.starts_with("0x"));
    }

    #[test]
    fn unsigned_offer_is_rejected() {
        let o = sample_offer();
        assert!(o.verify().is_err(), "unsigned offer must not verify");
    }

    #[test]
    fn tampered_field_breaks_verification() {
        for mutate in [
            |o: &mut ResourceOffer| o.endpoint = "https://evil.example".into(),
            |o: &mut ResourceOffer| o.price_schedule = json!({ "per_gb_month": "1" }),
            |o: &mut ResourceOffer| o.min_funding_wei = "1".into(),
            |o: &mut ResourceOffer| o.offer_nonce = 999,
            |o: &mut ResourceOffer| o.region = "eu-west".into(),
        ] {
            let mut o = signed_sample();
            mutate(&mut o);
            assert!(o.verify().is_err(), "mutation must invalidate the signature");
        }
    }

    #[test]
    fn wrong_provider_wallet_is_rejected() {
        let mut o = signed_sample();
        o.provider_wallet = "0x0000000000000000000000000000000000000000".into();
        assert!(o.verify().is_err(), "signer must match provider_wallet");
    }

    #[test]
    fn offer_ref_is_deterministic_and_binds_terms() {
        let a = sample_offer();
        let b = sample_offer();
        assert_eq!(a.offer_ref(), b.offer_ref(), "same terms => same offer_ref");
        assert!(a.offer_ref().starts_with("0x"));
        assert_eq!(a.offer_ref().len(), 66); // 0x + 32 bytes hex

        let mut c = sample_offer();
        c.endpoint = "https://other.example".into();
        assert_ne!(a.offer_ref(), c.offer_ref(), "different endpoint => different offer_ref");
    }

    #[test]
    fn dht_and_index_keys() {
        let mut o = sample_offer();
        o.provider_wallet = "0xABCDEF0000000000000000000000000000000123".into();
        assert_eq!(
            o.dht_key(),
            "chiral_offer_storage_0xabcdef0000000000000000000000000000000123"
        );
        assert_eq!(
            ResourceOffer::class_index_key(ResourceClass::Inference),
            "chiral_offers_inference"
        );
    }

    #[test]
    fn class_wire_bytes_are_stable() {
        assert_eq!(ResourceClass::Storage.wire_byte(), 1);
        assert_eq!(ResourceClass::Container.wire_byte(), 2);
        assert_eq!(ResourceClass::Inference.wire_byte(), 3);
    }
}
