//! Public feedback admission and canonicality revalidation. No relay signing wallet.
use crate::{
    chain_client::{ChainClient, ChainError},
    error::ApiError,
    offer_registry::validate_wallet,
    protocol,
    rating_storage::RatingStore,
    reputation::{self, FeedbackSubmission, ReputationEvent},
};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct RatingState {
    pub chain: Arc<ChainClient>,
    pub store: Arc<RatingStore>,
    gate: Semaphore,
}
impl RatingState {
    pub fn new(chain: Arc<ChainClient>, store: Arc<RatingStore>) -> Self {
        Self {
            chain,
            store,
            gate: Semaphore::new(4),
        }
    }
    async fn verify(&self, submission: &FeedbackSubmission) -> Result<String, ChainError> {
        let f = &submission.feedback.payload;
        let q = &submission.quote.payload;
        let data = protocol::opening_data(q).map_err(ChainError::Rejected)?;
        let payment = self
            .chain
            .verify_payment(
                &submission.raw_transaction,
                &f.consumer_wallet,
                &f.provider_wallet,
                Some(&q.funding_wei),
                &data,
            )
            .await?;
        if payment.tx_hash != f.tx_hash || payment.network != f.network {
            return Err(ChainError::Rejected(
                "funding hash or network does not match feedback".into(),
            ));
        }
        Ok(payment.value_wei)
    }
    async fn refresh(&self, wallet: &str) -> Result<Vec<ReputationEvent>, ApiError> {
        let mut events = self.store.events(wallet).map_err(ApiError::internal)?;
        for event in &mut events {
            // Stored evidence is reverified, including its provider and payer signatures.
            if reputation::validate_submission(
                &event.submission,
                self.chain.identity(),
                protocol::now().map_err(ApiError::internal)?,
            )
            .is_err()
            {
                event.eligible = false;
            } else {
                match self.verify(&event.submission).await {
                    Ok(amount) => event.eligible = amount == event.amount_wei,
                    Err(ChainError::Rejected(_) | ChainError::Pending(_)) => event.eligible = false,
                    Err(ChainError::Unavailable(_)) => {
                        return Err(ApiError::new(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "chain_unavailable",
                            "reputation canonicality cannot currently be verified",
                        ))
                    }
                }
            }
            self.store
                .set_eligible(&event.submission.feedback.payload.quote_id, event.eligible)
                .map_err(ApiError::internal)?;
        }
        Ok(events)
    }
}
pub fn router(state: Arc<RatingState>) -> Router {
    Router::new()
        .route("/api/ratings/feedback", post(submit))
        .route("/api/ratings/:wallet", get(snapshot))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .with_state(state)
}
async fn submit(
    State(state): State<Arc<RatingState>>,
    Json(submission): Json<FeedbackSubmission>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _permit = state.gate.try_acquire().map_err(|_| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "feedback_busy",
            "feedback verifier is busy",
        )
    })?;
    let now = protocol::now().map_err(ApiError::internal)?;
    reputation::validate_submission(&submission, state.chain.identity(), now)
        .map_err(|e| ApiError::bad_request("invalid_feedback", e))?;
    if now.saturating_sub(submission.feedback.payload.issued_at) > 300 {
        return Err(ApiError::bad_request(
            "expired_feedback",
            "sign feedback with a current timestamp and increasing revision",
        ));
    }
    let amount = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        state.verify(&submission),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "chain_timeout",
            "funding verification timed out",
        )
    })?
    .map_err(|e| match e {
        ChainError::Pending(_) => ApiError::new(
            StatusCode::CONFLICT,
            "payment_pending",
            "funding has not reached the settlement frontier",
        ),
        ChainError::Rejected(_) => ApiError::bad_request(
            "payment_rejected",
            "funding evidence is not an eligible canonical payment",
        ),
        ChainError::Unavailable(_) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "chain_unavailable",
            "funding cannot currently be verified",
        ),
    })?;
    state
        .store
        .upsert(&submission, &amount, now)
        .map_err(|e| ApiError::bad_request("feedback_conflict", e))?;
    Ok(Json(
        serde_json::json!({"accepted":true,"quote_id":submission.feedback.payload.quote_id,"revision":submission.feedback.payload.revision,"experimental_testnet":true}),
    ))
}
async fn snapshot(
    State(state): State<Arc<RatingState>>,
    Path(wallet): Path<String>,
) -> Result<Json<reputation::ReputationSnapshot>, ApiError> {
    validate_wallet(&wallet).map_err(|e| ApiError::bad_request("invalid_wallet", e))?;
    let _permit = state.gate.try_acquire().map_err(|_| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "feedback_busy",
            "feedback verifier is busy",
        )
    })?;
    let events = tokio::time::timeout(std::time::Duration::from_secs(30), state.refresh(&wallet))
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "chain_timeout",
                "reputation revalidation timed out",
            )
        })??;
    Ok(Json(reputation::compute_reputation(
        &events,
        state.chain.identity(),
        &wallet,
        protocol::now().map_err(ApiError::internal)?,
    )))
}
/// Revalidate incrementally even without HTTP readers; no stale vote is served on
/// RPC failure, and orphaned evidence remains available for future canonical recovery.
pub async fn reconcile(state: Arc<RatingState>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    let mut cursor = String::new();
    loop {
        tokio::select! { _=shutdown.changed()=>{if *shutdown.borrow(){break;}}, _=tokio::time::sleep(std::time::Duration::from_secs(30))=>{} }
        if *shutdown.borrow() {
            break;
        }
        let Ok(providers) = state.store.providers(&cursor) else {
            continue;
        };
        if providers.is_empty() {
            cursor.clear();
            continue;
        }
        for wallet in providers {
            if *shutdown.borrow() {
                return;
            }
            if let Ok(_permit) = state.gate.try_acquire() {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    state.refresh(&wallet),
                )
                .await;
            }
            cursor = wallet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(
        directory: &std::path::Path,
        network: crate::protocol::NetworkIdentity,
    ) -> Arc<RatingState> {
        let config = crate::network::NetworkConfig {
            identity: network.clone(),
            rpc_url: "http://127.0.0.1:1".into(),
            rpc_bearer_file: None,
            faucet_url: None,
            explorer_url: None,
            policy_url: None,
        };
        Arc::new(RatingState::new(
            Arc::new(ChainClient::new(config, directory.join("chain")).unwrap()),
            Arc::new(RatingStore::open(&directory.join("ratings.sqlite"), &network).unwrap()),
        ))
    }
    #[tokio::test]
    async fn handler_rejects_forgery_without_creating_vote() {
        let directory = tempfile::tempdir().unwrap();
        let mut submission = crate::rating_storage::tests::submission(protocol::now().unwrap());
        let state = state(
            directory.path(),
            submission.feedback.payload.network.clone(),
        );
        let wallet = submission.feedback.payload.provider_wallet.clone();
        submission.feedback.payload.score = 1;
        let error = submit(State(state.clone()), Json(submission))
            .await
            .unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(state.store.events(&wallet).unwrap().is_empty());
    }
    #[tokio::test]
    async fn handler_rejects_invalid_raw_payment_without_fake_eligibility() {
        let directory = tempfile::tempdir().unwrap();
        let submission = crate::rating_storage::tests::submission(protocol::now().unwrap());
        let state = state(
            directory.path(),
            submission.feedback.payload.network.clone(),
        );
        let wallet = submission.feedback.payload.provider_wallet.clone();
        let error = submit(State(state.clone()), Json(submission))
            .await
            .unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(state.store.events(&wallet).unwrap().is_empty());
    }
}
