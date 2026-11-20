//! Native consumer service. Wallet keys and credentials never cross the UI boundary implicitly.
use crate::{
    chain_client::{ChainClient, PaymentCancellation},
    network::NetworkConfig,
    protocol::{self, *},
    wallet,
};
use futures_util::StreamExt;
use reqwest::{Client, Method, Response};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, watch, Mutex};
use url::Url;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumerConfig {
    pub network: NetworkConfig,
    /// Shared wallet journal base: every provider/consumer spending the same key must use this
    /// same configured directory. ChainClient adds network scoping and wallet-journal itself.
    pub data_dir: PathBuf,
    pub catalog_url: Option<String>,
    #[serde(default)]
    pub development: bool,
    #[serde(default)]
    pub dht: Option<crate::provider_config::DhtConfig>,
}

pub struct Consumer {
    config: ConsumerConfig,
    root: PathBuf,
    chain: ChainClient,
    mutation: Mutex<()>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedAccount {
    offer: Signed<InferenceOffer>,
    quote: Signed<Quote>,
    account: Option<AccountView>,
    raw_transaction: Option<String>,
    tx_hash: Option<String>,
    #[serde(default)]
    funding_intent: Option<Signed<FundingIntent>>,
    #[serde(default)]
    funding_broadcast_started: bool,
    #[serde(default)]
    funding_cancellation_requested: bool,
    #[serde(default)]
    funding_cancellation: Option<PaymentCancellation>,
    credentials: Vec<Credential>,
    active_credential: Option<String>,
    topup: Option<TopupIntent>,
    receipt_sequence: u64,
    /// Top-up originals of this agreement that won their explicit cancellation race
    /// (`original_accepted`), recorded by any reconciliation. `resume` settles each one
    /// once with the provider (inference-funding-settlement requirements 16 and 17).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    won_topups: Vec<WonTopup>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WonTopup {
    operation_id: String,
    original_tx_hash: String,
    credited: bool,
}
/// One of this agreement's journaled operations that carries an explicit cancellation,
/// as last reconciled. Never contains signed bytes.
struct CancelledOperation {
    operation_id: String,
    tx_hash: String,
    value_wei: String,
    outcome: PaymentCancellation,
}
fn cancelled_operations(id: &str, history: &Value) -> Result<Vec<CancelledOperation>, String> {
    let opening = format!("open:{id}");
    let prefix = format!("topup:{id}:");
    let mut operations = Vec::new();
    for entry in history["transactions"]
        .as_array()
        .ok_or("invalid wallet journal")?
    {
        let operation = entry["operation_id"]
            .as_str()
            .ok_or("invalid wallet operation")?;
        if (operation != opening && !operation.starts_with(&prefix))
            || entry["cancellation"].is_null()
        {
            continue;
        }
        let mut cancellation = entry["cancellation"].clone();
        cancellation
            .as_object_mut()
            .ok_or("invalid wallet cancellation")?
            .remove("raw_transaction");
        operations.push(CancelledOperation {
            operation_id: operation.into(),
            tx_hash: entry["tx_hash"]
                .as_str()
                .ok_or("invalid wallet operation")?
                .into(),
            value_wei: entry["value_wei"]
                .as_str()
                .ok_or("invalid wallet operation")?
                .into(),
            outcome: serde_json::from_value(cancellation)
                .map_err(|_| "invalid wallet cancellation")?,
        });
    }
    Ok(operations)
}
/// This wallet journal's entry for one exact operation, if any.
fn journal_entry<'a>(history: &'a Value, operation_id: &str) -> Result<Option<&'a Value>, String> {
    Ok(history["transactions"]
        .as_array()
        .ok_or("invalid wallet journal")?
        .iter()
        .find(|entry| entry["operation_id"].as_str() == Some(operation_id)))
}
/// A reconciled journal entry's cancellation outcome, reported as `cancel_payment_once`
/// reports it. Without a journaled replacement the outcome is the original itself, and
/// `None` while that original is unresolved.
fn reconciled_outcome(entry: &Value) -> Option<&str> {
    entry["cancellation"]["status"]
        .as_str()
        .or(match entry["status"].as_str() {
            Some("accepted") => Some("original_accepted"),
            Some("failed") => Some("original_failed"),
            _ => None,
        })
}
fn mark_topup_credited(saved: &mut SavedAccount, operation_id: &str, original_tx_hash: &str) {
    match saved
        .won_topups
        .iter_mut()
        .find(|won| won.operation_id == operation_id)
    {
        Some(won) => won.credited = true,
        None => saved.won_topups.push(WonTopup {
            operation_id: operation_id.into(),
            original_tx_hash: original_tx_hash.into(),
            credited: true,
        }),
    }
}
#[derive(Serialize, Deserialize)]
struct TopupIntent {
    id: String,
    amount_wei: String,
    raw_transaction: Option<String>,
    /// Saved by `cancel_funding` before its cancellation attempt, so the request survives
    /// an attempt that fails before journaling a replacement. From then on no path
    /// broadcasts this operation's original or prepares another payment for it; `resume`
    /// only settles its reconciled outcome (inference-funding-settlement requirement 16).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    cancellation_requested: bool,
}
/// How a pending top-up whose cancellation was requested resolved.
enum CancelledTopup {
    /// Its original won and was credited once.
    Credited(AccountView),
    /// It resolved without credit (this status), and its intent was cleared.
    Resolved(String),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalRequest {
    contract_id: String,
    request_key: String,
    created_at: u64,
}

#[derive(Clone)]
pub struct Cancellation {
    sender: watch::Sender<bool>,
}
impl Cancellation {
    pub fn cancel(&self) {
        self.sender.send_replace(true);
    }
}
#[derive(Clone, Serialize)]
pub struct StreamEvent {
    pub kind: String,
    pub data: Value,
}
pub struct InferenceStream {
    pub events: mpsc::Receiver<StreamEvent>,
    pub cancel: Cancellation,
}

fn private_read(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = wallet::open_secret_file(path)?;
    if file
        .metadata()
        .map_err(|_| "cannot inspect protected consumer file")?
        .len()
        > 8 * 1024 * 1024
    {
        return Err("consumer file exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read protected consumer file")?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("consumer file exceeds size limit".into());
    }
    Ok(bytes)
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("consumer file has no parent")?;
    wallet::secure_directory(parent)?;
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = wallet::create_secret_file(&temporary)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "cannot persist consumer state")?;
        drop(file);
        fs::rename(&temporary, path).map_err(|_| "cannot commit consumer state")?;
        #[cfg(unix)]
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| "cannot sync consumer state")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|_| "invalid consumer protocol document".into())
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|e| e.to_string())
}

/// Reject ambiguous bases; callers append individual segments, never interpolate IDs into URLs.
pub fn endpoint_url(base: &str, segments: &[&str], development: bool) -> Result<Url, String> {
    let mut url = Url::parse(base).map_err(|_| "invalid endpoint URL")?;
    let local = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name == "localhost",
        None => false,
    };
    if url.scheme() != "https" && !(development && local && url.scheme() == "http") {
        return Err("HTTPS required except explicitly enabled loopback development".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(
            "endpoint must be an origin without credentials, path, query or fragment".into(),
        );
    }
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| "endpoint cannot hold path segments")?;
        path.clear();
        for segment in segments {
            if segment.is_empty()
                || *segment == "."
                || *segment == ".."
                || segment.contains('/')
                || segment.contains('\\')
            {
                return Err("invalid endpoint path segment".into());
            }
            path.push(segment);
        }
    }
    Ok(url)
}
async fn transport(base: &str, development: bool) -> Result<Client, String> {
    endpoint_url(base, &[], development)?;
    crate::offer_registry::safe_http_client(base, development).await
}
async fn bounded_body(response: Response) -> Result<(reqwest::StatusCode, Vec<u8>), String> {
    let status = response.status();
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| "HTTP response interrupted; do not replay paid generation")?;
        if body.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err("HTTP response exceeds size limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok((status, body))
}
async fn response_bytes(response: Response) -> Result<Vec<u8>, String> {
    let (status, body) = bounded_body(response).await?;
    if !status.is_success() {
        // Classify trusted protocol codes, never echo provider-supplied text or secrets.
        let error = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
        let (code, message) = safe_error(&error);
        return Err(format!("{code}: {message} (HTTP {})", status.as_u16()));
    }
    Ok(body)
}
/// An answer from a source that is not the provider (a catalog or the version-policy
/// source) is classified with that source's own table and reported as its answer, so a
/// catalog, RPC or policy-server outage is never attributed to a provider (book line 443).
/// The source's own text is never echoed.
async fn source_bytes(
    response: Response,
    source: &str,
    classify: fn(u16, &Value) -> (&'static str, &'static str),
) -> Result<Vec<u8>, String> {
    let (status, body) = bounded_body(response).await?;
    if !status.is_success() {
        let error = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
        let (code, message) = classify(status.as_u16(), &error);
        return Err(format!("{code}: {message} ({source} HTTP {})", status.as_u16()));
    }
    Ok(body)
}
/// Reputation catalog responses are classified with the catalog's own codes and reported
/// as the catalog's answer: a catalog or RPC outage is never attributed to the provider.
async fn catalog_bytes(response: Response) -> Result<Vec<u8>, String> {
    source_bytes(response, "catalog", catalog_error).await
}
/// Offer catalog pages (`/api/offers`) are the catalog's answer, never a provider's.
async fn offer_catalog_bytes(response: Response) -> Result<Vec<u8>, String> {
    source_bytes(response, "catalog", offer_catalog_error).await
}
/// The configured `policy_url` is the version-policy source, never a provider.
async fn policy_source_bytes(response: Response) -> Result<Vec<u8>, String> {
    source_bytes(response, "policy source", policy_source_error).await
}
/// Fixed messages naming the catalog and the remedy; catalog-supplied text is never echoed.
fn catalog_error(status: u16, value: &Value) -> (&'static str, &'static str) {
    if status == 413 {
        return ("feedback_too_large", "The reputation catalog refused an oversized request body");
    }
    match value["error"]["code"].as_str().unwrap_or("") {
        "invalid_feedback" => ("invalid_feedback", "The reputation catalog refused the feedback as invalid; check the score, revision and saved payment evidence"),
        "expired_feedback" => ("expired_feedback", "The reputation catalog refused feedback signed too long ago; sign a new revision with a higher number"),
        "feedback_conflict" => ("feedback_conflict", "The reputation catalog refused a conflicting vote; sign a new revision with a higher number"),
        "feedback_busy" => ("feedback_busy", "The reputation catalog is busy verifying evidence; retry later. This says nothing about the provider"),
        "payment_pending" => ("payment_pending", "The reputation catalog has not seen the opening payment at the eSpace safe frontier yet; wait, then retry"),
        "payment_rejected" => ("payment_rejected", "The reputation catalog found the opening payment ineligible; do not resubmit this evidence"),
        "chain_unavailable" => ("chain_unavailable", "The reputation catalog cannot verify chain evidence now; this says nothing about the provider"),
        "chain_timeout" => ("chain_timeout", "The reputation catalog timed out verifying chain evidence; retry later. This says nothing about the provider"),
        "invalid_wallet" => ("invalid_wallet", "The reputation catalog refused the provider wallet as not canonical"),
        "internal_error" => ("internal_error", "The reputation catalog failed internally; retry later. This says nothing about the provider"),
        _ => ("catalog_error", "Reputation catalog error; this says nothing about the provider"),
    }
}
/// The relay's own `/api/offers` codes, then the status: a rate limit or an outage of the
/// catalog (or of a proxy in front of it) is the catalog's. No message names a provider.
fn offer_catalog_error(status: u16, value: &Value) -> (&'static str, &'static str) {
    match value["error"]["code"].as_str().unwrap_or("") {
        "invalid_page" => return ("invalid_page", "The offer catalog refused the page request as out of range"),
        "invalid_model" => return ("invalid_model", "The offer catalog refused the model filter; use an exact model ID of 1 to 256 bytes"),
        _ => {}
    }
    match status {
        429 => ("catalog_busy", "The offer catalog is rate limiting discovery; retry later"),
        500..=599 => ("catalog_unavailable", "The offer catalog is unavailable; retry later or configure another catalog"),
        _ => ("catalog_error", "The offer catalog answered with an error; discovery returned no offers"),
    }
}
/// The version-policy source is classified by status alone. No message names a provider.
fn policy_source_error(status: u16, _value: &Value) -> (&'static str, &'static str) {
    match status {
        404 | 410 => ("policy_source_not_found", "The configured version policy source has no policy at this URL; check the profile's policy_url"),
        429 => ("policy_source_busy", "The configured version policy source is rate limiting requests; retry later"),
        500..=599 => ("policy_source_unavailable", "The configured version policy source is unavailable; retry later"),
        _ => ("policy_source_error", "The configured version policy source refused the request"),
    }
}
fn safe_error(value: &Value) -> (&'static str, &'static str) {
    match value["error"]["code"].as_str().unwrap_or("") {
        "funding_pending" => ("funding_pending", "Funding is pending eSpace safe acceptance; resume the same account and original transaction"),
        "funding_rejected" | "invalid_payment" => ("funding_rejected", "The payment was rejected; inspect funding history before sending anything else"),
        "settlement_unavailable" => ("settlement_unavailable", "Settlement verification is unavailable; no new credit is confirmed"),
        "funding_orphaned" | "account_frozen" => ("account_frozen", "Funding was orphaned or the account is frozen; inspect its receipt and funding history"),
        "insufficient_balance" | "insufficient_funds" => ("insufficient_balance", "Prepaid balance cannot cover the requested maximum usage"),
        "duplicate_request" => ("duplicate_request", "The request already exists and was not replayed; inspect its durable status"),
        "engine_timeout" | "tokenization_timeout" => ("engine_timeout", "The engine timed out; inspect request status and usage before retrying"),
        "engine_error" | "engine_usage_mismatch" | "engine_unavailable" | "upstream_error" | "upstream_failure" => ("engine_unavailable", "The provider engine failed; inspect usage before creating another request"),
        "provider_paused" | "provider_draining" | "model_unavailable" | "model_unqualified" | "model_revision_unavailable" => ("provider_unavailable", "The selected provider or exact model revision is unavailable; no automatic rerouting"),
        "engine_saturated" | "model_saturated" | "rate_limit" | "rate_limit_exceeded" | "saturation" => ("rate_limit", "Provider or account capacity is exhausted"),
        "invalid_api_key" | "invalid_credential" | "unauthorized" | "credential_expired" | "credential_revoked" => ("invalid_credential", "The credential is invalid, expired or revoked; rotate it with the wallet owner"),
        "credential_spend_cap" => ("credential_spend_cap", "This application's credential spending cap cannot cover the requested maximum usage"),
        "account_forbidden" | "account_paused" | "credential_scope" => ("account_forbidden", "The account is paused or the credential lacks the required scope"),
        "request_cancelled" => ("request_cancelled", "Inference was cancelled; retrieve authoritative usage"),
        "expired_quote" => ("expired_quote", "No new intent can be accepted after quote expiry; resume the original intent to recover an existing acknowledgement"),
        "funding_intent_required" => ("funding_intent_required", "A durable provider funding acknowledgement is required before sending payment"),
        "funding_replay" => ("funding_replay", "The original payment is already bound to an account; do not pay again"),
        "request_not_found" | "not_found" => ("not_found", "The requested account, request or offer was not found"),
        "upgrade_required" => ("upgrade_required", "Upgrade the Chiral client before using this provider"),
        "context_length_exceeded" => ("context_length_exceeded", "The tokenized request exceeds the contracted context or output limit"),
        _ => ("provider_error", "Provider operation failed; inspect account and request status before retrying"),
    }
}
async fn observe_provider(client: &Client, url: Url, readiness: bool) -> Value {
    match client
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response_bytes(response)
                .await
                .and_then(|bytes| decode::<Value>(&bytes));
            if readiness {
                json!({"reachable":true,"http_status":status,"ready":body.is_ok_and(|value|value["ready"]==true)})
            } else {
                json!({"reachable":true,"http_status":status,"live":body.is_ok_and(|value|value["status"]=="live")})
            }
        }
        Err(_) => {
            if readiness {
                json!({"reachable":false,"ready":false,"error":"provider probe failed"})
            } else {
                json!({"reachable":false,"live":false,"error":"provider probe failed"})
            }
        }
    }
}
fn account_matches(account: &AccountView, saved: &SavedAccount) -> Result<(), String> {
    let q = &saved.quote.payload;
    if account.contract_id != q.id
        || account.network != q.network
        || account.provider_wallet != q.provider_wallet
        || account.consumer_wallet != q.consumer_wallet
        || account.model != q.model
        || account.price != q.price
        || account.max_concurrency != q.max_concurrency
    {
        return Err("provider account does not match signed terms".into());
    }
    let amount = |value: &str| crate::offer_registry::validate_money(value).map(primitive_types::U256::from);
    let funded = amount(&account.funded_wei)? + amount(&account.provider_loss_wei)?;
    let used = amount(&account.fee_wei)? + amount(&account.reversed_wei)?
        + amount(&account.spent_wei)? + amount(&account.reserved_wei)? + amount(&account.available_wei)?;
    amount(&account.waived_wei)?;
    if funded != used { return Err("provider account monetary invariant failed".into()); }
    Ok(())
}
fn funding_intent_matches(
    quote: &Quote,
    tx_hash: &str,
    intent: &Signed<FundingIntent>,
) -> Result<(), String> {
    protocol::verify(INTENT_DOMAIN, intent, &quote.provider_wallet)?;
    let accepted = &intent.payload;
    if accepted.network != quote.network
        || accepted.provider_wallet != quote.provider_wallet
        || accepted.consumer_wallet != quote.consumer_wallet
        || accepted.quote_id != quote.id
        || accepted.tx_hash != tx_hash
        || accepted.registered_at < quote.issued_at
        || accepted.registered_at >= quote.expires_at
        || accepted.registered_at > protocol::now()?.saturating_add(30)
    {
        return Err(
            "provider funding acknowledgement does not match the original payment and signed quote"
                .into(),
        );
    }
    Ok(())
}
fn funding_stage(saved: &SavedAccount) -> &str {
    if saved.funding_cancellation_requested && saved.account.is_none() {
        if saved.raw_transaction.is_none() {
            return "not_prepared";
        }
        return saved
            .funding_cancellation
            .as_ref()
            .map(|result| result.status.as_str())
            .unwrap_or("cancellation_requested");
    }
    if let Some(topup) = &saved.topup {
        return if topup.cancellation_requested {
            "topup_cancellation_requested"
        } else if topup.raw_transaction.is_some() {
            "topup_settlement_pending"
        } else {
            "topup_preparation_pending"
        };
    }
    if saved.account.is_some() {
        "active"
    } else if saved.quote.payload.funding_wei == "0" {
        "free_open_pending"
    } else if saved.funding_broadcast_started {
        "settlement_pending"
    } else if saved.funding_intent.is_some() {
        "acknowledged"
    } else if saved.raw_transaction.is_some() {
        "registration_pending"
    } else {
        "preparation_pending"
    }
}

/// Resolves every relative path-valued field of a consumer profile against the profile's
/// directory, as provider, relay and RPC-gateway configurations already are (book line 449).
fn resolve_profile_paths(config: &mut ConsumerConfig, base: &Path) {
    use crate::provider_config::resolve_path;
    resolve_path(base, &mut config.data_dir);
    if let Some(path) = config.network.rpc_bearer_file.as_mut() {
        resolve_path(base, path);
    }
    if let Some(dht) = config.dht.as_mut() {
        resolve_path(base, &mut dht.identity_file);
    }
}

/// Before profile paths were resolved against the profile, a relative path named a place in the
/// process working directory. State found only there is never silently replaced by a fresh empty
/// location, and is never moved here: the profile is refused, naming both paths (book lines 470,
/// 472 and 652). The check looks only at the two candidate locations of each relative field.
fn refuse_stranded_profile_state(
    profile: &Path,
    unresolved: &ConsumerConfig,
    resolved: &ConsumerConfig,
) -> Result<(), String> {
    let present = |path: &Path| fs::symlink_metadata(path).is_ok();
    let shown = |path: &Path| {
        std::path::absolute(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string()
    };
    let profile = shown(profile);
    let (old, new) = (&unresolved.data_dir, &resolved.data_dir);
    if old.is_relative() && old != new {
        let old_state = unresolved.network.scoped_dir(old);
        let new_state = resolved.network.scoped_dir(new);
        if present(&old_state) && !present(&new_state) {
            return Err(format!(
                "consumer profile {profile}: relative data_dir {:?} now resolves against the profile's directory to {}, which holds no state for this network, but this network's existing state is only under the working-directory location {} ({}). Refusing to start with empty state; nothing was created, moved or deleted. Either set data_dir to the absolute path {}, or stop every process that uses that state and deliberately move {} to {}, keeping its wallet key, nonce journal, agreements and policy watermark together",
                old,
                shown(new),
                shown(old),
                shown(&old_state),
                shown(old),
                shown(&old_state),
                shown(&new_state),
            ));
        }
        // Any load, even a read-only command from another working directory, creates the
        // network-scoped directories, an empty journal and the policy directory at the resolved
        // location. That scaffold is not this profile's wallet: while the consumer wallet key
        // exists only at the working-directory location, the profile is still refused, so a later
        // `wallet create` cannot silently start a second wallet beside the old one.
        let old_key = old_state.join("consumer").join("wallet.key");
        let new_key = new_state.join("consumer").join("wallet.key");
        if present(&old_key) && !present(&new_key) {
            return Err(format!(
                "consumer profile {profile}: relative data_dir {:?} now resolves against the profile's directory to {}, which holds no consumer wallet key for this network ({} is absent), but this network's consumer wallet key exists only under the working-directory location {} ({}). Refusing to start without that wallet; nothing was created, moved or deleted. Either set data_dir to the absolute path {}, or stop every process that uses either location, set aside what is already at {}, and deliberately move {} to {}, keeping its wallet key, nonce journal, agreements and policy watermark together",
                old,
                shown(new),
                shown(&new_key),
                shown(old),
                shown(&old_key),
                shown(old),
                shown(&new_state),
                shown(&old_state),
                shown(&new_state),
            ));
        }
    }
    let files = [
        (
            "network.rpc_bearer_file",
            unresolved.network.rpc_bearer_file.as_ref(),
            resolved.network.rpc_bearer_file.as_ref(),
        ),
        (
            "dht.identity_file",
            unresolved.dht.as_ref().map(|dht| &dht.identity_file),
            resolved.dht.as_ref().map(|dht| &dht.identity_file),
        ),
    ];
    for (field, old, new) in files {
        let (Some(old), Some(new)) = (old, new) else {
            continue;
        };
        if old.is_relative() && old != new && present(old) && !present(new) {
            return Err(format!(
                "consumer profile {profile}: relative {field} {:?} now resolves against the profile's directory to {}, which does not exist, but the file exists only at the working-directory location {}. Refusing to start without it; nothing was created, moved or deleted. Either set {field} to the absolute path {}, or deliberately move that file to {}",
                old,
                shown(new),
                shown(old),
                shown(old),
                shown(new),
            ));
        }
    }
    Ok(())
}

impl Consumer {
    /// Loads a consumer profile. Relative paths in it are resolved against the profile's
    /// directory, so one profile reaches one `data_dir`, wallet, agreement store, policy watermark
    /// and nonce journal from any working directory (book lines 449 and 470).
    pub fn load(path: &Path) -> Result<Self, String> {
        let unresolved: ConsumerConfig =
            decode(&fs::read(path).map_err(|_| "cannot read consumer config")?)?;
        let mut config = unresolved.clone();
        resolve_profile_paths(&mut config, path.parent().unwrap_or_else(|| Path::new(".")));
        refuse_stranded_profile_state(path, &unresolved, &config)?;
        Self::new(config)
    }
    pub fn new(config: ConsumerConfig) -> Result<Self, String> {
        if let Some(url) = &config.catalog_url {
            endpoint_url(url, &[], config.development)?;
        }
        let root = config.network.scoped_dir(&config.data_dir).join("consumer");
        wallet::secure_directory(&root)?;
        let policy_directory = config
            .network
            .scoped_dir(&config.data_dir)
            .join("version-policy");
        wallet::secure_directory(&policy_directory)?;
        crate::version::restore_persisted_policy(&policy_directory)?;
        let chain = ChainClient::new(config.network.clone(), config.data_dir.clone())?;
        Ok(Self {
            config,
            root,
            chain,
            mutation: Mutex::new(()),
        })
    }
    pub fn network_config(&self) -> &NetworkConfig {
        &self.config.network
    }
    pub fn network_profile(&self) -> Value {
        json!({"identity":self.config.network.identity,"rpc_url":self.config.network.rpc_url,
            "faucet_url":self.config.network.faucet_url,"explorer_url":self.config.network.explorer_url,
            "catalog_url":self.config.catalog_url})
    }
    pub async fn provider_info(&self, offer: &Signed<InferenceOffer>) -> Result<Value, String> {
        crate::offer_registry::OfferRegistry::new(
            self.config.network.identity.clone(),
            self.config.development,
        )
        .insert(offer.clone())?;
        let observed_at = protocol::now()?;
        let (health, readiness) = match transport(&offer.payload.endpoint, self.config.development)
            .await
        {
            Ok(client) => tokio::join!(
                observe_provider(
                    &client,
                    endpoint_url(
                        &offer.payload.endpoint,
                        &["healthz"],
                        self.config.development
                    )?,
                    false
                ),
                observe_provider(
                    &client,
                    endpoint_url(
                        &offer.payload.endpoint,
                        &["readyz"],
                        self.config.development
                    )?,
                    true
                )
            ),
            Err(_) => (
                json!({"reachable":false,"live":false,"error":"safe provider transport unavailable"}),
                json!({"reachable":false,"ready":false,"error":"safe provider transport unavailable"}),
            ),
        };
        let reputation = if let Some(catalog) = &self.config.catalog_url {
            let result:Result<Value,String>=async {
                let client=transport(catalog,self.config.development).await?;
                let url=endpoint_url(catalog,&["api","ratings",&offer.payload.provider_wallet],self.config.development)?;
                let response=client.get(url).timeout(Duration::from_secs(30)).send().await.map_err(|_|"reputation catalog unavailable")?;
                let value:Value=decode(&catalog_bytes(response).await?)?;
                let network:NetworkIdentity=serde_json::from_value(value["network"].clone()).map_err(|_|"invalid reputation network")?;
                let elo=value["elo"].as_f64().filter(|n|n.is_finite()&&(0.0..=100.0).contains(n)).ok_or("invalid reputation score")?;
                if network!=self.config.network.identity || value["provider_wallet"].as_str()!=Some(offer.payload.provider_wallet.as_str())
                    || value["experimental_testnet"]!=true {return Err("reputation network or provider mismatch".into());}
                let votes=value["vote_count"].as_u64().ok_or("invalid reputation count")?;
                Ok(json!({"network":network,"provider_wallet":offer.payload.provider_wallet,"experimental_testnet":true,
                    "elo":elo,"base_elo":50,"vote_count":votes}))
            }.await;
            match result {
                Ok(snapshot) => json!({"available":true,"snapshot":snapshot}),
                Err(error) => json!({"available":false,"error":error}),
            }
        } else {
            json!({"available":false,"error":"no reputation catalog configured"})
        };
        Ok(
            json!({"observed_at":observed_at,"health":health,"readiness":readiness,"reputation":reputation}),
        )
    }
    fn key_path(&self) -> PathBuf {
        self.root.join("wallet.key")
    }
    fn process_lock(&self) -> Result<crate::file_lock::ExclusiveFileLock, String> {
        let path = self.root.join("consumer.lock");
        let file =
            wallet::create_secret_file(&path).or_else(|_| wallet::open_secret_file(&path))?;
        crate::file_lock::ExclusiveFileLock::try_acquire(file).map_err(|_| {
            "consumer state is busy in another process; retry after it finishes".into()
        })
    }
    fn key(&self) -> Result<String, String> {
        wallet::load_private_key(&self.key_path())
    }
    pub fn address(&self) -> Result<String, String> {
        wallet::address_from_private_key(&self.key()?)
    }
    pub fn wallet_create(&self) -> Result<String, String> {
        let key = wallet::generate_private_key();
        wallet::save_private_key(&self.key_path(), &key)?;
        wallet::address_from_private_key(&key)
    }
    pub fn wallet_import_file(&self, path: &Path) -> Result<String, String> {
        let key = wallet::load_private_key(path)?;
        wallet::save_private_key(&self.key_path(), &key)?;
        wallet::address_from_private_key(&key)
    }
    pub fn wallet_export_file(&self, path: &Path) -> Result<(), String> {
        wallet::save_private_key(path, &self.key()?)
    }
    pub async fn balance(&self) -> Result<Value, String> {
        let address = self.address()?;
        let balance = self
            .chain
            .balance(&address)
            .await
            .map_err(|e| e.to_string())?;
        Ok(json!({"address":address,"balance_wei":balance,"network":self.config.network.identity}))
    }
    pub async fn history(&self) -> Result<Value, String> {
        self.chain
            .history(&self.address()?)
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn network_status(&self) -> Result<Value, String> {
        self.chain.status().await.map_err(|e| e.to_string())
    }
    pub async fn version_status(&self) -> Result<Value, String> {
        let directory = self
            .config
            .network
            .scoped_dir(&self.config.data_dir)
            .join("version-policy");
        crate::version::restore_persisted_policy(&directory)?;
        if let Some(endpoint) = &self.config.network.policy_url {
            let url = Url::parse(endpoint).map_err(|_| "invalid version policy URL")?;
            if url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
                return Err("version policy URL must not contain credentials or fragments".into());
            }
            let origin = url.origin().ascii_serialization();
            let client = transport(&origin, self.config.development).await?;
            let response = client
                .get(url)
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(|_| "configured version policy unavailable")?;
            let policy: crate::version::VersionPolicy =
                decode(&policy_source_bytes(response).await?)?;
            if policy.valid_until != 0 && policy.valid_until <= protocol::now()? {
                return Err("configured version policy is expired".into());
            }
            if !crate::version::update_persisted_policy(&directory, policy)? {
                return Err(
                    "configured version policy failed signature, rollback or tightening validation"
                        .into(),
                );
            }
        }
        let policy = crate::version::effective_policy();
        let status = crate::version::compare_to_policy(crate::version::CURRENT_VERSION, &policy)?;
        Ok(
            json!({"currentVersion":crate::version::CURRENT_VERSION,"status":status,"policy":policy}),
        )
    }
    async fn ensure_version_supported(&self) -> Result<(), String> {
        if self.version_status().await?["status"] == "required" {
            return Err("a client upgrade is required before funding or inference".into());
        }
        Ok(())
    }
    fn account_directory(&self) -> Result<PathBuf, String> {
        let directory = self.root.join(self.address()?);
        wallet::secure_directory(&directory)?;
        Ok(directory)
    }
    fn account_path(&self, id: &str) -> Result<PathBuf, String> {
        protocol::topup_data(id)?;
        Ok(self.account_directory()?.join(format!(
            "{}.json",
            hex::encode(protocol::digest(id.as_bytes()))
        )))
    }
    fn load_account(&self, id: &str) -> Result<SavedAccount, String> {
        let saved: SavedAccount = decode(&private_read(&self.account_path(id)?)?)?;
        if saved.quote.payload.id != id
            || saved.quote.payload.consumer_wallet != self.address()?
            || saved.quote.payload.network != self.config.network.identity
        {
            return Err("saved account belongs to another wallet or network".into());
        }
        self.verify_quote(&saved.offer, &saved.quote, false)?;
        if let Some(intent) = &saved.funding_intent {
            self.verify_funding_intent(&saved, intent)?;
        } else if saved.funding_broadcast_started {
            return Err(
                "saved funding submission has no provider acknowledgement; refusing any payment"
                    .into(),
            );
        }
        Ok(saved)
    }
    fn save_account(&self, saved: &SavedAccount) -> Result<(), String> {
        private_write(
            &self.account_path(&saved.quote.payload.id)?,
            &encode(saved)?,
        )
    }
    pub fn accounts(&self) -> Result<Vec<AccountView>, String> {
        let mut result = Vec::new();
        for entry in
            fs::read_dir(self.account_directory()?).map_err(|_| "cannot list local accounts")?
        {
            let path = entry.map_err(|_| "cannot read local account entry")?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let saved: SavedAccount = decode(&private_read(&path)?)?;
            let saved = self.load_account(&saved.quote.payload.id)?;
            if let Some(account) = saved.account {
                result.push(account);
            }
        }
        Ok(result)
    }
    pub fn pending_accounts(&self) -> Result<Value, String> {
        let mut pending = Vec::new();
        for entry in
            fs::read_dir(self.account_directory()?).map_err(|_| "cannot list pending accounts")?
        {
            let path = entry
                .map_err(|_| "cannot read pending account entry")?
                .path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let saved: SavedAccount = decode(&private_read(&path)?)?;
            let saved = self.load_account(&saved.quote.payload.id)?;
            if saved.account.is_none() || saved.topup.is_some() {
                let topup_hash = saved.topup.as_ref().and_then(|topup| topup.raw_transaction.as_deref())
                    .map(|raw| {
                        let encoded = raw.strip_prefix("0x").ok_or("invalid saved topup transaction")?;
                        hex::decode(encoded).map(|bytes| protocol::hash_hex(&bytes))
                            .map_err(|_| "invalid saved topup transaction")
                    }).transpose()?;
                let tx_hash = if saved.topup.is_some() { topup_hash.as_deref() } else { saved.tx_hash.as_deref() };
                pending.push(json!({"contract_id":saved.quote.payload.id,"provider_wallet":saved.quote.payload.provider_wallet,
                    "model":saved.quote.payload.model.id,"funding_wei":saved.quote.payload.funding_wei,
                    "opening_pending":saved.account.is_none(),"topup_wei":saved.topup.as_ref().map(|t| &t.amount_wei),
                    "tx_hash":tx_hash,"funding_stage":funding_stage(&saved),
                    "cancellation":saved.funding_cancellation,
                    "funding_intent_registered_at":saved.funding_intent.as_ref().map(|intent|intent.payload.registered_at)}));
            }
        }
        Ok(json!({"pending":pending}))
    }
    /// Public, read-only terms of a saved agreement for disclosure before a top-up (A.1.6):
    /// the provider wallet and endpoint from the signed offer, and the platform fee rate
    /// from the signed quote. Contains no credential, key or signed payment bytes.
    pub fn agreement(&self, id: &str) -> Result<Value, String> {
        let saved = self.load_account(id)?;
        let (offer, quote) = (&saved.offer.payload, &saved.quote.payload);
        Ok(json!({"contract_id":quote.id,"provider_wallet":quote.provider_wallet,
            "endpoint":offer.endpoint,"peer_id":offer.peer_id,"model":quote.model.id,
            "fee_bps":quote.fee_bps}))
    }
    pub fn local_requests(&self, id: &str) -> Result<Value, String> {
        self.load_account(id)?;
        let directory = self.account_directory()?.join("requests");
        let mut requests = Vec::new();
        if directory.exists() {
            for entry in fs::read_dir(directory).map_err(|_| "cannot list local request keys")? {
                let path = entry.map_err(|_| "cannot read local request entry")?.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let request: LocalRequest = decode(&private_read(&path)?)?;
                if request.contract_id == id {
                    requests.push(request);
                }
            }
        }
        requests.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        Ok(json!({"requests":requests,"scope":"local_dispatch_keys_not_settlement"}))
    }
    pub async fn discover(
        &self,
        model: Option<&str>,
    ) -> Result<Vec<Signed<InferenceOffer>>, String> {
        let registry = Arc::new(crate::offer_registry::OfferRegistry::new(
            self.config.network.identity.clone(),
            self.config.development,
        ));
        if let Some(dht) = &self.config.dht {
            let service = crate::dht::DhtService::start(
                dht.clone(),
                self.config.network.identity.clone(),
                registry.clone(),
            )
            .await?;
            let discovered = service.discover(model).await;
            service.stop().await;
            for offer in discovered? {
                if model.is_none_or(|id| offer.payload.model.id == id) {
                    let _ = registry.insert(offer);
                }
            }
        } else if let Some(catalog) = &self.config.catalog_url {
            let client = transport(catalog, self.config.development).await?;
            let mut offset = 0usize;
            let mut visited = HashSet::new();
            for _ in 0..100 {
                if !visited.insert(offset) {
                    return Err("catalog pagination loop".into());
                }
                let mut url = endpoint_url(catalog, &["api", "offers"], self.config.development)?;
                url.query_pairs_mut()
                    .append_pair("offset", &offset.to_string())
                    .append_pair("limit", "50");
                if let Some(model) = model {
                    url.query_pairs_mut().append_pair("model", model);
                }
                let response = client
                    .get(url)
                    .timeout(Duration::from_secs(30))
                    .send()
                    .await
                    .map_err(|_| "catalog unreachable")?;
                #[derive(Deserialize)]
                struct Page {
                    offers: Vec<Value>,
                    next_offset: Option<usize>,
                }
                let page: Page = decode(&offer_catalog_bytes(response).await?)?;
                if page.offers.len() > 50 {
                    return Err("catalog exceeded requested page size".into());
                }
                for value in page.offers {
                    if let Ok(offer) = serde_json::from_value::<Signed<InferenceOffer>>(value) {
                        if model.is_none_or(|id| offer.payload.model.id == id) {
                            let _ = registry.insert(offer);
                        }
                    }
                }
                match page.next_offset {
                    Some(next) if next > offset => offset = next,
                    Some(_) => return Err("invalid catalog pagination".into()),
                    None => return Ok(registry.list()),
                }
            }
            return Err("catalog exceeds bounded discovery limit; narrow the model filter".into());
        } else {
            return Err("configure a catalog URL or direct DHT discovery".into());
        }
        Ok(registry.list())
    }
    fn verify_quote(
        &self,
        offer: &Signed<InferenceOffer>,
        quote: &Signed<Quote>,
        require_fresh: bool,
    ) -> Result<(), String> {
        protocol::verify(OFFER_DOMAIN, offer, &offer.payload.provider_wallet)?;
        protocol::verify(QUOTE_DOMAIN, quote, &offer.payload.provider_wallet)?;
        let q = &quote.payload;
        let o = &offer.payload;
        let now = protocol::now()?;
        protocol::topup_data(&q.id)?;
        let funding = q
            .funding_wei
            .parse::<u128>()
            .map_err(|_| "invalid quote funding amount")?;
        let minimum = o
            .min_funding_wei
            .parse::<u128>()
            .map_err(|_| "invalid offer minimum funding")?;
        let free = o.price.input_wei_per_1k == "0" && o.price.output_wei_per_1k == "0";
        if funding.to_string() != q.funding_wei
            || (free && funding != 0)
            || (!free && (funding == 0 || funding < minimum))
        {
            return Err("quote funding does not satisfy the selected offer".into());
        }
        if o.network != self.config.network.identity
            || q.network != o.network
            || q.offer_id != protocol::offer_id(o)?
            || q.provider_wallet != o.provider_wallet
            || q.consumer_wallet != self.address()?
            || q.model != o.model
            || q.price != o.price
            || q.platform_wallet != o.platform_wallet
            || q.fee_bps != o.fee_bps
            || q.max_concurrency != o.max_concurrency
            || q.issued_at > now + 30
            || q.expires_at <= q.issued_at
            || (require_fresh && (q.expires_at <= now || o.expires_at <= now))
        {
            return Err(
                "quote does not match selected offer, wallet, network or validity window".into(),
            );
        }
        endpoint_url(&o.endpoint, &[], self.config.development)?;
        Ok(())
    }
    pub async fn quote(
        &self,
        offer: &Signed<InferenceOffer>,
        funding_wei: &str,
    ) -> Result<Signed<Quote>, String> {
        let registry = crate::offer_registry::OfferRegistry::new(
            self.config.network.identity.clone(),
            self.config.development,
        );
        registry.insert(offer.clone())?;
        let amount = funding_wei
            .parse::<u128>()
            .map_err(|_| "invalid funding wei")?;
        if amount.to_string() != funding_wei {
            return Err("funding wei must be canonical decimal".into());
        }
        let body = ProposeRequest {
            offer_id: protocol::offer_id(&offer.payload)?,
            consumer_wallet: self.address()?,
            funding_wei: funding_wei.into(),
        };
        let client = transport(&offer.payload.endpoint, self.config.development).await?;
        let url = endpoint_url(
            &offer.payload.endpoint,
            &["v1", "contracts", "propose"],
            self.config.development,
        )?;
        let response = client
            .post(url)
            .header("X-Chiral-Client-Version", crate::version::CURRENT_VERSION)
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| "quote request failed")?;
        let quote: Signed<Quote> = decode(&response_bytes(response).await?)?;
        self.verify_quote(offer, &quote, true)?;
        if quote.payload.funding_wei != funding_wei {
            return Err("provider changed requested funding amount".into());
        }
        Ok(quote)
    }
    async fn owner_request(
        &self,
        saved: &SavedAccount,
        method: Method,
        segments: &[&str],
        body: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let base = &saved.offer.payload.endpoint;
        let client = transport(base, self.config.development).await?;
        let url = endpoint_url(base, segments, self.config.development)?;
        let request = ChallengeRequest {
            wallet: self.address()?,
            method: method.to_string(),
            path: url.path().into(),
            body_hash: protocol::hash_hex(&body),
        };
        let response = client
            .post(endpoint_url(
                base,
                &["v1", "auth", "challenge"],
                self.config.development,
            )?)
            .header("X-Chiral-Client-Version", crate::version::CURRENT_VERSION)
            .json(&request)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| "owner challenge unavailable")?;
        let challenge: Challenge = decode(&response_bytes(response).await?)?;
        let payload = crate::auth::AuthPayload {
            network: self.config.network.identity.clone(),
            provider_wallet: saved.quote.payload.provider_wallet.clone(),
            wallet: request.wallet.clone(),
            method: request.method,
            path: request.path,
            body_hash: request.body_hash,
            nonce: challenge.nonce.clone(),
            expires_at: challenge.expires_at,
        };
        let expected = crate::auth::challenge_payload(&payload)?;
        validate_challenge(&challenge, &expected, protocol::now()?)?;
        let signature = wallet::sign_message(&self.key()?, &expected)?;
        let response = client.request(method, url).header("X-Chiral-Owner", request.wallet).header("X-Chiral-Nonce", challenge.nonce)
            .header("X-Chiral-Client-Version",crate::version::CURRENT_VERSION).header("X-Chiral-Signature", signature).header("Content-Type", "application/json").body(body)
            .timeout(Duration::from_secs(120)).send().await.map_err(|_| "control request interrupted; resume with the same account/transaction, never fund again")?;
        response_bytes(response).await
    }
    fn verify_funding_intent(
        &self,
        saved: &SavedAccount,
        intent: &Signed<FundingIntent>,
    ) -> Result<(), String> {
        let quote = &saved.quote.payload;
        let raw = saved
            .raw_transaction
            .as_deref()
            .ok_or("acknowledgement has no original signed payment")?;
        let envelope = self
            .chain
            .validate_payment_envelope(
                raw,
                &quote.consumer_wallet,
                &quote.provider_wallet,
                &quote.funding_wei,
                &protocol::opening_data(quote)?,
            )
            .map_err(|e| e.to_string())?;
        if saved.tx_hash.as_deref() != Some(envelope.tx_hash.as_str()) {
            return Err("saved funding transaction hash differs from its signed bytes".into());
        }
        funding_intent_matches(quote, &envelope.tx_hash, intent)
    }
    pub async fn fund(
        &self,
        offer: &Signed<InferenceOffer>,
        quote: &Signed<Quote>,
    ) -> Result<AccountView, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        self.ensure_version_supported().await?;
        let path = self.account_path(&quote.payload.id)?;
        self.verify_quote(offer, quote, !path.exists())?;
        // Resolve and reject unsafe destinations before any payment can leave the wallet.
        transport(&offer.payload.endpoint, self.config.development).await?;
        if !path.exists() {
            crate::offer_registry::OfferRegistry::new(
                self.config.network.identity.clone(),
                self.config.development,
            )
            .insert(offer.clone())?;
            self.save_account(&SavedAccount {
                offer: offer.clone(),
                quote: quote.clone(),
                account: None,
                raw_transaction: None,
                tx_hash: None,
                funding_intent: None,
                funding_broadcast_started: false,
                funding_cancellation_requested: false,
                funding_cancellation: None,
                credentials: Vec::new(),
                active_credential: None,
                topup: None,
                receipt_sequence: 0,
                won_topups: Vec::new(),
            })?;
        } else {
            let old = self.load_account(&quote.payload.id)?;
            if old.quote.payload != quote.payload || old.quote.signature != quote.signature {
                return Err("account already has different signed terms".into());
            }
        }
        self.resume_inner(&quote.payload.id).await
    }
    pub async fn resume(&self, id: &str) -> Result<AccountView, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        self.resume_inner(id).await
    }
    /// Explicit wallet action only. A provider may broadcast bytes it received without ever
    /// acknowledging them. Recovery spends gas and can lose that race; accepted funding is
    /// non-refundable. Repeated calls retry one journaled replacement, never increase its fee.
    pub async fn cancel_funding(
        &self,
        id: &str,
        operation_id: Option<&str>,
    ) -> Result<Value, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        let mut saved = self.load_account(id)?;
        let opening = format!("open:{id}");
        let prefix = format!("topup:{id}:");
        let pending = saved
            .topup
            .as_ref()
            .map(|intent| format!("{prefix}{}", intent.id));
        let operation=operation_id.or(pending.as_deref()).or_else(||saved.account.is_none().then_some(opening.as_str()))
            .ok_or("no pending topup; choose an exact operation_id from funding recovery history (including the opening after a reorg)")?;
        let is_opening = operation == opening;
        if !is_opening && (!operation.starts_with(&prefix) || operation.len() == prefix.len()) {
            return Err("recovery operation does not belong to this contract".into());
        }
        let original = self
            .chain
            .original_payment_raw(operation, &self.address()?)
            .map_err(|e| e.to_string())?;
        if is_opening {
            if saved
                .raw_transaction
                .as_ref()
                .is_some_and(|raw| Some(raw) != original.as_ref())
            {
                return Err(
                    "original funding journal entry is missing or inconsistent; refusing recovery"
                        .into(),
                );
            }
            if let Some(raw) = &original {
                let q = &saved.quote.payload;
                let tx = self
                    .chain
                    .validate_payment_envelope(
                        raw,
                        &q.consumer_wallet,
                        &q.provider_wallet,
                        &q.funding_wei,
                        &protocol::opening_data(q)?,
                    )
                    .map_err(|e| e.to_string())?;
                saved.raw_transaction = Some(raw.clone());
                saved.tx_hash = Some(tx.tx_hash);
            } else if saved.account.is_some() {
                return Err("no opening payment exists for this saved account".into());
            }
            saved.funding_cancellation_requested = true;
            self.save_account(&saved)?;
        } else if let Some(raw) = &original {
            if pending.as_deref() == Some(operation) {
                let intent = saved.topup.as_mut().ok_or("missing topup intent")?;
                if intent
                    .raw_transaction
                    .as_ref()
                    .is_some_and(|saved_raw| saved_raw != raw)
                {
                    return Err(
                        "original topup journal entry is inconsistent; refusing recovery".into(),
                    );
                }
                self.chain
                    .validate_payment_envelope(
                        raw,
                        &saved.quote.payload.consumer_wallet,
                        &saved.quote.payload.provider_wallet,
                        &intent.amount_wei,
                        &protocol::topup_data(id)?,
                    )
                    .map_err(|e| e.to_string())?;
                intent.raw_transaction = Some(raw.clone());
                // Durable before the attempt, as for an opening: an attempt that fails
                // before journaling a replacement must not let resume, fund or topup
                // broadcast this original again (requirement 16).
                intent.cancellation_requested = true;
                self.save_account(&saved)?;
            }
        } else {
            if pending.as_deref() != Some(operation) {
                return Err("no journaled payment exists for this topup operation".into());
            }
            if saved
                .topup
                .as_ref()
                .and_then(|intent| intent.raw_transaction.as_ref())
                .is_some()
            {
                return Err("original topup journal entry is missing; refusing recovery".into());
            }
            saved.topup = None;
            self.save_account(&saved)?;
        }
        let Some(raw) = original else {
            return Ok(
                json!({"contract_id":id,"operation_id":operation,"status":"not_prepared","message":"No signed payment exists; no transfer was sent"}),
            );
        };
        let result = self
            .chain
            .cancel_payment_once(operation, &self.key()?)
            .await
            .map_err(|e| e.to_string())?;
        if is_opening {
            saved.funding_cancellation = Some(result.clone());
            self.save_account(&saved)?;
        } else {
            // The wallet journal retains BOTH originals and replacements forever, including
            // completed cancellations. An explicit operation ID can recover them after a reorg.
            if result.status == "original_accepted" {
                self.accept_topup(&mut saved, raw).await?;
                mark_topup_credited(&mut saved, operation, &result.original_tx_hash);
                self.save_account(&saved)?;
            }
            if pending.as_deref() == Some(operation)
                && matches!(
                    result.status.as_str(),
                    "original_accepted" | "cancelled" | "original_failed" | "cancellation_failed"
                )
            {
                saved.topup = None;
                self.save_account(&saved)?;
            }
        }
        let mut result = serde_json::to_value(result).map_err(|e| e.to_string())?;
        result["contract_id"] = json!(id);
        Ok(result)
    }
    /// Read-only wallet history for recovery selection, including historical cancellations.
    /// Does not expose signed bytes and never signs, broadcasts or bumps a fee. It durably
    /// records each observed cancellation outcome with the saved agreement, so `resume` can
    /// settle an original that won without cancellation authority.
    pub async fn funding_recovery(&self, id: &str) -> Result<Value, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        let mut saved = self.load_account(id)?;
        self.chain
            .reconcile(&self.address()?)
            .await
            .map_err(|e| e.to_string())?;
        let history = self.history().await?;
        self.record_cancellation_outcomes(&mut saved, &history)?;
        let opening = format!("open:{id}");
        let prefix = format!("topup:{id}:");
        let mut operations = Vec::new();
        for entry in history["transactions"]
            .as_array()
            .ok_or("invalid wallet journal")?
        {
            let operation = entry["operation_id"]
                .as_str()
                .ok_or("invalid wallet operation")?;
            if operation != opening && !operation.starts_with(&prefix) {
                continue;
            }
            let mut entry = entry.clone();
            entry
                .as_object_mut()
                .ok_or("invalid wallet operation")?
                .remove("raw_transaction");
            if let Some(cancellation) = entry["cancellation"].as_object_mut() {
                cancellation.remove("raw_transaction");
            }
            operations.push(entry);
        }
        Ok(json!({"contract_id":id,"operations":operations}))
    }
    /// Durably record what a reconciliation observed for this agreement's explicitly
    /// cancelled operations: the opening's latest outcome, and each top-up original that
    /// won (`original_accepted`). Never signs, broadcasts or contacts the provider. Callers
    /// pass a `history` read after a fresh reconciliation whenever an unopened opening or a
    /// pending top-up has a requested cancellation, because that outcome gates resume.
    fn record_cancellation_outcomes(
        &self,
        saved: &mut SavedAccount,
        history: &Value,
    ) -> Result<Vec<CancelledOperation>, String> {
        let opening = format!("open:{}", saved.quote.payload.id);
        let cancelled = cancelled_operations(&saved.quote.payload.id, history)?;
        let mut changed = false;
        // A requested opening cancellation can have no journaled replacement, for example
        // when the original had already consumed its nonce. Its outcome is then the
        // reconciled original itself, reported exactly as `cancel_payment_once` reports it,
        // and nothing while the original is unresolved. This also withdraws a recorded win
        // that a reorg displaced, so resume never passes its gate on a stale outcome.
        if saved.account.is_none()
            && saved.funding_cancellation_requested
            && !cancelled
                .iter()
                .any(|operation| operation.operation_id == opening)
        {
            if let Some(entry) = journal_entry(history, &opening)? {
                let tx_hash = entry["tx_hash"].as_str().ok_or("invalid wallet operation")?;
                if saved.tx_hash.as_deref() == Some(tx_hash) {
                    let status = match entry["status"].as_str() {
                        Some("accepted") => Some("original_accepted"),
                        Some("failed") => Some("original_failed"),
                        _ => None,
                    };
                    let outcome = status.map(|status| PaymentCancellation {
                        operation_id: opening.clone(),
                        original_tx_hash: tx_hash.into(),
                        replacement_tx_hash: None,
                        status: status.into(),
                        gas_price_wei: None,
                        maximum_gas_cost_wei: None,
                        error: None,
                    });
                    if saved
                        .funding_cancellation
                        .as_ref()
                        .map(|result| result.status.as_str())
                        != status
                    {
                        saved.funding_cancellation = outcome;
                        changed = true;
                    }
                }
            }
        }
        // Likewise for a pending top-up whose cancellation request journaled no
        // replacement: record a won original for resume to settle, and withdraw an
        // uncredited recorded win that a reorg displaced.
        if let Some(intent) = saved
            .topup
            .as_ref()
            .filter(|intent| intent.cancellation_requested)
        {
            let operation = format!("topup:{}:{}", saved.quote.payload.id, intent.id);
            let entry = journal_entry(history, &operation)?.filter(|entry| {
                entry["cancellation"].is_null()
                    && entry["raw_transaction"].as_str() == intent.raw_transaction.as_deref()
            });
            if let Some(entry) = entry {
                let recorded = saved
                    .won_topups
                    .iter()
                    .position(|won| won.operation_id == operation);
                match (
                    reconciled_outcome(entry) == Some("original_accepted"),
                    recorded,
                ) {
                    (true, None) => {
                        saved.won_topups.push(WonTopup {
                            original_tx_hash: entry["tx_hash"]
                                .as_str()
                                .ok_or("invalid wallet operation")?
                                .into(),
                            operation_id: operation,
                            credited: false,
                        });
                        changed = true;
                    }
                    (false, Some(index)) if !saved.won_topups[index].credited => {
                        saved.won_topups.remove(index);
                        changed = true;
                    }
                    _ => {}
                }
            }
        }
        for operation in &cancelled {
            if operation.operation_id == opening {
                // An inconsistent saved original is refused by resume itself; never adopt it.
                if saved
                    .tx_hash
                    .as_ref()
                    .is_some_and(|hash| hash != &operation.tx_hash)
                {
                    continue;
                }
                let observed = serde_json::to_value(&operation.outcome).map_err(|e| e.to_string())?;
                if !saved.funding_cancellation_requested
                    || serde_json::to_value(&saved.funding_cancellation).map_err(|e| e.to_string())?
                        != observed
                {
                    saved.funding_cancellation_requested = true;
                    saved.funding_cancellation = Some(operation.outcome.clone());
                    changed = true;
                }
            } else if operation.outcome.status == "original_accepted"
                && !saved
                    .won_topups
                    .iter()
                    .any(|won| won.operation_id == operation.operation_id)
            {
                saved.won_topups.push(WonTopup {
                    operation_id: operation.operation_id.clone(),
                    original_tx_hash: operation.tx_hash.clone(),
                    credited: false,
                });
                changed = true;
            }
        }
        if changed {
            self.save_account(saved)?;
        }
        Ok(cancelled)
    }
    /// Reconcile this agreement's explicitly cancelled operations only while one of them can
    /// still need settlement: an unopened opening (including one whose cancellation request
    /// journaled no replacement), a pending top-up whose cancellation was requested (with or
    /// without a journaled replacement), or a top-up not yet credited. This is the read-only
    /// reconciliation of `funding-recovery`; it never calls `cancel_payment_once`, so it can
    /// neither sign nor resubmit a replacement nor broadcast an original.
    async fn reconcile_cancelled_operations(
        &self,
        saved: &mut SavedAccount,
    ) -> Result<Vec<CancelledOperation>, String> {
        let id = saved.quote.payload.id.clone();
        let opening = format!("open:{id}");
        let mut history = self.history().await?;
        let unsettled = saved
            .topup
            .as_ref()
            .is_some_and(|intent| intent.cancellation_requested)
            || (saved.account.is_none()
                && saved.funding_cancellation_requested
                && journal_entry(&history, &opening)?.is_some())
            || cancelled_operations(&id, &history)?
                .iter()
                .any(|operation| {
                    if operation.operation_id == opening {
                        saved.account.is_none()
                    } else {
                        !saved.won_topups.iter().any(|won| {
                            won.operation_id == operation.operation_id && won.credited
                        })
                    }
                });
        if unsettled {
            self.chain
                .reconcile(&self.address()?)
                .await
                .map_err(|e| e.to_string())?;
            history = self.history().await?;
        }
        self.record_cancellation_outcomes(saved, &history)
    }
    /// Credit, once, each recorded top-up original that won its cancellation race after its
    /// pending intent was cleared. Submits nothing to the chain: the provider verifies the
    /// exact original bytes and deduplicates credit by transaction.
    async fn settle_won_topups(
        &self,
        saved: &mut SavedAccount,
        observed: &[CancelledOperation],
    ) -> Result<(), String> {
        let id = saved.quote.payload.id.clone();
        let pending = saved
            .topup
            .as_ref()
            .map(|intent| format!("topup:{id}:{}", intent.id));
        for operation in observed {
            // A still-pending top-up settles through `resume_topup`, from the same journal entry.
            if operation.outcome.status != "original_accepted"
                || pending.as_ref() == Some(&operation.operation_id)
            {
                continue;
            }
            let Some(index) = saved
                .won_topups
                .iter()
                .position(|won| won.operation_id == operation.operation_id && !won.credited)
            else {
                continue;
            };
            let raw = self
                .chain
                .original_payment_raw(&operation.operation_id, &self.address()?)
                .map_err(|e| e.to_string())?
                .ok_or("won topup original is missing from the wallet journal; refusing recovery")?;
            let q = &saved.quote.payload;
            let tx = self
                .chain
                .validate_payment_envelope(
                    &raw,
                    &q.consumer_wallet,
                    &q.provider_wallet,
                    &operation.value_wei,
                    &protocol::topup_data(&id)?,
                )
                .map_err(|e| e.to_string())?;
            if tx.tx_hash != operation.tx_hash
                || tx.tx_hash != saved.won_topups[index].original_tx_hash
            {
                return Err("won topup journal entry is inconsistent; refusing recovery".into());
            }
            self.accept_topup(saved, raw).await?;
            saved.won_topups[index].credited = true;
            self.save_account(saved)?;
        }
        Ok(())
    }
    async fn resume_inner(&self, id: &str) -> Result<AccountView, String> {
        let mut saved = self.load_account(id)?;
        // A cancelled operation whose original won (`original_accepted`) is settled here,
        // without cancellation authority and without any call that could submit the
        // replacement (inference-funding-settlement, requirements 16/17).
        let observed = self.reconcile_cancelled_operations(&mut saved).await?;
        // An explicit opening cancellation blocks only that opening's payment
        // (`open:<id>`): no rebroadcast of, or new payment for, it. A saved
        // account still refreshes, and a pending top-up still recovers its
        // exact original bytes (inference-funding-settlement, requirements 4/16).
        let opening_cancelled = saved.funding_cancellation_requested
            && saved
                .funding_cancellation
                .as_ref()
                .map(|result| result.status.as_str())
                != Some("original_accepted");
        if opening_cancelled && saved.account.is_none() {
            if saved.raw_transaction.is_none() && saved.funding_cancellation.is_none() {
                return Err("opening funding was cancelled before any payment was prepared; no transfer was sent and resume will not prepare one. Do not fund this agreement again".into());
            }
            let status = saved
                .funding_cancellation
                .as_ref()
                .map(|result| result.status.as_str())
                .unwrap_or("cancellation_requested");
            return Err(format!("funding recovery was requested for this opening ({status}); resume never broadcasts the cancelled original or signs another payment for it. If the original payment wins, resume settles it with the provider automatically; run funding-recovery {id} to see the outcome. Do not fund this agreement again"));
        }
        if saved.account.is_some() {
            self.settle_won_topups(&mut saved, &observed).await?;
        }
        if saved.topup.is_some() {
            return self.resume_topup(&mut saved).await;
        }
        if saved.quote.payload.funding_wei != "0" && !opening_cancelled {
            let operation = format!("open:{id}");
            let original = self
                .chain
                .original_payment_raw(&operation, &self.address()?)
                .map_err(|e| e.to_string())?;
            if let Some(raw) = &saved.raw_transaction {
                if original.as_deref() != Some(raw.as_str()) {
                    return Err("original funding journal entry is missing or inconsistent; refusing to sign a replacement".into());
                }
            } else if original.is_none() && saved.quote.payload.expires_at <= protocol::now()? {
                return Err("quote expired before preparing payment; no transfer was sent".into());
            }
            // Recover the same signed bytes even after a crash between journal and account persistence.
            let quote = &saved.quote.payload;
            let tx = self
                .chain
                .prepare_payment_once(
                    &operation,
                    &self.key()?,
                    &quote.provider_wallet,
                    &quote.funding_wei,
                    &protocol::opening_data(quote)?,
                )
                .await
                .map_err(|e| e.to_string())?;
            saved.raw_transaction = Some(tx.raw_transaction);
            saved.tx_hash = Some(tx.tx_hash);
            self.save_account(&saved)?;
            if saved.funding_intent.is_none() {
                let request = FundingIntentRequest {
                    raw_transaction: saved
                        .raw_transaction
                        .clone()
                        .ok_or("missing original funding transaction")?,
                };
                let intent: Signed<FundingIntent> = decode(
                    &self
                        .owner_request(
                            &saved,
                            Method::POST,
                            &["v1", "contracts", id, "funding-intent"],
                            encode(&request)?,
                        )
                        .await?,
                )?;
                self.verify_funding_intent(&saved, &intent)?;
                saved.funding_intent = Some(intent);
                // Acknowledgement must reach durable storage before the first possible broadcast.
                self.save_account(&saved)?;
            }
            self.verify_funding_intent(
                &saved,
                saved
                    .funding_intent
                    .as_ref()
                    .ok_or("missing provider acknowledgement")?,
            )?;
            if !saved.funding_broadcast_started {
                saved.funding_broadcast_started = true;
                self.save_account(&saved)?;
            }
            // A requested cancellation passes the gate only with a won original, so there is
            // nothing to broadcast. Never enter a broadcasting path for that opening: a
            // reorg between reconciliations must not rebroadcast it, even when no
            // replacement was journaled. The provider verifies the original when opening.
            if !saved.funding_cancellation_requested {
                let quote = &saved.quote.payload;
                self.chain
                    .send_payment_once(
                        &operation,
                        &self.key()?,
                        &quote.provider_wallet,
                        &quote.funding_wei,
                        &protocol::opening_data(quote)?,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        // A saved account only needs a refresh after original-payment recovery.
        // Opening again cannot recover a lost credential secret.
        if saved.account.is_some() {
            return self.refresh_account(&mut saved).await;
        }
        let request = OpenRequest {
            quote_id: id.into(),
            raw_transaction: saved.raw_transaction.clone(),
        };
        let result: OpenResult = decode(
            &self
                .owner_request(
                    &saved,
                    Method::POST,
                    &["v1", "contracts", "open"],
                    encode(&request)?,
                )
                .await?,
        )?;
        account_matches(&result.account, &saved)?;
        if let Some(credential) = result.credential {
            self.store_credential(&mut saved, credential, true)?;
        }
        saved.account = Some(result.account.clone());
        self.save_account(&saved)?;
        Ok(result.account)
    }
    pub async fn topup(&self, id: &str, amount_wei: &str) -> Result<AccountView, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        self.ensure_version_supported().await?;
        let amount = amount_wei
            .parse::<u128>()
            .map_err(|_| "invalid topup amount")?;
        if amount == 0 || amount.to_string() != amount_wei {
            return Err("topup amount must be positive canonical wei".into());
        }
        let mut saved = self.load_account(id)?;
        if saved.quote.payload.price.input_wei_per_1k == "0"
            && saved.quote.payload.price.output_wei_per_1k == "0"
        {
            return Err("free sessions do not accept paid topups".into());
        }
        if saved.topup.is_none() {
            let current: AccountView = decode(
                &self
                    .owner_request(&saved, Method::GET, &["v1", "contracts", id], Vec::new())
                    .await?,
            )?;
            account_matches(&current, &saved)?;
            if current.status != "active" {
                return Err("account is not active; no topup payment was sent".into());
            }
        }
        match &saved.topup {
            // Never broadcast an original whose cancellation was requested, nor pay again
            // for that operation: only settle its reconciled outcome (requirement 16).
            Some(pending) if pending.cancellation_requested => {
                if pending.amount_wei != amount_wei {
                    return Err("a topup whose cancellation was requested is pending; run resume to settle it (nothing is broadcast for it) before a different topup".into());
                }
                self.reconcile_cancelled_operations(&mut saved).await?;
                return match self.settle_cancelled_topup(&mut saved).await? {
                    CancelledTopup::Credited(account) => Ok(account),
                    CancelledTopup::Resolved(status) => Err(format!("the pending topup's cancellation resolved ({status}) without crediting its original; no payment was sent. Run topup again to send a new payment")),
                };
            }
            Some(pending) if pending.amount_wei != amount_wei => {
                return Err("a different topup is pending; resume its original amount first".into())
            }
            Some(_) => {}
            None => {
                saved.topup = Some(TopupIntent {
                    id: uuid::Uuid::new_v4().to_string(),
                    amount_wei: amount_wei.into(),
                    raw_transaction: None,
                    cancellation_requested: false,
                });
                self.save_account(&saved)?;
            }
        }
        self.resume_topup(&mut saved).await
    }
    async fn resume_topup(&self, saved: &mut SavedAccount) -> Result<AccountView, String> {
        // Every explicit resume re-enters the same journal operation, even after an ambiguous
        // broadcast saved its raw bytes. Never generate a new operation or silently replace it.
        let id = saved.quote.payload.id.clone();
        let intent = saved.topup.as_ref().ok_or("missing topup intent")?;
        let operation = format!("topup:{}:{}", id, intent.id);
        if let Some(raw) = &intent.raw_transaction {
            if self
                .chain
                .original_payment_raw(&operation, &self.address()?)
                .map_err(|e| e.to_string())?
                .as_ref()
                != Some(raw)
            {
                return Err("original topup journal entry is missing or inconsistent; refusing to sign a replacement".into());
            }
        }
        // A requested cancellation blocks this operation's original: never enter
        // `send_payment_once` for it, even when the attempt journaled no replacement
        // (requirement 16). `resume_inner` has just reconciled it.
        if intent.cancellation_requested {
            return match self.settle_cancelled_topup(saved).await? {
                CancelledTopup::Credited(account) => Ok(account),
                CancelledTopup::Resolved(_) => self.refresh_account(saved).await,
            };
        }
        let tx = self
            .chain
            .send_payment_once(
                &operation,
                &self.key()?,
                &saved.quote.payload.provider_wallet,
                &intent.amount_wei,
                &protocol::topup_data(&id)?,
            )
            .await
            .map_err(|e| e.to_string())?;
        if intent
            .raw_transaction
            .as_ref()
            .is_some_and(|raw| raw != &tx.raw_transaction)
        {
            return Err("topup journal returned different signed bytes".into());
        }
        saved
            .topup
            .as_mut()
            .ok_or("missing topup intent")?
            .raw_transaction = Some(tx.raw_transaction);
        self.save_account(saved)?;
        let raw = saved
            .topup
            .as_ref()
            .and_then(|p| p.raw_transaction.clone())
            .ok_or("missing original topup transaction")?;
        let account = self.accept_topup(saved, raw).await?;
        if let Some(won) = saved
            .won_topups
            .iter_mut()
            .find(|won| won.operation_id == operation)
        {
            won.credited = true;
        }
        saved.topup = None;
        self.save_account(saved)?;
        Ok(account)
    }
    /// Settle a pending top-up whose cancellation was requested, from the wallet journal as
    /// last reconciled: callers reconcile first (`reconcile_cancelled_operations`). Never
    /// calls `send_payment_once`, `prepare_payment_once` or `cancel_payment_once`, so it can
    /// neither broadcast the original nor sign or submit a replacement. A won original is
    /// credited once from its exact journaled bytes. An outcome without credit clears the
    /// intent and keeps the journal history. An unresolved request still refreshes the saved
    /// account and is reported as an error, like any other unresolved top-up.
    async fn settle_cancelled_topup(
        &self,
        saved: &mut SavedAccount,
    ) -> Result<CancelledTopup, String> {
        let id = saved.quote.payload.id.clone();
        let intent = saved.topup.as_ref().ok_or("missing topup intent")?;
        let operation = format!("topup:{id}:{}", intent.id);
        let amount = intent.amount_wei.clone();
        let raw = intent
            .raw_transaction
            .clone()
            .ok_or("cancelled topup has no original signed payment; refusing recovery")?;
        let history = self.history().await?;
        let entry = journal_entry(&history, &operation)?
            .filter(|entry| entry["raw_transaction"].as_str() == Some(raw.as_str()))
            .ok_or("original topup journal entry is missing or inconsistent; refusing recovery")?;
        let status = reconciled_outcome(entry)
            .unwrap_or("cancellation_requested")
            .to_string();
        match status.as_str() {
            "original_accepted" => {
                let q = &saved.quote.payload;
                let tx = self
                    .chain
                    .validate_payment_envelope(
                        &raw,
                        &q.consumer_wallet,
                        &q.provider_wallet,
                        &amount,
                        &protocol::topup_data(&id)?,
                    )
                    .map_err(|e| e.to_string())?;
                if entry["tx_hash"].as_str() != Some(tx.tx_hash.as_str()) {
                    return Err("won topup journal entry is inconsistent; refusing recovery".into());
                }
                let account = self.accept_topup(saved, raw).await?;
                mark_topup_credited(saved, &operation, &tx.tx_hash);
                saved.topup = None;
                self.save_account(saved)?;
                Ok(CancelledTopup::Credited(account))
            }
            "original_failed" | "cancelled" | "cancellation_failed" => {
                saved.topup = None;
                self.save_account(saved)?;
                Ok(CancelledTopup::Resolved(status))
            }
            _ => {
                let refresh = match self.refresh_account(saved).await {
                    Ok(_) => String::new(),
                    Err(error) => format!(" The account refresh also failed: {error}"),
                };
                Err(format!("cancellation was requested for pending topup {operation} ({status}); resume, fund and topup never broadcast its original or sign another payment for it. If the original payment wins, resume credits it automatically; run funding-recovery {id} to see the outcome, or retry cancel-funding to resubmit the cancellation. Do not send this topup again.{refresh}"))
            }
        }
    }
    async fn accept_topup(
        &self,
        saved: &mut SavedAccount,
        raw: String,
    ) -> Result<AccountView, String> {
        let id = &saved.quote.payload.id;
        let account: AccountView = decode(
            &self
                .owner_request(
                    saved,
                    Method::POST,
                    &["v1", "contracts", id, "topup"],
                    encode(&TopupRequest {
                        raw_transaction: raw,
                    })?,
                )
                .await?,
        )?;
        account_matches(&account, saved)?;
        saved.account = Some(account.clone());
        self.save_account(saved)?;
        Ok(account)
    }
    pub async fn account(&self, id: &str) -> Result<AccountView, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        let mut saved = self.load_account(id)?;
        self.refresh_account(&mut saved).await
    }
    async fn refresh_account(&self, saved: &mut SavedAccount) -> Result<AccountView, String> {
        let id = &saved.quote.payload.id;
        let account = decode(
            &self
                .owner_request(saved, Method::GET, &["v1", "contracts", id], Vec::new())
                .await?,
        )?;
        account_matches(&account, saved)?;
        saved.account = Some(account.clone());
        self.save_account(saved)?;
        Ok(account)
    }
    pub async fn usage(&self, id: &str) -> Result<Value, String> {
        let saved = self.load_account(id)?;
        decode(
            &self
                .owner_request(
                    &saved,
                    Method::GET,
                    &["v1", "contracts", id, "requests"],
                    Vec::new(),
                )
                .await?,
        )
    }
    pub async fn funding(&self, id: &str) -> Result<Value, String> {
        let saved = self.load_account(id)?;
        decode(
            &self
                .owner_request(
                    &saved,
                    Method::GET,
                    &["v1", "contracts", id, "funding"],
                    Vec::new(),
                )
                .await?,
        )
    }
    pub async fn receipt(&self, id: &str) -> Result<Signed<UsageReceipt>, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        let mut saved = self.load_account(id)?;
        let receipt: Signed<UsageReceipt> = decode(
            &self
                .owner_request(
                    &saved,
                    Method::GET,
                    &["v1", "contracts", id, "receipt"],
                    Vec::new(),
                )
                .await?,
        )?;
        protocol::verify(
            RECEIPT_DOMAIN,
            &receipt,
            &saved.quote.payload.provider_wallet,
        )?;
        account_matches(&receipt.payload.account, &saved)?;
        if receipt.payload.network != self.config.network.identity
            || receipt.payload.provider_wallet != saved.quote.payload.provider_wallet
            || receipt.payload.sequence <= saved.receipt_sequence
            || receipt.payload.issued_at > protocol::now()? + 30
        {
            return Err("receipt network, identity, sequence or time mismatch".into());
        }
        if receipt.payload.requests.len() > 100 {
            return Err("receipt request page exceeds bound".into());
        }
        let mut ids = std::collections::HashSet::new();
        for request in &receipt.payload.requests {
            if request.contract_id != saved.quote.payload.id || !ids.insert(&request.id) {
                return Err("receipt request identity mismatch".into());
            }
            let cost = crate::offer_registry::validate_money(&request.cost_wei)?;
            let waived = crate::offer_registry::validate_money(&request.waived_wei)?;
            let maximum = crate::offer_registry::validate_money(&request.max_cost_wei)?;
            crate::offer_registry::validate_money(&request.provider_loss_wei)?;
            if cost > maximum || waived > maximum || (waived != 0 && cost != 0) {
                return Err("receipt request charge/waiver invariant failed".into());
            }
            if let Some(measured) = &request.performance {
                match &saved.quote.payload.model.serving {
                    Some(profile) => {
                        // Renewal keeps account terms but binds a newer report per request.
                        // This signed digest is attribution, not qualification attestation.
                        if measured.evidence_sha256.len() != 64
                            || !measured.evidence_sha256.bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                            || measured.policy != profile.policy
                            || (measured.attained == Some(false)) != (measured.remedy == "inference_waiver")
                            || (measured.remedy == "inference_waiver" && cost != 0) {
                            return Err("receipt performance identity or remedy mismatch".into());
                        }
                    }
                    None if measured.policy != "unqualified" || measured.attained.is_some() || measured.remedy != "none" => {
                        return Err("receipt claims an uncontracted guarantee".into());
                    }
                    None => {}
                }
                if measured.remedy == "inference_waiver" && !request.status.ends_with("unknown_usage")
                    && waived != crate::billing_store::usage_cost(&saved.quote.payload.price, &request.usage)? {
                    return Err("receipt waiver does not preserve authoritative measured usage".into());
                }
            } else if saved.quote.payload.model.serving.is_some() && request.status != "reserved" {
                return Err("receipt omits contracted performance outcome".into());
            }
        }
        saved.receipt_sequence = receipt.payload.sequence;
        self.save_account(&saved)?;
        Ok(receipt)
    }
    fn store_credential(
        &self,
        saved: &mut SavedAccount,
        credential: Credential,
        active: bool,
    ) -> Result<(), String> {
        if credential.contract_id != saved.quote.payload.id
            || credential.bearer.is_empty()
            || credential.expires_at <= protocol::now()?
        {
            return Err("invalid issued credential".into());
        }
        if active {
            // Selecting a replacement does not erase other locally exported credentials.
            saved.active_credential = Some(credential.id.clone());
        }
        saved.credentials.retain(|c| c.id != credential.id);
        saved.credentials.push(credential);
        Ok(())
    }
    pub async fn credentials(&self, id: &str) -> Result<Value, String> {
        let saved = self.load_account(id)?;
        let views: Vec<crate::billing_store::CredentialView> = decode(
            &self
                .owner_request(
                    &saved,
                    Method::GET,
                    &["v1", "contracts", id, "credentials"],
                    Vec::new(),
                )
                .await?,
        )?;
        serde_json::to_value(views).map_err(|e| e.to_string())
    }
    pub async fn credential_issue(&self, id: &str, options: Value) -> Result<Value, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        let mut saved = self.load_account(id)?;
        let options: crate::billing_store::CredentialIssueRequest =
            serde_json::from_value(options).map_err(|_| "invalid credential options")?;
        let views: Vec<crate::billing_store::CredentialView> = decode(
            &self
                .owner_request(
                    &saved,
                    Method::GET,
                    &["v1", "contracts", id, "credentials"],
                    Vec::new(),
                )
                .await?,
        )?;
        let now = protocol::now()?;
        let stored_default = saved.active_credential.as_ref().and_then(|active| {
            saved
                .credentials
                .iter()
                .find(|c| &c.id == active && c.expires_at > now)
        });
        // Issuing restores a missing usable default without replacing a valid one (book
        // line 495). The provider's list is bounded, so a default missing from it is
        // checked directly instead of being treated as revoked.
        let valid_default = match stored_default {
            None => false,
            Some(stored) => match views.iter().find(|c| c.id == stored.id) {
                Some(view) => {
                    view.contract_id == id
                        && !view.revoked
                        && view.expires_at > now
                        && view.scopes.iter().any(|scope| scope == "infer")
                }
                None => self.default_still_infers(&saved, stored).await?,
            },
        };
        if !valid_default {
            saved.active_credential = None;
        }
        let activate = !valid_default && options.scopes.iter().any(|scope| scope == "infer");
        let credential: Credential = decode(
            &self
                .owner_request(
                    &saved,
                    Method::POST,
                    &["v1", "contracts", id, "credentials"],
                    encode(&options)?,
                )
                .await?,
        )?;
        let metadata = credential_metadata(&credential);
        self.store_credential(&mut saved, credential, activate)?;
        self.save_account(&saved)?;
        Ok(metadata)
    }
    /// Whether the stored default, absent from the provider's bounded credential list,
    /// still authorizes inference. The stored bearer goes only to its own provider, over
    /// the same validated transport as inference, on the `infer`-scoped model list. That
    /// route authenticates first and then answers 200 even when the provider is not
    /// accepting, funding is not ready or no model is qualified, so it depends on no
    /// admission state (account-credentials requirement 6). Only 401 `invalid_api_key`
    /// (unknown, revoked or expired) and 403 `credential_scope` (no `infer`) show the
    /// default unusable. A transport failure or any other answer is no evidence either
    /// way: the issuance fails before a credential is created, and the default is kept.
    async fn default_still_infers(
        &self,
        saved: &SavedAccount,
        credential: &Credential,
    ) -> Result<bool, String> {
        const UNCHECKED: &str = "the default credential could not be checked at its provider; no credential was issued and the default is unchanged";
        if credential.contract_id != saved.quote.payload.id {
            return Ok(false);
        }
        let base = &saved.offer.payload.endpoint;
        let client = transport(base, self.config.development).await?;
        let response = client
            .get(endpoint_url(base, &["v1", "models"], self.config.development)?)
            .bearer_auth(&credential.bearer)
            .header("X-Chiral-Client-Version", crate::version::CURRENT_VERSION)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| UNCHECKED)?;
        let (status, body) = bounded_body(response).await.map_err(|_| UNCHECKED)?;
        let document = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
        match (status.as_u16(), document["error"]["code"].as_str()) {
            (200, _) if document["object"] == "list" && document["data"].is_array() => Ok(true),
            (401, Some("invalid_api_key")) | (403, Some("credential_scope")) => Ok(false),
            _ => Err(UNCHECKED.into()),
        }
    }
    pub async fn credential_revoke(&self, id: &str, credential_id: &str) -> Result<Value, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        let mut saved = self.load_account(id)?;
        let result = decode(
            &self
                .owner_request(
                    &saved,
                    Method::DELETE,
                    &["v1", "contracts", id, "credentials", credential_id],
                    Vec::new(),
                )
                .await?,
        )?;
        saved.credentials.retain(|c| c.id != credential_id);
        if saved.active_credential.as_deref() == Some(credential_id) {
            saved.active_credential = None;
        }
        self.save_account(&saved)?;
        Ok(result)
    }
    pub async fn credential_rotate(
        &self,
        id: &str,
        credential_id: &str,
        options: Value,
    ) -> Result<Value, String> {
        let _guard = self.mutation.lock().await;
        let _file_guard = self.process_lock()?;
        let mut saved = self.load_account(id)?;
        let options: crate::billing_store::CredentialIssueRequest =
            serde_json::from_value(options).map_err(|_| "invalid credential options")?;
        let credential: Credential = decode(
            &self
                .owner_request(
                    &saved,
                    Method::POST,
                    &[
                        "v1",
                        "contracts",
                        id,
                        "credentials",
                        credential_id,
                        "rotate",
                    ],
                    encode(&options)?,
                )
                .await?,
        )?;
        let metadata = credential_metadata(&credential);
        let was_active = saved.active_credential.as_deref() == Some(credential_id);
        if was_active {
            saved.active_credential = None;
        }
        let active = was_active && options.scopes.iter().any(|scope| scope == "infer");
        saved.credentials.retain(|c| c.id != credential_id);
        self.store_credential(&mut saved, credential, active)?;
        self.save_account(&saved)?;
        Ok(metadata)
    }
    /// Explicit secret export only. Normal list/issue/rotate outputs contain metadata only.
    /// An SDK reaches `base_url` through its own HTTP stack, without the pinned client, so
    /// the saved origin must pass the full origin rule under the current development mode
    /// before a credential is selected or returned. Loading stays lenient so that recovery
    /// remains reachable.
    pub fn credential_export(
        &self,
        id: &str,
        credential_id: Option<&str>,
    ) -> Result<Value, String> {
        let saved = self.load_account(id)?;
        crate::offer_registry::validate_endpoint(
            &saved.offer.payload.endpoint,
            self.config.development,
        )?;
        let credential = self.credential(&saved, credential_id)?;
        let base = endpoint_url(
            &saved.offer.payload.endpoint,
            &["v1"],
            self.config.development,
        )?;
        Ok(
            json!({"api_key":credential.bearer,"base_url":base.as_str(),"model":saved.quote.payload.model.id,"contract_id":id,"credential_id":credential.id}),
        )
    }
    fn credential<'a>(
        &self,
        saved: &'a SavedAccount,
        id: Option<&str>,
    ) -> Result<&'a Credential, String> {
        let id = id.or(saved.active_credential.as_deref()).ok_or(
            "no active inference credential; explicitly issue an inference-capable credential",
        )?;
        let credential = saved
            .credentials
            .iter()
            .find(|c| c.id == id)
            .ok_or("credential is not stored locally")?;
        if credential.expires_at <= protocol::now()? {
            return Err("credential expired; explicitly rotate it or issue an inference-capable replacement".into());
        }
        Ok(credential)
    }
    async fn inference_response(
        &self,
        id: &str,
        kind: &str,
        mut body: Value,
        stream: bool,
    ) -> Result<Response, String> {
        self.ensure_version_supported().await?;
        let saved = self.load_account(id)?;
        let credential = self.credential(&saved, None)?;
        let path = match kind {
            "chat" => vec!["v1", "chat", "completions"],
            "completions" => vec!["v1", "completions"],
            "embeddings" => vec!["v1", "embeddings"],
            _ => return Err("unsupported inference API".into()),
        };
        let object = body
            .as_object_mut()
            .ok_or("request must be a JSON object")?;
        if let Some(model) = object.get("model") {
            if model.as_str() != Some(saved.quote.payload.model.id.as_str()) {
                return Err("request model differs from selected account".into());
            }
        }
        object.insert("model".into(), json!(saved.quote.payload.model.id));
        if kind != "embeddings" {
            object.insert("stream".into(), json!(stream));
        }
        if stream {
            object.insert("stream_options".into(), json!({"include_usage":true}));
        }
        let client = transport(&saved.offer.payload.endpoint, self.config.development).await?;
        // Persist the ambiguity lookup key, not the prompt or output, before dispatch.
        let request_key = uuid::Uuid::new_v4().to_string();
        let intent_path = self
            .account_directory()?
            .join("requests")
            .join(format!("{request_key}.json"));
        private_write(
            &intent_path,
            &encode(
                &json!({"contract_id":id,"request_key":request_key,"created_at":protocol::now()?}),
            )?,
        )?;
        // A new user invocation is a new paid request. No automatic retries or provider rerouting.
        client.post(endpoint_url(&saved.offer.payload.endpoint,&path,self.config.development)?)
            .bearer_auth(&credential.bearer).header("X-Chiral-Client-Version",crate::version::CURRENT_VERSION).header("Idempotency-Key",&request_key).json(&body)
            .send().await.map_err(|_|format!("inference dispatch interrupted; inspect request {request_key} before creating another request"))
    }
    pub async fn infer(&self, id: &str, kind: &str, request: Value) -> Result<Value, String> {
        let response = tokio::time::timeout(
            Duration::from_secs(3600),
            self.inference_response(id, kind, request, false),
        )
        .await
        .map_err(|_| "inference timed out; inspect usage before retrying")??;
        decode(
            &tokio::time::timeout(Duration::from_secs(3600), response_bytes(response))
                .await
                .map_err(|_| "inference response timed out; inspect usage before retrying")??,
        )
    }
    pub async fn request_status(&self, id: &str, request_id: &str) -> Result<Value, String> {
        self.request_control(id, request_id, false).await
    }
    pub async fn cancel_request(&self, id: &str, request_id: &str) -> Result<Value, String> {
        self.request_control(id, request_id, true).await
    }
    async fn request_control(
        &self,
        id: &str,
        request_id: &str,
        cancel: bool,
    ) -> Result<Value, String> {
        let saved = self.load_account(id)?;
        let credential = self.credential(&saved, None)?;
        let mut path = vec!["v1", "requests", request_id];
        if cancel {
            path.push("cancel");
        }
        let client = transport(&saved.offer.payload.endpoint, self.config.development).await?;
        let response = client
            .request(
                if cancel { Method::POST } else { Method::GET },
                endpoint_url(
                    &saved.offer.payload.endpoint,
                    &path,
                    self.config.development,
                )?,
            )
            .bearer_auth(&credential.bearer)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| "request status/cancellation unavailable")?;
        decode(&response_bytes(response).await?)
    }
    pub async fn stream_kind(
        &self,
        id: &str,
        kind: &str,
        request: Value,
    ) -> Result<InferenceStream, String> {
        if !matches!(kind, "chat" | "completions") {
            return Err("only chat and completions support streaming".into());
        }
        let response = tokio::time::timeout(
            Duration::from_secs(120),
            self.inference_response(id, kind, request, true),
        )
        .await
        .map_err(|_| "stream dispatch timed out; inspect usage before retrying")??;
        if !response.status().is_success() {
            response_bytes(response).await?;
            return Err("unexpected stream status".into());
        }
        if !response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.starts_with("text/event-stream"))
        {
            return Err("provider did not return an SSE stream; inspect usage".into());
        }
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let (sender, events) = mpsc::channel(16);
        let (cancel_sender, mut cancellation) = watch::channel(false);
        tokio::spawn(async move {
            let _ = sender
                .send(StreamEvent {
                    kind: "started".into(),
                    data: json!({"request_id":request_id}),
                })
                .await;
            let mut chunks = response.bytes_stream();
            let mut parser = SseParser::default();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(3600);
            let mut total_bytes = 0usize;
            loop {
                tokio::select! {
                    _=sender.closed()=>break,
                    _=cancellation.changed()=>{let _=sender.try_send(StreamEvent{kind:"cancelled".into(),data:Value::Null});break;},
                    _=tokio::time::sleep_until(deadline)=>{
                        let event=StreamEvent{kind:"error".into(),data:json!({"message":"stream duration limit reached; inspect usage"})};
                        tokio::select!{_=cancellation.changed()=>{},_=tokio::time::timeout(Duration::from_secs(1),sender.send(event))=>{}}
                        break;
                    },
                    chunk=tokio::time::timeout(Duration::from_secs(120),chunks.next())=>{
                        let result=match chunk {
                            Ok(Some(Ok(chunk)))=>{
                                total_bytes=total_bytes.saturating_add(chunk.len());
                                if total_bytes>8*1024*1024 {Err("stream response exceeds cumulative limit; inspect usage".into())}
                                else {parser.push(&chunk)}
                            },
                            Ok(None)=>Err("stream ended without [DONE]; inspect usage".into()),
                            Ok(Some(Err(_)))=>Err("stream interrupted; inspect usage, do not replay".into()),
                            Err(_)=>Err("stream idle timeout; inspect usage".into())
                        };
                        match result {
                            Ok(frames)=>{
                                let mut done=false;
                                for frame in frames {
                                    let event=if frame=="[DONE]" {done=true;StreamEvent{kind:"done".into(),data:Value::Null}} else {
                                        match serde_json::from_str::<Value>(&frame) {
                                            Ok(data) if data.get("error").is_some() => {done=true;let(code,message)=safe_error(&data);StreamEvent{kind:"error".into(),data:json!({"code":code,"message":message})}},
                                            Ok(data)=>StreamEvent{kind:"chunk".into(),data},
                                            Err(_)=>{done=true;StreamEvent{kind:"error".into(),data:json!({"message":"invalid SSE JSON"})}}
                                        }
                                    };
                                    tokio::select!{_ = sender.closed()=>return,_=cancellation.changed()=>return,_=tokio::time::sleep_until(deadline)=>return,result=sender.send(event)=>{if result.is_err(){return;}}}
                                    if done { break; }
                                }
                                if done {break;}
                            },
                            Err(message)=>{
                                let event=StreamEvent{kind:"error".into(),data:json!({"message":message})};
                                tokio::select!{_=cancellation.changed()=>{},_=tokio::time::timeout(Duration::from_secs(1),sender.send(event))=>{}}
                                break;
                            }
                        }
                    }
                }
            }
            // Dropping the response stream disconnects the gateway, which cancels its engine request.
        });
        Ok(InferenceStream {
            events,
            cancel: Cancellation {
                sender: cancel_sender,
            },
        })
    }
    pub async fn feedback(&self, id: &str, score: u8, revision: u64) -> Result<Value, String> {
        if score > 100 || revision == 0 {
            return Err("feedback score must be 0..100 and revision positive".into());
        }
        let saved = self.load_account(id)?;
        let catalog = self
            .config
            .catalog_url
            .as_ref()
            .ok_or("feedback requires a configured reputation catalog")?;
        let raw = saved
            .raw_transaction
            .as_ref()
            .ok_or("free sessions cannot submit payment-backed feedback")?;
        let feedback = crate::reputation::Feedback {
            network: self.config.network.identity.clone(),
            provider_wallet: saved.quote.payload.provider_wallet.clone(),
            consumer_wallet: self.address()?,
            quote_id: id.into(),
            tx_hash: saved
                .tx_hash
                .clone()
                .ok_or("missing funding transaction hash")?,
            score,
            revision,
            issued_at: protocol::now()?,
        };
        let body = json!({"feedback":protocol::sign(FEEDBACK_DOMAIN,feedback,&self.key()?)?,"quote":saved.quote,"raw_transaction":raw});
        let client = transport(catalog, self.config.development).await?;
        let response = client
            .post(endpoint_url(
                catalog,
                &["api", "ratings", "feedback"],
                self.config.development,
            )?)
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|_| "feedback submission failed")?;
        decode(&catalog_bytes(response).await?)
    }
    pub async fn provider_admin(
        &self,
        endpoint: &str,
        token_file: &Path,
        action: &str,
    ) -> Result<Value, String> {
        let url = Url::parse(endpoint).map_err(|_| "invalid admin URL")?;
        let loopback = match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if !loopback {
            return Err("administration requires a literal loopback endpoint".into());
        }
        let (method, path) = match action {
            "status" => (Method::GET, vec!["admin", "status"]),
            "offers" => (Method::GET, vec!["admin", "offers"]),
            "accounts" => (Method::GET, vec!["admin", "accounts"]),
            "fees" => (Method::GET, vec!["admin", "fees"]),
            "requests" => (Method::GET, vec!["admin", "requests"]),
            "drain" => (Method::POST, vec!["admin", "drain"]),
            "resume" => (Method::POST, vec!["admin", "resume"]),
            "refresh" => (Method::POST, vec!["admin", "offers", "refresh"]),
            "remit" => (Method::POST, vec!["admin", "fees", "remit"]),
            _ => return Err("unknown provider admin action".into()),
        };
        let bytes = private_read(token_file)?;
        let token = std::str::from_utf8(&bytes)
            .map_err(|_| "invalid admin token file")?
            .trim();
        if token.is_empty() {
            return Err("empty admin token".into());
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| "cannot create admin client")?;
        let mut request = client
            .request(method.clone(), endpoint_url(endpoint, &path, true)?)
            .bearer_auth(token)
            .timeout(Duration::from_secs(120));
        if method == Method::POST {
            request = request.json(&json!({}));
        }
        let response = request
            .send()
            .await
            .map_err(|_| "local provider admin unavailable")?;
        decode(&response_bytes(response).await?)
    }
}
fn credential_metadata(credential: &Credential) -> Value {
    json!({"id":credential.id,"contract_id":credential.contract_id,"expires_at":credential.expires_at})
}
fn validate_challenge(challenge: &Challenge, expected: &[u8], now: u64) -> Result<(), String> {
    if challenge.expires_at <= now
        || challenge.expires_at > now + 600
        || challenge.nonce.is_empty()
        || challenge.nonce.len() > 256
    {
        return Err("invalid owner challenge lifetime or nonce".into());
    }
    let actual = hex::decode(
        challenge
            .payload_hex
            .strip_prefix("0x")
            .ok_or("invalid challenge encoding")?,
    )
    .map_err(|_| "invalid challenge encoding")?;
    if actual != expected {
        return Err("provider challenge does not bind the intended request".into());
    }
    Ok(())
}
#[derive(Default)]
struct SseParser {
    buffer: Vec<u8>,
    data: Vec<String>,
    event_bytes: usize,
}
impl SseParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, String> {
        if self.buffer.len() + bytes.len() > 1024 * 1024 {
            return Err("SSE line exceeds limit".into());
        }
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        let mut consumed = 0;
        while let Some(relative) = self.buffer[consumed..].iter().position(|b| *b == b'\n') {
            let end = consumed + relative;
            let line = std::str::from_utf8(&self.buffer[consumed..end])
                .map_err(|_| "invalid SSE UTF-8")?
                .trim_end_matches('\r');
            consumed = end + 1;
            if line.is_empty() {
                if !self.data.is_empty() {
                    frames.push(self.data.join("\n"));
                    self.data.clear();
                    self.event_bytes = 0;
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                let data = data.strip_prefix(' ').unwrap_or(data);
                self.event_bytes = self.event_bytes.saturating_add(data.len() + 1);
                if self.event_bytes > 1024 * 1024 {
                    return Err("SSE event exceeds limit".into());
                }
                self.data.push(data.into());
            }
        }
        self.buffer.drain(..consumed);
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn configuration(path: &Path) -> ConsumerConfig {
        ConsumerConfig {
            network: NetworkConfig {
                identity: NetworkIdentity {
                    chain_id: crate::network::ESPACE_TESTNET_CHAIN_ID,
                    genesis_hash: crate::network::ESPACE_TESTNET_GENESIS.into(),
                    deployment_id: "consumer-test".into(),
                },
                rpc_url: "https://rpc.example".into(),
                rpc_bearer_file: None,
                faucet_url: None,
                explorer_url: None,
                policy_url: None,
            },
            data_dir: path.into(),
            catalog_url: None,
            development: false,
            dht: None,
        }
    }
    #[test]
    fn every_relative_profile_path_resolves_against_the_profile_directory() {
        let mut config = configuration(Path::new("data"));
        config.network.rpc_bearer_file = Some("secrets/rpc.token".into());
        config.dht = Some(crate::provider_config::DhtConfig {
            listen: "/ip4/127.0.0.1/tcp/0".into(),
            bootstrap: vec![],
            identity_file: "peer.key".into(),
        });
        // An absolute base on every platform; nothing is created there.
        let root = std::env::temp_dir();
        let base = root.join("profiles");
        let base = base.as_path();
        resolve_profile_paths(&mut config, base);
        assert_eq!(config.data_dir, base.join("data"));
        assert_eq!(
            config.network.rpc_bearer_file.as_deref(),
            Some(base.join("secrets/rpc.token").as_path())
        );
        assert_eq!(
            config.dht.as_ref().unwrap().identity_file,
            base.join("peer.key")
        );
        // Absolute paths are unchanged, and an absent optional path stays absent.
        let mut absolute = configuration(&root.join("state"));
        absolute.dht = Some(crate::provider_config::DhtConfig {
            listen: "/ip4/127.0.0.1/tcp/0".into(),
            bootstrap: vec![],
            identity_file: root.join("peer.key"),
        });
        resolve_profile_paths(&mut absolute, base);
        assert_eq!(absolute.data_dir, root.join("state"));
        assert_eq!(absolute.network.rpc_bearer_file, None);
        assert_eq!(absolute.dht.unwrap().identity_file, root.join("peer.key"));
    }
    #[test]
    fn network_scoping_and_process_mutation_locks() {
        let directory = tempfile::tempdir().unwrap();
        let config = configuration(directory.path());
        let consumer = Consumer::new(config.clone()).unwrap();
        consumer.wallet_create().unwrap();
        let second = Consumer::new(config.clone()).unwrap();
        let guard = consumer.process_lock().unwrap();
        assert!(second.process_lock().is_err());
        drop(guard);
        assert!(second.process_lock().is_ok());
        let mut other = config;
        other.network.identity.deployment_id = "another-deployment".into();
        let other = Consumer::new(other).unwrap();
        assert_ne!(consumer.root, other.root);
        assert!(other.address().is_err());
        assert!(consumer.account_path("../../wallet.key").is_err());
    }
    #[test]
    fn provider_signed_quotes_still_must_match_selected_terms_and_payer() {
        let directory = tempfile::tempdir().unwrap();
        let consumer = Consumer::new(configuration(directory.path())).unwrap();
        let payer = consumer.wallet_create().unwrap();
        let key = wallet::generate_private_key();
        let provider = wallet::address_from_private_key(&key).unwrap();
        let now = protocol::now().unwrap();
        let offer = InferenceOffer {
            network: consumer.config.network.identity.clone(),
            provider_wallet: provider.clone(),
            peer_id: libp2p::identity::Keypair::generate_ed25519()
                .public()
                .to_peer_id()
                .to_string(),
            deployment_id: "gpu-a".into(),
            endpoint: "https://provider.example".into(),
            model: ModelSpec {
                id: "model-a".into(),
                revision: "ab".repeat(20),
                tokenizer_revision: "cd".repeat(20),
                chat_template_sha256: "ef".repeat(32),
                quantization: None,
                context_length: 4096,
                max_output_tokens: 128,
                capabilities: vec!["chat".into()],
                serving: None,
            },
            price: PriceSchedule {
                input_wei_per_1k: "1".into(),
                output_wei_per_1k: "2".into(),
            },
            min_funding_wei: "10".into(),
            platform_wallet: wallet::address_from_private_key(&wallet::generate_private_key())
                .unwrap(),
            fee_bps: 50,
            max_concurrency: 1,
            revision: 1,
            issued_at: now,
            expires_at: now + 600,
        };
        let quote = Quote {
            id: format!("0x{}", "01".repeat(32)),
            network: offer.network.clone(),
            offer_id: protocol::offer_id(&offer).unwrap(),
            provider_wallet: provider,
            consumer_wallet: payer,
            model: offer.model.clone(),
            price: offer.price.clone(),
            funding_wei: "100".into(),
            platform_wallet: offer.platform_wallet.clone(),
            fee_bps: 50,
            max_concurrency: 1,
            nonce: "quote-nonce".into(),
            issued_at: now,
            expires_at: now + 300,
        };
        let offer = protocol::sign(OFFER_DOMAIN, offer, &key).unwrap();
        let signed = protocol::sign(QUOTE_DOMAIN, quote.clone(), &key).unwrap();
        assert!(consumer.verify_quote(&offer, &signed, true).is_ok());
        let mut altered = quote.clone();
        altered.price.output_wei_per_1k = "999".into();
        assert!(consumer
            .verify_quote(
                &offer,
                &protocol::sign(QUOTE_DOMAIN, altered, &key).unwrap(),
                true
            )
            .is_err());
        let mut altered = quote.clone();
        altered.consumer_wallet =
            wallet::address_from_private_key(&wallet::generate_private_key()).unwrap();
        assert!(consumer
            .verify_quote(
                &offer,
                &protocol::sign(QUOTE_DOMAIN, altered, &key).unwrap(),
                true
            )
            .is_err());
        let mut altered = quote;
        altered.network.deployment_id = "replayed-network".into();
        assert!(consumer
            .verify_quote(
                &offer,
                &protocol::sign(QUOTE_DOMAIN, altered, &key).unwrap(),
                true
            )
            .is_err());
    }
    fn funding_terms(
        consumer: &Consumer,
        key: &str,
        endpoint: &str,
        expired: bool,
        free: bool,
    ) -> (Signed<InferenceOffer>, Signed<Quote>) {
        let now = protocol::now().unwrap();
        let provider = wallet::address_from_private_key(key).unwrap();
        let offer = InferenceOffer {
            network: consumer.config.network.identity.clone(),
            provider_wallet: provider.clone(),
            peer_id: libp2p::identity::Keypair::generate_ed25519()
                .public()
                .to_peer_id()
                .to_string(),
            deployment_id: "funding-test".into(),
            endpoint: endpoint.into(),
            model: ModelSpec {
                id: "model-a".into(),
                revision: "ab".repeat(20),
                tokenizer_revision: "cd".repeat(20),
                chat_template_sha256: "ef".repeat(32),
                quantization: None,
                context_length: 4096,
                max_output_tokens: 128,
                capabilities: vec!["chat".into()],
                serving: None,
            },
            price: PriceSchedule {
                input_wei_per_1k: if free { "0" } else { "1" }.into(),
                output_wei_per_1k: if free { "0" } else { "2" }.into(),
            },
            min_funding_wei: if free { "0" } else { "10" }.into(),
            platform_wallet: format!("0x{}", "33".repeat(20)),
            fee_bps: 50,
            max_concurrency: 1,
            revision: 1,
            issued_at: now - 120,
            expires_at: now + 600,
        };
        let quote = Quote {
            id: format!("0x{}", "01".repeat(32)),
            network: offer.network.clone(),
            offer_id: protocol::offer_id(&offer).unwrap(),
            provider_wallet: provider,
            consumer_wallet: consumer.address().unwrap(),
            model: offer.model.clone(),
            price: offer.price.clone(),
            funding_wei: if free { "0" } else { "100" }.into(),
            platform_wallet: offer.platform_wallet.clone(),
            fee_bps: 50,
            max_concurrency: 1,
            nonce: "quote-nonce".into(),
            issued_at: now - 120,
            expires_at: if expired { now - 1 } else { now + 300 },
        };
        (
            protocol::sign(OFFER_DOMAIN, offer, key).unwrap(),
            protocol::sign(QUOTE_DOMAIN, quote, key).unwrap(),
        )
    }
    fn saved_funding(offer: Signed<InferenceOffer>, quote: Signed<Quote>) -> SavedAccount {
        SavedAccount {
            offer,
            quote,
            account: None,
            raw_transaction: None,
            tx_hash: None,
            funding_intent: None,
            funding_broadcast_started: false,
            funding_cancellation_requested: false,
            funding_cancellation: None,
            credentials: Vec::new(),
            active_credential: None,
            topup: None,
            receipt_sequence: 0,
            won_topups: Vec::new(),
        }
    }
    fn accepted_funding(quote: &Quote, hash: String, key: &str) -> Signed<FundingIntent> {
        protocol::sign(
            INTENT_DOMAIN,
            FundingIntent {
                network: quote.network.clone(),
                provider_wallet: quote.provider_wallet.clone(),
                consumer_wallet: quote.consumer_wallet.clone(),
                quote_id: quote.id.clone(),
                tx_hash: hash,
                registered_at: quote.issued_at + 1,
            },
            key,
        )
        .unwrap()
    }
    #[test]
    fn funding_acknowledgements_bind_all_terms_and_survive_quote_expiry() {
        let directory = tempfile::tempdir().unwrap();
        let consumer = Consumer::new(configuration(directory.path())).unwrap();
        consumer.wallet_create().unwrap();
        let key = wallet::generate_private_key();
        let (_, quote) = funding_terms(&consumer, &key, "https://provider.example", true, false);
        let hash = format!("0x{}", "44".repeat(32));
        let intent = accepted_funding(&quote.payload, hash.clone(), &key);
        funding_intent_matches(&quote.payload, &hash, &intent).unwrap();
        for field in [
            "chain",
            "genesis",
            "deployment",
            "provider",
            "payer",
            "quote",
            "hash",
            "early",
            "late",
            "future",
        ] {
            let mut changed = intent.payload.clone();
            match field {
                "chain" => changed.network.chain_id += 1,
                "genesis" => changed.network.genesis_hash = format!("0x{}", "55".repeat(32)),
                "deployment" => changed.network.deployment_id.push_str("-wrong"),
                "provider" => changed.provider_wallet = quote.payload.consumer_wallet.clone(),
                "payer" => changed.consumer_wallet = quote.payload.provider_wallet.clone(),
                "quote" => changed.quote_id = format!("0x{}", "66".repeat(32)),
                "hash" => changed.tx_hash = format!("0x{}", "77".repeat(32)),
                "early" => changed.registered_at = quote.payload.issued_at - 1,
                "late" => changed.registered_at = quote.payload.expires_at,
                _ => changed.registered_at = protocol::now().unwrap() + 60,
            }
            assert!(
                funding_intent_matches(
                    &quote.payload,
                    &hash,
                    &protocol::sign(INTENT_DOMAIN, changed, &key).unwrap()
                )
                .is_err(),
                "{field}"
            );
        }
        let wrong_signer = protocol::sign(
            INTENT_DOMAIN,
            intent.payload.clone(),
            &wallet::generate_private_key(),
        )
        .unwrap();
        assert!(funding_intent_matches(&quote.payload, &hash, &wrong_signer).is_err());
        let wrong_domain = protocol::sign(QUOTE_DOMAIN, intent.payload, &key).unwrap();
        assert!(funding_intent_matches(&quote.payload, &hash, &wrong_domain).is_err());
    }
    struct FundingFixture {
        key: String,
        quote: Quote,
        account_path: PathBuf,
        challenge: std::sync::Mutex<Option<crate::auth::AuthPayload>>,
        intent: std::sync::Mutex<Option<Signed<FundingIntent>>>,
        raw: std::sync::Mutex<Option<String>>,
        submissions: std::sync::Mutex<Vec<String>>,
        events: std::sync::Mutex<Vec<&'static str>>,
        mode: std::sync::atomic::AtomicU8,
        open_pending: std::sync::atomic::AtomicBool,
        receipt: std::sync::Mutex<Value>,
        /// Further receipts kept canonical beside `receipt`, keyed by transaction hash.
        receipts: std::sync::Mutex<std::collections::HashMap<String, Value>>,
        usage_receipt: std::sync::Mutex<Option<Signed<UsageReceipt>>>,
        started_at: u64,
        canonical_payment_hash: std::sync::Mutex<String>,
        nonce: std::sync::atomic::AtomicU64,
        credentials: std::sync::Mutex<Vec<(Credential, crate::billing_store::CredentialView)>>,
        credited_topups: std::sync::Mutex<std::collections::HashMap<String, u128>>,
        /// The wallet balance `eth_getBalance` reports; zero makes cancellation gas unaffordable.
        balance: std::sync::Mutex<u128>,
        /// Every provider control request received, challenges included, as method and path.
        controls: std::sync::Mutex<Vec<String>>,
    }
    async fn funding_rpc(
        axum::extract::State(state): axum::extract::State<Arc<FundingFixture>>,
        axum::Json(request): axum::Json<Value>,
    ) -> axum::Json<Value> {
        let result = match request["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x47"),
            "eth_syncing" => json!(false),
            "eth_getBlockByNumber" => {
                let head = 100 + protocol::now().unwrap().saturating_sub(state.started_at);
                let height = match request["params"][0].as_str().unwrap() {
                    "latest" => head,
                    "safe" => head - 10,
                    "finalized" => head - 20,
                    tag => u64::from_str_radix(tag.strip_prefix("0x").unwrap(), 16).unwrap(),
                };
                let hash = match height {
                    0 => state.quote.network.genesis_hash.clone(),
                    9 => state.canonical_payment_hash.lock().unwrap().clone(),
                    _ => format!("0x{height:064x}"),
                };
                let timestamp = if height == 0 { 0 } else { state.started_at - 100 + height };
                json!({"number":format!("0x{height:x}"),"hash":hash,"timestamp":format!("0x{timestamp:x}")})
            }
            "eth_estimateGas" => {
                let data = request["params"][0]["data"].as_str().unwrap();
                let bytes = hex::decode(data.strip_prefix("0x").unwrap()).unwrap();
                let gas = 21_000u64 + bytes.iter().map(|byte| if *byte == 0 { 10 } else { 40 }).sum::<u64>();
                json!(format!("0x{gas:x}"))
            }
            "eth_getTransactionCount" => json!(format!(
                "0x{:x}",
                state.nonce.load(std::sync::atomic::Ordering::SeqCst)
            )),
            "eth_getBalance" => json!(format!("0x{:x}", *state.balance.lock().unwrap())),
            "eth_gasPrice" => json!("0x1"),
            "eth_getTransactionReceipt" => {
                let receipt = state.receipt.lock().unwrap();
                if receipt["transactionHash"] == request["params"][0] {
                    receipt.clone()
                } else {
                    state
                        .receipts
                        .lock()
                        .unwrap()
                        .get(request["params"][0].as_str().unwrap())
                        .cloned()
                        .unwrap_or(Value::Null)
                }
            }
            "eth_sendRawTransaction" => {
                let raw = request["params"][0].as_str().unwrap();
                let hash = protocol::hash_hex(&hex::decode(&raw[2..]).unwrap());
                let persisted: SavedAccount =
                    decode(&private_read(&state.account_path).unwrap()).unwrap();
                let bytes = hex::decode(&raw[2..]).unwrap();
                let transaction = rlp::Rlp::new(&bytes);
                let data: Vec<u8> = transaction.val_at(5).unwrap();
                if data.is_empty() {
                    assert_eq!(transaction.val_at::<u64>(4).unwrap(), 0);
                    assert_eq!(
                        format!(
                            "0x{}",
                            hex::encode(transaction.val_at::<Vec<u8>>(3).unwrap())
                        ),
                        state.quote.consumer_wallet
                    );
                } else if let Some(topup) = persisted.topup.as_ref() {
                    if let Some(original) = &topup.raw_transaction {
                        assert_eq!(original, raw);
                    }
                } else {
                    funding_intent_matches(
                        &state.quote,
                        &hash,
                        persisted
                            .funding_intent
                            .as_ref()
                            .expect("ack durable before broadcast"),
                    )
                    .unwrap();
                    assert!(persisted.funding_broadcast_started);
                    assert_eq!(persisted.raw_transaction.as_deref(), Some(raw));
                    assert_eq!(state.raw.lock().unwrap().as_deref(), Some(raw));
                }
                state.events.lock().unwrap().push("broadcast");
                state.submissions.lock().unwrap().push(raw.into());
                json!(hash)
            }
            method => panic!("unexpected funding RPC {method}"),
        };
        axum::Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
    }
    async fn funding_control(
        axum::extract::State(state): axum::extract::State<Arc<FundingFixture>>,
        axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
        method: axum::http::Method,
        headers: axum::http::HeaderMap,
        body: axum::body::Bytes,
    ) -> (axum::http::StatusCode, axum::Json<Value>) {
        use axum::http::StatusCode;
        use std::sync::atomic::Ordering;
        state.controls.lock().unwrap().push(format!("{method} {}", uri.path()));
        if uri.path() == "/v1/auth/challenge" {
            let request: ChallengeRequest = decode(&body).unwrap();
            let payload = crate::auth::AuthPayload {
                network: state.quote.network.clone(),
                provider_wallet: state.quote.provider_wallet.clone(),
                wallet: request.wallet,
                method: request.method,
                path: request.path,
                body_hash: request.body_hash,
                nonce: uuid::Uuid::new_v4().to_string(),
                expires_at: protocol::now().unwrap() + 120,
            };
            let challenge = Challenge {
                nonce: payload.nonce.clone(),
                expires_at: payload.expires_at,
                payload_hex: format!(
                    "0x{}",
                    hex::encode(crate::auth::challenge_payload(&payload).unwrap())
                ),
            };
            *state.challenge.lock().unwrap() = Some(payload);
            return (
                StatusCode::OK,
                axum::Json(serde_json::to_value(challenge).unwrap()),
            );
        }
        let challenge = state.challenge.lock().unwrap().take().unwrap();
        assert_eq!(challenge.method, method.as_str());
        assert_eq!(challenge.path, uri.path());
        assert_eq!(challenge.body_hash, protocol::hash_hex(&body));
        assert_eq!(
            headers["X-Chiral-Owner"].to_str().unwrap(),
            state.quote.consumer_wallet
        );
        assert_eq!(headers["X-Chiral-Nonce"].to_str().unwrap(), challenge.nonce);
        assert!(wallet::verify_signature(
            &crate::auth::challenge_payload(&challenge).unwrap(),
            headers["X-Chiral-Signature"].to_str().unwrap(),
            &state.quote.consumer_wallet
        ));
        if uri.path().contains("/credentials") {
            if method == axum::http::Method::GET {
                let views: Vec<_> = state
                    .credentials
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(_, view)| view.clone())
                    .collect();
                return (
                    StatusCode::OK,
                    axum::Json(serde_json::to_value(views).unwrap()),
                );
            }
            if method == axum::http::Method::DELETE || uri.path().ends_with("/rotate") {
                let segments: Vec<_> = uri.path().split('/').collect();
                let id = segments[5];
                state
                    .credentials
                    .lock()
                    .unwrap()
                    .iter_mut()
                    .find(|(key, _)| key.id == id)
                    .unwrap()
                    .1
                    .revoked = true;
                if method == axum::http::Method::DELETE {
                    return (StatusCode::OK, axum::Json(json!({"revoked":true})));
                }
            }
            let options: crate::billing_store::CredentialIssueRequest = decode(&body).unwrap();
            let credential = fixture_credential(&state, options.expires_at, options.scopes);
            return (
                StatusCode::OK,
                axum::Json(serde_json::to_value(credential).unwrap()),
            );
        }
        if uri.path().ends_with("/receipt") {
            assert_eq!(method, axum::http::Method::GET);
            return (
                StatusCode::OK,
                axum::Json(serde_json::to_value(
                    state.usage_receipt.lock().unwrap().as_ref().unwrap(),
                ).unwrap()),
            );
        }
        if method == axum::http::Method::GET {
            let mut account = funding_account(&state.quote);
            if state.mode.load(Ordering::SeqCst) == 5 {
                account.status = "frozen".into();
            }
            return (
                StatusCode::OK,
                axum::Json(serde_json::to_value(account).unwrap()),
            );
        }
        if uri.path().ends_with("/topup") {
            state.events.lock().unwrap().push("topup");
            let request: TopupRequest = decode(&body).unwrap();
            let saved: SavedAccount = decode(&private_read(&state.account_path).unwrap()).unwrap();
            if let Some(topup) = saved.topup.as_ref() {
                assert_eq!(
                    topup.raw_transaction.as_ref(),
                    Some(&request.raw_transaction)
                );
            }
            assert!(state
                .submissions
                .lock()
                .unwrap()
                .contains(&request.raw_transaction));
            if state.open_pending.load(Ordering::SeqCst) {
                return (
                    StatusCode::CONFLICT,
                    axum::Json(json!({"error":{"code":"funding_pending"}})),
                );
            }
            let bytes = hex::decode(&request.raw_transaction[2..]).unwrap();
            let transaction = rlp::Rlp::new(&bytes);
            let amount = u128::from(transaction.val_at::<u64>(4).unwrap());
            let mut credited = state.credited_topups.lock().unwrap();
            credited.insert(request.raw_transaction, amount);
            let total: u128 = credited.values().sum();
            let mut account = funding_account(&state.quote);
            account.funded_wei = (account.funded_wei.parse::<u128>().unwrap() + total).to_string();
            account.available_wei =
                (account.available_wei.parse::<u128>().unwrap() + total).to_string();
            return (
                StatusCode::OK,
                axum::Json(serde_json::to_value(account).unwrap()),
            );
        }
        if uri.path().ends_with("/funding-intent") {
            state.events.lock().unwrap().push("register");
            let request: FundingIntentRequest = decode(&body).unwrap();
            let mut raw = state.raw.lock().unwrap();
            if let Some(original) = raw.as_ref() {
                assert_eq!(original, &request.raw_transaction);
            } else {
                *raw = Some(request.raw_transaction.clone());
            }
            let hash = protocol::hash_hex(&hex::decode(&request.raw_transaction[2..]).unwrap());
            let mode = state.mode.load(Ordering::SeqCst);
            if mode == 1 {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    axum::Json(json!({"error":{"code":"provider_draining"}})),
                );
            }
            if mode == 2 {
                return (StatusCode::OK, axum::Json(json!({"malformed":true})));
            }
            if mode == 3 {
                return (
                    StatusCode::OK,
                    axum::Json(
                        serde_json::to_value(accepted_funding(
                            &state.quote,
                            format!("0x{}", "88".repeat(32)),
                            &state.key,
                        ))
                        .unwrap(),
                    ),
                );
            }
            let mut accepted = state.intent.lock().unwrap();
            if accepted.is_none() {
                if state.quote.expires_at <= protocol::now().unwrap() {
                    return (
                        StatusCode::BAD_REQUEST,
                        axum::Json(json!({"error":{"code":"expired_quote"}})),
                    );
                }
                *accepted = Some(accepted_funding(&state.quote, hash, &state.key));
            }
            if mode == 4 {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    axum::Json(json!({"error":{"code":"provider_draining"}})),
                );
            }
            return (
                StatusCode::OK,
                axum::Json(serde_json::to_value(accepted.as_ref().unwrap()).unwrap()),
            );
        }
        assert_eq!(uri.path(), "/v1/contracts/open");
        state.events.lock().unwrap().push("open");
        let request: OpenRequest = decode(&body).unwrap();
        assert_eq!(request.quote_id, state.quote.id);
        if state.quote.funding_wei != "0" {
            assert_eq!(
                request.raw_transaction.as_deref(),
                state.raw.lock().unwrap().as_deref()
            );
            assert!(!state.submissions.lock().unwrap().is_empty());
        } else {
            assert!(request.raw_transaction.is_none());
        }
        if state.open_pending.load(Ordering::SeqCst) {
            return (
                StatusCode::CONFLICT,
                axum::Json(json!({"error":{"code":"funding_pending"}})),
            );
        }
        let q = &state.quote;
        let account = funding_account(q);
        let credential = fixture_credential(
            &state,
            protocol::now().unwrap() + 3600,
            vec!["infer".into(), "read".into()],
        );
        (
            StatusCode::OK,
            axum::Json(
                serde_json::to_value(OpenResult {
                    account,
                    credential: Some(credential),
                })
                .unwrap(),
            ),
        )
    }
    fn fixture_credential(
        state: &FundingFixture,
        expires_at: u64,
        scopes: Vec<String>,
    ) -> Credential {
        let credential = Credential {
            id: uuid::Uuid::new_v4().to_string(),
            bearer: uuid::Uuid::new_v4().to_string(),
            contract_id: state.quote.id.clone(),
            expires_at,
        };
        let view = crate::billing_store::CredentialView {
            id: credential.id.clone(),
            contract_id: credential.contract_id.clone(),
            expires_at,
            spend_cap_wei: None,
            spent_wei: "0".into(),
            reserved_wei: "0".into(),
            max_concurrency: 1,
            scopes,
            revoked: false,
            created_at: protocol::now().unwrap(),
            root: false,
        };
        state
            .credentials
            .lock()
            .unwrap()
            .push((credential.clone(), view));
        credential
    }
    async fn funding_infer(
        axum::extract::State(state): axum::extract::State<Arc<FundingFixture>>,
        headers: axum::http::HeaderMap,
    ) -> (axum::http::StatusCode, axum::Json<Value>) {
        let bearer = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        let credentials = state.credentials.lock().unwrap();
        let valid = credentials.iter().any(|(key, view)| {
            Some(key.bearer.as_str()) == bearer
                && !view.revoked
                && view.expires_at > protocol::now().unwrap()
                && view.scopes.iter().any(|scope| scope == "infer")
        });
        if valid {
            (
                axum::http::StatusCode::OK,
                axum::Json(json!({"accepted":true})),
            )
        } else {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                axum::Json(json!({"error":{"code":"invalid_credential"}})),
            )
        }
    }
    fn funding_account(q: &Quote) -> AccountView {
        AccountView {
            contract_id: q.id.clone(),
            network: q.network.clone(),
            provider_wallet: q.provider_wallet.clone(),
            consumer_wallet: q.consumer_wallet.clone(),
            model: q.model.clone(),
            price: q.price.clone(),
            status: "active".into(),
            funded_wei: q.funding_wei.clone(),
            fee_wei: "0".into(),
            spent_wei: "0".into(),
            reserved_wei: "0".into(),
            available_wei: q.funding_wei.clone(),
            reversed_wei: "0".into(),
            provider_loss_wei: "0".into(),
            usage: TokenUsage::default(),
            waived_wei: "0".into(),
            max_concurrency: q.max_concurrency,
        }
    }
    async fn funding_fixture(
        path: &Path,
        expired: bool,
        free: bool,
    ) -> (
        Consumer,
        SavedAccount,
        Arc<FundingFixture>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let mut config = configuration(path);
        config.development = true;
        config.network.rpc_url = endpoint.clone();
        let consumer = Consumer::new(config).unwrap();
        consumer.wallet_create().unwrap();
        let key = wallet::generate_private_key();
        let (offer, quote) = funding_terms(&consumer, &key, &endpoint, expired, free);
        let saved = saved_funding(offer, quote);
        let state = Arc::new(FundingFixture {
            key,
            quote: saved.quote.payload.clone(),
            account_path: consumer.account_path(&saved.quote.payload.id).unwrap(),
            challenge: std::sync::Mutex::new(None),
            intent: std::sync::Mutex::new(None),
            raw: std::sync::Mutex::new(None),
            submissions: std::sync::Mutex::new(Vec::new()),
            events: std::sync::Mutex::new(Vec::new()),
            mode: std::sync::atomic::AtomicU8::new(0),
            open_pending: std::sync::atomic::AtomicBool::new(false),
            receipt: std::sync::Mutex::new(Value::Null),
            receipts: std::sync::Mutex::new(std::collections::HashMap::new()),
            usage_receipt: std::sync::Mutex::new(None),
            started_at: protocol::now().unwrap(),
            canonical_payment_hash: std::sync::Mutex::new(format!("0x{}", "77".repeat(32))),
            nonce: std::sync::atomic::AtomicU64::new(0),
            credentials: std::sync::Mutex::new(Vec::new()),
            credited_topups: std::sync::Mutex::new(std::collections::HashMap::new()),
            balance: std::sync::Mutex::new(u128::MAX),
            controls: std::sync::Mutex::new(Vec::new()),
        });
        let app = axum::Router::new()
            .route("/", axum::routing::post(funding_rpc))
            .route("/v1/auth/challenge", axum::routing::post(funding_control))
            .route("/v1/contracts/:id", axum::routing::get(funding_control))
            .route("/v1/contracts/:id/receipt", axum::routing::get(funding_control))
            .route(
                "/v1/contracts/:id/funding-intent",
                axum::routing::post(funding_control),
            )
            .route(
                "/v1/contracts/:id/topup",
                axum::routing::post(funding_control),
            )
            .route(
                "/v1/contracts/:id/credentials",
                axum::routing::get(funding_control).post(funding_control),
            )
            .route(
                "/v1/contracts/:id/credentials/:credential",
                axum::routing::delete(funding_control),
            )
            .route(
                "/v1/contracts/:id/credentials/:credential/rotate",
                axum::routing::post(funding_control),
            )
            .route("/v1/chat/completions", axum::routing::post(funding_infer))
            .route("/v1/contracts/open", axum::routing::post(funding_control))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (consumer, saved, state, task)
    }
    #[tokio::test]
    async fn signed_receipt_rejects_noncanonical_request_exposure_without_advancing_sequence() {
        let directory = tempfile::tempdir().unwrap();
        let (mut consumer, saved, state, task) = funding_fixture(directory.path(), false, false).await;
        let id = &saved.quote.payload.id;
        let mut account = consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        let credential = consumer.credential_export(id, None).unwrap();
        let config = consumer.config.clone();
        let now = protocol::now().unwrap();
        // The counterparty supplies signed accounting claims, not measured engine usage.
        account.spent_wei = "1".into();
        account.available_wei = "99".into();
        account.usage = TokenUsage { prompt_tokens: 1, completion_tokens: 1 };
        let known = crate::billing_store::RequestView {
            id: uuid::Uuid::new_v4().to_string(),
            request_id: "receipt-known".into(),
            contract_id: id.clone(),
            credential_id: consumer.load_account(id).unwrap().active_credential.unwrap(),
            status: "completed".into(),
            usage: account.usage.clone(),
            cost_wei: "1".into(),
            max_cost_wei: "1".into(),
            provider_loss_wei: "0".into(),
            prompt_tokens_reserved: 1,
            max_output_tokens: 1,
            created_at: now - 1,
            settled_at: Some(now),
            performance: None,
            waived_wei: "0".into(),
        };
        let unknown = crate::billing_store::RequestView {
            id: uuid::Uuid::new_v4().to_string(),
            request_id: "receipt-unknown".into(),
            status: "cancelled_unknown_usage".into(),
            usage: TokenUsage::default(),
            cost_wei: "0".into(),
            provider_loss_wei: "1".into(),
            created_at: now - 2,
            settled_at: Some(now - 1),
            ..known.clone()
        };
        let mut payload = UsageReceipt {
            network: account.network.clone(),
            provider_wallet: account.provider_wallet.clone(),
            account,
            requests: vec![known],
            sequence: 1,
            issued_at: now,
        };
        let serve = |payload: &UsageReceipt| {
            *state.usage_receipt.lock().unwrap() =
                Some(protocol::sign(RECEIPT_DOMAIN, payload.clone(), &state.key).unwrap());
        };
        serve(&payload);
        let first = consumer.receipt(id).await.unwrap();
        assert_eq!(serde_json::to_value(&first.payload).unwrap(), serde_json::to_value(&payload).unwrap());
        let before = private_read(&state.account_path).unwrap();
        let submissions = state.submissions.lock().unwrap().clone();
        assert_eq!(consumer.load_account(id).unwrap().receipt_sequence, 1);

        payload.sequence = 2;
        payload.requests.push(unknown);
        for amount in [
            "", "00", "01", "+1", "-1", " 1", "1 ", "1.0", "1e0", "0x1",
            "١", "1\n", "\t1", "340282366920938463463374607431768211456",
        ] {
            // Put the bad exposure after a valid request and sign the changed payload:
            // a stale signature or malformed envelope must not mask this validation.
            payload.requests[1].provider_loss_wei = amount.into();
            serve(&payload);
            assert!(consumer.receipt(id).await.is_err(), "accepted exposure {amount:?}");
            assert_eq!(private_read(&state.account_path).unwrap(), before, "exposure {amount:?}");
            assert_eq!(consumer.credential_export(id, None).unwrap(), credential);
            drop(consumer);
            consumer = Consumer::new(config.clone()).unwrap();
            assert_eq!(consumer.load_account(id).unwrap().receipt_sequence, 1);
            assert_eq!(private_read(&state.account_path).unwrap(), before);
        }

        // Correction at the very same sequence remains recoverable after reopening.
        payload.requests[1].provider_loss_wei = "1".into();
        serve(&payload);
        let recovered = consumer.receipt(id).await.unwrap();
        assert_eq!(serde_json::to_value(&recovered.payload).unwrap(), serde_json::to_value(&payload).unwrap());
        let mut expected: Value = decode(&before).unwrap();
        expected["receipt_sequence"] = json!(2);
        assert_eq!(serde_json::to_value(consumer.load_account(id).unwrap()).unwrap(), expected);
        assert_eq!(consumer.credential_export(id, None).unwrap(), credential);
        let accepted = private_read(&state.account_path).unwrap();
        drop(consumer);
        consumer = Consumer::new(config).unwrap();
        assert!(consumer.receipt(id).await.is_err(), "accepted a replay of sequence two");
        assert_eq!(private_read(&state.account_path).unwrap(), accepted);
        assert_eq!(*state.submissions.lock().unwrap(), submissions);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    fn credential_options(scopes: &[&str]) -> Value {
        json!({"expires_at":protocol::now().unwrap()+3600,"spend_cap_wei":null,"max_concurrency":1,"scopes":scopes})
    }
    /// Save signed terms for `endpoint` with one active stored credential, under a profile
    /// with the given development mode. No provider or chain is contacted.
    fn account_with_credential(
        directory: &Path,
        endpoint: &str,
        development: bool,
        secret: &str,
    ) -> (Consumer, String) {
        let mut config = configuration(directory);
        config.development = development;
        let consumer = Consumer::new(config).unwrap();
        consumer.wallet_create().unwrap();
        let key = wallet::generate_private_key();
        let (offer, quote) = funding_terms(&consumer, &key, endpoint, false, false);
        let id = quote.payload.id.clone();
        let mut saved = saved_funding(offer, quote);
        let credential = Credential {
            id: "credential-a".into(),
            bearer: secret.into(),
            contract_id: id.clone(),
            expires_at: protocol::now().unwrap() + 3600,
        };
        consumer.store_credential(&mut saved, credential, true).unwrap();
        consumer.save_account(&saved).unwrap();
        (consumer, id)
    }
    #[test]
    fn credential_export_refuses_a_saved_origin_that_no_longer_validates() {
        let secret = "sk-chiral-export-secret-0123456789abcdef";
        for (endpoint, saved_in_development) in
            [("https://intranet./", false), ("https://localhost:8443", true)]
        {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, id) =
                account_with_credential(directory.path(), endpoint, saved_in_development, secret);
            if saved_in_development {
                // Exported while development is on, the localhost origin is admitted.
                let exported = consumer.credential_export(&id, None).unwrap();
                assert_eq!(exported["base_url"], "https://localhost:8443/v1");
                assert_eq!(exported["api_key"], secret);
            }
            // Reload the same data directory with development off.
            let config = configuration(directory.path());
            drop(consumer);
            let consumer = Consumer::new(config).unwrap();
            let path = consumer.account_path(&id).unwrap();
            let before = fs::read(&path).unwrap();
            let expected = crate::offer_registry::validate_endpoint(endpoint, false).unwrap_err();
            for credential_id in [None, Some("credential-a")] {
                let error = consumer.credential_export(&id, credential_id).unwrap_err();
                assert_eq!(error, expected, "{endpoint}/{credential_id:?}");
                assert!(!error.contains(secret), "{endpoint}: error carries the secret");
            }
            assert_eq!(fs::read(&path).unwrap(), before, "{endpoint}: account file changed");
            // Loading stays lenient, so the signed terms and credential remain recoverable.
            let loaded = consumer.load_account(&id).unwrap();
            assert_eq!(loaded.offer.payload.endpoint, endpoint);
            assert_eq!(loaded.active_credential.as_deref(), Some("credential-a"));
            assert_eq!(loaded.credentials.len(), 1);
            assert_eq!(fs::read(&path).unwrap(), before, "{endpoint}: account file changed");
        }
    }
    #[test]
    fn credential_export_base_url_is_the_validated_origin_plus_v1() {
        let secret = "sk-chiral-export-secret-0123456789abcdef";
        for (endpoint, development, base_url) in [
            ("https://provider.example", false, "https://provider.example/v1"),
            ("http://127.0.0.1:9419", true, "http://127.0.0.1:9419/v1"),
            ("http://[::1]:9419", true, "http://[::1]:9419/v1"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, id) =
                account_with_credential(directory.path(), endpoint, development, secret);
            let exported = consumer.credential_export(&id, None).unwrap();
            assert_eq!(exported["base_url"], base_url, "{endpoint}");
            assert_eq!(exported["api_key"], secret, "{endpoint}");
            assert_eq!(exported["credential_id"], "credential-a", "{endpoint}");
            assert_eq!(exported["contract_id"], id.as_str(), "{endpoint}");
        }
    }
    /// account-credentials requirement 1: native issuance and rotation decode their
    /// options as one JSON object and refuse the positional form before any provider request.
    #[tokio::test]
    async fn native_credential_options_refuse_a_positional_array_before_any_provider_request() {
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = funding_fixture(directory.path(), false, true).await;
        let id = &saved.quote.payload.id;
        consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        let original = consumer.credential_export(id, None).unwrap();
        let original_id = original["credential_id"].as_str().unwrap().to_string();
        let account = private_read(&consumer.account_path(id).unwrap()).unwrap();
        let provider_credentials = state.credentials.lock().unwrap().len();
        state.controls.lock().unwrap().clear();
        let positional = json!([protocol::now().unwrap() + 3600, null, 1, ["read"]]);
        assert_eq!(
            consumer.credential_issue(id, positional.clone()).await.unwrap_err(),
            "invalid credential options"
        );
        assert_eq!(
            consumer
                .credential_rotate(id, &original_id, positional)
                .await
                .unwrap_err(),
            "invalid credential options"
        );
        let controls = state.controls.lock().unwrap().clone();
        assert!(controls.is_empty(), "{controls:?}");
        assert_eq!(state.credentials.lock().unwrap().len(), provider_credentials);
        assert_eq!(private_read(&consumer.account_path(id).unwrap()).unwrap(), account);
        assert_eq!(consumer.credential_export(id, None).unwrap(), original);
        // The object form of the same options is the request the provider receives.
        let issued = consumer
            .credential_issue(id, credential_options(&["read"]))
            .await
            .unwrap();
        assert_eq!(
            *state.controls.lock().unwrap(),
            [
                "POST /v1/auth/challenge".to_string(),
                format!("GET /v1/contracts/{id}/credentials"),
                "POST /v1/auth/challenge".to_string(),
                format!("POST /v1/contracts/{id}/credentials"),
            ]
        );
        assert!(consumer.credential_export(id, issued["id"].as_str()).is_ok());
        assert_eq!(consumer.credential_export(id, None).unwrap(), original);
        task.abort();
    }
    #[tokio::test]
    async fn explicit_credential_issue_recovers_default_without_rotating_on_resume() {
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = funding_fixture(directory.path(), false, false).await;
        let id = &saved.quote.payload.id;
        consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        let original = consumer.credential_export(id, None).unwrap();
        let original_id = original["credential_id"].as_str().unwrap();
        consumer
            .credential_issue(id, credential_options(&["infer", "read"]))
            .await
            .unwrap();
        consumer
            .credential_issue(id, credential_options(&["read"]))
            .await
            .unwrap();
        assert_eq!(consumer.credential_export(id, None).unwrap(), original);
        consumer.credential_revoke(id, original_id).await.unwrap();
        consumer.resume(id).await.unwrap();
        assert!(consumer
            .credential_export(id, None)
            .unwrap_err()
            .contains("explicitly issue"));
        let read_only = consumer
            .credential_issue(id, credential_options(&["read"]))
            .await
            .unwrap();
        assert!(consumer.credential_export(id, None).is_err());
        assert!(consumer
            .credential_export(id, read_only["id"].as_str())
            .is_ok());
        let replacement = consumer
            .credential_issue(id, credential_options(&["infer", "read"]))
            .await
            .unwrap();
        let exported = consumer.credential_export(id, None).unwrap();
        assert_eq!(exported["credential_id"], replacement["id"]);
        assert_ne!(exported["api_key"], original["api_key"]);
        assert_eq!(
            consumer
                .infer(id, "chat", json!({"messages":[]}))
                .await
                .unwrap()["accepted"],
            true
        );
        let config = consumer.config.clone();
        drop(consumer);
        let restarted = Consumer::new(config).unwrap();
        restarted.resume(id).await.unwrap();
        assert_eq!(restarted.credential_export(id, None).unwrap(), exported);
        assert_eq!(
            state
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "open")
                .count(),
            1
        );
        assert_eq!(
            restarted.history().await.unwrap()["transactions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        task.abort();
    }
    #[tokio::test]
    async fn issuance_recovers_remote_revocation_and_expiry_but_read_only_rotation_never_becomes_default(
    ) {
        for expired in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, saved, state, task) =
                funding_fixture(directory.path(), false, true).await;
            let id = &saved.quote.payload.id;
            consumer.fund(&saved.offer, &saved.quote).await.unwrap();
            let original = consumer.credential_export(id, None).unwrap();
            {
                let mut credentials = state.credentials.lock().unwrap();
                if expired {
                    credentials[0].1.expires_at = protocol::now().unwrap() - 1;
                } else {
                    credentials[0].1.revoked = true;
                }
            }
            let issued = consumer
                .credential_issue(id, credential_options(&["infer"]))
                .await
                .unwrap();
            assert_ne!(
                consumer.credential_export(id, None).unwrap()["api_key"],
                original["api_key"]
            );
            assert_eq!(
                consumer.infer(id, "chat", json!({})).await.unwrap()["accepted"],
                true
            );
            let rotated = consumer
                .credential_rotate(
                    id,
                    issued["id"].as_str().unwrap(),
                    credential_options(&["read"]),
                )
                .await
                .unwrap();
            assert!(consumer.credential_export(id, None).is_err());
            assert!(consumer
                .credential_export(id, rotated["id"].as_str())
                .is_ok());
            consumer
                .credential_issue(id, credential_options(&["infer"]))
                .await
                .unwrap();
            assert_eq!(
                consumer.infer(id, "chat", json!({})).await.unwrap()["accepted"],
                true
            );
            task.abort();
        }
    }
    fn fixture_receipt(state: &FundingFixture, raw: &str, success: bool) -> Value {
        let bytes = hex::decode(&raw[2..]).unwrap();
        let transaction = rlp::Rlp::new(&bytes);
        json!({"transactionHash":protocol::hash_hex(&bytes),
            "from":state.quote.consumer_wallet,"to":format!("0x{}",hex::encode(transaction.val_at::<Vec<u8>>(3).unwrap())),
            "type":"0x0","gasUsed":format!("0x{:x}",transaction.val_at::<u64>(2).unwrap()),"status":if success{"0x1"}else{"0x0"},
            "blockHash":state.canonical_payment_hash.lock().unwrap().clone(),"blockNumber":"0x9"})
    }
    fn settle_fixture(state: &FundingFixture, raw: &str, success: bool) {
        let bytes = hex::decode(&raw[2..]).unwrap();
        let transaction = rlp::Rlp::new(&bytes);
        *state.receipt.lock().unwrap() = fixture_receipt(state, raw, success);
        state.nonce.store(
            transaction.val_at::<u64>(0).unwrap() + 1,
            std::sync::atomic::Ordering::SeqCst,
        );
    }
    /// Keep an earlier successful payment canonical (in the current canonical block)
    /// while `settle_fixture` moves the primary receipt to another transaction.
    fn retain_fixture_receipt(state: &FundingFixture, raw: &str) {
        let receipt = fixture_receipt(state, raw, true);
        state.receipts.lock().unwrap().insert(
            receipt["transactionHash"].as_str().unwrap().to_string(),
            receipt,
        );
    }
    #[tokio::test]
    async fn cancelled_topup_history_survives_restart_reorg_and_original_winning_race() {
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (consumer, mut saved, state, task) =
            funding_fixture(directory.path(), true, false).await;
        let id = saved.quote.payload.id.clone();
        saved.account = Some(funding_account(&saved.quote.payload));
        let credential = fixture_credential(
            &state,
            protocol::now().unwrap() + 3600,
            vec!["infer".into()],
        );
        consumer
            .store_credential(&mut saved, credential, true)
            .unwrap();
        consumer.save_account(&saved).unwrap();
        state.open_pending.store(true, Ordering::SeqCst);
        assert!(consumer
            .topup(&id, "50")
            .await
            .unwrap_err()
            .contains("funding_pending"));
        let original = state.submissions.lock().unwrap()[0].clone();
        let result = consumer.cancel_funding(&id, None).await.unwrap();
        assert_eq!(result["status"], "cancellation_pending");
        let operation = result["operation_id"].as_str().unwrap().to_string();
        let replacement = state.submissions.lock().unwrap()[1].clone();
        assert_ne!(original, replacement);
        assert!(consumer.resume(&id).await.is_err());
        assert_eq!(consumer.account(&id).await.unwrap().available_wei, "100");
        assert_eq!(
            consumer.infer(&id, "chat", json!({})).await.unwrap()["accepted"],
            true
        );
        let config = consumer.config.clone();
        drop(consumer);
        let consumer = Consumer::new(config).unwrap();
        assert_eq!(
            consumer
                .cancel_funding(&id, Some(&operation))
                .await
                .unwrap()["status"],
            "cancellation_pending"
        );
        assert_eq!(
            state.submissions.lock().unwrap().as_slice(),
            [original.clone(), replacement.clone(), replacement.clone()]
        );
        settle_fixture(&state, &replacement, true);
        assert_eq!(
            consumer
                .cancel_funding(&id, Some(&operation))
                .await
                .unwrap()["status"],
            "cancelled"
        );
        assert!(consumer.load_account(&id).unwrap().topup.is_none());
        assert!(consumer
            .cancel_funding(&id, None)
            .await
            .unwrap_err()
            .contains("exact operation_id"));
        let recovery = consumer.funding_recovery(&id).await.unwrap();
        assert_eq!(recovery["operations"][0]["operation_id"], operation);
        assert!(!recovery.to_string().contains("raw_transaction"));
        let config = consumer.config.clone();
        drop(consumer);
        let consumer = Consumer::new(config).unwrap();
        *state.receipt.lock().unwrap() = Value::Null;
        *state.canonical_payment_hash.lock().unwrap() = format!("0x{}", "88".repeat(32));
        state.nonce.store(0, Ordering::SeqCst);
        let recovery = consumer.funding_recovery(&id).await.unwrap();
        assert_eq!(
            recovery["operations"][0]["cancellation"]["status"],
            "cancellation_pending"
        );
        assert_eq!(state.submissions.lock().unwrap().len(), 3); // inspection never broadcasts
        assert_eq!(
            consumer
                .cancel_funding(&id, Some(&operation))
                .await
                .unwrap()["status"],
            "cancellation_pending"
        );
        assert_eq!(state.submissions.lock().unwrap()[3], replacement); // no automatic fee bump
        settle_fixture(&state, &original, true);
        state.open_pending.store(false, Ordering::SeqCst);
        assert_eq!(
            consumer
                .cancel_funding(&id, Some(&operation))
                .await
                .unwrap()["status"],
            "original_accepted"
        );
        assert_eq!(
            consumer
                .load_account(&id)
                .unwrap()
                .account
                .unwrap()
                .available_wei,
            "150"
        );
        assert_eq!(
            consumer
                .cancel_funding(&id, Some(&operation))
                .await
                .unwrap()["status"],
            "original_accepted"
        );
        assert_eq!(
            consumer
                .load_account(&id)
                .unwrap()
                .account
                .unwrap()
                .funded_wei,
            "150"
        );
        assert_eq!(state.credited_topups.lock().unwrap().len(), 1);
        assert_eq!(state.submissions.lock().unwrap().len(), 4);
        let journal = consumer.history().await.unwrap();
        assert_eq!(journal["transactions"].as_array().unwrap().len(), 1);
        assert_eq!(journal["transactions"][0]["raw_transaction"], original);
        assert_eq!(
            journal["transactions"][0]["cancellation"]["raw_transaction"],
            replacement
        );
        task.abort();
    }
    #[tokio::test]
    async fn topup_cancellation_original_wins_or_execution_fails_without_erasing_credit() {
        use std::sync::atomic::Ordering;
        for outcome in [
            "original_accepted",
            "original_failed",
            "cancellation_failed",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, mut saved, state, task) =
                funding_fixture(directory.path(), true, false).await;
            let id = saved.quote.payload.id.clone();
            saved.account = Some(funding_account(&saved.quote.payload));
            consumer.save_account(&saved).unwrap();
            state.open_pending.store(true, Ordering::SeqCst);
            assert!(consumer.topup(&id, "50").await.is_err());
            let original = state.submissions.lock().unwrap()[0].clone();
            let pending = consumer.cancel_funding(&id, None).await.unwrap();
            let operation = pending["operation_id"].as_str().unwrap();
            let replacement = state.submissions.lock().unwrap()[1].clone();
            settle_fixture(
                &state,
                if outcome == "cancellation_failed" {
                    &replacement
                } else {
                    &original
                },
                outcome == "original_accepted",
            );
            state.open_pending.store(false, Ordering::SeqCst);
            assert_eq!(
                consumer.cancel_funding(&id, Some(operation)).await.unwrap()["status"],
                outcome
            );
            let saved = consumer.load_account(&id).unwrap();
            assert!(saved.topup.is_none());
            let account = saved.account.unwrap();
            assert_eq!(account.status, "active");
            assert_eq!(
                account.available_wei,
                if outcome == "original_accepted" {
                    "150"
                } else {
                    "100"
                }
            );
            assert_eq!(state.submissions.lock().unwrap().len(), 2);
            assert_eq!(
                consumer.funding_recovery(&id).await.unwrap()["operations"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            task.abort();
        }
    }
    #[tokio::test]
    async fn orphaned_opening_can_be_cancelled_even_with_a_saved_account() {
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = funding_fixture(directory.path(), false, false).await;
        let id = &saved.quote.payload.id;
        consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        let original = state.submissions.lock().unwrap()[0].clone();
        settle_fixture(&state, &original, true);
        consumer.funding_recovery(id).await.unwrap();
        *state.receipt.lock().unwrap() = Value::Null;
        *state.canonical_payment_hash.lock().unwrap() = format!("0x{}", "88".repeat(32));
        state.nonce.store(0, Ordering::SeqCst);
        assert_eq!(
            consumer
                .cancel_funding(id, Some(&format!("open:{id}")))
                .await
                .unwrap()["status"],
            "cancellation_pending"
        );
        assert!(consumer.load_account(id).unwrap().account.is_some());
        assert!(consumer.credential_export(id, None).is_ok());
        assert_eq!(state.submissions.lock().unwrap().len(), 2);
        task.abort();
    }
    #[tokio::test]
    async fn opening_cancellation_on_a_saved_account_still_refreshes_without_reusing_the_opening() {
        // inference-funding-settlement requirements 4 and 16 (task 2.7): the
        // cancellation gate covers only the cancelled `open:<id>` payment.
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = funding_fixture(directory.path(), false, false).await;
        let id = &saved.quote.payload.id;
        consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        let original = state.submissions.lock().unwrap()[0].clone();
        settle_fixture(&state, &original, true);
        consumer.funding_recovery(id).await.unwrap();
        *state.receipt.lock().unwrap() = Value::Null;
        *state.canonical_payment_hash.lock().unwrap() = format!("0x{}", "88".repeat(32));
        state.nonce.store(0, Ordering::SeqCst);
        assert_eq!(
            consumer
                .cancel_funding(id, Some(&format!("open:{id}")))
                .await
                .unwrap()["status"],
            "cancellation_pending"
        );
        let after_cancellation = state.submissions.lock().unwrap().clone();
        assert_eq!(after_cancellation.len(), 2);
        let before = consumer.load_account(id).unwrap();
        let resumed = consumer.resume(id).await.unwrap();
        assert_eq!(resumed.contract_id, *id);
        let refunded = consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        assert_eq!(refunded.contract_id, *id);
        // Neither the cancelled opening nor any new payment was prepared or sent.
        assert_eq!(*state.submissions.lock().unwrap(), after_cancellation);
        let after = consumer.load_account(id).unwrap();
        assert!(after.account.is_some());
        assert!(after.funding_cancellation_requested);
        assert_eq!(after.raw_transaction, before.raw_transaction);
        assert_eq!(after.tx_hash, before.tx_hash);
        assert_eq!(
            consumer.funding_recovery(id).await.unwrap()["operations"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(consumer.credential_export(id, None).is_ok());
        task.abort();
    }
    #[tokio::test]
    async fn opening_cancellation_on_a_saved_account_keeps_pending_topup_recovery() {
        // Task 2.7: a pending top-up still recovers its exact original bytes
        // after an explicit cancellation of the account's opening.
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = funding_fixture(directory.path(), false, false).await;
        let id = &saved.quote.payload.id;
        consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        let opening = state.submissions.lock().unwrap()[0].clone();
        settle_fixture(&state, &opening, true);
        consumer.funding_recovery(id).await.unwrap();
        state.open_pending.store(true, Ordering::SeqCst);
        assert!(consumer.topup(id, "50").await.is_err());
        let topup = consumer
            .load_account(id)
            .unwrap()
            .topup
            .and_then(|intent| intent.raw_transaction)
            .expect("pending top-up keeps its original bytes");
        assert_ne!(topup, opening);
        *state.receipt.lock().unwrap() = Value::Null;
        *state.canonical_payment_hash.lock().unwrap() = format!("0x{}", "88".repeat(32));
        state.nonce.store(0, Ordering::SeqCst);
        assert_eq!(
            consumer
                .cancel_funding(id, Some(&format!("open:{id}")))
                .await
                .unwrap()["status"],
            "cancellation_pending"
        );
        let after_cancellation = state.submissions.lock().unwrap().clone();
        let replacement = after_cancellation.last().unwrap().clone();
        state.open_pending.store(false, Ordering::SeqCst);
        let result = consumer.resume(id).await;
        if let Err(error) = &result {
            assert!(
                !error.contains("funding recovery was requested"),
                "opening cancellation must not gate top-up recovery: {error}"
            );
        }
        let submissions = state.submissions.lock().unwrap().clone();
        for raw in &submissions[after_cancellation.len()..] {
            assert_eq!(raw, &topup, "only the top-up's exact original may be (re)sent");
        }
        assert!(!submissions[after_cancellation.len()..].contains(&opening));
        assert!(!submissions[after_cancellation.len()..].contains(&replacement));
        let after = consumer.load_account(id).unwrap();
        assert!(after.account.is_some());
        if let Some(intent) = &after.topup {
            assert_eq!(intent.raw_transaction.as_ref(), Some(&topup));
        }
        task.abort();
    }
    fn event_count(state: &FundingFixture, event: &str) -> usize {
        state
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|recorded| **recorded == event)
            .count()
    }
    #[tokio::test]
    async fn won_opening_original_is_settled_by_resume_without_cancellation_authority() {
        // inference-funding-settlement requirements 16 and 17 (task 2.6). An unopened
        // opening's explicit cancellation loses to its original, either directly while the
        // cancellation is pending, or after `cancelled` when a reorg makes the original
        // canonical at safe again. Plain `resume` (no `cancel_funding`) opens the account
        // once from the original bytes and signs, resubmits or broadcasts nothing.
        use std::sync::atomic::Ordering;
        for displaced in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, saved, state, task) =
                funding_fixture(directory.path(), false, false).await;
            let id = saved.quote.payload.id.clone();
            state.open_pending.store(true, Ordering::SeqCst);
            assert!(consumer
                .fund(&saved.offer, &saved.quote)
                .await
                .unwrap_err()
                .contains("funding_pending"));
            let original = state.submissions.lock().unwrap()[0].clone();
            assert_eq!(
                consumer.cancel_funding(&id, None).await.unwrap()["status"],
                "cancellation_pending"
            );
            let replacement = state.submissions.lock().unwrap()[1].clone();
            assert_ne!(original, replacement);
            let consumer = if displaced {
                // The replacement first wins at safe ...
                settle_fixture(&state, &replacement, true);
                let recovery = consumer.funding_recovery(&id).await.unwrap();
                assert_eq!(
                    recovery["operations"][0]["cancellation"]["status"],
                    "cancelled"
                );
                assert!(consumer.resume(&id).await.is_err());
                // ... then a reorg displaces it and the original is canonical at safe.
                *state.canonical_payment_hash.lock().unwrap() = format!("0x{}", "88".repeat(32));
                settle_fixture(&state, &original, true);
                let recovery = consumer.funding_recovery(&id).await.unwrap();
                assert_eq!(
                    recovery["operations"][0]["cancellation"]["status"],
                    "original_accepted"
                );
                assert!(!recovery.to_string().contains("raw_transaction"));
                // funding-recovery durably records the won original with the agreement.
                assert_eq!(
                    consumer
                        .load_account(&id)
                        .unwrap()
                        .funding_cancellation
                        .unwrap()
                        .status,
                    "original_accepted"
                );
                let config = consumer.config.clone();
                drop(consumer);
                Consumer::new(config).unwrap()
            } else {
                // The original wins the race directly; nothing inspects it before resume.
                settle_fixture(&state, &original, true);
                consumer
            };
            state.open_pending.store(false, Ordering::SeqCst);
            let opens = event_count(&state, "open");
            let account = consumer.resume(&id).await.unwrap();
            assert_eq!(account.contract_id, id);
            assert_eq!(account.funded_wei, "100");
            assert_eq!(event_count(&state, "open"), opens + 1);
            let after = consumer.load_account(&id).unwrap();
            assert!(after.account.is_some());
            assert_eq!(after.raw_transaction.as_ref(), Some(&original));
            assert_eq!(
                after.funding_cancellation.as_ref().unwrap().status,
                "original_accepted"
            );
            assert!(consumer.credential_export(&id, None).is_ok());
            // A second resume only refreshes: no second open, so no second credit.
            consumer.resume(&id).await.unwrap();
            assert_eq!(event_count(&state, "open"), opens + 1);
            // No replacement was resubmitted and nothing new was signed or broadcast.
            assert_eq!(
                *state.submissions.lock().unwrap(),
                [original.clone(), replacement.clone()]
            );
            let journal = consumer.history().await.unwrap();
            assert_eq!(journal["transactions"].as_array().unwrap().len(), 1);
            assert_eq!(journal["transactions"][0]["raw_transaction"], original);
            assert_eq!(
                journal["transactions"][0]["cancellation"]["raw_transaction"],
                replacement
            );
            assert!(consumer.pending_accounts().unwrap()["pending"]
                .as_array()
                .unwrap()
                .is_empty());
            task.abort();
        }
    }
    #[tokio::test]
    async fn won_topup_original_after_cleared_intent_is_credited_once_by_resume() {
        won_topup_original_case(false).await;
    }
    #[tokio::test]
    async fn won_topup_original_recorded_by_funding_recovery_is_credited_once_by_resume() {
        won_topup_original_case(true).await;
    }
    async fn won_topup_original_case(inspect_first: bool) {
        // Task 2.6. A top-up's cancellation is reported `cancelled`, which clears the pending
        // intent; a reorg then displaces the replacement and makes the original canonical at
        // safe. Plain `resume` (no `cancel_funding`), with or without a prior
        // `funding-recovery`, credits that original exactly once; a second resume does not
        // credit again, and nothing is signed, resubmitted or broadcast.
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) =
            funding_fixture(directory.path(), false, false).await;
        let id = saved.quote.payload.id.clone();
        consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        let opening = state.submissions.lock().unwrap()[0].clone();
        settle_fixture(&state, &opening, true);
        retain_fixture_receipt(&state, &opening);
        state.open_pending.store(true, Ordering::SeqCst);
        assert!(consumer
            .topup(&id, "50")
            .await
            .unwrap_err()
            .contains("funding_pending"));
        let original = state.submissions.lock().unwrap()[1].clone();
        let pending = consumer.cancel_funding(&id, None).await.unwrap();
        assert_eq!(pending["status"], "cancellation_pending");
        let operation = pending["operation_id"].as_str().unwrap().to_string();
        let replacement = state.submissions.lock().unwrap()[2].clone();
        settle_fixture(&state, &replacement, true);
        assert_eq!(
            consumer
                .cancel_funding(&id, Some(&operation))
                .await
                .unwrap()["status"],
            "cancelled"
        );
        assert!(consumer.load_account(&id).unwrap().topup.is_none());
        // Reorg: the new canonical block holds the opening and the top-up original.
        *state.canonical_payment_hash.lock().unwrap() = format!("0x{}", "88".repeat(32));
        retain_fixture_receipt(&state, &opening);
        settle_fixture(&state, &original, true);
        state.open_pending.store(false, Ordering::SeqCst);
        let consumer = if inspect_first {
            let recovery = consumer.funding_recovery(&id).await.unwrap();
            let entry = recovery["operations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["operation_id"] == operation.as_str())
                .unwrap()
                .clone();
            assert_eq!(entry["cancellation"]["status"], "original_accepted");
            assert!(!recovery.to_string().contains("raw_transaction"));
            let recorded = consumer.load_account(&id).unwrap();
            assert_eq!(recorded.won_topups.len(), 1);
            assert_eq!(recorded.won_topups[0].operation_id, operation);
            assert!(!recorded.won_topups[0].credited);
            // Inspection records the outcome but never credits or submits anything.
            assert!(state.credited_topups.lock().unwrap().is_empty());
            let config = consumer.config.clone();
            drop(consumer);
            Consumer::new(config).unwrap()
        } else {
            consumer
        };
        let topups = event_count(&state, "topup");
        consumer.resume(&id).await.unwrap();
        assert_eq!(event_count(&state, "topup"), topups + 1);
        assert_eq!(
            *state.credited_topups.lock().unwrap(),
            std::collections::HashMap::from([(original.clone(), 50u128)])
        );
        let after = consumer.load_account(&id).unwrap();
        assert!(after.topup.is_none());
        assert_eq!(after.won_topups.len(), 1);
        assert!(after.won_topups[0].credited);
        // A second resume never settles the same original again.
        consumer.resume(&id).await.unwrap();
        assert_eq!(event_count(&state, "topup"), topups + 1);
        assert_eq!(state.credited_topups.lock().unwrap().len(), 1);
        assert_eq!(
            *state.submissions.lock().unwrap(),
            [opening.clone(), original.clone(), replacement.clone()]
        );
        let journal = consumer.history().await.unwrap();
        assert_eq!(journal["transactions"].as_array().unwrap().len(), 2);
        task.abort();
    }
    #[tokio::test]
    async fn won_opening_original_without_a_journaled_replacement_is_settled_and_never_rebroadcast(
    ) {
        // Task 2.6, requirements 16 and 17. The opening's cancellation is requested after its
        // original consumed the nonce but before it settled, so `cancel_payment_once` refuses
        // as pending and no replacement is journaled. The won original must still be settled
        // by plain `resume` (recorded by `funding-recovery`, or reported by an earlier explicit
        // cancel-funding). A reorg that displaces it before opening must not let resume
        // rebroadcast it; once it is canonical at safe again, resume opens exactly once.
        use std::sync::atomic::Ordering;
        for explicit in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, saved, state, task) =
                funding_fixture(directory.path(), false, false).await;
            let id = saved.quote.payload.id.clone();
            state.open_pending.store(true, Ordering::SeqCst);
            assert!(consumer
                .fund(&saved.offer, &saved.quote)
                .await
                .unwrap_err()
                .contains("funding_pending"));
            let original = state.submissions.lock().unwrap()[0].clone();
            // The original consumed its nonce, but no receipt is available yet.
            state.nonce.store(1, Ordering::SeqCst);
            assert!(consumer
                .cancel_funding(&id, None)
                .await
                .unwrap_err()
                .contains("original nonce is not current"));
            let requested = consumer.load_account(&id).unwrap();
            assert!(requested.funding_cancellation_requested);
            assert!(requested.funding_cancellation.is_none());
            let refusal = consumer.resume(&id).await.unwrap_err();
            assert!(refusal.contains("(cancellation_requested)"));
            assert_eq!(*state.submissions.lock().unwrap(), [original.clone()]);
            // The original wins at safe.
            settle_fixture(&state, &original, true);
            if explicit {
                assert_eq!(
                    consumer.cancel_funding(&id, None).await.unwrap()["status"],
                    "original_accepted"
                );
            } else {
                let recovery = consumer.funding_recovery(&id).await.unwrap();
                assert_eq!(recovery["operations"][0]["status"], "accepted");
                assert!(recovery["operations"][0]["cancellation"].is_null());
            }
            assert_eq!(
                consumer
                    .load_account(&id)
                    .unwrap()
                    .funding_cancellation
                    .unwrap()
                    .status,
                "original_accepted"
            );
            // A reorg displaces the original before the account is opened: the recorded win
            // is stale, so resume refuses and never broadcasts the cancelled original.
            *state.canonical_payment_hash.lock().unwrap() = format!("0x{}", "88".repeat(32));
            let refusal = consumer.resume(&id).await.unwrap_err();
            assert!(refusal.contains("(cancellation_requested)"), "{refusal}");
            assert_eq!(*state.submissions.lock().unwrap(), [original.clone()]);
            assert!(consumer
                .load_account(&id)
                .unwrap()
                .funding_cancellation
                .is_none());
            // Canonical at safe again: plain resume opens once from the original bytes.
            settle_fixture(&state, &original, true);
            state.open_pending.store(false, Ordering::SeqCst);
            let opens = event_count(&state, "open");
            let account = consumer.resume(&id).await.unwrap();
            assert_eq!(account.funded_wei, "100");
            assert_eq!(event_count(&state, "open"), opens + 1);
            assert_eq!(
                consumer
                    .load_account(&id)
                    .unwrap()
                    .funding_cancellation
                    .unwrap()
                    .status,
                "original_accepted"
            );
            consumer.resume(&id).await.unwrap();
            assert_eq!(event_count(&state, "open"), opens + 1);
            assert_eq!(*state.submissions.lock().unwrap(), [original.clone()]);
            let journal = consumer.history().await.unwrap();
            assert_eq!(journal["transactions"].as_array().unwrap().len(), 1);
            assert!(journal["transactions"][0]["cancellation"].is_null());
            task.abort();
        }
    }
    /// Task 2.8 setup: an open account's top-up original is broadcast but not yet credited,
    /// then its explicit cancellation attempt fails before journaling a replacement, because
    /// the wallet cannot afford the cancellation gas. The original's nonce is still current,
    /// so `send_payment_once` would rebroadcast it. Returns the opening and top-up original
    /// bytes and the top-up operation ID; the provider now credits any top-up it is sent.
    async fn failed_topup_cancellation_attempt(
        consumer: &Consumer,
        saved: &SavedAccount,
        state: &FundingFixture,
    ) -> (String, String, String) {
        use std::sync::atomic::Ordering;
        let id = &saved.quote.payload.id;
        consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        let opening = state.submissions.lock().unwrap()[0].clone();
        settle_fixture(state, &opening, true);
        retain_fixture_receipt(state, &opening);
        state.open_pending.store(true, Ordering::SeqCst);
        let error = consumer.topup(id, "50").await.unwrap_err();
        assert!(error.contains("funding_pending"), "{error}");
        let original = state.submissions.lock().unwrap()[1].clone();
        *state.balance.lock().unwrap() = 0;
        let error = consumer.cancel_funding(id, None).await.unwrap_err();
        assert!(
            error.contains("insufficient balance for explicit cancellation gas"),
            "{error}"
        );
        *state.balance.lock().unwrap() = u128::MAX;
        let requested = consumer.load_account(id).unwrap();
        let intent = requested
            .topup
            .as_ref()
            .expect("the top-up intent stays pending");
        assert!(
            intent.cancellation_requested,
            "the request is durable before the attempt"
        );
        assert_eq!(intent.raw_transaction.as_ref(), Some(&original));
        let operation = format!("topup:{id}:{}", intent.id);
        let journal = consumer.history().await.unwrap();
        let entry = journal_entry(&journal, &operation).unwrap().unwrap();
        assert!(
            entry["cancellation"].is_null(),
            "no replacement was journaled"
        );
        assert_eq!(
            *state.submissions.lock().unwrap(),
            [opening.clone(), original.clone()]
        );
        state.open_pending.store(false, Ordering::SeqCst);
        (opening, original, operation)
    }
    #[tokio::test]
    async fn failed_topup_cancellation_attempt_keeps_the_request_and_never_rebroadcasts_the_original(
    ) {
        // Task 2.8, requirement 16. The top-up's cancellation attempt fails before journaling
        // a replacement. resume, topup and fund, also after a restart, never broadcast the
        // original or pay again for that operation. An explicit cancel-funding retry still
        // submits the one replacement, and once it wins, resume clears the intent without
        // credit and refreshes the account.
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = funding_fixture(directory.path(), false, false).await;
        let id = saved.quote.payload.id.clone();
        let (opening, original, operation) =
            failed_topup_cancellation_attempt(&consumer, &saved, &state).await;
        let attempted = [opening.clone(), original.clone()];
        let topups = event_count(&state, "topup");
        let resumed = consumer.resume(&id).await;
        assert_eq!(
            *state.submissions.lock().unwrap(),
            attempted,
            "resume must not rebroadcast the original"
        );
        let refusal = resumed.unwrap_err();
        assert!(
            refusal.contains("cancellation was requested")
                && refusal.contains("(cancellation_requested)"),
            "{refusal}"
        );
        for amount in ["50", "60"] {
            let refusal = consumer.topup(&id, amount).await.unwrap_err();
            assert!(refusal.contains("cancellation was requested"), "{refusal}");
        }
        let refusal = consumer.fund(&saved.offer, &saved.quote).await.unwrap_err();
        assert!(refusal.contains("cancellation was requested"), "{refusal}");
        assert_eq!(*state.submissions.lock().unwrap(), attempted);
        assert_eq!(event_count(&state, "topup"), topups);
        assert!(state.credited_topups.lock().unwrap().is_empty());
        assert_eq!(
            consumer.pending_accounts().unwrap()["pending"][0]["funding_stage"],
            "topup_cancellation_requested"
        );
        // The request survives a restart.
        let config = consumer.config.clone();
        drop(consumer);
        let consumer = Consumer::new(config).unwrap();
        assert!(
            consumer
                .load_account(&id)
                .unwrap()
                .topup
                .expect("the top-up intent stays pending")
                .cancellation_requested
        );
        assert!(consumer.resume(&id).await.is_err());
        assert_eq!(*state.submissions.lock().unwrap(), attempted);
        // The explicit cancellation retry for the same operation still works.
        let retried = consumer.cancel_funding(&id, None).await.unwrap();
        assert_eq!(retried["status"], "cancellation_pending");
        assert_eq!(retried["operation_id"], operation.as_str());
        let replacement = state.submissions.lock().unwrap()[2].clone();
        assert!(!attempted.contains(&replacement));
        let refusal = consumer.resume(&id).await.unwrap_err();
        assert!(refusal.contains("(cancellation_pending)"), "{refusal}");
        assert_eq!(state.submissions.lock().unwrap().len(), 3);
        // The replacement wins: resume clears the intent, credits nothing and refreshes.
        settle_fixture(&state, &replacement, true);
        assert_eq!(consumer.resume(&id).await.unwrap().available_wei, "100");
        let after = consumer.load_account(&id).unwrap();
        assert!(after.topup.is_none());
        assert!(after.won_topups.is_empty());
        assert_eq!(event_count(&state, "topup"), topups);
        assert!(state.credited_topups.lock().unwrap().is_empty());
        assert_eq!(
            *state.submissions.lock().unwrap(),
            [opening, original, replacement]
        );
        let recovery = consumer.funding_recovery(&id).await.unwrap();
        let entry = recovery["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["operation_id"] == operation.as_str())
            .unwrap()
            .clone();
        assert_eq!(entry["cancellation"]["status"], "cancelled");
        task.abort();
    }
    #[tokio::test]
    async fn won_topup_original_after_a_failed_cancellation_attempt_is_credited_once_by_resume() {
        // Task 2.8 with task 2.6. After the failed attempt, resume neither broadcasts nor
        // credits the unresolved original. Once the original (broadcast before the request)
        // is accepted at safe, plain resume, with or without a prior funding-recovery and
        // restart, credits it exactly once from its journaled bytes with no replacement ever
        // signed or submitted, and a second resume does not post it again.
        for inspect_first in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, saved, state, task) =
                funding_fixture(directory.path(), false, false).await;
            let id = saved.quote.payload.id.clone();
            let (opening, original, operation) =
                failed_topup_cancellation_attempt(&consumer, &saved, &state).await;
            let attempted = [opening.clone(), original.clone()];
            let topups = event_count(&state, "topup");
            let resumed = consumer.resume(&id).await;
            assert_eq!(
                *state.submissions.lock().unwrap(),
                attempted,
                "resume must not rebroadcast the original"
            );
            assert_eq!(event_count(&state, "topup"), topups);
            let refusal = resumed.unwrap_err();
            assert!(refusal.contains("(cancellation_requested)"), "{refusal}");
            settle_fixture(&state, &original, true);
            let consumer = if inspect_first {
                let recovery = consumer.funding_recovery(&id).await.unwrap();
                let entry = recovery["operations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry["operation_id"] == operation.as_str())
                    .unwrap()
                    .clone();
                assert_eq!(entry["status"], "accepted");
                assert!(entry["cancellation"].is_null());
                let recorded = consumer.load_account(&id).unwrap();
                assert_eq!(recorded.won_topups.len(), 1);
                assert_eq!(recorded.won_topups[0].operation_id, operation);
                assert!(!recorded.won_topups[0].credited);
                assert!(recorded.topup.unwrap().cancellation_requested);
                // Inspection records the outcome but never credits or submits anything.
                assert!(state.credited_topups.lock().unwrap().is_empty());
                let config = consumer.config.clone();
                drop(consumer);
                Consumer::new(config).unwrap()
            } else {
                consumer
            };
            let account = consumer.resume(&id).await.unwrap();
            assert_eq!(account.available_wei, "150");
            assert_eq!(event_count(&state, "topup"), topups + 1);
            assert_eq!(
                *state.credited_topups.lock().unwrap(),
                std::collections::HashMap::from([(original.clone(), 50u128)])
            );
            let after = consumer.load_account(&id).unwrap();
            assert!(after.topup.is_none());
            assert_eq!(after.won_topups.len(), 1);
            assert_eq!(after.won_topups[0].operation_id, operation);
            assert!(after.won_topups[0].credited);
            // A second resume never posts the original again.
            consumer.resume(&id).await.unwrap();
            assert_eq!(event_count(&state, "topup"), topups + 1);
            assert_eq!(state.credited_topups.lock().unwrap().len(), 1);
            // Nothing was rebroadcast, and no replacement was signed or submitted.
            assert_eq!(*state.submissions.lock().unwrap(), attempted);
            let journal = consumer.history().await.unwrap();
            assert_eq!(journal["transactions"].as_array().unwrap().len(), 2);
            assert!(
                journal_entry(&journal, &operation).unwrap().unwrap()["cancellation"].is_null()
            );
            task.abort();
        }
    }
    #[tokio::test]
    async fn missing_malformed_or_mismatched_ack_never_broadcasts() {
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = funding_fixture(directory.path(), false, false).await;
        let mut original = None;
        for mode in [1, 2, 3, 4] {
            state.mode.store(mode, Ordering::SeqCst);
            assert!(consumer.fund(&saved.offer, &saved.quote).await.is_err());
            assert!(state.submissions.lock().unwrap().is_empty());
            let persisted = consumer.load_account(&saved.quote.payload.id).unwrap();
            assert!(persisted.funding_intent.is_none());
            assert!(!persisted.funding_broadcast_started);
            if let Some(raw) = &original {
                assert_eq!(persisted.raw_transaction.as_ref(), Some(raw));
            } else {
                original = persisted.raw_transaction;
            }
            let pending = consumer.pending_accounts().unwrap();
            assert_eq!(
                pending["pending"][0]["funding_stage"],
                "registration_pending"
            );
            assert!(!pending.to_string().contains("raw_transaction"));
            assert!(!pending.to_string().contains("signature"));
        }
        state.mode.store(0, Ordering::SeqCst);
        consumer.resume(&saved.quote.payload.id).await.unwrap();
        assert_eq!(
            state.submissions.lock().unwrap().as_slice(),
            [original.unwrap()]
        );
        task.abort();
    }
    #[tokio::test]
    async fn paid_funding_persists_ack_before_broadcast_and_free_open_skips_intent() {
        for free in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, saved, state, task) =
                funding_fixture(directory.path(), false, free).await;
            consumer.fund(&saved.offer, &saved.quote).await.unwrap();
            let events = state.events.lock().unwrap();
            if free {
                assert_eq!(events.as_slice(), ["open"]);
                assert!(state.submissions.lock().unwrap().is_empty());
            } else {
                assert_eq!(events.as_slice(), ["register", "broadcast", "open"]);
            }
            assert!(consumer.pending_accounts().unwrap()["pending"]
                .as_array()
                .unwrap()
                .is_empty());
            task.abort();
        }
    }
    #[tokio::test]
    async fn agreement_exposes_only_public_disclosure_terms() {
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, _state, task) = funding_fixture(directory.path(), false, false).await;
        let id = &saved.quote.payload.id;
        consumer.fund(&saved.offer, &saved.quote).await.unwrap();
        let terms = consumer.agreement(id).unwrap();
        assert_eq!(
            terms,
            json!({"contract_id":id,"provider_wallet":saved.quote.payload.provider_wallet,
                "endpoint":saved.offer.payload.endpoint,"peer_id":saved.offer.payload.peer_id,
                "model":saved.quote.payload.model.id,"fee_bps":saved.quote.payload.fee_bps})
        );
        // Public data only: no credential, key, signature or signed payment bytes.
        let bearer = consumer.credential_export(id, None).unwrap()["api_key"].as_str().unwrap().to_owned();
        let text = terms.to_string();
        assert!(!text.contains(&bearer));
        assert!(!text.contains(&saved.quote.signature));
        assert!(!text.contains("raw_transaction") && !text.contains("api_key"));
        assert!(consumer.agreement("missing-agreement").is_err());
        task.abort();
    }
    #[tokio::test]
    async fn resumed_open_accounts_preserve_exported_credentials_and_refresh_funding_status() {
        for free in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, saved, state, task) =
                funding_fixture(directory.path(), false, free).await;
            let id = &saved.quote.payload.id;
            consumer.fund(&saved.offer, &saved.quote).await.unwrap();
            let exported = consumer.credential_export(id, None).unwrap();
            let config = consumer.config.clone();
            drop(consumer);
            let restarted = Consumer::new(config).unwrap();
            state.mode.store(5, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(restarted.resume(id).await.unwrap().status, "frozen");
            assert_eq!(
                restarted
                    .fund(&saved.offer, &saved.quote)
                    .await
                    .unwrap()
                    .status,
                "frozen"
            );
            assert_eq!(
                restarted.credential_export(id, None).unwrap()["api_key"],
                exported["api_key"]
            );
            assert_eq!(
                restarted.load_account(id).unwrap().account.unwrap().status,
                "frozen"
            );
            task.abort();
        }
    }
    #[tokio::test]
    async fn funding_crash_boundaries_resume_original_bytes_and_accepted_expired_terms() {
        use std::sync::atomic::Ordering;
        for checkpoint in 0..5 {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, mut saved, state, task) =
                funding_fixture(directory.path(), checkpoint > 0, false).await;
            let q = &saved.quote.payload;
            let original = consumer
                .chain
                .prepare_payment_once(
                    &format!("open:{}", q.id),
                    &consumer.key().unwrap(),
                    &q.provider_wallet,
                    &q.funding_wei,
                    &protocol::opening_data(q).unwrap(),
                )
                .await
                .unwrap();
            if checkpoint > 0 {
                let intent = accepted_funding(q, original.tx_hash.clone(), &state.key);
                *state.intent.lock().unwrap() = Some(intent.clone());
                *state.raw.lock().unwrap() = Some(original.raw_transaction.clone());
                saved.raw_transaction = Some(original.raw_transaction.clone());
                saved.tx_hash = Some(original.tx_hash.clone());
                if checkpoint >= 2 {
                    saved.funding_intent = Some(intent);
                }
                if checkpoint >= 3 {
                    saved.funding_broadcast_started = true;
                }
            }
            consumer.save_account(&saved).unwrap();
            if checkpoint == 4 {
                let q = &saved.quote.payload;
                consumer
                    .chain
                    .send_payment_once(
                        &format!("open:{}", q.id),
                        &consumer.key().unwrap(),
                        &q.provider_wallet,
                        &q.funding_wei,
                        &protocol::opening_data(q).unwrap(),
                    )
                    .await
                    .unwrap();
            }
            let config = consumer.config.clone();
            drop(consumer);
            let consumer = Consumer::new(config).unwrap();
            state.open_pending.store(true, Ordering::SeqCst);
            assert!(consumer
                .resume(&saved.quote.payload.id)
                .await
                .unwrap_err()
                .contains("funding_pending"));
            let pending = consumer.pending_accounts().unwrap();
            assert_eq!(pending["pending"][0]["funding_stage"], "settlement_pending");
            state.open_pending.store(false, Ordering::SeqCst);
            consumer.fund(&saved.offer, &saved.quote).await.unwrap();
            assert!(state
                .submissions
                .lock()
                .unwrap()
                .iter()
                .all(|raw| raw == &original.raw_transaction));
            assert_eq!(
                consumer
                    .chain
                    .history(&consumer.address().unwrap())
                    .await
                    .unwrap()["transactions"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                state
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|event| **event == "register")
                    .count(),
                if checkpoint < 2 { 1 } else { 0 }
            );
            task.abort();
        }
    }
    #[tokio::test]
    async fn missing_journal_never_replaces_a_saved_original_transaction() {
        let directory = tempfile::tempdir().unwrap();
        let (consumer, mut saved, state, task) =
            funding_fixture(directory.path(), false, false).await;
        saved.raw_transaction = Some("0x00".into());
        saved.tx_hash = Some(format!("0x{}", "11".repeat(32)));
        consumer.save_account(&saved).unwrap();
        assert!(consumer
            .resume(&saved.quote.payload.id)
            .await
            .unwrap_err()
            .contains("refusing to sign a replacement"));
        assert!(state.submissions.lock().unwrap().is_empty());
        assert!(state.events.lock().unwrap().is_empty());
        assert!(consumer
            .chain
            .history(&consumer.address().unwrap())
            .await
            .unwrap()["transactions"]
            .as_array()
            .unwrap()
            .is_empty());
        task.abort();
    }
    #[tokio::test]
    async fn expired_unaccepted_quotes_never_broadcast_or_replace_preparation() {
        for prepared in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (consumer, mut saved, state, task) =
                funding_fixture(directory.path(), true, false).await;
            if prepared {
                let q = &saved.quote.payload;
                let tx = consumer
                    .chain
                    .prepare_payment_once(
                        &format!("open:{}", q.id),
                        &consumer.key().unwrap(),
                        &q.provider_wallet,
                        &q.funding_wei,
                        &protocol::opening_data(q).unwrap(),
                    )
                    .await
                    .unwrap();
                saved.raw_transaction = Some(tx.raw_transaction);
                saved.tx_hash = Some(tx.tx_hash);
            }
            consumer.save_account(&saved).unwrap();
            for _ in 0..2 {
                assert!(consumer.resume(&saved.quote.payload.id).await.is_err());
            }
            assert!(state.submissions.lock().unwrap().is_empty());
            assert!(state.intent.lock().unwrap().is_none());
            let history = consumer
                .chain
                .history(&consumer.address().unwrap())
                .await
                .unwrap();
            assert_eq!(
                history["transactions"].as_array().unwrap().len(),
                if prepared { 1 } else { 0 }
            );
            if prepared {
                assert_eq!(
                    history["transactions"][0]["raw_transaction"].as_str(),
                    saved.raw_transaction.as_deref()
                );
                assert_eq!(
                    consumer.pending_accounts().unwrap()["pending"][0]["funding_stage"],
                    "registration_pending"
                );
            }
            task.abort();
        }
    }
    #[tokio::test]
    async fn expired_unacknowledged_opening_recovers_only_by_explicit_durable_cancellation() {
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (consumer, mut saved, state, task) =
            funding_fixture(directory.path(), true, false).await;
        let q = &saved.quote.payload;
        let id = q.id.clone();
        let original = consumer
            .chain
            .prepare_payment_once(
                &format!("open:{id}"),
                &consumer.key().unwrap(),
                &q.provider_wallet,
                &q.funding_wei,
                &protocol::opening_data(q).unwrap(),
            )
            .await
            .unwrap();
        saved.raw_transaction = Some(original.raw_transaction.clone());
        saved.tx_hash = Some(original.tx_hash.clone());
        consumer.save_account(&saved).unwrap();
        // The provider has the signed bytes, but supplies no acknowledgement.
        state.mode.store(1, Ordering::SeqCst);
        assert!(consumer.resume(&id).await.is_err());
        assert_eq!(
            state.raw.lock().unwrap().as_ref(),
            Some(&original.raw_transaction)
        );
        assert!(state.submissions.lock().unwrap().is_empty());
        assert_eq!(
            consumer.cancel_funding(&id, None).await.unwrap()["status"],
            "cancellation_pending"
        );
        let replacement = state.submissions.lock().unwrap()[0].clone();
        assert_ne!(replacement, original.raw_transaction);
        consumer
            .chain
            .validate_payment_envelope(
                &replacement,
                &consumer.address().unwrap(),
                &consumer.address().unwrap(),
                "0",
                &[],
            )
            .unwrap();
        let refusal = consumer.resume(&id).await.unwrap_err();
        assert!(refusal.contains("resume settles it with the provider automatically"));
        assert!(refusal.contains(&format!("funding-recovery {id}")));
        assert!(refusal.contains("Do not fund this agreement again"));
        assert!(!refusal.contains("cancel-funding"));
        let config = consumer.config.clone();
        drop(consumer);
        let consumer = Consumer::new(config).unwrap();
        assert_eq!(
            consumer.cancel_funding(&id, None).await.unwrap()["status"],
            "cancellation_pending"
        );
        assert_eq!(
            state.submissions.lock().unwrap().as_slice(),
            [replacement.clone(), replacement.clone()]
        );
        let hash = protocol::hash_hex(&hex::decode(&replacement[2..]).unwrap());
        *state.receipt.lock().unwrap() = json!({"transactionHash":hash,"from":consumer.address().unwrap(),"to":consumer.address().unwrap(),
            "type":"0x0","gasUsed":"0x5208","status":"0x1","blockHash":format!("0x{}","77".repeat(32)),"blockNumber":"0x9"});
        state.nonce.store(1, Ordering::SeqCst);
        assert_eq!(
            consumer.cancel_funding(&id, None).await.unwrap()["status"],
            "cancelled"
        );
        let pending = consumer.pending_accounts().unwrap();
        assert_eq!(pending["pending"][0]["funding_stage"], "cancelled");
        assert!(!pending.to_string().contains("raw_transaction"));
        assert!(!pending.to_string().contains("signature"));
        assert_eq!(
            consumer.load_account(&id).unwrap().raw_transaction.as_ref(),
            Some(&original.raw_transaction)
        );
        assert!(consumer.resume(&id).await.is_err());
        assert_eq!(state.submissions.lock().unwrap().len(), 2);
        task.abort();
    }
    #[tokio::test]
    async fn cancelling_before_preparation_never_creates_a_payment_or_reopens_the_intent() {
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = funding_fixture(directory.path(), false, false).await;
        consumer.save_account(&saved).unwrap();
        let id = &saved.quote.payload.id;
        assert_eq!(
            consumer.cancel_funding(id, None).await.unwrap()["status"],
            "not_prepared"
        );
        assert!(consumer.resume(id).await.is_err());
        assert!(state.submissions.lock().unwrap().is_empty());
        assert_eq!(
            consumer.pending_accounts().unwrap()["pending"][0]["funding_stage"],
            "not_prepared"
        );
        task.abort();
    }
    #[tokio::test]
    async fn topup_retries_and_resume_resubmit_exact_original_after_dropped_broadcast() {
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (consumer, mut saved, state, task) =
            funding_fixture(directory.path(), true, false).await;
        saved.account = Some(funding_account(&saved.quote.payload));
        saved.topup = Some(TopupIntent {
            id: "existing-topup".into(),
            amount_wei: "50".into(),
            raw_transaction: None,
            cancellation_requested: false,
        });
        consumer.save_account(&saved).unwrap();
        assert!(consumer.pending_accounts().unwrap()["pending"][0]["tx_hash"].is_null());
        let id = saved.quote.payload.id.clone();
        state.open_pending.store(true, Ordering::SeqCst);
        assert!(consumer
            .topup(&id, "50")
            .await
            .unwrap_err()
            .contains("funding_pending"));
        let original = state.submissions.lock().unwrap()[0].clone();
        assert_eq!(consumer.pending_accounts().unwrap()["pending"][0]["tx_hash"],
            protocol::hash_hex(&hex::decode(original.trim_start_matches("0x")).unwrap()));
        assert!(consumer
            .topup(&id, "50")
            .await
            .unwrap_err()
            .contains("funding_pending"));
        assert_eq!(
            state.submissions.lock().unwrap().as_slice(),
            [original.clone(), original.clone()]
        );
        let config = consumer.config.clone();
        drop(consumer);
        let consumer = Consumer::new(config).unwrap();
        assert!(consumer
            .resume(&id)
            .await
            .unwrap_err()
            .contains("funding_pending"));
        state.open_pending.store(false, Ordering::SeqCst);
        consumer.resume(&id).await.unwrap();
        assert_eq!(
            state.submissions.lock().unwrap().as_slice(),
            [
                original.clone(),
                original.clone(),
                original.clone(),
                original
            ]
        );
        assert_eq!(
            state
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "topup")
                .count(),
            4
        );
        assert!(!state
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(*event, "open" | "register")));
        let history = consumer.history().await.unwrap();
        assert_eq!(history["transactions"].as_array().unwrap().len(), 1);
        assert_eq!(
            history["transactions"][0]["operation_id"],
            format!("topup:{id}:existing-topup")
        );
        assert!(consumer.load_account(&id).unwrap().topup.is_none());
        assert!(consumer
            .cancel_funding(&id, None)
            .await
            .unwrap_err()
            .contains("no pending topup"));
        task.abort();
    }
    #[tokio::test]
    async fn missing_topup_journal_never_silently_signs_different_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let (consumer, mut saved, state, task) =
            funding_fixture(directory.path(), false, false).await;
        saved.account = Some(funding_account(&saved.quote.payload));
        saved.topup = Some(TopupIntent {
            id: "missing".into(),
            amount_wei: "50".into(),
            raw_transaction: Some("0x00".into()),
            cancellation_requested: false,
        });
        consumer.save_account(&saved).unwrap();
        assert!(consumer
            .topup(&saved.quote.payload.id, "50")
            .await
            .unwrap_err()
            .contains("original topup journal"));
        assert!(consumer
            .resume(&saved.quote.payload.id)
            .await
            .unwrap_err()
            .contains("original topup journal"));
        assert!(state.submissions.lock().unwrap().is_empty());
        task.abort();
    }
    #[test]
    fn owner_challenge_cannot_be_substituted_across_body_or_provider() {
        let mut payload = crate::auth::AuthPayload {
            network: configuration(Path::new(".")).network.identity,
            provider_wallet: format!("0x{}", "11".repeat(20)),
            wallet: format!("0x{}", "22".repeat(20)),
            method: "POST".into(),
            path: "/v1/contracts/open".into(),
            body_hash: protocol::hash_hex(b"original"),
            nonce: "one-use".into(),
            expires_at: 200,
        };
        let bytes = crate::auth::challenge_payload(&payload).unwrap();
        let challenge = Challenge {
            nonce: payload.nonce.clone(),
            payload_hex: format!("0x{}", hex::encode(&bytes)),
            expires_at: 200,
        };
        assert!(validate_challenge(&challenge, &bytes, 100).is_ok());
        payload.body_hash = protocol::hash_hex(b"substituted");
        assert!(validate_challenge(
            &challenge,
            &crate::auth::challenge_payload(&payload).unwrap(),
            100
        )
        .is_err());
        payload.body_hash = protocol::hash_hex(b"original");
        payload.provider_wallet = format!("0x{}", "33".repeat(20));
        assert!(validate_challenge(
            &challenge,
            &crate::auth::challenge_payload(&payload).unwrap(),
            100
        )
        .is_err());
    }
    #[test]
    fn errors_do_not_echo_untrusted_messages_or_codes() {
        let malicious = json!({"error":{"code":"secret-token","message":"wallet-key"}});
        let (code, message) = safe_error(&malicious);
        assert_eq!(code, "provider_error");
        assert!(!message.contains("wallet-key"));
        assert_eq!(
            safe_error(&json!({"error":{"code":"funding_pending","message":"secret"}})).0,
            "funding_pending"
        );
        assert_eq!(
            safe_error(&json!({"error":{"code":"insufficient_balance"}})).0,
            "insufficient_balance"
        );
    }
    #[test]
    fn urls_reject_credential_and_path_confusion() {
        for base in [
            "http://provider.example",
            "https://user:secret@provider.example",
            "https://provider.example/v1",
            "https://provider.example?x=1",
            "https://provider.example/#frag",
            "http://127.0.0.1",
        ] {
            assert!(endpoint_url(base, &["v1"], false).is_err());
        }
        assert!(endpoint_url("http://127.0.0.1:9419", &["v1"], true).is_ok());
        assert!(endpoint_url("http://10.0.0.1", &[], true).is_err());
        assert!(endpoint_url("https://example.com", &[".."], false).is_err());
        assert!(endpoint_url("https://example.com", &["a/b"], false).is_err());
        assert_eq!(
            endpoint_url("https://example.com", &["v1", "a?b#c"], false)
                .unwrap()
                .path(),
            "/v1/a%3Fb%23c"
        );
    }
    #[test]
    fn challenges_require_exact_bytes_and_freshness() {
        let mut challenge = Challenge {
            nonce: "n".into(),
            payload_hex: "0x616263".into(),
            expires_at: 200,
        };
        assert!(validate_challenge(&challenge, b"abc", 100).is_ok());
        assert!(validate_challenge(&challenge, b"abd", 100).is_err());
        assert!(validate_challenge(&challenge, b"abc", 200).is_err());
        challenge.expires_at = 1000;
        assert!(validate_challenge(&challenge, b"abc", 100).is_err());
    }
    #[test]
    fn sse_handles_split_unicode_crlf_and_usage() {
        let mut parser = SseParser::default();
        let input="data: {\"text\":\"é\"}\r\n\r\ndata: {\"usage\":{\"prompt_tokens\":2}}\n\ndata: [DONE]\n\n".as_bytes();
        let mut frames = Vec::new();
        for byte in input {
            frames.extend(parser.push(&[*byte]).unwrap());
        }
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[2], "[DONE]");
        assert!(frames[1].contains("prompt_tokens"));
    }
    #[test]
    fn sse_rejects_invalid_encoding_and_oversized_events() {
        assert!(SseParser::default().push(b"data: \xff\n\n").is_err());
        assert!(SseParser::default()
            .push(&vec![b'x'; 1024 * 1024 + 1])
            .is_err());
    }
    #[cfg(unix)]
    #[test]
    fn persisted_credentials_are_private_and_symlinks_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        let protected = directory.path().join("protected");
        wallet::secure_directory(&protected).unwrap();
        let path = protected.join("credential.json");
        private_write(&path, b"secret").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = protected.join("link");
        symlink(&path, &link).unwrap();
        assert!(private_read(&link).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(private_read(&path).is_err());
    }
    /// One loopback origin serving a live, ready provider and a reputation catalog that
    /// answers every rating request with `answer`, counting catalog requests.
    struct CatalogFixture {
        answer: std::sync::Mutex<(u16, String, &'static str)>,
        requests: std::sync::atomic::AtomicU64,
    }
    async fn catalog_answer(
        axum::extract::State(state): axum::extract::State<Arc<CatalogFixture>>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        state
            .requests
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (status, body, content_type) = state.answer.lock().unwrap().clone();
        (
            axum::http::StatusCode::from_u16(status).unwrap(),
            [(axum::http::header::CONTENT_TYPE, content_type)],
            body,
        )
            .into_response()
    }
    async fn catalog_fixture(
        path: &Path,
    ) -> (
        Consumer,
        SavedAccount,
        Arc<CatalogFixture>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let mut config = configuration(path);
        config.development = true;
        config.catalog_url = Some(origin.clone());
        let consumer = Consumer::new(config).unwrap();
        consumer.wallet_create().unwrap();
        let key = wallet::generate_private_key();
        let (offer, quote) = funding_terms(&consumer, &key, &origin, false, false);
        let mut saved = saved_funding(offer, quote);
        saved.raw_transaction = Some("0x01".into());
        saved.tx_hash = Some(format!("0x{}", "44".repeat(32)));
        consumer.save_account(&saved).unwrap();
        let state = Arc::new(CatalogFixture {
            answer: std::sync::Mutex::new((200, "{}".into(), "application/json")),
            requests: std::sync::atomic::AtomicU64::new(0),
        });
        let app = axum::Router::new()
            .route(
                "/healthz",
                axum::routing::get(|| async { axum::Json(json!({"status":"live"})) }),
            )
            .route(
                "/readyz",
                axum::routing::get(|| async { axum::Json(json!({"ready":true})) }),
            )
            .route("/api/ratings/feedback", axum::routing::post(catalog_answer))
            .route("/api/ratings/:wallet", axum::routing::get(catalog_answer))
            .with_state(state.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (consumer, saved, state, task)
    }
    /// Catalog-supplied text that must never be echoed.
    const CATALOG_TEXT: &str = "catalog-supplied detail";
    fn catalog_error_body(code: &str) -> String {
        json!({"error":{"message":CATALOG_TEXT,"type":"invalid_request_error","param":null,"code":code}})
            .to_string()
    }
    fn assert_catalog_answer(error: &str, code: &str) {
        assert!(error.starts_with(&format!("{code}: ")), "{error}");
        assert!(error.contains("reputation catalog") || error.contains("Reputation catalog"), "{error}");
        for provider_text in ["Provider operation failed", "provider_error", CATALOG_TEXT] {
            assert!(!error.contains(provider_text), "{error}");
        }
    }
    #[tokio::test]
    async fn feedback_reports_catalog_codes_not_provider_failures() {
        // payment-gated-reputation requirement 11, task 2.3 (design code difference 3).
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = catalog_fixture(directory.path()).await;
        let id = &saved.quote.payload.id;
        for (status, code) in [
            (429, "feedback_busy"),
            (400, "feedback_conflict"),
            (409, "payment_pending"),
            (400, "invalid_feedback"),
            (400, "expired_feedback"),
            (400, "payment_rejected"),
            (503, "chain_unavailable"),
            (503, "chain_timeout"),
            (500, "internal_error"),
        ] {
            *state.answer.lock().unwrap() = (status, catalog_error_body(code), "application/json");
            let error = consumer.feedback(id, 80, 2).await.unwrap_err();
            assert_catalog_answer(&error, code);
            assert!(error.ends_with(&format!("(catalog HTTP {status})")), "{error}");
        }
        // The body limit's plain-text 413, and a code the catalog does not define.
        *state.answer.lock().unwrap() = (413, CATALOG_TEXT.into(), "text/plain");
        assert_catalog_answer(&consumer.feedback(id, 80, 2).await.unwrap_err(), "feedback_too_large");
        *state.answer.lock().unwrap() = (418, catalog_error_body("teapot"), "application/json");
        assert_catalog_answer(&consumer.feedback(id, 80, 2).await.unwrap_err(), "catalog_error");
        assert_eq!(state.requests.load(std::sync::atomic::Ordering::SeqCst), 11);
        // A success is still returned as the catalog's body.
        *state.answer.lock().unwrap() = (200, json!({"accepted":true}).to_string(), "application/json");
        assert_eq!(consumer.feedback(id, 80, 2).await.unwrap(), json!({"accepted":true}));
        task.abort();
    }
    #[tokio::test]
    async fn provider_info_reports_catalog_outage_without_blaming_provider() {
        // payment-gated-reputation requirement 12, task 2.3: book line 443.
        let directory = tempfile::tempdir().unwrap();
        let (consumer, saved, state, task) = catalog_fixture(directory.path()).await;
        for (status, code) in [
            (503, "chain_unavailable"),
            (503, "chain_timeout"),
            (429, "feedback_busy"),
            (400, "invalid_wallet"),
        ] {
            *state.answer.lock().unwrap() = (status, catalog_error_body(code), "application/json");
            let info = consumer.provider_info(&saved.offer).await.unwrap();
            assert_eq!(info["reputation"]["available"], false, "{info}");
            assert!(info["reputation"].get("snapshot").is_none(), "{info}");
            assert_catalog_answer(info["reputation"]["error"].as_str().unwrap(), code);
            // Health and readiness are observed and reported unchanged.
            assert_eq!(info["health"]["live"], true, "{info}");
            assert_eq!(info["readiness"]["ready"], true, "{info}");
        }
        assert_eq!(state.requests.load(std::sync::atomic::Ordering::SeqCst), 4);
        task.abort();
    }
    #[test]
    fn catalog_and_provider_error_tables_stay_separate() {
        // Provider responses keep the provider table; catalog codes are not provider codes.
        let body = |code: &str| json!({"error":{"code":code}});
        assert_eq!(safe_error(&body("funding_pending")).0, "funding_pending");
        assert_eq!(safe_error(&body("chain_unavailable")).0, "provider_error");
        assert_eq!(catalog_error(503, &body("chain_unavailable")).0, "chain_unavailable");
        assert_eq!(catalog_error(400, &body("funding_pending")).0, "catalog_error");
        assert_eq!(catalog_error(413, &Value::Null).0, "feedback_too_large");
        // The offer catalog and the policy source have their own tables.
        assert_eq!(offer_catalog_error(429, &body("rate_limit")).0, "catalog_busy");
        assert_eq!(offer_catalog_error(400, &body("invalid_page")).0, "invalid_page");
        assert_eq!(policy_source_error(503, &body("engine_unavailable")).0, "policy_source_unavailable");
    }
    /// A loopback origin serving one fixed answer per catalog page (page `n` at offset
    /// `50 n`) and a version policy, recording every query and counting every other
    /// request, which would be a provider contact.
    struct SourceFixture {
        pages: std::sync::Mutex<Vec<(u16, String, &'static str)>>,
        queries: std::sync::Mutex<Vec<String>>,
        policy: std::sync::Mutex<(u16, String, &'static str)>,
        policy_requests: std::sync::atomic::AtomicU64,
        other_requests: std::sync::atomic::AtomicU64,
    }
    fn fixed_answer((status, body, content_type): (u16, String, &'static str)) -> axum::response::Response {
        use axum::response::IntoResponse;
        (
            axum::http::StatusCode::from_u16(status).unwrap(),
            [(axum::http::header::CONTENT_TYPE, content_type)],
            body,
        )
            .into_response()
    }
    async fn source_fixture() -> (String, Arc<SourceFixture>, tokio::task::JoinHandle<()>) {
        use axum::extract::{RawQuery, State};
        use std::sync::atomic::Ordering;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(SourceFixture {
            pages: std::sync::Mutex::new(Vec::new()),
            queries: std::sync::Mutex::new(Vec::new()),
            policy: std::sync::Mutex::new((503, "Service Unavailable".into(), "text/plain")),
            policy_requests: std::sync::atomic::AtomicU64::new(0),
            other_requests: std::sync::atomic::AtomicU64::new(0),
        });
        let app = axum::Router::new()
            .route(
                "/api/offers",
                axum::routing::get(
                    |State(state): State<Arc<SourceFixture>>, RawQuery(query): RawQuery| async move {
                        let query = query.unwrap_or_default();
                        state.queries.lock().unwrap().push(query.clone());
                        let offset = query
                            .split('&')
                            .find_map(|pair| pair.strip_prefix("offset="))
                            .and_then(|offset| offset.parse::<usize>().ok())
                            .unwrap_or(0);
                        let answer = state.pages.lock().unwrap().get(offset / 50).cloned();
                        fixed_answer(answer.unwrap_or((404, "{}".into(), "application/json")))
                    },
                ),
            )
            .route(
                "/policy",
                axum::routing::get(|State(state): State<Arc<SourceFixture>>| async move {
                    state.policy_requests.fetch_add(1, Ordering::SeqCst);
                    let answer = state.policy.lock().unwrap().clone();
                    fixed_answer(answer)
                }),
            )
            .fallback(|State(state): State<Arc<SourceFixture>>| async move {
                state.other_requests.fetch_add(1, Ordering::SeqCst);
                fixed_answer((404, "{}".into(), "application/json"))
            })
            .with_state(state.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (origin, state, task)
    }
    fn page(offers: Vec<Value>, next_offset: Option<usize>) -> (u16, String, &'static str) {
        (200, json!({"offers":offers,"next_offset":next_offset}).to_string(), "application/json")
    }
    /// A source's error: its own code, a message naming the source, no provider wording
    /// and none of the source's own text.
    #[track_caller]
    fn assert_source_answer(error: &str, code: &str, source: &str, status: u16, suffix: &str) {
        assert!(error.starts_with(&format!("{code}: ")), "{error}");
        assert!(error.contains(source), "{error}");
        assert!(error.ends_with(&format!("({suffix} HTTP {status})")), "{error}");
        assert!(!error.to_ascii_lowercase().contains("provider"), "{error}");
        assert!(!error.contains(CATALOG_TEXT), "{error}");
    }
    #[tokio::test]
    async fn offer_catalog_errors_are_the_catalogs_not_a_providers() {
        // inference-offer-discovery requirements 13 and 14, task 2.5: book line 443.
        let directory = tempfile::tempdir().unwrap();
        let (origin, state, task) = source_fixture().await;
        let mut config = configuration(directory.path());
        config.development = true;
        config.catalog_url = Some(origin.clone());
        let consumer = Consumer::new(config).unwrap();
        consumer.wallet_create().unwrap();
        let error_answer = |status: u16, code: &str| {
            (
                status,
                json!({"error":{"message":CATALOG_TEXT,"type":"server_error","param":null,"code":code}})
                    .to_string(),
                "application/json",
            )
        };
        // 429 and 503 from the catalog, or from a proxy in front of it, in JSON (with
        // codes the provider table knows) and plain text.
        for (answer, code) in [
            (error_answer(429, "rate_limit"), "catalog_busy"),
            ((429, CATALOG_TEXT.into(), "text/plain"), "catalog_busy"),
            (error_answer(503, "upstream_failure"), "catalog_unavailable"),
            ((503, CATALOG_TEXT.into(), "text/plain"), "catalog_unavailable"),
            (error_answer(400, "invalid_page"), "invalid_page"),
            (error_answer(400, "invalid_model"), "invalid_model"),
            (error_answer(418, "teapot"), "catalog_error"),
        ] {
            let status = answer.0;
            *state.pages.lock().unwrap() = vec![answer];
            let error = consumer.discover(None).await.unwrap_err();
            assert_source_answer(&error, code, "offer catalog", status, "catalog");
        }
        // A catalog failure on a later page fails discovery and returns no offers.
        let key = wallet::generate_private_key();
        let (offer, _) = funding_terms(&consumer, &key, &origin, false, false);
        let first = serde_json::to_value(&offer).unwrap();
        *state.pages.lock().unwrap() = vec![
            page(vec![first.clone()], Some(50)),
            error_answer(503, "upstream_failure"),
        ];
        let error = consumer.discover(None).await.unwrap_err();
        assert_source_answer(&error, "catalog_unavailable", "offer catalog", 503, "catalog");
        // Successful pages are unchanged: an undecodable entry is skipped and both pages
        // are read with the same queries.
        let other = wallet::generate_private_key();
        let (second, _) = funding_terms(&consumer, &other, &origin, false, false);
        *state.pages.lock().unwrap() = vec![
            page(vec![json!({"not":"an offer"}), first.clone()], Some(50)),
            page(vec![serde_json::to_value(&second).unwrap()], None),
        ];
        state.queries.lock().unwrap().clear();
        let mut found: Vec<_> = consumer
            .discover(Some("model-a"))
            .await
            .unwrap()
            .into_iter()
            .map(|offer| offer.payload.provider_wallet)
            .collect();
        found.sort();
        let mut expected = vec![offer.payload.provider_wallet.clone(), second.payload.provider_wallet.clone()];
        expected.sort();
        assert_eq!(found, expected);
        assert_eq!(
            *state.queries.lock().unwrap(),
            ["offset=0&limit=50&model=model-a", "offset=50&limit=50&model=model-a"]
        );
        // The pagination bounds are unchanged.
        *state.pages.lock().unwrap() = vec![page(vec![first.clone()], Some(0))];
        assert_eq!(consumer.discover(None).await.unwrap_err(), "invalid catalog pagination");
        *state.pages.lock().unwrap() = vec![page(vec![json!({}); 51], None)];
        assert_eq!(
            consumer.discover(None).await.unwrap_err(),
            "catalog exceeded requested page size"
        );
        *state.pages.lock().unwrap() = (0..=100)
            .map(|n| page(vec![first.clone(); 50], Some((n + 1) * 50)))
            .collect();
        assert_eq!(
            consumer.discover(None).await.unwrap_err(),
            "catalog exceeds bounded discovery limit; narrow the model filter"
        );
        assert_eq!(state.other_requests.load(std::sync::atomic::Ordering::SeqCst), 0);
        task.abort();
    }
    #[tokio::test]
    async fn policy_url_with_a_query_is_refused_at_profile_load_before_any_request() {
        // client-version-policy requirement 9 (task 2.6): no credentials, query or fragment.
        // Enforced at profile load by network::validate_endpoint for every endpoint.
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (origin, state, task) = source_fixture().await;
        for policy_url in [
            format!("{origin}/policy?network=71"),
            format!("{origin}/policy?"),
            format!("{origin}/policy#fragment"),
            origin.replacen("http://", "http://user:secret@", 1) + "/policy",
        ] {
            let mut config = configuration(directory.path());
            config.development = true;
            config.network.policy_url = Some(policy_url.clone());
            let error = Consumer::new(config).err().expect(&policy_url);
            assert_eq!(error, "endpoint credentials, queries and fragments are forbidden; use a bearer file", "{policy_url}");
            assert!(!error.contains("secret"), "{policy_url}");
        }
        assert_eq!(state.policy_requests.load(Ordering::SeqCst), 0);
        assert_eq!(state.other_requests.load(Ordering::SeqCst), 0);
        task.abort();
    }
    #[tokio::test]
    async fn policy_source_errors_name_no_provider_and_still_fail_closed() {
        // client-version-policy requirements 9 and 11, task 2.5: book line 443.
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let (origin, state, task) = source_fixture().await;
        let mut config = configuration(directory.path());
        config.development = true;
        config.network.policy_url = Some(format!("{origin}/policy"));
        let consumer = Consumer::new(config).unwrap();
        consumer.wallet_create().unwrap();
        for (answer, code) in [
            ((503, CATALOG_TEXT.to_string(), "text/plain"), "policy_source_unavailable"),
            ((503, catalog_error_body("upstream_failure"), "application/json"), "policy_source_unavailable"),
            ((429, catalog_error_body("rate_limit"), "application/json"), "policy_source_busy"),
            ((404, catalog_error_body("not_found"), "application/json"), "policy_source_not_found"),
            ((403, catalog_error_body("unauthorized"), "application/json"), "policy_source_error"),
        ] {
            let status = answer.0;
            *state.policy.lock().unwrap() = answer;
            let error = consumer.version_status().await.unwrap_err();
            assert_source_answer(&error, code, "version policy source", status, "policy source");
        }
        // Fail-closed behaviour is unchanged: with the source answering 503, funding,
        // top-ups and inference are refused with the policy-source error before any
        // provider is contacted or any account file is written.
        *state.policy.lock().unwrap() = (503, "Service Unavailable".into(), "text/plain");
        let refused = |error: String| {
            assert_source_answer(&error, "policy_source_unavailable", "version policy source", 503, "policy source");
        };
        let key = wallet::generate_private_key();
        let (offer, quote) = funding_terms(&consumer, &key, &origin, false, false);
        let id = quote.payload.id.clone();
        refused(consumer.fund(&offer, &quote).await.unwrap_err());
        let path = consumer.account_path(&id).unwrap();
        assert!(!path.exists());
        consumer.save_account(&saved_funding(offer, quote)).unwrap();
        let before = fs::read(&path).unwrap();
        refused(consumer.topup(&id, "10").await.unwrap_err());
        let chat = json!({"model":"model-a","messages":[{"role":"user","content":"hi"}],"max_tokens":8});
        refused(consumer.infer(&id, "chat", chat.clone()).await.unwrap_err());
        refused(consumer.stream_kind(&id, "chat", chat).await.err().unwrap());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(state.other_requests.load(Ordering::SeqCst), 0);
        assert_eq!(state.policy_requests.load(Ordering::SeqCst), 9);
        task.abort();
    }
    /// A loopback listener that counts the TCP connections it accepts and answers none:
    /// a request that reached it would fail only after the count.
    async fn connection_recorder() -> (
        std::net::SocketAddr,
        Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = count.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                drop(stream);
            }
        });
        (address, count, task)
    }
    /// Runs `future` with the test resolver answering only `answers` (every other host fails
    /// to resolve, so nothing reaches DNS): its output and the hosts that reached resolution.
    async fn resolving<T>(
        answers: &[(&str, &[&str])],
        future: impl std::future::Future<Output = T>,
    ) -> (T, Vec<String>) {
        let resolver = crate::offer_registry::tests::test_resolver(answers);
        let output = crate::offer_registry::TEST_RESOLVER
            .scope(resolver.clone(), future)
            .await;
        let queries = resolver.queries.lock().unwrap().clone();
        (output, queries)
    }
    /// outbound-http-safeguards requirement 1 scenario "A non-public catalog origin is
    /// refused before any connection" (task 3.1). Without development and without `dht`,
    /// discovery and feedback fail, and the reputation read of provider inspection reports
    /// unavailable, with the origin validation error. The catalog host never reaches the
    /// resolver, and a loopback listener named as a catalog receives no connection.
    #[tokio::test]
    async fn non_conforming_catalog_origins_fail_before_resolution_or_connection() {
        let (recorder, connections, task) = connection_recorder().await;
        let loopback = format!("https://{recorder}");
        // The loopback recorder comes first, so that a request path which bypassed the
        // pinned client would fail on it before any `.example` name could reach DNS.
        for catalog in [
            loopback.as_str(),
            "https://relay.example:8443",
            "https://10.0.0.5",
            "https://relay",
            "https://relay.localhost",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut config = configuration(directory.path());
            config.catalog_url = Some(catalog.into());
            assert!(!config.development && config.dht.is_none());
            // Profile load checks only the catalog's syntax (design decision 4).
            let consumer = Consumer::new(config).unwrap();
            consumer.wallet_create().unwrap();
            let key = wallet::generate_private_key();
            let (offer, quote) =
                funding_terms(&consumer, &key, "https://provider.example", false, false);
            let id = quote.payload.id.clone();
            let mut saved = saved_funding(offer.clone(), quote);
            saved.raw_transaction = Some("0x01".into());
            saved.tx_hash = Some(format!("0x{}", "44".repeat(32)));
            consumer.save_account(&saved).unwrap();
            let expected = crate::offer_registry::validate_endpoint(catalog, false).unwrap_err();
            let ((discovered, feedback, info), queries) = resolving(&[], async {
                (
                    consumer.discover(None).await,
                    consumer.feedback(&id, 80, 2).await,
                    consumer.provider_info(&offer).await,
                )
            })
            .await;
            assert_eq!(discovered.unwrap_err(), expected, "{catalog}");
            assert_eq!(feedback.unwrap_err(), expected, "{catalog}");
            assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 0, "{catalog}");
            let info = info.unwrap();
            assert_eq!(
                info["reputation"],
                json!({"available": false, "error": expected}),
                "{catalog}"
            );
            // Only the provider's own origin, which validates, reached resolution; the test
            // resolver has no answer for it, so it was not contacted either.
            assert_eq!(queries, ["provider.example"], "{catalog}");
            assert_eq!(info["health"]["error"], "safe provider transport unavailable");
        }
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 0);
        task.abort();
    }
    /// outbound-http-safeguards requirement 1 scenario "A provider origin that fails
    /// validation is not contacted" (task 3.1). A validly signed offer whose endpoint fails
    /// validation without development is refused by inspection, quoting and funding, and a
    /// saved account at that origin by control and inference requests, each with the origin
    /// validation error: nothing reaches the resolver, the loopback listener named by the
    /// offer, or the chain RPC.
    #[tokio::test]
    async fn non_conforming_provider_origins_are_refused_before_resolution_or_connection() {
        use std::sync::atomic::Ordering;
        let (provider, provider_connections, provider_task) = connection_recorder().await;
        let (rpc, rpc_connections, rpc_task) = connection_recorder().await;
        let loopback = format!("https://{provider}");
        let secret = "sk-chiral-origin-secret-0123456789abcdef";
        for endpoint in [loopback.as_str(), "https://provider.example:8443"] {
            let directory = tempfile::tempdir().unwrap();
            let mut config = configuration(directory.path());
            config.network.rpc_url = format!("http://{rpc}");
            let consumer = Consumer::new(config).unwrap();
            consumer.wallet_create().unwrap();
            let key = wallet::generate_private_key();
            let (offer, quote) = funding_terms(&consumer, &key, endpoint, false, false);
            // The signatures are valid: only the origin is refused.
            protocol::verify(OFFER_DOMAIN, &offer, &offer.payload.provider_wallet).unwrap();
            protocol::verify(QUOTE_DOMAIN, &quote, &offer.payload.provider_wallet).unwrap();
            let expected = crate::offer_registry::validate_endpoint(endpoint, false).unwrap_err();
            let id = quote.payload.id.clone();
            let path = consumer.account_path(&id).unwrap();
            let ((info, quoted, funded), queries) = resolving(&[], async {
                (
                    consumer.provider_info(&offer).await,
                    consumer.quote(&offer, "100").await,
                    consumer.fund(&offer, &quote).await,
                )
            })
            .await;
            assert_eq!(info.unwrap_err(), expected, "{endpoint}");
            assert_eq!(quoted.unwrap_err(), expected, "{endpoint}");
            assert_eq!(funded.unwrap_err(), expected, "{endpoint}");
            assert!(queries.is_empty(), "{endpoint}: {queries:?}");
            // Current behaviour (design decision 10): funding refuses before it saves the
            // account or prepares any payment.
            assert!(!path.exists(), "{endpoint}");
            // A saved account at that origin (as an earlier release could have saved it)
            // with a stored credential: control and inference requests are refused alike,
            // and no inference request key is persisted.
            let mut saved = saved_funding(offer.clone(), quote.clone());
            let credential = Credential {
                id: "credential-a".into(),
                bearer: secret.into(),
                contract_id: id.clone(),
                expires_at: protocol::now().unwrap() + 3600,
            };
            consumer.store_credential(&mut saved, credential, true).unwrap();
            consumer.save_account(&saved).unwrap();
            let before = fs::read(&path).unwrap();
            let chat = json!({"model":"model-a","messages":[{"role":"user","content":"hi"}],"max_tokens":8});
            let (errors, queries) = resolving(&[], async {
                vec![
                    consumer.infer(&id, "chat", chat.clone()).await.unwrap_err(),
                    consumer.stream_kind(&id, "chat", chat.clone()).await.err().unwrap(),
                    consumer.request_status(&id, "request-a").await.unwrap_err(),
                    consumer.cancel_request(&id, "request-a").await.unwrap_err(),
                    consumer.receipt(&id).await.unwrap_err(),
                    consumer.account(&id).await.unwrap_err(),
                    consumer.usage(&id).await.unwrap_err(),
                    consumer.funding(&id).await.unwrap_err(),
                    consumer.credential_issue(&id, credential_options(&["read"])).await.unwrap_err(),
                    consumer.credential_revoke(&id, "credential-a").await.unwrap_err(),
                    consumer
                        .credential_rotate(&id, "credential-a", credential_options(&["infer"]))
                        .await
                        .unwrap_err(),
                    consumer.topup(&id, "10").await.unwrap_err(),
                    consumer.fund(&offer, &quote).await.unwrap_err(),
                    consumer.provider_info(&offer).await.unwrap_err(),
                ]
            })
            .await;
            for error in errors {
                assert_eq!(error, expected, "{endpoint}");
                assert!(!error.contains(secret), "{endpoint}");
            }
            assert!(queries.is_empty(), "{endpoint}: {queries:?}");
            // Compared without printing: the account file holds the stored credential.
            assert!(fs::read(&path).unwrap() == before, "{endpoint}: the account file changed");
            assert!(!consumer.account_directory().unwrap().join("requests").exists());
        }
        assert_eq!(provider_connections.load(Ordering::SeqCst), 0);
        assert_eq!(rpc_connections.load(Ordering::SeqCst), 0);
        provider_task.abort();
        rpc_task.abort();
    }
    /// outbound-http-safeguards requirement 2, task 3.2: `development` defaults to false
    /// (book line 463). A consumer profile without the member is not a development profile:
    /// a `http://127.0.0.1` catalog is refused at load, and a signed offer at a
    /// `http://127.0.0.1` origin is refused by inspection, quoting and funding before any
    /// connection. The same profile with `"development": true` loads and does connect.
    #[tokio::test]
    async fn a_consumer_profile_without_a_development_member_is_not_development() {
        use std::sync::atomic::Ordering;
        let (recorder, connections, task) = connection_recorder().await;
        let origin = format!("http://{recorder}");
        let directory = tempfile::tempdir().unwrap();
        let mut document = serde_json::to_value(configuration(&directory.path().join("state"))).unwrap();
        assert_eq!(document.as_object_mut().unwrap().remove("development"), Some(json!(false)));
        document["catalog_url"] = json!(origin);
        let profile = directory.path().join("consumer.json");
        let write = |document: &Value| fs::write(&profile, serde_json::to_vec(document).unwrap()).unwrap();
        write(&document);
        assert!(!decode::<ConsumerConfig>(&fs::read(&profile).unwrap()).unwrap().development);
        assert_eq!(
            Consumer::load(&profile).err().unwrap(),
            "HTTPS required except explicitly enabled loopback development"
        );
        // Without a catalog the profile loads, still without development.
        document["catalog_url"] = Value::Null;
        write(&document);
        let consumer = Consumer::load(&profile).unwrap();
        assert!(!consumer.config.development);
        consumer.wallet_create().unwrap();
        let key = wallet::generate_private_key();
        let (offer, quote) = funding_terms(&consumer, &key, &origin, false, false);
        let expected = crate::offer_registry::validate_endpoint(&origin, false).unwrap_err();
        let ((info, quoted, funded), queries) = resolving(&[], async {
            (
                consumer.provider_info(&offer).await,
                consumer.quote(&offer, "100").await,
                consumer.fund(&offer, &quote).await,
            )
        })
        .await;
        // `fund` checks the quote's origin with the scheme rule first.
        assert_eq!(info.unwrap_err(), expected);
        assert_eq!(quoted.unwrap_err(), expected);
        assert_eq!(
            funded.unwrap_err(),
            "HTTPS required except explicitly enabled loopback development"
        );
        assert!(!consumer.account_path(&quote.payload.id).unwrap().exists());
        assert!(queries.is_empty(), "{queries:?}");
        assert_eq!(connections.load(Ordering::SeqCst), 0);
        // Positive control: with `"development": true` the same profile loads its loopback
        // catalog, and inspecting the same offer connects to the listener.
        drop(consumer);
        document["development"] = json!(true);
        document["catalog_url"] = json!(origin);
        write(&document);
        let consumer = Consumer::load(&profile).unwrap();
        assert!(consumer.config.development);
        consumer.provider_info(&offer).await.unwrap();
        assert!(connections.load(Ordering::SeqCst) > 0);
        task.abort();
    }
    /// outbound-http-safeguards requirement 3 scenario "A catalog path is not a catalog
    /// origin" (task 3.3). A `catalog_url` with a path, query, fragment or credentials is
    /// refused before any catalog request; in current behaviour (design decision 10)
    /// `Consumer::new` refuses it at profile load. A loopback catalog named that way receives
    /// no request; the origin itself, as a positive control, does.
    #[tokio::test]
    async fn catalog_urls_with_a_path_query_fragment_or_credentials_are_refused_before_any_request() {
        use std::sync::atomic::Ordering;
        let (origin, state, task) = source_fixture().await;
        let authority = origin.trim_start_matches("http://");
        for (catalog, development) in [
            ("https://relay.example/api/offers".to_string(), false),
            ("https://relay.example/?page=1".into(), false),
            ("https://relay.example/#offers".into(), false),
            ("https://reader:secret@relay.example".into(), false),
            (format!("{origin}/api/offers"), true),
            (format!("{origin}/?page=1"), true),
            (format!("{origin}/#offers"), true),
            (format!("http://reader:secret@{authority}"), true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut config = configuration(directory.path());
            config.development = development;
            config.catalog_url = Some(catalog.clone());
            let error = Consumer::new(config).err().expect(&catalog);
            assert_eq!(
                error,
                "endpoint must be an origin without credentials, path, query or fragment",
                "{catalog}"
            );
            assert!(!error.contains("secret"), "{catalog}");
        }
        assert!(state.queries.lock().unwrap().is_empty());
        assert_eq!(state.other_requests.load(Ordering::SeqCst), 0);
        // Positive control: the origin loads, and discovery adds `/api/offers` to it.
        let directory = tempfile::tempdir().unwrap();
        let mut config = configuration(directory.path());
        config.development = true;
        config.catalog_url = Some(origin.clone());
        let consumer = Consumer::new(config).unwrap();
        *state.pages.lock().unwrap() = vec![page(Vec::new(), None)];
        assert!(consumer.discover(None).await.unwrap().is_empty());
        assert_eq!(*state.queries.lock().unwrap(), ["offset=0&limit=50"]);
        assert_eq!(state.other_requests.load(Ordering::SeqCst), 0);
        task.abort();
    }
    /// outbound-http-safeguards requirement 6 scenario "The native administration client
    /// contacts only local loopback" (task 3.6). `http://10.0.0.5:9421` and
    /// `https://provider.example` are refused with the loopback error, and the administration
    /// fixture on loopback receives nothing. Implementation values of design decision 10,
    /// pinned as such: `localhost` is refused too, and the endpoint is checked before the
    /// token file is read (a missing token file gives the same refusal).
    #[tokio::test]
    async fn the_native_administration_client_contacts_only_literal_loopback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let recorded = seen.clone();
        let app = axum::Router::new().fallback(
            move |method: axum::http::Method, uri: axum::http::Uri, headers: axum::http::HeaderMap| {
                let recorded = recorded.clone();
                async move {
                    let authorization = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    recorded.lock().unwrap().push(format!("{method} {} {authorization}", uri.path()));
                    axum::Json(json!({"ok": true}))
                }
            },
        );
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let directory = tempfile::tempdir().unwrap();
        let consumer = Consumer::new(configuration(directory.path())).unwrap();
        let token = "administration-test-token-0123456789abcdef";
        let token_file = directory.path().join("admin.token");
        wallet::create_secret_file(&token_file)
            .unwrap()
            .write_all(token.as_bytes())
            .unwrap();
        let missing = directory.path().join("missing.token");
        let refused = "administration requires a literal loopback endpoint";
        let localhost = origin.replacen("127.0.0.1", "localhost", 1);
        for endpoint in ["http://10.0.0.5:9421", "https://provider.example", localhost.as_str()] {
            for action in ["status", "drain"] {
                assert_eq!(
                    consumer.provider_admin(endpoint, &missing, action).await.unwrap_err(),
                    refused,
                    "{endpoint}"
                );
                assert_eq!(
                    consumer.provider_admin(endpoint, &token_file, action).await.unwrap_err(),
                    refused,
                    "{endpoint}"
                );
            }
        }
        assert!(seen.lock().unwrap().is_empty());
        // Positive control: the literal loopback endpoint receives the bearer.
        assert_eq!(
            consumer.provider_admin(&origin, &token_file, "status").await.unwrap(),
            json!({"ok": true})
        );
        assert_eq!(*seen.lock().unwrap(), [format!("GET /admin/status Bearer {token}")]);
        task.abort();
    }
}
