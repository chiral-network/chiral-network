use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Payment checkpoint configuration
const INITIAL_CHECKPOINT_MB: u64 = 10; // First payment after 10 MB
const MIN_CHECKPOINT_MB: u64 = 1; // Minimum checkpoint size (exponential scaling)
const MB_BYTES: u64 = 1024 * 1024;
const MAX_HISTORY_LEN: usize = 100; // cap history to avoid unbounded memory growth
const RATE_LIMIT_WINDOW_SECS: u64 = 60; // window for rate limiting payment attempts
const MAX_PAYMENT_ATTEMPTS: usize = 5; // max attempts per window

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
    /// Seen transaction hashes to provide idempotency/replay protection
    #[serde(default)]
    pub seen_transaction_hashes: HashSet<String>,
    /// Recent payment attempt timestamps (unix seconds) for simple rate limiting
    #[serde(default)]
    pub recent_payment_attempts: Vec<u64>,
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
    /// Per-session async mutexes to avoid concurrent mutations on same session
    session_locks: Arc<RwLock<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl PaymentCheckpointService {
    /// Create a new payment checkpoint service
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_locks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or create per-session mutex
    async fn get_or_create_session_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.session_locks.write().await;
        locks
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
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
        // Basic input validation
        if session_id.is_empty() {
            return Err("session_id must not be empty".to_string());
        }
        if file_hash.is_empty() {
            return Err("file_hash must not be empty".to_string());
        }
        if file_size == 0 {
            return Err("file_size must be > 0".to_string());
        }
        if !price_per_mb.is_finite() || price_per_mb < 0.0 {
            return Err("price_per_mb must be a finite non-negative number".to_string());
        }

        let initial_checkpoint = if payment_mode == "upfront" {
            file_size // Full file payment upfront
        } else {
            INITIAL_CHECKPOINT_MB.saturating_mul(MB_BYTES)
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
            seen_transaction_hashes: HashSet::new(),
            recent_payment_attempts: Vec::new(),
        };

        let mut sessions = self.sessions.write().await;
        // Prevent accidental overwrite of existing session
        if sessions.contains_key(&session_id) {
            return Err(format!("session already exists: {}", session_id));
        }
        sessions.insert(session_id.clone(), checkpoint);

        // Ensure a per-session lock exists for this session
        let mut locks = self.session_locks.write().await;
        locks.entry(session_id.clone()).or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())));

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
        // Acquire per-session lock to avoid races with other callers
        let lock = self.get_or_create_session_lock(session_id).await;
        let _guard = lock.lock().await;

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
        // Basic input validation
        if transaction_hash.is_empty() {
            return Err("transaction_hash must not be empty".to_string());
        }
        if !amount_paid.is_finite() || amount_paid < 0.0 {
            return Err("amount_paid must be finite and non-negative".to_string());
        }

        // Acquire per-session lock to avoid races
        let lock = self.get_or_create_session_lock(session_id).await;
        let _guard = lock.lock().await;

        let mut sessions = self.sessions.write().await;
        let checkpoint = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        // Rate limiting: prune old attempts and check count
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        checkpoint.recent_payment_attempts.retain(|ts| now_secs.saturating_sub(*ts) <= RATE_LIMIT_WINDOW_SECS);
        if checkpoint.recent_payment_attempts.len() >= MAX_PAYMENT_ATTEMPTS {
            warn!("Rate limit exceeded for session {}", session_id);
            return Err("rate limit exceeded for payment attempts".to_string());
        }
        checkpoint.recent_payment_attempts.push(now_secs);

        // Idempotency / replay protection
        if checkpoint.seen_transaction_hashes.contains(&transaction_hash) {
            warn!("Duplicate transaction hash for session {}: {}", session_id, transaction_hash);
            return Err("duplicate transaction hash".to_string());
        }

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

        // Cap history
        if checkpoint.checkpoint_history.len() > MAX_HISTORY_LEN {
            // keep the newest entries
            let excess = checkpoint.checkpoint_history.len() - MAX_HISTORY_LEN;
            checkpoint.checkpoint_history.drain(0..excess);
        }

        checkpoint.total_paid_chiral += amount_paid;

        // Record tx hash for idempotency
        checkpoint.seen_transaction_hashes.insert(transaction_hash.clone());

        // Calculate next checkpoint using exponential scaling
        let next_checkpoint_mb = if checkpoint.payment_mode == "upfront" {
            // For upfront mode, no more checkpoints
            u64::MAX
        } else {
            // Exponential scaling: 1 -> 2 -> 4 -> 8 -> 16 MB
            let next_mb = (checkpoint_mb * 2).max(MIN_CHECKPOINT_MB);
            checkpoint.last_checkpoint_mb = next_mb;
            next_mb
        };

        // Use saturating arithmetic to avoid overflow
        let added = next_checkpoint_mb.saturating_mul(MB_BYTES);
        checkpoint.next_checkpoint_bytes = checkpoint.bytes_transferred.saturating_add(added);

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
        // Acquire per-session lock
        let lock = self.get_or_create_session_lock(session_id).await;
        let _guard = lock.lock().await;

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
        // Acquire per-session lock
        let lock = self.get_or_create_session_lock(session_id).await;
        let _guard = lock.lock().await;

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
        // Acquire per-session lock to avoid races with in-flight operations
        let lock = self.get_or_create_session_lock(session_id).await;
        let _guard = lock.lock().await;

        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);

        // Remove the associated lock entry
        let mut locks = self.session_locks.write().await;
        locks.remove(session_id);

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
        assert_eq!(checkpoint.next_checkpoint_bytes, INITIAL_CHECKPOINT_MB * MB_BYTES);
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

        // Check next checkpoint is at 10 MB + 20 MB = 30 MB (exponential: 10 -> 20)
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
