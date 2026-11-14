//! Deterministic contracts for specify-inference-wire-foundation task 2.1.
//! No live RPC, wall-clock-dependent offers, or production credentials.
use chiral_network::{
    billing_store::BillingStore,
    network::NetworkConfig,
    offer_registry::validate_money,
    protocol::{self, ModelSpec, NetworkIdentity, PriceSchedule, Quote, Signed},
    wallet,
};
use serde_json::{json, Value};

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
