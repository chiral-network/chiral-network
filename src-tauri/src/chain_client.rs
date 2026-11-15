//! Native CFX payments on Conflux eSpace, with signed evidence and a durable sender journal.
//! Funding requires canonical successful execution at `safe`; PoS `finalized` is distinct.
use crate::file_lock::ExclusiveFileLock;
use crate::{
    network::{normalize_address, validate_hash, NetworkConfig},
    protocol::{self, NetworkIdentity},
    rpc_client, wallet,
};
use primitive_types::U256;
use rlp::{Rlp, RlpStream};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    path::PathBuf,
    sync::{Arc, Mutex},
};

// Public free eSpace endpoints limit block/transaction reads to fewer than five per second.
// Serialize these reads per adapter, including proxy calls, without sharing the submission budget.
const CHAIN_READ_INTERVAL: std::time::Duration = std::time::Duration::from_millis(300);
const MAX_HEAD_AGE_SECONDS: u64 = 120;
const MAX_SAFE_AGE_SECONDS: u64 = 300;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct BlockObservation {
    height: u64,
    hash: String,
    timestamp: u64,
}
impl BlockObservation {
    fn parse(value: &Value) -> Result<Self, ChainError> {
        let hash = value["hash"]
            .as_str()
            .ok_or_else(|| unavailable("RPC block hash missing"))?;
        validate_hash(hash).map_err(unavailable)?;
        Ok(Self {
            height: quantity64(&value["number"])?,
            hash: hash.to_ascii_lowercase(),
            timestamp: quantity64(&value["timestamp"])?,
        })
    }
}
struct SettlementView {
    head: BlockObservation,
    safe: BlockObservation,
}
#[derive(PartialEq, Eq)]
struct ExecutionReceipt {
    hash: String,
    height: u64,
    status: u64,
}
struct ExecutionObservation {
    receipt: Option<ExecutionReceipt>,
    canonical: BlockObservation,
}

pub const MAX_RAW_BYTES: usize = 16 * 1024;
pub const MAX_PAYMENT_DATA: usize = 4096;
pub const MAX_ESTIMATE_GAS: u64 = 1_000_000;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    Pending(String),
    Rejected(String),
    Unavailable(String),
}
impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending(s) => write!(f, "pending: {s}"),
            Self::Rejected(s) => write!(f, "rejected: {s}"),
            Self::Unavailable(s) => write!(f, "unavailable: {s}"),
        }
    }
}
impl std::error::Error for ChainError {}
fn rejected(error: impl ToString) -> ChainError {
    ChainError::Rejected(error.to_string())
}
fn unavailable(error: impl ToString) -> ChainError {
    ChainError::Unavailable(error.to_string())
}
impl From<rpc_client::RpcError> for ChainError {
    fn from(error: rpc_client::RpcError) -> Self {
        unavailable(error)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedPayment {
    pub network: NetworkIdentity,
    pub tx_hash: String,
    pub from: String,
    pub to: String,
    pub value_wei: String,
    pub data: Vec<u8>,
    pub block_hash: String,
    pub block_number: u64,
    pub raw_transaction: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaymentState {
    Accepted(VerifiedPayment),
    Pending,
    Orphaned,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmittedTransaction {
    pub tx_hash: String,
    pub raw_transaction: String,
    pub status: String,
}
/// Explicit recovery observation, never a refund or an irreversible-finality promise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentCancellation {
    pub operation_id: String,
    pub original_tx_hash: String,
    pub replacement_tx_hash: Option<String>,
    pub status: String,
    pub gas_price_wei: Option<String>,
    pub maximum_gas_cost_wei: Option<String>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPaymentEnvelope {
    pub tx_hash: String,
    pub from: String,
    pub to: String,
    pub value_wei: String,
    pub data: Vec<u8>,
    pub nonce: u64,
}
#[derive(Debug, Clone)]
struct Decoded {
    tx_hash: String,
    from: String,
    to: String,
    value: u128,
    data: Vec<u8>,
    nonce: u64,
    kind: u8,
    gas: u64,
    max_fee: u128,
}
impl Decoded {
    fn envelope(self) -> ValidatedPaymentEnvelope {
        ValidatedPaymentEnvelope {
            tx_hash: self.tx_hash,
            from: self.from,
            to: self.to,
            value_wei: self.value.to_string(),
            data: self.data,
            nonce: self.nonce,
        }
    }
}
fn quantity(value: &Value) -> Result<u128, ChainError> {
    rpc_client::hex_to_u128(
        value
            .as_str()
            .ok_or_else(|| unavailable("RPC quantity is not a string"))?,
    )
    .map_err(unavailable)
}
fn quantity64(value: &Value) -> Result<u64, ChainError> {
    u64::try_from(quantity(value)?).map_err(|_| unavailable("RPC quantity exceeds u64"))
}
fn wei(value: &str) -> Result<u128, ChainError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(rejected("wei must be a canonical unsigned decimal integer"));
    }
    value.parse().map_err(|_| rejected("wei exceeds u128"))
}
fn canonical_rlp(rlp: &Rlp<'_>, depth: u8) -> Result<(), String> {
    if depth > 4 {
        return Err("transaction RLP nesting exceeds limit".into());
    }
    let raw = rlp.as_raw();
    let prefix = *raw.first().ok_or("empty RLP item")?;
    let (header, length) = match prefix {
        0..=0x7f => (0, 1),
        0x80..=0xb7 => (1, usize::from(prefix - 0x80)),
        0xc0..=0xf7 => (1, usize::from(prefix - 0xc0)),
        _ => {
            let width = usize::from(if prefix < 0xc0 {
                prefix - 0xb7
            } else {
                prefix - 0xf7
            });
            if raw.len() <= width || raw[1] == 0 {
                return Err("non-canonical RLP length".into());
            }
            let length = raw[1..=width]
                .iter()
                .try_fold(0usize, |n, b| {
                    n.checked_mul(256)
                        .and_then(|n| n.checked_add(usize::from(*b)))
                })
                .ok_or("RLP length overflow")?;
            if length < 56 {
                return Err("non-canonical long RLP item".into());
            }
            (1 + width, length)
        }
    };
    if header.checked_add(length) != Some(raw.len()) {
        return Err("truncated or trailing RLP bytes".into());
    }
    if prefix == 0x81 && raw[1] < 0x80 {
        return Err("non-canonical single-byte RLP item".into());
    }
    if prefix >= 0xc0 {
        let count = rlp.item_count().map_err(|_| "invalid RLP list")?;
        if count > 1024 {
            return Err("RLP item limit".into());
        }
        let mut consumed = header;
        for i in 0..count {
            let item = rlp.at(i).map_err(|_| "invalid RLP item")?;
            canonical_rlp(&item, depth + 1)?;
            consumed += item.as_raw().len();
        }
        if consumed != raw.len() {
            return Err("invalid RLP list payload".into());
        }
    }
    Ok(())
}
fn field_bytes<'a>(rlp: &Rlp<'a>, index: usize) -> Result<&'a [u8], String> {
    rlp.at(index)
        .map_err(|_| "missing transaction field")?
        .data()
        .map_err(|_| "transaction field must be bytes".into())
}
fn uint(rlp: &Rlp<'_>, index: usize, max: usize) -> Result<U256, String> {
    let bytes = field_bytes(rlp, index)?;
    if bytes.len() > max || bytes.first() == Some(&0) {
        return Err("non-canonical or oversized transaction integer".into());
    }
    Ok(U256::from_big_endian(bytes))
}
fn uint64(rlp: &Rlp<'_>, index: usize) -> Result<u64, String> {
    Ok(uint(rlp, index, 8)?.low_u64())
}
fn intrinsic(data: &[u8], addresses: u64, slots: u64) -> Result<u64, String> {
    let bytes = data.iter().try_fold(21000u64, |gas, b| {
        gas.checked_add(if *b == 0 { 4 } else { 16 })
            .ok_or("intrinsic gas overflow")
    })?;
    bytes
        .checked_add(
            addresses
                .checked_mul(2400)
                .ok_or("access-list gas overflow")?,
        )
        .and_then(|n| n.checked_add(slots.checked_mul(1900)?))
        .ok_or_else(|| "intrinsic gas overflow".into())
}
fn access_list(rlp: &Rlp<'_>, index: usize) -> Result<(u64, u64), String> {
    let list = rlp.at(index).map_err(|_| "missing access list")?;
    if !list.is_list() {
        return Err("access list must be a list".into());
    }
    let count = list.item_count().map_err(|_| "invalid access list")?;
    if count > 64 {
        return Err("access list address limit".into());
    }
    let mut slots = 0usize;
    for i in 0..count {
        let entry = list.at(i).map_err(|_| "invalid access list entry")?;
        if !entry.is_list()
            || entry
                .item_count()
                .map_err(|_| "invalid access list entry")?
                != 2
            || field_bytes(&entry, 0)?.len() != 20
        {
            return Err("invalid access list address".into());
        }
        let keys = entry.at(1).map_err(|_| "invalid storage keys")?;
        if !keys.is_list() {
            return Err("storage keys must be a list".into());
        }
        let count = keys.item_count().map_err(|_| "invalid storage keys")?;
        slots += count;
        if slots > 256 {
            return Err("access list storage-key limit".into());
        }
        for j in 0..count {
            if field_bytes(&keys, j)?.len() != 32 {
                return Err("storage key must be 32 bytes".into());
            }
        }
    }
    Ok((count as u64, slots as u64))
}
fn decode(raw: &str, chain_id: u64) -> Result<Decoded, String> {
    let encoded = raw
        .strip_prefix("0x")
        .ok_or("raw transaction must be 0x-prefixed")?;
    if encoded.is_empty() || encoded.len() > MAX_RAW_BYTES * 2 || encoded.len() % 2 != 0 {
        return Err("raw transaction size is invalid".into());
    }
    let bytes = hex::decode(encoded).map_err(|_| "invalid raw transaction hex")?;
    let (kind, payload) = match bytes[0] {
        1 | 2 => (bytes[0], &bytes[1..]),
        0xc0..=0xff => (0, &bytes[..]),
        _ => return Err("unsupported transaction envelope".into()),
    };
    let rlp = Rlp::new(payload);
    canonical_rlp(&rlp, 0)?;
    let expected = match kind {
        0 => 9,
        1 => 11,
        _ => 12,
    };
    if !rlp.is_list() || rlp.item_count().map_err(|_| "invalid transaction fields")? != expected {
        return Err("wrong transaction field count".into());
    }
    let (nonce_i, gas_i, to_i, value_i, data_i, sig_i, unsigned, parity, max_fee) = if kind == 0 {
        let v = uint(&rlp, 6, 32)?;
        let base = U256::from(chain_id) * U256::from(2u8) + U256::from(35u8);
        if v != base && v != base + U256::one() {
            return Err("legacy transaction must use configured EIP-155 chain id".into());
        }
        let max_fee = uint(&rlp, 1, 16)?.low_u128();
        (0, 2, 3, 4, 5, 7, 6, (v - base).low_u32() as u8, max_fee)
    } else {
        if uint64(&rlp, 0)? != chain_id {
            return Err("transaction chain id mismatch".into());
        }
        let parity_i = if kind == 1 { 8 } else { 9 };
        let parity = uint64(&rlp, parity_i)?;
        if parity > 1 {
            return Err("invalid transaction signature parity".into());
        }
        if kind == 1 {
            let max_fee = uint(&rlp, 2, 16)?.low_u128();
            (1, 3, 4, 5, 6, 9, 8, parity as u8, max_fee)
        } else {
            let max_fee = uint(&rlp, 3, 16)?;
            if uint(&rlp, 2, 16)? > max_fee {
                return Err("priority fee exceeds maximum fee".into());
            }
            (1, 4, 5, 6, 7, 10, 9, parity as u8, max_fee.low_u128())
        }
    };
    let to = field_bytes(&rlp, to_i)?;
    if to.len() != 20 {
        return Err("payment recipient must be exactly 20 bytes (no contract creation)".into());
    }
    let value = uint(&rlp, value_i, 16)?.low_u128();
    let data = field_bytes(&rlp, data_i)?;
    if data.len() > MAX_PAYMENT_DATA {
        return Err("payment commitment exceeds size limit".into());
    }
    let (addresses, slots) = if kind == 0 {
        (0, 0)
    } else {
        access_list(&rlp, data_i + 1)?
    };
    let gas = uint64(&rlp, gas_i)?;
    if gas < intrinsic(data, addresses, slots)? {
        return Err("gas is below the signed transaction's structural minimum".into());
    }
    let mut signing = RlpStream::new_list(if kind == 0 { 9 } else { unsigned });
    for i in 0..unsigned {
        signing.append_raw(rlp.at(i).map_err(|_| "missing signed field")?.as_raw(), 1);
    }
    if kind == 0 {
        signing.append(&chain_id);
        signing.append(&0u8);
        signing.append(&0u8);
    }
    let signing = signing.out();
    let digest = if kind == 0 {
        protocol::digest(&signing)
    } else {
        let mut bytes = Vec::with_capacity(signing.len() + 1);
        bytes.push(kind);
        bytes.extend_from_slice(&signing);
        protocol::digest(&bytes)
    };
    let mut signature = [0u8; 64];
    uint(&rlp, sig_i, 32)?.to_big_endian(&mut signature[..32]);
    uint(&rlp, sig_i + 1, 32)?.to_big_endian(&mut signature[32..]);
    let from = wallet::recover_digest(digest, &signature, parity)?;
    Ok(Decoded {
        tx_hash: protocol::hash_hex(&bytes),
        from,
        to: format!("0x{}", hex::encode(to)),
        value,
        data: data.to_vec(),
        nonce: uint64(&rlp, nonce_i)?,
        kind,
        gas,
        max_fee,
    })
}
#[cfg(test)]
pub(crate) fn payment_sender_nonce(raw: &str, chain_id: u64) -> Result<(String, u64), String> {
    let tx = decode(raw, chain_id)?;
    Ok((tx.from, tx.nonce))
}
fn append_uint(stream: &mut RlpStream, value: U256) {
    let mut bytes = [0; 32];
    value.to_big_endian(&mut bytes);
    let start = bytes.iter().position(|b| *b != 0).unwrap_or(32);
    stream.append(&&bytes[start..]);
}
pub(crate) fn sign_transaction(
    key: &str,
    chain_id: u64,
    nonce: u64,
    to: &str,
    value: u128,
    data: &[u8],
    gas_price: u128,
    gas: u64,
) -> Result<SubmittedTransaction, String> {
    if data.len() > MAX_PAYMENT_DATA {
        return Err("payment commitment exceeds size limit".into());
    }
    let to = hex::decode(&normalize_address(to)?[2..]).map_err(|_| "invalid recipient")?;
    if gas < intrinsic(data, 0, 0)? {
        return Err("gas limit is below the transaction's structural minimum".into());
    }
    let fields = |stream: &mut RlpStream| {
        stream.append(&nonce);
        append_uint(stream, U256::from(gas_price));
        stream.append(&gas);
        stream.append(&to);
        append_uint(stream, U256::from(value));
        stream.append(&data);
    };
    let mut unsigned = RlpStream::new_list(9);
    fields(&mut unsigned);
    unsigned.append(&chain_id);
    unsigned.append(&0u8);
    unsigned.append(&0u8);
    let digest = protocol::digest(&unsigned.out());
    let (id, signature) = secp256k1::Secp256k1::new()
        .sign_ecdsa_recoverable(
            &secp256k1::Message::from_digest(digest),
            &wallet::secret(key)?,
        )
        .serialize_compact();
    if id.to_i32() > 1 {
        return Err("unsupported signature recovery parity; retry signing".into());
    }
    let mut signed = RlpStream::new_list(9);
    fields(&mut signed);
    append_uint(
        &mut signed,
        U256::from(chain_id) * U256::from(2u8) + U256::from(35 + id.to_i32() as u64),
    );
    append_uint(&mut signed, U256::from_big_endian(&signature[..32]));
    append_uint(&mut signed, U256::from_big_endian(&signature[32..]));
    let raw = signed.out();
    Ok(SubmittedTransaction {
        tx_hash: protocol::hash_hex(&raw),
        raw_transaction: format!("0x{}", hex::encode(raw)),
        status: "prepared".into(),
    })
}

pub struct ChainClient {
    config: NetworkConfig,
    dir: PathBuf,
    journal: Mutex<Connection>,
    senders: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    genesis_checked: tokio::sync::OnceCell<()>,
    chain_read_at: tokio::sync::Mutex<Option<tokio::time::Instant>>,
}
impl ChainClient {
    /// Every process spending the same key MUST share this configured base, independently of
    /// provider ledger paths. The actual journal is network.scoped_dir(base)/wallet-journal.
    pub fn new(config: NetworkConfig, journal_base: PathBuf) -> Result<Self, String> {
        config.validate()?;
        let dir = config.scoped_dir(&journal_base).join("wallet-journal");
        wallet::secure_directory(&dir)?;
        let path = dir.join("transactions.sqlite3");
        let mut journal_options = OpenOptions::new();
        journal_options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            journal_options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let journal_file = journal_options
            .open(&path)
            .map_err(|_| "cannot securely open wallet journal")?;
        if !journal_file
            .metadata()
            .map_err(|_| "cannot inspect wallet journal")?
            .is_file()
        {
            return Err("wallet journal must be a regular file".into());
        }
        let connection = Connection::open(path).map_err(|_| "cannot open wallet journal")?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "cannot configure wallet journal")?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS transactions (sender TEXT NOT NULL, operation TEXT NOT NULL, nonce TEXT NOT NULL, tx_hash TEXT NOT NULL UNIQUE, raw TEXT NOT NULL, status TEXT NOT NULL, error TEXT, evidence TEXT, created_at INTEGER NOT NULL, PRIMARY KEY(sender,operation));").map_err(|_|"cannot initialize wallet journal")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS cancellations (sender TEXT NOT NULL, operation TEXT NOT NULL, raw TEXT NOT NULL, status TEXT NOT NULL, error TEXT, evidence TEXT, PRIMARY KEY(sender,operation));").map_err(|_|"cannot initialize cancellation journal")?;
        // Existing duplicate assignments are retained for recovery, but no new original
        // operation may reuse any durable sender/nonce, including a terminal one.
        connection.execute_batch("CREATE INDEX IF NOT EXISTS transactions_sender_nonce ON transactions(sender,nonce);
            CREATE TRIGGER IF NOT EXISTS transactions_unique_original_nonce BEFORE INSERT ON transactions
            WHEN EXISTS(SELECT 1 FROM transactions WHERE sender=NEW.sender AND nonce=NEW.nonce)
            BEGIN SELECT RAISE(ABORT,'sender nonce already belongs to an original operation'); END;").map_err(|_|"cannot protect wallet nonce assignments")?;
        #[cfg(unix)]
        File::open(&dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| "cannot persist journal directory")?;
        Ok(Self {
            config,
            dir,
            journal: Mutex::new(connection),
            senders: Mutex::new(HashMap::new()),
            genesis_checked: tokio::sync::OnceCell::new(),
            chain_read_at: tokio::sync::Mutex::new(None),
        })
    }
    pub fn config(&self) -> &NetworkConfig {
        &self.config
    }
    pub fn identity(&self) -> &NetworkIdentity {
        &self.config.identity
    }
    async fn call(&self, method: &str, params: Value) -> Result<Value, ChainError> {
        if matches!(method, "eth_getBlockByNumber" | "eth_getTransactionReceipt") {
            let mut last = self.chain_read_at.lock().await;
            if let Some(last) = *last {
                tokio::time::sleep_until(last + CHAIN_READ_INTERVAL).await;
            }
            *last = Some(tokio::time::Instant::now());
        }
        Ok(rpc_client::call(&self.config, method, params).await?)
    }
    async fn block(&self, tag: &str) -> Result<BlockObservation, ChainError> {
        let block = BlockObservation::parse(
            &self.call("eth_getBlockByNumber", json!([tag, false])).await?,
        )?;
        if tag.starts_with("0x")
            && rpc_client::hex_to_u64(tag).map_err(unavailable)? != block.height
        {
            return Err(unavailable("RPC returned a different block height"));
        }
        Ok(block)
    }
    pub async fn identity_check(&self) -> Result<(), ChainError> {
        if quantity64(&self.call("eth_chainId", json!([])).await?)? != self.identity().chain_id {
            return Err(rejected("configured RPC chain id mismatch"));
        }
        // Immutable genesis is checked once for this fixed endpoint/configuration, not per receipt.
        // A restart establishes identity again; chain id remains checked for every operation.
        self.genesis_checked.get_or_try_init(|| async {
            let genesis = self.block("0x0").await?;
            if genesis.hash != self.identity().genesis_hash {
                return Err(rejected("configured RPC genesis mismatch"));
            }
            Ok(())
        }).await?;
        Ok(())
    }
    pub async fn balance(&self, address: &str) -> Result<String, ChainError> {
        let address = normalize_address(address).map_err(rejected)?;
        self.identity_check().await?;
        Ok(quantity(
            &rpc_client::call(&self.config, "eth_getBalance", json!([address, "latest"])).await?,
        )?
        .to_string())
    }
    pub async fn status(&self) -> Result<Value, ChainError> {
        let view = self.settlement_view().await?;
        let finalized = self.block("finalized").await?;
        Self::validate_frontier(&view.head, &finalized)?;
        Ok(json!({
            "chainId": self.identity().chain_id,
            "genesis": self.identity().genesis_hash,
            "observedAtUnix": protocol::now().map_err(unavailable)?,
            "syncing": false,
            "head": view.head,
            "safe": view.safe,
            "finalized": finalized,
            "acceptance": "safe"
        }))
    }
    fn validate_frontier(head: &BlockObservation, frontier: &BlockObservation) -> Result<(), ChainError> {
        if frontier.height > head.height
            || frontier.timestamp > head.timestamp
            || (frontier.height == head.height && frontier.hash != head.hash)
        {
            return Err(unavailable("RPC frontier contradicts the canonical head"));
        }
        Ok(())
    }
    async fn settlement_view(&self) -> Result<SettlementView, ChainError> {
        self.identity_check().await?;
        if self.call("eth_syncing", json!([])).await? != Value::Bool(false) {
            return Err(unavailable("eSpace node is syncing or sync health is unavailable"));
        }
        // Read safe before latest so normal head advancement does not appear contradictory.
        let safe = self.block("safe").await?;
        let head = self.block("latest").await?;
        Self::validate_frontier(&head, &safe)?;
        let now = protocol::now().map_err(unavailable)?;
        if now.abs_diff(head.timestamp) > MAX_HEAD_AGE_SECONDS
            || now.saturating_sub(safe.timestamp) > MAX_SAFE_AGE_SECONDS
        {
            return Err(unavailable("eSpace head or safe frontier is stale"));
        }
        Ok(SettlementView { head, safe })
    }
    async fn nonce_state(&self, sender: &str) -> Result<(u64, u64), ChainError> {
        let latest = quantity64(&self.call("eth_getTransactionCount", json!([sender, "latest"])).await?)?;
        let pending = quantity64(&self.call("eth_getTransactionCount", json!([sender, "pending"])).await?)?;
        if pending < latest {
            return Err(unavailable("pending account nonce precedes executed nonce"));
        }
        Ok((latest, pending))
    }
    async fn estimate_gas(
        &self,
        from: &str,
        to: &str,
        value: u128,
        data: &[u8],
        gas_price: u128,
    ) -> Result<u64, ChainError> {
        // eSpace gas accounting is not a London-intrinsic-gas promise. In particular,
        // native calldata estimates can exceed 21_000 + 4/16 per byte. Do not multiply
        // the estimate: eSpace may charge at least three quarters of the signed limit.
        let gas = quantity64(&self.call("eth_estimateGas", json!([{
            "from": from, "to": to, "value": format!("0x{value:x}"),
            "data": format!("0x{}", hex::encode(data)),
            "gasPrice": format!("0x{gas_price:x}"),
            "gas": format!("0x{MAX_ESTIMATE_GAS:x}")
        }])).await?)?;
        if gas < intrinsic(data, 0, 0).map_err(rejected)? || gas > MAX_ESTIMATE_GAS {
            return Err(unavailable("native payment gas estimate is outside supported bounds"));
        }
        Ok(gas)
    }
    async fn receipt(&self, hash: &str) -> Result<Value, ChainError> {
        self.call("eth_getTransactionReceipt", json!([hash])).await
    }
    fn execution_receipt_fields(
        &self,
        receipt: &Value,
        tx: &Decoded,
    ) -> Result<ExecutionReceipt, ChainError> {
        if receipt.is_null() {
            return Err(ChainError::Pending("canonical receipt unavailable".into()));
        }
        for (field, expected) in [
            ("transactionHash", &tx.tx_hash),
            ("from", &tx.from),
            ("to", &tx.to),
        ] {
            if !receipt[field]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(expected))
            {
                return Err(rejected("receipt does not match signed transaction"));
            }
        }
        if quantity64(&receipt["type"])? != u64::from(tx.kind)
            || quantity64(&receipt["gasUsed"])? > tx.gas
        {
            return Err(rejected(
                "receipt type/gas does not match signed transaction",
            ));
        }
        let status = quantity64(&receipt["status"])?;
        if status > 1 {
            return Err(unavailable("receipt has an invalid execution status"));
        }
        let hash = receipt["blockHash"]
            .as_str()
            .ok_or_else(|| unavailable("receipt missing block hash"))?;
        validate_hash(hash).map_err(unavailable)?;
        Ok(ExecutionReceipt {
            hash: hash.to_ascii_lowercase(),
            height: quantity64(&receipt["blockNumber"])?,
            status,
        })
    }
    /// Validate signed payment terms without consulting a node or observing settlement.
    pub fn validate_payment_envelope(
        &self,
        raw: &str,
        from: &str,
        to: &str,
        value_wei: &str,
        data: &[u8],
    ) -> Result<ValidatedPaymentEnvelope, ChainError> {
        Ok(self
            .decoded_payment(raw, from, to, value_wei, data)?
            .envelope())
    }
    fn decoded_payment(
        &self,
        raw: &str,
        from: &str,
        to: &str,
        value_wei: &str,
        data: &[u8],
    ) -> Result<Decoded, ChainError> {
        let tx = decode(raw, self.identity().chain_id).map_err(rejected)?;
        if tx.from != normalize_address(from).map_err(rejected)?
            || tx.to != normalize_address(to).map_err(rejected)?
            || tx.value != wei(value_wei)?
            || tx.data != data
        {
            return Err(rejected(
                "signed payment does not match payer, recipient, value or commitment",
            ));
        }
        Ok(tx)
    }
    /// Admission only, not settlement: inspect current spendability before acknowledging
    /// an unbroadcast opening. No submission or reservation on the chain is implied.
    pub async fn admit_payment_envelope(
        &self,
        raw: &str,
        from: &str,
        to: &str,
        value: &str,
        data: &[u8],
    ) -> Result<ValidatedPaymentEnvelope, ChainError> {
        let tx = self.decoded_payment(raw, from, to, value, data)?;
        let first = self.settlement_view().await?;
        let (nonce, pending) = self.nonce_state(&tx.from).await?;
        if nonce != tx.nonce || nonce == u64::MAX {
            return Err(rejected(
                "funding intent must use the sender's current available nonce",
            ));
        }
        if pending != nonce {
            return Err(ChainError::Pending("sender has another pending transaction".into()));
        }
        if tx.max_fee == 0 {
            return Err(rejected("funding intent must pay nonzero gas"));
        }
        let balance = quantity(
            &rpc_client::call(&self.config, "eth_getBalance", json!([tx.from, "latest"])).await?,
        )?;
        let needed = U256::from(tx.max_fee) * U256::from(tx.gas) + U256::from(tx.value);
        if needed > U256::from(balance) {
            return Err(rejected(
                "insufficient balance for funding value plus signed gas limit",
            ));
        }
        let (next_nonce, next_pending) = self.nonce_state(&tx.from).await?;
        let second = self.settlement_view().await?;
        if nonce != next_nonce || pending != next_pending
            || second.head.height < first.head.height
            || (second.head.height == first.head.height && second.head.hash != first.head.hash)
        {
            return Err(ChainError::Pending(
                "canonical sender state changed during funding admission".into(),
            ));
        }
        Ok(tx.envelope())
    }
    pub async fn verify_payment(
        &self,
        raw: &str,
        from: &str,
        to: &str,
        value: Option<&str>,
        data: &[u8],
    ) -> Result<VerifiedPayment, ChainError> {
        let tx = decode(raw, self.identity().chain_id).map_err(rejected)?;
        if tx.from != normalize_address(from).map_err(rejected)?
            || tx.to != normalize_address(to).map_err(rejected)?
            || tx.data != data
            || value.map(wei).transpose()?.is_some_and(|v| v != tx.value)
        {
            return Err(rejected(
                "signed payment does not match payer, recipient, value or commitment",
            ));
        }
        let (block_hash, block_number) = self.verify_canonical_execution(&tx, 1).await?;
        Ok(VerifiedPayment {
            network: self.identity().clone(),
            tx_hash: tx.tx_hash,
            from: tx.from,
            to: tx.to,
            value_wei: tx.value.to_string(),
            data: tx.data,
            block_hash,
            block_number,
            raw_transaction: raw.to_owned(),
        })
    }
    async fn observe_execution(
        &self,
        tx: &Decoded,
        previous: Option<&VerifiedPayment>,
    ) -> Result<Option<ExecutionObservation>, ChainError> {
        let view = self.settlement_view().await?;
        let value = self.receipt(&tx.tx_hash).await?;
        let receipt = if value.is_null() {
            None
        } else {
            Some(self.execution_receipt_fields(&value, tx)?)
        };
        let height = match (&receipt, previous) {
            (Some(receipt), _) => receipt.height,
            (None, Some(previous)) => previous.block_number,
            (None, None) => return Ok(None),
        };
        if height > view.safe.height {
            return Ok(None);
        }
        // Never infer canonicality just from a receipt or an arbitrary confirmation count.
        let canonical = self.block(&format!("0x{height:x}")).await?;
        Self::validate_frontier(&view.safe, &canonical)?;
        Ok(Some(ExecutionObservation { receipt, canonical }))
    }
    async fn verify_canonical_execution(
        &self,
        tx: &Decoded,
        expected_status: u64,
    ) -> Result<(String, u64), ChainError> {
        let pending = || ChainError::Pending("receipt is not stably canonical at the eSpace safe frontier".into());
        let first = self.observe_execution(tx, None).await?.ok_or_else(pending)?;
        let second = self.observe_execution(tx, None).await?.ok_or_else(pending)?;
        if first.receipt != second.receipt || first.canonical != second.canonical {
            return Err(pending());
        }
        let receipt = second.receipt.ok_or_else(pending)?;
        if receipt.hash != second.canonical.hash {
            return Err(pending());
        }
        if receipt.status != expected_status {
            return Err(rejected(if expected_status == 1 {
                "transaction execution failed"
            } else {
                "transaction execution did not fail"
            }));
        }
        Ok((receipt.hash, receipt.height))
    }
    pub async fn recheck_payment(
        &self,
        payment: &VerifiedPayment,
    ) -> Result<PaymentState, ChainError> {
        if &payment.network != self.identity() {
            return Err(rejected("payment belongs to another deployment"));
        }
        let tx = decode(&payment.raw_transaction, self.identity().chain_id).map_err(rejected)?;
        if tx.tx_hash != payment.tx_hash
            || tx.from != payment.from
            || tx.to != payment.to
            || tx.value.to_string() != payment.value_wei
            || tx.data != payment.data
        {
            return Err(rejected("stored payment evidence is inconsistent"));
        }
        let Some(first) = self.observe_execution(&tx, Some(payment)).await? else {
            return Ok(PaymentState::Pending);
        };
        let Some(second) = self.observe_execution(&tx, Some(payment)).await? else {
            return Ok(PaymentState::Pending);
        };
        if first.receipt != second.receipt || first.canonical != second.canonical {
            return Ok(PaymentState::Pending);
        }
        match second.receipt {
            Some(receipt) if receipt.hash == second.canonical.hash && receipt.status == 1 => {
                let mut verified = payment.clone();
                verified.block_hash = receipt.hash;
                verified.block_number = receipt.height;
                Ok(PaymentState::Accepted(verified))
            }
            _ => {
                // A missing/failed/noncanonical receipt alone is not loss evidence:
                // establish that the previously accepted inclusion was actually replaced.
                let previous = if second.canonical.height == payment.block_number {
                    second.canonical
                } else {
                    let view = self.settlement_view().await?;
                    if payment.block_number > view.safe.height {
                        return Ok(PaymentState::Pending);
                    }
                    let tag = format!("0x{:x}", payment.block_number);
                    let first = self.block(&tag).await?;
                    let second = self.block(&tag).await?;
                    Self::validate_frontier(&view.safe, &first)?;
                    Self::validate_frontier(&view.safe, &second)?;
                    if first != second {
                        return Ok(PaymentState::Pending);
                    }
                    second
                };
                if previous.hash == payment.block_hash {
                    return Err(unavailable(
                        "receipt contradicts its previously accepted canonical block",
                    ));
                }
                Ok(PaymentState::Orphaned)
            }
        }
    }
    pub async fn proxy_call(&self, method: &str, params: Value) -> Result<Value, ChainError> {
        if !crate::chain_rpc_api::allowed_method(method) {
            return Err(rejected("RPC method is not exposed"));
        }
        self.identity_check().await?;
        if method == "eth_sendRawTransaction" {
            let raw = params
                .get(0)
                .and_then(Value::as_str)
                .ok_or_else(|| rejected("missing signed transaction"))?;
            decode(raw, self.identity().chain_id).map_err(rejected)?;
        }
        self.call(method, params).await
    }
    fn sender_lock(&self, sender: &str) -> Result<Arc<tokio::sync::Mutex<()>>, ChainError> {
        let mut senders = self
            .senders
            .lock()
            .map_err(|_| unavailable("sender lock poisoned"))?;
        Ok(senders
            .entry(sender.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone())
    }
    fn process_lock(&self, sender: &str) -> Result<ExclusiveFileLock, ChainError> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(self.dir.join(format!("{}.lock", &sender[2..])))
            .map_err(|_| unavailable("cannot open wallet process lock"))?;
        ExclusiveFileLock::try_acquire(file).map_err(|_| {
            ChainError::Pending("another process is submitting or reconciling this wallet".into())
        })
    }
    fn records(&self, sender: &str) -> Result<Vec<JournalRecord>, ChainError> {
        let connection = self
            .journal
            .lock()
            .map_err(|_| unavailable("journal lock poisoned"))?;
        let mut statement=connection.prepare("SELECT operation,raw,status,error,evidence,created_at FROM transactions WHERE sender=?1 ORDER BY created_at,nonce").map_err(|_|unavailable("cannot read wallet journal"))?;
        let rows = statement
            .query_map([sender], |r| {
                Ok(JournalRecord {
                    operation: r.get(0)?,
                    raw: r.get(1)?,
                    status: r.get(2)?,
                    error: r.get(3)?,
                    evidence: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })
            .map_err(|_| unavailable("cannot read wallet journal"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| unavailable("corrupt wallet journal"))
    }
    fn update(
        &self,
        sender: &str,
        operation: &str,
        status: &str,
        error: Option<&str>,
        evidence: Option<&VerifiedPayment>,
    ) -> Result<(), ChainError> {
        let evidence = evidence
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| unavailable("cannot serialize payment evidence"))?;
        self.journal.lock().map_err(|_|unavailable("journal lock poisoned"))?.execute("UPDATE transactions SET status=?3,error=?4,evidence=COALESCE(?5,evidence) WHERE sender=?1 AND operation=?2",params![sender,operation,status,error,evidence]).map_err(|_|unavailable("cannot persist wallet status"))?;
        Ok(())
    }
    fn cancellation_record(
        &self,
        sender: &str,
        operation: &str,
    ) -> Result<Option<CancellationRecord>, ChainError> {
        self.journal
            .lock()
            .map_err(|_| unavailable("journal lock poisoned"))?
            .query_row(
                "SELECT raw,status,error FROM cancellations WHERE sender=?1 AND operation=?2",
                params![sender, operation],
                |r| {
                    Ok(CancellationRecord {
                        raw: r.get(0)?,
                        status: r.get(1)?,
                        error: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|_| unavailable("cannot read cancellation journal"))
    }
    fn update_cancellation(
        &self,
        sender: &str,
        operation: &str,
        status: &str,
        error: Option<&str>,
        evidence: Option<&VerifiedPayment>,
    ) -> Result<(), ChainError> {
        let evidence = evidence
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| unavailable("cannot serialize cancellation evidence"))?;
        self.journal.lock().map_err(|_|unavailable("journal lock poisoned"))?.execute(
            "UPDATE cancellations SET status=?3,error=?4,evidence=COALESCE(?5,evidence) WHERE sender=?1 AND operation=?2",
            params![sender,operation,status,error,evidence]).map_err(|_|unavailable("cannot persist cancellation status"))?;
        Ok(())
    }
    async fn reconcile_cancellation(
        &self,
        sender: &str,
        record: &JournalRecord,
        cancellation: &CancellationRecord,
    ) -> Result<(), ChainError> {
        let original = decode(&record.raw, self.identity().chain_id).map_err(unavailable)?;
        let replacement =
            decode(&cancellation.raw, self.identity().chain_id).map_err(unavailable)?;
        let fee = original
            .max_fee
            .checked_add(original.max_fee / 8)
            .and_then(|fee| fee.checked_add(1));
        if original.from != sender
            || replacement.from != sender
            || replacement.to != sender
            || replacement.value != 0
            || !replacement.data.is_empty()
            || replacement.nonce != original.nonce
            || fee != Some(replacement.max_fee)
            || replacement.kind != 0
            || replacement.gas != 21000
        {
            return Err(unavailable(
                "cancellation journal does not match original transaction",
            ));
        }
        match self
            .verify_payment(
                &record.raw,
                sender,
                &original.to,
                Some(&original.value.to_string()),
                &original.data,
            )
            .await
        {
            Ok(evidence) => {
                self.update(sender, &record.operation, "accepted", None, Some(&evidence))?;
                return self.update_cancellation(
                    sender,
                    &record.operation,
                    "original_accepted",
                    Some("original payment won; funding is non-refundable"),
                    None,
                );
            }
            Err(ChainError::Rejected(reason)) if reason == "transaction execution failed" => {
                match self.verify_canonical_execution(&original, 0).await {
                    Ok(_) => {
                        self.update(
                            sender,
                            &record.operation,
                            "failed",
                            Some("original execution failed and consumed its nonce"),
                            None,
                        )?;
                        return self.update_cancellation(
                            sender,
                            &record.operation,
                            "original_failed",
                            Some("original execution failed; gas was spent"),
                            None,
                        );
                    }
                    Err(ChainError::Pending(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            Err(ChainError::Pending(_)) => {}
            Err(error) => return Err(error),
        }
        match self
            .verify_payment(&cancellation.raw, sender, sender, Some("0"), &[])
            .await
        {
            Ok(evidence) => {
                self.update_cancellation(
                    sender,
                    &record.operation,
                    "cancelled",
                    None,
                    Some(&evidence),
                )?;
                self.update(
                    sender,
                    &record.operation,
                    "cancelled",
                    Some("same-nonce self-transfer is successfully canonical at safe"),
                    None,
                )
            }
            Err(ChainError::Rejected(reason)) if reason == "transaction execution failed" => {
                match self.verify_canonical_execution(&replacement, 0).await {
                    Ok(_) => {
                        self.update(
                            sender,
                            &record.operation,
                            "failed",
                            Some("cancellation execution failed and consumed its nonce"),
                            None,
                        )?;
                        self.update_cancellation(
                            sender,
                            &record.operation,
                            "cancellation_failed",
                            Some("replacement execution failed; gas was spent"),
                            None,
                        )
                    }
                    Err(ChainError::Pending(reason)) => {
                        self.update(
                            sender,
                            &record.operation,
                            "cancellation_pending",
                            Some(&reason),
                            None,
                        )?;
                        self.update_cancellation(
                            sender,
                            &record.operation,
                            "cancellation_pending",
                            Some(&reason),
                            None,
                        )
                    }
                    Err(error) => Err(error),
                }
            }
            Err(ChainError::Pending(reason)) => {
                self.update(
                    sender,
                    &record.operation,
                    "cancellation_pending",
                    Some(&reason),
                    None,
                )?;
                self.update_cancellation(
                    sender,
                    &record.operation,
                    "cancellation_pending",
                    Some(&reason),
                    None,
                )
            }
            Err(error) => Err(error),
        }
    }
    fn cancellation_outcome(
        &self,
        sender: &str,
        record: &JournalRecord,
    ) -> Result<PaymentCancellation, ChainError> {
        let original = decode(&record.raw, self.identity().chain_id).map_err(unavailable)?;
        let cancellation = self.cancellation_record(sender, &record.operation)?;
        let replacement = cancellation
            .as_ref()
            .map(|r| decode(&r.raw, self.identity().chain_id).map_err(unavailable))
            .transpose()?;
        Ok(PaymentCancellation {
            operation_id: record.operation.clone(),
            original_tx_hash: original.tx_hash,
            replacement_tx_hash: replacement.as_ref().map(|tx| tx.tx_hash.clone()),
            status: cancellation
                .as_ref()
                .map(|r| r.status.clone())
                .unwrap_or_else(|| {
                    (if record.status == "accepted" {
                        "original_accepted"
                    } else {
                        "original_failed"
                    })
                    .into()
                }),
            gas_price_wei: replacement.as_ref().map(|tx| tx.max_fee.to_string()),
            maximum_gas_cost_wei: replacement
                .as_ref()
                .map(|tx| (U256::from(tx.max_fee) * U256::from(tx.gas)).to_string()),
            error: cancellation.and_then(|r| r.error),
        })
    }
    /// Reconcile a recovery without signing or broadcasting; safe for status-only displays.
    pub async fn cancellation_status(
        &self,
        operation_id: &str,
        address: &str,
    ) -> Result<Option<PaymentCancellation>, ChainError> {
        let sender = normalize_address(address).map_err(rejected)?;
        let lock = self.sender_lock(&sender)?;
        let _guard = lock.lock().await;
        let _file = self.process_lock(&sender)?;
        if self.cancellation_record(&sender, operation_id)?.is_none() {
            return Ok(None);
        }
        self.reconcile_locked(&sender).await?;
        let record = self
            .records(&sender)?
            .into_iter()
            .find(|r| r.operation == operation_id)
            .ok_or_else(|| unavailable("cancellation has no original journal record"))?;
        self.cancellation_outcome(&sender, &record).map(Some)
    }
    /// Explicit user-authorized recovery only: costs gas and races the original, even without
    /// provider acknowledgement. One replacement at old_max_fee + floor(old_max_fee/8) + 1;
    /// every later call retries those exact bytes, never bumps again. No automatic caller.
    pub async fn cancel_payment_once(
        &self,
        operation_id: &str,
        key: &str,
    ) -> Result<PaymentCancellation, ChainError> {
        let sender = wallet::address_from_private_key(key).map_err(rejected)?;
        let lock = self.sender_lock(&sender)?;
        let _guard = lock.lock().await;
        let _file = self.process_lock(&sender)?;
        self.reconcile_locked(&sender).await?;
        let record = self
            .records(&sender)?
            .into_iter()
            .find(|r| r.operation == operation_id)
            .ok_or_else(|| rejected("no original payment exists for this operation"))?;
        if matches!(record.status.as_str(), "accepted" | "failed" | "cancelled") {
            return self.cancellation_outcome(&sender, &record);
        }
        let original = decode(&record.raw, self.identity().chain_id).map_err(unavailable)?;
        self.settlement_view().await?;
        // A queued original increases pending, not latest. Cancellation deliberately
        // replaces that nonce; it must not allocate the next pool nonce.
        let (nonce, _) = self.nonce_state(&sender).await?;
        if nonce != original.nonce {
            if self.cancellation_record(&sender, operation_id)?.is_some() {
                return self.cancellation_outcome(&sender, &record);
            }
            return Err(ChainError::Pending("original nonce is not current; await safe original receipt, never assume cancellation".into()));
        }
        if self.cancellation_record(&sender, operation_id)?.is_none() {
            let gas_price = original
                .max_fee
                .checked_add(original.max_fee / 8)
                .and_then(|fee| fee.checked_add(1))
                .ok_or_else(|| rejected("cancellation fee exceeds u128"))?;
            let balance = quantity(
                &rpc_client::call(&self.config, "eth_getBalance", json!([sender, "latest"]))
                    .await?,
            )?;
            if U256::from(gas_price) * U256::from(21000u64) > U256::from(balance) {
                return Err(rejected(
                    "insufficient balance for explicit cancellation gas",
                ));
            }
            if self.estimate_gas(&sender, &sender, 0, &[], gas_price).await? != 21000 {
                return Err(rejected("cancellation requires a plain native self-transfer"));
            }
            let replacement = sign_transaction(
                key,
                self.identity().chain_id,
                nonce,
                &sender,
                0,
                &[],
                gas_price,
                21000,
            )
            .map_err(rejected)?;
            self.journal.lock().map_err(|_|unavailable("journal lock poisoned"))?.execute(
                "INSERT INTO cancellations(sender,operation,raw,status) VALUES(?1,?2,?3,'cancellation_pending')",
                params![sender,operation_id,replacement.raw_transaction]).map_err(|_|unavailable("cannot durably prepare cancellation; nothing broadcast"))?;
        }
        let cancellation = self
            .cancellation_record(&sender, operation_id)?
            .ok_or_else(|| unavailable("cancellation disappeared"))?;
        let replacement =
            decode(&cancellation.raw, self.identity().chain_id).map_err(unavailable)?;
        // The original remains in transactions forever; the replacement has its own FULL-sync row.
        let error = match rpc_client::call(
            &self.config,
            "eth_sendRawTransaction",
            json!([cancellation.raw]),
        )
        .await
        {
            Ok(hash)
                if hash
                    .as_str()
                    .is_some_and(|hash| hash.eq_ignore_ascii_case(&replacement.tx_hash)) =>
            {
                None
            }
            Ok(_) => Some(
                "RPC returned a different replacement hash; submission is uncertain".to_string(),
            ),
            Err(error) => Some(error.to_string()),
        };
        self.update(
            &sender,
            operation_id,
            "cancellation_pending",
            error.as_deref(),
            None,
        )?;
        self.update_cancellation(
            &sender,
            operation_id,
            "cancellation_pending",
            error.as_deref(),
            None,
        )?;
        self.cancellation_outcome(&sender, &record)
    }
    async fn reconcile_locked(&self, sender: &str) -> Result<(), ChainError> {
        self.identity_check().await?;
        for record in self.records(sender)? {
            if let Some(cancellation) = self.cancellation_record(sender, &record.operation)? {
                self.reconcile_cancellation(sender, &record, &cancellation)
                    .await?;
                continue;
            }
            let tx = decode(&record.raw, self.identity().chain_id).map_err(unavailable)?;
            if let Some(evidence) = &record.evidence {
                let evidence: VerifiedPayment = serde_json::from_str(evidence)
                    .map_err(|_| unavailable("corrupt journal evidence"))?;
                match self.recheck_payment(&evidence).await? {
                    PaymentState::Accepted(evidence) => {
                        self.update(sender, &record.operation, "accepted", None, Some(&evidence))?;
                        continue;
                    }
                    PaymentState::Orphaned => {
                        self.update(
                            sender,
                            &record.operation,
                            "orphaned",
                            Some("previously accepted payment is no longer canonical"),
                            None,
                        )?;
                    }
                    PaymentState::Pending => {
                        self.update(
                            sender,
                            &record.operation,
                            "pending",
                            Some("previously accepted payment is awaiting settlement"),
                            None,
                        )?;
                        continue;
                    }
                }
            }
            match self
                .verify_payment(
                    &record.raw,
                    sender,
                    &tx.to,
                    Some(&tx.value.to_string()),
                    &tx.data,
                )
                .await
            {
                Ok(evidence) => {
                    self.update(sender, &record.operation, "accepted", None, Some(&evidence))?
                }
                Err(ChainError::Pending(reason)) => {
                    let receipt = self.receipt(&tx.tx_hash).await?;
                    let nonce = quantity64(
                        &rpc_client::call(
                            &self.config,
                            "eth_getTransactionCount",
                            json!([sender, "latest"]),
                        )
                        .await?,
                    )?;
                    if receipt.is_null() && nonce > tx.nonce {
                        self.update(
                            sender,
                            &record.operation,
                            "nonce_conflict",
                            Some("sender nonce consumed without this canonical receipt"),
                            None,
                        )?;
                    } else if record.status == "prepared" {
                        self.update(sender, &record.operation, "prepared", None, None)?;
                    } else {
                        self.update(sender, &record.operation, "pending", Some(&reason), None)?;
                    }
                }
                Err(ChainError::Rejected(reason)) if reason == "transaction execution failed" => {
                    // A failed execution consumes the nonce only when repeatedly canonical at safe.
                    match self.verify_canonical_execution(&tx, 0).await {
                        Ok(_) => {
                            self.update(sender, &record.operation, "failed", Some(&reason), None)?
                        }
                        Err(ChainError::Pending(_)) => self.update(
                            sender,
                            &record.operation,
                            "pending",
                            Some("failed execution is not yet stably settled"),
                            None,
                        )?,
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
    pub async fn reconcile(&self, address: &str) -> Result<(), ChainError> {
        let sender = normalize_address(address).map_err(rejected)?;
        let lock = self.sender_lock(&sender)?;
        let _guard = lock.lock().await;
        let _file = self.process_lock(&sender)?;
        self.reconcile_locked(&sender).await
    }
    /// Local prepared/submitted operations only; not an account's complete on-chain history.
    pub async fn history(&self, address: &str) -> Result<Value, ChainError> {
        let sender = normalize_address(address).map_err(rejected)?;
        let records = self.records(&sender)?;
        let entries=records.into_iter().map(|record| {
            let tx=decode(&record.raw,self.identity().chain_id).map_err(unavailable)?;
            let cancellation=if let Some(replacement)=self.cancellation_record(&sender,&record.operation)? {
                let mut value=serde_json::to_value(self.cancellation_outcome(&sender,&record)?).map_err(unavailable)?;
                value["raw_transaction"]=json!(replacement.raw);value
            } else {Value::Null};
            Ok(json!({"operation_id":record.operation,"tx_hash":tx.tx_hash,"raw_transaction":record.raw,"from":sender,"to":tx.to,"value_wei":tx.value.to_string(),"nonce":tx.nonce,"status":record.status,"error":record.error,"created_at":record.created_at,"cancellation":cancellation}))
        }).collect::<Result<Vec<_>,ChainError>>()?;
        Ok(
            json!({"network":self.identity(),"scope":"local_payment_operations_only","complete_chain_history":false,"transactions":entries}),
        )
    }
    /// Look up one durable original using the caller's canonical wallet address; never signs or broadcasts.
    pub(crate) fn original_payment_raw(
        &self,
        operation_id: &str,
        sender: &str,
    ) -> Result<Option<String>, ChainError> {
        let connection = self
            .journal
            .lock()
            .map_err(|_| unavailable("journal lock poisoned"))?;
        connection
            .query_row(
                "SELECT raw FROM transactions WHERE sender=?1 AND operation=?2",
                params![sender, operation_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| unavailable("cannot read wallet operation"))
    }
    pub async fn send_payment(
        &self,
        key: &str,
        to: &str,
        value_wei: &str,
        data: &[u8],
    ) -> Result<SubmittedTransaction, ChainError> {
        self.send_payment_once(&uuid::Uuid::new_v4().to_string(), key, to, value_wei, data)
            .await
    }
    pub async fn send_payment_once(
        &self,
        operation_id: &str,
        key: &str,
        to: &str,
        value_wei: &str,
        data: &[u8],
    ) -> Result<SubmittedTransaction, ChainError> {
        self.payment_once(operation_id, key, to, value_wei, data, true)
            .await
    }
    /// Persist the original signed transaction, reserving its nonce, but NEVER broadcast it.
    pub async fn prepare_payment_once(
        &self,
        operation_id: &str,
        key: &str,
        to: &str,
        value_wei: &str,
        data: &[u8],
    ) -> Result<SubmittedTransaction, ChainError> {
        self.payment_once(operation_id, key, to, value_wei, data, false)
            .await
    }
    async fn payment_once(
        &self,
        operation_id: &str,
        key: &str,
        to: &str,
        value_wei: &str,
        data: &[u8],
        broadcast: bool,
    ) -> Result<SubmittedTransaction, ChainError> {
        if operation_id.is_empty()
            || operation_id.len() > 256
            || operation_id.bytes().any(|b| b.is_ascii_control())
        {
            return Err(rejected("operation id must be 1..256 printable bytes"));
        }
        let sender = wallet::address_from_private_key(key).map_err(rejected)?;
        let to = normalize_address(to).map_err(rejected)?;
        let value = wei(value_wei)?;
        if data.len() > MAX_PAYMENT_DATA {
            return Err(rejected("payment commitment exceeds size limit"));
        }
        let lock = self.sender_lock(&sender)?;
        let _guard = lock.lock().await;
        let _file = self.process_lock(&sender)?;
        let existing = self.original_payment_raw(operation_id, &sender)?;
        if let Some(raw) = existing {
            let tx = decode(&raw, self.identity().chain_id).map_err(unavailable)?;
            if tx.from != sender || tx.to != to || tx.value != value || tx.data != data {
                return Err(rejected(
                    "operation id is already bound to different payment terms",
                ));
            }
            let mut record = self
                .records(&sender)?
                .into_iter()
                .find(|r| r.operation == operation_id)
                .ok_or_else(|| unavailable("journal operation disappeared"))?;
            // Ordinary preparation can recover the bytes locally, even offline.
            // A blocking state may be a receipt gap/reorg, not a permanent loss:
            // recheck it under the same sender/process locks before rejecting resume.
            // Reconciliation never signs or broadcasts, including cancellation races.
            if broadcast || !matches!(
                record.status.as_str(),
                "prepared" | "submitted" | "pending" | "accepted"
            ) {
                self.reconcile_locked(&sender).await?;
                record = self
                    .records(&sender)?
                    .into_iter()
                    .find(|r| r.operation == operation_id)
                    .ok_or_else(|| unavailable("journal operation disappeared"))?;
            }
            if !matches!(
                record.status.as_str(),
                "prepared" | "submitted" | "pending" | "accepted"
            ) {
                return Err(rejected(format!(
                    "journal operation {}: {}",
                    record.status,
                    record.error.unwrap_or_default()
                )));
            }
            if self.cancellation_record(&sender, operation_id)?.is_some()
                && record.status != "accepted"
            {
                return Err(rejected(
                    "explicit cancellation exists; original payment will not be broadcast again",
                ));
            }
            let mut submitted = SubmittedTransaction {
                tx_hash: tx.tx_hash,
                raw_transaction: raw,
                status: record.status,
            };
            if broadcast && submitted.status != "accepted" {
                submitted.status = self.broadcast(&sender, operation_id, &submitted).await?;
            }
            return Ok(submitted);
        }
        self.reconcile_locked(&sender).await?;
        if self
            .records(&sender)?
            .iter()
            .any(|r| !matches!(r.status.as_str(), "accepted" | "failed" | "cancelled"))
        {
            return Err(ChainError::Pending("wallet already has an unresolved transaction; reconcile, resume, or explicitly cancel its operation".into()));
        }
        self.settlement_view().await?;
        let (nonce, pending) = self.nonce_state(&sender).await?;
        if nonce == u64::MAX {
            return Err(rejected("wallet nonce is exhausted"));
        }
        // This journal intentionally permits only one outstanding operation per wallet.
        // Do not queue behind unknown external transactions or replace them accidentally.
        if pending != nonce {
            return Err(ChainError::Pending("wallet has an external pending transaction; wait for its execution".into()));
        }
        let assigned: bool = self
            .journal
            .lock()
            .map_err(|_| unavailable("journal lock poisoned"))?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM transactions WHERE sender=?1 AND nonce=?2)",
                params![sender, nonce.to_string()],
                |r| r.get(0),
            )
            .map_err(|_| unavailable("cannot check durable nonce assignment"))?;
        if assigned {
            self.reconcile_locked(&sender).await?;
            return Err(ChainError::Pending("chain nonce already belongs to an original wallet operation; resume or explicitly cancel that operation".into()));
        }
        let balance = quantity(
            &rpc_client::call(&self.config, "eth_getBalance", json!([sender, "latest"])).await?,
        )?;
        let gas_price =
            quantity(&rpc_client::call(&self.config, "eth_gasPrice", json!([])).await?)?;
        if gas_price == 0 {
            return Err(unavailable("node returned zero gas price"));
        }
        let gas = self.estimate_gas(&sender, &to, value, data, gas_price).await?;
        let needed = U256::from(gas_price)
            .checked_mul(U256::from(gas))
            .and_then(|n| n.checked_add(U256::from(value)))
            .ok_or_else(|| rejected("payment plus gas overflow"))?;
        if needed > U256::from(balance) {
            return Err(rejected(
                "insufficient balance for value plus estimated native gas",
            ));
        }
        if self.nonce_state(&sender).await? != (nonce, pending) {
            return Err(ChainError::Pending("sender nonce changed before signing; nothing prepared".into()));
        }
        let mut submitted = sign_transaction(
            key,
            self.identity().chain_id,
            nonce,
            &to,
            value,
            data,
            gas_price,
            gas,
        )
        .map_err(rejected)?;
        self.journal.lock().map_err(|_|unavailable("journal lock poisoned"))?.execute("INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at) VALUES(?1,?2,?3,?4,?5,'prepared',?6)",params![sender,operation_id,nonce.to_string(),submitted.tx_hash,submitted.raw_transaction,protocol::now().map_err(unavailable)?]).map_err(|_|unavailable("cannot durably prepare payment; nothing broadcast"))?;
        if broadcast {
            submitted.status = self.broadcast(&sender, operation_id, &submitted).await?;
        }
        Ok(submitted)
    }
    async fn broadcast(
        &self,
        sender: &str,
        operation: &str,
        tx: &SubmittedTransaction,
    ) -> Result<String, ChainError> {
        let (status, error) = match rpc_client::call(
            &self.config,
            "eth_sendRawTransaction",
            json!([tx.raw_transaction]),
        )
        .await
        {
            Ok(hash)
                if hash
                    .as_str()
                    .is_some_and(|h| h.eq_ignore_ascii_case(&tx.tx_hash)) =>
            {
                ("submitted", None)
            }
            Ok(_) => (
                "pending",
                Some(
                    "RPC returned a different transaction hash; submission outcome uncertain"
                        .to_owned(),
                ),
            ),
            // Even a remote rejection can mean an earlier attempt was admitted.
            Err(error) => ("pending", Some(error.to_string())),
        };
        self.update(sender, operation, status, error.as_deref(), None)?;
        Ok(status.to_owned())
    }
}
struct JournalRecord {
    operation: String,
    raw: String,
    status: String,
    error: Option<String>,
    evidence: Option<String>,
    created_at: u64,
}
struct CancellationRecord {
    raw: String,
    status: String,
    error: Option<String>,
}

#[cfg(test)]
#[path = "chain_client_tests.rs"]
mod tests;
