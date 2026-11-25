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
                // Qualification evidence binds a managed launch's verified runtime, artifacts
                // and devices; an operator-owned external engine is for calibration only
                // (book lines 73, 609 and 615-617).
                if config.runtime.is_none() && !self.development {
                    return Err("a qualified serving profile requires a Rust-managed runtime; an external engine is for calibration only".into());
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

    /// Every engine key, managed or external, is a separate credential (book line 474;
    /// `CLAUDE.md`: llama.cpp runs "with a protected API-key file and no wallet key"). The daemon
    /// hands a managed engine its key file and presents the key as the bearer of every private
    /// engine request, so a key that repeats the signing key in any spelling (`0x`, letter case,
    /// leading zeros), the administration token or the RPC bearer would give that credential to
    /// the engine. Reads the protected files; call it before any engine starts. Errors name the
    /// rule, never a value.
    pub fn verify_separate_engine_keys(&self) -> Result<(), String> {
        use subtle::ConstantTimeEq;
        use zeroize::Zeroizing;
        let engines: Vec<_> = self
            .models
            .iter()
            .filter_map(|m| m.engine_api_key_file.as_deref().map(|path| (&m.model.id, path)))
            .collect();
        if engines.is_empty() {
            return Ok(());
        }
        // Lower-case hexadecimal without `0x`.
        let signing = Zeroizing::new(crate::wallet::load_private_key(&self.signing_key_file)?);
        let admin = Zeroizing::new(read_secret(&self.admin_token_file)?);
        let bearer = match &self.network.rpc_bearer_file {
            Some(path) => Some((path.as_path(), read_rpc_bearer(path)?)),
            None => None,
        };
        let equal = |a: &str, b: &str| bool::from(a.as_bytes().ct_eq(b.as_bytes()));
        for (model, path) in engines {
            let key = Zeroizing::new(
                read_secret(path)
                    .map_err(|e| format!("engine API key file of model {model}: {e}"))?,
            );
            let key: &str = &key;
            let unprefixed = key
                .strip_prefix("0x")
                .or_else(|| key.strip_prefix("0X"))
                .unwrap_or(key);
            // As 64 lower-case digits: leading zeros added or removed spell the same 256-bit key.
            let digits = unprefixed.trim_start_matches('0');
            let mut spelling = Zeroizing::new(String::with_capacity(digits.len().max(64)));
            spelling.extend(std::iter::repeat_n('0', 64usize.saturating_sub(digits.len())));
            spelling.push_str(digits);
            spelling.make_ascii_lowercase();
            let repeated = if same_file(path, &self.signing_key_file) | equal(&spelling, &signing)
            {
                Some("the provider signing key")
            } else if same_file(path, &self.admin_token_file) | equal(key, &admin) {
                Some("the administration token")
            } else if bearer
                .as_ref()
                .is_some_and(|(file, bearer)| same_file(path, file) | equal(key, bearer))
            {
                Some("the RPC bearer")
            } else {
                None
            };
            if let Some(credential) = repeated {
                return Err(format!(
                    "engine_api_key_file of model {model} must hold a separate credential, not {credential}; issue a distinct engine token (book line 474)"
                ));
            }
        }
        Ok(())
    }
}

/// Two configured paths that name one file, compared without reading either.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// The RPC bearer as `rpc_client` sends it: a protected file's trimmed token.
fn read_rpc_bearer(path: &Path) -> Result<zeroize::Zeroizing<String>, String> {
    let unreadable =
        || "cannot read the protected RPC bearer file to check that engine keys differ from it";
    let mut bearer = zeroize::Zeroizing::new(String::new());
    crate::wallet::protected_file(path)
        .map_err(|_| unreadable())?
        .take(4097)
        .read_to_string(&mut bearer)
        .map_err(|_| unreadable())?;
    let token = bearer.trim();
    if token.is_empty() || token.len() > 4096 || token.bytes().any(|b| b.is_ascii_control()) {
        return Err(unreadable().into());
    }
    Ok(zeroize::Zeroizing::new(token.to_owned()))
}

/// Configuration paths name places next to their configuration file, not in the process working
/// directory (book line 449). Absolute paths are kept unchanged.
pub(crate) fn resolve_path(base: &Path, path: &mut PathBuf) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// A provider configuration that passes validation in either mode: one external
    /// embedding engine on loopback (so that no serving profile is needed outside
    /// development), public `127.0.0.1:9419` and administration `127.0.0.1:9421` listeners,
    /// and the public endpoint `https://provider.example` (`http://127.0.0.1:9419` in
    /// development). Its files are under `directory` and do not exist; validation reads none.
    fn provider(directory: &Path, development: bool) -> ProviderConfig {
        let offer = crate::offer_registry::tests::offer().payload;
        let mut model = offer.model;
        model.capabilities = vec!["embeddings".into()];
        model.max_output_tokens = 0;
        ProviderConfig {
            network: NetworkConfig {
                identity: offer.network,
                rpc_url: "http://127.0.0.1:18545".into(),
                rpc_bearer_file: None,
                faucet_url: None,
                explorer_url: None,
                policy_url: None,
            },
            data_dir: directory.join("data"),
            signing_key_file: directory.join("wallet.key"),
            admin_token_file: directory.join("admin.token"),
            public_endpoint: if development { "http://127.0.0.1:9419" } else { "https://provider.example" }.into(),
            public_bind: "127.0.0.1:9419".parse().unwrap(),
            admin_bind: "127.0.0.1:9421".parse().unwrap(),
            platform_wallet: offer.platform_wallet,
            fee_bps: 50,
            min_funding_wei: "1000".into(),
            models: vec![ModelConfig {
                model,
                price: PriceSchedule { input_wei_per_1k: "1000".into(), output_wei_per_1k: "0".into() },
                engine_url: "http://127.0.0.1:18080".into(),
                engine_api_key_file: None,
                runtime: None,
                max_concurrency: 1,
                request_timeout_secs: 30,
                qualification_evidence_file: None,
            }],
            dht: DhtConfig {
                listen: "/ip4/127.0.0.1/tcp/0".into(),
                bootstrap: vec![],
                identity_file: directory.join("dht.key"),
            },
            development,
        }
    }

    /// Validation's answer for the configuration `document`, checked to be the same from a
    /// configuration file (`ProviderConfig::load`, which the daemon runs before it binds the
    /// public or administration listener). A refused configuration is also refused, with
    /// the same error, by `ProviderRuntime::start`, before it creates its data directory or
    /// starts its DHT listener, so no runtime exists to advertise anything.
    async fn admission_of(document: &Value) -> Result<(), String> {
        let config: ProviderConfig = serde_json::from_value(document.clone()).unwrap();
        let direct = config.validate();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("provider.json");
        std::fs::write(&path, serde_json::to_vec(document).unwrap()).unwrap();
        assert_eq!(ProviderConfig::load(&path).map(drop), direct, "{document}");
        if let Err(error) = &direct {
            let started = crate::app::ProviderRuntime::start(config.clone()).await;
            assert_eq!(started.err().as_ref(), Some(error), "{document}");
            assert!(!config.data_dir.exists(), "{document}");
        }
        direct
    }
    async fn admission(config: &ProviderConfig) -> Result<(), String> {
        admission_of(&serde_json::to_value(config).unwrap()).await
    }

    /// outbound-http-safeguards requirement 1 scenario "A provider does not advertise a
    /// non-conforming origin" (task 3.1), and in development too, where the remote rule is
    /// unchanged.
    #[tokio::test]
    async fn a_non_conforming_public_endpoint_is_refused_before_anything_starts() {
        for development in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut config = provider(directory.path(), development);
            assert_eq!(admission(&config).await, Ok(()), "{development}");
            config.public_endpoint = "https://provider.example".into();
            assert_eq!(admission(&config).await, Ok(()), "{development}");
            for endpoint in [
                "https://provider.example:8443",
                "https://provider.example/v1",
                "http://provider.example",
                "https://203.0.113.7",
            ] {
                config.public_endpoint = endpoint.into();
                assert_eq!(
                    admission(&config).await,
                    Err(validate_endpoint(endpoint, development).unwrap_err()),
                    "{endpoint}/{development}"
                );
            }
        }
    }

    /// outbound-http-safeguards requirement 2 scenario "A development provider listens on
    /// loopback only" (task 3.2).
    #[tokio::test]
    async fn a_development_provider_listens_on_loopback_only() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = provider(directory.path(), true);
        for bind in ["0.0.0.0:9419", "192.168.1.5:9419", "[::]:9419"] {
            config.public_bind = bind.parse().unwrap();
            assert_eq!(
                admission(&config).await,
                Err("development providers may only listen on loopback".into()),
                "{bind}"
            );
        }
        for bind in ["127.0.0.1:9419", "[::1]:9419"] {
            config.public_bind = bind.parse().unwrap();
            assert_eq!(admission(&config).await, Ok(()), "{bind}");
        }
    }

    /// outbound-http-safeguards requirement 2, task 3.2: `development` defaults to false
    /// (book line 463). A configuration file without the member is validated as a
    /// non-development provider, which refuses the `http://127.0.0.1:9419` endpoint that
    /// the same file with `"development": true` advertises.
    #[tokio::test]
    async fn a_provider_configuration_without_a_development_member_is_not_development() {
        let directory = tempfile::tempdir().unwrap();
        let mut document = serde_json::to_value(provider(directory.path(), true)).unwrap();
        assert_eq!(document["public_endpoint"], "http://127.0.0.1:9419");
        assert_eq!(document.as_object_mut().unwrap().remove("development"), Some(json!(true)));
        let config: ProviderConfig = serde_json::from_value(document.clone()).unwrap();
        assert!(!config.development);
        assert_eq!(
            admission_of(&document).await,
            Err(validate_endpoint("http://127.0.0.1:9419", false).unwrap_err())
        );
        document["development"] = json!(true);
        assert_eq!(admission_of(&document).await, Ok(()));
    }

    /// outbound-http-safeguards requirement 6 scenario "Administration binds a nonzero
    /// loopback port distinct from the public port" (task 3.6), in both modes.
    #[tokio::test]
    async fn administration_binds_a_nonzero_loopback_port_distinct_from_the_public_port() {
        for development in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut config = provider(directory.path(), development);
            assert_eq!(admission(&config).await, Ok(()), "{development}");
            for admin in ["0.0.0.0:9421", "192.168.1.5:9421", "127.0.0.1:0", "[::]:9421", "[::1]:0"] {
                config.admin_bind = admin.parse().unwrap();
                assert_eq!(
                    admission(&config).await,
                    Err("administration must bind an explicit nonzero loopback port".into()),
                    "{admin}/{development}"
                );
            }
            config.admin_bind = "[::1]:9421".parse().unwrap();
            assert_eq!(admission(&config).await, Ok(()), "{development}");
            config.admin_bind = "127.0.0.1:9421".parse().unwrap();
            for public in ["0.0.0.0:9421", "127.0.0.1:9421", "[::1]:9421", "127.0.0.1:0"] {
                config.public_bind = public.parse().unwrap();
                assert_eq!(
                    admission(&config).await,
                    Err("public and administration listeners require distinct nonzero ports".into()),
                    "{public}/{development}"
                );
            }
            config.public_bind = "127.0.0.1:9419".parse().unwrap();
            assert_eq!(admission(&config).await, Ok(()), "{development}");
        }
    }

    /// outbound-http-safeguards requirement 7 scenarios "Private literal addresses and exact
    /// localhost are accepted" and "DNS names, public addresses and non-origins are refused"
    /// (task 3.7), for an external embedding engine in both modes (so that no other rule
    /// decides): `validate`, `ProviderConfig::load` and `ProviderRuntime::start` (nothing
    /// created) accept the five origins and refuse the ten with the engine rule's error, and
    /// `Backend::new`, which builds the engine client on paths that run no configuration
    /// validation, refuses the same ten (as an engine failure) and builds the five. Loopback
    /// variants of the refused shapes, a recorder's own port behind a path, user information,
    /// another scheme, a root-dot `localhost.` and the mapped `::ffff:127.0.0.1`, are refused
    /// alike, and the recorder accepts no connection. No accepted engine is contacted.
    /// Beyond the scenarios, the requirement's own ranges and origin shape: the edges of
    /// `172.16.0.0/12` and `192.168.0.0/16`, both halves of `fc00::/7`, HTTPS on a literal
    /// and a trailing `/` are accepted; the neighbours of those ranges, `fe00::1`, the
    /// unspecified addresses, a `localhost` subdomain, a password, a query and a fragment are
    /// refused.
    #[tokio::test]
    async fn an_engine_origin_is_a_private_literal_or_exact_localhost() {
        use crate::inference_backend::Backend;
        let (recorder, connections) = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counter = count.clone();
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    drop(stream);
                }
            });
            (address, count)
        };
        let port = recorder.port();
        let not_private = "engine endpoints require a literal private/loopback IP or localhost";
        let not_origin = "endpoint must be an origin without credentials, path, query or fragment";
        let not_http = "engine endpoint must use HTTP or HTTPS";
        let refused: Vec<(String, &str)> = [
            ("http://engine.internal:8080", not_private),
            ("http://localhost.:8000", not_private),
            ("http://8.8.8.8:8000", not_private),
            ("http://169.254.169.254", not_private),
            ("http://100.64.0.1:8000", not_private),
            ("http://[fe80::1]:8000", not_private),
            ("http://[::ffff:10.0.0.1]:8000", not_private),
            ("http://127.0.0.1:8000/v1", not_origin),
            ("http://user@127.0.0.1:8000", not_origin),
            ("ftp://127.0.0.1", not_http),
        ]
        .into_iter()
        .map(|(origin, error)| (origin.to_string(), error))
        .chain([
            (format!("http://127.0.0.1:{port}/v1"), not_origin),
            (format!("http://user@127.0.0.1:{port}"), not_origin),
            (format!("ftp://127.0.0.1:{port}"), not_http),
            (format!("http://localhost.:{port}"), not_private),
            (format!("http://[::ffff:127.0.0.1]:{port}"), not_private),
        ])
        .chain(
            [
                ("http://172.15.255.255:8000", not_private),
                ("http://172.32.0.1:8000", not_private),
                ("http://192.167.255.255:8000", not_private),
                ("http://192.169.0.1:8000", not_private),
                ("http://11.0.0.1:8000", not_private),
                ("http://126.255.255.255:8000", not_private),
                ("http://0.0.0.0:8000", not_private),
                ("http://[::]:8000", not_private),
                ("http://[fe00::1]:8000", not_private),
                ("http://[fbff::1]:8000", not_private),
                ("http://engine.localhost:8000", not_private),
                ("http://:secret@127.0.0.1:8000", not_origin),
                ("http://127.0.0.1:8000/?model=other", not_origin),
                ("http://127.0.0.1:8000/#health", not_origin),
            ]
            .map(|(origin, error)| (origin.to_string(), error)),
        )
        .collect();
        let accepted = [
            "http://127.0.0.1:8000",
            "http://10.0.0.5:8080",
            "https://[fd00::1]:8443",
            "http://[::1]:8000",
            "http://localhost:8000",
            // The requirement's other ranges and origin shapes.
            "http://127.255.255.254:8000",
            "http://10.255.255.255:8000",
            "http://172.16.0.1:8000",
            "http://172.31.255.254:8000",
            "http://192.168.0.1:8000",
            "http://192.168.255.254:8000",
            "http://[fc00::1]:8000",
            "http://[fdff:ffff::1]:8000",
            "https://127.0.0.1:8443",
            "http://localhost:8000/",
        ];
        for development in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut config = provider(directory.path(), development);
            for origin in accepted {
                config.models[0].engine_url = origin.into();
                assert_eq!(admission(&config).await, Ok(()), "{origin}/{development}");
                assert!(Backend::new(&config.models[0]).is_ok(), "{origin}");
            }
            for (origin, error) in &refused {
                config.models[0].engine_url = origin.clone();
                assert_eq!(admission(&config).await, Err(error.to_string()), "{origin}/{development}");
                let backend = Backend::new(&config.models[0]).err().unwrap_or_else(|| panic!("{origin}: engine client built"));
                assert_eq!(
                    (backend.status, backend.code.as_str()),
                    (axum::http::StatusCode::BAD_GATEWAY, "engine_error"),
                    "{origin}"
                );
            }
        }
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// A CPU runtime that passes validation; its two directories are the only path fields.
    fn cpu_runtime(runtime_dir: PathBuf, artifact_dir: PathBuf) -> crate::engine_runtime::EngineRuntimeConfig {
        crate::engine_runtime::EngineRuntimeConfig {
            strata_catalog: None,            runtime_dir,
            artifact_dir,
            backend: "cpu".into(),
            device_ids: vec![],
            memory_reserve_mib: 0,
            gpu_layers: 0,
            split_mode: "none".into(),
            tensor_split: vec![],
            cache_type_k: "f16".into(),
            cache_type_v: "f16".into(),
            flash_attention: false,
            batch_size: 512,
            ubatch_size: 512,
            startup_timeout_secs: 60,
        }
    }

    /// Every path field of a provider configuration, read back from a loaded configuration in
    /// the order `paths_of` lists them.
    fn paths_of(config: &ProviderConfig) -> Vec<(String, Option<PathBuf>)> {
        let mut paths = vec![
            ("data_dir".to_string(), Some(config.data_dir.clone())),
            ("signing_key_file".into(), Some(config.signing_key_file.clone())),
            ("admin_token_file".into(), Some(config.admin_token_file.clone())),
            ("dht.identity_file".into(), Some(config.dht.identity_file.clone())),
            ("network.rpc_bearer_file".into(), config.network.rpc_bearer_file.clone()),
        ];
        for (index, model) in config.models.iter().enumerate() {
            let runtime = model.runtime.as_ref();
            paths.extend([
                (format!("models[{index}].engine_api_key_file"), model.engine_api_key_file.clone()),
                (format!("models[{index}].qualification_evidence_file"), model.qualification_evidence_file.clone()),
                (format!("models[{index}].runtime.runtime_dir"), runtime.map(|r| r.runtime_dir.clone())),
                (format!("models[{index}].runtime.artifact_dir"), runtime.map(|r| r.artifact_dir.clone())),
            ]);
        }
        paths
    }

    /// A development provider whose every path field is `path(name)`: two managed CPU embedding
    /// models, so that each model's four path fields are present twice under distinct names.
    fn every_path(directory: &Path, path: impl Fn(&str) -> PathBuf) -> ProviderConfig {
        let mut config = provider(directory, true);
        config.data_dir = path("state/data");
        config.signing_key_file = path("keys/signing.key");
        config.admin_token_file = path("keys/admin.token");
        config.dht.identity_file = path("keys/dht.key");
        config.network.rpc_bearer_file = Some(path("secrets/rpc.token"));
        let template = config.models.remove(0);
        for (index, name) in ["first", "second"].into_iter().enumerate() {
            let mut model = template.clone();
            model.model.id = format!("{}-{name}", template.model.id);
            model.engine_url = format!("http://127.0.0.1:{}", 18080 + index);
            model.engine_api_key_file = Some(path(&format!("secrets/{name}-engine.key")));
            model.qualification_evidence_file = Some(path(&format!("evidence/{name}.json")));
            model.runtime = Some(cpu_runtime(
                path(&format!("runtime/{name}")),
                path(&format!("../shared/artifacts/{name}")),
            ));
            config.models.push(model);
        }
        config
    }

    /// profile-path-resolution requirement 1, provider configuration (task 3.1): `load`
    /// resolves each of the provider's nine path-valued fields (`data_dir`, `signing_key_file`,
    /// `admin_token_file`, `dht.identity_file`, `network.rpc_bearer_file`, and for every model
    /// `engine_api_key_file`, `qualification_evidence_file`, `runtime.runtime_dir` and
    /// `runtime.artifact_dir`) against the configuration file's directory, lexically (a `..`
    /// component is kept, none of the files exists, and nothing is created), keeps absolute
    /// paths unchanged, and leaves absent optional paths absent.
    #[test]
    fn every_relative_provider_path_resolves_against_the_configuration_directory() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let base = root.join("operator").join("provider");
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("provider.json");

        let relative = every_path(root, |name| PathBuf::from(name));
        let expected: Vec<_> = paths_of(&relative)
            .into_iter()
            .map(|(field, path)| (field, path.map(|path| base.join(path))))
            .collect();
        assert_eq!(expected.len(), 13);
        assert!(expected.iter().all(|(_, path)| path.is_some()));
        std::fs::write(&file, serde_json::to_vec(&relative).unwrap()).unwrap();
        let loaded = ProviderConfig::load(&file).unwrap();
        assert_eq!(paths_of(&loaded), expected);
        // Lexical: the `..` component is not collapsed and nothing is created or required.
        assert_eq!(
            loaded.models[1].runtime.as_ref().unwrap().artifact_dir,
            base.join("..").join("shared").join("artifacts").join("second")
        );
        let names = |path: &Path| -> Vec<_> {
            std::fs::read_dir(path).unwrap().map(|entry| entry.unwrap().file_name()).collect()
        };
        assert_eq!(names(&base), ["provider.json"]);
        assert_eq!(names(root), ["operator"]);

        // Absolute paths are used unchanged, wherever the configuration file is.
        let elsewhere = root.join("elsewhere");
        let absolute = every_path(root, |name| elsewhere.join(name));
        std::fs::write(&file, serde_json::to_vec(&absolute).unwrap()).unwrap();
        assert_eq!(paths_of(&ProviderConfig::load(&file).unwrap()), paths_of(&absolute));
        assert!(!elsewhere.exists());

        // Absent optional paths stay absent; the present ones still resolve.
        let mut optional = provider(root, true);
        optional.data_dir = "data".into();
        std::fs::write(&file, serde_json::to_vec(&optional).unwrap()).unwrap();
        let loaded = ProviderConfig::load(&file).unwrap();
        assert_eq!(loaded.data_dir, base.join("data"));
        assert_eq!(loaded.network.rpc_bearer_file, None);
        assert_eq!(loaded.models[0].engine_api_key_file, None);
        assert_eq!(loaded.models[0].qualification_evidence_file, None);
        assert!(loaded.models[0].runtime.is_none());
        assert_eq!(names(&base), ["provider.json"]);
    }
}
