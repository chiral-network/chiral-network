//! Deterministic contracts for specify-inference-wire-foundation tasks 2.1–2.2.
//! No live RPC, wall-clock-dependent offers, or production credentials.
use chiral_network::{
    billing_store::BillingStore,
    network::NetworkConfig,
    offer_registry::validate_money,
    protocol::{self, InferenceOffer, ModelSpec, NetworkIdentity, PriceSchedule, Quote, Signed},
    wallet,
};
use serde_json::{json, Value};

// Independent vectors: Python json.dumps(sort_keys=True, ensure_ascii=False,
// separators=(',', ':')) and explicit big-endian framing; OpenSSL 3.6.3
// `dgst -KECCAK-256 -binary`, not SHA3-256. The public scalar-one signature
// was generated/verified with OpenSSL pkeyutl on the prehashed personal frame
// (digest:sha256, nonce-type:1 for RFC6979), then normalized to low s.
// Its recovery parity was derived independently from R = ((digest + r) / s) G.
// Tests consume fixed answers, never invoke OpenSSL or generate their own oracle.
const VECTOR_WALLET: &str = "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf";
const QUOTE_SIGNATURE: &str = concat!(
    "0d06f84175a4eaa64299bf63e2df85338dd6e9d8ff82af6fcd9dbb03270f9c61",
    "2fad6705c8d56c932390d78be3f7eb82330407dc49e0c835f419170fefa4e9a11b"
);
const QUOTE_PERSONAL_DIGEST: &str =
    "c1414bc6b7b4f7617fad939363d9fe4f7d0240f70f5b3f109592ee232aea3f0c";
const OPENING_DATA: &str =
    "4348524c3100bc598c5c5402510fed3c9e146fa25faf087a3097f7c1995abd699d4e5da83552";
const OFFER_ID: &str = "0xcb4a5847bcae0acd0ed979cf40e5e97e287357ec56bc1034546b3915c7ef0556";

fn network() -> NetworkIdentity {
    NetworkIdentity {
        chain_id: 71,
        genesis_hash: "0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804".into(),
        deployment_id: "wire-foundation".into(),
    }
}

fn config() -> NetworkConfig {
    NetworkConfig {
        identity: network(),
        rpc_url: "http://127.0.0.1:8545".into(),
        rpc_bearer_file: None,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    }
}

fn signed_quote() -> Signed<Quote> {
    // Public, deterministic test scalar; never a funded or operator key.
    let key = format!("{:064x}", 1);
    protocol::sign(
        protocol::QUOTE_DOMAIN,
        Quote {
            id: format!("0x{}", "01".repeat(32)),
            network: network(),
            offer_id: format!("0x{}", "02".repeat(32)),
            provider_wallet: wallet::address_from_private_key(&key).unwrap(),
            consumer_wallet: format!("0x{}", "22".repeat(20)),
            model: ModelSpec {
                id: "test/model".into(),
                revision: "ab".repeat(20),
                tokenizer_revision: "ab".repeat(20),
                chat_template_sha256: "cd".repeat(32),
                quantization: None,
                context_length: 4096,
                max_output_tokens: 256,
                capabilities: vec!["completions".into()],
                serving: None,
            },
            price: PriceSchedule {
                input_wei_per_1k: "1".into(),
                output_wei_per_1k: "2".into(),
            },
            funding_wei: "1000000000000000000".into(),
            platform_wallet: format!("0x{}", "33".repeat(20)),
            fee_bps: 50,
            max_concurrency: 1,
            nonce: format!("0x{}", "04".repeat(32)),
            issued_at: 100,
            expires_at: 400,
        },
        &key,
    )
    .unwrap()
}

fn offer() -> InferenceOffer {
    let quote = signed_quote().payload;
    InferenceOffer {
        network: quote.network,
        provider_wallet: quote.provider_wallet,
        // Base58 SHA-256 multihash with the fixed digest [0x11; 32].
        peer_id: "QmPVGjYFugq4XUyBfoTHG6c3qxfBS26jEdaFM1gdAVuMZ2".into(),
        deployment_id: "provider-a".into(),
        endpoint: "https://inference.example.org".into(),
        model: quote.model,
        price: quote.price,
        min_funding_wei: quote.funding_wei,
        platform_wallet: quote.platform_wallet,
        fee_bps: quote.fee_bps,
        max_concurrency: quote.max_concurrency,
        revision: 1,
        issued_at: quote.issued_at,
        expires_at: quote.expires_at,
    }
}

#[test]
fn network_scopes_and_reopened_ledgers_bind_every_identity_field() {
    let directory = tempfile::tempdir().unwrap();
    let original = config();
    let original_scope = original.scoped_dir(directory.path());
    let ledger = original_scope.join("ledger.sqlite3");
    let provider = format!("0x{}", "11".repeat(20));
    let other_provider = format!("0x{}", "22".repeat(20));
    let store = BillingStore::open(&ledger, &original.identity).unwrap();
    store.bind_provider(&provider).unwrap();
    drop(store);

    let mut another_chain = original.clone();
    another_chain.identity.chain_id = 72;
    let mut another_genesis = original.clone();
    another_genesis.identity.genesis_hash = format!("0x{}", "ab".repeat(32));
    let mut another_deployment = original.clone();
    another_deployment.identity.deployment_id.push_str("-other");

    for (field, changed) in [
        ("chain_id", another_chain),
        ("genesis_hash", another_genesis),
        ("deployment_id", another_deployment),
    ] {
        assert_ne!(changed.scoped_dir(directory.path()), original_scope, "{field}");
        assert!(
            BillingStore::open(&ledger, &changed.identity).is_err(),
            "a different {field} must not adopt the existing ledger"
        );
        // A rejected identity must not retag or reset the original deployment.
        let reopened = BillingStore::open(&ledger, &original.identity).unwrap();
        reopened.bind_provider(&provider).unwrap();
        assert!(reopened.bind_provider(&other_provider).is_err(), "{field}");
    }
}

#[test]
fn configuration_requires_canonical_genesis_and_bounded_ascii_deployment() {
    for deployment in ["a".into(), "A_z-09".into(), "z".repeat(128)] {
        let mut selected = config();
        selected.identity.deployment_id = deployment;
        assert!(selected.validate().is_ok(), "{:?}", selected.identity);
    }
    for deployment in [
        String::new(),
        "z".repeat(129),
        "a/b".into(),
        "a b".into(),
        "a.b".into(),
        "é".into(),
        "a\nb".into(),
    ] {
        let mut selected = config();
        selected.identity.deployment_id = deployment;
        assert!(selected.validate().is_err(), "{:?}", selected.identity);
    }
    let mut selected = config();
    selected.identity.genesis_hash = format!(
        "0x{}",
        selected.identity.genesis_hash[2..].to_ascii_uppercase()
    );
    assert!(selected.validate().is_err());

    let mut missing_rpc = serde_json::to_value(config()).unwrap();
    missing_rpc.as_object_mut().unwrap().remove("rpc_url");
    assert!(serde_json::from_value::<NetworkConfig>(missing_rpc).is_err());
}

#[test]
fn signed_envelope_and_nested_primitives_reject_unknown_fields() {
    let quote = signed_quote();
    let encoded = serde_json::to_value(&quote).unwrap();
    let decoded: Signed<Quote> = serde_json::from_value(encoded.clone()).unwrap();
    protocol::verify(
        protocol::QUOTE_DOMAIN,
        &decoded,
        &quote.payload.provider_wallet,
    )
    .unwrap();

    for path in ["", "/payload/network", "/payload/price"] {
        let mut extended = encoded.clone();
        extended
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unrecognized_term".into(), json!("not in the signed terms"));
        assert!(
            serde_json::from_value::<Signed<Quote>>(extended).is_err(),
            "unknown fields at {path:?} must not be projected out of a signed object"
        );
    }
}

#[test]
fn network_identity_rejects_wrong_primitive_types() {
    let identity = serde_json::to_value(network()).unwrap();
    for value in [json!("71"), json!(71.0), json!(-1), Value::Null, json!(true)] {
        let mut invalid = identity.clone();
        invalid["chain_id"] = value;
        assert!(serde_json::from_value::<NetworkIdentity>(invalid).is_err());
    }
    for field in ["genesis_hash", "deployment_id"] {
        for value in [json!(71), Value::Null, json!(true), json!([])] {
            let mut invalid = identity.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<NetworkIdentity>(invalid).is_err(), "{field}");
        }
    }
}

#[test]
fn primitive_schemas_reject_missing_fields() {
    let quote = serde_json::to_value(signed_quote()).unwrap();
    for (path, fields) in [
        ("", &["payload", "signature"][..]),
        (
            "/payload/network",
            &["chain_id", "genesis_hash", "deployment_id"][..],
        ),
        (
            "/payload/price",
            &["input_wei_per_1k", "output_wei_per_1k"][..],
        ),
    ] {
        for field in fields {
            let mut incomplete = quote.clone();
            incomplete
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(*field);
            assert!(
                serde_json::from_value::<Signed<Quote>>(incomplete).is_err(),
                "missing {path}/{field} must not acquire an implicit default"
            );
        }
    }
}

#[test]
fn price_schedule_requires_string_money_fields() {
    let price = json!({"input_wei_per_1k": "1", "output_wei_per_1k": "2"});
    for field in ["input_wei_per_1k", "output_wei_per_1k"] {
        for value in [json!(1), json!(1.0), Value::Null, json!(true), json!([])] {
            let mut invalid = price.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<PriceSchedule>(invalid).is_err(), "{field}");
        }
    }
}

#[test]
fn canonical_wei_accepts_exact_boundaries() {
    for (encoded, expected) in [
        ("0", 0),
        ("1", 1),
        ("1000000000000000001", 1_000_000_000_000_000_001),
        ("340282366920938463463374607431768211455", u128::MAX),
    ] {
        assert_eq!(validate_money(encoded).unwrap(), expected, "{encoded}");
    }
}

#[test]
fn canonical_wei_rejects_aliases_and_overflow() {
    for invalid in [
        "", "00", "01", "+1", "-1", " 1", "1 ", "1\n", "1.0", "1e3", "١", "１",
        "340282366920938463463374607431768211456",
        "9999999999999999999999999999999999999999",
    ] {
        assert!(validate_money(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn canonical_json_matches_independent_nested_utf8_and_escape_bytes() {
    let value = json!({
        "z": [2, {"β": "e\u{301}", "a": null}],
        "s": "é💡\"\\/\u{8}\t\n\u{c}\r\0\u{1f}",
        "a": {"b": 1, "a": -2},
    });
    let expected = hex::decode(concat!(
        "7b2261223a7b2261223a2d322c2262223a317d2c2273223a22c3a9f09f92a1",
        "5c225c5c2f5c625c745c6e5c665c725c75303030305c7530303166222c227a",
        "223a5b322c7b2261223a6e756c6c2c22ceb2223a2265cc81227d5d7d"
    )).unwrap();
    assert_eq!(protocol::canonical_json(&value).unwrap(), expected);

    let reordered: Value = serde_json::from_str(
        r#"{"a":{"a":-2,"b":1},"z":[2,{"a":null,"β":"e\u0301"}],"s":"é💡\"\\/\b\t\n\f\r\u0000\u001f"}"#
    ).unwrap();
    assert_eq!(protocol::canonical_json(&reordered).unwrap(), expected);
    let mut reversed = value;
    reversed["z"].as_array_mut().unwrap().reverse();
    assert_ne!(protocol::canonical_json(&reversed).unwrap(), expected);
    assert_eq!(
        protocol::canonical_json(&json!({"min": i64::MIN, "max": u64::MAX})).unwrap(),
        br#"{"max":18446744073709551615,"min":-9223372036854775808}"#
    );
}

#[test]
fn canonical_json_rejects_floats_recursively_and_out_of_range_integers() {
    for invalid in [
        json!(1.0),
        json!(-0.0),
        json!([0, {"value": 1.0}]),
        json!({"outer": {"inner": [1, 2.5]}}),
    ] {
        assert!(protocol::canonical_json(&invalid).is_err(), "{invalid}");
    }
    assert!(protocol::canonical_json(&u128::MAX).is_err());
    assert!(protocol::canonical_json(&i128::MIN).is_err());
}

#[test]
fn operation_frames_match_fixed_byte_lengths_and_domain_vectors() {
    let value = json!({"x": "é"});
    for (domain, expected) in [
        (protocol::OFFER_DOMAIN, "0000001963686972616c2d696e666572656e63652d6f666665722d76310000000a7b2278223a22c3a9227d"),
        (protocol::QUOTE_DOMAIN, "0000001963686972616c2d696e666572656e63652d71756f74652d76310000000a7b2278223a22c3a9227d"),
        (protocol::INTENT_DOMAIN, "0000002263686972616c2d696e666572656e63652d66756e64696e672d696e74656e742d76310000000a7b2278223a22c3a9227d"),
        (protocol::RECEIPT_DOMAIN, "0000001b63686972616c2d696e666572656e63652d726563656970742d76310000000a7b2278223a22c3a9227d"),
        (protocol::AUTH_DOMAIN, "0000001863686972616c2d696e666572656e63652d617574682d76310000000a7b2278223a22c3a9227d"),
        (protocol::FEEDBACK_DOMAIN, "0000001c63686972616c2d696e666572656e63652d666565646261636b2d76310000000a7b2278223a22c3a9227d"),
        ("é", "00000002c3a90000000a7b2278223a22c3a9227d"),
    ] {
        assert_eq!(
            protocol::signing_payload(domain, &value).unwrap(),
            hex::decode(expected).unwrap(),
            "{domain}"
        );
    }
}

#[test]
fn independent_personal_signature_binds_signer_domain_and_terms() {
    let mut signed = Signed {
        payload: signed_quote().payload,
        signature: QUOTE_SIGNATURE.into(),
    };
    assert_eq!(signed.payload.provider_wallet, VECTOR_WALLET);
    for encoding in [
        QUOTE_SIGNATURE.to_string(),
        format!("0x{QUOTE_SIGNATURE}"),
        QUOTE_SIGNATURE.to_ascii_uppercase(),
        format!("0x{}", QUOTE_SIGNATURE.to_ascii_uppercase()),
    ] {
        signed.signature = encoding;
        protocol::verify(protocol::QUOTE_DOMAIN, &signed, VECTOR_WALLET).unwrap();
    }
    for domain in [
        protocol::OFFER_DOMAIN,
        protocol::INTENT_DOMAIN,
        protocol::RECEIPT_DOMAIN,
        protocol::AUTH_DOMAIN,
        protocol::FEEDBACK_DOMAIN,
    ] {
        assert!(protocol::verify(domain, &signed, VECTOR_WALLET).is_err(), "{domain}");
    }
    assert!(protocol::verify(
        protocol::QUOTE_DOMAIN, &signed, &signed.payload.consumer_wallet
    ).is_err());
    let noncanonical_wallet = format!("0x{}", VECTOR_WALLET[2..].to_ascii_uppercase());
    assert!(protocol::verify(protocol::QUOTE_DOMAIN, &signed, &noncanonical_wallet).is_err());
    signed.payload.funding_wei = "1000000000000000001".into();
    assert!(protocol::verify(protocol::QUOTE_DOMAIN, &signed, VECTOR_WALLET).is_err());
}

#[test]
fn signer_uses_independently_hashed_personal_frames_without_pinning_nonce_choice() {
    use secp256k1::{ecdsa::Signature, Message, PublicKey, Secp256k1};
    let public = PublicKey::from_slice(&hex::decode(concat!(
        "0479be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
        "483ada7726a3c4655da4fbfc0e1108a8fd17b448a68554199c47d08ffb10d4b8"
    )).unwrap()).unwrap();
    let frame = protocol::signing_payload(protocol::QUOTE_DOMAIN, &signed_quote().payload).unwrap();
    for (message, expected_digest) in [
        (frame.as_slice(), QUOTE_PERSONAL_DIGEST),
        ("é💡".as_bytes(), "d2c062f4253c0f8a95492f49ccae5c559b67fc31673d72e24b46fbafbe722bce"),
    ] {
        let emitted = wallet::sign_message(&format!("{:064x}", 1), message).unwrap();
        assert_eq!(emitted.len(), 130);
        assert_eq!(emitted, emitted.to_ascii_lowercase());
        let bytes = hex::decode(emitted).unwrap();
        assert!(matches!(bytes[64], 27 | 28));
        let signature = Signature::from_compact(&bytes[..64]).unwrap();
        let mut normalized = signature;
        normalized.normalize_s();
        assert_eq!(signature, normalized);
        let digest: [u8; 32] = hex::decode(expected_digest).unwrap().try_into().unwrap();
        Secp256k1::verification_only()
            .verify_ecdsa(&Message::from_digest(digest), &signature, &public)
            .unwrap();
    }
}

#[test]
fn personal_signature_rejects_malformed_encodings_and_recovery_ids() {
    let frame = protocol::signing_payload(protocol::QUOTE_DOMAIN, &signed_quote().payload).unwrap();
    for invalid in [
        String::new(),
        QUOTE_SIGNATURE[..128].into(),
        format!("{QUOTE_SIGNATURE}00"),
        format!("0X{QUOTE_SIGNATURE}"),
        format!("gg{}", &QUOTE_SIGNATURE[2..]),
        format!("{}1b", "00".repeat(64)),
    ] {
        assert!(wallet::recover_signer(&frame, &invalid).is_err(), "{invalid}");
    }
    for recovery in [0, 1, 26, 29, 255] {
        let mut bytes = hex::decode(QUOTE_SIGNATURE).unwrap();
        bytes[64] = recovery;
        assert!(wallet::recover_signer(&frame, &hex::encode(bytes)).is_err(), "{recovery}");
    }
}

#[test]
fn content_hashes_and_payment_commitments_match_independent_vectors() {
    assert_eq!(
        protocol::hash_hex(b""),
        "0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
    );
    assert_eq!(
        protocol::hash_hex(b"abc"),
        "0x4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"
    );
    assert_eq!(protocol::offer_id(&offer()).unwrap(), OFFER_ID);
    assert_eq!(
        protocol::opening_data(&signed_quote().payload).unwrap(),
        hex::decode(OPENING_DATA).unwrap()
    );
    let expected_topup = hex::decode(
        "4348524c3200abababababababababababababababababababababababababababababababab"
    ).unwrap();
    for id in [format!("0x{}", "ab".repeat(32)), format!("0x{}", "AB".repeat(32))] {
        assert_eq!(protocol::topup_data(&id).unwrap(), expected_topup);
    }
}

#[test]
fn opening_commitment_binds_every_agreement_dimension() {
    let original = serde_json::to_value(signed_quote().payload).unwrap();
    let original_commitment = hex::decode(OPENING_DATA).unwrap();
    for (path, value) in [
        ("/id", json!(format!("0x{}", "05".repeat(32)))),
        ("/network/chain_id", json!(72)),
        ("/network/genesis_hash", json!(format!("0x{}", "ab".repeat(32)))),
        ("/network/deployment_id", json!("another-network-deployment")),
        ("/offer_id", json!(format!("0x{}", "06".repeat(32)))),
        ("/provider_wallet", json!(format!("0x{}", "44".repeat(20)))),
        ("/consumer_wallet", json!(format!("0x{}", "55".repeat(20)))),
        ("/model/id", json!("test/another-model")),
        ("/model/revision", json!("ef".repeat(20))),
        ("/model/tokenizer_revision", json!("ef".repeat(20))),
        ("/model/chat_template_sha256", json!("ef".repeat(32))),
        ("/model/quantization", json!("Q4_K_M")),
        ("/model/context_length", json!(8192)),
        ("/model/max_output_tokens", json!(512)),
        ("/model/capabilities", json!(["completions", "streaming"])),
        ("/price/input_wei_per_1k", json!("3")),
        ("/price/output_wei_per_1k", json!("3")),
        ("/funding_wei", json!("1000000000000000001")),
        ("/platform_wallet", json!(format!("0x{}", "66".repeat(20)))),
        ("/fee_bps", json!(51)),
        ("/max_concurrency", json!(2)),
        ("/nonce", json!(format!("0x{}", "07".repeat(32)))),
        ("/issued_at", json!(101)),
        ("/expires_at", json!(401)),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(path).unwrap() = value;
        let quote: Quote = serde_json::from_value(changed).unwrap();
        assert_ne!(protocol::opening_data(&quote).unwrap(), original_commitment, "{path}");
    }
}

#[test]
fn offer_identifier_binds_revision_prices_and_validity_not_signature_encoding() {
    let original = serde_json::to_value(offer()).unwrap();
    for (path, value) in [
        ("/revision", json!(2)),
        ("/price/input_wei_per_1k", json!("3")),
        ("/issued_at", json!(101)),
        ("/expires_at", json!(401)),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(path).unwrap() = value;
        let changed: InferenceOffer = serde_json::from_value(changed).unwrap();
        assert_ne!(protocol::offer_id(&changed).unwrap(), OFFER_ID, "{path}");
    }
    let mut signed = protocol::sign(protocol::OFFER_DOMAIN, offer(), &format!("{:064x}", 1)).unwrap();
    signed.signature = format!("0x{}", signed.signature.to_ascii_uppercase());
    protocol::verify(protocol::OFFER_DOMAIN, &signed, VECTOR_WALLET).unwrap();
    assert_eq!(protocol::offer_id(&signed.payload).unwrap(), OFFER_ID);
}

#[test]
fn topup_commitments_reject_malformed_account_identifiers() {
    for invalid in [
        String::new(),
        "ab".repeat(32),
        format!("0X{}", "ab".repeat(32)),
        format!("0x{}", "ab".repeat(31)),
        format!("0x{}", "ab".repeat(33)),
        format!("0x{}z", "ab".repeat(32)),
        format!("0x{}", "gg".repeat(32)),
    ] {
        assert!(protocol::topup_data(&invalid).is_err(), "{invalid}");
    }
}
