use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Payment checkpoint configuration
const INITIAL_CHECKPOINT_MB: u64 = 10; // First payment after 10 MB
const MIN_CHECKPOINT_MB: u64 = 1; // Minimum checkpoint size (exponential scaling)

/// Payment checkpoint states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CheckpointState {
    /// Download is active, no payment needed yet
    Active,
    /// Payment checkpoint reached, waiting for payment
    WaitingForPayment { checkpoint_mb: u64, amount_chiral: f64 },
    /// Payment received, download can resume
    PaymentReceived { transaction_hash: String },
    /// Payment failed or timed out
    PaymentFailed { reason: String },
    /// Download completed
    Completed,
}

/// Tracks payment checkpoint for a single download session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentCheckpoint {
    /// Unique session ID
    pub session_id: String,
    /// File hash being downloaded
    pub file_hash: String,
    /// Total file size in bytes
    pub file_size: u64,
    /// Total bytes transferred so far
    pub bytes_transferred: u64,
    /// Next checkpoint threshold in bytes
    pub next_checkpoint_bytes: u64,
    /// Current checkpoint state
    pub state: CheckpointState,
    /// Last checkpoint size (for exponential scaling)
    pub last_checkpoint_mb: u64,
    /// Total amount paid so far
    pub total_paid_chiral: f64,
    /// Seeder wallet address
    pub seeder_address: String,
    /// Seeder peer ID
    pub seeder_peer_id: String,
    /// Price per MB in Chiral
    pub price_per_mb: f64,
    /// Payment mode: "exponential" or "upfront"
    pub payment_mode: String,
    /// Checkpoint history (for tracking)
    pub checkpoint_history: Vec<CheckpointRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub checkpoint_mb: u64,
    pub bytes_at_checkpoint: u64,
    pub amount_paid: f64,
    pub transaction_hash: Option<String>,
    pub timestamp: u64,
}

/// Payment checkpoint service
pub struct PaymentCheckpointService {
    /// Active download sessions with payment checkpoints
    sessions: Arc<RwLock<HashMap<String, PaymentCheckpoint>>>,
}

impl PaymentCheckpointService {
    /// Create a new payment checkpoint service
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize a new payment checkpoint session
    pub async fn init_session(
        &self,
        session_id: String,
        file_hash: String,
        file_size: u64,
        seeder_address: String,
        seeder_peer_id: String,
        price_per_mb: f64,
        payment_mode: String, // "exponential" or "upfront"
    ) -> Result<(), String> {
        let initial_checkpoint = if payment_mode == "upfront" {
            file_size // Full file payment upfront
        } else {
            INITIAL_CHECKPOINT_MB * 1024 * 1024 // 10 MB
        };

        let checkpoint = PaymentCheckpoint {
            session_id: session_id.clone(),
            file_hash,
            file_size,
            bytes_transferred: 0,
            next_checkpoint_bytes: initial_checkpoint,
            state: CheckpointState::Active,
            last_checkpoint_mb: INITIAL_CHECKPOINT_MB,
            total_paid_chiral: 0.0,
            seeder_address,
            seeder_peer_id,
            price_per_mb,
            payment_mode: payment_mode.clone(),
            checkpoint_history: Vec::new(),
        };

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), checkpoint);

        info!(
            "Payment checkpoint session initialized: {} (mode: {}, first checkpoint: {} bytes)",
            session_id, payment_mode, initial_checkpoint
        );

        Ok(())
    }

    /// Update bytes transferred and check if payment checkpoint is reached
    pub async fn update_progress(
        &self,
        session_id: &str,
        bytes_transferred: u64,
    ) -> Result<CheckpointState, String> {
        let mut sessions = self.sessions.write().await;
        let checkpoint = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        checkpoint.bytes_transferred = bytes_transferred;

        // Check if we've reached the payment checkpoint
        if bytes_transferred >= checkpoint.next_checkpoint_bytes
            && checkpoint.state == CheckpointState::Active
        {
            // Calculate payment amount for this checkpoint
            let checkpoint_mb = checkpoint.last_checkpoint_mb;
            let amount_chiral = (checkpoint_mb as f64) * checkpoint.price_per_mb;

            // Update state to waiting for payment
            checkpoint.state = CheckpointState::WaitingForPayment {
                checkpoint_mb,
                amount_chiral,
            };

            info!(
                "Payment checkpoint reached for session {}: {} MB (amount: {} Chiral)",
                session_id, checkpoint_mb, amount_chiral
            );

            return Ok(checkpoint.state.clone());
        }

        Ok(checkpoint.state.clone())
    }

    /// Check if download should be paused (waiting for payment)
    pub async fn should_pause_serving(&self, session_id: &str) -> Result<bool, String> {
        let sessions = self.sessions.read().await;
        let checkpoint = sessions
            .get(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        match checkpoint.state {
            CheckpointState::WaitingForPayment { .. } => Ok(true),
            CheckpointState::PaymentFailed { .. } => Ok(true),
            _ => Ok(false),
        }
    }

    /// Record payment and resume download
    pub async fn record_payment(
        &self,
        session_id: &str,
        transaction_hash: String,
        amount_paid: f64,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;
        let checkpoint = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        // Verify we're in the right state
        let checkpoint_mb = match &checkpoint.state {
            CheckpointState::WaitingForPayment { checkpoint_mb, .. } => *checkpoint_mb,
            _ => {
                return Err(format!(
                    "Invalid state for payment: {:?}",
                    checkpoint.state
                ))
            }
        };

        // Record the checkpoint
        checkpoint.checkpoint_history.push(CheckpointRecord {
            checkpoint_mb,
            bytes_at_checkpoint: checkpoint.bytes_transferred,
            amount_paid,
            transaction_hash: Some(transaction_hash.clone()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        });

        checkpoint.total_paid_chiral += amount_paid;

        // Calculate next checkpoint using exponential scaling
        let next_checkpoint_mb = if checkpoint.payment_mode == "upfront" {
            // For upfront mode, no more checkpoints
            u64::MAX
        } else {
            // Exponential scaling: 1 → 2 → 4 → 8 → 16 MB
            let next_mb = (checkpoint_mb * 2).max(MIN_CHECKPOINT_MB);
            checkpoint.last_checkpoint_mb = next_mb;
            next_mb
        };

        checkpoint.next_checkpoint_bytes =
            checkpoint.bytes_transferred + (next_checkpoint_mb * 1024 * 1024);

        // Update state to allow download to continue
        checkpoint.state = CheckpointState::PaymentReceived { transaction_hash: transaction_hash.clone() };

        // Immediately reset to Active state so download can continue
        checkpoint.state = CheckpointState::Active;

        info!(
            "Payment recorded for session {}: {} Chiral (tx: {}). Next checkpoint: {} MB",
            session_id, amount_paid, transaction_hash, next_checkpoint_mb
        );

        Ok(())
    }

    /// Mark payment as failed
    pub async fn mark_payment_failed(&self, session_id: &str, reason: String) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;
        let checkpoint = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        checkpoint.state = CheckpointState::PaymentFailed { reason: reason.clone() };

        warn!("Payment failed for session {}: {}", session_id, reason);

        Ok(())
    }

    /// Mark session as completed
    pub async fn mark_completed(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;
        let checkpoint = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        checkpoint.state = CheckpointState::Completed;

        info!("Download session completed: {} (total paid: {} Chiral)", session_id, checkpoint.total_paid_chiral);

        Ok(())
    }

    /// Get checkpoint info for a session
    pub async fn get_checkpoint_info(&self, session_id: &str) -> Result<PaymentCheckpoint, String> {
        let sessions = self.sessions.read().await;
        let checkpoint = sessions
            .get(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        Ok(checkpoint.clone())
    }

    /// Remove a session (cleanup)
    pub async fn remove_session(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);

        debug!("Removed payment checkpoint session: {}", session_id);

        Ok(())
    }

    /// Get all active sessions
    pub async fn get_all_sessions(&self) -> HashMap<String, PaymentCheckpoint> {
        let sessions = self.sessions.read().await;
        sessions.clone()
    }

    /// Calculate total amount required for remaining download
    pub async fn calculate_remaining_payment(
        &self,
        session_id: &str,
    ) -> Result<f64, String> {
        let sessions = self.sessions.read().await;
        let checkpoint = sessions
            .get(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        let remaining_bytes = checkpoint.file_size.saturating_sub(checkpoint.bytes_transferred);
        let remaining_mb = remaining_bytes as f64 / (1024.0 * 1024.0);
        let remaining_cost = remaining_mb * checkpoint.price_per_mb;

        Ok(remaining_cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_checkpoint_initialization() {
        let service = PaymentCheckpointService::new();

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024, // 100 MB file
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        let checkpoint = service.get_checkpoint_info("test-session").await.unwrap();
        assert_eq!(checkpoint.state, CheckpointState::Active);
        assert_eq!(checkpoint.next_checkpoint_bytes, 10 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_checkpoint_reached() {
        let service = PaymentCheckpointService::new();

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        // Update progress to 10 MB (checkpoint)
        let state = service
            .update_progress("test-session", 10 * 1024 * 1024)
            .await
            .unwrap();

        match state {
            CheckpointState::WaitingForPayment { checkpoint_mb, amount_chiral } => {
                assert_eq!(checkpoint_mb, 10);
                assert_eq!(amount_chiral, 0.01); // 10 MB * 0.001 Chiral/MB
            }
            _ => panic!("Expected WaitingForPayment state"),
        }
    }

    #[tokio::test]
    async fn test_exponential_scaling() {
        let service = PaymentCheckpointService::new();

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        // Reach first checkpoint (10 MB)
        service.update_progress("test-session", 10 * 1024 * 1024).await.unwrap();

        // Record payment
        service
            .record_payment("test-session", "tx1".to_string(), 0.01)
            .await
            .unwrap();

        // Check next checkpoint is at 10 MB + 20 MB = 30 MB (exponential: 10 → 20)
        let checkpoint = service.get_checkpoint_info("test-session").await.unwrap();
        assert_eq!(checkpoint.next_checkpoint_bytes, 30 * 1024 * 1024);
        assert_eq!(checkpoint.last_checkpoint_mb, 20);
    }

    #[tokio::test]
    async fn test_upfront_payment_mode() {
        let service = PaymentCheckpointService::new();
        let file_size = 100 * 1024 * 1024; // 100 MB

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                file_size,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "upfront".to_string(),
            )
            .await
            .unwrap();

        let checkpoint = service.get_checkpoint_info("test-session").await.unwrap();
        // In upfront mode, first checkpoint should be the full file size
        assert_eq!(checkpoint.next_checkpoint_bytes, file_size);
    }

    #[tokio::test]
    async fn test_should_pause_serving() {
        let service = PaymentCheckpointService::new();

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        // Should not pause before checkpoint
        let should_pause = service.should_pause_serving("test-session").await.unwrap();
        assert_eq!(should_pause, false);

        // Trigger checkpoint
        service.update_progress("test-session", 10 * 1024 * 1024).await.unwrap();

        // Should pause at checkpoint
        let should_pause = service.should_pause_serving("test-session").await.unwrap();
        assert_eq!(should_pause, true);

        // Record payment
        service
            .record_payment("test-session", "tx1".to_string(), 0.01)
            .await
            .unwrap();

        // Should not pause after payment
        let should_pause = service.should_pause_serving("test-session").await.unwrap();
        assert_eq!(should_pause, false);
    }

    #[tokio::test]
    async fn test_payment_history_tracking() {
        let service = PaymentCheckpointService::new();

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        // First checkpoint
        service.update_progress("test-session", 10 * 1024 * 1024).await.unwrap();
        service
            .record_payment("test-session", "tx1".to_string(), 0.01)
            .await
            .unwrap();

        // Second checkpoint
        service.update_progress("test-session", 30 * 1024 * 1024).await.unwrap();
        service
            .record_payment("test-session", "tx2".to_string(), 0.02)
            .await
            .unwrap();

        let checkpoint = service.get_checkpoint_info("test-session").await.unwrap();
        assert_eq!(checkpoint.checkpoint_history.len(), 2);
        assert_eq!(checkpoint.total_paid_chiral, 0.03);
        assert_eq!(checkpoint.checkpoint_history[0].transaction_hash, Some("tx1".to_string()));
        assert_eq!(checkpoint.checkpoint_history[1].transaction_hash, Some("tx2".to_string()));
    }

    #[tokio::test]
    async fn test_calculate_remaining_payment() {
        let service = PaymentCheckpointService::new();

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        // Download 30 MB, so 70 MB remaining
        service.update_progress("test-session", 30 * 1024 * 1024).await.unwrap();

        let remaining_cost = service.calculate_remaining_payment("test-session").await.unwrap();
        // 70 MB * 0.001 Chiral/MB = 0.07 Chiral
        assert!((remaining_cost - 0.07).abs() < 0.0001);
    }

    #[tokio::test]
    async fn test_mark_payment_failed() {
        let service = PaymentCheckpointService::new();

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        service.update_progress("test-session", 10 * 1024 * 1024).await.unwrap();
        service
            .mark_payment_failed("test-session", "Insufficient balance".to_string())
            .await
            .unwrap();

        let checkpoint = service.get_checkpoint_info("test-session").await.unwrap();
        match checkpoint.state {
            CheckpointState::PaymentFailed { reason } => {
                assert_eq!(reason, "Insufficient balance");
            }
            _ => panic!("Expected PaymentFailed state"),
        }

        // Should pause when payment failed
        let should_pause = service.should_pause_serving("test-session").await.unwrap();
        assert_eq!(should_pause, true);
    }

    #[tokio::test]
    async fn test_mark_completed() {
        let service = PaymentCheckpointService::new();

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        service.mark_completed("test-session").await.unwrap();

        let checkpoint = service.get_checkpoint_info("test-session").await.unwrap();
        assert_eq!(checkpoint.state, CheckpointState::Completed);
    }

    #[tokio::test]
    async fn test_remove_session() {
        let service = PaymentCheckpointService::new();

        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        service.remove_session("test-session").await.unwrap();

        let result = service.get_checkpoint_info("test-session").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_full_download_cycle() {
        let service = PaymentCheckpointService::new();

        // Initialize 100 MB download
        service
            .init_session(
                "test-session".to_string(),
                "test-hash".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-123".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        // First checkpoint at 10 MB
        service.update_progress("test-session", 10 * 1024 * 1024).await.unwrap();
        service.record_payment("test-session", "tx1".to_string(), 0.01).await.unwrap();

        // Second checkpoint at 30 MB (10 + 20)
        service.update_progress("test-session", 30 * 1024 * 1024).await.unwrap();
        service.record_payment("test-session", "tx2".to_string(), 0.02).await.unwrap();

        // Third checkpoint at 70 MB (30 + 40)
        service.update_progress("test-session", 70 * 1024 * 1024).await.unwrap();
        service.record_payment("test-session", "tx3".to_string(), 0.04).await.unwrap();

        // Complete download at 100 MB
        service.update_progress("test-session", 100 * 1024 * 1024).await.unwrap();
        service.mark_completed("test-session").await.unwrap();

        let checkpoint = service.get_checkpoint_info("test-session").await.unwrap();
        assert_eq!(checkpoint.state, CheckpointState::Completed);
        assert_eq!(checkpoint.checkpoint_history.len(), 3);
        assert_eq!(checkpoint.total_paid_chiral, 0.07); // 0.01 + 0.02 + 0.04
    }

    #[tokio::test]
    async fn test_concurrent_sessions() {
        let service = PaymentCheckpointService::new();

        // Initialize two sessions
        service
            .init_session(
                "session-1".to_string(),
                "hash-1".to_string(),
                100 * 1024 * 1024,
                "0x123".to_string(),
                "peer-1".to_string(),
                0.001,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        service
            .init_session(
                "session-2".to_string(),
                "hash-2".to_string(),
                50 * 1024 * 1024,
                "0x456".to_string(),
                "peer-2".to_string(),
                0.002,
                "exponential".to_string(),
            )
            .await
            .unwrap();

        // Progress both independently
        service.update_progress("session-1", 10 * 1024 * 1024).await.unwrap();
        service.update_progress("session-2", 10 * 1024 * 1024).await.unwrap();

        let checkpoint1 = service.get_checkpoint_info("session-1").await.unwrap();
        let checkpoint2 = service.get_checkpoint_info("session-2").await.unwrap();

        // Different prices should yield different amounts
        match checkpoint1.state {
            CheckpointState::WaitingForPayment { amount_chiral, .. } => {
                assert_eq!(amount_chiral, 0.01); // 10 MB * 0.001
            }
            _ => panic!("Expected WaitingForPayment for session-1"),
        }

        match checkpoint2.state {
            CheckpointState::WaitingForPayment { amount_chiral, .. } => {
                assert_eq!(amount_chiral, 0.02); // 10 MB * 0.002
            }
            _ => panic!("Expected WaitingForPayment for session-2"),
        }
    }
}
