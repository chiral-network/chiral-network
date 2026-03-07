use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub const BASE_ELO: f64 = 50.0;
pub const MIN_ELO: f64 = 0.0;
pub const MAX_ELO: f64 = 100.0;
pub const LOOKBACK_DAYS: u64 = 180;
pub const LOOKBACK_SECS: u64 = LOOKBACK_DAYS * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TransferOutcome {
    Completed,
    Failed,
}

/// Single transfer-backed reputation event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationEvent {
    pub id: String,
    pub transfer_id: String,
    pub seeder_wallet: String,
    pub downloader_wallet: String,
    pub file_hash: String,
    /// Amount paid to seeder in wei.
    pub amount_wei: String,
    pub outcome: TransferOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    /// Optional downloader rating 1-5 for this transfer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating_score: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating_comment: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RatingManifest {
    pub events: Vec<ReputationEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationSnapshot {
    pub elo: f64,
    pub base_elo: f64,
    pub completed_count: usize,
    pub failed_count: usize,
    pub transaction_count: usize,
    pub rating_count: usize,
    pub total_earned_wei: String,
}

#[derive(Clone)]
pub struct RatingState {
    pub manifest: Arc<RwLock<RatingManifest>>,
    data_dir: PathBuf,
}

impl RatingState {
    pub fn new(data_dir: PathBuf) -> Self {
        let manifest = load_manifest(&data_dir);
        Self {
            manifest: Arc::new(RwLock::new(manifest)),
            data_dir,
        }
    }

    pub async fn persist(&self) {
        let m = self.manifest.read().await;
        save_manifest(&self.data_dir, &m);
    }
}

fn ratings_dir(data_dir: &PathBuf) -> PathBuf {
    // v2 path intentionally ignores legacy star-rating history.
    data_dir.join("chiral-reputation-v2")
}

fn manifest_path(data_dir: &PathBuf) -> PathBuf {
    ratings_dir(data_dir).join("reputation.json")
}

fn load_manifest(data_dir: &PathBuf) -> RatingManifest {
    let path = manifest_path(data_dir);
    let Ok(data) = std::fs::read_to_string(&path) else {
        return RatingManifest::default();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

fn save_manifest(data_dir: &PathBuf, manifest: &RatingManifest) {
    let path = manifest_path(data_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(manifest) {
        let _ = std::fs::write(&path, json);
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn generate_event_id(
    transfer_id: &str,
    seeder_wallet: &str,
    downloader_wallet: &str,
    file_hash: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(transfer_id.as_bytes());
    hasher.update(b"|");
    hasher.update(seeder_wallet.to_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(downloader_wallet.to_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(file_hash.as_bytes());
    hex::encode(hasher.finalize())
}

fn wei_to_chi_f64(wei: &str) -> f64 {
    match wei.parse::<u128>() {
        Ok(v) => v as f64 / 1e18,
        Err(_) => 0.0,
    }
}

fn clamp_elo(v: f64) -> f64 {
    v.max(MIN_ELO).min(MAX_ELO)
}

/// Compute wallet Elo from transfer outcomes in the last 180 days.
/// Uses simple bounded Elo updates with time and amount weighting plus rating signal.
pub fn compute_reputation_for_wallet(
    events: &[ReputationEvent],
    seeder_wallet: &str,
    now: u64,
) -> ReputationSnapshot {
    let mut scoped: Vec<&ReputationEvent> = events
        .iter()
        .filter(|e| e.seeder_wallet.eq_ignore_ascii_case(seeder_wallet))
        .filter(|e| now.saturating_sub(e.created_at) <= LOOKBACK_SECS)
        .collect();
    scoped.sort_by_key(|e| e.created_at);

    let mut elo = BASE_ELO;
    let mut completed = 0usize;
    let mut failed = 0usize;
    let mut rating_count = 0usize;
    let mut total_earned_wei: u128 = 0;

    for event in &scoped {
        let age_days = now.saturating_sub(event.created_at) as f64 / 86_400.0;
        let w_time = (1.0 - age_days / LOOKBACK_DAYS as f64).max(0.0);

        let amount_chi = wei_to_chi_f64(&event.amount_wei);
        let w_amount = 1.0 + (amount_chi.ln_1p() / 51f64.ln()).clamp(0.0, 1.0);

        let outcome = match event.outcome {
            TransferOutcome::Completed => {
                completed += 1;
                if let Ok(v) = event.amount_wei.parse::<u128>() {
                    total_earned_wei = total_earned_wei.saturating_add(v);
                }
                1.0
            }
            TransferOutcome::Failed => {
                failed += 1;
                0.0
            }
        };

        let rating_signal = match event.rating_score {
            Some(score) => {
                rating_count += 1;
                ((score as f64) - 1.0) / 4.0
            }
            None => outcome,
        };

        let actual = 0.8 * outcome + 0.2 * rating_signal;
        let expected = 1.0 / (1.0 + 10f64.powf((BASE_ELO - elo) / 12.0));
        let k = 4.0 * w_time * w_amount;
        elo = clamp_elo(elo + k * (actual - expected));
    }

    ReputationSnapshot {
        elo: (elo * 10.0).round() / 10.0,
        base_elo: BASE_ELO,
        completed_count: completed,
        failed_count: failed,
        transaction_count: scoped.len(),
        rating_count,
        total_earned_wei: total_earned_wei.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_event(
        transfer_id: &str,
        seeder_wallet: &str,
        downloader_wallet: &str,
        outcome: TransferOutcome,
        amount_wei: &str,
        rating_score: Option<u8>,
        created_at: u64,
    ) -> ReputationEvent {
        ReputationEvent {
            id: generate_event_id(transfer_id, seeder_wallet, downloader_wallet, "hash"),
            transfer_id: transfer_id.to_string(),
            seeder_wallet: seeder_wallet.to_string(),
            downloader_wallet: downloader_wallet.to_string(),
            file_hash: "hash".to_string(),
            amount_wei: amount_wei.to_string(),
            outcome,
            tx_hash: None,
            rating_score,
            rating_comment: None,
            created_at,
            updated_at: created_at,
        }
    }

    #[test]
    fn test_event_id_is_deterministic() {
        let a = generate_event_id("t-1", "0xA", "0xB", "h");
        let b = generate_event_id("t-1", "0xA", "0xB", "h");
        assert_eq!(a, b);
    }

    #[test]
    fn test_base_score_with_no_events() {
        let snap = compute_reputation_for_wallet(&[], "0xA", 1_700_000_000);
        assert_eq!(snap.elo, 50.0);
        assert_eq!(snap.transaction_count, 0);
    }

    #[test]
    fn test_completed_transfer_with_positive_rating_increases_elo() {
        let now = 1_700_000_000;
        let events = vec![mk_event(
            "t-1",
            "0xA",
            "0xB",
            TransferOutcome::Completed,
            "1000000000000000000",
            Some(5),
            now - 86_400,
        )];
        let snap = compute_reputation_for_wallet(&events, "0xA", now);
        assert!(snap.elo > 50.0);
        assert_eq!(snap.completed_count, 1);
        assert_eq!(snap.rating_count, 1);
    }

    #[test]
    fn test_failed_transfer_decreases_elo() {
        let now = 1_700_000_000;
        let events = vec![mk_event(
            "t-1",
            "0xA",
            "0xB",
            TransferOutcome::Failed,
            "0",
            None,
            now - 86_400,
        )];
        let snap = compute_reputation_for_wallet(&events, "0xA", now);
        assert!(snap.elo < 50.0);
        assert_eq!(snap.failed_count, 1);
    }

    #[test]
    fn test_recent_events_have_more_weight_than_old_events() {
        let now = 1_700_000_000;
        let recent = mk_event(
            "recent",
            "0xA",
            "0xB",
            TransferOutcome::Completed,
            "1000000000000000000",
            Some(5),
            now - 2 * 86_400,
        );
        let old = mk_event(
            "old",
            "0xA",
            "0xC",
            TransferOutcome::Completed,
            "1000000000000000000",
            Some(5),
            now - 170 * 86_400,
        );
        let recent_only = compute_reputation_for_wallet(&[recent.clone()], "0xA", now);
        let old_only = compute_reputation_for_wallet(&[old], "0xA", now);
        assert!(recent_only.elo > old_only.elo);
    }

    #[test]
    fn test_events_older_than_lookback_are_ignored() {
        let now = 1_700_000_000;
        let events = vec![mk_event(
            "old",
            "0xA",
            "0xB",
            TransferOutcome::Completed,
            "1000000000000000000",
            Some(5),
            now - 200 * 86_400,
        )];
        let snap = compute_reputation_for_wallet(&events, "0xA", now);
        assert_eq!(snap.elo, 50.0);
        assert_eq!(snap.transaction_count, 0);
    }
}
