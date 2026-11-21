//! inference-account-snapshot task 2.5: the `chiral` CLI and the native consumer's protected
//! files present and export account snapshots, signed receipts and request records exactly at
//! 2^53 + 1, the 2^64 - 1 counter maximum and u128::MAX wei. A genuine free opening against the
//! production control and inference routers on isolated loopback, in development mode, comes
//! first. Afterwards, only successfully authorized production reads are replaced: the account,
//! the receipt (re-signed with the fixture provider key) and the request page carry
//! response-only provider claims for receiver validation, not ledger transitions, funding or
//! engine evidence. No chain, engine or payment is involved.
#[path = "support/proposal.rs"]
mod proposal;

use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{header, Method, StatusCode},
    middleware::{from_fn, Next},
    response::Response,
};
use chiral_network::{
    billing_store::RequestView,
    consumer::Consumer,
    contracts, inference_gateway,
    protocol::{self, Signed, UsageReceipt, RECEIPT_DOMAIN},
    wallet,
};
use proposal::PreparedProvider;
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Output,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;

const TWO_53_PLUS_1: u64 = (1 << 53) + 1;

async fn chiral(profile: &Path, args: &[&str]) -> Output {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_chiral"))
        .env_remove("CHIRAL_CONSUMER_CONFIG")
        .env_remove("CHIRAL_POLICY_PUBLIC_KEY")
        .arg("--config")
        .arg(profile)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .unwrap()
}

/// A successful command: exit status 0, nothing on stderr, and one JSON document on stdout,
/// returned with its raw text.
fn document(output: &Output, label: &str) -> (Value, String) {
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    assert!(output.status.success(), "{label}: exit {:?}: {stderr}", output.status.code());
    assert!(stderr.is_empty(), "{label}: stderr {stderr}");
    let parsed = serde_json::from_str(&stdout)
        .unwrap_or_else(|_| panic!("{label}: stdout is not one JSON document"));
    (parsed, stdout)
}

/// Response-only claims installed after the genuine opening, keyed by the exact route.
#[derive(Clone, Default)]
struct Claims {
    account: Option<(String, Vec<u8>)>,
    /// Route, account, request page and the claimed receipt sequence.
    receipt: Option<(String, Value, Vec<RequestView>, u64)>,
    requests: Option<(String, Vec<u8>)>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_and_native_files_keep_large_counters_wei_and_receipt_watermarks_exact() {
    let directory = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = PreparedProvider::new(directory.path(), 25, "0", "0",
        listener.local_addr().unwrap(), admin.local_addr().unwrap());
    let claims = Arc::new(Mutex::new(Claims::default()));
    let app = contracts::router(provider.state.clone())
        .merge(inference_gateway::router(provider.state.clone()))
        .layer(from_fn({
            let claims = claims.clone();
            let signer = provider.state.private_key.clone();
            let provider_wallet = provider.state.wallet.clone();
            move |request: Request, next: Next| {
                let read = (request.method() == Method::GET).then(|| request.uri().path().to_owned());
                let claims = claims.lock().unwrap().clone();
                let signer = signer.clone();
                let provider_wallet = provider_wallet.clone();
                async move {
                    let response = next.run(request).await;
                    let Some(path) = read else { return response };
                    // Replace only a successfully authorized production read.
                    if response.status() != StatusCode::OK {
                        return response;
                    }
                    let (mut parts, body) = response.into_parts();
                    let genuine = to_bytes(body, 1 << 20).await.unwrap().to_vec();
                    let bytes = match (&claims.account, &claims.receipt, &claims.requests) {
                        (Some((route, bytes)), _, _) if *route == path => bytes.clone(),
                        (_, Some((route, account, requests, sequence)), _) if *route == path => {
                            let mut signed: Signed<UsageReceipt> = serde_json::from_slice(&genuine).unwrap();
                            protocol::verify(RECEIPT_DOMAIN, &signed, &provider_wallet).unwrap();
                            signed.payload.account = serde_json::from_value(account.clone()).unwrap();
                            signed.payload.requests = requests.clone();
                            signed.payload.sequence = *sequence;
                            serde_json::to_vec(&protocol::sign(RECEIPT_DOMAIN, signed.payload, &signer).unwrap()).unwrap()
                        }
                        (_, _, Some((route, bytes))) if *route == path => bytes.clone(),
                        _ => genuine,
                    };
                    parts.headers.remove(header::CONTENT_LENGTH);
                    Response::from_parts(parts, Body::from(bytes))
                }
            }
        }));
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).with_graceful_shutdown(async move { stopped.await.ok(); })
            .await.unwrap()
    });
    let data_dir = directory.path().join("consumer");
    let profile = directory.path().join("consumer.json");
    std::fs::write(&profile, serde_json::to_vec(&json!({"network": provider.state.config.network,
        "data_dir": data_dir, "catalog_url": null, "development": true})).unwrap()).unwrap();
    let scalar = format!("{:064x}", 71_025);
    let (opened, root_secret) = {
        let consumer = Consumer::load(&profile).unwrap();
        let import = data_dir.join("public-test-scalar.key");
        wallet::save_private_key(&import, &scalar).unwrap();
        consumer.wallet_import_file(&import).unwrap();
        let quote = consumer.quote(&provider.offer, "0").await.unwrap();
        assert_eq!(quote.payload.offer_id, provider.offer_id);
        let opened = consumer.fund(&provider.offer, &quote).await.unwrap();
        assert_eq!(opened.status, "free");
        let secret = consumer.credential_export(&quote.payload.id, None).unwrap()["api_key"]
            .as_str().unwrap().to_owned();
        (opened, secret)
    };
    let id = opened.contract_id.clone();
    let owner = wallet::address_from_private_key(&scalar).unwrap();
    let account_path = provider.state.config.network.scoped_dir(&data_dir).join("consumer")
        .join(&owner).join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))));
    let saved = || serde_json::from_slice::<Value>(&std::fs::read(&account_path).unwrap()).unwrap();
    assert_eq!(saved()["receipt_sequence"], 0);

    // The original terms with an extended balance identity totalling 2 * u128::MAX on each side,
    // the largest counters and an unfamiliar status the receiver keeps as stated.
    let max = u128::MAX.to_string();
    let mut wide = serde_json::to_value(&opened).unwrap();
    wide["status"] = json!("provider_review");
    for (field, amount) in [("funded_wei", max.clone()), ("provider_loss_wei", max.clone()),
        ("available_wei", max.clone()), ("waived_wei", max.clone()), ("fee_wei", "5".into()),
        ("reversed_wei", "7".into()), ("spent_wei", (u128::MAX - 15).to_string()), ("reserved_wei", "3".into())] {
        wide[field] = json!(amount);
    }
    wide["usage"] = json!({"prompt_tokens": u64::MAX, "completion_tokens": TWO_53_PLUS_1});
    let record = json!({
        "id": "request-exact", "request_id": "chatcmpl-exact", "contract_id": id,
        "credential_id": "credential-exact", "status": "settled",
        "usage": {"prompt_tokens": (1u64 << 53) - 1, "completion_tokens": u64::MAX},
        "cost_wei": "0", "max_cost_wei": max, "provider_loss_wei": max,
        "prompt_tokens_reserved": TWO_53_PLUS_1, "max_output_tokens": u64::MAX,
        "created_at": protocol::now().unwrap(), "settled_at": 1u64 << 53,
        "performance": {"evidence_sha256": "ab".repeat(32), "policy": "unqualified",
            "eligibility": "not_applicable", "ttft_us": u64::MAX, "decode_us": TWO_53_PLUS_1,
            "elapsed_us": (1u64 << 53) - 1, "output_tokens": u64::MAX, "attained": null, "remedy": "none"},
        "waived_wei": max,
    });
    let requests: Vec<RequestView> = vec![serde_json::from_value(record.clone()).unwrap()];
    // The typed request record reserializes to exactly the claimed integers.
    assert_eq!(serde_json::to_value(&requests).unwrap(), json!([record]));
    let mut outputs = Vec::new();

    // `chiral account`: native acceptance (original terms, U256 balance identity) and save, then
    // exact JSON integers and decimal wei on stdout and in the protected account file.
    claims.lock().unwrap().account = Some((format!("/v1/contracts/{id}"), serde_json::to_vec(&wide).unwrap()));
    let output = chiral(&profile, &["account", &id]).await;
    let (account, stdout) = document(&output, "account");
    assert_eq!(account, wide);
    for exact in [r#""prompt_tokens": 18446744073709551615"#, r#""completion_tokens": 9007199254740993"#,
        r#""funded_wei": "340282366920938463463374607431768211455""#, r#""status": "provider_review""#] {
        assert!(stdout.contains(exact), "account stdout lacks {exact}");
    }
    outputs.push(output);
    assert_eq!(saved()["account"], wide);
    let stored = String::from_utf8(std::fs::read(&account_path).unwrap()).unwrap();
    assert!(stored.contains(r#""usage":{"prompt_tokens":18446744073709551615,"completion_tokens":9007199254740993}"#));

    // `chiral receipt` at sequence 2^53 + 1 first. A watermark or export kept at double
    // precision would hold 2^53 here; at 2^64 - 1 alone an f64 round trip is invisible, because
    // Rust's float-to-integer cast saturates back to u64::MAX.
    let receipt_route = format!("/v1/contracts/{id}/receipt");
    claims.lock().unwrap().receipt = Some((receipt_route.clone(), wide.clone(), requests.clone(), TWO_53_PLUS_1));
    let output = chiral(&profile, &["receipt", &id]).await;
    let (receipt, stdout) = document(&output, "receipt at 2^53 + 1");
    assert_eq!(receipt["payload"]["sequence"], json!(TWO_53_PLUS_1));
    let exported: Signed<UsageReceipt> = serde_json::from_value(receipt).unwrap();
    protocol::verify(RECEIPT_DOMAIN, &exported, &provider.state.wallet).unwrap();
    assert!(stdout.contains(r#""sequence": 9007199254740993"#), "receipt stdout lacks the exact sequence");
    outputs.push(output);
    assert_eq!(saved()["receipt_sequence"], json!(TWO_53_PLUS_1));
    assert!(String::from_utf8(std::fs::read(&account_path).unwrap()).unwrap()
        .contains(r#""receipt_sequence":9007199254740993"#));

    // `chiral receipt`: the authentically re-signed statement is verified natively, exported
    // byte-for-value and still verifies; the native receipt watermark is exactly 2^64 - 1.
    claims.lock().unwrap().receipt = Some((receipt_route, wide.clone(), requests.clone(), u64::MAX));
    let output = chiral(&profile, &["receipt", &id]).await;
    let (receipt, stdout) = document(&output, "receipt");
    assert_eq!(receipt["payload"]["account"], wide);
    assert_eq!(receipt["payload"]["requests"], json!([record]));
    assert_eq!(receipt["payload"]["sequence"], json!(u64::MAX));
    let exported: Signed<UsageReceipt> = serde_json::from_value(receipt).unwrap();
    protocol::verify(RECEIPT_DOMAIN, &exported, &provider.state.wallet).unwrap();
    for exact in [r#""sequence": 18446744073709551615"#, r#""ttft_us": 18446744073709551615"#,
        r#""settled_at": 9007199254740992"#, r#""elapsed_us": 9007199254740991"#] {
        assert!(stdout.contains(exact), "receipt stdout lacks {exact}");
    }
    outputs.push(output);
    assert_eq!(saved()["receipt_sequence"], json!(u64::MAX));
    assert!(String::from_utf8(std::fs::read(&account_path).unwrap()).unwrap()
        .contains(r#""receipt_sequence":18446744073709551615"#));

    // `chiral usage`: the request page passes through with its exact integers.
    claims.lock().unwrap().requests = Some((format!("/v1/contracts/{id}/requests"),
        serde_json::to_vec(&json!([record])).unwrap()));
    let output = chiral(&profile, &["usage", &id]).await;
    let (page, stdout) = document(&output, "usage");
    assert_eq!(page, json!([record]));
    assert!(stdout.contains(r#""max_output_tokens": 18446744073709551615"#));
    outputs.push(output);

    // `chiral accounts` reads only the protected native files, with the provider gone.
    stop.send(()).unwrap();
    server.await.unwrap();
    let output = chiral(&profile, &["accounts"]).await;
    assert_eq!(document(&output, "accounts").0, json!([wide]));
    outputs.push(output);

    // The claims were responses only: the provider ledger keeps the genuine free account.
    assert_eq!(serde_json::to_value(provider.state.ledger.account(&id).unwrap()).unwrap(),
        serde_json::to_value(&opened).unwrap());

    // No presentation or export of these statements carries the bearer or the wallet key.
    for output in &outputs {
        for stream in [&output.stdout, &output.stderr] {
            let text = String::from_utf8_lossy(stream);
            assert!(!text.contains(&root_secret) && !text.contains(&scalar), "a statement holds a secret");
        }
    }
}
