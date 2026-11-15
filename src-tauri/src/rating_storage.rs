//! Durable one-vote-per-agreement storage, scoped to the complete network identity.
use crate::{
    protocol::NetworkIdentity,
    reputation::{FeedbackSubmission, ReputationEvent},
};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;

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
    ) -> Result<(), String> {
        crate::reputation::validate_submission(submission, &self.network, now)?;
        if crate::offer_registry::validate_money(amount)? == 0 {
            return Err("zero payment cannot vote".into());
        }
        let f = &submission.feedback.payload;
        let mut db = self.db.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let previous: Option<(String, String, u64, String, String)> = tx
            .query_row(
                "SELECT tx_hash,revision,issued_at,evidence,amount FROM feedback WHERE quote_id=?1",
                [&f.quote_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let evidence = serde_json::to_string(submission).map_err(|e| e.to_string())?;
        if let Some((hash, revision, issued_at, old_evidence, old_amount)) = previous {
            if hash != f.tx_hash || old_amount != amount {
                return Err("agreement cannot change its funding evidence".into());
            }
            let revision = revision
                .parse::<u64>()
                .map_err(|_| "corrupt feedback revision")?;
            if revision == f.revision && old_evidence == evidence {
                tx.execute(
                    "UPDATE feedback SET eligible=1 WHERE quote_id=?1",
                    [&f.quote_id],
                )
                .map_err(|e| e.to_string())?;
            } else {
                if f.revision <= revision || f.issued_at <= issued_at {
                    return Err("feedback update is stale or equivocal".into());
                }
                tx.execute("UPDATE feedback SET revision=?1,issued_at=?2,evidence=?3,eligible=1 WHERE quote_id=?4",params![f.revision.to_string(),f.issued_at,evidence,f.quote_id]).map_err(|e|e.to_string())?;
            }
        } else {
            let count: u64 = tx
                .query_row("SELECT COUNT(*) FROM feedback", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            let provider_count: u64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM feedback WHERE provider=?1",
                    [&f.provider_wallet],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if count >= 100_000 || provider_count >= 1000 {
                return Err("feedback registry capacity reached".into());
            }
            tx.execute("INSERT INTO feedback(quote_id,tx_hash,provider,payer,revision,issued_at,created_at,amount,eligible,evidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1,?9)",params![f.quote_id,f.tx_hash,f.provider_wallet,f.consumer_wallet,f.revision.to_string(),f.issued_at,now,amount,evidence]).map_err(|e|format!("payment already voted or storage failure: {e}"))?;
        }
        tx.commit().map_err(|e| e.to_string())
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
    const PAYER: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const PROVIDER: &str = "2222222222222222222222222222222222222222222222222222222222222222";
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
}
