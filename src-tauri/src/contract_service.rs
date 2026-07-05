//! Contract service — the provider-side handshake that turns the settlement
//! engine into a running service. It ties `resource_offer` (the advertised
//! terms), `service_contract` (the on-chain commitment codec), `contract_ledger`
//! (balance accounting), `session_credential` (auth), and `usage_receipt`
//! (evidence) into the `propose → open → get → topup → receipt` flow described
//! in `docs/chiral-book.md` → "Service Contracts and the Handshake" and
//! "Provider Implementation".
//!
//! This is the HTTP-agnostic core (unit-tested against a mock chain verifier);
//! the Axum handlers that expose `/v1/contracts/*` are a thin wrapper over it,
//! and the real on-chain verification plugs in via the [`ChainVerifier`] trait
//! (backed by `wallet` + `rpc_client`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::contract_ledger::ContractLedger;
use crate::resource_offer::{ResourceClass, ResourceOffer};
use crate::service_contract::{self, ContractData, ContractTerms};
use crate::session_credential::{SessionCredential, SessionStore};
use crate::usage_receipt::UsageReceipt;

/// Result of verifying a funding / top-up transaction against the chain.
#[derive(Debug, Clone)]
pub struct VerifiedFunding {
    /// Sender (lowercased 0x address) — the consumer.
    pub from: String,
    /// `value` in wei.
    pub value_wei: u128,
    /// The tx `data` field (the contract commitment).
    pub data: Vec<u8>,
    /// `false` if seen but not yet mined/confirmed (retryable).
    pub confirmed: bool,
}

/// On-chain verification of a payment to the provider. The production impl
/// checks `to == provider`, `chainId`, mined status, and reads `value`/`data`
/// via `wallet` + `rpc_client`; tests use a mock.
pub trait ChainVerifier {
    fn verify_funding(&self, tx_hash: &str, expected_to: &str) -> Result<VerifiedFunding, String>;
}

/// A quote the provider is holding open, awaiting the consumer's on-chain commit.
#[derive(Debug, Clone)]
struct PendingQuote {
    terms: ContractTerms,
    expires_at: u64,
}

/// A propose request from a consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposeRequest {
    pub offer_ref: String,
    pub consumer_wallet: String,
    pub funding_amount_wei: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// The quote returned from `propose`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub provider_wallet: String,
    pub resource_class: ResourceClass,
    pub contract_nonce: String,
    pub terms_hash: String,
    pub terms: ContractTerms,
    pub quote_expires_at: u64,
}

/// The outcome of `open`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum OpenOutcome {
    Open {
        contract_id: String,
        funded_wei: String,
        fee_wei: String,
        balance_wei: String,
        credential: SessionCredential,
        expires_at: u64,
    },
    /// Tx seen but not yet confirmed — retry.
    Pending { contract_id: String, retry_after_s: u64 },
}

/// A read-only view of a contract's balance for `get`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractView {
    pub contract_id: String,
    pub status: String,
    pub funded_wei: String,
    pub spent_wei: String,
    pub balance_wei: String,
}

/// The provider's live state: what it offers, who it owes service, and the
/// pending quotes it is holding.
pub struct ProviderState<V: ChainVerifier> {
    pub provider_wallet: String,
    pub provider_private_key: String,
    pub offer: ResourceOffer,
    pub session_ttl_s: u64,
    pub quote_ttl_s: u64,
    pub ledger: ContractLedger,
    pub sessions: SessionStore,
    pub verifier: V,
    quotes: HashMap<String, PendingQuote>,
}

fn parse_wei(s: &str) -> Result<u128, String> {
    s.trim()
        .parse::<u128>()
        .map_err(|e| format!("invalid wei amount '{}': {}", s, e))
}

impl<V: ChainVerifier> ProviderState<V> {
    pub fn new(
        provider_wallet: String,
        provider_private_key: String,
        offer: ResourceOffer,
        verifier: V,
    ) -> Self {
        ProviderState {
            provider_wallet: provider_wallet.to_lowercase(),
            provider_private_key,
            offer,
            session_ttl_s: 86_400,
            quote_ttl_s: 300,
            ledger: ContractLedger::new(),
            sessions: SessionStore::new(),
            verifier,
            quotes: HashMap::new(),
        }
    }

    /// Quote terms for a proposed contract. `nonce_bytes` and `now_unix` are
    /// supplied by the caller (the HTTP layer draws real randomness / clock).
    pub fn propose(
        &mut self,
        req: &ProposeRequest,
        now_unix: u64,
        nonce_bytes: [u8; 16],
    ) -> Result<Quote, String> {
        // Consumer must have accepted *this* provider's current offer.
        if req.offer_ref.to_lowercase() != self.offer.offer_ref().to_lowercase() {
            return Err("offer_not_found".to_string());
        }
        let amount = parse_wei(&req.funding_amount_wei)?;
        let min = parse_wei(&self.offer.min_funding_wei)?;
        if amount < min {
            return Err("below_min_funding".to_string());
        }
        self.prune_quotes(now_unix);

        let contract_nonce = format!("0x{}", hex::encode(nonce_bytes));
        let terms = ContractTerms {
            offer_ref: self.offer.offer_ref(),
            provider_wallet: self.provider_wallet.clone(),
            consumer_wallet: req.consumer_wallet.to_lowercase(),
            resource_class: self.offer.resource_class,
            funding_amount_wei: amount.to_string(),
            rates: self.offer.price_schedule.clone(),
            params: req.params.clone(),
            contract_nonce: contract_nonce.clone(),
            expiry: now_unix + self.quote_ttl_s,
        };
        let terms_hash = terms.terms_hash_hex()?;
        self.quotes.insert(
            contract_nonce.clone(),
            PendingQuote {
                terms: terms.clone(),
                expires_at: now_unix + self.quote_ttl_s,
            },
        );
        Ok(Quote {
            provider_wallet: self.provider_wallet.clone(),
            resource_class: self.offer.resource_class,
            contract_nonce,
            terms_hash,
            terms,
            quote_expires_at: now_unix + self.quote_ttl_s,
        })
    }

    /// Verify the on-chain funding tx and open the contract, returning a
    /// contract-scoped session credential. `session_entropy` needs ≥ 64 bytes.
    pub fn open(
        &mut self,
        tx_hash: &str,
        now_unix: u64,
        session_entropy: &[u8],
    ) -> Result<OpenOutcome, String> {
        // Idempotent: an already-open contract returns its current view.
        if let Some(c) = self.ledger.get(tx_hash) {
            return Ok(OpenOutcome::Open {
                contract_id: c.contract_id.clone(),
                funded_wei: c.funded_wei.to_string(),
                fee_wei: (c.funded_wei - c.credited_wei).to_string(),
                balance_wei: c.balance_wei().to_string(),
                credential: self
                    .sessions
                    .credential_for_contract(&c.contract_id)
                    .cloned()
                    .ok_or("contract open but session missing")?,
                expires_at: now_unix + self.session_ttl_s,
            });
        }

        let vf = self.verifier.verify_funding(tx_hash, &self.provider_wallet)?;
        if !vf.confirmed {
            return Ok(OpenOutcome::Pending {
                contract_id: tx_hash.to_lowercase(),
                retry_after_s: 15,
            });
        }

        let (offer_ref, terms_hash, nonce) = match service_contract::parse(&vf.data)? {
            ContractData::Open {
                offer_ref,
                terms_hash,
                contract_nonce,
            } => (offer_ref, terms_hash, contract_nonce),
            ContractData::TopUp { .. } => return Err("expected open tx, got top-up".to_string()),
        };
        let nonce_hex = format!("0x{}", hex::encode(nonce));

        let quote = self
            .quotes
            .get(&nonce_hex)
            .cloned()
            .ok_or("quote_expired")?;
        if now_unix > quote.expires_at {
            self.quotes.remove(&nonce_hex);
            return Err("quote_expired".to_string());
        }
        // The committed offer_ref/terms_hash must match what we quoted.
        let want_offer_ref = crate::codec::hex_to_array::<32>(&quote.terms.offer_ref)?;
        let want_terms_hash = quote.terms.terms_hash()?;
        if offer_ref != want_offer_ref || terms_hash != want_terms_hash {
            return Err("payment_invalid: terms mismatch".to_string());
        }
        // The consumer who signed the funding tx must be the quoted consumer,
        // and the value must be the quoted amount.
        if vf.from.to_lowercase() != quote.terms.consumer_wallet.to_lowercase() {
            return Err("payment_invalid: wrong sender".to_string());
        }
        let amount = parse_wei(&quote.terms.funding_amount_wei)?;
        if vf.value_wei != amount {
            return Err("payment_invalid: wrong amount".to_string());
        }

        // Credit the balance (fee cut inside the ledger) and mint the session.
        self.ledger.open(
            tx_hash,
            &quote.terms.consumer_wallet,
            quote.terms.resource_class,
            amount,
        )?;
        self.quotes.remove(&nonce_hex);

        let expires_at = now_unix + self.session_ttl_s;
        let cred = SessionCredential::derive(tx_hash, expires_at, session_entropy)?;
        self.sessions.insert(cred.clone());

        let c = self.ledger.get(tx_hash).ok_or("ledger open failed")?;
        Ok(OpenOutcome::Open {
            contract_id: c.contract_id.clone(),
            funded_wei: c.funded_wei.to_string(),
            fee_wei: (c.funded_wei - c.credited_wei).to_string(),
            balance_wei: c.balance_wei().to_string(),
            credential: cred,
            expires_at,
        })
    }

    /// Verify a top-up tx (`CHR2` referencing this contract) and add to balance.
    pub fn topup(&mut self, tx_hash: &str) -> Result<ContractView, String> {
        let vf = self.verifier.verify_funding(tx_hash, &self.provider_wallet)?;
        if !vf.confirmed {
            return Err("tx_not_found: not yet confirmed".to_string());
        }
        let contract_id = match service_contract::parse(&vf.data)? {
            ContractData::TopUp { contract_id } => format!("0x{}", hex::encode(contract_id)),
            ContractData::Open { .. } => return Err("expected top-up tx, got open".to_string()),
        };
        self.ledger.topup(&contract_id, tx_hash, vf.value_wei)?;
        self.view(&contract_id).ok_or("unknown contract".to_string())
    }

    pub fn view(&self, contract_id: &str) -> Option<ContractView> {
        self.ledger.get(contract_id).map(|c| ContractView {
            contract_id: c.contract_id.clone(),
            status: format!("{:?}", c.status).to_lowercase(),
            funded_wei: c.funded_wei.to_string(),
            spent_wei: c.spent_wei.to_string(),
            balance_wei: c.balance_wei().to_string(),
        })
    }

    /// A provider-wallet-signed usage receipt for the contract.
    pub fn receipt(&self, contract_id: &str, now_unix: u64) -> Result<UsageReceipt, String> {
        let c = self.ledger.get(contract_id).ok_or("unknown contract")?;
        let mut r = UsageReceipt {
            contract_id: c.contract_id.clone(),
            provider_wallet: self.provider_wallet.clone(),
            funded_wei: c.funded_wei.to_string(),
            spent_wei: c.spent_wei.to_string(),
            balance_wei: c.balance_wei().to_string(),
            usage: serde_json::json!({ "spent_wei": c.spent_wei.to_string() }),
            as_of: now_unix,
            signature: String::new(),
        };
        r.sign(&self.provider_private_key)?;
        Ok(r)
    }

    /// Resolve a presented bearer to its contract id (auth for data-plane calls).
    pub fn resolve_bearer(&self, bearer: &str, now_unix: u64) -> Option<String> {
        self.sessions
            .resolve_bearer(bearer, now_unix)
            .map(|s| s.to_string())
    }

    /// Charge `cost_wei` of metered usage to a contract.
    pub fn draw_down(&mut self, contract_id: &str, cost_wei: u128) -> Result<u128, String> {
        self.ledger.draw_down(contract_id, cost_wei)
    }

    fn prune_quotes(&mut self, now_unix: u64) {
        self.quotes.retain(|_, q| now_unix <= q.expires_at);
    }
}

/// Production [`ChainVerifier`]: reads the funding transaction on-chain through
/// the shared RPC client (`eth_getTransactionByHash`), extracting the sender,
/// value, and `data` (the contract commitment), and checking the recipient is
/// the provider and the chain id is this network's. `confirmed` is `false` while
/// the tx is only in the mempool (no `blockNumber`), which the caller turns into
/// a retryable "pending".
///
/// The `ChainVerifier` trait is synchronous but the RPC is async; this bridges
/// with `block_in_place` + the current runtime handle, which is sound on the
/// daemon's multi-threaded Tokio runtime (a provider always runs one).
pub struct RpcChainVerifier;

impl ChainVerifier for RpcChainVerifier {
    fn verify_funding(&self, tx_hash: &str, expected_to: &str) -> Result<VerifiedFunding, String> {
        let tx_hash = tx_hash.to_string();
        let expected_to = expected_to.to_lowercase();
        tokio::task::block_in_place(move || {
            tokio::runtime::Handle::current().block_on(async move {
                let endpoints = crate::geth::wallet_rpc_endpoints();
                let tx = crate::rpc_client::call_with_fallbacks(
                    &endpoints,
                    "eth_getTransactionByHash",
                    serde_json::json!([tx_hash]),
                )
                .await?;
                if tx.is_null() {
                    return Err("tx_not_found".to_string());
                }
                let to = tx
                    .get("to")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                if to != expected_to {
                    return Err("payment_invalid: wrong recipient".to_string());
                }
                if let Some(cid) = tx.get("chainId").and_then(|v| v.as_str()) {
                    let observed = crate::rpc_client::hex_to_u128(cid).map_err(|e| format!("chainId: {e}"))?;
                    if observed != crate::geth::chain_id() as u128 {
                        return Err("payment_invalid: wrong chain".to_string());
                    }
                }
                let from = tx
                    .get("from")
                    .and_then(|f| f.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let value_hex = tx
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or("tx missing value")?;
                let value_wei =
                    crate::rpc_client::hex_to_u128(value_hex).map_err(|e| format!("value: {e}"))?;
                let input = tx.get("input").and_then(|v| v.as_str()).unwrap_or("0x");
                let data = hex::decode(input.trim_start_matches("0x"))
                    .map_err(|e| format!("bad input hex: {e}"))?;
                let confirmed = tx
                    .get("blockNumber")
                    .map(|b| !b.is_null())
                    .unwrap_or(false);
                Ok(VerifiedFunding {
                    from,
                    value_wei,
                    data,
                    confirmed,
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_contract::encode_open;
    use crate::wallet;

    const PROVIDER_KEY: &str = "0x3333333333333333333333333333333333333333333333333333333333333333";
    const CONSUMER: &str = "0xc0nsumer0000000000000000000000000000abcd";
    const ONE_CHI: u128 = 1_000_000_000_000_000_000;

    fn provider_address() -> String {
        let probe = b"chiral-test-address-probe";
        let sig = wallet::sign_message(PROVIDER_KEY, probe).unwrap();
        wallet::recover_signer(probe, &sig).unwrap()
    }

    fn sample_offer(provider: &str) -> ResourceOffer {
        ResourceOffer {
            provider_wallet: provider.to_string(),
            resource_class: ResourceClass::Storage,
            capacity: serde_json::json!({ "gb_available": 100 }),
            price_schedule: serde_json::json!({ "per_gb_month": "10000000000000000" }),
            endpoint: "https://p.example".to_string(),
            region: "us-east".to_string(),
            min_funding_wei: "100000000000000000".to_string(), // 0.1 CHI
            offer_nonce: 1,
            valid_until: 0,
            signature: String::new(),
        }
    }

    /// A mock chain that returns a preset funding for one tx.
    struct MockChain {
        tx: String,
        vf: VerifiedFunding,
    }
    impl ChainVerifier for MockChain {
        fn verify_funding(&self, tx_hash: &str, expected_to: &str) -> Result<VerifiedFunding, String> {
            assert_eq!(expected_to, provider_address_lower());
            if tx_hash.eq_ignore_ascii_case(&self.tx) {
                Ok(self.vf.clone())
            } else {
                Err("tx not found".to_string())
            }
        }
    }
    fn provider_address_lower() -> String {
        provider_address().to_lowercase()
    }

    fn state_with(vf_tx: &str, vf: VerifiedFunding) -> ProviderState<MockChain> {
        let provider = provider_address();
        let offer = sample_offer(&provider);
        ProviderState::new(
            provider,
            PROVIDER_KEY.to_string(),
            offer,
            MockChain {
                tx: vf_tx.to_string(),
                vf,
            },
        )
    }

    // Drive a full propose -> commit -> open, returning (state, tx_hash).
    fn opened() -> (ProviderState<MockChain>, String) {
        let tx = "0xfeed00000000000000000000000000000000000000000000000000000000beef";
        // Placeholder state to compute the quote (verifier is filled after).
        let provider = provider_address();
        let offer = sample_offer(&provider);
        let offer_ref = crate::codec::hex_to_array::<32>(&offer.offer_ref()).unwrap();

        let mut st = state_with(
            tx,
            VerifiedFunding {
                from: CONSUMER.to_string(),
                value_wei: ONE_CHI,
                data: vec![],
                confirmed: true,
            },
        );
        let req = ProposeRequest {
            offer_ref: st.offer.offer_ref(),
            consumer_wallet: CONSUMER.to_string(),
            funding_amount_wei: ONE_CHI.to_string(),
            params: serde_json::json!({ "bucket": "c-x" }),
        };
        let quote = st.propose(&req, 1000, [7u8; 16]).unwrap();
        let terms_hash = quote.terms.terms_hash().unwrap();
        let nonce = crate::codec::hex_to_array::<16>(&quote.contract_nonce).unwrap();
        let data = encode_open(&offer_ref, &terms_hash, &nonce);
        // Refresh the mock verifier's data now that we know the commitment.
        st.verifier = MockChain {
            tx: tx.to_string(),
            vf: VerifiedFunding {
                from: CONSUMER.to_string(),
                value_wei: ONE_CHI,
                data,
                confirmed: true,
            },
        };
        let out = st.open(tx, 1001, &[9u8; 64]).unwrap();
        assert!(matches!(out, OpenOutcome::Open { .. }));
        (st, tx.to_string())
    }

    #[test]
    fn propose_rejects_wrong_offer_and_low_funding() {
        let mut st = state_with(
            "0x00",
            VerifiedFunding { from: CONSUMER.into(), value_wei: 0, data: vec![], confirmed: true },
        );
        let mut req = ProposeRequest {
            offer_ref: "0xdeadbeef".into(),
            consumer_wallet: CONSUMER.into(),
            funding_amount_wei: ONE_CHI.to_string(),
            params: serde_json::Value::Null,
        };
        assert_eq!(st.propose(&req, 1, [0u8; 16]).unwrap_err(), "offer_not_found");
        req.offer_ref = st.offer.offer_ref();
        req.funding_amount_wei = "1".into(); // below 0.1 CHI min
        assert_eq!(st.propose(&req, 1, [0u8; 16]).unwrap_err(), "below_min_funding");
    }

    #[test]
    fn full_open_credits_and_mints_session() {
        let (st, tx) = opened();
        let c = st.ledger.get(&tx).unwrap();
        assert_eq!(c.funded_wei, ONE_CHI);
        assert_eq!(c.balance_wei(), 995_000_000_000_000_000); // net of 0.5% fee
        // Session resolves to the contract.
        let out = st.receipt(&tx, 2000).unwrap();
        assert!(out.verify().is_ok());
        assert_eq!(out.provider_wallet.to_lowercase(), st.provider_wallet);
    }

    #[test]
    fn open_is_idempotent() {
        let (mut st, tx) = opened();
        let again = st.open(&tx, 1002, &[9u8; 64]).unwrap();
        assert!(matches!(again, OpenOutcome::Open { .. }));
        // Still exactly one credit.
        assert_eq!(st.ledger.get(&tx).unwrap().funded_wei, ONE_CHI);
    }

    #[test]
    fn unconfirmed_tx_returns_pending() {
        let tx = "0xabc0000000000000000000000000000000000000000000000000000000000001";
        let mut st = state_with(
            tx,
            VerifiedFunding { from: CONSUMER.into(), value_wei: ONE_CHI, data: vec![], confirmed: false },
        );
        let req = ProposeRequest {
            offer_ref: st.offer.offer_ref(),
            consumer_wallet: CONSUMER.into(),
            funding_amount_wei: ONE_CHI.to_string(),
            params: serde_json::Value::Null,
        };
        st.propose(&req, 1, [1u8; 16]).unwrap();
        assert!(matches!(st.open(tx, 2, &[9u8; 64]).unwrap(), OpenOutcome::Pending { .. }));
    }

    #[test]
    fn tampered_amount_rejected() {
        let tx = "0xdd0000000000000000000000000000000000000000000000000000000000000a";
        let provider = provider_address();
        let offer = sample_offer(&provider);
        let offer_ref = crate::codec::hex_to_array::<32>(&offer.offer_ref()).unwrap();
        let mut st = state_with(tx, VerifiedFunding { from: CONSUMER.into(), value_wei: ONE_CHI, data: vec![], confirmed: true });
        let req = ProposeRequest {
            offer_ref: st.offer.offer_ref(),
            consumer_wallet: CONSUMER.into(),
            funding_amount_wei: ONE_CHI.to_string(),
            params: serde_json::Value::Null,
        };
        let quote = st.propose(&req, 1, [3u8; 16]).unwrap();
        let terms_hash = quote.terms.terms_hash().unwrap();
        let nonce = crate::codec::hex_to_array::<16>(&quote.contract_nonce).unwrap();
        let data = encode_open(&offer_ref, &terms_hash, &nonce);
        // Chain reports a DIFFERENT value than the quote.
        st.verifier = MockChain {
            tx: tx.to_string(),
            vf: VerifiedFunding { from: CONSUMER.into(), value_wei: ONE_CHI / 2, data, confirmed: true },
        };
        assert!(st.open(tx, 2, &[9u8; 64]).unwrap_err().contains("wrong amount"));
    }

    #[test]
    fn topup_and_drawdown() {
        let (mut st, tx) = opened();
        // Draw down half the balance.
        let bal = st.ledger.balance_wei(&tx).unwrap();
        st.draw_down(&tx, bal / 2).unwrap();
        assert!(st.view(&tx).unwrap().spent_wei != "0");

        // Top up with a CHR2 tx.
        let topup_tx = "0x1100000000000000000000000000000000000000000000000000000000000011";
        let cid = crate::codec::hex_to_array::<32>(&tx).unwrap();
        let data = crate::service_contract::encode_topup(&cid);
        st.verifier = MockChain {
            tx: topup_tx.to_string(),
            vf: VerifiedFunding { from: CONSUMER.into(), value_wei: ONE_CHI, data, confirmed: true },
        };
        let view = st.topup(topup_tx).unwrap();
        assert_eq!(view.funded_wei, (ONE_CHI * 2).to_string());
    }
}
