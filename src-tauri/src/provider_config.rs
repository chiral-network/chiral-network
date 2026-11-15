//! Explicit operator configuration; no embedded legacy-chain or public-provider defaults.
use crate::network::NetworkConfig;
use crate::offer_registry::{validate_endpoint, validate_model, validate_money, validate_wallet};
use crate::protocol::{self, ModelSpec, PriceSchedule};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub model: ModelSpec,
    pub price: PriceSchedule,
    pub engine_url: String,
    pub engine_api_key_file: Option<PathBuf>,
    #[serde(default)]
    pub runtime: Option<crate::engine_runtime::EngineRuntimeConfig>,
    pub max_concurrency: u32,
    pub request_timeout_secs: u64,
    pub qualification_evidence_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DhtConfig {
    pub listen: String,
    pub bootstrap: Vec<String>,
    pub identity_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub network: NetworkConfig,
    pub data_dir: PathBuf,
    pub signing_key_file: PathBuf,
    pub admin_token_file: PathBuf,
    pub public_endpoint: String,
    pub public_bind: SocketAddr,
    pub admin_bind: SocketAddr,
    pub platform_wallet: String,
    pub fee_bps: u16,
    pub min_funding_wei: String,
    pub models: Vec<ModelConfig>,
    pub dht: DhtConfig,
    #[serde(default)]
    pub development: bool,
}

impl ProviderConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|e| format!("cannot read provider configuration: {e}"))?;
        if metadata.len() > 1024 * 1024 {
            return Err("provider configuration exceeds 1 MiB".into());
        }
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read provider configuration: {e}"))?;
        let mut config: Self = serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid provider configuration: {e}"))?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        for value in [
            &mut config.data_dir,
            &mut config.signing_key_file,
            &mut config.admin_token_file,
            &mut config.dht.identity_file,
        ] {
            resolve_path(base, value);
        }
        if let Some(value) = config.network.rpc_bearer_file.as_mut() {
            resolve_path(base, value);
        }
        for model in &mut config.models {
            if let Some(value) = model.engine_api_key_file.as_mut() {
                resolve_path(base, value);
            }
            if let Some(value) = model.qualification_evidence_file.as_mut() {
                resolve_path(base, value);
            }
            if let Some(runtime) = model.runtime.as_mut() {
                resolve_path(base, &mut runtime.runtime_dir);
                resolve_path(base, &mut runtime.artifact_dir);
            }
        }
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.admin_bind.ip().is_loopback() || self.admin_bind.port() == 0 {
            return Err("administration must bind an explicit nonzero loopback port".into());
        }
        if self.public_bind.port() == 0 || self.public_bind.port() == self.admin_bind.port() {
            return Err(
                "public and administration listeners require distinct nonzero ports".into(),
            );
        }
        if self.development && !self.public_bind.ip().is_loopback() {
            return Err("development providers may only listen on loopback".into());
        }
        validate_endpoint(&self.public_endpoint, self.development)?;
        validate_wallet(&self.platform_wallet)?;
        if self.fee_bps != 50 {
            return Err("this protocol release uses the agreed 50 basis-point platform fee".into());
        }
        validate_money(&self.min_funding_wei)?;
        if self.models.is_empty() || self.models.len() > 64 {
            return Err("configure between 1 and 64 model deployments".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut devices = std::collections::HashSet::new();
        if self.models.iter().any(|m| m.model.serving.is_some())
            && self.models.iter().any(|m| m.model.serving.is_none()) {
            return Err("qualified GPU deployments cannot share a provider with unprofiled hardware; use an isolated provider process".into());
        }
        for config in &self.models {
            validate_model(&config.model)?;
            if let Some(profile) = &config.model.serving {
                if profile.devices.iter().any(|device| !devices.insert(&device.id)) {
                    return Err("simultaneous qualified models require disjoint physical device inventories".into());
                }
                if profile.max_concurrency != config.max_concurrency
                    || config.qualification_evidence_file.is_none() {
                    return Err("serving profile requires matching concurrency and qualification evidence".into());
                }
            } else if !self.development
                && config.model.capabilities.iter().any(|c| matches!(c.as_str(), "chat" | "completions")) {
                return Err("public generation requires a qualified serving profile".into());
            }
            if !ids.insert(&config.model.id) {
                return Err("model API identifiers must be unique within a provider".into());
            }
            validate_money(&config.price.input_wei_per_1k)?;
            let output_price = validate_money(&config.price.output_wei_per_1k)?;
            if output_price != 0
                && config.model.capabilities.iter().any(|capability| capability == "embeddings")
            {
                return Err("embedding deployments require zero output price".into());
            }
            if config.max_concurrency == 0 || config.max_concurrency > 256 {
                return Err("model concurrency must be between 1 and 256".into());
            }
            if !(1..=3600).contains(&config.request_timeout_secs) {
                return Err("model request timeout must be between 1 and 3600 seconds".into());
            }
            let engine = parse_origin(&config.engine_url)?;
            if !matches!(engine.scheme(), "http" | "https") {
                return Err("engine endpoint must use HTTP or HTTPS".into());
            }
            let private = match engine.host() {
                Some(url::Host::Ipv4(ip)) => is_private_engine_ip(ip.into()),
                Some(url::Host::Ipv6(ip)) => is_private_engine_ip(ip.into()),
                Some(url::Host::Domain("localhost")) => true,
                _ => false,
            };
            if !private {
                return Err(
                    "engine endpoints require a literal private/loopback IP or localhost".into(),
                );
            }
            if let Some(runtime) = &config.runtime {
                runtime.validate(config)?;
            }
        }
        if self.dht.bootstrap.len() > 32 || self.dht.listen.len() > 512 {
            return Err("DHT configuration exceeds bounds".into());
        }
        self.dht
            .listen
            .parse::<libp2p::Multiaddr>()
            .map_err(|_| "invalid DHT listen multiaddress")?;
        for address in &self.dht.bootstrap {
            address
                .parse::<libp2p::Multiaddr>()
                .map_err(|_| "invalid DHT bootstrap multiaddress")?;
        }
        self.network.validate()?;
        Ok(())
    }

    pub fn runtime_data_dir(&self) -> Result<PathBuf, String> {
        let identity = protocol::canonical_json(&self.network.identity)?;
        Ok(self.data_dir.join(hex::encode(protocol::digest(&identity))))
    }
}

fn resolve_path(base: &Path, path: &mut PathBuf) {
    if path.is_relative() {
        *path = base.join(&*path);
    }
}

pub fn parse_origin(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|_| "invalid endpoint URL")?;
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(
            "endpoint must be an origin without credentials, path, query or fragment".into(),
        );
    }
    Ok(url)
}

pub fn is_private_engine_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
        IpAddr::V6(ip) => ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// Secrets are read only from regular, owner-private files; their content is never logged.
pub fn read_secret(path: &Path) -> Result<String, String> {
    let file = crate::wallet::open_secret_file(path)?;
    let size = file
        .metadata()
        .map_err(|_| "cannot inspect configured secret")?
        .len();
    if size == 0 || size > 4096 {
        return Err("secret must be a nonempty regular file of at most 4096 bytes".into());
    }
    let mut secret = String::new();
    file.take(4097)
        .read_to_string(&mut secret)
        .map_err(|_| "cannot read configured secret file")?;
    if secret.len() > 4096 {
        return Err("secret exceeds size limit".into());
    }
    let secret = secret.trim();
    if secret.is_empty()
        || secret
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return Err("secret file contains an invalid token".into());
    }
    Ok(secret.to_owned())
}
