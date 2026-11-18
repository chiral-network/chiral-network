//! Durable one-vote-per-agreement storage, scoped to the complete network identity.
use crate::{
    protocol::NetworkIdentity,
    reputation::{FeedbackSubmission, ReputationEvent},
};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;

/// Why the store refused a submission. Each message is fixed text that is safe to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The evidence fails admission validation, or the verified amount is not positive.
    Invalid,
    /// The agreement's stored opening hash or verified amount differs.
    ChangedEvidence,
    /// The revision and `issued_at` do not both increase over the stored vote.
    Stale,
    /// The opening transaction already backs another agreement's vote.
    PaymentReused,
    /// The total or per-provider admission bound is reached.
    Capacity,
}
impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Self::Invalid => "feedback evidence is invalid",
            Self::ChangedEvidence => "an agreement cannot change its funding evidence",
            Self::Stale => "feedback update is stale or equivocal; sign a new revision with a higher number and a later timestamp",
            Self::PaymentReused => "this payment already backs another agreement's vote",
            Self::Capacity => "feedback registry capacity reached",
        }
    }
}
/// A refusal of the submission, or a store fault whose detail may hold database text
/// and must never enter a response.
#[derive(Debug)]
pub enum UpsertError {
    Refused(Refusal),
    Fault(String),
}
impl std::fmt::Display for UpsertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(refusal) => f.write_str(refusal.message()),
            Self::Fault(detail) => write!(f, "feedback store fault: {detail}"),
        }
    }
}
fn fault(error: impl std::fmt::Display) -> UpsertError {
    UpsertError::Fault(error.to_string())
}

pub struct RatingStore {
    db: Mutex<Connection>,
    network: NetworkIdentity,
}
impl RatingStore {
    pub fn open(path: &Path, network: &NetworkIdentity) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut db = Connection::open(path).map_err(|e| e.to_string())?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS rating_network (id INTEGER PRIMARY KEY CHECK(id=1),identity TEXT NOT NULL); CREATE TABLE IF NOT EXISTS feedback (quote_id TEXT PRIMARY KEY, tx_hash TEXT NOT NULL UNIQUE, provider TEXT NOT NULL, payer TEXT NOT NULL, revision TEXT NOT NULL, issued_at INTEGER NOT NULL, created_at INTEGER NOT NULL, amount TEXT NOT NULL, eligible INTEGER NOT NULL, evidence TEXT NOT NULL); CREATE INDEX IF NOT EXISTS feedback_provider ON feedback(provider);").map_err(|e| e.to_string())?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let identity = serde_json::to_string(network).map_err(|e| e.to_string())?;
        let existing: Option<String> = tx
            .query_row("SELECT identity FROM rating_network WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        match existing {
            Some(existing) if existing != identity => {
                return Err("feedback database belongs to a different network".into())
            }
            None => {
                tx.execute("INSERT INTO rating_network VALUES(1,?1)", [identity])
                    .map_err(|e| e.to_string())?;
            }
            _ => {}
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Self {
            db: Mutex::new(db),
            network: network.clone(),
        })
    }
    /// Caller passes only the amount derived from canonical chain evidence.
    pub fn upsert(
        &self,
        submission: &FeedbackSubmission,
        amount: &str,
        now: u64,
    ) -> Result<(), UpsertError> {
        crate::reputation::validate_submission(submission, &self.network, now)
            .map_err(|_| UpsertError::Refused(Refusal::Invalid))?;
        match crate::offer_registry::validate_money(amount) {
            Ok(value) if value > 0 => {}
            // A zero or malformed amount cannot back a vote.
            _ => return Err(UpsertError::Refused(Refusal::Invalid)),
        }
        let f = &submission.feedback.payload;
        let mut db = self.db.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(fault)?;
        let previous: Option<(String, String, u64, String, String)> = tx
            .query_row(
                "SELECT tx_hash,revision,issued_at,evidence,amount FROM feedback WHERE quote_id=?1",
                [&f.quote_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(fault)?;
        let evidence = serde_json::to_string(submission).map_err(fault)?;
        if let Some((hash, revision, issued_at, old_evidence, old_amount)) = previous {
            if hash != f.tx_hash || old_amount != amount {
                return Err(UpsertError::Refused(Refusal::ChangedEvidence));
            }
            let revision = revision
                .parse::<u64>()
                .map_err(|_| UpsertError::Fault("corrupt feedback revision".into()))?;
            if revision == f.revision && old_evidence == evidence {
                tx.execute(
                    "UPDATE feedback SET eligible=1 WHERE quote_id=?1",
                    [&f.quote_id],
                )
                .map_err(fault)?;
            } else {
                if f.revision <= revision || f.issued_at <= issued_at {
                    return Err(UpsertError::Refused(Refusal::Stale));
                }
                tx.execute("UPDATE feedback SET revision=?1,issued_at=?2,evidence=?3,eligible=1 WHERE quote_id=?4",params![f.revision.to_string(),f.issued_at,evidence,f.quote_id]).map_err(fault)?;
            }
        } else {
            // The opening hash is unique: look it up inside this write transaction rather
            // than interpreting a constraint failure's database text.
            let voted: Option<String> = tx
                .query_row(
                    "SELECT quote_id FROM feedback WHERE tx_hash=?1",
                    [&f.tx_hash],
                    |r| r.get(0),
                )
                .optional()
                .map_err(fault)?;
            if voted.is_some() {
                return Err(UpsertError::Refused(Refusal::PaymentReused));
            }
            let count: u64 = tx
                .query_row("SELECT COUNT(*) FROM feedback", [], |r| r.get(0))
                .map_err(fault)?;
            let provider_count: u64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM feedback WHERE provider=?1",
                    [&f.provider_wallet],
                    |r| r.get(0),
                )
                .map_err(fault)?;
            if count >= 100_000 || provider_count >= 1000 {
                return Err(UpsertError::Refused(Refusal::Capacity));
            }
            tx.execute("INSERT INTO feedback(quote_id,tx_hash,provider,payer,revision,issued_at,created_at,amount,eligible,evidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1,?9)",params![f.quote_id,f.tx_hash,f.provider_wallet,f.consumer_wallet,f.revision.to_string(),f.issued_at,now,amount,evidence]).map_err(fault)?;
        }
        tx.commit().map_err(fault)
    }
    pub fn events(&self, wallet: &str) -> Result<Vec<ReputationEvent>, String> {
        let db = self.db.lock();
        let mut statement=db.prepare("SELECT evidence,amount,created_at,eligible FROM feedback WHERE provider=?1 ORDER BY created_at,quote_id LIMIT 1000").map_err(|e|e.to_string())?;
        let rows = statement
            .query_map([wallet], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, u64>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            let (evidence, amount_wei, created_at, eligible) = row.map_err(|e| e.to_string())?;
            Ok(ReputationEvent {
                submission: serde_json::from_str(&evidence).map_err(|e| e.to_string())?,
                amount_wei,
                created_at,
                eligible,
            })
        })
        .collect()
    }
    pub fn set_eligible(&self, quote_id: &str, eligible: bool) -> Result<(), String> {
        self.db
            .lock()
            .execute(
                "UPDATE feedback SET eligible=?1 WHERE quote_id=?2",
                params![eligible, quote_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    pub fn providers(&self, after: &str) -> Result<Vec<String>, String> {
        let db = self.db.lock();
        let mut s=db.prepare("SELECT DISTINCT provider FROM feedback WHERE provider>?1 ORDER BY provider LIMIT 32").map_err(|e|e.to_string())?;
        let rows = s
            .query_map([after], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())).collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::protocol::{self, ModelSpec, PriceSchedule, Quote};
    use crate::reputation::{compute_reputation, Feedback};
    pub(crate) const PAYER: &str =
        "1111111111111111111111111111111111111111111111111111111111111111";
    pub(crate) const PROVIDER: &str =
        "2222222222222222222222222222222222222222222222222222222222222222";
    pub(crate) fn submission(now: u64) -> FeedbackSubmission {
        let network = NetworkIdentity {
            chain_id: crate::network::ESPACE_TESTNET_CHAIN_ID,
            genesis_hash: crate::network::ESPACE_TESTNET_GENESIS.into(),
            deployment_id: "test".into(),
        };
        let q = Quote {
            id: format!("0x{}", "11".repeat(32)),
            network: network.clone(),
            offer_id: format!("0x{}", "22".repeat(32)),
            provider_wallet: crate::wallet::address_from_private_key(PROVIDER).unwrap(),
            consumer_wallet: crate::wallet::address_from_private_key(PAYER).unwrap(),
            model: ModelSpec {
                id: "m".into(),
                revision: "ab".repeat(20),
                tokenizer_revision: "ab".repeat(20),
                chat_template_sha256: "ab".repeat(32),
                quantization: None,
                context_length: 4096,
                max_output_tokens: 512,
                capabilities: vec!["chat".into()],
                serving: None,
            },
            price: PriceSchedule {
                input_wei_per_1k: "1".into(),
                output_wei_per_1k: "2".into(),
            },
            funding_wei: "1000000000000000000".into(),
            platform_wallet: format!("0x{}", "33".repeat(20)),
            fee_bps: 50,
            max_concurrency: 1,
            nonce: "nonce".into(),
            issued_at: now,
            expires_at: now + 60,
        };
        let f = Feedback {
            network,
            provider_wallet: q.provider_wallet.clone(),
            consumer_wallet: q.consumer_wallet.clone(),
            quote_id: q.id.clone(),
            tx_hash: format!("0x{}", "44".repeat(32)),
            score: 100,
            revision: 1,
            issued_at: now,
        };
        FeedbackSubmission {
            feedback: protocol::sign(protocol::FEEDBACK_DOMAIN, f, PAYER).unwrap(),
            quote: protocol::sign(protocol::QUOTE_DOMAIN, q, PROVIDER).unwrap(),
            raw_transaction: "0x01".into(),
        }
    }
    #[test]
    fn updates_replace_and_reorg_removes_vote() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = submission(100);
        let n = s.feedback.payload.network.clone();
        let wallet = s.feedback.payload.provider_wallet.clone();
        let store = RatingStore::open(&dir.path().join("ratings.db"), &n).unwrap();
        store.upsert(&s, "1000000000000000000", 100).unwrap();
        store.upsert(&s, "1000000000000000000", 100).unwrap();
        assert_eq!(store.events(&wallet).unwrap().len(), 1);
        s.feedback.payload.score = 0;
        s.feedback.payload.revision = 2;
        s.feedback.payload.issued_at = 101;
        s.feedback = protocol::sign(protocol::FEEDBACK_DOMAIN, s.feedback.payload, PAYER).unwrap();
        store.upsert(&s, "1000000000000000000", 101).unwrap();
        let events = store.events(&wallet).unwrap();
        assert_eq!(events[0].created_at, 100);
        assert_eq!(compute_reputation(&events, &n, &wallet, 101).vote_count, 1);
        assert!(compute_reputation(&events, &n, &wallet, 101).elo < 50.0);
        store
            .set_eligible(&s.feedback.payload.quote_id, false)
            .unwrap();
        assert_eq!(
            compute_reputation(&store.events(&wallet).unwrap(), &n, &wallet, 101).vote_count,
            0
        );
        assert!(store
            .upsert(&submission(100), "1000000000000000000", 102)
            .is_err());
    }
    #[test]
    fn rejects_free_self_and_forged_feedback() {
        let mut s = submission(100);
        let n = s.feedback.payload.network.clone();
        s.feedback.payload.score = 50;
        assert!(crate::reputation::validate_submission(&s, &n, 100).is_err());
        let mut s = submission(100);
        s.quote.payload.funding_wei = "0".into();
        assert!(crate::reputation::validate_submission(&s, &n, 100).is_err());
        let mut s = submission(100);
        s.feedback.payload.consumer_wallet = s.feedback.payload.provider_wallet.clone();
        assert!(crate::reputation::validate_submission(&s, &n, 100).is_err());
    }
    #[test]
    fn refuses_database_network_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.db");
        let mut n = submission(100).feedback.payload.network;
        drop(RatingStore::open(&path, &n).unwrap());
        n.deployment_id = "reset".into();
        assert!(RatingStore::open(&path, &n).is_err());
    }
    #[test]
    fn one_payment_cannot_vote_for_two_agreements() {
        let dir = tempfile::tempdir().unwrap();
        let first = submission(100);
        let store =
            RatingStore::open(&dir.path().join("r.db"), &first.feedback.payload.network).unwrap();
        store.upsert(&first, "1000000000000000000", 100).unwrap();
        let mut second = first.clone();
        second.quote.payload.id = format!("0x{}", "55".repeat(32));
        second.feedback.payload.quote_id = second.quote.payload.id.clone();
        second.quote =
            protocol::sign(protocol::QUOTE_DOMAIN, second.quote.payload, PROVIDER).unwrap();
        second.feedback =
            protocol::sign(protocol::FEEDBACK_DOMAIN, second.feedback.payload, PAYER).unwrap();
        assert!(store.upsert(&second, "1000000000000000000", 100).is_err());
        assert_eq!(
            store
                .events(&first.feedback.payload.provider_wallet)
                .unwrap()
                .len(),
            1
        );
    }
    /// Holds the store's write lock from another connection until dropped.
    pub(crate) fn exclusive_lock(path: &Path) -> Connection {
        let holder = Connection::open(path).unwrap();
        holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
        holder
    }
    #[test]
    fn locked_store_is_a_fault_not_a_refusal() {
        // payment-gated-reputation requirement 7, task 2.2: a lock held beyond the
        // 5-second busy timeout is a store fault, never a refusal of the submission.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.db");
        let s = submission(100);
        let store = RatingStore::open(&path, &s.feedback.payload.network).unwrap();
        let holder = exclusive_lock(&path);
        let error = store.upsert(&s, "1000000000000000000", 100).unwrap_err();
        assert!(matches!(error, UpsertError::Fault(_)), "{error:?}");
        drop(holder);
        store.upsert(&s, "1000000000000000000", 100).unwrap();
        assert_eq!(store.events(&s.feedback.payload.provider_wallet).unwrap().len(), 1);
    }
    #[test]
    fn store_conflicts_are_typed_refusals_without_database_text() {
        // payment-gated-reputation requirements 6 and 7, task 2.2.
        let dir = tempfile::tempdir().unwrap();
        let first = submission(100);
        let store =
            RatingStore::open(&dir.path().join("r.db"), &first.feedback.payload.network).unwrap();
        store.upsert(&first, "1000000000000000000", 100).unwrap();
        let resign = |mut s: FeedbackSubmission| {
            s.feedback = protocol::sign(protocol::FEEDBACK_DOMAIN, s.feedback.payload, PAYER).unwrap();
            s
        };
        // A payment already voted for another agreement, found without its constraint text.
        let mut second = first.clone();
        second.quote.payload.id = format!("0x{}", "55".repeat(32));
        second.feedback.payload.quote_id = second.quote.payload.id.clone();
        second.quote =
            protocol::sign(protocol::QUOTE_DOMAIN, second.quote.payload, PROVIDER).unwrap();
        let second = resign(second);
        let reused = store.upsert(&second, "1000000000000000000", 100).unwrap_err();
        assert!(matches!(reused, UpsertError::Refused(Refusal::PaymentReused)), "{reused:?}");
        // Equivocation at one revision, and a higher revision without a later timestamp.
        let mut equivocal = first.clone();
        equivocal.feedback.payload.score = 0;
        let equivocal = store.upsert(&resign(equivocal), "1000000000000000000", 100).unwrap_err();
        assert!(matches!(equivocal, UpsertError::Refused(Refusal::Stale)), "{equivocal:?}");
        let mut same_time = first.clone();
        same_time.feedback.payload.revision = 2;
        let same_time = store.upsert(&resign(same_time), "1000000000000000000", 100).unwrap_err();
        assert!(matches!(same_time, UpsertError::Refused(Refusal::Stale)), "{same_time:?}");
        // The same agreement with another verified amount or another opening hash.
        let amount = store.upsert(&first, "999999999999999999", 100).unwrap_err();
        assert!(matches!(amount, UpsertError::Refused(Refusal::ChangedEvidence)), "{amount:?}");
        let mut hash = first.clone();
        hash.feedback.payload.tx_hash = format!("0x{}", "66".repeat(32));
        let hash = store.upsert(&resign(hash), "1000000000000000000", 100).unwrap_err();
        assert!(matches!(hash, UpsertError::Refused(Refusal::ChangedEvidence)), "{hash:?}");
        for error in [&reused, &equivocal, &same_time, &amount, &hash] {
            let text = error.to_string();
            assert!(!text.contains("UNIQUE constraint") && !text.contains("feedback.tx_hash"), "{text}");
        }
        let events = store.events(&first.feedback.payload.provider_wallet).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].submission.feedback.payload.score, 100);
    }
}
