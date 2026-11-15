//! Public Consumer::quote contracts for specify-inference-commercial-terms tasks 2.1 and 2.4.
//! Isolated loopback counterparties return complete signed quotes, including adversarial terms.
//! Public test scalars only; no funding, chain simulation, external services or qualification claims.
//! Fresh timestamps keep fixtures admissible; no sleeps or current-time boundary assertions.
use axum::{extract::State, routing::post, Json, Router};
use chiral_network::{
    consumer::{Consumer, ConsumerConfig},
    network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    protocol::{self, InferenceOffer, ModelSpec, PriceSchedule, Quote, Signed, OFFER_DOMAIN, QUOTE_DOMAIN},
    wallet,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::{net::TcpListener, task::JoinHandle};

struct Replies {
    next: Mutex<Option<Value>>,
    requests: Mutex<Vec<Value>>,
}

async fn propose(State(state): State<Arc<Replies>>, Json(request): Json<Value>) -> Json<Value> {
    state.requests.lock().unwrap().push(request);
    let response = state.next.lock().unwrap().take().expect("one prepared response per request");
    Json(response)
}

fn declared_model(measured_at: u64) -> ModelSpec {
    // Signed synthetic declaration, not an artifact inventory or measured performance report.
    serde_json::from_value(json!({
        "id": "test/quote-model",
        "revision": "ab".repeat(20),
        "tokenizer_revision": "ab".repeat(20),
        "chat_template_sha256": "cd".repeat(32),
        "quantization": null,
        "context_length": 4096,
        "max_output_tokens": 512,
        "capabilities": ["chat", "completions"],
        "serving": {
            "hf_repo": "test/model",
            "tokenizer_repo": "test/model",
            "artifact_manifest_sha256": "a".repeat(64),
            "engine": "llama.cpp",
            "engine_version": "test-only",
            "runtime_sha256": "b".repeat(64),
            "platform": "linux",
            "backend": "cuda",
            "memory_kind": "dedicated",
            "devices": [{
                "id": "synthetic-test-device",
                "name": "synthetic declaration, not measured hardware",
                "memory_mib": 32000,
                "driver_version": "test-only",
                "engine_device": "CUDA0"
            }],
            "memory_reserve_mib": 256,
            "gpu_layers": 999,
            "split_mode": "none",
            "tensor_split": [],
            "cache_type_k": "f16",
            "cache_type_v": "f16",
            "flash_attention": true,
            "batch_size": 512,
            "ubatch_size": 128,
            "capacity_tier": "32gb",
            "region": "test",
            "max_concurrency": 2,
            "arrival_milli_rps": 1000,
            "output_tokens_per_second": 20,
            "policy": "warm-generation-v1",
            "qualification": {"evidence_sha256": "c".repeat(64), "measured_at": measured_at}
        }
    }))
    .unwrap()
}

struct Fixture {
    consumer: Consumer,
    offer: Signed<InferenceOffer>,
    quote: Quote,
    provider_key: String,
    replies: Arc<Replies>,
    server: JoinHandle<()>,
    // Drop protected consumer/journal handles before their temporary directory.
    _directory: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    async fn paid(case: u64) -> Self {
        Self::new(case, "1", "2", "100", "100").await
    }

    async fn new(case: u64, input: &str, output: &str, minimum: &str, funding: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let network = NetworkConfig {
            identity: protocol::NetworkIdentity {
                chain_id: ESPACE_TESTNET_CHAIN_ID,
                genesis_hash: ESPACE_TESTNET_GENESIS.into(),
                deployment_id: "quote-acceptance-test".into(),
            },
            // All unspecified routes are 404. There is no chain simulator or real RPC endpoint.
            rpc_url: endpoint.clone(),
            rpc_bearer_file: None,
            faucet_url: None,
            explorer_url: None,
            policy_url: None,
        };
        let consumer = Consumer::new(ConsumerConfig {
            network: network.clone(),
            data_dir: directory.path().into(),
            catalog_url: None,
            development: true,
            dht: None,
        })
        .unwrap();
        // Known public scalars, distinct consumer identities per test case; never funded keys.
        let import = directory.path().join("public-test-scalar.key");
        wallet::save_private_key(&import, &format!("{:064x}", 1000 + case)).unwrap();
        consumer.wallet_import_file(&import).unwrap();
        let provider_key = format!("{:064x}", 2);
        let now = protocol::now().unwrap();
        let offer = protocol::sign(
            OFFER_DOMAIN,
            InferenceOffer {
                network: network.identity,
                provider_wallet: wallet::address_from_private_key(&provider_key).unwrap(),
                peer_id: "QmPVGjYFugq4XUyBfoTHG6c3qxfBS26jEdaFM1gdAVuMZ2".into(),
                deployment_id: "quote-test-provider".into(),
                endpoint,
                model: declared_model(now.checked_sub(3600).unwrap()),
                price: PriceSchedule {
                    input_wei_per_1k: input.into(),
                    output_wei_per_1k: output.into(),
                },
                min_funding_wei: minimum.into(),
                platform_wallet: "0x3333333333333333333333333333333333333333".into(),
                fee_bps: 50,
                max_concurrency: 2,
                revision: 1,
                issued_at: now,
                expires_at: now + 3600,
            },
            &provider_key,
        )
        .unwrap();
        let quote = Quote {
            id: format!("0x{}", "11".repeat(32)),
            network: offer.payload.network.clone(),
            offer_id: protocol::offer_id(&offer.payload).unwrap(),
            provider_wallet: offer.payload.provider_wallet.clone(),
            consumer_wallet: consumer.address().unwrap(),
            model: offer.payload.model.clone(),
            price: offer.payload.price.clone(),
            funding_wei: funding.into(),
            platform_wallet: offer.payload.platform_wallet.clone(),
            fee_bps: offer.payload.fee_bps,
            max_concurrency: offer.payload.max_concurrency,
            nonce: format!("0x{}", "22".repeat(32)),
            issued_at: now,
            expires_at: now + 300,
        };
        let signed = protocol::sign(QUOTE_DOMAIN, quote.clone(), &provider_key).unwrap();
        let replies = Arc::new(Replies {
            next: Mutex::new(Some(serde_json::to_value(signed).unwrap())),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/v1/contracts/propose", post(propose))
            .with_state(replies.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { consumer, offer, quote, provider_key, replies, server, _directory: directory }
    }

    fn select_offer(&mut self, payload: InferenceOffer) {
        self.offer = protocol::sign(OFFER_DOMAIN, payload, &self.provider_key).unwrap();
        let offer = &self.offer.payload;
        self.quote.network = offer.network.clone();
        self.quote.offer_id = protocol::offer_id(offer).unwrap();
        self.quote.provider_wallet = offer.provider_wallet.clone();
        self.quote.model = offer.model.clone();
        self.quote.price = offer.price.clone();
        self.quote.platform_wallet = offer.platform_wallet.clone();
        self.quote.fee_bps = offer.fee_bps;
        self.quote.max_concurrency = offer.max_concurrency;
        self.reply(self.quote.clone());
    }

    fn reply(&self, quote: Quote) -> Signed<Quote> {
        let signed = protocol::sign(QUOTE_DOMAIN, quote, &self.provider_key).unwrap();
        self.reply_raw(serde_json::to_value(&signed).unwrap());
        signed
    }

    fn reply_raw(&self, body: Value) {
        *self.replies.next.lock().unwrap() = Some(body);
    }

    async fn request(&self, funding: &str) -> Result<Signed<Quote>, String> {
        let result = self.consumer.quote(&self.offer, funding).await;
        let mut requests = self.replies.requests.lock().unwrap();
        // A negative assertion must exercise the delivered quote, not pass on a transport failure.
        assert_eq!(requests.len(), 1, "the prepared response must be requested exactly once");
        assert!(self.replies.next.lock().unwrap().is_none(), "response was not served");
        assert_eq!(
            requests.pop().unwrap(),
            json!({
                "offer_id": self.quote.offer_id,
                "consumer_wallet": self.quote.consumer_wallet,
                "funding_wei": funding
            })
        );
        result
    }
}

#[tokio::test]
async fn native_quote_accepts_the_complete_selected_signed_agreement() {
    let fixture = Fixture::paid(1).await;
    let expected = fixture.reply(fixture.quote.clone());
    let accepted = fixture.request("100").await.unwrap();
    assert_eq!(accepted.payload, expected.payload);
    assert_eq!(accepted.signature, expected.signature);
}

async fn reject_positional_quote_terms(case: u64, path: &str, fields: &[&str]) {
    let fixture = Fixture::paid(case).await;
    let expected = fixture.reply(fixture.quote.clone());
    let commitment = protocol::opening_data(&expected.payload).unwrap();
    assert_eq!(fixture.request("100").await.unwrap().payload, expected.payload);
    let original = serde_json::to_value(&expected).unwrap();
    let object = original.pointer(path).unwrap();
    let encoded_object = serde_json::to_string(object).unwrap();
    let encoded = serde_json::to_string(&original).unwrap();
    for (field, value) in object.as_object().unwrap() {
        let duplicate = format!("{{{}:{},{}", json!(field), value, &encoded_object[1..]);
        let raw = encoded.replacen(&encoded_object, &duplicate, 1);
        assert!(serde_json::from_str::<Signed<Quote>>(&raw).is_err(),
            "raw duplicate {path}/{field} must not be projected out");
    }
    let positional = Value::Array(fields.iter()
        .map(|field| object.get(*field).expect("complete declared field").clone()).collect());
    let mut changed = original.clone();
    *changed.pointer_mut(path).unwrap() = positional;
    // Preserve the original signature and every value; change only object shape.
    fixture.reply_raw(changed.clone());
    assert!(fixture.request("100").await.is_err(),
        "native quote acceptance must reject positional terms at {path}");
    assert!(serde_json::from_slice::<Signed<Quote>>(
        &serde_json::to_vec(&changed).unwrap()
    ).is_err());
    assert!(serde_json::from_value::<Signed<Quote>>(changed).is_err());

    fixture.reply_raw(original);
    let accepted = fixture.request("100").await.unwrap();
    assert_eq!(accepted.payload, expected.payload);
    assert_eq!(accepted.signature, expected.signature);
    assert_eq!(protocol::opening_data(&accepted.payload).unwrap(), commitment);
}

#[tokio::test]
async fn native_quote_rejects_positional_model_terms() {
    reject_positional_quote_terms(300, "/payload/model", &[
        "id", "revision", "tokenizer_revision", "chat_template_sha256", "quantization",
        "context_length", "max_output_tokens", "capabilities", "serving",
    ]).await;
}

#[tokio::test]
async fn native_quote_rejects_positional_profile_terms() {
    reject_positional_quote_terms(301, "/payload/model/serving", &[
        "hf_repo", "tokenizer_repo", "artifact_manifest_sha256", "engine", "engine_version",
        "runtime_sha256", "platform", "backend", "memory_kind", "devices",
        "memory_reserve_mib", "gpu_layers", "split_mode", "tensor_split", "cache_type_k",
        "cache_type_v", "flash_attention", "batch_size", "ubatch_size", "capacity_tier",
        "region", "max_concurrency", "arrival_milli_rps", "output_tokens_per_second",
        "policy", "qualification",
    ]).await;
}

#[tokio::test]
async fn native_quote_rejects_positional_quote_terms() {
    reject_positional_quote_terms(302, "/payload", &[
        "id", "network", "offer_id", "provider_wallet", "consumer_wallet", "model",
        "price", "funding_wei", "platform_wallet", "fee_bps", "max_concurrency",
        "nonce", "issued_at", "expires_at",
    ]).await;
}

#[tokio::test]
async fn native_offer_rejects_positional_offer_terms() {
    let fixture = Fixture::paid(303).await;
    let original = serde_json::to_value(&fixture.offer).unwrap();
    let decoded: Signed<InferenceOffer> = serde_json::from_value(original.clone()).unwrap();
    protocol::verify(OFFER_DOMAIN, &decoded, &fixture.offer.payload.provider_wallet).unwrap();
    let id = protocol::offer_id(&decoded.payload).unwrap();
    assert_eq!(decoded.payload, fixture.offer.payload);
    assert_eq!(fixture.request("100").await.unwrap().payload.offer_id, id);

    let object = &original["payload"];
    let encoded_object = serde_json::to_string(object).unwrap();
    let encoded = serde_json::to_string(&original).unwrap();
    for (field, value) in object.as_object().unwrap() {
        let duplicate = format!("{{{}:{},{}", json!(field), value, &encoded_object[1..]);
        let raw = encoded.replacen(&encoded_object, &duplicate, 1);
        assert!(serde_json::from_str::<Signed<InferenceOffer>>(&raw).is_err(),
            "raw duplicate offer {field} must not be projected out");
    }
    let fields = [
        "network", "provider_wallet", "peer_id", "deployment_id", "endpoint", "model",
        "price", "min_funding_wei", "platform_wallet", "fee_bps", "max_concurrency",
        "revision", "issued_at", "expires_at",
    ];
    let positional = Value::Array(fields.iter()
        .map(|field| object.get(*field).expect("complete offer field").clone()).collect());
    let mut changed = original;
    changed["payload"] = positional;
    assert!(serde_json::from_slice::<Signed<InferenceOffer>>(
        &serde_json::to_vec(&changed).unwrap()
    ).is_err(), "offer payload must be an object, not a positional array");
    assert!(serde_json::from_value::<Signed<InferenceOffer>>(changed).is_err());
}

#[tokio::test]
async fn provider_resigning_changed_commercial_terms_does_not_authorize_substitution() {
    let fixture = Fixture::paid(2).await;
    let changes: &[(&str, fn(&mut Quote))] = &[
        ("chain", |q| q.network.chain_id += 1),
        ("genesis", |q| q.network.genesis_hash = format!("0x{}", "44".repeat(32))),
        ("network deployment", |q| q.network.deployment_id.push_str("-other")),
        ("offer", |q| q.offer_id = format!("0x{}", "55".repeat(32))),
        ("provider", |q| q.provider_wallet = "0x4444444444444444444444444444444444444444".into()),
        ("payer", |q| q.consumer_wallet = "0x5555555555555555555555555555555555555555".into()),
        ("model alias", |q| q.model.id.push_str("-other")),
        ("model revision", |q| q.model.revision = "ef".repeat(20)),
        ("tokenizer revision", |q| q.model.tokenizer_revision = "ef".repeat(20)),
        ("template", |q| q.model.chat_template_sha256 = "ef".repeat(32)),
        ("quantization", |q| q.model.quantization = Some("Q4_K_M".into())),
        ("context", |q| q.model.context_length += 1),
        ("output budget", |q| q.model.max_output_tokens += 1),
        ("capability order", |q| q.model.capabilities.reverse()),
        ("profile presence", |q| q.model.serving = None),
        ("runtime identity", |q| q.model.serving.as_mut().unwrap().runtime_sha256 = "d".repeat(64)),
        ("device identity", |q| q.model.serving.as_mut().unwrap().devices[0].id.push_str("-other")),
        ("speed class", |q| q.model.serving.as_mut().unwrap().output_tokens_per_second = 50),
        ("input price", |q| q.price.input_wei_per_1k = "3".into()),
        ("output price", |q| q.price.output_wei_per_1k = "3".into()),
        ("platform wallet", |q| q.platform_wallet = "0x4444444444444444444444444444444444444444".into()),
        ("platform fee", |q| q.fee_bps = 49),
        ("concurrency", |q| q.max_concurrency = 1),
    ];
    for (case, change) in changes {
        let mut quote = fixture.quote.clone();
        change(&mut quote);
        fixture.reply(quote);
        assert!(fixture.request("100").await.is_err(), "accepted changed {case}");
    }
}

#[tokio::test]
async fn execution_compatible_evidence_renewal_cannot_rewrite_the_selected_quote() {
    let fixture = Fixture::paid(3).await;
    let mut renewed = fixture.quote.clone();
    let evidence = &mut renewed.model.serving.as_mut().unwrap().qualification;
    evidence.evidence_sha256 = "d".repeat(64);
    evidence.measured_at += 1;
    assert!(fixture.offer.payload.model.accepts_execution(&renewed.model));
    fixture.reply(renewed);
    assert!(fixture.request("100").await.is_err());
}

#[tokio::test]
async fn requested_funding_is_exact_even_when_the_changed_amount_meets_the_minimum() {
    let fixture = Fixture::paid(4).await;
    let mut increased = fixture.quote.clone();
    increased.funding_wei = "101".into();
    fixture.reply(increased);
    assert!(fixture.request("100").await.is_err());
}

#[tokio::test]
async fn free_and_paid_quote_boundaries_depend_on_both_rates_and_exact_funding() {
    for (index, (input, output, minimum, funding, accepted)) in [
        ("1", "0", "100", "100", true),
        ("0", "2", "100", "100", true),
        ("1", "0", "100", "0", false),
        ("0", "2", "100", "0", false),
        ("1", "2", "100", "99", false),
        ("0", "0", "0", "0", true),
        ("0", "0", "0", "1", false),
        // A remote offer's minimum cannot turn zero token rates into a paid session.
        ("0", "0", "100", "0", true),
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = Fixture::new(100 + index as u64, input, output, minimum, funding).await;
        let result = fixture.request(funding).await;
        assert_eq!(result.is_ok(), accepted, "input={input}, output={output}, funding={funding}");
    }
}

#[tokio::test]
async fn noncanonical_requested_funding_is_rejected_before_contacting_the_provider() {
    let fixture = Fixture::paid(5).await;
    for amount in ["01", "+100", "1.0", " 100", "340282366920938463463374607431768211456"] {
        assert!(fixture.consumer.quote(&fixture.offer, amount).await.is_err(), "{amount}");
        assert!(fixture.replies.requests.lock().unwrap().is_empty());
    }
    assert!(fixture.request("100").await.is_ok());
}

#[tokio::test]
async fn quote_signatures_must_use_the_selected_provider_and_quote_domain() {
    let fixture = Fixture::paid(6).await;
    let wrong_signer = protocol::sign(QUOTE_DOMAIN, fixture.quote.clone(), &format!("{:064x}", 3))
        .unwrap();
    fixture.reply_raw(serde_json::to_value(wrong_signer).unwrap());
    assert!(fixture.request("100").await.is_err());
    let wrong_domain = protocol::sign(OFFER_DOMAIN, fixture.quote.clone(), &fixture.provider_key)
        .unwrap();
    fixture.reply_raw(serde_json::to_value(wrong_domain).unwrap());
    assert!(fixture.request("100").await.is_err());
}

#[tokio::test]
async fn an_invalid_selected_offer_signature_is_rejected_before_proposal() {
    let mut fixture = Fixture::paid(7).await;
    let wrong = protocol::sign(OFFER_DOMAIN, fixture.offer.payload.clone(), &format!("{:064x}", 3))
        .unwrap();
    let original = std::mem::replace(&mut fixture.offer.signature, wrong.signature);
    assert!(fixture.consumer.quote(&fixture.offer, "100").await.is_err());
    assert!(fixture.replies.requests.lock().unwrap().is_empty());
    fixture.offer.signature = original;
    assert!(fixture.request("100").await.is_ok());
}

#[tokio::test]
async fn signed_quote_identifiers_money_and_unknown_fields_are_not_silently_normalized() {
    let fixture = Fixture::paid(8).await;
    let changes: &[(&str, fn(&mut Quote))] = &[
        ("path-like identifier", |q| q.id = "../wallet.key".into()),
        ("short identifier", |q| q.id = "0x01".into()),
        ("noncanonical funding", |q| q.funding_wei = "0100".into()),
        ("funding overflow", |q| q.funding_wei = "340282366920938463463374607431768211456".into()),
    ];
    for (case, change) in changes {
        let mut quote = fixture.quote.clone();
        change(&mut quote);
        fixture.reply(quote);
        assert!(fixture.request("100").await.is_err(), "{case}");
    }
    for path in ["", "/payload"] {
        let signed = fixture.reply(fixture.quote.clone());
        let mut extended = serde_json::to_value(signed).unwrap();
        extended.pointer_mut(path).unwrap().as_object_mut().unwrap()
            .insert("unrecognized_term".into(), json!(true));
        // If an unknown field were projected out, the original typed signature would still verify.
        fixture.reply_raw(extended);
        assert!(fixture.request("100").await.is_err(), "unknown field at {path:?}");
    }
}

#[tokio::test]
async fn signed_invalid_offer_metadata_is_rejected_before_proposal() {
    let fixture = Fixture::paid(200).await;
    let cases: &[(&str, fn(&mut InferenceOffer))] = &[
        ("chain ID", |o| o.network.chain_id += 1),
        ("genesis hash", |o| o.network.genesis_hash = format!("0x{}", "ef".repeat(32))),
        ("network deployment", |o| o.network.deployment_id.push_str("-foreign")),
        ("provider wallet case", |o| {
            o.provider_wallet = format!("0x{}", o.provider_wallet[2..].to_ascii_uppercase());
        }),
        ("platform wallet case", |o| o.platform_wallet = format!("0x{}", "AB".repeat(20))),
        ("peer identity", |o| o.peer_id = "not-a-peer".into()),
        ("empty provider deployment", |o| o.deployment_id.clear()),
        ("overlong provider deployment", |o| o.deployment_id = "a".repeat(129)),
        ("provider deployment separator", |o| o.deployment_id = "provider/other".into()),
        ("non-ASCII provider deployment", |o| o.deployment_id = "déployment".into()),
        ("zero revision", |o| o.revision = 0),
        ("zero unprofiled concurrency", |o| {
            o.model.serving = None;
            o.max_concurrency = 0;
        }),
        ("excessive unprofiled concurrency", |o| {
            o.model.serving = None;
            o.max_concurrency = 257;
        }),
        ("profile concurrency mismatch", |o| o.max_concurrency = 3),
        ("absent platform fee", |o| o.fee_bps = 0),
        ("different platform fee", |o| o.fee_bps = 51),
        ("empty model identifier", |o| o.model.id.clear()),
        ("duplicate capability", |o| o.model.capabilities.push("chat".into())),
        ("unsupported profile engine", |o| {
            o.model.serving.as_mut().unwrap().engine = "vllm".into();
        }),
        ("noncanonical input price", |o| o.price.input_wei_per_1k = "01".into()),
        ("noncanonical output price", |o| o.price.output_wei_per_1k = "+2".into()),
        ("overflowing minimum funding", |o| {
            o.min_funding_wei = "340282366920938463463374607431768211456".into();
        }),
    ];
    for (case, mutate) in cases {
        let mut payload = fixture.offer.payload.clone();
        mutate(&mut payload);
        let signed = protocol::sign(OFFER_DOMAIN, payload, &fixture.provider_key).unwrap();
        assert!(fixture.consumer.quote(&signed, "100").await.is_err(), "{case}");
        assert!(fixture.replies.requests.lock().unwrap().is_empty(), "{case} reached proposal");
    }
    // The same consumer and counterparty remain usable after rejecting every malformed offer.
    assert_eq!(fixture.request("100").await.unwrap().payload, fixture.quote);
}

#[tokio::test]
async fn valid_offer_namespace_revision_and_capacity_boundaries_remain_usable() {
    let mut fixture = Fixture::paid(201).await;
    let original = fixture.offer.payload.clone();
    let cases: &[(&str, fn(&mut InferenceOffer))] = &[
        ("one-character provider deployment", |o| o.deployment_id = "_".into()),
        ("maximum provider deployment", |o| o.deployment_id = "A0_-".repeat(32)),
        ("maximum revision", |o| o.revision = u64::MAX),
        ("minimum matching concurrency", |o| {
            o.max_concurrency = 1;
            o.model.serving.as_mut().unwrap().max_concurrency = 1;
        }),
        ("maximum matching concurrency", |o| {
            o.max_concurrency = 256;
            o.model.serving.as_mut().unwrap().max_concurrency = 256;
        }),
    ];
    for (case, mutate) in cases {
        let mut payload = original.clone();
        mutate(&mut payload);
        fixture.select_offer(payload);
        let returned = fixture.request("100").await.unwrap_or_else(|error| panic!("{case}: {error}"));
        assert_eq!(returned.payload, fixture.quote, "{case}");
    }
}

#[tokio::test]
async fn offer_size_limit_counts_the_complete_signed_envelope() {
    const LIMIT: usize = 16 * 1024;
    // Control the legal wire spelling, independently of the local signer's output convention.
    fn prefix_signature(offer: &mut Signed<InferenceOffer>) {
        if !offer.signature.starts_with("0x") {
            offer.signature.insert_str(0, "0x");
        }
    }
    let mut fixture = Fixture::paid(202).await;
    let mut declaration = fixture.offer.clone();
    prefix_signature(&mut declaration);
    let profile = declaration.payload.model.serving.as_mut().unwrap();
    let template = profile.devices[0].clone();
    // Valid synthetic multi-device metadata supplies enough bounded text to reach the wire limit.
    // This is not an inventory, a native launch configuration or a qualification report.
    profile.devices = (0..32)
        .map(|index| {
            let mut device = template.clone();
            device.id = format!("test-device-{index}");
            device.engine_device = format!("CUDA{index}");
            device
        })
        .collect();
    profile.split_mode = "layer".into();
    profile.tensor_split = vec![1; 32];
    // The existing signature supplies its encoded width; select_offer re-signs before any use.
    let mut remaining = LIMIT.checked_sub(serde_json::to_vec(&declaration).unwrap().len()).unwrap();
    for device in &mut declaration.payload.model.serving.as_mut().unwrap().devices {
        for field in [&mut device.name, &mut device.driver_version] {
            let added = remaining.min(256 - field.len());
            field.extend(std::iter::repeat_n('x', added));
            remaining -= added;
        }
    }
    assert_eq!(remaining, 0, "fixture must be able to fill the envelope without invalid fields");
    fixture.select_offer(declaration.payload);
    prefix_signature(&mut fixture.offer);
    assert_eq!(serde_json::to_vec(&fixture.offer).unwrap().len(), LIMIT);
    assert_eq!(fixture.request("100").await.unwrap().payload, fixture.quote);

    let mut oversized = fixture.offer.payload.clone();
    let field = oversized.model.serving.as_mut().unwrap().devices.iter_mut()
        .flat_map(|device| [&mut device.name, &mut device.driver_version])
        .find(|field| field.len() < 256)
        .unwrap();
    field.push('x');
    fixture.select_offer(oversized);
    prefix_signature(&mut fixture.offer);
    assert_eq!(serde_json::to_vec(&fixture.offer).unwrap().len(), LIMIT + 1);
    assert!(serde_json::to_vec(&fixture.offer.payload).unwrap().len() < LIMIT);
    assert!(fixture.consumer.quote(&fixture.offer, "100").await.is_err());
    assert!(fixture.replies.requests.lock().unwrap().is_empty());

    // The wire foundation also accepts unprefixed signatures. Removing only "0x" puts the
    // identical, still-valid payload below the envelope limit; no metadata rejection may mask it.
    fixture.offer.signature = fixture.offer.signature.strip_prefix("0x").unwrap().to_owned();
    assert_eq!(serde_json::to_vec(&fixture.offer).unwrap().len(), LIMIT - 1);
    assert_eq!(fixture.request("100").await.unwrap().payload, fixture.quote);
}

#[tokio::test]
async fn offer_lifetime_bound_is_enforced_without_a_now_relative_edge() {
    let mut fixture = Fixture::paid(203).await;
    assert_eq!(fixture.offer.payload.expires_at - fixture.offer.payload.issued_at, 3600);
    assert_eq!(fixture.request("100").await.unwrap().payload, fixture.quote);
    let mut payload = fixture.offer.payload.clone();
    payload.expires_at += 1;
    fixture.select_offer(payload);
    assert!(fixture.consumer.quote(&fixture.offer, "100").await.is_err());
    assert!(fixture.replies.requests.lock().unwrap().is_empty());
}
