//! Network-scoped inference wire types. Money is always a decimal base-unit string.
use serde::{
    de::{value::MapAccessDeserializer, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::Value;
use tiny_keccak::{Hasher, Keccak};
use std::{fmt, marker::PhantomData};

// Require an object while retaining the derived field decoder's duplicate,
// unknown and missing-field checks. Do not buffer through serde_json::Value.
fn deserialize_object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct ObjectVisitor<T>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a JSON object")
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_map(ObjectVisitor(PhantomData))
}

pub const OFFER_DOMAIN: &str = "chiral-inference-offer-v1";
pub const QUOTE_DOMAIN: &str = "chiral-inference-quote-v1";
pub const INTENT_DOMAIN: &str = "chiral-inference-funding-intent-v1";
pub const RECEIPT_DOMAIN: &str = "chiral-inference-receipt-v1";
pub const AUTH_DOMAIN: &str = "chiral-inference-auth-v1";
pub const FEEDBACK_DOMAIN: &str = "chiral-inference-feedback-v1";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NetworkIdentity {
    pub chain_id: u64,
    pub genesis_hash: String,
    pub deployment_id: String,
}

impl<'de> Deserialize<'de> for NetworkIdentity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            chain_id: u64,
            genesis_hash: String,
            deployment_id: String,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            chain_id: fields.chain_id,
            genesis_hash: fields.genesis_hash,
            deployment_id: fields.deployment_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PriceSchedule {
    pub input_wei_per_1k: String,
    pub output_wei_per_1k: String,
}

impl<'de> Deserialize<'de> for PriceSchedule {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            input_wei_per_1k: String,
            output_wei_per_1k: String,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            input_wei_per_1k: fields.input_wei_per_1k,
            output_wei_per_1k: fields.output_wei_per_1k,
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelSpec {
    pub id: String,
    pub revision: String,
    pub tokenizer_revision: String,
    pub chat_template_sha256: String,
    pub quantization: Option<String>,
    pub context_length: u64,
    pub max_output_tokens: u64,
    /// Certified capabilities: chat, completions, embeddings, tools, structured_output.
    pub capabilities: Vec<String>,
    /// None makes no speed/capacity claim; production generation requires a qualified profile.
    pub serving: Option<ServingProfile>,
}

impl<'de> Deserialize<'de> for ModelSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            id: String,
            revision: String,
            tokenizer_revision: String,
            chat_template_sha256: String,
            quantization: Option<String>,
            context_length: u64,
            max_output_tokens: u64,
            capabilities: Vec<String>,
            serving: Option<ServingProfile>,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            id: fields.id,
            revision: fields.revision,
            tokenizer_revision: fields.tokenizer_revision,
            chat_template_sha256: fields.chat_template_sha256,
            quantization: fields.quantization,
            context_length: fields.context_length,
            max_output_tokens: fields.max_output_tokens,
            capabilities: fields.capabilities,
            serving: fields.serving,
        })
    }
}

impl ModelSpec {
    /// Agreements keep every original term. Only a strictly newer measurement
    /// of the identical deployment may replace their qualification evidence.
    pub fn accepts_execution(&self, current: &Self) -> bool {
        self.id == current.id
            && self.revision == current.revision
            && self.tokenizer_revision == current.tokenizer_revision
            && self.chat_template_sha256 == current.chat_template_sha256
            && self.quantization == current.quantization
            && self.context_length == current.context_length
            && self.max_output_tokens == current.max_output_tokens
            && self.capabilities == current.capabilities
            && match (&self.serving, &current.serving) {
                (None, None) => true,
                (Some(original), Some(next)) => original.same_terms(next)
                    && (original.qualification == next.qualification
                        || next.qualification.measured_at > original.qualification.measured_at),
                _ => false,
            }
    }
}

/// Physical device identity and its explicit llama.cpp selector.
/// Unified system memory is represented by one device, never summed per GPU.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EngineDevice {
    pub id: String,
    pub name: String,
    pub memory_mib: u64,
    pub driver_version: String,
    pub engine_device: String,
}

impl<'de> Deserialize<'de> for EngineDevice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            id: String,
            name: String,
            memory_mib: u64,
            driver_version: String,
            engine_device: String,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            id: fields.id,
            name: fields.name,
            memory_mib: fields.memory_mib,
            driver_version: fields.driver_version,
            engine_device: fields.engine_device,
        })
    }
}

/// Provider-declared deployment identity, not independent execution attestation.
/// The entire object travels inside signed offers, immutable quotes and receipts.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServingProfile {
    pub hf_repo: String,
    pub tokenizer_repo: String,
    pub artifact_manifest_sha256: String,
    pub engine: String,
    pub engine_version: String,
    pub runtime_sha256: String,
    pub platform: String,
    pub backend: String,
    pub memory_kind: String,
    pub devices: Vec<EngineDevice>,
    pub memory_reserve_mib: u64,
    pub gpu_layers: u32,
    pub split_mode: String,
    pub tensor_split: Vec<u32>,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub flash_attention: bool,
    pub batch_size: u32,
    pub ubatch_size: u32,
    pub capacity_tier: String,
    pub region: String,
    pub max_concurrency: u32,
    pub arrival_milli_rps: u32,
    pub output_tokens_per_second: u32,
    /// Fixed policy: warm/non-repeated, <=2048 prompt, <=256 budget, <=4096 context;
    /// gateway-observed output times, 2s TTFT, 95% joint attainment; inference waiver.
    pub policy: String,
    pub qualification: QualificationIdentity,
}

impl<'de> Deserialize<'de> for ServingProfile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            hf_repo: String,
            tokenizer_repo: String,
            artifact_manifest_sha256: String,
            engine: String,
            engine_version: String,
            runtime_sha256: String,
            platform: String,
            backend: String,
            memory_kind: String,
            devices: Vec<EngineDevice>,
            memory_reserve_mib: u64,
            gpu_layers: u32,
            split_mode: String,
            tensor_split: Vec<u32>,
            cache_type_k: String,
            cache_type_v: String,
            flash_attention: bool,
            batch_size: u32,
            ubatch_size: u32,
            capacity_tier: String,
            region: String,
            max_concurrency: u32,
            arrival_milli_rps: u32,
            output_tokens_per_second: u32,
            policy: String,
            qualification: QualificationIdentity,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            hf_repo: fields.hf_repo,
            tokenizer_repo: fields.tokenizer_repo,
            artifact_manifest_sha256: fields.artifact_manifest_sha256,
            engine: fields.engine,
            engine_version: fields.engine_version,
            runtime_sha256: fields.runtime_sha256,
            platform: fields.platform,
            backend: fields.backend,
            memory_kind: fields.memory_kind,
            devices: fields.devices,
            memory_reserve_mib: fields.memory_reserve_mib,
            gpu_layers: fields.gpu_layers,
            split_mode: fields.split_mode,
            tensor_split: fields.tensor_split,
            cache_type_k: fields.cache_type_k,
            cache_type_v: fields.cache_type_v,
            flash_attention: fields.flash_attention,
            batch_size: fields.batch_size,
            ubatch_size: fields.ubatch_size,
            capacity_tier: fields.capacity_tier,
            region: fields.region,
            max_concurrency: fields.max_concurrency,
            arrival_milli_rps: fields.arrival_milli_rps,
            output_tokens_per_second: fields.output_tokens_per_second,
            policy: fields.policy,
            qualification: fields.qualification,
        })
    }
}

impl ServingProfile {
    fn same_terms(&self, other: &Self) -> bool {
        self.hf_repo == other.hf_repo
            && self.tokenizer_repo == other.tokenizer_repo
            && self.artifact_manifest_sha256 == other.artifact_manifest_sha256
            && self.engine == other.engine
            && self.engine_version == other.engine_version
            && self.runtime_sha256 == other.runtime_sha256
            && self.platform == other.platform
            && self.backend == other.backend
            && self.memory_kind == other.memory_kind
            && self.devices == other.devices
            && self.memory_reserve_mib == other.memory_reserve_mib
            && self.gpu_layers == other.gpu_layers
            && self.split_mode == other.split_mode
            && self.tensor_split == other.tensor_split
            && self.cache_type_k == other.cache_type_k
            && self.cache_type_v == other.cache_type_v
            && self.flash_attention == other.flash_attention
            && self.batch_size == other.batch_size
            && self.ubatch_size == other.ubatch_size
            && self.capacity_tier == other.capacity_tier
            && self.region == other.region
            && self.max_concurrency == other.max_concurrency
            && self.arrival_milli_rps == other.arrival_milli_rps
            && self.output_tokens_per_second == other.output_tokens_per_second
            && self.policy == other.policy
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QualificationIdentity {
    pub evidence_sha256: String,
    pub measured_at: u64,
}

impl<'de> Deserialize<'de> for QualificationIdentity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            evidence_sha256: String,
            measured_at: u64,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            evidence_sha256: fields.evidence_sha256,
            measured_at: fields.measured_at,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PerformanceOutcome {
    pub evidence_sha256: String,
    pub policy: String,
    pub eligibility: String,
    pub ttft_us: Option<u64>,
    pub decode_us: Option<u64>,
    pub elapsed_us: Option<u64>,
    pub output_tokens: Option<u64>,
    pub attained: Option<bool>,
    pub remedy: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InferenceOffer {
    pub network: NetworkIdentity,
    pub provider_wallet: String,
    pub peer_id: String,
    pub deployment_id: String,
    pub endpoint: String,
    pub model: ModelSpec,
    pub price: PriceSchedule,
    pub min_funding_wei: String,
    pub platform_wallet: String,
    pub fee_bps: u16,
    pub max_concurrency: u32,
    pub revision: u64,
    pub issued_at: u64,
    pub expires_at: u64,
}

impl<'de> Deserialize<'de> for InferenceOffer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            network: NetworkIdentity,
            provider_wallet: String,
            peer_id: String,
            deployment_id: String,
            endpoint: String,
            model: ModelSpec,
            price: PriceSchedule,
            min_funding_wei: String,
            platform_wallet: String,
            fee_bps: u16,
            max_concurrency: u32,
            revision: u64,
            issued_at: u64,
            expires_at: u64,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            network: fields.network,
            provider_wallet: fields.provider_wallet,
            peer_id: fields.peer_id,
            deployment_id: fields.deployment_id,
            endpoint: fields.endpoint,
            model: fields.model,
            price: fields.price,
            min_funding_wei: fields.min_funding_wei,
            platform_wallet: fields.platform_wallet,
            fee_bps: fields.fee_bps,
            max_concurrency: fields.max_concurrency,
            revision: fields.revision,
            issued_at: fields.issued_at,
            expires_at: fields.expires_at,
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Quote {
    /// Random 32-byte hex identifier, also the account/contract identifier.
    pub id: String,
    pub network: NetworkIdentity,
    pub offer_id: String,
    pub provider_wallet: String,
    pub consumer_wallet: String,
    pub model: ModelSpec,
    pub price: PriceSchedule,
    pub funding_wei: String,
    pub platform_wallet: String,
    pub fee_bps: u16,
    pub max_concurrency: u32,
    pub nonce: String,
    pub issued_at: u64,
    pub expires_at: u64,
}

impl<'de> Deserialize<'de> for Quote {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            id: String,
            network: NetworkIdentity,
            offer_id: String,
            provider_wallet: String,
            consumer_wallet: String,
            model: ModelSpec,
            price: PriceSchedule,
            funding_wei: String,
            platform_wallet: String,
            fee_bps: u16,
            max_concurrency: u32,
            nonce: String,
            issued_at: u64,
            expires_at: u64,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            id: fields.id,
            network: fields.network,
            offer_id: fields.offer_id,
            provider_wallet: fields.provider_wallet,
            consumer_wallet: fields.consumer_wallet,
            model: fields.model,
            price: fields.price,
            funding_wei: fields.funding_wei,
            platform_wallet: fields.platform_wallet,
            fee_bps: fields.fee_bps,
            max_concurrency: fields.max_concurrency,
            nonce: fields.nonce,
            issued_at: fields.issued_at,
            expires_at: fields.expires_at,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FundingIntentRequest {
    pub raw_transaction: String,
}

impl<'de> Deserialize<'de> for FundingIntentRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            raw_transaction: String,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            raw_transaction: fields.raw_transaction,
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FundingIntent {
    pub network: NetworkIdentity,
    pub provider_wallet: String,
    pub consumer_wallet: String,
    pub quote_id: String,
    pub tx_hash: String,
    pub registered_at: u64,
}

impl<'de> Deserialize<'de> for FundingIntent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            network: NetworkIdentity,
            provider_wallet: String,
            consumer_wallet: String,
            quote_id: String,
            tx_hash: String,
            registered_at: u64,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            network: fields.network,
            provider_wallet: fields.provider_wallet,
            consumer_wallet: fields.consumer_wallet,
            quote_id: fields.quote_id,
            tx_hash: fields.tx_hash,
            registered_at: fields.registered_at,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Signed<T> {
    pub payload: T,
    pub signature: String,
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Signed<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields<T> {
            payload: T,
            signature: String,
        }
        let fields: Fields<T> = deserialize_object(deserializer)?;
        Ok(Self {
            payload: fields.payload,
            signature: fields.signature,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountView {
    pub contract_id: String,
    pub network: NetworkIdentity,
    pub provider_wallet: String,
    pub consumer_wallet: String,
    pub model: ModelSpec,
    pub price: PriceSchedule,
    pub status: String,
    pub funded_wei: String,
    pub fee_wei: String,
    pub spent_wei: String,
    pub reserved_wei: String,
    pub available_wei: String,
    pub reversed_wei: String,
    pub provider_loss_wei: String,
    pub usage: TokenUsage,
    /// Cumulative foregone inference charges; not a balance credit or refundable money.
    pub waived_wei: String,
    pub max_concurrency: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageReceipt {
    pub network: NetworkIdentity,
    pub provider_wallet: String,
    pub account: AccountView,
    /// Most recent bounded request page; each outcome is covered by this signature.
    pub requests: Vec<crate::billing_store::RequestView>,
    pub sequence: u64,
    pub issued_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: String,
    pub bearer: String,
    pub contract_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenResult {
    pub account: AccountView,
    /// Present only on first creation; recovery never reissues a bearer.
    pub credential: Option<Credential>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeRequest {
    pub offer_id: String,
    pub consumer_wallet: String,
    pub funding_wei: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRequest {
    pub quote_id: String,
    /// Absent only for a genuinely zero-priced offer.
    pub raw_transaction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopupRequest {
    pub raw_transaction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeRequest {
    pub wallet: String,
    pub method: String,
    pub path: String,
    pub body_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    pub nonce: String,
    pub payload_hex: String,
    pub expires_at: u64,
}

pub fn now() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| "system clock precedes the Unix epoch".into())
}

pub fn digest(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Keccak::v256();
    let mut out = [0; 32];
    hash.update(bytes);
    hash.finalize(&mut out);
    out
}

pub fn hash_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(digest(bytes)))
}

/// Signed protocol objects have integer numbers only. serde_json::Map's default
/// BTreeMap sorts keys recursively; do not enable its preserve_order feature.
/// This deliberately does not claim RFC-8785 support for arbitrary floats.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    fn check(value: &Value) -> Result<(), String> {
        match value {
            Value::Number(n) if !n.is_u64() && !n.is_i64() => {
                Err("signed protocol objects cannot contain floating-point numbers".into())
            }
            Value::Array(values) => values.iter().try_for_each(check),
            Value::Object(values) => values.values().try_for_each(check),
            _ => Ok(()),
        }
    }
    let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    check(&value)?;
    serde_json::to_vec(&value).map_err(|e| e.to_string())
}

pub fn signing_payload<T: Serialize>(domain: &str, value: &T) -> Result<Vec<u8>, String> {
    let json = canonical_json(value)?;
    let domain_len = u32::try_from(domain.len()).map_err(|_| "domain too long")?;
    let json_len = u32::try_from(json.len()).map_err(|_| "payload too long")?;
    let mut bytes = Vec::with_capacity(8 + domain.len() + json.len());
    bytes.extend_from_slice(&domain_len.to_be_bytes());
    bytes.extend_from_slice(domain.as_bytes());
    bytes.extend_from_slice(&json_len.to_be_bytes());
    bytes.extend_from_slice(&json);
    Ok(bytes)
}

pub fn sign<T: Serialize>(
    domain: &str,
    payload: T,
    private_key: &str,
) -> Result<Signed<T>, String> {
    let signature = crate::wallet::sign_message(private_key, &signing_payload(domain, &payload)?)?;
    Ok(Signed { payload, signature })
}

pub fn verify<T: Serialize>(domain: &str, signed: &Signed<T>, wallet: &str) -> Result<(), String> {
    let recovered = crate::wallet::recover_signer(
        &signing_payload(domain, &signed.payload)?,
        &signed.signature,
    )?;
    if recovered != wallet {
        return Err("signature does not match the canonical wallet address".into());
    }
    Ok(())
}

pub fn offer_id(offer: &InferenceOffer) -> Result<String, String> {
    Ok(hash_hex(&signing_payload(OFFER_DOMAIN, offer)?))
}

pub fn opening_data(quote: &Quote) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(38);
    bytes.extend_from_slice(b"CHRL1\0");
    bytes.extend_from_slice(&digest(&signing_payload(QUOTE_DOMAIN, quote)?));
    Ok(bytes)
}

pub fn topup_data(contract_id: &str) -> Result<Vec<u8>, String> {
    let id = hex::decode(
        contract_id
            .strip_prefix("0x")
            .ok_or("contract id must be 0x-prefixed")?,
    )
    .map_err(|_| "invalid contract id")?;
    if id.len() != 32 {
        return Err("contract id must be 32 bytes".into());
    }
    let mut bytes = Vec::with_capacity(38);
    bytes.extend_from_slice(b"CHRL2\0");
    bytes.extend_from_slice(&id);
    Ok(bytes)
}

pub fn random_id() -> String {
    use rand::RngCore;
    let mut bytes = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!("0x{}", hex::encode(bytes))
}
