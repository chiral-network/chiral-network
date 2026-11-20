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

/// The revision stream an offer belongs to: `provider_wallet/deployment_id/model.id`.
pub(crate) fn namespace(offer: &InferenceOffer) -> String {
    format!(
        "{}/{}/{}",
        offer.provider_wallet, offer.deployment_id, offer.model.id
    )
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
        // A DNS root dot neither leaves the reserved localhost namespace nor adds a
        // label: classify the trimmed name, but return the origin as given. An empty
        // leading label (`.intranet`) adds no label either, so only nonempty labels count.
        // Do not use this spelling to broaden the exact development exception.
        let dns_name = host.trim_end_matches('.');
        if url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || literal.is_some_and(|ip| !public_ip(ip))
            || local
            || dns_name.eq_ignore_ascii_case("localhost")
            || dns_name.ends_with(".localhost")
            || (literal.is_none() && dns_name.split('.').filter(|l| !l.is_empty()).count() < 2)
        {
            return Err("endpoint must be public HTTPS on port 443; development only permits loopback exceptions".into());
        }
    }
    Ok(url)
}
/// Decide whether a resolved answer set may be pinned for a validated origin.
/// `loopback_origin` says that the origin's host is a literal loopback address or exactly
/// `localhost`. Every origin needs between 1 and 32 answers. Under the development
/// exception every answer must be loopback: the exception never authorizes remote
/// cleartext HTTP, so a resolver that also answers `localhost` with a public or private
/// address refuses the origin. Every other origin needs only public answers.
fn check_resolved_answers(
    answers: &[IpAddr],
    development: bool,
    loopback_origin: bool,
) -> Result<(), String> {
    if answers.is_empty() || answers.len() > 32 {
        return Err("endpoint resolves to forbidden addresses".into());
    }
    if development && loopback_origin {
        if answers.iter().any(|ip| !ip.is_loopback()) {
            return Err("a development localhost origin must resolve only to loopback addresses; use 127.0.0.1 or ::1".into());
        }
    } else if answers.iter().any(|ip| !public_ip(*ip)) {
        return Err("endpoint resolves to forbidden addresses".into());
    }
    Ok(())
}
/// Test-only resolver seam (outbound-http-safeguards tasks 3.1, 3.2 and 3.5). Inside
/// `TEST_RESOLVER.scope(..)`, `safe_http_client` records every host it is about to resolve
/// and takes that host's answers from `answers` instead of the system resolver; a host that
/// `answers` does not name fails to resolve. A scoped test therefore sends no DNS query, and
/// its `queries` show whether resolution was reached at all. Compiled only into this
/// crate's own tests: no production path can reach it, and it replaces only the resolver,
/// never validation, the answer check or pinning.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestResolver {
    pub(crate) answers: std::collections::HashMap<String, Vec<IpAddr>>,
    pub(crate) queries: std::sync::Mutex<Vec<String>>,
}
#[cfg(test)]
tokio::task_local! {
    pub(crate) static TEST_RESOLVER: std::sync::Arc<TestResolver>;
}
/// Resolves the origin's host once with the system resolver (or, in this crate's tests
/// only, with an active `TEST_RESOLVER`).
async fn resolve(host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
    #[cfg(test)]
    if let Ok(resolver) = TEST_RESOLVER.try_with(std::sync::Arc::clone) {
        resolver.queries.lock().unwrap().push(host.to_owned());
        return resolver
            .answers
            .get(host)
            .map(|ips| ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect())
            .ok_or_else(|| std::io::Error::other(format!("test resolver has no answer for {host}")));
    }
    Ok(tokio::net::lookup_host((host, port)).await?.collect())
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
    let addresses: Vec<SocketAddr> =
        tokio::time::timeout(Duration::from_secs(5), resolve(host, port))
            .await
            .map_err(|_| "DNS resolution timeout")?
            .map_err(|e| e.to_string())?
            .into_iter()
            .take(33)
            .collect();
    let answers: Vec<IpAddr> = addresses.iter().map(SocketAddr::ip).collect();
    check_resolved_answers(&answers, development, loopback_origin)?;
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
        let namespace = namespace(o);
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
    fn development_localhost_origin_requires_every_answer_to_be_loopback() {
        let ips = |answers: &[&str]| -> Vec<IpAddr> {
            answers.iter().map(|a| a.parse().unwrap()).collect()
        };
        // A public or private answer beside loopback would carry development cleartext
        // HTTP off the host if the loopback port is closed.
        for answers in [
            ips(&["127.0.0.1", "93.184.216.34"]),
            ips(&["::1", "2606:4700:4700::1111"]),
            ips(&["93.184.216.34"]),
            ips(&["127.0.0.1", "10.0.0.5"]),
        ] {
            let error = check_resolved_answers(&answers, true, true).unwrap_err();
            assert!(
                error.contains("development localhost origin must resolve only to loopback")
                    && error.contains("127.0.0.1")
                    && error.contains("::1"),
                "{answers:?}: {error}"
            );
        }
        assert!(check_resolved_answers(&ips(&["127.0.0.1", "::1"]), true, true).is_ok());
        // An IPv4-mapped answer is not a loopback address to the rule: a mapped public
        // answer (public to the old rule) is refused, and mapped loopback stays refused, as
        // `validate_endpoint` refuses the literal `[::ffff:127.0.0.1]` as an exception origin.
        for answer in ["::ffff:93.184.216.34", "::ffff:127.0.0.1"] {
            let error =
                check_resolved_answers(&ips(&["127.0.0.1", answer]), true, true).unwrap_err();
            assert!(
                error.contains("must resolve only to loopback"),
                "{answer}: {error}"
            );
        }
        assert!(validate_endpoint("http://[::ffff:127.0.0.1]:9419", true).is_err());
        // A literal loopback origin resolves to itself and stays admitted.
        assert!(check_resolved_answers(&ips(&["127.0.0.1"]), true, true).is_ok());
        assert!(check_resolved_answers(&ips(&["::1"]), true, true).is_ok());
        // Other origins keep the public rule and its message, with or without development.
        for development in [false, true] {
            assert!(check_resolved_answers(&ips(&["93.184.216.34"]), development, false).is_ok());
            for answers in [
                Vec::new(),
                ips(&["93.184.216.34", "10.0.0.5"]),
                ips(&["127.0.0.1"]),
            ] {
                assert_eq!(
                    check_resolved_answers(&answers, development, false).unwrap_err(),
                    "endpoint resolves to forbidden addresses",
                    "{development}/{answers:?}"
                );
            }
        }
        // Without development the loopback flag grants nothing.
        assert_eq!(
            check_resolved_answers(&ips(&["127.0.0.1"]), false, true).unwrap_err(),
            "endpoint resolves to forbidden addresses"
        );
        // The empty-answer rule also holds under the exception.
        assert_eq!(
            check_resolved_answers(&[], true, true).unwrap_err(),
            "endpoint resolves to forbidden addresses"
        );
        // The 32-answer bound is unchanged for both kinds of origin and is checked first.
        let loopback: Vec<IpAddr> = (1..=33u8)
            .map(|n| IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, n)))
            .collect();
        let public: Vec<IpAddr> = (1..=33u8)
            .map(|n| IpAddr::V4(std::net::Ipv4Addr::new(93, 184, 216, n)))
            .collect();
        for (answers, development, loopback_origin) in [
            (&loopback, true, true),
            (&public, false, false),
            (&public, true, false),
        ] {
            assert!(check_resolved_answers(&answers[..32], development, loopback_origin).is_ok());
            assert_eq!(
                check_resolved_answers(answers, development, loopback_origin).unwrap_err(),
                "endpoint resolves to forbidden addresses",
                "{development}/{loopback_origin}"
            );
        }
        let mut mixed = loopback[..32].to_vec();
        mixed.push("93.184.216.34".parse().unwrap());
        assert_eq!(
            check_resolved_answers(&mixed, true, true).unwrap_err(),
            "endpoint resolves to forbidden addresses"
        );
    }
    /// outbound-http-safeguards requirement 5, task 3.5: `public_ip` judged directly, at every
    /// address of the three predicate scenarios and at both edges of each listed range.
    #[test]
    fn public_ip_is_the_exact_documented_predicate() {
        let judge = |ip: &str| public_ip(ip.parse().unwrap());
        // Scenario "IPv4 range boundaries".
        for ip in ["100.63.255.255", "100.128.0.0", "198.17.255.255", "198.20.0.0", "192.0.1.1", "1.1.1.1"] {
            assert!(judge(ip), "{ip} must be public");
        }
        for ip in [
            "100.64.0.0", "100.127.255.255", "198.18.0.0", "198.19.255.255", "192.0.0.1",
            "172.31.255.254", "169.254.169.254", "0.1.2.3", "240.0.0.1",
        ] {
            assert!(!judge(ip), "{ip} must not be public");
        }
        // Scenario "IPv4-mapped IPv6 follows the IPv4 predicate".
        for ip in ["::ffff:10.0.0.1", "::ffff:127.0.0.1", "::ffff:169.254.169.254"] {
            assert!(!judge(ip), "{ip} must not be public");
        }
        assert!(judge("::ffff:1.1.1.1"));
        // Scenario "IPv6 global unicast with documented exclusions".
        for ip in ["2606:4700:4700::1111", "2001:200::1", "3fff:1000::1"] {
            assert!(judge(ip), "{ip} must be public");
        }
        for ip in [
            "::", "::1", "fd00::1", "fe80::1", "ff02::1", "2001:1ff:ffff::1", "2001:db8::1",
            "2002:7f00:1::", "3fff:fff::1",
        ] {
            assert!(!judge(ip), "{ip} must not be public");
        }
        // Each IPv4 range of the requirement, with the public address on either side.
        for (first, last, before, after) in [
            ("0.0.0.0", "0.255.255.255", None, Some("1.0.0.0")),
            ("10.0.0.0", "10.255.255.255", Some("9.255.255.255"), Some("11.0.0.0")),
            ("100.64.0.0", "100.127.255.255", Some("100.63.255.255"), Some("100.128.0.0")),
            ("127.0.0.0", "127.255.255.255", Some("126.255.255.255"), Some("128.0.0.0")),
            ("169.254.0.0", "169.254.255.255", Some("169.253.255.255"), Some("169.255.0.0")),
            ("172.16.0.0", "172.31.255.255", Some("172.15.255.255"), Some("172.32.0.0")),
            ("192.0.0.0", "192.0.0.255", Some("191.255.255.255"), Some("192.0.1.0")),
            ("192.0.2.0", "192.0.2.255", Some("192.0.1.255"), Some("192.0.3.0")),
            ("192.168.0.0", "192.168.255.255", Some("192.167.255.255"), Some("192.169.0.0")),
            ("198.18.0.0", "198.19.255.255", Some("198.17.255.255"), Some("198.20.0.0")),
            ("198.51.100.0", "198.51.100.255", Some("198.51.99.255"), Some("198.51.101.0")),
            ("203.0.113.0", "203.0.113.255", Some("203.0.112.255"), Some("203.0.114.0")),
            // Multicast 224.0.0.0/4 and reserved 240.0.0.0/4 are adjacent, so their shared
            // edge has no public neighbour; both of its sides are checked.
            ("224.0.0.0", "239.255.255.255", Some("223.255.255.255"), None),
            ("240.0.0.0", "255.255.255.255", None, None),
        ] {
            assert!(!judge(first) && !judge(last), "{first}-{last} must not be public");
            for public in [before, after].into_iter().flatten() {
                assert!(judge(public), "{public} must be public");
            }
            // A mapped address is judged by the same IPv4 rule.
            assert!(!judge(&format!("::ffff:{first}")) && !judge(&format!("::ffff:{last}")));
        }
        // IPv6: public exactly within 2000::/3 outside 2001::/23, 2001:db8::/32, 2002::/16
        // and 3fff::/20. IPv4-compatible and NAT64 spellings are not mapped addresses.
        for ip in [
            "2000::", "2001:200::", "2001:db7:ffff:ffff:ffff:ffff:ffff:ffff", "2001:db9::",
            "2001:ffff:ffff:ffff:ffff:ffff:ffff:ffff", "2003::", "3ffe:ffff::1", "3fff:1000::", "3fff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
        ] {
            assert!(judge(ip), "{ip} must be public");
        }
        for ip in [
            "1fff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", "4000::", "2001::", "2001:1ff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:db8:ffff:ffff:ffff:ffff:ffff:ffff", "2002::", "2002:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "3fff::", "3fff:fff:ffff:ffff:ffff:ffff:ffff:ffff", "::1.1.1.1", "64:ff9b::101:101",
        ] {
            assert!(!judge(ip), "{ip} must not be public");
        }
    }
    /// The answers `answers` gives `TEST_RESOLVER` for `host`.
    pub(crate) fn test_resolver(answers: &[(&str, &[&str])]) -> std::sync::Arc<TestResolver> {
        std::sync::Arc::new(TestResolver {
            answers: answers
                .iter()
                .map(|(host, ips)| (host.to_string(), ips.iter().map(|ip| ip.parse().unwrap()).collect()))
                .collect(),
            queries: Default::default(),
        })
    }
    /// outbound-http-safeguards requirement 5 scenario "Literal and resolved addresses use one
    /// predicate" (task 3.5) and requirement 2 scenario "A localhost origin that resolves to a
    /// remote address is refused" (task 3.2), through `safe_http_client` with resolver answers
    /// that the test controls. A refusal builds no client, so nothing can connect; a built
    /// client is never used here.
    #[tokio::test]
    async fn safe_http_client_judges_every_resolved_answer_by_the_same_rules() {
        let client = |resolver: std::sync::Arc<TestResolver>, endpoint: &'static str, development: bool| async move {
            let built = TEST_RESOLVER.scope(resolver.clone(), safe_http_client(endpoint, development)).await;
            let queries = resolver.queries.lock().unwrap().clone();
            (built.map(drop), queries)
        };
        // The answer-set rule itself refuses a resolved IPv4-mapped loopback address.
        let mapped: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        for development in [false, true] {
            assert_eq!(
                check_resolved_answers(&[mapped], development, false).unwrap_err(),
                "endpoint resolves to forbidden addresses"
            );
        }
        // A hostname whose resolution includes `::ffff:127.0.0.1`, alone or beside a public
        // answer, is refused after one resolution, with or without development.
        for answers in [&["93.184.216.34", "::ffff:127.0.0.1"][..], &["::ffff:127.0.0.1"]] {
            for development in [false, true] {
                let resolver = test_resolver(&[("provider.example", answers)]);
                assert_eq!(
                    client(resolver, "https://provider.example", development).await,
                    (Err("endpoint resolves to forbidden addresses".into()), vec!["provider.example".to_string()]),
                    "{answers:?}/{development}"
                );
            }
        }
        // The literal origin is refused by the same predicate before any resolution.
        for development in [false, true] {
            assert_eq!(
                client(test_resolver(&[]), "https://[::ffff:127.0.0.1]", development).await,
                (Err(validate_endpoint("https://[::ffff:127.0.0.1]", development).unwrap_err()), Vec::new())
            );
        }
        // Positive control: only public answers build the client.
        let resolver = test_resolver(&[("provider.example", &["93.184.216.34", "2606:4700:4700::1111"])]);
        assert_eq!(client(resolver, "https://provider.example", false).await.0, Ok(()));
        // Development `localhost`: a public answer beside loopback builds no client; only
        // loopback answers do. Without development the origin is refused before resolution.
        let localhost = "http://localhost:9419";
        let resolver = test_resolver(&[("localhost", &["127.0.0.1", "93.184.216.34"])]);
        let (built, queries) = client(resolver, localhost, true).await;
        assert!(built.unwrap_err().contains("must resolve only to loopback"));
        assert_eq!(queries, ["localhost"]);
        let resolver = test_resolver(&[("localhost", &["127.0.0.1", "::1"])]);
        assert_eq!(client(resolver, localhost, true).await, (Ok(()), vec!["localhost".to_string()]));
        let resolver = test_resolver(&[("localhost", &["127.0.0.1", "::1"])]);
        assert_eq!(
            client(resolver, localhost, false).await,
            (Err(validate_endpoint(localhost, false).unwrap_err()), Vec::new())
        );
    }
    /// outbound-http-safeguards requirement 2 scenarios "Development admits loopback origins
    /// only" and "Without development, loopback origins are refused" (task 3.2), with their
    /// exact origins, through `safe_http_client`: an admitted origin reaches resolution once
    /// and builds its client from loopback answers; a refused origin never reaches resolution.
    #[tokio::test]
    async fn development_scenario_origins_reach_resolution_only_when_admitted() {
        let resolver = || {
            test_resolver(&[
                ("127.0.0.1", &["127.0.0.1"]),
                ("::1", &["::1"]),
                ("localhost", &["127.0.0.1", "::1"]),
            ])
        };
        for (origin, host) in [
            ("http://127.0.0.1:9419", "127.0.0.1"),
            ("http://[::1]:9419", "::1"),
            ("http://localhost:8080", "localhost"),
        ] {
            let resolver = resolver();
            let built = TEST_RESOLVER.scope(resolver.clone(), safe_http_client(origin, true)).await;
            assert_eq!(built.map(drop), Ok(()), "{origin}");
            assert_eq!(*resolver.queries.lock().unwrap(), [host], "{origin}");
        }
        for (origin, development) in [
            ("http://10.0.0.1:9419", true),
            ("https://192.168.10.1", true),
            ("http://169.254.169.254", true),
            ("https://provider.localhost", true),
            ("http://localhost.:8080", true),
            ("http://provider.example:8080", true),
            ("http://127.0.0.1:9419", false),
            ("https://127.0.0.1", false),
            ("https://localhost", false),
        ] {
            let resolver = resolver();
            let built = TEST_RESOLVER.scope(resolver.clone(), safe_http_client(origin, development)).await;
            assert_eq!(
                built.map(drop),
                Err(validate_endpoint(origin, development).unwrap_err()),
                "{origin}/{development}"
            );
            assert!(resolver.queries.lock().unwrap().is_empty(), "{origin}/{development}");
        }
    }
    /// outbound-http-safeguards requirement 2 scenario "A localhost origin that resolves to a
    /// remote address is refused", its last clause (task 3.2): a development `localhost`
    /// client built from loopback answers is pinned to them. Two recorders listen on one port,
    /// at `127.0.0.1` and at `[::1]`, and the system resolver answers `localhost` with both;
    /// a client built from one answer connects only to that answer's recorder, so an unpinned
    /// client fails one of the two cases whichever address the system prefers. Loopback only,
    /// no DNS query. (Pinning of public answers, the deadline, the connect timeout, proxies
    /// and redirects remain task 3.4's evidence.)
    #[tokio::test]
    async fn a_development_localhost_client_connects_only_to_its_loopback_answers() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        fn record(listener: tokio::net::TcpListener) -> Arc<AtomicUsize> {
            let count = Arc::new(AtomicUsize::new(0));
            let counter = count.clone();
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    counter.fetch_add(1, Ordering::SeqCst);
                    drop(stream);
                }
            });
            count
        }
        let mut pair = None;
        for _ in 0..100 {
            let v4 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = v4.local_addr().unwrap().port();
            if let Ok(v6) = tokio::net::TcpListener::bind(("::1", port)).await {
                pair = Some((v4, v6));
                break;
            }
        }
        let (v4, v6) = pair.expect("no port free on both 127.0.0.1 and [::1]; this test needs IPv6 loopback");
        let origin = format!("http://localhost:{}", v4.local_addr().unwrap().port());
        let (v4, v6) = (record(v4), record(v6));
        for (answer, pinned, other) in [("127.0.0.1", &v4, &v6), ("::1", &v6, &v4)] {
            let before = (pinned.load(Ordering::SeqCst), other.load(Ordering::SeqCst));
            let resolver = test_resolver(&[("localhost", &[answer])]);
            let client = TEST_RESOLVER
                .scope(resolver.clone(), safe_http_client(&origin, true))
                .await
                .unwrap();
            assert_eq!(*resolver.queries.lock().unwrap(), ["localhost"]);
            // The recorder closes the connection unanswered, after counting it.
            assert!(client.get(format!("{origin}/healthz")).send().await.is_err());
            assert_eq!(pinned.load(Ordering::SeqCst), before.0 + 1, "pinned to {answer}");
            assert_eq!(other.load(Ordering::SeqCst), before.1, "pinned to {answer}");
        }
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
