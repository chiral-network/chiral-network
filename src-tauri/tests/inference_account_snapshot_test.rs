//! Decoder contracts for specify-inference-account-snapshot tasks 2.1–2.2.
//! Synthetic provider statements, not chain, engine or native HTTP acceptance.
use chiral_network::protocol::{self, AccountView, Signed, TokenUsage, UsageReceipt};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

const PROVIDER: &str = "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf";
const ACCOUNT_FIELDS: [&str; 17] = [
    "contract_id", "network", "provider_wallet", "consumer_wallet", "model", "price", "status",
    "funded_wei", "fee_wei", "spent_wei", "reserved_wei", "available_wei", "reversed_wei",
    "provider_loss_wei", "usage", "waived_wei", "max_concurrency",
];

fn account_value() -> Value {
    json!({
        "contract_id": format!("0x{}", "01".repeat(32)),
        "network": {
            "chain_id": 71,
            "genesis_hash": "0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804",
            "deployment_id": "account-snapshot-test"
        },
        "provider_wallet": PROVIDER,
        "consumer_wallet": format!("0x{}", "22".repeat(20)),
        "model": {
            "id": "test/model",
            "revision": "ab".repeat(20),
            "tokenizer_revision": "ab".repeat(20),
            "chat_template_sha256": "cd".repeat(32),
            "quantization": null,
            "context_length": 4096,
            "max_output_tokens": 256,
            "capabilities": ["completions"],
            "serving": null
        },
        "price": {"input_wei_per_1k": "0", "output_wei_per_1k": "0"},
        "status": "free",
        "funded_wei": "0", "fee_wei": "0", "spent_wei": "0", "reserved_wei": "0",
        "available_wei": "0", "reversed_wei": "0", "provider_loss_wei": "0", "waived_wei": "0",
        "usage": {"prompt_tokens": 9007199254740993_u64, "completion_tokens": 9007199254740995_u64},
        "max_concurrency": 1
    })
}

fn rejects<T: DeserializeOwned>(value: Value) {
    let wire = serde_json::to_string(&value).unwrap();
    assert!(serde_json::from_str::<T>(&wire).is_err(), "accepted bytes: {wire}");
    assert!(serde_json::from_value::<T>(value).is_err(), "accepted value: {wire}");
}

fn reversed_object(value: &Value) -> String {
    let members = value.as_object().unwrap().iter().rev()
        .map(|(key, value)| format!("{}:{value}", serde_json::to_string(key).unwrap()))
        .collect::<Vec<_>>().join(",");
    format!("{{{members}}}")
}

fn positional_account(value: &Value) -> Value {
    Value::Array(ACCOUNT_FIELDS.iter().map(|field| value[*field].clone()).collect())
}

fn duplicate_member(value: &Value, field: &str) -> String {
    let encoded = serde_json::to_string(value).unwrap();
    format!("{{{}:{},{}", serde_json::to_string(field).unwrap(), value[field], &encoded[1..])
}

#[test]
fn named_account_snapshots_preserve_every_field_and_canonical_bytes() {
    let expected = account_value();
    // The reference is the declared wire object, not an AccountView round-trip.
    let canonical = protocol::canonical_json(&expected).unwrap();
    for wire in [serde_json::to_string(&expected).unwrap(), reversed_object(&expected)] {
        let decoded: AccountView = serde_json::from_str(&wire).unwrap();
        assert_eq!(protocol::canonical_json(&decoded).unwrap(), canonical);
        let from_value: AccountView = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(protocol::canonical_json(&from_value).unwrap(), canonical);
    }
    // Nullable model members retain their existing optional-field semantics.
    let mut omitted = expected;
    omitted["model"].as_object_mut().unwrap().remove("quantization");
    omitted["model"].as_object_mut().unwrap().remove("serving");
    let decoded: AccountView = serde_json::from_value(omitted).unwrap();
    assert_eq!(protocol::canonical_json(&decoded).unwrap(), canonical);
}

#[test]
fn account_snapshots_reject_positional_arrays_and_nonobjects() {
    for value in [positional_account(&account_value()), json!([]), Value::Null, json!(true), json!(1), json!("account")] {
        rejects::<AccountView>(value);
    }
}

#[test]
fn account_snapshots_require_all_named_members_and_refuse_additions() {
    let expected = account_value();
    let mut extra = expected.clone();
    extra["unverified_label"] = json!("not part of the provider statement");
    rejects::<AccountView>(extra);
    for field in ACCOUNT_FIELDS {
        let mut missing = expected.clone();
        missing.as_object_mut().unwrap().remove(field);
        rejects::<AccountView>(missing);
        let wrong_types = match field {
            "network" | "model" | "price" | "usage" => vec![Value::Null, json!(false), json!(1), json!("object"), json!([])],
            "max_concurrency" => vec![Value::Null, json!(false), json!(-1), json!(1.0), json!("1"), json!([]), json!({}), json!(4294967296_u64)],
            _ => vec![Value::Null, json!(false), json!(1), json!(1.0), json!([]), json!({})],
        };
        for wrong in wrong_types {
            let mut invalid = expected.clone();
            invalid[field] = wrong;
            rejects::<AccountView>(invalid);
        }
    }
}

#[test]
fn accounting_usage_rejects_positional_arrays_and_nonobjects() {
    for value in [json!([1, 2]), json!([u64::MAX, u64::MAX]), json!([]), Value::Null, json!(false), json!(1), json!("usage")] {
        rejects::<TokenUsage>(value.clone());
        let mut account = account_value();
        account["usage"] = value;
        rejects::<AccountView>(account);
    }
}

#[test]
fn accounting_usage_requires_two_exact_unsigned_counters_without_defaults() {
    for (prompt, completion) in [
        (0, 0), (1, 2), (0, u64::MAX), (u64::MAX, 0),
        (9007199254740991, 9007199254740992),
        (9007199254740993, 9007199254740995), (u64::MAX, u64::MAX),
    ] {
        let canonical = format!("{{\"completion_tokens\":{completion},\"prompt_tokens\":{prompt}}}");
        for wire in [canonical.clone(), format!("{{\"prompt_tokens\":{prompt},\"completion_tokens\":{completion}}}")] {
            let decoded: TokenUsage = serde_json::from_str(&wire).unwrap();
            assert_eq!((decoded.prompt_tokens, decoded.completion_tokens), (prompt, completion));
            assert_eq!(protocol::canonical_json(&decoded).unwrap(), canonical.as_bytes());
            let value = serde_json::from_str(&wire).unwrap();
            let decoded: TokenUsage = serde_json::from_value(value).unwrap();
            assert_eq!(protocol::canonical_json(&decoded).unwrap(), canonical.as_bytes());
        }
    }
    let usage = json!({"prompt_tokens": 1, "completion_tokens": 2});
    for field in ["prompt_tokens", "completion_tokens"] {
        let mut missing = usage.clone();
        missing.as_object_mut().unwrap().remove(field);
        rejects::<TokenUsage>(missing);
        for invalid in ["null", "true", "false", "\"1\"", "-1", "-0", "1.0", "1e0", "1.5", "18446744073709551616", "[]", "{}"] {
            let (prompt, completion) = if field == "prompt_tokens" { (invalid, "2") } else { ("1", invalid) };
            let raw = format!("{{\"prompt_tokens\":{prompt},\"completion_tokens\":{completion}}}");
            assert!(serde_json::from_str::<TokenUsage>(&raw).is_err(), "accepted {raw}");
            let value: Value = serde_json::from_str(&raw).unwrap();
            rejects::<TokenUsage>(value.clone());
            let mut account = account_value();
            account["usage"] = value;
            rejects::<AccountView>(account);
        }
    }
    // OpenAI response usage is a different schema, not an accounting alias.
    let mut extra = usage;
    extra["total_tokens"] = json!(3);
    rejects::<TokenUsage>(extra);
}

#[test]
fn account_and_usage_decoders_reject_raw_duplicate_members() {
    let account = account_value();
    for field in ACCOUNT_FIELDS {
        let raw = duplicate_member(&account, field);
        assert!(serde_json::from_str::<AccountView>(&raw).is_err(), "duplicate {field}");
    }
    for field in ["prompt_tokens", "completion_tokens"] {
        let raw = duplicate_member(&account["usage"], field);
        assert!(serde_json::from_str::<TokenUsage>(&raw).is_err(), "duplicate {field}");
        let usage_member = format!("\"usage\":{}", account["usage"]);
        let nested = serde_json::to_string(&account).unwrap().replacen(&usage_member, &format!("\"usage\":{raw}"), 1);
        assert!(serde_json::from_str::<AccountView>(&nested).is_err(), "nested duplicate {field}");
    }
}

#[test]
fn receipts_preserve_named_claims_but_reject_account_and_usage_projections() {
    let account: AccountView = serde_json::from_value(account_value()).unwrap();
    // Public scalar one, never an operator or funded wallet key.
    let signed = protocol::sign(protocol::RECEIPT_DOMAIN, UsageReceipt {
        network: account.network.clone(), provider_wallet: PROVIDER.into(), account,
        requests: Vec::new(), sequence: 37, issued_at: 100,
    }, &format!("{:064x}", 1)).unwrap();
    let canonical = protocol::canonical_json(&signed.payload).unwrap();
    let encoded = serde_json::to_string(&signed).unwrap();
    let account_member = format!("\"account\":{}", serde_json::to_string(&signed.payload.account).unwrap());
    for account in [serde_json::to_string(&account_value()).unwrap(), reversed_object(&account_value())] {
        let wire = encoded.replacen(&account_member, &format!("\"account\":{account}"), 1);
        let decoded: Signed<UsageReceipt> = serde_json::from_str(&wire).unwrap();
        protocol::verify(protocol::RECEIPT_DOMAIN, &decoded, PROVIDER).unwrap();
        assert_eq!(protocol::canonical_json(&decoded.payload).unwrap(), canonical);
        let decoded: Signed<UsageReceipt> = serde_json::from_value(serde_json::from_str(&wire).unwrap()).unwrap();
        protocol::verify(protocol::RECEIPT_DOMAIN, &decoded, PROVIDER).unwrap();
        assert_eq!(protocol::canonical_json(&decoded.payload).unwrap(), canonical);
    }
    let mut extra = account_value();
    extra["unverified_label"] = json!("not covered by this signature");
    let mut positional_usage = account_value();
    positional_usage["usage"] = json!([9007199254740993_u64, 9007199254740995_u64]);
    for account in [positional_account(&account_value()), extra, positional_usage] {
        // The existing signature is unchanged. Refuse projection before verification.
        let wire = encoded.replacen(&account_member, &format!("\"account\":{account}"), 1);
        assert!(serde_json::from_str::<Signed<UsageReceipt>>(&wire).is_err());
        rejects::<Signed<UsageReceipt>>(serde_json::from_str(&wire).unwrap());
    }
}
