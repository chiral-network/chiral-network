//! Consumer-side resource discovery — the mirror of the provider's offer
//! publication (`provider_daemon::publish_offer`). A buyer enumerates the sellers
//! of a resource class off the DHT, fetches each one's signed offer, and keeps
//! only the records whose signature verifies.
//!
//! Discovery keys by **peer id**: the per-class provider index
//! (`chiral_offers_<class>`) resolves to peer ids, and the offer each provider
//! stored under `chiral_offer_<class>_<peer_id>` is fetched by that peer id (see
//! [`ResourceOffer::peer_dht_key`]). The offer's ECDSA signature still binds the
//! provider's wallet (the payee), which the consumer recovers and checks — so the
//! peer-id keying is only a lookup convenience, never a trust input. A forged,
//! malformed, wrong-class, or otherwise unverifiable record is silently dropped.
//!
//! The trust filter ([`accept_offer_record`]) and ranking ([`rank_by_price`]) are
//! pure so they're unit-testable without a live DHT; [`search_offers`] wires them
//! to the transport.

use std::collections::HashSet;

use crate::dht::DhtService;
use crate::resource_offer::{ResourceClass, ResourceOffer};

/// Parse and verify a fetched offer record, accepting it only when its signature
/// is valid *and* its class matches what we searched for. Pure (no network), so
/// the trust filter is testable in isolation.
///
/// Returns `None` for malformed JSON, a class mismatch, or a bad/missing
/// signature — the caller treats `None` as "skip this record".
pub fn accept_offer_record(json: &str, class: ResourceClass) -> Option<ResourceOffer> {
    let offer: ResourceOffer = serde_json::from_str(json).ok()?;
    if offer.resource_class != class {
        return None;
    }
    offer.verify().ok()?;
    Some(offer)
}

/// Sort offers ascending by a numeric wei field in their price schedule (e.g.
/// `"per_gb_month"` for storage, `"per_vcpu_hour_wei"` for compute,
/// `"per_1k_output_wei"` for inference). Offers missing or with a non-numeric
/// field sort last. Pure — the ranking policy is testable without a network.
///
/// Reputation-aware ranking layers on top: the caller fetches each
/// `provider_wallet`'s Elo from the rating API and re-orders. Kept separate so
/// discovery has no dependency on the reputation service.
pub fn rank_by_price(mut offers: Vec<ResourceOffer>, price_field: &str) -> Vec<ResourceOffer> {
    offers.sort_by_key(|o| price_wei(o, price_field).unwrap_or(u128::MAX));
    offers
}

/// Read a `u128` wei value out of an offer's price schedule (stringified to avoid
/// JSON's 2^53 precision loss), if present and parseable.
pub fn price_wei(offer: &ResourceOffer, field: &str) -> Option<u128> {
    offer
        .price_schedule
        .get(field)
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u128>().ok())
}

/// Discover verified offers for a resource class:
///
/// 1. enumerate the class provider-index (`chiral_offers_<class>`) → peer ids,
/// 2. fetch each peer's offer record (`chiral_offer_<class>_<peer_id>`),
/// 3. keep only records that parse, verify, and match the class.
///
/// Every returned offer is signature-verified, so the caller may trust its
/// `provider_wallet` / `endpoint` / `price_schedule`. At most one offer per
/// provider wallet is returned (first verified record wins) so a provider that
/// answers under several peer ids can't flood the results. Fetch is best-effort
/// per peer: a peer with no record or a transient get error is skipped, not fatal.
pub async fn search_offers(
    dht: &DhtService,
    class: ResourceClass,
) -> Result<Vec<ResourceOffer>, String> {
    let peers = dht
        .get_file_providers(ResourceOffer::class_index_key(class))
        .await?;

    let mut offers = Vec::new();
    let mut seen_wallets = HashSet::new();
    for peer in peers {
        let key = ResourceOffer::peer_dht_key(class, &peer);
        // A missing record or transient get error just means "skip this peer".
        if let Ok(Some(json)) = dht.get_dht_value(key).await {
            if let Some(offer) = accept_offer_record(&json, class) {
                if seen_wallets.insert(offer.provider_wallet.to_lowercase()) {
                    offers.push(offer);
                }
            }
        }
    }
    Ok(offers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet;
    use serde_json::json;

    const TEST_KEY: &str = "0x2222222222222222222222222222222222222222222222222222222222222222";

    fn test_address() -> String {
        let probe = b"chiral-discovery-test-probe";
        let sig = wallet::sign_message(TEST_KEY, probe).expect("probe sign");
        wallet::recover_signer(probe, &sig).expect("probe recover")
    }

    fn signed_offer(class: ResourceClass, price_field: &str, price: &str) -> ResourceOffer {
        let mut o = ResourceOffer {
            provider_wallet: test_address(),
            resource_class: class,
            capacity: json!({ "gb_available": 100 }),
            price_schedule: json!({ price_field: price }),
            endpoint: "https://p.example:8443".to_string(),
            region: "us-east".to_string(),
            min_funding_wei: "1000000000000000000".to_string(),
            offer_nonce: 1,
            valid_until: 0,
            signature: String::new(),
        };
        o.sign(TEST_KEY).expect("sign");
        o
    }

    #[test]
    fn accepts_a_valid_matching_offer() {
        let o = signed_offer(ResourceClass::Storage, "per_gb_month", "1000");
        let json = serde_json::to_string(&o).unwrap();
        let got = accept_offer_record(&json, ResourceClass::Storage).expect("should accept");
        assert_eq!(got.provider_wallet, o.provider_wallet);
    }

    #[test]
    fn rejects_class_mismatch() {
        // A storage offer must not be returned when searching for inference, even
        // though it's validly signed.
        let o = signed_offer(ResourceClass::Storage, "per_gb_month", "1000");
        let json = serde_json::to_string(&o).unwrap();
        assert!(accept_offer_record(&json, ResourceClass::Inference).is_none());
    }

    #[test]
    fn rejects_forged_signature() {
        // Tamper with a signed offer's terms without re-signing: the recovered
        // signer no longer matches provider_wallet, so verify() fails.
        let mut o = signed_offer(ResourceClass::Storage, "per_gb_month", "1000");
        o.endpoint = "https://evil.example".to_string();
        let json = serde_json::to_string(&o).unwrap();
        assert!(accept_offer_record(&json, ResourceClass::Storage).is_none());
    }

    #[test]
    fn rejects_unsigned_and_malformed() {
        let mut o = signed_offer(ResourceClass::Storage, "per_gb_month", "1000");
        o.signature = String::new();
        let unsigned = serde_json::to_string(&o).unwrap();
        assert!(accept_offer_record(&unsigned, ResourceClass::Storage).is_none());
        assert!(accept_offer_record("not json", ResourceClass::Storage).is_none());
    }

    #[test]
    fn ranks_cheapest_first_missing_last() {
        let cheap = signed_offer(ResourceClass::Storage, "per_gb_month", "1000");
        let dear = signed_offer(ResourceClass::Storage, "per_gb_month", "9000");
        let mut no_price = signed_offer(ResourceClass::Storage, "per_gb_month", "1");
        no_price.price_schedule = json!({ "unrelated": "5" }); // missing the ranked field

        let ranked = rank_by_price(vec![dear.clone(), no_price.clone(), cheap.clone()], "per_gb_month");
        assert_eq!(price_wei(&ranked[0], "per_gb_month"), Some(1000));
        assert_eq!(price_wei(&ranked[1], "per_gb_month"), Some(9000));
        assert_eq!(price_wei(&ranked[2], "per_gb_month"), None); // missing sorts last
    }
}
