//! Durable inference accounting. SQLite owns all credit, credential and request
//! transitions; monetary values are decimal TEXT, never SQLite numeric values.
use crate::{
    auth::AuthPayload,
    chain_client::VerifiedPayment,
    protocol::{
        self, AccountView, Credential, FundingIntent, ModelSpec, NetworkIdentity, PriceSchedule,
        Quote, Signed, TokenUsage, UsageReceipt,
    },
};
use primitive_types::U256;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, sync::Mutex, time::Duration};

pub const FEE_MIN_REMIT_WEI: u128 = 1_000_000_000_000_000;
pub const FEE_MAX_AGE_SECS: u64 = 86_400;
const MAX_PENDING: i64 = 10_000;
const MAX_CREDENTIALS: i64 = 64;
/// `metadata` key prefix marking a credited payment whose chain recheck the
/// ledger refused to apply; see `BillingStore::quarantine_funding_recheck`.
const FUNDING_RECHECK_REFUSED: &str = "funding-recheck-refused:";
/// `metadata` key prefix holding the time a confirmed fee batch was last
/// rechecked; see `BillingStore::mark_fee_batch_checked`.
const FEE_BATCH_CHECKED: &str = "fee-batch-checked:";
/// Pending fee obligations read per chunk while sealing one platform wallet.
const FEE_SEAL_CHUNK: i64 = 1_000;
const CREDENTIAL_TTL_MAX: u64 = 90 * 86_400;

/// The test-only pause points of the ledger's admission write transactions
/// (specify-non-reserving-quotes 3.3/4.3), each after a final capacity check has passed and
/// before the accepted evidence is inserted, inside the same write transaction.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdmissionPoint {
    /// `register_funding_intent` has passed the active paid-lease check and has not yet
    /// inserted the acknowledgement and intent.
    PaidLease,
    /// `open_account`'s first free opening has passed the owned and ledger-wide account
    /// checks and has not yet inserted the account.
    FreeAccount,
}

pub struct BillingStore {
    db: Mutex<Connection>,
    network: NetworkIdentity,
    /// Test-only pause point of the admission transactions: when armed, the next transaction
    /// of this store that reaches the armed `AdmissionPoint` reports on the sender and
    /// continues once the receiver resolves (or its sender is dropped).
    #[cfg(test)]
    admission_pause: Mutex<
        Option<(AdmissionPoint, std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
    >,
}

#[derive(Debug, Clone)]
pub struct CredentialContext {
    pub account: AccountView,
    pub credential_id: String,
    pub scopes: Vec<String>,
}

/// The credential options body of issuance and rotation (book line 345). It decodes
/// only from one JSON object, like `OpenRequest` and `TopupRequest`: a derived decoder
/// would also accept the four members as a positional array.
#[derive(Debug, Clone, Serialize)]
pub struct CredentialIssueRequest {
    pub expires_at: u64,
    pub spend_cap_wei: Option<String>,
    pub max_concurrency: u32,
    pub scopes: Vec<String>,
}

impl<'de> Deserialize<'de> for CredentialIssueRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            expires_at: u64,
            spend_cap_wei: Option<String>,
            max_concurrency: u32,
            scopes: Vec<String>,
        }
        let fields: Fields = protocol::deserialize_object(deserializer)?;
        Ok(Self {
            expires_at: fields.expires_at,
            spend_cap_wei: fields.spend_cap_wei,
            max_concurrency: fields.max_concurrency,
            scopes: fields.scopes,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialView {
    pub id: String,
    pub contract_id: String,
    pub expires_at: u64,
    pub spend_cap_wei: Option<String>,
    pub spent_wei: String,
    pub reserved_wei: String,
    pub max_concurrency: u32,
    pub scopes: Vec<String>,
    pub revoked: bool,
    pub created_at: u64,
    pub root: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    /// Opaque global identifier passed to settle; client keys remain account-scoped.
    pub id: String,
    pub contract_id: String,
    pub model: ModelSpec,
    pub price: PriceSchedule,
    pub max_cost_wei: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestView {
    pub id: String,
    pub request_id: String,
    pub contract_id: String,
    pub credential_id: String,
    pub status: String,
    pub usage: TokenUsage,
    pub cost_wei: String,
    pub max_cost_wei: String,
    pub provider_loss_wei: String,
    pub prompt_tokens_reserved: u64,
    pub max_output_tokens: u64,
    pub created_at: u64,
    pub settled_at: Option<u64>,
    pub performance: Option<protocol::PerformanceOutcome>,
    pub waived_wei: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingView {
    pub tx_hash: String,
    pub contract_id: String,
    pub status: String,
    pub value_wei: Option<String>,
    pub fee_wei: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeBatch {
    pub id: String,
    pub platform_wallet: String,
    pub amount_wei: String,
    pub status: String,
    pub tx_hash: Option<String>,
    pub raw_transaction: Option<String>,
    pub evidence: Option<VerifiedPayment>,
    pub created_at: u64,
}

impl FeeBatch {
    pub fn payment_data(&self) -> Result<Vec<u8>, String> {
        let id = hex::decode(self.id.strip_prefix("0x").ok_or("invalid fee batch id")?)
            .map_err(|_| "invalid fee batch id")?;
        if id.len() != 32 {
            return Err("invalid fee batch id".into());
        }
        let mut data = b"CHRL-FEE1\0".to_vec();
        data.extend_from_slice(&id);
        Ok(data)
    }
}

fn encode<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| format!("ledger serialization: {e}"))
}
fn decode<T: DeserializeOwned>(value: String) -> Result<T, String> {
    serde_json::from_str(&value).map_err(|e| format!("ledger corruption: {e}"))
}
fn sql(error: rusqlite::Error) -> String {
    format!("ledger database: {error}")
}
pub fn money(value: &str) -> Result<u128, String> {
    if value.is_empty()
        || value.len() > 39
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err("invalid_amount: expected canonical decimal wei".into());
    }
    value
        .parse()
        .map_err(|_| "invalid_amount: wei exceeds u128".into())
}
fn add(a: u128, b: u128) -> Result<u128, String> {
    a.checked_add(b)
        .ok_or_else(|| "amount_overflow: monetary range exceeded".into())
}
fn sub(a: u128, b: u128) -> Result<u128, String> {
    a.checked_sub(b)
        .ok_or_else(|| "ledger invariant: monetary underflow".into())
}
fn narrow(value: U256) -> Result<u128, String> {
    if value > U256::from(u128::MAX) {
        return Err("amount_overflow: monetary range exceeded".into());
    }
    Ok(value.low_u128())
}
/// Round once per request: ceil((input * prompt + output * generated) / 1000).
pub fn usage_cost(price: &PriceSchedule, usage: &TokenUsage) -> Result<u128, String> {
    let input = U256::from(money(&price.input_wei_per_1k)?) * U256::from(usage.prompt_tokens);
    let output = U256::from(money(&price.output_wei_per_1k)?) * U256::from(usage.completion_tokens);
    let total = input
        .checked_add(output)
        .ok_or("amount_overflow: token cost")?;
    narrow((total + U256::from(999u64)) / U256::from(1000u64))
}
pub fn fee_cost(value: u128, bps: u16) -> Result<u128, String> {
    if bps != 50 {
        return Err("invalid_fee: launch fee must be 50 basis points".into());
    }
    // Floor on funding; a sub-200-wei payment cannot create a one-wei debt.
    narrow(U256::from(value) * U256::from(bps) / U256::from(10_000u64))
}
fn get_account(db: &Connection, id: &str) -> Result<AccountView, String> {
    let value = db
        .query_row("SELECT account FROM accounts WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()
        .map_err(sql)?;
    decode(value.ok_or("not_found: account not found")?)
}
fn save_account(db: &Connection, account: &AccountView) -> Result<(), String> {
    // Wide arithmetic accommodates consumed orphan funding without underflow.
    let left =
        U256::from(money(&account.funded_wei)?) + U256::from(money(&account.provider_loss_wei)?);
    let right = U256::from(money(&account.fee_wei)?)
        + U256::from(money(&account.reversed_wei)?)
        + U256::from(money(&account.spent_wei)?)
        + U256::from(money(&account.reserved_wei)?)
        + U256::from(money(&account.available_wei)?);
    if left != right {
        return Err("ledger invariant: account does not balance".into());
    }
    db.execute(
        "UPDATE accounts SET account=?2 WHERE id=?1",
        params![account.contract_id, encode(account)?],
    )
    .map_err(sql)?;
    Ok(())
}
fn get_credential(db: &Connection, id: &str) -> Result<CredentialView, String> {
    let value = db
        .query_row("SELECT record FROM credentials WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()
        .map_err(sql)?;
    decode(value.ok_or("unauthorized: credential not found")?)
}
fn save_credential(db: &Connection, credential: &CredentialView) -> Result<(), String> {
    db.execute(
        "UPDATE credentials SET record=?2,revoked=?3 WHERE id=?1",
        params![credential.id, encode(credential)?, credential.revoked],
    )
    .map_err(sql)?;
    Ok(())
}
fn get_request(db: &Connection, id: &str) -> Result<RequestView, String> {
    let value = db
        .query_row("SELECT record FROM requests WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()
        .map_err(sql)?;
    decode(value.ok_or("not_found: request not found")?)
}
fn save_request(db: &Connection, request: &RequestView) -> Result<(), String> {
    db.execute(
        "UPDATE requests SET record=?2,status=?3 WHERE id=?1",
        params![request.id, encode(request)?, request.status],
    )
    .map_err(sql)?;
    Ok(())
}
fn rows<T: DeserializeOwned>(
    db: &Connection,
    query: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<T>, String> {
    let mut statement = db.prepare(query).map_err(sql)?;
    let mapped = statement
        .query_map(parameters, |r| r.get::<_, String>(0))
        .map_err(sql)?;
    mapped.map(|r| decode(r.map_err(sql)?)).collect()
}

// Reorg loss is the current unfunded liability, not a cumulative debit. Recompute
// it from immutable funding totals so restoration order cannot manufacture credit.
fn balance_funding(account: &mut AccountView) -> Result<(), String> {
    let resources = U256::from(money(&account.funded_wei)?);
    let liabilities = U256::from(money(&account.fee_wei)?)
        + U256::from(money(&account.reversed_wei)?)
        + U256::from(money(&account.spent_wei)?)
        + U256::from(money(&account.reserved_wei)?);
    account.provider_loss_wei = if liabilities > resources {
        narrow(liabilities - resources)?
    } else {
        0
    }
    .to_string();
    account.available_wei = if resources > liabilities {
        narrow(resources - liabilities)?
    } else {
        0
    }
    .to_string();
    Ok(())
}


impl BillingStore {
    pub fn open(path: &Path, network: &NetworkIdentity) -> Result<Self, String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut db = Connection::open(path).map_err(sql)?;
        db.busy_timeout(Duration::from_secs(5)).map_err(sql)?;
        let has_metadata: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='metadata')",
            [], |row| row.get(0),
        ).map_err(sql)?;
        if has_metadata {
            let schema: String = db.query_row("SELECT value FROM metadata WHERE key='schema'", [], |row| row.get(0)).map_err(sql)?;
            if schema != "3" {
                return Err("performance protocol requires a fresh network-scoped ledger; preserve the old ledger and agreements without conversion".into());
            }
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS quotes(id TEXT PRIMARY KEY,owner TEXT NOT NULL,expires INTEGER NOT NULL,record TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS quotes_owner ON quotes(owner);
            CREATE TABLE IF NOT EXISTS challenges(nonce TEXT PRIMARY KEY,wallet TEXT NOT NULL,expires INTEGER NOT NULL,record TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS challenges_wallet ON challenges(wallet);
            CREATE TABLE IF NOT EXISTS accounts(id TEXT PRIMARY KEY,owner TEXT NOT NULL,quote TEXT NOT NULL,account TEXT NOT NULL,sequence INTEGER NOT NULL DEFAULT 0,created INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS credentials(id TEXT PRIMARY KEY,contract TEXT NOT NULL REFERENCES accounts(id),token_hash TEXT UNIQUE NOT NULL,record TEXT NOT NULL,revoked INTEGER NOT NULL DEFAULT 0);
            CREATE INDEX IF NOT EXISTS credentials_contract ON credentials(contract);
            CREATE INDEX IF NOT EXISTS credentials_active ON credentials(contract) WHERE revoked=0;
            CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY,contract TEXT NOT NULL REFERENCES accounts(id),request_key TEXT NOT NULL,credential TEXT NOT NULL REFERENCES credentials(id),status TEXT NOT NULL,created INTEGER NOT NULL,record TEXT NOT NULL,UNIQUE(contract,request_key));
            CREATE INDEX IF NOT EXISTS requests_credential ON requests(credential,created);
            CREATE INDEX IF NOT EXISTS requests_contract ON requests(contract,created);
            CREATE INDEX IF NOT EXISTS requests_active_contract ON requests(contract) WHERE status='reserved';
            CREATE INDEX IF NOT EXISTS requests_active_credential ON requests(credential) WHERE status='reserved';
            CREATE TABLE IF NOT EXISTS funding(tx_hash TEXT PRIMARY KEY,contract TEXT NOT NULL REFERENCES accounts(id),record TEXT NOT NULL,evidence TEXT NOT NULL,status TEXT NOT NULL,checked INTEGER NOT NULL DEFAULT 0);
            CREATE INDEX IF NOT EXISTS funding_recheck ON funding(checked);
            CREATE INDEX IF NOT EXISTS funding_contract ON funding(contract,status);
            CREATE TABLE IF NOT EXISTS funding_attempts(tx_hash TEXT PRIMARY KEY,contract TEXT NOT NULL,record TEXT NOT NULL,raw_transaction TEXT NOT NULL,updated INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS funding_attempts_contract ON funding_attempts(contract);
            CREATE TABLE IF NOT EXISTS funding_intents(contract TEXT PRIMARY KEY,tx_hash TEXT UNIQUE NOT NULL,raw_transaction TEXT NOT NULL,ack TEXT NOT NULL,quote TEXT NOT NULL,sender TEXT NOT NULL,nonce TEXT NOT NULL,admission_until INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS fee_obligations(tx_hash TEXT PRIMARY KEY REFERENCES funding(tx_hash),platform TEXT NOT NULL,amount TEXT NOT NULL,created INTEGER NOT NULL,status TEXT NOT NULL,batch TEXT);
            CREATE INDEX IF NOT EXISTS fee_pending ON fee_obligations(created,tx_hash) WHERE status='pending' AND batch IS NULL;
            CREATE TABLE IF NOT EXISTS fee_batches(id TEXT PRIMARY KEY,status TEXT NOT NULL,record TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS rate_limits(bucket TEXT PRIMARY KEY,window INTEGER NOT NULL,count INTEGER NOT NULL);") .map_err(sql)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let identity = encode(network)?;
        let previous: Option<String> = tx
            .query_row("SELECT value FROM metadata WHERE key='network'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)?;
        match previous {
            Some(previous) if previous != identity => {
                return Err("wrong_network: ledger belongs to another chain/deployment".into())
            }
            None => {
                tx.execute(
                    "INSERT INTO metadata(key,value) VALUES('network',?1),('schema','3')",
                    [identity],
                )
                .map_err(sql)?;
            }
            _ => {}
        }
        tx.execute_batch("CREATE TABLE IF NOT EXISTS performance_observations(
                sequence INTEGER PRIMARY KEY,request TEXT UNIQUE NOT NULL REFERENCES requests(id),
                model TEXT NOT NULL,evidence TEXT NOT NULL,miss INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS performance_observation_window ON performance_observations(model,evidence,sequence);
            CREATE TABLE IF NOT EXISTS performance_withdrawals(
                model TEXT NOT NULL,evidence TEXT NOT NULL,PRIMARY KEY(model,evidence));").map_err(sql)?;
        // Sender nonce identity remains permanent after acknowledgement expiry.
        tx.execute_batch("CREATE INDEX IF NOT EXISTS funding_intents_sender_nonce ON funding_intents(sender,nonce);
            CREATE INDEX IF NOT EXISTS funding_intents_admission ON funding_intents(admission_until);
            CREATE TRIGGER IF NOT EXISTS funding_intents_unique_nonce BEFORE INSERT ON funding_intents
            WHEN NEW.sender IS NULL OR NEW.nonce IS NULL OR NEW.quote IS NULL
                OR EXISTS(SELECT 1 FROM funding_intents WHERE sender=NEW.sender AND nonce=NEW.nonce)
            BEGIN SELECT RAISE(ABORT,'sender nonce already has an acknowledged funding agreement'); END;").map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(Self {
            db: Mutex::new(db),
            network: network.clone(),
            #[cfg(test)]
            admission_pause: Mutex::new(None),
        })
    }

    /// The test-only pause point `point` of an admission transaction, when it is the armed
    /// one: reports that the transaction has reached it and waits, still inside that write
    /// transaction, until the test lets it continue.
    #[cfg(test)]
    fn pause_for_test(&self, point: AdmissionPoint) {
        let armed = {
            let mut pause = self
                .admission_pause
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if pause.as_ref().is_some_and(|(armed, _, _)| *armed == point) {
                pause.take()
            } else {
                None
            }
        };
        if let Some((_, reached, resume)) = armed {
            let _ = reached.send(());
            let _ = resume.recv();
        }
    }

    fn transaction<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let result = operation(&tx)?;
        tx.commit().map_err(sql)?;
        Ok(result)
    }

    pub fn bind_provider(&self, provider: &str) -> Result<(), String> {
        self.transaction(|tx| {
            let previous: Option<String> = tx
                .query_row("SELECT value FROM metadata WHERE key='provider'", [], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(sql)?;
            if let Some(previous) = previous {
                if previous != provider {
                    return Err("wrong_provider: ledger belongs to another provider".into());
                }
            } else {
                tx.execute(
                    "INSERT INTO metadata(key,value) VALUES('provider',?1)",
                    [provider],
                )
                .map_err(sql)?;
            }
            Ok(())
        })
    }

    pub fn create_challenge(&self, payload: &AuthPayload, now: u64) -> Result<(), String> {
        if payload.network != self.network
            || payload.expires_at <= now
            || payload.expires_at > now.saturating_add(crate::auth::CHALLENGE_TTL_SECS)
        {
            return Err("invalid_challenge: invalid lifetime or network".into());
        }
        self.transaction(|tx| {
            cleanup_pending(tx, now)?;
            challenge_rate_limit(tx, &payload.wallet, now)?;
            let total: i64 = tx
                .query_row("SELECT COUNT(*) FROM challenges", [], |r| r.get(0))
                .map_err(sql)?;
            let owner: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM challenges WHERE wallet=?1",
                    [&payload.wallet],
                    |r| r.get(0),
                )
                .map_err(sql)?;
            if total >= MAX_PENDING || owner >= 8 {
                return Err("rate_limit: too many pending challenges".into());
            }
            tx.execute(
                "INSERT INTO challenges(nonce,wallet,expires,record) VALUES(?1,?2,?3,?4)",
                params![
                    payload.nonce,
                    payload.wallet,
                    payload.expires_at,
                    encode(payload)?
                ],
            )
            .map_err(sql)?;
            Ok(())
        })
    }
    pub fn challenge(&self, nonce: &str) -> Result<AuthPayload, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        let value = db
            .query_row(
                "SELECT record FROM challenges WHERE nonce=?1",
                [nonce],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        decode(value.ok_or("unauthorized: challenge missing or already consumed")?)
    }
    pub fn consume_challenge(
        &self,
        nonce: &str,
        expected: &AuthPayload,
        now: u64,
    ) -> Result<(), String> {
        self.transaction(|tx| {
            let changed = tx
                .execute(
                    "DELETE FROM challenges WHERE nonce=?1 AND record=?2 AND expires>?3",
                    params![nonce, encode(expected)?, now],
                )
                .map_err(sql)?;
            if changed != 1 {
                return Err("unauthorized: challenge expired or already consumed".into());
            }
            Ok(())
        })
    }
    /// Spend durable signing work before invoking the signer. Neither signing
    /// failure nor a later quote-insertion rollback refunds this permit.
    pub(crate) fn sign_quote(
        &self,
        quote: Quote,
        private_key: &str,
        now: u64,
    ) -> Result<Signed<Quote>, String> {
        // Initialization refuses outwardly but must commit its exhausted state.
        if !self.transaction(|tx| claim_quote_work(tx, now))? {
            return Err("rate_limit: quote signing work limit exceeded".into());
        }
        protocol::sign(protocol::QUOTE_DOMAIN, quote, private_key)
    }

    pub fn store_quote(&self, quote: &Signed<Quote>, now: u64) -> Result<(), String> {
        if quote.payload.network != self.network || quote.payload.expires_at <= now {
            return Err("invalid_quote: wrong network or expired quote".into());
        }
        validate_quote(&quote.payload)?;
        self.bind_provider(&quote.payload.provider_wallet)?;
        self.transaction(|tx| {
            proposal_rate_limit(tx, now)?;
            cleanup_pending(tx, now)?;
            let total: i64 = tx
                .query_row("SELECT COUNT(*) FROM quotes", [], |r| r.get(0))
                .map_err(sql)?;
            if total >= MAX_PENDING {
                return Err("rate_limit: too many pending quotes".into());
            }
            tx.execute(
                "INSERT INTO quotes(id,owner,expires,record) VALUES(?1,?2,?3,?4)",
                params![
                    quote.payload.id,
                    quote.payload.consumer_wallet,
                    quote.payload.expires_at,
                    encode(quote)?
                ],
            )
            .map_err(sql)?;
            Ok(())
        })
    }
    pub fn quote(&self, id: &str) -> Result<Signed<Quote>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        let value: Option<String> = db.query_row("SELECT quote FROM accounts WHERE id=?1 UNION ALL SELECT quote FROM funding_intents WHERE contract=?1 UNION ALL SELECT record FROM quotes WHERE id=?1 LIMIT 1", [id], |r| r.get(0)).optional().map_err(sql)?;
        decode(value.ok_or("not_found: quote not found")?)
    }
    pub fn account(&self, id: &str) -> Result<AccountView, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        get_account(&db, id)
    }
    pub fn authenticate(&self, bearer: &str, now: u64) -> Result<CredentialContext, String> {
        if bearer.len() != 71 || !bearer.starts_with("chiral_") {
            return Err("unauthorized: invalid credential".into());
        }
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        let value: Option<String> = db
            .query_row(
                "SELECT record FROM credentials WHERE token_hash=?1",
                [protocol::hash_hex(bearer.as_bytes())],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        let credential: CredentialView = decode(value.ok_or("unauthorized: invalid credential")?)?;
        if credential.revoked || credential.expires_at <= now {
            return Err("unauthorized: expired or revoked credential".into());
        }
        Ok(CredentialContext {
            account: get_account(&db, &credential.contract_id)?,
            credential_id: credential.id,
            scopes: credential.scopes,
        })
    }
}

fn cleanup_pending(tx: &Connection, now: u64) -> Result<(), String> {
    tx.execute("DELETE FROM challenges WHERE expires<=?1", [now])
        .map_err(sql)?;
    tx.execute("DELETE FROM quotes WHERE expires<=?1", [now])
        .map_err(sql)?;
    tx.execute("UPDATE funding_intents SET admission_until=0 WHERE admission_until>0 AND admission_until<=?1", [now]).map_err(sql)?;
    tx.execute("DELETE FROM funding_attempts WHERE updated<?1 AND json_extract(record,'$.status')='rejected' AND NOT EXISTS(SELECT 1 FROM funding_intents WHERE tx_hash=funding_attempts.tx_hash)", [now.saturating_sub(86_400)]).map_err(sql)?;
    // Proposal, signing-work and challenge rates prune only their own evidence,
    // relative to their own committed minutes; another operation's time never does.
    tx.execute("DELETE FROM rate_limits WHERE window<?1 AND bucket NOT GLOB 'quote:*' AND bucket<>'quote-work:global'
        AND bucket NOT GLOB 'challenge:*' AND bucket<>'challenge-minute'", [now / 60])
        .map_err(sql)?;
    Ok(())
}
// This independent transaction conservatively initializes exhausted. Missing or
// inconsistent installed evidence is never repaired by a request.
fn claim_quote_work(tx: &Connection, now: u64) -> Result<bool, String> {
    const INVALID: &str = "ledger invariant: inconsistent quote signing work evidence";
    let watermark: Option<String> = tx.query_row(
        "SELECT value FROM metadata WHERE key='quote_work_minute'",
        [], |row| row.get(0),
    ).optional().map_err(sql)?;
    let bucket: Option<(u64, u64)> = tx.query_row(
        "SELECT window,count FROM rate_limits WHERE bucket='quote-work:global'",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(sql)?;
    let sampled = now / 60;
    let (window, count, permitted) = match (watermark, bucket) {
        (None, None) => (sampled, 600, false),
        (Some(value), Some((previous, spent))) => {
            if value.is_empty() || (value.len() > 1 && value.starts_with('0'))
                || !value.bytes().all(|byte| byte.is_ascii_digit())
                || value.parse::<u64>().map_err(|_| INVALID.to_string())? != previous
                || previous > u64::MAX / 60 || !(1..=600).contains(&spent) {
                return Err(INVALID.into());
            }
            let window = sampled.max(previous);
            if window == previous && spent == 600 {
                return Ok(false);
            }
            (window, if window == previous { spent + 1 } else { 1 }, true)
        }
        _ => return Err(INVALID.into()),
    };
    tx.execute(
        "INSERT INTO rate_limits(bucket,window,count) VALUES('quote-work:global',?1,?2)
         ON CONFLICT(bucket) DO UPDATE SET window=excluded.window,count=excluded.count",
        params![window, count],
    ).map_err(sql)?;
    tx.execute(
        "INSERT INTO metadata(key,value) VALUES('quote_work_minute',?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [window],
    ).map_err(sql)?;
    Ok(permitted)
}

// Committed issuance is independent of signing work and named payer identity.
// Adoption, clock advancement and legacy retirement share the quote transaction.
fn proposal_rate_limit(tx: &Connection, now: u64) -> Result<(), String> {
    const INVALID: &str = "ledger invariant: inconsistent committed quote rate evidence";
    let watermark: Option<String> = tx.query_row(
        "SELECT value FROM metadata WHERE key='quote_committed_minute'",
        [], |row| row.get(0),
    ).optional().map_err(sql)?;
    let global: Option<(u64, u64)> = tx.query_row(
        "SELECT window,count FROM rate_limits WHERE bucket='quote:global'",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(sql)?;
    if global.is_some_and(|(minute, count)| minute > u64::MAX / 60 || !(1..=600).contains(&count)) {
        return Err(INVALID.into());
    }
    if let Some(value) = &watermark {
        if value.is_empty() || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(INVALID.into());
        }
        let minute = value.parse::<u64>().map_err(|_| INVALID.to_string())?;
        if global.map(|(window, _)| window) != Some(minute) {
            return Err(INVALID.into());
        }
    }
    {
        let mut statement = tx.prepare(
            "SELECT window,count FROM rate_limits WHERE bucket GLOB 'quote:*' AND bucket<>'quote:global'",
        ).map_err(sql)?;
        let mut legacy = statement.query([]).map_err(sql)?;
        while let Some(row) = legacy.next().map_err(sql)? {
            // Installed state must have retired all old wallet rows atomically.
            // Before installation they establish consistency, never allowance.
            if watermark.is_some() {
                return Err(INVALID.into());
            }
            let minute: u64 = row.get(0).map_err(sql)?;
            let count: u64 = row.get(1).map_err(sql)?;
            let Some((global_minute, global_count)) = global else {
                return Err(INVALID.into());
            };
            if minute > global_minute || !(1..=12).contains(&count)
                || (minute == global_minute && count > global_count) {
                return Err(INVALID.into());
            }
        }
    }
    let sampled = now / 60;
    let window = global.map_or(sampled, |(previous, _)| sampled.max(previous));
    let spent = global.filter(|(previous, _)| *previous == window).map_or(0, |(_, count)| count);
    if spent == 600 {
        return Err("rate_limit: committed quote issuance limit exceeded".into());
    }
    tx.execute(
        "INSERT INTO rate_limits(bucket,window,count) VALUES('quote:global',?1,?2)
         ON CONFLICT(bucket) DO UPDATE SET window=excluded.window,count=excluded.count",
        params![window, spent + 1],
    ).map_err(sql)?;
    tx.execute(
        "INSERT INTO metadata(key,value) VALUES('quote_committed_minute',?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [window],
    ).map_err(sql)?;
    if watermark.is_none() {
        tx.execute("DELETE FROM rate_limits WHERE bucket GLOB 'quote:*' AND bucket<>'quote:global'", [])
            .map_err(sql)?;
    }
    Ok(())
}

// Committed challenge issuance has its own non-regressing minute E = max(W, L).
// 'challenge-minute' is the watermark L with the ledger units committed in it,
// beside the 'challenge:global' (600) and 'challenge:<wallet>' (12) counters.
// All of it lives in rate_limits, apart from both quote clocks, so issuance
// touches only challenge rows. A watermark is installed by the first committed
// issuance: before that, retained counters are adopted at the greatest retained
// minute. Inconsistent evidence fails closed rather than granting allowance.
fn challenge_rate_limit(tx: &Connection, wallet: &str, now: u64) -> Result<(), String> {
    const INVALID: &str = "ledger invariant: inconsistent challenge rate evidence";
    let row = |bucket: &str| -> Result<Option<(u64, u64)>, String> {
        tx.query_row(
            "SELECT window,count FROM rate_limits WHERE bucket=?1",
            [bucket], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(sql)
    };
    let watermark = row("challenge-minute")?;
    let global = row("challenge:global")?;
    if global.is_some_and(|(minute, count)| minute > u64::MAX / 60 || !(1..=600).contains(&count)) {
        return Err(INVALID.into());
    }
    // An installed watermark always commits together with the ledger counter.
    if watermark.is_some() && watermark != global {
        return Err(INVALID.into());
    }
    let own_bucket = format!("challenge:{wallet}");
    let (mut current, mut own) = (0_u64, 0_u64);
    {
        let mut statement = tx.prepare(
            "SELECT bucket,window,count FROM rate_limits WHERE bucket GLOB 'challenge:*' AND bucket<>'challenge:global'",
        ).map_err(sql)?;
        let mut wallets = statement.query([]).map_err(sql)?;
        while let Some(row) = wallets.next().map_err(sql)? {
            let bucket: String = row.get(0).map_err(sql)?;
            let minute: u64 = row.get(1).map_err(sql)?;
            let count: u64 = row.get(2).map_err(sql)?;
            // Spent wallet units need ledger evidence at or after their minute.
            // Older rows are superseded, not current charges; commit prunes them.
            let Some((global_minute, _)) = global else {
                return Err(INVALID.into());
            };
            if minute > global_minute || !(1..=12).contains(&count) {
                return Err(INVALID.into());
            }
            if minute == global_minute {
                current = current.saturating_add(count);
                if bucket == own_bucket {
                    own = count;
                }
            }
        }
    }
    if global.is_some_and(|(_, count)| count != current) {
        return Err(INVALID.into());
    }
    let sampled = now / 60;
    let window = global.map_or(sampled, |(previous, _)| sampled.max(previous));
    let (spent, own) = match global {
        Some((previous, count)) if previous == window => (count, own),
        _ => (0, 0),
    };
    if spent >= 600 || own >= 12 {
        return Err("rate_limit: control request limit exceeded".into());
    }
    for (bucket, count) in [("challenge:global", spent + 1), ("challenge-minute", spent + 1),
        (own_bucket.as_str(), own + 1)] {
        tx.execute(
            "INSERT INTO rate_limits(bucket,window,count) VALUES(?1,?2,?3)
             ON CONFLICT(bucket) DO UPDATE SET window=excluded.window,count=excluded.count",
            params![bucket, window, count],
        ).map_err(sql)?;
    }
    tx.execute(
        "DELETE FROM rate_limits WHERE bucket GLOB 'challenge:*' AND bucket<>'challenge:global' AND window<?1",
        [window],
    ).map_err(sql)?;
    Ok(())
}
fn validate_quote(quote: &Quote) -> Result<(), String> {
    money(&quote.funding_wei)?;
    money(&quote.price.input_wei_per_1k)?;
    money(&quote.price.output_wei_per_1k)?;
    fee_cost(0, quote.fee_bps)?;
    crate::auth::canonical_wallet(&quote.consumer_wallet)?;
    crate::auth::canonical_wallet(&quote.provider_wallet)?;
    crate::auth::canonical_wallet(&quote.platform_wallet)?;
    if quote.max_concurrency == 0
        || quote.model.context_length == 0
        || quote.model.max_output_tokens > quote.model.context_length
    {
        return Err("invalid_quote: invalid model limits".into());
    }
    let free =
        money(&quote.price.input_wei_per_1k)? == 0 && money(&quote.price.output_wei_per_1k)? == 0;
    if free != (money(&quote.funding_wei)? == 0) {
        return Err(
            "invalid_quote: free sessions require zero funding; paid sessions require funding"
                .into(),
        );
    }
    Ok(())
}

impl BillingStore {
    /// The owner proof is consumed before this operation. An existing account
    /// is returned without changing credentials or disclosing an old bearer.
    pub fn open_account(
        &self,
        quote: &Signed<Quote>,
        payment: Option<&VerifiedPayment>,
        now: u64,
    ) -> Result<crate::protocol::OpenResult, String> {
        validate_quote(&quote.payload)?;
        if quote.payload.network != self.network {
            return Err("wrong_network: quote belongs to another network".into());
        }
        self.bind_provider(&quote.payload.provider_wallet)?;
        self.transaction(|tx| {
            let q = &quote.payload;
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)", [&q.id], |r| r.get(0)).map_err(sql)?;
            if exists {
                let stored: Signed<Quote> = decode(tx.query_row("SELECT quote FROM accounts WHERE id=?1", [&q.id], |r| r.get(0)).map_err(sql)?)?;
                if stored.payload != *q { return Err("conflict: account terms do not match quote".into()); }
            } else {
                if q.funding_wei == "0" && q.expires_at <= now { return Err("expired_quote: quote expired".into()); }
                let pending: Option<String> = tx.query_row("SELECT quote FROM funding_intents WHERE contract=?1 UNION ALL SELECT record FROM quotes WHERE id=?1 LIMIT 1", [&q.id], |r| r.get(0)).optional().map_err(sql)?;
                if pending.as_deref() != Some(encode(quote)?.as_str()) { return Err("invalid_quote: quote was not issued by this provider".into()); }
                let free = money(&q.funding_wei)? == 0;
                if free {
                    if payment.is_some() { return Err("invalid_payment: free sessions do not accept a transfer".into()); }
                    let count: i64 = tx.query_row("SELECT COUNT(*) FROM accounts WHERE owner=?1", [&q.consumer_wallet], |r| r.get(0)).map_err(sql)?;
                    let total: i64 = tx.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0)).map_err(sql)?;
                    if count >= 8 || total >= MAX_PENDING { return Err("rate_limit: free account limit reached".into()); }
                    #[cfg(test)]
                    self.pause_for_test(AdmissionPoint::FreeAccount);
                } else {
                    let verified = payment.ok_or("invalid_payment: funding transaction required")?;
                    validate_payment(q, verified, true)?;
                    require_funding_intent(tx,q,&verified.raw_transaction,&verified.tx_hash)?;
                }
                let account = AccountView {
                    contract_id:q.id.clone(),network:q.network.clone(),provider_wallet:q.provider_wallet.clone(),
                    consumer_wallet:q.consumer_wallet.clone(),model:q.model.clone(),price:q.price.clone(),
                    status:if free { "free".into() } else { "active".into() },
                    funded_wei:"0".into(),fee_wei:"0".into(),spent_wei:"0".into(),reserved_wei:"0".into(),
                    available_wei:"0".into(),reversed_wei:"0".into(),provider_loss_wei:"0".into(),
                    usage:TokenUsage::default(),max_concurrency:q.max_concurrency,waived_wei:"0".into(),
                };
                tx.execute("INSERT INTO accounts(id,owner,quote,account,created) VALUES(?1,?2,?3,?4,?5)", params![q.id,q.consumer_wallet,encode(quote)?,encode(&account)?,now]).map_err(sql)?;
                if let Some(verified) = payment { credit_funding(tx, q, verified, now)?; }
                tx.execute("DELETE FROM quotes WHERE id=?1", [&q.id]).map_err(sql)?;
                tx.execute("UPDATE funding_intents SET admission_until=0 WHERE contract=?1", [&q.id]).map_err(sql)?;
            }
            let account = get_account(tx, &q.id)?;
            if account.status == "frozen" { return Err("account_frozen: funding requires operator investigation".into()); }
            if exists {
                return Ok(crate::protocol::OpenResult { account, credential: None });
            }
            let request = CredentialIssueRequest { expires_at:now.saturating_add(30*86_400),spend_cap_wei:None,max_concurrency:q.max_concurrency,scopes:vec!["infer".into(),"read".into()] };
            let credential = issue_credential(tx, &account, &request, true, now)?;
            Ok(crate::protocol::OpenResult { account, credential: Some(credential) })
        })
    }

    pub fn credit_topup(
        &self,
        id: &str,
        payment: &VerifiedPayment,
        now: u64,
    ) -> Result<AccountView, String> {
        self.transaction(|tx| {
            let signed: Signed<Quote> = decode(
                tx.query_row("SELECT quote FROM accounts WHERE id=?1", [id], |r| r.get(0))
                    .optional()
                    .map_err(sql)?
                    .ok_or("not_found: account not found")?,
            )?;
            let account = get_account(tx, id)?;
            if account.status != "active" {
                return Err("forbidden: only active paid accounts accept top-ups".into());
            }
            validate_payment(&signed.payload, payment, false)?;
            credit_funding(tx, &signed.payload, payment, now)?;
            get_account(tx, id)
        })
    }

    pub fn record_funding_attempt(
        &self,
        id: &str,
        raw: &str,
        status: &str,
        now: u64,
    ) -> Result<(), String> {
        if !matches!(status, "pending" | "rejected" | "unavailable") {
            return Err("invalid funding state".into());
        }
        let hash = raw_hash(raw)?;
        self.transaction(|tx| {
            let previous: Option<String> = tx.query_row("SELECT record FROM funding_attempts WHERE tx_hash=?1",[&hash],|r|r.get(0)).optional().map_err(sql)?;
            let previous: Option<FundingView> = previous.map(decode).transpose()?;
            if previous.as_ref().is_some_and(|p|p.contract_id != id) { return Err("funding_replay: transaction belongs to another agreement".into()); }
            let registered:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM funding_intents WHERE contract=?1 AND tx_hash=?2 AND raw_transaction=?3)",params![id,hash,raw],|r|r.get(0)).map_err(sql)?;
            if previous.is_none() && !registered {
                let count: i64 = tx.query_row("SELECT COUNT(*) FROM funding_attempts WHERE NOT EXISTS(SELECT 1 FROM funding_intents WHERE tx_hash=funding_attempts.tx_hash)", [], |r| r.get(0)).map_err(sql)?;
                let per_account: i64 = tx.query_row("SELECT COUNT(*) FROM funding_attempts WHERE contract=?1",[id],|r|r.get(0)).map_err(sql)?;
                if count >= MAX_PENDING || per_account >= 32 { return Err("rate_limit: too many pending funding attempts".into()); }
            }
            let view = FundingView { tx_hash:hash.clone(),contract_id:id.into(),status:status.into(),value_wei:None,fee_wei:None,created_at:previous.map(|p|p.created_at).unwrap_or(now) };
            tx.execute("INSERT INTO funding_attempts(tx_hash,contract,record,raw_transaction,updated) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(tx_hash) DO UPDATE SET record=excluded.record,updated=excluded.updated", params![hash,id,encode(&view)?,raw,now]).map_err(sql)?;
            if status=="rejected" {tx.execute("UPDATE funding_intents SET admission_until=0 WHERE contract=?1 AND tx_hash=?2",params![id,hash]).map_err(sql)?;}
            Ok(())
        })
    }

    pub fn funding_history(&self, id: &str) -> Result<Vec<FundingView>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        rows(&db, "SELECT record FROM (SELECT record,json_extract(record,'$.created_at') AS created FROM funding WHERE contract=?1 UNION ALL SELECT record,updated AS created FROM funding_attempts WHERE contract=?1 AND tx_hash NOT IN (SELECT tx_hash FROM funding)) ORDER BY created DESC LIMIT 100", [id])
    }

    pub fn issue_credential(
        &self,
        id: &str,
        request: &CredentialIssueRequest,
        now: u64,
    ) -> Result<Credential, String> {
        self.transaction(|tx| {
            let account = get_account(tx, id)?;
            issue_credential(tx, &account, request, false, now)
        })
    }
    pub fn credentials(&self, id: &str) -> Result<Vec<CredentialView>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        rows(
            &db,
            "SELECT record FROM credentials WHERE contract=?1 ORDER BY rowid DESC LIMIT 100",
            [id],
        )
    }
    pub fn revoke_credential(&self, id: &str, credential_id: &str) -> Result<(), String> {
        self.transaction(|tx| {
            let mut credential = get_credential(tx, credential_id)?;
            if credential.contract_id != id {
                return Err("not_found: credential not found".into());
            }
            credential.revoked = true;
            save_credential(tx, &credential)
        })
    }
    pub fn rotate_credential(
        &self,
        id: &str,
        credential_id: &str,
        request: &CredentialIssueRequest,
        now: u64,
    ) -> Result<Credential, String> {
        self.transaction(|tx| {
            let mut previous = get_credential(tx, credential_id)?;
            if previous.contract_id != id {
                return Err("not_found: credential not found".into());
            }
            previous.revoked = true;
            save_credential(tx, &previous)?;
            let account = get_account(tx, id)?;
            issue_credential(tx, &account, request, previous.root, now)
        })
    }
}

fn issue_credential(
    tx: &Connection,
    account: &AccountView,
    request: &CredentialIssueRequest,
    root: bool,
    now: u64,
) -> Result<Credential, String> {
    if request.expires_at <= now
        || request.expires_at > now.saturating_add(CREDENTIAL_TTL_MAX)
        || request.max_concurrency == 0
        || request.max_concurrency > account.max_concurrency
        || request.scopes.is_empty()
        || request.scopes.len() > 2
        || request.scopes.iter().any(|s| s != "infer" && s != "read")
    {
        return Err("invalid_credential: invalid expiry, concurrency or scopes".into());
    }
    if let Some(cap) = &request.spend_cap_wei {
        money(cap)?;
    }
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM credentials WHERE contract=?1 AND revoked=0 AND json_extract(record,'$.expires_at')>?2", params![account.contract_id,now], |r| r.get(0)).map_err(sql)?;
    if count >= MAX_CREDENTIALS {
        return Err("rate_limit: too many active credentials".into());
    }
    let id = protocol::random_id();
    let bearer = format!("chiral_{}", &protocol::random_id()[2..]);
    let view = CredentialView {
        id: id.clone(),
        contract_id: account.contract_id.clone(),
        expires_at: request.expires_at,
        spend_cap_wei: request.spend_cap_wei.clone(),
        spent_wei: "0".into(),
        reserved_wei: "0".into(),
        max_concurrency: request.max_concurrency,
        scopes: request.scopes.clone(),
        revoked: false,
        created_at: now,
        root,
    };
    tx.execute(
        "INSERT INTO credentials(id,contract,token_hash,record) VALUES(?1,?2,?3,?4)",
        params![
            id,
            account.contract_id,
            protocol::hash_hex(bearer.as_bytes()),
            encode(&view)?
        ],
    )
    .map_err(sql)?;
    Ok(Credential {
        id,
        bearer,
        contract_id: account.contract_id.clone(),
        expires_at: request.expires_at,
    })
}

fn validate_payment(quote: &Quote, payment: &VerifiedPayment, opening: bool) -> Result<(), String> {
    let data = if opening {
        protocol::opening_data(quote)?
    } else {
        protocol::topup_data(&quote.id)?
    };
    if payment.network != quote.network
        || payment.from != quote.consumer_wallet
        || payment.to != quote.provider_wallet
        || payment.data != data
        || money(&payment.value_wei)? == 0
        || (opening && payment.value_wei != quote.funding_wei)
    {
        return Err("invalid_payment: funding does not match agreement".into());
    }
    Ok(())
}
fn credit_funding(
    tx: &Connection,
    quote: &Quote,
    payment: &VerifiedPayment,
    now: u64,
) -> Result<(), String> {
    let previous: Option<(String, String)> = tx
        .query_row(
            "SELECT contract,status FROM funding WHERE tx_hash=?1",
            [&payment.tx_hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    if let Some((contract, status)) = previous {
        if contract != quote.id {
            return Err("funding_replay: transaction already belongs to another account".into());
        }
        if status != "accepted" {
            return Err("funding_orphaned: transaction credit was reversed".into());
        }
        return Ok(());
    }
    let amount = money(&payment.value_wei)?;
    let fee = fee_cost(amount, quote.fee_bps)?;
    let mut account = get_account(tx, &quote.id)?;
    account.funded_wei = add(money(&account.funded_wei)?, amount)?.to_string();
    account.fee_wei = add(money(&account.fee_wei)?, fee)?.to_string();
    account.available_wei = add(money(&account.available_wei)?, sub(amount, fee)?)?.to_string();
    let funding = FundingView {
        tx_hash: payment.tx_hash.clone(),
        contract_id: quote.id.clone(),
        status: "accepted".into(),
        value_wei: Some(amount.to_string()),
        fee_wei: Some(fee.to_string()),
        created_at: now,
    };
    tx.execute("INSERT INTO funding(tx_hash,contract,record,evidence,status,checked) VALUES(?1,?2,?3,?4,'accepted',?5)", params![payment.tx_hash,quote.id,encode(&funding)?,encode(payment)?,now]).map_err(sql)?;
    if fee > 0 {
        tx.execute("INSERT INTO fee_obligations(tx_hash,platform,amount,created,status) VALUES(?1,?2,?3,?4,'pending')", params![payment.tx_hash,quote.platform_wallet,fee.to_string(),now]).map_err(sql)?;
    }
    tx.execute(
        "DELETE FROM funding_attempts WHERE tx_hash=?1",
        [&payment.tx_hash],
    )
    .map_err(sql)?;
    save_account(tx, &account)
}

fn withdrawn(db: &Connection, model: &ModelSpec) -> Result<bool, String> {
    let Some(profile) = &model.serving else { return Ok(false); };
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM performance_withdrawals WHERE model=?1 AND evidence=?2)",
        params![model.id, profile.qualification.evidence_sha256], |row| row.get(0),
    ).map_err(sql)
}

fn require_execution(db: &Connection, agreement: &ModelSpec, current: &ModelSpec) -> Result<(), String> {
    if !agreement.accepts_execution(current) {
        return Err("model_revision_unavailable: contracted serving terms changed".into());
    }
    if withdrawn(db, current)? {
        return Err("model_unqualified: current qualification has been withdrawn".into());
    }
    Ok(())
}

impl BillingStore {
    pub fn reserve(
        &self,
        contract_id: &str,
        credential_id: &str,
        request_id: &str,
        prompt_tokens: u64,
        max_output_tokens: u64,
        now: u64,
        execution_model: &ModelSpec,
    ) -> Result<Reservation, String> {
        if request_id.is_empty()
            || request_id.len() > 128
            || request_id.bytes().any(|b| !b.is_ascii_graphic())
        {
            return Err("invalid_request_id: expected 1-128 printable ASCII bytes".into());
        }
        self.transaction(|tx| {
            let mut account = get_account(tx,contract_id)?;
            let mut credential = get_credential(tx,credential_id)?;
            if credential.contract_id != contract_id || credential.revoked || credential.expires_at <= now || !credential.scopes.iter().any(|s| s == "infer") {
                return Err("forbidden: credential cannot infer on this account".into());
            }
            if !matches!(account.status.as_str(),"active"|"free") { return Err("account_frozen: funding is not spendable".into()); }
            let duplicate: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM requests WHERE contract=?1 AND (request_key=?2 OR id=?2))", params![contract_id,request_id], |r| r.get(0)).map_err(sql)?;
            if duplicate { return Err("duplicate_request: request already exists; inspect its status".into()); }
            require_execution(tx, &account.model, execution_model)?;
            let context_limit = if max_output_tokens == 0 && account.model.capabilities.iter().any(|capability|capability=="embeddings") {
                account.model.context_length.checked_mul(32).ok_or("invalid_tokens: embedding batch limit overflow")?
            } else { account.model.context_length };
            if max_output_tokens > account.model.max_output_tokens || prompt_tokens.checked_add(max_output_tokens).ok_or("invalid_tokens: token count overflow")? > context_limit {
                return Err("invalid_tokens: model context or output limit exceeded".into());
            }
            let account_active: u32 = tx.query_row("SELECT COUNT(*) FROM requests WHERE contract=?1 AND status='reserved'", [contract_id], |r| r.get(0)).map_err(sql)?;
            let credential_active: u32 = tx.query_row("SELECT COUNT(*) FROM requests WHERE credential=?1 AND status='reserved'", [credential_id], |r| r.get(0)).map_err(sql)?;
            if account_active >= account.max_concurrency || credential_active >= credential.max_concurrency { return Err("saturation: concurrency limit reached".into()); }
            let recent: u32 = tx.query_row("SELECT COUNT(*) FROM requests WHERE credential=?1 AND created>=?2", params![credential_id,now.saturating_sub(60)], |r| r.get(0)).map_err(sql)?;
            let losses: u32 = tx.query_row("SELECT COUNT(*) FROM requests WHERE contract=?1 AND created>=?2 AND status LIKE '%unknown_usage'", params![contract_id,now.saturating_sub(3600)], |r| r.get(0)).map_err(sql)?;
            if recent >= 120 || losses >= 30 { return Err("rate_limit: inference admission limit reached".into()); }
            let maximum = usage_cost(&account.price,&TokenUsage{prompt_tokens,completion_tokens:max_output_tokens})?;
            if maximum > money(&account.available_wei)? { return Err("insufficient_funds: prepaid balance cannot cover the request".into()); }
            let credential_reserved = add(money(&credential.reserved_wei)?,maximum)?;
            if let Some(cap) = &credential.spend_cap_wei {
                if add(money(&credential.spent_wei)?,credential_reserved)? > money(cap)? { return Err("spend_cap: credential spending limit reached".into()); }
            }
            account.available_wei = sub(money(&account.available_wei)?,maximum)?.to_string();
            account.reserved_wei = add(money(&account.reserved_wei)?,maximum)?.to_string();
            credential.reserved_wei = credential_reserved.to_string();
            let id = protocol::random_id();
            let performance = execution_model.serving.as_ref().map(|profile| protocol::PerformanceOutcome {
                evidence_sha256: profile.qualification.evidence_sha256.clone(),
                policy: profile.policy.clone(), eligibility: "pending".into(),
                ttft_us: None, decode_us: None, elapsed_us: None, output_tokens: None,
                attained: None, remedy: "none".into(),
            });
            let view = RequestView {id:id.clone(),request_id:request_id.into(),contract_id:contract_id.into(),credential_id:credential_id.into(),status:"reserved".into(),
                usage:TokenUsage::default(),cost_wei:"0".into(),max_cost_wei:maximum.to_string(),provider_loss_wei:"0".into(),
                prompt_tokens_reserved:prompt_tokens,max_output_tokens,created_at:now,settled_at:None,performance,waived_wei:"0".into()};
            tx.execute("INSERT INTO requests(id,contract,request_key,credential,status,created,record) VALUES(?1,?2,?3,?4,'reserved',?5,?6)", params![id,contract_id,request_id,credential_id,now,encode(&view)?]).map_err(sql)?;
            save_account(tx,&account)?; save_credential(tx,&credential)?;
            if let (Some(bound), Some(current)) = (&mut account.model.serving, &execution_model.serving) {
                bound.qualification.evidence_sha256.clone_from(&current.qualification.evidence_sha256);
                bound.qualification.measured_at = current.qualification.measured_at;
            }
            Ok(Reservation{id,contract_id:contract_id.into(),model:account.model,price:account.price,max_cost_wei:maximum.to_string()})
        })
    }

    pub fn settle(
        &self,
        request_id: &str,
        usage: &TokenUsage,
        status: &str,
        now: u64,
    ) -> Result<RequestView, String> {
        self.settle_measured(request_id, usage, status, None, now)
    }

    pub fn settle_measured(
        &self, request_id: &str, usage: &TokenUsage, status: &str,
        performance: Option<&protocol::PerformanceOutcome>, now: u64,
    ) -> Result<RequestView, String> {
        if status == "reserved"
            || status.is_empty()
            || status.len() > 64
            || !status.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        {
            return Err("invalid_status: invalid terminal request status".into());
        }
        let (request, over_limit) =
            self.transaction(|tx| settle_request(tx, request_id, usage, status, performance, now))?;
        if over_limit {
            return Err("usage_over_reservation: request released as provider loss".into());
        }
        Ok(request)
    }
    pub fn performance_withdrawn(&self, model: &ModelSpec) -> Result<bool, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        withdrawn(&db, model)
    }


    pub fn request_view(&self, contract_id: &str, request_id: &str) -> Result<RequestView, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        let value: Option<String> = db.query_row("SELECT record FROM requests WHERE contract=?1 AND (id=?2 OR request_key=?2) ORDER BY CASE WHEN id=?2 THEN 0 ELSE 1 END LIMIT 1", params![contract_id,request_id], |r| r.get(0)).optional().map_err(sql)?;
        decode(value.ok_or("not_found: request not found")?)
    }
    pub fn requests(
        &self,
        contract_id: &str,
        before: Option<u64>,
    ) -> Result<Vec<RequestView>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        rows(&db,"SELECT record FROM requests WHERE contract=?1 AND created<?2 ORDER BY created DESC,rowid DESC LIMIT 100",params![contract_id,before.unwrap_or(i64::MAX as u64)])
    }
    pub fn recover_requests(&self, now: u64) -> Result<usize, String> {
        self.transaction(|tx| {
            let pending: Vec<RequestView> = rows(
                tx,
                "SELECT record FROM requests WHERE status='reserved'",
                [],
            )?;
            for request in &pending {
                settle_request(
                    tx,
                    &request.id,
                    &TokenUsage::default(),
                    "restart_unknown_usage",
                    None,
                    now,
                )?;
            }
            Ok(pending.len())
        })
    }
    pub fn receipt(
        &self,
        contract_id: &str,
        private_key: &str,
        now: u64,
    ) -> Result<Signed<UsageReceipt>, String> {
        self.transaction(|tx| {
            let account = get_account(tx, contract_id)?;
            if crate::wallet::address_from_private_key(private_key)? != account.provider_wallet {
                return Err("wrong_provider: receipt signer mismatch".into());
            }
            let previous: u64 = tx
                .query_row(
                    "SELECT sequence FROM accounts WHERE id=?1",
                    [contract_id],
                    |r| r.get(0),
                )
                .map_err(sql)?;
            let sequence = previous
                .checked_add(1)
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or("receipt sequence exhausted")?;
            let signed = protocol::sign(
                protocol::RECEIPT_DOMAIN,
                UsageReceipt {
                    network: self.network.clone(),
                    provider_wallet: account.provider_wallet.clone(),
                    account,
                    requests: rows(tx, "SELECT record FROM requests WHERE contract=?1 ORDER BY created DESC,rowid DESC LIMIT 100", [contract_id])?,
                    sequence,
                    issued_at: now,
                },
                private_key,
            )?;
            tx.execute(
                "UPDATE accounts SET sequence=?2 WHERE id=?1",
                params![contract_id, sequence],
            )
            .map_err(sql)?;
            Ok(signed)
        })
    }
}

fn settle_request(
    tx: &Connection,
    id: &str,
    usage: &TokenUsage,
    status: &str,
    performance: Option<&protocol::PerformanceOutcome>,
    now: u64,
) -> Result<(RequestView, bool), String> {
    let mut request = get_request(tx, id)?;
    if request.status != "reserved" {
        return Ok((request, false));
    }
    let mut account = get_account(tx, &request.contract_id)?;
    let mut credential = get_credential(tx, &request.credential_id)?;
    let maximum = money(&request.max_cost_wei)?;
    let unknown = status.ends_with("unknown_usage");
    let over_limit = !unknown
        && (usage.prompt_tokens != request.prompt_tokens_reserved
            || usage.completion_tokens > request.max_output_tokens
            || account
                .usage
                .prompt_tokens
                .checked_add(usage.prompt_tokens)
                .is_none()
            || account
                .usage
                .completion_tokens
                .checked_add(usage.completion_tokens)
                .is_none());
    let nominal = if unknown || over_limit {
        0
    } else {
        usage_cost(&account.price, usage)?
    };
    // New reservations bind the admitted report before dispatch. Earlier schema-3
    // reservations could only execute their exact agreement report.
    let admitted_evidence = request.performance.as_ref().map(|p| p.evidence_sha256.as_str())
        .or_else(|| account.model.serving.as_ref().map(|p| p.qualification.evidence_sha256.as_str()));
    let performance = performance.cloned().or_else(|| account.model.serving.as_ref().map(|profile| {
        crate::performance::outcome(profile,
            request.prompt_tokens_reserved <= 2048 && request.max_output_tokens <= 256,
            None, None, if unknown { None } else { Some(usage) }, status, false, None,
            admitted_evidence.unwrap_or(""))
    }));
    if let Some(measured) = &performance {
        let matches = match account.model.serving.as_ref() {
            Some(profile) => Some(measured.evidence_sha256.as_str()) == admitted_evidence && measured.policy == profile.policy,
            None => measured.evidence_sha256.is_empty() && measured.policy == "unqualified"
                && measured.attained.is_none() && measured.remedy == "none",
        };
        if !matches { return Err("performance outcome differs from admitted execution evidence".into()); }
        if !matches!(measured.remedy.as_str(), "none" | "inference_waiver")
            || (measured.attained == Some(false)) != (measured.remedy == "inference_waiver")
            || measured.output_tokens.is_some_and(|tokens| unknown || tokens != usage.completion_tokens) {
            return Err("invalid performance remedy or authoritative usage identity".into());
        }
    }
    let waived = performance.as_ref().is_some_and(|p| p.remedy == "inference_waiver");
    let cost = if waived { 0 } else { nominal };
    request.waived_wei = if waived { nominal } else { 0 }.to_string();
    request.performance = performance;
    account.waived_wei = add(money(&account.waived_wei)?, money(&request.waived_wei)?)?.to_string();
    if cost > maximum {
        return Err("ledger invariant: usage exceeds reservation".into());
    }
    request.status = if over_limit {
        "invalid_usage_unknown_usage".into()
    } else {
        status.into()
    };
    request.usage = if unknown || over_limit {
        TokenUsage::default()
    } else {
        usage.clone()
    };
    request.cost_wei = cost.to_string();
    request.provider_loss_wei = if unknown || over_limit {
        maximum.to_string()
    } else {
        "0".into()
    };
    request.settled_at = Some(now);
    account.reserved_wei = sub(money(&account.reserved_wei)?, maximum)?.to_string();
    account.available_wei = add(money(&account.available_wei)?, sub(maximum, cost)?)?.to_string();
    account.spent_wei = add(money(&account.spent_wei)?, cost)?.to_string();
    account.usage.prompt_tokens = account
        .usage
        .prompt_tokens
        .checked_add(request.usage.prompt_tokens)
        .ok_or("usage counter overflow")?;
    account.usage.completion_tokens = account
        .usage
        .completion_tokens
        .checked_add(request.usage.completion_tokens)
        .ok_or("usage counter overflow")?;
    credential.reserved_wei = sub(money(&credential.reserved_wei)?, maximum)?.to_string();
    credential.spent_wei = add(money(&credential.spent_wei)?, cost)?.to_string();
    save_account(tx, &account)?;
    save_credential(tx, &credential)?;
    save_request(tx, &request)?;
    if let Some(measured) = request.performance.as_ref().filter(|p|
        p.attained.is_some() && (p.eligibility != "short_output_ttft_only" || p.attained == Some(false))
    ) {
        // Insertion order is settlement order, including late-finishing requests
        // and multiple observations in one clock tick. A withdrawal is latched
        // in this same settlement transaction and never erased by later success.
        tx.execute(
            "INSERT INTO performance_observations(request,model,evidence,miss) VALUES(?1,?2,?3,?4)",
            params![request.id, account.model.id, measured.evidence_sha256, measured.attained == Some(false)],
        ).map_err(sql)?;
        tx.execute(
            "INSERT OR IGNORE INTO performance_withdrawals(model,evidence)
             SELECT ?1,?2 FROM (
                SELECT COUNT(*) AS count,COALESCE(SUM(miss),0) AS misses FROM (
                    SELECT miss FROM performance_observations WHERE model=?1 AND evidence=?2 ORDER BY sequence DESC LIMIT 20
                )
             ) WHERE misses>=3 OR (count=20 AND misses>=2)",
            params![account.model.id, measured.evidence_sha256],
        ).map_err(sql)?;
    }
    Ok((request, over_limit))
}

impl BillingStore {
    /// A bounded oldest-checked scan eventually revisits every credited payment.
    pub fn funding_to_recheck(&self) -> Result<Vec<VerifiedPayment>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        rows(
            &db,
            "SELECT evidence FROM funding ORDER BY checked ASC,rowid ASC LIMIT 100",
            [],
        )
    }
    /// Transport failure is not canonical evidence: retain both accepted credit
    /// and an already-pending freeze until an actual chain observation replaces it.
    pub fn apply_funding_recheck(
        &self,
        hash: &str,
        observation: &Result<crate::chain_client::PaymentState, crate::chain_client::ChainError>,
        now: u64,
    ) -> Result<(), String> {
        use crate::chain_client::{ChainError, PaymentState};
        match observation {
            Ok(PaymentState::Accepted(evidence)) => {
                self.transition_funding(hash, "accepted", Some(evidence), now)
            }
            Ok(PaymentState::Pending)
            | Err(ChainError::Pending(_))
            | Err(ChainError::Rejected(_)) => self.transition_funding(hash, "pending", None, now),
            Ok(PaymentState::Orphaned) => self.transition_funding(hash, "orphaned", None, now),
            Err(ChainError::Unavailable(_)) => self.transaction(|tx| {
                tx.execute(
                    "UPDATE funding SET checked=?2 WHERE tx_hash=?1",
                    params![hash, now],
                )
                .map_err(sql)?;
                Ok(())
            }),
        }
    }
    /// A chain observation the ledger refused to apply (for example refreshed
    /// inclusion that would change immutable funding evidence) is quarantined
    /// durably: its `checked` time advances so the bounded oldest-first scan
    /// still reaches every other credited payment, and the refusal is kept in
    /// `metadata` so maintenance stays fail-closed for new funding and offers
    /// until a later actual chain observation for that payment applies
    /// (inference-funding-settlement requirement 9). Funding state, credit and
    /// evidence are left exactly as they were.
    pub fn quarantine_funding_recheck(&self, hash: &str, now: u64) -> Result<(), String> {
        self.transaction(|tx| {
            if tx
                .execute(
                    "UPDATE funding SET checked=?2 WHERE tx_hash=?1",
                    params![hash, now],
                )
                .map_err(sql)?
                == 0
            {
                return Err("not_found: funding not found".into());
            }
            tx.execute(
                "INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![format!("{FUNDING_RECHECK_REFUSED}{hash}"), now.to_string()],
            )
            .map_err(sql)?;
            Ok(())
        })
    }
    /// Quarantined payments, oldest-checked first, so maintenance revisits them
    /// on every pass instead of waiting for their turn in the bounded scan.
    pub fn quarantined_funding_rechecks(&self) -> Result<Vec<VerifiedPayment>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        rows(
            &db,
            "SELECT f.evidence FROM funding f JOIN metadata m ON m.key=?1||f.tx_hash ORDER BY f.checked ASC,f.rowid ASC",
            [FUNDING_RECHECK_REFUSED],
        )
    }
    /// Releases a quarantine once an actual chain observation for the payment
    /// has been applied by the ledger.
    pub fn clear_funding_recheck_quarantine(&self, hash: &str) -> Result<(), String> {
        self.transaction(|tx| {
            tx.execute(
                "DELETE FROM metadata WHERE key=?1",
                [format!("{FUNDING_RECHECK_REFUSED}{hash}")],
            )
            .map_err(sql)?;
            Ok(())
        })
    }
    #[cfg(test)]
    fn mark_funding_state(&self, hash: &str, state: &str, now: u64) -> Result<(), String> {
        self.transition_funding(hash, state, None, now)
    }
    fn transition_funding(
        &self,
        hash: &str,
        state: &str,
        evidence: Option<&VerifiedPayment>,
        now: u64,
    ) -> Result<(), String> {
        if !matches!(state, "accepted" | "pending" | "orphaned") {
            return Err("invalid funding state".into());
        }
        self.transaction(|tx| {
            let (record,stored):(String,String)=tx.query_row("SELECT record,evidence FROM funding WHERE tx_hash=?1",[hash],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?.ok_or("not_found: funding not found")?;
            let mut funding:FundingView=decode(record)?;
            if let Some(evidence)=evidence {
                let mut previous:VerifiedPayment=decode(stored)?;
                previous.block_hash=evidence.block_hash.clone();previous.block_number=evidence.block_number;
                if &previous!=evidence {return Err("invalid_payment: refreshed inclusion changes immutable funding evidence".into());}
                tx.execute("UPDATE funding SET evidence=?2 WHERE tx_hash=?1",params![hash,encode(evidence)?]).map_err(sql)?;
            }
            let id=&funding.contract_id;
            if state=="orphaned" && funding.status!="orphaned" {
                // Release every hold before reversing funding. Late engine completion
                // is idempotently uncharged, even if canonical funding later returns.
                let pending:Vec<RequestView>=rows(tx,"SELECT record FROM requests WHERE contract=?1 AND status='reserved'",[id])?;
                for request in pending {settle_request(tx,&request.id,&TokenUsage::default(),"funding_orphan_unknown_usage",None,now)?;}
                let mut account=get_account(tx,id)?;
                let amount=money(funding.value_wei.as_deref().ok_or("ledger funding amount missing")?)?;
                let fee=money(funding.fee_wei.as_deref().ok_or("ledger funding fee missing")?)?;
                let batched:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM fee_obligations WHERE tx_hash=?1 AND batch IS NOT NULL)",[hash],|r|r.get(0)).map_err(sql)?;
                account.reversed_wei=add(money(&account.reversed_wei)?,amount)?.to_string();
                // Sealed fee batches cannot be rewritten, including already-remitted
                // fees. Retain their liability and recognize any shortfall as loss.
                if !batched {account.fee_wei=sub(money(&account.fee_wei)?,fee)?.to_string();}
                tx.execute("UPDATE fee_obligations SET status=CASE WHEN batch IS NULL THEN 'cancelled' WHEN status='remitted' THEN 'orphaned_remitted' ELSE 'orphaned_batched' END WHERE tx_hash=?1",[hash]).map_err(sql)?;
                balance_funding(&mut account)?;save_account(tx,&account)?;
            } else if state=="accepted" && funding.status=="orphaned" {
                let mut account=get_account(tx,id)?;
                let amount=money(funding.value_wei.as_deref().ok_or("ledger funding amount missing")?)?;
                let fee=money(funding.fee_wei.as_deref().ok_or("ledger funding fee missing")?)?;
                let batched:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM fee_obligations WHERE tx_hash=?1 AND batch IS NOT NULL)",[hash],|r|r.get(0)).map_err(sql)?;
                account.reversed_wei=sub(money(&account.reversed_wei)?,amount)?.to_string();
                if !batched {account.fee_wei=add(money(&account.fee_wei)?,fee)?.to_string();}
                tx.execute("UPDATE fee_obligations SET status=CASE WHEN batch IS NULL THEN 'pending' WHEN EXISTS(SELECT 1 FROM fee_batches WHERE id=batch AND status='accepted') THEN 'remitted' ELSE 'batched' END WHERE tx_hash=?1",[hash]).map_err(sql)?;
                balance_funding(&mut account)?;save_account(tx,&account)?;
            }
            // Pending cannot undo a reversal: only verified canonical acceptance can.
            if funding.status!="orphaned" || state=="accepted" {funding.status=state.into();}
            tx.execute("UPDATE funding SET status=?2,record=?3,checked=?4 WHERE tx_hash=?1",params![hash,funding.status,encode(&funding)?,now]).map_err(sql)?;
            let mut account=get_account(tx,id)?;
            let orphaned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM funding WHERE contract=?1 AND status='orphaned')",[id],|r|r.get(0)).map_err(sql)?;
            let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM funding WHERE contract=?1 AND status='pending')",[id],|r|r.get(0)).map_err(sql)?;
            account.status=if orphaned {"frozen"} else if pending {"settlement_pending"} else {"active"}.into();
            save_account(tx,&account)
        })
    }
    /// Pending obligations are grouped by platform wallet and sealed into
    /// immutable batches before chain submission. No per-funding best-effort
    /// transfer exists.
    ///
    /// Each wallet's pending total (exact, checked) and oldest acceptance time
    /// are evaluated over every pending obligation of that wallet, never a
    /// bounded subset. A wallet that meets either inclusive threshold, or every
    /// wallet when `force` is set, has every one of its pending obligations
    /// sealed in this transaction (inference-fee-remittance requirements 4 and
    /// 6). A wallet splits into several batches only where one more member
    /// would overflow a `u128` transfer value; there is no per-obligation work
    /// bound. Rows are streamed in bounded chunks, so memory stays bounded by
    /// the number of wallets, not obligations. Submission work per pass stays
    /// bounded: at most 100 unconfirmed batches are returned.
    pub fn prepare_fee_batches(&self, now: u64, force: bool) -> Result<Vec<FeeBatch>, String> {
        self.transaction(|tx| {
            // Decide for every wallet over all of its pending obligations before
            // sealing any of them.
            let mut wallets: std::collections::BTreeMap<String, (U256, u64)> =
                std::collections::BTreeMap::new();
            {
                let mut statement = tx.prepare("SELECT platform,amount,created FROM fee_obligations WHERE status='pending' AND batch IS NULL").map_err(sql)?;
                let mapped = statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,u64>(2)?))).map_err(sql)?;
                for row in mapped {
                    let (wallet,amount,created) = row.map_err(sql)?;
                    let entry = wallets.entry(wallet).or_insert((U256::zero(),created));
                    entry.0 = entry.0.checked_add(U256::from(money(&amount)?)).ok_or("fee aggregate overflow")?;
                    entry.1 = entry.1.min(created);
                }
            }
            for (wallet,(total,oldest)) in wallets {
                if total.is_zero() || (!force && total < U256::from(FEE_MIN_REMIT_WEI) && now.saturating_sub(oldest) < FEE_MAX_AGE_SECS) { continue; }
                seal_fee_wallet(tx,&wallet,total,now)?;
            }
            rows(tx,"SELECT record FROM fee_batches WHERE status!='accepted' ORDER BY rowid LIMIT 100",[])
        })
    }
    /// The newest 100 fee batches, newest first.
    pub fn fee_batches(&self) -> Result<Vec<FeeBatch>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        newest_fee_batches(&db)
    }
    /// At most 100 confirmed batches, least recently rechecked first: batches
    /// never rechecked come first, then the oldest time recorded by
    /// `mark_fee_batch_checked`, ties in sealing order. Because the order depends
    /// on when each batch was last checked, not on when the pass starts, every
    /// confirmed batch is rechecked within a bounded number of passes however far
    /// apart they start (inference-fee-remittance requirement 9). A recorded time
    /// later than `now` (the clock moved backwards) is not evidence of a recent
    /// recheck, so it sorts as never checked instead of starving the others.
    pub fn fee_batches_to_recheck(&self, now: u64) -> Result<Vec<FeeBatch>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        rows(
            &db,
            "SELECT b.record FROM fee_batches b LEFT JOIN metadata m ON m.key=?1||b.id
             WHERE b.status='accepted'
             ORDER BY CASE WHEN m.value IS NULL OR CAST(m.value AS INTEGER)>?2 THEN -1 ELSE CAST(m.value AS INTEGER) END,b.rowid
             LIMIT 100",
            params![FEE_BATCH_CHECKED, now],
        )
    }
    /// Records that maintenance rechecked a confirmed batch at `now`, whatever
    /// the outcome (accepted again, pending, orphaned, an unavailable RPC or a
    /// ledger refusal), so the oldest-checked scan moves on to the other batches.
    /// The time is kept in `metadata` beside the batch; the immutable sealed
    /// batch record is never rewritten for it, and the ledger schema is unchanged.
    pub fn mark_fee_batch_checked(&self, id: &str, now: u64) -> Result<(), String> {
        self.transaction(|tx| {
            let known: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM fee_batches WHERE id=?1)",
                    [id],
                    |r| r.get(0),
                )
                .map_err(sql)?;
            if !known {
                return Err("not_found: fee batch not found".into());
            }
            tx.execute(
                "INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![format!("{FEE_BATCH_CHECKED}{id}"), now.to_string()],
            )
            .map_err(sql)?;
            Ok(())
        })
    }
    pub fn update_fee_batch(&self, batch: &FeeBatch) -> Result<(), String> {
        if !matches!(
            batch.status.as_str(),
            "prepared" | "submitted" | "accepted" | "orphaned"
        ) {
            return Err("invalid fee outbox state".into());
        }
        if batch.status != "prepared"
            && (batch.tx_hash.is_none() || batch.raw_transaction.is_none())
        {
            return Err("fee submission lacks original transaction evidence".into());
        }
        if batch.status == "accepted" {
            let evidence = batch
                .evidence
                .as_ref()
                .ok_or("accepted fee lacks verified evidence")?;
            if evidence.network != self.network
                || Some(&evidence.tx_hash) != batch.tx_hash.as_ref()
                || Some(&evidence.raw_transaction) != batch.raw_transaction.as_ref()
                || evidence.to != batch.platform_wallet
                || evidence.value_wei != batch.amount_wei
                || evidence.data != batch.payment_data()?
            {
                return Err("fee evidence does not match immutable batch".into());
            }
        }
        self.transaction(|tx| {
            let previous: FeeBatch = decode(tx.query_row("SELECT record FROM fee_batches WHERE id=?1",[&batch.id],|r|r.get(0)).map_err(sql)?)?;
            if batch.status=="accepted" {
                let provider:String=tx.query_row("SELECT value FROM metadata WHERE key='provider'",[],|r|r.get(0)).map_err(sql)?;
                if batch.evidence.as_ref().map_or(true,|evidence|evidence.from!=provider) { return Err("fee evidence has the wrong sender".into()); }
            }
            if previous.amount_wei != batch.amount_wei || previous.platform_wallet != batch.platform_wallet
                || (previous.tx_hash.is_some() && previous.tx_hash != batch.tx_hash)
                || (previous.raw_transaction.is_some() && previous.raw_transaction != batch.raw_transaction)
            { return Err("fee outbox immutable batch mismatch".into()); }
            tx.execute("UPDATE fee_batches SET record=?2,status=?3 WHERE id=?1",params![batch.id,encode(batch)?,batch.status]).map_err(sql)?;
            if batch.status == "accepted" {
                tx.execute("UPDATE fee_obligations SET status=CASE WHEN status='orphaned_batched' THEN 'orphaned_remitted' ELSE 'remitted' END WHERE batch=?1 AND status IN ('batched','orphaned_batched')",[&batch.id]).map_err(sql)?;
            } else if previous.status == "accepted" {
                tx.execute("UPDATE fee_obligations SET status=CASE WHEN status='orphaned_remitted' THEN 'orphaned_batched' ELSE 'batched' END WHERE batch=?1 AND status IN ('remitted','orphaned_remitted')",[&batch.id]).map_err(sql)?;
            }
            Ok(())
        })
    }
    pub fn list_accounts(&self) -> Result<Vec<AccountView>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        rows(
            &db,
            "SELECT account FROM accounts ORDER BY created DESC,rowid DESC LIMIT 100",
            [],
        )
    }
    pub fn list_requests(&self) -> Result<Vec<RequestView>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        rows(
            &db,
            "SELECT record FROM requests ORDER BY created DESC,rowid DESC LIMIT 100",
            [],
        )
    }
    pub fn admin_summary(&self) -> Result<Value, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        let accounts: u64 = db
            .query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))
            .map_err(sql)?;
        let active_requests: u64 = db
            .query_row(
                "SELECT COUNT(*) FROM requests WHERE status='reserved'",
                [],
                |r| r.get(0),
            )
            .map_err(sql)?;
        let requests: u64 = db
            .query_row("SELECT COUNT(*) FROM requests", [], |r| r.get(0))
            .map_err(sql)?;
        let frozen_accounts: u64 = db.query_row("SELECT COUNT(*) FROM accounts WHERE json_extract(account,'$.status') IN ('frozen','settlement_pending')",[],|r|r.get(0)).map_err(sql)?;
        let pending_funding_intents: u64 = db.query_row("SELECT COUNT(*) FROM funding_intents WHERE NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract)",[],|r|r.get(0)).map_err(sql)?;
        let active_funding_intents:u64=db.query_row("SELECT COUNT(*) FROM funding_intents WHERE admission_until>?1 AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract)",[protocol::now()?],|r|r.get(0)).map_err(sql)?;
        Ok(
            json!({"accounts":accounts,"active_requests":active_requests,"requests":requests,"frozen_accounts":frozen_accounts,"pending_funding_intents":pending_funding_intents,"active_funding_intents":active_funding_intents,"max_pending_funding_intents":MAX_PENDING,"settlement_policy":"canonical-successful-receipt-at-espace-safe-v1","unknown_usage_policy":"release-hold-provider-loss","fee_min_remit_wei":FEE_MIN_REMIT_WEI.to_string(),"fee_max_age_secs":FEE_MAX_AGE_SECS}),
        )
    }
    /// The operator fee view (inference-fee-remittance requirements 3, 11 and
    /// 12), returned by `GET /admin/fees` and `POST /admin/fees/remit`.
    ///
    /// `amounts_wei` totals ordinary obligations in the book's four states,
    /// derived from each obligation's stored status and its batch's status
    /// (design decision 4); `cancelled_wei` and `orphaned_amounts_wei` (by batch
    /// state) are kept out of those totals. Every total is an exact decimal wei
    /// string, zero included. `batches` lists the newest 100 remittance batches
    /// without their signed bytes. The totals and the batch list are read in one
    /// ledger transaction, so they describe the same state. Stored statuses are
    /// not renamed: funding orphan restoration depends on them.
    pub fn fee_summary(&self) -> Result<Value, String> {
        let mut db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        let tx = db.transaction().map_err(sql)?;
        let zero = |states: &[&'static str]| -> std::collections::BTreeMap<&'static str, U256> {
            states.iter().map(|state| (*state, U256::zero())).collect()
        };
        let mut ordinary = zero(&["pending", "batched", "submitted", "confirmed"]);
        let mut orphaned = zero(&["batched", "submitted", "confirmed"]);
        let mut cancelled = U256::zero();
        {
            let mut statement = tx
                .prepare("SELECT o.status,o.amount,o.batch IS NOT NULL,b.status FROM fee_obligations o LEFT JOIN fee_batches b ON b.id=o.batch")
                .map_err(sql)?;
            let mapped = statement
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, bool>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                })
                .map_err(sql)?;
            for row in mapped {
                let (status, amount, sealed, batch) = row.map_err(sql)?;
                let batch = match (sealed, batch) {
                    (false, _) => None,
                    (true, Some(batch)) => Some(batch),
                    (true, None) => {
                        return Err("ledger invariant: fee obligation references a missing batch".into())
                    }
                };
                let total = match reported_fee_obligation(&status, batch.as_deref())? {
                    ReportedFee::Ordinary(state) => ordinary.get_mut(state),
                    ReportedFee::Orphaned(state) => orphaned.get_mut(state),
                    ReportedFee::Cancelled => Some(&mut cancelled),
                }
                .ok_or("ledger invariant: unreported fee obligation state")?;
                *total = total
                    .checked_add(U256::from(money(&amount)?))
                    .ok_or("fee aggregate overflow")?;
            }
        }
        let batches = newest_fee_batches(&tx)?
            .iter()
            .map(fee_batch_view)
            .collect::<Result<Vec<_>, _>>()?;
        let decimal = |totals: std::collections::BTreeMap<&'static str, U256>| -> std::collections::BTreeMap<&'static str, String> {
            totals.into_iter().map(|(state, total)| (state, total.to_string())).collect()
        };
        Ok(
            json!({"amounts_wei":decimal(ordinary),"cancelled_wei":cancelled.to_string(),"orphaned_amounts_wei":decimal(orphaned),"batches":batches,"minimum_remittance_wei":FEE_MIN_REMIT_WEI.to_string(),"maximum_age_secs":FEE_MAX_AGE_SECS,"aggregation":"per-platform-wallet; floor50bps-per-funding; amount-threshold-or-age; gas-paid-by-provider","collection":"provider-remitted-not-trustless","platform_receipt":FEE_RECEIPT_STATEMENT}),
        )
    }
}

/// Book line 528 and inference-fee-remittance requirement 12.
const FEE_RECEIPT_STATEMENT: &str =
    "A recorded fee obligation is not proof that the platform received the fee.";

/// One remittance batch as the operator fee view lists it (inference-fee-
/// remittance requirement 11). It never carries the signed transaction bytes.
/// Inclusion evidence appears only while the batch is confirmed: evidence kept
/// from a remittance that monitoring no longer confirms is not inclusion.
#[derive(Debug, Clone, Serialize)]
pub struct FeeBatchView {
    pub id: String,
    pub platform_wallet: String,
    pub amount_wei: String,
    pub state: &'static str,
    pub tx_hash: Option<String>,
    pub inclusion_block_hash: Option<String>,
    pub inclusion_block_number: Option<u64>,
    pub created_at: u64,
}

fn fee_batch_view(batch: &FeeBatch) -> Result<FeeBatchView, String> {
    let state = reported_fee_batch_state(&batch.status)?;
    let inclusion = match state {
        "confirmed" => Some(
            batch
                .evidence
                .as_ref()
                .ok_or("ledger invariant: confirmed fee batch lacks inclusion evidence")?,
        ),
        _ => None,
    };
    Ok(FeeBatchView {
        id: batch.id.clone(),
        platform_wallet: batch.platform_wallet.clone(),
        amount_wei: batch.amount_wei.clone(),
        state,
        tx_hash: batch.tx_hash.clone(),
        inclusion_block_hash: inclusion.map(|evidence| evidence.block_hash.clone()),
        inclusion_block_number: inclusion.map(|evidence| evidence.block_number),
        created_at: batch.created_at,
    })
}

fn newest_fee_batches(db: &Connection) -> Result<Vec<FeeBatch>, String> {
    rows(
        db,
        "SELECT record FROM fee_batches ORDER BY rowid DESC LIMIT 100",
        [],
    )
}

/// The reported state of a stored batch status (inference-fee-remittance design
/// decision 4). A batch whose remittance is not recorded is batched. A recorded
/// remittance is submitted until confirmed, including after monitoring proved a
/// confirmed remittance orphaned (`orphaned`), because the batch keeps its
/// signed bytes. An `accepted` batch is confirmed.
fn reported_fee_batch_state(status: &str) -> Result<&'static str, String> {
    match status {
        "prepared" => Ok("batched"),
        "submitted" | "orphaned" => Ok("submitted"),
        "accepted" => Ok("confirmed"),
        _ => Err("ledger invariant: unknown fee batch state".into()),
    }
}

/// Where the fee view reports one obligation.
enum ReportedFee {
    Ordinary(&'static str),
    Cancelled,
    /// Of orphaned funding, by its batch's reported state.
    Orphaned(&'static str),
}

/// Maps a stored obligation status and its batch's stored status (`None` when
/// unsealed) to the reported state (design decision 4). A combination that the
/// ledger's transitions never produce is refused instead of being reported
/// under a guessed state.
fn reported_fee_obligation(status: &str, batch: Option<&str>) -> Result<ReportedFee, String> {
    let sealed = |confirmed: bool| match batch {
        Some("accepted") if confirmed => Ok("confirmed"),
        Some(state @ ("prepared" | "submitted" | "orphaned")) if !confirmed => {
            reported_fee_batch_state(state)
        }
        _ => Err("ledger invariant: fee obligation state disagrees with its batch".to_string()),
    };
    match (status, batch) {
        ("pending", None) => Ok(ReportedFee::Ordinary("pending")),
        ("cancelled", None) => Ok(ReportedFee::Cancelled),
        ("batched", Some(_)) => sealed(false).map(ReportedFee::Ordinary),
        ("remitted", Some(_)) => sealed(true).map(ReportedFee::Ordinary),
        ("orphaned_batched", Some(_)) => sealed(false).map(ReportedFee::Orphaned),
        ("orphaned_remitted", Some(_)) => sealed(true).map(ReportedFee::Orphaned),
        _ => Err("ledger invariant: fee obligation state disagrees with its batch".into()),
    }
}

/// Seals every pending obligation of one platform wallet, oldest first, into
/// batches that each pay at most `u128::MAX` wei; a new batch starts only where
/// one more member would overflow that value. Rows are read in bounded keyset
/// chunks in `fee_pending` index order, and each chunk is read completely before
/// its rows are updated. The sealed members must sum exactly to `total`, the
/// pending amount on which the caller decided, or the transaction fails.
fn seal_fee_wallet(tx: &Transaction<'_>, wallet: &str, total: U256, now: u64) -> Result<(), String> {
    let mut statement = tx.prepare("SELECT tx_hash,amount,created FROM fee_obligations WHERE status='pending' AND batch IS NULL AND platform=?1 AND (created,tx_hash)>(?2,?3) ORDER BY created,tx_hash LIMIT ?4").map_err(sql)?;
    let mut cursor: (i64, String) = (-1, String::new());
    let mut open: Option<(String, u128)> = None;
    let mut sealed = U256::zero();
    loop {
        let chunk = statement
            .query_map(params![wallet, cursor.0, cursor.1, FEE_SEAL_CHUNK], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        let Some((hash, _, created)) = chunk.last() else { break };
        cursor = (*created, hash.clone());
        for (hash, amount, _) in chunk {
            let amount = money(&amount)?;
            if let Some((id, sum)) = open.take_if(|(_, sum)| sum.checked_add(amount).is_none()) {
                insert_fee_batch(tx, id, wallet, sum, now)?;
            }
            let (id, sum) = open.get_or_insert_with(|| (protocol::random_id(), 0));
            *sum = add(*sum, amount)?;
            if tx.execute("UPDATE fee_obligations SET batch=?2,status='batched' WHERE tx_hash=?1 AND status='pending' AND batch IS NULL",params![hash,id.as_str()]).map_err(sql)? != 1 {
                return Err("ledger invariant: fee obligation changed while sealing".into());
            }
            sealed = sealed.checked_add(U256::from(amount)).ok_or("fee aggregate overflow")?;
        }
    }
    if let Some((id, sum)) = open {
        insert_fee_batch(tx, id, wallet, sum, now)?;
    }
    if sealed != total {
        return Err("ledger invariant: sealed fees differ from the pending total".into());
    }
    Ok(())
}
fn insert_fee_batch(tx: &Transaction<'_>, id: String, wallet: &str, amount: u128, now: u64) -> Result<(), String> {
    let batch = FeeBatch {
        id,
        platform_wallet: wallet.into(),
        amount_wei: amount.to_string(),
        status: "prepared".into(),
        tx_hash: None,
        raw_transaction: None,
        evidence: None,
        created_at: now,
    };
    tx.execute(
        "INSERT INTO fee_batches(id,status,record) VALUES(?1,'prepared',?2)",
        params![batch.id, encode(&batch)?],
    )
    .map_err(sql)?;
    Ok(())
}

fn raw_hash(raw: &str) -> Result<String, String> {
    if raw.len() > 65_538 {
        return Err("invalid_payment: transaction too large".into());
    }
    let bytes = hex::decode(
        raw.strip_prefix("0x")
            .ok_or("invalid_payment: raw transaction must be hex")?,
    )
    .map_err(|_| "invalid_payment: malformed raw transaction")?;
    if bytes.is_empty() {
        return Err("invalid_payment: empty transaction".into());
    }
    Ok(protocol::hash_hex(&bytes))
}
fn require_funding_intent(
    db: &Connection,
    quote: &Quote,
    raw: &str,
    hash: &str,
) -> Result<(), String> {
    let stored: Option<(String, String, String)> = db
        .query_row(
            "SELECT raw_transaction,tx_hash,ack FROM funding_intents WHERE contract=?1",
            [&quote.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(sql)?;
    let (registered_raw, registered_hash, ack) =
        stored.ok_or("funding_intent_required: register signed funding before broadcasting")?;
    if registered_raw != raw || registered_hash != hash || raw_hash(raw)? != hash {
        return Err("conflict: funding transaction differs from the registered intent".into());
    }
    let ack: Signed<FundingIntent> = decode(ack)?;
    if ack.payload.network != quote.network
        || ack.payload.provider_wallet != quote.provider_wallet
        || ack.payload.consumer_wallet != quote.consumer_wallet
        || ack.payload.quote_id != quote.id
        || ack.payload.tx_hash != hash
        || ack.payload.registered_at >= quote.expires_at
    {
        return Err("invalid_payment: registered intent does not match immutable quote".into());
    }
    Ok(())
}
impl BillingStore {
    /// Called only after chain-envelope and current-spendability admission. Persist
    /// exact terms forever; admission expiry releases quota, never the agreement.
    pub fn register_funding_intent(
        &self,
        quote: &Signed<Quote>,
        owner: &str,
        raw: &str,
        hash: &str,
        nonce: u64,
        key: &str,
        clock: impl FnOnce() -> Result<u64, String>,
        accepting: bool,
        execution_model: &ModelSpec,
    ) -> Result<Signed<FundingIntent>, String> {
        let q = &quote.payload;
        if q.network != self.network {
            return Err("wrong_network: quote belongs to another network".into());
        }
        if owner != q.consumer_wallet {
            return Err("forbidden: quote belongs to another wallet".into());
        }
        if q.funding_wei == "0" {
            return Err("invalid_payment: free sessions do not accept funding intents".into());
        }
        if raw_hash(raw)? != hash {
            return Err("invalid_payment: transaction hash does not match signed bytes".into());
        }
        self.bind_provider(&q.provider_wallet)?;
        self.transaction(|tx| {
            let previous:Option<String>=tx.query_row("SELECT ack FROM funding_intents WHERE contract=?1",[&q.id],|r|r.get(0)).optional().map_err(sql)?;
            if let Some(previous)=previous {
                require_funding_intent(tx,q,raw,hash)?;
                return decode(previous);
            }
            // Sample time only after obtaining the write transaction: a request
            // queued behind another writer must not backdate its acceptance.
            let now=clock()?;
            if now < q.issued_at { return Err("invalid_quote: funding intent cannot precede quote issuance".into()); }
            if q.expires_at <= now { return Err("expired_quote: funding intent must be registered before quote expiry".into()); }
            if !accepting { return Err("provider_draining: provider is not accepting new funding".into()); }
            require_execution(tx, &q.model, execution_model)?;
            let stored:Option<String>=tx.query_row("SELECT record FROM quotes WHERE id=?1",[&q.id],|r|r.get(0)).optional().map_err(sql)?;
            if stored.as_deref() != Some(encode(quote)?.as_str()) { return Err("invalid_quote: quote was not issued by this provider".into()); }
            let used:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM funding_intents WHERE tx_hash=?1 UNION ALL SELECT 1 FROM funding WHERE tx_hash=?1)",[hash],|r|r.get(0)).map_err(sql)?;
            if used { return Err("funding_replay: transaction belongs to another agreement".into()); }
            let committed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM funding_intents WHERE sender=?1 AND nonce=?2)",params![owner,nonce.to_string()],|r|r.get(0)).map_err(sql)?;
            if committed {return Err("conflict: sender nonce already has an acknowledged funding agreement".into());}
            let pending:i64=tx.query_row("SELECT COUNT(*) FROM funding_intents WHERE admission_until>?1 AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract)",[now],|r|r.get(0)).map_err(sql)?;
            if pending >= MAX_PENDING { return Err("rate_limit: active funding intent capacity reached".into()); }
            #[cfg(test)]
            self.pause_for_test(AdmissionPoint::PaidLease);
            let ack=protocol::sign(protocol::INTENT_DOMAIN,FundingIntent{network:q.network.clone(),provider_wallet:q.provider_wallet.clone(),consumer_wallet:q.consumer_wallet.clone(),quote_id:q.id.clone(),tx_hash:hash.into(),registered_at:now},key)?;
            protocol::verify(protocol::INTENT_DOMAIN,&ack,&q.provider_wallet)?;
            tx.execute("INSERT INTO funding_intents(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![q.id,hash,raw,encode(&ack)?,encode(quote)?,owner,nonce.to_string(),q.expires_at]).map_err(sql)?;
            tx.execute("DELETE FROM quotes WHERE id=?1",[&q.id]).map_err(sql)?;
            Ok(ack)
        })
    }

    /// Read an exact acknowledgement without requiring current spendability,
    /// accepting status, or an unexpired admission lease.
    pub fn funding_intent(
        &self,
        quote: &Quote,
        raw: &str,
    ) -> Result<Option<Signed<FundingIntent>>, String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        let ack: Option<String> = db
            .query_row(
                "SELECT ack FROM funding_intents WHERE contract=?1",
                [&quote.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        if ack.is_some() {
            require_funding_intent(&db, quote, raw, &raw_hash(raw)?)?;
        }
        ack.map(decode).transpose()
    }

    pub fn check_open_funding(
        &self,
        quote: &Quote,
        raw: Option<&str>,
        now: u64,
    ) -> Result<(), String> {
        let db = self.db.lock().map_err(|_| "ledger lock poisoned")?;
        if quote.funding_wei == "0" {
            if raw.is_some() {
                return Err("invalid_payment: free sessions do not accept a transaction".into());
            }
            let exists: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
                    [&quote.id],
                    |r| r.get(0),
                )
                .map_err(sql)?;
            if !exists && quote.expires_at <= now {
                return Err("expired_quote: quote expired".into());
            }
            return Ok(());
        }
        let raw = raw.ok_or(
            "funding_intent_required: original registered funding transaction bytes are required",
        )?;
        require_funding_intent(&db, quote, raw, &raw_hash(raw)?)
    }
}

#[cfg(test)]
#[path = "billing_store_tests.rs"]
mod tests;
