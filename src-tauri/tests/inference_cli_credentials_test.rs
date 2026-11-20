//! The `chiral credentials` commands as processes (account-credentials requirement 8).
//! `list`, `issue`, `rotate` and `revoke` print credential metadata as one JSON document on
//! stdout, or a single `chiral:` line on stderr when they fail, and never a secret. Only
//! `export` prints a secret, as one JSON document with the SDK base URL and the model.
//! Real credentials come from a native free opening against the production control and
//! inference routers on isolated loopback, in development mode; no funds, chain or engine.
//! Failure messages never print a secret: command output is shown only masked (hex runs of
//! 64 or more digits replaced), and other values only after they are shown to hold none.
#[path = "support/proposal.rs"]
mod proposal;

use chiral_network::{consumer::Consumer, contracts, inference_gateway, protocol, wallet};
use proposal::PreparedProvider;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path, process::Output};
use tokio::net::TcpListener;

/// `text` with every run of 64 or more hex digits (a secret's random part, or an
/// identifier) replaced, so that a failure message can show command output.
fn masked(text: &str) -> String {
    fn flush(out: &mut String, run: &mut String) {
        out.push_str(if run.len() >= 64 { "<hex>" } else { run.as_str() });
        run.clear();
    }
    let (mut out, mut run) = (String::new(), String::new());
    for character in text.chars() {
        if character.is_ascii_hexdigit() {
            run.push(character);
        } else {
            flush(&mut out, &mut run);
            out.push(character);
        }
    }
    flush(&mut out, &mut run);
    out
}

/// Whether `text` holds anything of the provider's secret shape: `chiral_` followed by 64
/// hex digits.
fn secret_shaped(text: &str) -> bool {
    text.match_indices("chiral_").any(|(at, prefix)| {
        let digits = &text.as_bytes()[at + prefix.len()..];
        digits.len() >= 64 && digits[..64].iter().all(u8::is_ascii_hexdigit)
    })
}

/// Whether `text` holds `secret`, whole or as its 64 random hex digits in either case.
fn holds(text: &str, secret: &str) -> bool {
    let random = secret.strip_prefix("chiral_").unwrap();
    text.contains(secret)
        || text.contains(random)
        || text.contains(&random.to_uppercase())
}

/// No secret, known or of the secret shape, on stdout or stderr.
fn assert_no_secret(output: &Output, label: &str, secrets: &[String]) {
    for (stream, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
        let text = String::from_utf8_lossy(bytes);
        assert!(!secret_shaped(&text), "{label}: {stream} holds a secret-shaped string");
        for secret in secrets {
            assert!(!holds(&text, secret), "{label}: {stream} holds a known secret");
        }
    }
}

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

/// A successful command: exit status 0, exactly one JSON document on stdout, and nothing
/// on stderr.
fn document(output: &Output, label: &str) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    assert!(output.status.success(), "{label}: exit {:?}: {}", output.status.code(), masked(&stderr));
    assert!(stderr.is_empty(), "{label}: stderr {}", masked(&stderr));
    serde_json::from_str(&stdout)
        .unwrap_or_else(|_| panic!("{label}: stdout is not one JSON document: {}", masked(&stdout)))
}

/// A failed command: a non-zero exit, nothing on stdout, and one `chiral:` line on stderr,
/// returned without its prefix.
fn failure(output: &Output, label: &str) -> String {
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    assert!(!output.status.success(), "{label} succeeded: {}", masked(&stdout));
    assert!(stdout.is_empty(), "{label}: stdout {}", masked(&stdout));
    assert_eq!(stderr.lines().count(), 1, "{label}: stderr {}", masked(&stderr));
    stderr
        .trim_end()
        .strip_prefix("chiral: ")
        .unwrap_or_else(|| panic!("{label}: stderr {}", masked(&stderr)))
        .to_owned()
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| name.to_string()).collect()
}

fn members(value: &Value) -> BTreeSet<String> {
    value.as_object().unwrap().keys().cloned().collect()
}

/// `chiral credentials <contract> export [--id <credential>]`: exactly one JSON document
/// of the five export members on stdout, and nothing on stderr.
async fn export(profile: &Path, contract: &str, credential: Option<&str>) -> (Value, Output) {
    let mut args = vec!["credentials", contract, "export"];
    if let Some(credential) = credential {
        args.extend(["--id", credential]);
    }
    let output = chiral(profile, &args).await;
    let exported = document(&output, "export");
    assert_eq!(
        members(&exported),
        names(&["api_key", "base_url", "contract_id", "credential_id", "model"])
    );
    let secret = exported["api_key"].as_str().unwrap();
    assert!(secret_shaped(secret) && secret.len() == 71, "export: api_key is not a secret");
    (exported, output)
}

/// `document[member]` is the string `expected`; a mismatch shows the value masked.
fn field(document: &Value, member: &str, expected: &str) {
    assert!(document[member] == expected, "{member}: {}", masked(&document[member].to_string()));
}

fn write_options(path: &Path, options: Value) -> String {
    std::fs::write(path, serde_json::to_vec(&options).unwrap()).unwrap();
    path.to_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credential_commands_print_metadata_and_only_export_prints_a_secret() {
    let directory = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = PreparedProvider::new(directory.path(), 1, "0", "0",
        listener.local_addr().unwrap(), admin.local_addr().unwrap());
    let app = contracts::router(provider.state.clone())
        .merge(inference_gateway::router(provider.state.clone()));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let data_dir = directory.path().join("consumer");
    let profile = directory.path().join("consumer.json");
    std::fs::write(&profile, serde_json::to_vec(&json!({"network": provider.state.config.network,
        "data_dir": data_dir, "catalog_url": null, "development": true})).unwrap()).unwrap();
    // The opening runs in process from the same profile; every credential command below
    // runs as its own `chiral` process.
    let scalar = format!("{:064x}", 71_001);
    let id = {
        let consumer = Consumer::load(&profile).unwrap();
        let import = data_dir.join("public-test-scalar.key");
        wallet::save_private_key(&import, &scalar).unwrap();
        consumer.wallet_import_file(&import).unwrap();
        let quote = consumer.quote(&provider.offer, "0").await.unwrap();
        assert_eq!(quote.payload.offer_id, provider.offer_id);
        assert_eq!(consumer.fund(&provider.offer, &quote).await.unwrap().status, "free");
        quote.payload.id
    };
    let ledger = provider.state.ledger.clone();
    // The account and credential a secret authenticates as at the provider, with its scopes.
    let authenticates = |secret: &str| {
        ledger.authenticate(secret, protocol::now().unwrap()).ok()
            .map(|context| (context.account.contract_id, context.credential_id, context.scopes))
    };
    let base_url = format!("{}/v1", provider.offer.payload.endpoint);
    let model = provider.offer.payload.model.id.clone();
    let metadata = names(&["contract_id", "expires_at", "id"]);
    let mut secrets: Vec<String> = Vec::new();
    let mut management: Vec<(&str, Output)> = Vec::new();
    let mut exports: Vec<(String, Output)> = Vec::new();
    let both = vec!["infer".to_string(), "read".to_string()];

    let output = chiral(&profile, &["credentials", &id, "list"]).await;
    let listed = document(&output, "list");
    management.push(("list", output));
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let root = listed[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(listed[0]["root"], true);

    // The export without an identifier is the native default, the first-opening root.
    let (exported, output) = export(&profile, &id, None).await;
    let root_secret = exported["api_key"].as_str().unwrap().to_owned();
    field(&exported, "credential_id", root.as_str());
    field(&exported, "contract_id", id.as_str());
    field(&exported, "base_url", base_url.as_str());
    field(&exported, "model", model.as_str());
    assert!(authenticates(&root_secret) == Some((id.clone(), root.clone(), both.clone())));
    secrets.push(root_secret.clone());
    exports.push((root_secret.clone(), output));

    // Issuance with an options file; the valid default is kept.
    let options = write_options(&directory.path().join("issue.json"), json!({
        "expires_at": protocol::now().unwrap() + 600, "spend_cap_wei": "900",
        "max_concurrency": 2, "scopes": ["infer", "read"]}));
    let output = chiral(&profile, &["credentials", &id, "issue", "--options", &options]).await;
    let issued = document(&output, "issue");
    management.push(("issue", output));
    assert_eq!(members(&issued), metadata);
    let issued_id = issued["id"].as_str().unwrap().to_owned();
    let (exported, output) = export(&profile, &id, Some(&issued_id)).await;
    let issued_secret = exported["api_key"].as_str().unwrap().to_owned();
    field(&exported, "credential_id", issued_id.as_str());
    assert!(authenticates(&issued_secret) == Some((id.clone(), issued_id.clone(), both.clone())));
    secrets.push(issued_secret.clone());
    exports.push((issued_secret.clone(), output));
    field(&export(&profile, &id, None).await.0, "credential_id", root.as_str());

    // Rotation with the CLI's default options.
    let output = chiral(&profile, &["credentials", &id, "rotate", &issued_id]).await;
    let rotated = document(&output, "rotate");
    management.push(("rotate", output));
    assert_eq!(members(&rotated), metadata);
    let rotated_id = rotated["id"].as_str().unwrap().to_owned();
    let (exported, output) = export(&profile, &id, Some(&rotated_id)).await;
    let rotated_secret = exported["api_key"].as_str().unwrap().to_owned();
    assert!(authenticates(&rotated_secret) == Some((id.clone(), rotated_id.clone(), both.clone())));
    assert!(authenticates(&issued_secret).is_none(), "the rotated credential still authenticates");
    secrets.push(rotated_secret.clone());
    exports.push((rotated_secret.clone(), output));

    // A read-only issuance, then its revocation.
    let options = write_options(&directory.path().join("read.json"), json!({
        "expires_at": protocol::now().unwrap() + 600, "spend_cap_wei": null,
        "max_concurrency": 1, "scopes": ["read"]}));
    let output = chiral(&profile, &["credentials", &id, "issue", "--options", &options]).await;
    let read_only = document(&output, "issue read-only");
    management.push(("issue read-only", output));
    assert_eq!(members(&read_only), metadata);
    let read_only_id = read_only["id"].as_str().unwrap().to_owned();
    let (exported, output) = export(&profile, &id, Some(&read_only_id)).await;
    let read_only_secret = exported["api_key"].as_str().unwrap().to_owned();
    assert!(authenticates(&read_only_secret)
        == Some((id.clone(), read_only_id.clone(), vec!["read".to_string()])));
    secrets.push(read_only_secret.clone());
    exports.push((read_only_secret.clone(), output));
    let output = chiral(&profile, &["credentials", &id, "revoke", &read_only_id]).await;
    let revoked = document(&output, "revoke");
    assert!(revoked == json!({"revoked": true}), "revoke: {}", masked(&revoked.to_string()));
    management.push(("revoke", output));
    assert!(authenticates(&read_only_secret).is_none(), "the revoked credential still authenticates");

    // Rotating the native default makes its replacement the default (requirement 9).
    let output = chiral(&profile, &["credentials", &id, "rotate", &root]).await;
    let replacement = document(&output, "rotate default");
    management.push(("rotate default", output));
    assert_eq!(members(&replacement), metadata);
    let replacement_id = replacement["id"].as_str().unwrap().to_owned();
    let (exported, output) = export(&profile, &id, None).await;
    let replacement_secret = exported["api_key"].as_str().unwrap().to_owned();
    field(&exported, "credential_id", replacement_id.as_str());
    assert!(authenticates(&replacement_secret)
        == Some((id.clone(), replacement_id.clone(), both.clone())));
    assert!(authenticates(&root_secret).is_none(), "the rotated default still authenticates");
    secrets.push(replacement_secret.clone());
    exports.push((replacement_secret.clone(), output));

    let output = chiral(&profile, &["credentials", &id, "list"]).await;
    let listed = document(&output, "list again");
    management.push(("list again", output));
    let states: BTreeSet<(String, bool)> = listed.as_array().unwrap().iter()
        .map(|entry| (entry["id"].as_str().unwrap().to_owned(), entry["revoked"].as_bool().unwrap()))
        .collect();
    assert!(states == BTreeSet::from([(root.clone(), true), (issued_id.clone(), true),
        (rotated_id.clone(), false), (read_only_id.clone(), true), (replacement_id.clone(), false)]),
        "list again: the credentials or their revocation states differ");
    for entry in listed.as_array().unwrap() {
        for member in ["api_key", "bearer", "secret", "token_hash"] {
            assert!(entry.get(member).is_none(), "a list entry carries {member}");
        }
    }

    // Failing runs, while the provider answers. None changes the stored account.
    let owner = wallet::address_from_private_key(&scalar).unwrap();
    let account_path = provider.state.config.network.scoped_dir(&data_dir).join("consumer")
        .join(&owner).join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))));
    let account = std::fs::read(&account_path).unwrap();
    let unknown = format!("0x{}", "ab".repeat(32));
    let positional = write_options(&directory.path().join("positional.json"),
        json!([protocol::now().unwrap() + 600, null, 1, ["read"]]));
    let mut failures = Vec::new();
    for (label, args) in [
        ("revoke unknown", vec!["credentials", &id, "revoke", &unknown]),
        ("rotate unknown", vec!["credentials", &id, "rotate", &unknown]),
        ("issue positional", vec!["credentials", &id, "issue", "--options", &positional]),
        ("export unknown", vec!["credentials", &id, "export", "--id", &unknown]),
    ] {
        let output = chiral(&profile, &args).await;
        assert_no_secret(&output, label, &secrets);
        failures.push((label, failure(&output, label)));
        management.push((label, output));
    }
    assert!(std::fs::read(&account_path).unwrap() == account, "a failing run changed the account");

    // Failing runs at a lost provider.
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    for (label, args) in [
        ("list at a lost provider", vec!["credentials", &id, "list"]),
        ("issue at a lost provider", vec!["credentials", &id, "issue"]),
        ("rotate at a lost provider", vec!["credentials", &id, "rotate", &rotated_id]),
        ("revoke at a lost provider", vec!["credentials", &id, "revoke", &rotated_id]),
    ] {
        let output = chiral(&profile, &args).await;
        assert_no_secret(&output, label, &secrets);
        failures.push((label, failure(&output, label)));
        management.push((label, output));
    }
    assert!(std::fs::read(&account_path).unwrap() == account, "a failing run changed the account");
    // Each failure was shown above to hold no secret; its fixed text is pinned here.
    let failures: Vec<(&str, &str)> = failures.iter()
        .map(|(label, error)| (*label, error.as_str())).collect();
    assert_eq!(failures, [
        ("revoke unknown", "invalid_credential: The credential is invalid, expired or revoked; rotate it with the wallet owner (HTTP 401)"),
        ("rotate unknown", "invalid_credential: The credential is invalid, expired or revoked; rotate it with the wallet owner (HTTP 401)"),
        ("issue positional", "invalid credential options"),
        ("export unknown", "credential is not stored locally"),
        ("list at a lost provider", "owner challenge unavailable"),
        ("issue at a lost provider", "owner challenge unavailable"),
        ("rotate at a lost provider", "owner challenge unavailable"),
        ("revoke at a lost provider", "owner challenge unavailable"),
    ]);

    // No management output, successful or failed, holds any secret; each export holds
    // exactly its own on stdout and none on stderr.
    assert_eq!(secrets.len(), 5);
    for (label, output) in &management {
        assert_no_secret(output, label, &secrets);
    }
    assert_eq!(exports.len(), 5);
    for (secret, output) in &exports {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.matches(secret.as_str()).count(), 1, "export: its secret is not printed once");
        for other in secrets.iter().filter(|other| *other != secret) {
            assert!(!holds(&stdout, other), "export: stdout holds another secret");
        }
        assert!(output.stderr.is_empty(), "export: stderr is not empty");
    }
}
