//! Consumer-side contract client — turns a discovered [`ResourceOffer`] into an
//! open contract with a session credential, by driving the provider's handshake:
//!
//! 1. **propose** — `POST /v1/contracts/propose` with the offer_ref + funding →
//!    a signed [`Quote`] (terms_hash, contract_nonce, payee).
//! 2. **commit on-chain** — build the `CHR1` calldata committing to
//!    `(offer_ref, terms_hash, contract_nonce)` and broadcast a funding tx to the
//!    provider's wallet ([`crate::wallet::send_transaction_with_data`]).
//! 3. **open** — `POST /v1/contracts/open` with the tx hash; poll while the
//!    provider reports the tx as pending, until it returns the session credential.
//!
//! The trust-critical, network-free steps — checking the quote is for the offer
//! we discovered ([`verify_quote`]) and building the exact commitment calldata
//! ([`build_open_tx_data`]) — are pure and unit-tested. [`open_contract`] wires
//! them to HTTP + the chain.

use std::time::Duration;

use serde_json::json;

use crate::codec::hex_to_array;
use crate::contract_service::{OpenOutcome, ProposeRequest, Quote};
use crate::resource_offer::ResourceOffer;
use crate::service_contract::encode_open;
use crate::session_credential::SessionCredential;

/// Check a returned quote is for the offer we actually discovered and names that
/// offer's provider as the payee — so a tampered or substituted provider can't
/// get us to fund a different recipient or different terms. Pure.
pub fn verify_quote(offer: &ResourceOffer, quote: &Quote) -> Result<(), String> {
    if quote.terms.offer_ref.to_lowercase() != offer.offer_ref().to_lowercase() {
        return Err("quote offer_ref does not match the discovered offer".to_string());
    }
    if quote.provider_wallet.to_lowercase() != offer.provider_wallet.to_lowercase() {
        return Err("quote provider_wallet does not match the offer's payee".to_string());
    }
    if quote.terms.provider_wallet.to_lowercase() != offer.provider_wallet.to_lowercase() {
        return Err("quote terms name a different provider_wallet".to_string());
    }
    Ok(())
}

/// Build the `CHR1` contract-open `data` field committing to
/// `(offer_ref, terms_hash, contract_nonce)`. Verifies the quote first, then
/// decodes the three fields to fixed-width arrays — a malformed hex length is a
/// hard error rather than a silently truncated commitment. Pure; the bytes go
/// straight into the on-chain funding tx.
pub fn build_open_tx_data(offer: &ResourceOffer, quote: &Quote) -> Result<Vec<u8>, String> {
    verify_quote(offer, quote)?;
    let offer_ref: [u8; 32] = hex_to_array(&offer.offer_ref())?;
    let terms_hash: [u8; 32] = hex_to_array(&quote.terms_hash)?;
    let contract_nonce: [u8; 16] = hex_to_array(&quote.contract_nonce)?;
    Ok(encode_open(&offer_ref, &terms_hash, &contract_nonce))
}

/// The result of a successful [`open_contract`]: an active contract with a
/// session credential the consumer uses against the data plane.
#[derive(Debug, Clone)]
pub struct OpenedContract {
    pub contract_id: String,
    pub credential: SessionCredential,
    pub balance_wei: String,
    pub expires_at: u64,
    pub tx_hash: String,
}

/// Drive the full handshake against a provider: propose, commit on-chain, open.
///
/// `funding_chi` is the deposit (in CHI) — it becomes both the quoted
/// `funding_amount_wei` and the funding tx value (one CHI→wei conversion, so no
/// rounding drift). `rpc_endpoints` is the ordered fallback list for broadcasting
/// the tx (`geth::wallet_rpc_endpoints()`). Polls `open` for up to ~5 minutes
/// while the provider reports the tx as still-pending, then gives up with the
/// provider's retry hint surfaced in the error.
///
/// The deposit is **non-refundable** once broadcast (it moves directly to the
/// provider); this returns only after the provider has acknowledged it and issued
/// a credential, or errors with the on-chain tx hash so the caller can reconcile.
pub async fn open_contract(
    http: &reqwest::Client,
    provider_base_url: &str,
    offer: &ResourceOffer,
    consumer_wallet: &str,
    funding_chi: &str,
    private_key: &str,
    rpc_endpoints: &[String],
) -> Result<OpenedContract, String> {
    let base = provider_base_url.trim_end_matches('/');
    let funding_wei = crate::wallet::parse_chi_to_wei(funding_chi)?;

    // 1. propose — get a quote bound to this offer.
    let propose_req = ProposeRequest {
        offer_ref: offer.offer_ref(),
        consumer_wallet: consumer_wallet.to_string(),
        funding_amount_wei: funding_wei.to_string(),
        params: json!({}),
    };
    let quote: Quote = http
        .post(format!("{base}/v1/contracts/propose"))
        .json(&propose_req)
        .send()
        .await
        .map_err(|e| format!("propose request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("propose rejected: {e}"))?
        .json()
        .await
        .map_err(|e| format!("propose response was not a quote: {e}"))?;

    // 2. commit on-chain — build the CHR1 calldata and broadcast the funding tx.
    let data = build_open_tx_data(offer, &quote)?;
    let sent = crate::wallet::send_transaction_with_data(
        rpc_endpoints,
        consumer_wallet,
        &quote.provider_wallet,
        funding_chi,
        &data,
        private_key,
    )
    .await
    .map_err(|e| format!("funding tx failed: {e}"))?;

    // 3. open — poll while the provider still sees the tx as unconfirmed.
    let deadline_polls = 30u32; // ~5 min at 10s cap
    for _ in 0..deadline_polls {
        let resp = http
            .post(format!("{base}/v1/contracts/open"))
            .json(&json!({ "tx_hash": sent.hash }))
            .send()
            .await
            .map_err(|e| format!("open request failed (tx {}): {e}", sent.hash))?;
        let outcome: OpenOutcome = resp
            .json()
            .await
            .map_err(|e| format!("open response was not an outcome (tx {}): {e}", sent.hash))?;
        match outcome {
            OpenOutcome::Open {
                contract_id,
                balance_wei,
                credential,
                expires_at,
                ..
            } => {
                return Ok(OpenedContract {
                    contract_id,
                    credential,
                    balance_wei,
                    expires_at,
                    tx_hash: sent.hash,
                });
            }
            OpenOutcome::Pending { retry_after_s, .. } => {
                tokio::time::sleep(Duration::from_secs(retry_after_s.clamp(1, 10))).await;
            }
        }
    }
    Err(format!(
        "contract still pending after polling; funding tx {} is on-chain — retry open with it",
        sent.hash
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract_service::{ChainVerifier, ProviderState, VerifiedFunding};
    use crate::resource_offer::ResourceClass;
    use crate::service_contract::{parse, ContractData};
    use crate::wallet;
    use serde_json::json as jval;

    const PROVIDER_KEY: &str = "0x3333333333333333333333333333333333333333333333333333333333333333";

    /// A verifier that is never invoked in these tests (only `propose` is driven,
    /// and `propose` does no on-chain check) — it just satisfies the type bound.
    struct NoChain;
    impl ChainVerifier for NoChain {
        fn verify_funding(&self, _tx_hash: &str, _expected_to: &str) -> Result<VerifiedFunding, String> {
            Err("not used in these tests".to_string())
        }
    }

    fn provider_address() -> String {
        let probe = b"chiral-consumer-test-probe";
        let sig = wallet::sign_message(PROVIDER_KEY, probe).unwrap();
        wallet::recover_signer(probe, &sig).unwrap()
    }

    fn signed_offer() -> ResourceOffer {
        let mut o = ResourceOffer {
            provider_wallet: provider_address(),
            resource_class: ResourceClass::Storage,
            capacity: jval!({ "gb_available": 100, "max_object_bytes": 5_000_000u64 }),
            price_schedule: jval!({ "per_gb_month": "10000000000000000", "per_gb_egress": "1000000000000000" }),
            endpoint: "https://p.example:8443".to_string(),
            region: "us-east".to_string(),
            min_funding_wei: "1000000000000000000".to_string(),
            offer_nonce: 1,
            valid_until: 0,
            signature: String::new(),
        };
        o.sign(PROVIDER_KEY).unwrap();
        o
    }

    /// Get a *real* quote by driving the provider's own `propose` — the same
    /// object the consumer would receive over HTTP — so the test exercises the
    /// actual terms_hash / nonce the provider commits to.
    fn real_quote(offer: &ResourceOffer) -> Quote {
        let mut state = ProviderState::new(
            provider_address(),
            PROVIDER_KEY.to_string(),
            offer.clone(),
            NoChain,
        );
        let req = ProposeRequest {
            offer_ref: offer.offer_ref(),
            consumer_wallet: "0x00000000000000000000000000000000000000cc".to_string(),
            funding_amount_wei: "1000000000000000000".to_string(),
            params: jval!({}),
        };
        state.propose(&req, 1_700_000_000, [7u8; 16]).unwrap()
    }

    #[test]
    fn builds_valid_open_calldata_from_a_real_quote() {
        let offer = signed_offer();
        let quote = real_quote(&offer);
        let data = build_open_tx_data(&offer, &quote).expect("should build");
        // Round-trips through the provider's own parser as an Open commitment.
        match parse(&data).expect("parse") {
            ContractData::Open { offer_ref, terms_hash, contract_nonce } => {
                assert_eq!(format!("0x{}", hex::encode(offer_ref)), offer.offer_ref());
                assert_eq!(format!("0x{}", hex::encode(terms_hash)), quote.terms_hash);
                assert_eq!(format!("0x{}", hex::encode(contract_nonce)), quote.contract_nonce);
            }
            _ => panic!("expected an Open commitment"),
        }
    }

    #[test]
    fn rejects_quote_for_a_different_offer() {
        let offer = signed_offer();
        let mut quote = real_quote(&offer);
        // Provider tries to bind a different offer_ref than the one we discovered.
        quote.terms.offer_ref = "0x".to_string() + &"ab".repeat(32);
        assert!(build_open_tx_data(&offer, &quote).is_err());
    }

    #[test]
    fn rejects_quote_with_substituted_payee() {
        let offer = signed_offer();
        let mut quote = real_quote(&offer);
        quote.provider_wallet = "0x00000000000000000000000000000000000000ff".to_string();
        assert!(verify_quote(&offer, &quote).is_err());
    }
}
