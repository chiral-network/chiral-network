//! Verified, network-scoped offers. The stable namespace is wallet/deployment/model;
//! content IDs pin exact signed versions and never confer ownership on first writers.
use crate::protocol::{self, InferenceOffer, ModelSpec, NetworkIdentity, Signed};
use parking_lot::RwLock;
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

pub const MAX_OFFERS: usize = 8192;
pub const MAX_OFFER_BYTES: usize = 16 * 1024;
pub const MAX_OFFER_LIFETIME: u64 = 3600;
struct Entry {
    revision: u64,
    id: String,
    offer: Option<Signed<InferenceOffer>>,
    retain_until: u64,
}
#[derive(Default)]
struct RegistryState {
    entries: BTreeMap<String, Entry>,
    ids: BTreeMap<String, String>,
}
pub struct OfferRegistry {
    network: NetworkIdentity,
    development: bool,
    state: RwLock<RegistryState>,
}

pub fn validate_wallet(wallet: &str) -> Result<(), String> {
    if wallet.len() != 42
        || !wallet.starts_with("0x")
        || !wallet[2..]
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("wallet must be canonical lowercase 20-byte hex".into());
    }
    Ok(())
}
pub fn validate_money(value: &str) -> Result<u128, String> {
    if value.is_empty()
        || value.len() > 39
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("money must be a canonical decimal wei string".into());
    }
    value.parse().map_err(|_| "money exceeds u128 range".into())
}
pub fn validate_model(model: &ModelSpec) -> Result<(), String> {
    for value in [&model.id, &model.revision, &model.tokenizer_revision] {
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err("invalid model identity".into());
        }
    }
    for revision in [&model.revision, &model.tokenizer_revision] {
        if !matches!(revision.len(), 40 | 64) || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(
                "model and tokenizer revisions must be immutable 40/64-hex content hashes".into(),
            );
        }
    }
    if model.chat_template_sha256.len() != 64
        || !model
            .chat_template_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err("chat template requires a SHA256 digest".into());
    }
    if model
        .quantization
        .as_ref()
        .is_some_and(|q| q.is_empty() || q.len() > 64)
    {
        return Err("invalid quantization".into());
    }
    if model.capabilities.is_empty() || model.capabilities.len() > 6 {
        return Err("invalid capabilities".into());
    }
    let mut seen = std::collections::HashSet::new();
    for capability in &model.capabilities {
        if !matches!(
            capability.as_str(),
            "chat" | "completions" | "embeddings" | "tools" | "structured_output" | "streaming"
        ) || !seen.insert(capability)
        {
            return Err("unknown or duplicate capability".into());
        }
    }
    if !model
        .capabilities
        .iter()
        .any(|c| matches!(c.as_str(), "chat" | "completions" | "embeddings"))
    {
        return Err("model must expose an inference endpoint capability".into());
    }
    let generates = model
        .capabilities
        .iter()
        .any(|c| matches!(c.as_str(), "chat" | "completions"));
    if model.context_length == 0
        || model.context_length > 1_048_576
        || model.max_output_tokens > model.context_length
        || (generates && model.max_output_tokens == 0)
        || (!generates && model.max_output_tokens != 0)
    {
        return Err("invalid model token limits".into());
    }
    if let Some(profile) = &model.serving {
        crate::performance::validate_profile(model, profile)?;
    }
    Ok(())
}
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0 && (c == 0 || c == 2))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let s = ip.segments();
            // Only global unicast; exclude documentation, transition and special-use ranges.
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
pub fn validate_endpoint(endpoint: &str, development: bool) -> Result<url::Url, String> {
    if endpoint.len() > 2048 {
        return Err("endpoint too long".into());
    }
    let url = url::Url::parse(endpoint).map_err(|_| "invalid endpoint URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("endpoint must be an HTTP(S) origin without credentials".into());
    }
    let host = url.host_str().ok_or("endpoint has no host")?;
    let literal = host.trim_matches(['[', ']']).parse::<IpAddr>().ok();
    let local =
        literal.is_some_and(|ip| ip.is_loopback()) || host.eq_ignore_ascii_case("localhost");
    if !(development && local) {
        // A DNS root dot does not leave the reserved localhost namespace.
        // Do not use this spelling to broaden the exact development exception.
        let dns_name = host.trim_end_matches('.');
        if url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || literal.is_some_and(|ip| !public_ip(ip))
            || local
            || dns_name.eq_ignore_ascii_case("localhost")
            || dns_name.ends_with(".localhost")
            || (literal.is_none() && !host.contains('.'))
        {
            return Err("endpoint must be public HTTPS on port 443; development only permits loopback exceptions".into());
        }
    }
    Ok(url)
}
/// Resolve once, validate every result, pin addresses and disable both redirects and
/// environment proxies. Call for each origin; do not reuse across unvalidated hosts.
pub async fn safe_http_client(
    endpoint: &str,
    development: bool,
) -> Result<reqwest::Client, String> {
    let url = validate_endpoint(endpoint, development)?;
    let host = url
        .host_str()
        .ok_or("missing host")?
        .trim_matches(['[', ']']);
    let port = url.port_or_known_default().ok_or("missing port")?;
    let loopback_origin = host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
        || host.eq_ignore_ascii_case("localhost");
    let addresses: Vec<SocketAddr> = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| "DNS resolution timeout")?
    .map_err(|e| e.to_string())?
    .take(33)
    .collect();
    if addresses.is_empty()
        || addresses.len() > 32
        || addresses.iter().any(|a| {
            !public_ip(a.ip()) && !(development && loopback_origin && a.ip().is_loopback())
        })
    {
        return Err("endpoint resolves to forbidden addresses".into());
    }
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|e| e.to_string())
}
impl OfferRegistry {
    pub fn new(network: NetworkIdentity, development: bool) -> Self {
        Self {
            network,
            development,
            state: RwLock::new(RegistryState::default()),
        }
    }
    pub fn get(&self, id: &str) -> Option<Signed<InferenceOffer>> {
        let now = protocol::now().ok()?;
        let state = self.state.read();
        let offer = state.entries.get(state.ids.get(id)?)?.offer.as_ref()?;
        (offer.payload.expires_at > now).then(|| offer.clone())
    }
    pub fn list(&self) -> Vec<Signed<InferenceOffer>> {
        let now = protocol::now().unwrap_or(u64::MAX);
        self.state
            .read()
            .entries
            .values()
            .filter_map(|e| e.offer.as_ref())
            .filter(|o| o.payload.expires_at > now)
            .cloned()
            .collect()
    }
    pub fn insert(&self, signed: Signed<InferenceOffer>) -> Result<String, String> {
        self.insert_at(signed, protocol::now()?)
    }
    fn insert_at(&self, signed: Signed<InferenceOffer>, now: u64) -> Result<String, String> {
        if serde_json::to_vec(&signed)
            .map_err(|e| e.to_string())?
            .len()
            > MAX_OFFER_BYTES
        {
            return Err("offer exceeds size limit".into());
        }
        let o = &signed.payload;
        if o.network != self.network {
            return Err("offer belongs to a different network".into());
        }
        validate_wallet(&o.provider_wallet)?;
        validate_wallet(&o.platform_wallet)?;
        protocol::verify(protocol::OFFER_DOMAIN, &signed, &o.provider_wallet)?;
        validate_model(&o.model)?;
        if let Some(profile) = &o.model.serving {
            if profile.max_concurrency != o.max_concurrency
                || profile.qualification.measured_at > now {
                return Err("offer concurrency or current qualification identity is invalid".into());
            }
        }
        validate_endpoint(&o.endpoint, self.development)?;
        validate_money(&o.price.input_wei_per_1k)?;
        validate_money(&o.price.output_wei_per_1k)?;
        validate_money(&o.min_funding_wei)?;
        o.peer_id
            .parse::<libp2p::PeerId>()
            .map_err(|_| "invalid peer ID")?;
        if o.deployment_id.is_empty()
            || o.deployment_id.len() > 128
            || !o
                .deployment_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("invalid deployment ID".into());
        }
        if o.revision == 0 || o.max_concurrency == 0 || o.max_concurrency > 256 || o.fee_bps != 50 {
            return Err("invalid capacity, revision or platform fee".into());
        }
        if o.issued_at > now.saturating_add(30)
            || o.expires_at <= now
            || o.expires_at <= o.issued_at
            || o.expires_at - o.issued_at > MAX_OFFER_LIFETIME
        {
            return Err("expired or invalid offer lifetime".into());
        }
        let namespace = format!("{}/{}/{}", o.provider_wallet, o.deployment_id, o.model.id);
        let id = protocol::offer_id(o)?;
        let mut state = self.state.write();
        // Any lower-revision signature issued before this watermark was observed
        // must expire by this horizon. Replays never extend the retention window.
        let expired: Vec<_> = state
            .entries
            .iter()
            .filter(|(_, e)| e.retain_until <= now)
            .map(|(namespace, e)| (namespace.clone(), e.id.clone()))
            .collect();
        for (namespace, id) in expired {
            state.entries.remove(&namespace);
            state.ids.remove(&id);
        }
        if let Some(old) = state.entries.get(&namespace) {
            if o.revision < old.revision
                || (o.revision == old.revision && (old.id != id || old.offer.is_none()))
            {
                return Err("offer revision rollback or equivocation".into());
            }
            if old.id == id {
                return Ok(id);
            }
        } else if state.entries.len() >= MAX_OFFERS {
            return Err("offer registry capacity exhausted".into());
        }
        let entry = Entry {
            revision: o.revision,
            id: id.clone(),
            offer: Some(signed),
            retain_until: now.saturating_add(MAX_OFFER_LIFETIME + 30),
        };
        if let Some(old) = state.entries.insert(namespace.clone(), entry) {
            state.ids.remove(&old.id);
        }
        state.ids.insert(id.clone(), namespace);
        Ok(id)
    }
    pub fn withdraw(&self, id: &str) {
        let mut state = self.state.write();
        if let Some(namespace) = state.ids.remove(id) {
            if let Some(entry) = state.entries.get_mut(&namespace) {
                entry.offer = None;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn offer() -> Signed<InferenceOffer> {
        let key = "1111111111111111111111111111111111111111111111111111111111111111";
        let now = protocol::now().unwrap();
        protocol::sign(
            protocol::OFFER_DOMAIN,
            InferenceOffer {
                network: NetworkIdentity {
                    chain_id: 71,
                    genesis_hash: "0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804".into(),
                    deployment_id: "test".into(),
                },
                provider_wallet: crate::wallet::address_from_private_key(key).unwrap(),
                peer_id: libp2p::identity::Keypair::generate_ed25519()
                    .public()
                    .to_peer_id()
                    .to_string(),
                deployment_id: "gpu-a".into(),
                endpoint: "https://inference.example.org/".into(),
                model: ModelSpec {
                    id: "model".into(),
                    revision: "ab".repeat(20),
                    tokenizer_revision: "ab".repeat(20),
                    chat_template_sha256: "ab".repeat(32),
                    quantization: None,
                    context_length: 4096,
                    max_output_tokens: 512,
                    capabilities: vec!["chat".into()],
                    serving: None,
                },
                price: crate::protocol::PriceSchedule {
                    input_wei_per_1k: "1".into(),
                    output_wei_per_1k: "2".into(),
                },
                min_funding_wei: "1".into(),
                platform_wallet: format!("0x{}", "12".repeat(20)),
                fee_bps: 50,
                max_concurrency: 1,
                revision: 1,
                issued_at: now,
                expires_at: now + 300,
            },
            key,
        )
        .unwrap()
    }
    #[test]
    fn embedding_limits_do_not_claim_generated_output() {
        let mut model = offer().payload.model;
        model.max_output_tokens = 0;
        assert!(validate_model(&model).is_err());
        model.capabilities = vec!["embeddings".into()];
        assert!(validate_model(&model).is_ok());
        model.max_output_tokens = 1;
        assert!(validate_model(&model).is_err());
        model.capabilities.push("chat".into());
        assert!(validate_model(&model).is_ok());
    }
    fn resign(
        mut o: Signed<InferenceOffer>,
        revision: u64,
        deployment: &str,
    ) -> Signed<InferenceOffer> {
        o.payload.revision = revision;
        o.payload.deployment_id = deployment.into();
        protocol::sign(
            protocol::OFFER_DOMAIN,
            o.payload,
            "1111111111111111111111111111111111111111111111111111111111111111",
        )
        .unwrap()
    }
    #[test]
    fn invalid_first_does_not_poison_namespace() {
        let good = offer();
        let registry = OfferRegistry::new(good.payload.network.clone(), false);
        let mut bad = good.clone();
        bad.payload.revision = 100;
        assert!(registry.insert(bad).is_err());
        assert!(registry.insert(good).is_ok());
    }
    #[test]
    fn revisions_deployments_and_withdrawals() {
        let a = offer();
        let r = OfferRegistry::new(a.payload.network.clone(), false);
        r.insert(a.clone()).unwrap();
        r.insert(resign(a.clone(), 2, "gpu-a")).unwrap();
        assert!(r.insert(a.clone()).is_err());
        let id = r.insert(resign(a.clone(), 1, "gpu-b")).unwrap();
        assert_eq!(r.list().len(), 2);
        r.withdraw(&id);
        assert_eq!(r.list().len(), 1);
        assert!(r.insert(resign(a, 1, "gpu-b")).is_err());
    }
    #[test]
    fn expiry_network_and_tampering_rejected() {
        let a = offer();
        let r = OfferRegistry::new(a.payload.network.clone(), false);
        assert!(r.insert_at(a.clone(), a.payload.expires_at).is_err());
        let mut b = a.clone();
        b.payload.network.chain_id += 1;
        assert!(r.insert(b).is_err());
        let mut c = a;
        c.payload.price.input_wei_per_1k = "0".into();
        assert!(r.insert(c).is_err());
    }
    #[test]
    fn rejects_ssrf_origins() {
        for url in [
            "http://example.com",
            "https://127.0.0.1",
            "https://[::ffff:127.0.0.1]",
            "https://169.254.169.254",
            "https://100.64.0.1",
            "https://example.com:8443",
            "https://example.com/path",
            "https://user@example.com",
            "https://localhost",
        ] {
            assert!(validate_endpoint(url, false).is_err(), "{url}");
        }
        assert!(validate_endpoint("http://127.0.0.1:8080", true).is_ok());
    }
    #[test]
    fn development_does_not_allow_metadata_or_lan() {
        for endpoint in [
            "http://169.254.169.254",
            "https://10.0.0.1",
            "https://100.64.0.1",
            "http://example.com",
            "https://[fd00::1]",
        ] {
            assert!(validate_endpoint(endpoint, true).is_err(), "{endpoint}");
        }
        assert!(validate_endpoint("http://localhost:8080", true).is_ok());
    }
    #[test]
    fn expired_watermarks_can_be_evicted_without_replaying_old_signatures() {
        let old = offer();
        let now = old.payload.issued_at;
        let registry = OfferRegistry::new(old.payload.network.clone(), false);
        let id = registry.insert_at(old.clone(), now).unwrap();
        registry.withdraw(&id);
        assert!(registry.get(&id).is_none());
        let later = now + MAX_OFFER_LIFETIME + 31;
        let mut fresh = old.payload.clone();
        fresh.issued_at = later;
        fresh.expires_at = later + 300;
        let fresh = protocol::sign(
            protocol::OFFER_DOMAIN,
            fresh,
            "1111111111111111111111111111111111111111111111111111111111111111",
        )
        .unwrap();
        registry.insert_at(fresh, later).unwrap();
        assert!(registry.insert_at(old, later).is_err());
        assert_eq!(registry.state.read().entries.len(), 1);
    }
}
