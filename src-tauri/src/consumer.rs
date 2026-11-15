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
}
#[derive(Serialize, Deserialize)]
struct TopupIntent {
    id: String,
    amount_wei: String,
    raw_transaction: Option<String>,
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
async fn response_bytes(response: Response) -> Result<Vec<u8>, String> {
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
    if !status.is_success() {
        // Classify trusted protocol codes, never echo provider-supplied text or secrets.
        let error = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
        let (code, message) = safe_error(&error);
        return Err(format!("{code}: {message} (HTTP {})", status.as_u16()));
    }
    Ok(body)
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
        return if topup.raw_transaction.is_some() {
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

impl Consumer {
    pub fn load(path: &Path) -> Result<Self, String> {
        let config = decode(&fs::read(path).map_err(|_| "cannot read consumer config")?)?;
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
                let value:Value=decode(&response_bytes(response).await?)?;
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
            let policy: crate::version::VersionPolicy = decode(&response_bytes(response).await?)?;
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
                let page: Page = decode(&response_bytes(response).await?)?;
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
    /// Does not expose signed bytes and never signs, broadcasts or bumps a fee.
    pub async fn funding_recovery(&self, id: &str) -> Result<Value, String> {
        self.load_account(id)?;
        self.chain
            .reconcile(&self.address()?)
            .await
            .map_err(|e| e.to_string())?;
        let history = self.history().await?;
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
    async fn resume_inner(&self, id: &str) -> Result<AccountView, String> {
        let mut saved = self.load_account(id)?;
        if saved.funding_cancellation_requested
            && saved
                .funding_cancellation
                .as_ref()
                .map(|result| result.status.as_str())
                != Some("original_accepted")
        {
            return Err("funding recovery was requested; use explicit cancel-funding to reconcile/resubmit its replacement, not resume the original".into());
        }
        if saved.topup.is_some() {
            return self.resume_topup(&mut saved).await;
        }
        if saved.quote.payload.funding_wei != "0" {
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
            Some(pending) if pending.amount_wei != amount_wei => {
                return Err("a different topup is pending; resume its original amount first".into())
            }
            Some(_) => {}
            None => {
                saved.topup = Some(TopupIntent {
                    id: uuid::Uuid::new_v4().to_string(),
                    amount_wei: amount_wei.into(),
                    raw_transaction: None,
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
        saved.topup = None;
        self.save_account(saved)?;
        Ok(account)
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
                        if measured.evidence_sha256 != profile.qualification.evidence_sha256
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
        let valid_default = saved.active_credential.as_ref().is_some_and(|active| {
            saved
                .credentials
                .iter()
                .any(|c| &c.id == active && c.expires_at > now)
                && views.iter().any(|c| {
                    &c.id == active
                        && c.contract_id == id
                        && !c.revoked
                        && c.expires_at > now
                        && c.scopes.iter().any(|scope| scope == "infer")
                })
        });
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
    pub fn credential_export(
        &self,
        id: &str,
        credential_id: Option<&str>,
    ) -> Result<Value, String> {
        let saved = self.load_account(id)?;
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
        decode(&response_bytes(response).await?)
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
        usage_receipt: std::sync::Mutex<Option<Signed<UsageReceipt>>>,
        started_at: u64,
        canonical_payment_hash: std::sync::Mutex<String>,
        nonce: std::sync::atomic::AtomicU64,
        credentials: std::sync::Mutex<Vec<(Credential, crate::billing_store::CredentialView)>>,
        credited_topups: std::sync::Mutex<std::collections::HashMap<String, u128>>,
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
            "eth_getBalance" => json!(format!("0x{:x}", u128::MAX)),
            "eth_gasPrice" => json!("0x1"),
            "eth_getTransactionReceipt" => {
                let receipt = state.receipt.lock().unwrap();
                if receipt["transactionHash"] == request["params"][0] {
                    receipt.clone()
                } else {
                    Value::Null
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
            usage_receipt: std::sync::Mutex::new(None),
            started_at: protocol::now().unwrap(),
            canonical_payment_hash: std::sync::Mutex::new(format!("0x{}", "77".repeat(32))),
            nonce: std::sync::atomic::AtomicU64::new(0),
            credentials: std::sync::Mutex::new(Vec::new()),
            credited_topups: std::sync::Mutex::new(std::collections::HashMap::new()),
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
    fn settle_fixture(state: &FundingFixture, raw: &str, success: bool) {
        let bytes = hex::decode(&raw[2..]).unwrap();
        let transaction = rlp::Rlp::new(&bytes);
        *state.receipt.lock().unwrap() = json!({"transactionHash":protocol::hash_hex(&bytes),
            "from":state.quote.consumer_wallet,"to":format!("0x{}",hex::encode(transaction.val_at::<Vec<u8>>(3).unwrap())),
            "type":"0x0","gasUsed":format!("0x{:x}",transaction.val_at::<u64>(2).unwrap()),"status":if success{"0x1"}else{"0x0"},
            "blockHash":state.canonical_payment_hash.lock().unwrap().clone(),"blockNumber":"0x9"});
        state.nonce.store(
            transaction.val_at::<u64>(0).unwrap() + 1,
            std::sync::atomic::Ordering::SeqCst,
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
        assert!(consumer
            .resume(&id)
            .await
            .unwrap_err()
            .contains("explicit cancel-funding"));
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
}
