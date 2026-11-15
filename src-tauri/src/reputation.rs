//! Payer-signed subjective feedback backed by a committed opening payment.
//! This is experimental testnet reputation, not proof of model correctness.
use crate::{
    offer_registry::{validate_model, validate_money, validate_wallet},
    protocol::{self, NetworkIdentity, Quote, Signed},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feedback {
    pub network: NetworkIdentity,
    pub provider_wallet: String,
    pub consumer_wallet: String,
    pub quote_id: String,
    pub tx_hash: String,
    pub score: u8,
    pub revision: u64,
    pub issued_at: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackSubmission {
    pub feedback: Signed<Feedback>,
    pub quote: Signed<Quote>,
    pub raw_transaction: String,
}
pub fn validate_submission(
    submission: &FeedbackSubmission,
    network: &NetworkIdentity,
    now: u64,
) -> Result<(), String> {
    let f = &submission.feedback.payload;
    let q = &submission.quote.payload;
    if &f.network != network || &q.network != network {
        return Err("feedback belongs to a different network".into());
    }
    validate_wallet(&f.consumer_wallet)?;
    validate_wallet(&f.provider_wallet)?;
    validate_wallet(&q.platform_wallet)?;
    if f.provider_wallet == f.consumer_wallet {
        return Err("self feedback is forbidden".into());
    }
    if f.provider_wallet != q.provider_wallet
        || f.consumer_wallet != q.consumer_wallet
        || f.quote_id != q.id
    {
        return Err("feedback does not match quote parties and identity".into());
    }
    if f.score > 100
        || f.revision == 0
        || f.issued_at > now.saturating_add(30)
        || f.issued_at < q.issued_at
    {
        return Err("invalid feedback score, revision or timestamp".into());
    }
    if q.expires_at <= q.issued_at || q.max_concurrency == 0 || q.fee_bps != 50 {
        return Err("invalid signed quote terms".into());
    }
    if validate_money(&q.funding_wei)? == 0
        || (validate_money(&q.price.input_wei_per_1k)? == 0
            && validate_money(&q.price.output_wei_per_1k)? == 0)
    {
        return Err("free sessions cannot vote".into());
    }
    validate_model(&q.model)?;
    for hash in [&f.tx_hash, &f.quote_id, &q.offer_id] {
        if hash.len() != 66
            || !hash.starts_with("0x")
            || !hash[2..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid canonical feedback identity".into());
        }
    }
    if submission.raw_transaction.len() > 32768 {
        return Err("funding evidence is too large".into());
    }
    protocol::verify(
        protocol::QUOTE_DOMAIN,
        &submission.quote,
        &q.provider_wallet,
    )?;
    protocol::verify(
        protocol::FEEDBACK_DOMAIN,
        &submission.feedback,
        &f.consumer_wallet,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationEvent {
    pub submission: FeedbackSubmission,
    pub amount_wei: String,
    pub created_at: u64,
    pub eligible: bool,
}
#[derive(Debug, Serialize)]
pub struct ReputationSnapshot {
    pub network: NetworkIdentity,
    pub provider_wallet: String,
    pub experimental_testnet: bool,
    pub elo: f64,
    pub base_elo: f64,
    pub vote_count: usize,
}
/// Existing bounded Elo engine: linear 180-day decay, logarithmic amount weight,
/// K=4 and a 50-point baseline. Only admission and subjective signal have changed.
pub fn compute_reputation(
    events: &[ReputationEvent],
    network: &NetworkIdentity,
    wallet: &str,
    now: u64,
) -> ReputationSnapshot {
    let mut scoped: Vec<_> = events
        .iter()
        .filter(|e| {
            e.eligible
                && &e.submission.feedback.payload.network == network
                && e.submission.feedback.payload.provider_wallet == wallet
                && now.saturating_sub(e.created_at) <= 180 * 86400
        })
        .collect();
    // Stable tie-break is critical: arrival order must not change the Elo result.
    scoped.sort_by(|a, b| {
        (a.created_at, &a.submission.feedback.payload.quote_id)
            .cmp(&(b.created_at, &b.submission.feedback.payload.quote_id))
    });
    let mut elo = 50.0;
    for event in &scoped {
        let age_days = now.saturating_sub(event.created_at) as f64 / 86400.0;
        let w_time = (1.0 - age_days / 180.0).max(0.0);
        let amount = event.amount_wei.parse::<u128>().unwrap_or(0) as f64 / 1e18;
        let w_amount = 1.0 + (amount.ln_1p() / 51f64.ln()).clamp(0.0, 1.0);
        let expected = 1.0 / (1.0 + 10f64.powf((50.0 - elo) / 12.0));
        elo = (elo
            + 4.0
                * w_time
                * w_amount
                * (event.submission.feedback.payload.score as f64 / 100.0 - expected))
            .clamp(0.0, 100.0);
    }
    ReputationSnapshot {
        network: network.clone(),
        provider_wallet: wallet.into(),
        experimental_testnet: true,
        elo: (elo * 10.0).round() / 10.0,
        base_elo: 50.0,
        vote_count: scoped.len(),
    }
}
