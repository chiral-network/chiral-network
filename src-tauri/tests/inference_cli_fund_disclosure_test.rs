//! The `chiral fund` command discloses residual registration risk on stderr before any work.
//! The documents are missing, so no wallet, quote, provider or RPC is ever used: the explicit
//! temporary profile only lets the consumer load, and the loopback RPC is never contacted.
use chiral_network::network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS};
use chiral_network::protocol::NetworkIdentity;
use rusqlite::Connection;
use serde_json::json;
use std::process::Command;

#[test]
fn fund_discloses_registration_risk_on_stderr_and_keeps_stdout_machine_readable() {
    let directory = tempfile::tempdir().unwrap();
    let network = NetworkConfig {
        identity: NetworkIdentity {
            chain_id: ESPACE_TESTNET_CHAIN_ID,
            genesis_hash: ESPACE_TESTNET_GENESIS.into(),
            deployment_id: "cli-fund-disclosure".into(),
        },
        rpc_url: "http://127.0.0.1:9".into(),
        rpc_bearer_file: None,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    };
    let data_dir = directory.path().join("data");
    let profile = directory.path().join("consumer.json");
    std::fs::write(
        &profile,
        serde_json::to_vec(&json!({"network":network,"data_dir":data_dir,"catalog_url":null})).unwrap(),
    )
    .unwrap();
    let missing = directory.path().join("missing.json");
    let output = Command::new(env!("CARGO_BIN_EXE_chiral"))
        .env_remove("CHIRAL_CONSUMER_CONFIG")
        .arg("--config")
        .arg(&profile)
        .args(["fund", "--offer"])
        .arg(&missing)
        .arg("--quote")
        .arg(&missing)
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!output.status.success(), "{stderr}");
    assert!(output.stdout.is_empty(), "stdout must carry only machine-readable JSON");

    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    let disclosure = lines[0];
    for required in [
        "Funding is provider-specific and non-refundable.",
        "The client broadcasts only after a durable provider acknowledgement",
        "the provider receives independently broadcastable signed bytes",
        "could broadcast them before or without acknowledging",
        "this ordering is not cryptographic escrow",
        "An HTTP error, timeout or missing reply does not prove that no money moved",
        "inspect pending and resume CONTRACT_ID instead of funding again",
        "cancel-funding costs gas and can lose the race",
    ] {
        assert!(disclosure.contains(required), "missing {required:?} in {disclosure}");
    }
    // Only the client's own broadcast may be described as following the acknowledgement.
    for sentence in disclosure.split_inclusive(['.', ';', ':']) {
        if sentence.contains("only after") {
            assert!(sentence.contains("client"), "overstated ordering: {sentence}");
        }
    }
    assert_eq!(lines[1], "chiral: cannot read JSON input file");

    // The disclosure precedes any preparation: the wallet journal holds no transaction.
    let journal = network
        .scoped_dir(&data_dir)
        .join("wallet-journal/transactions.sqlite3");
    let connection = Connection::open(journal).unwrap();
    let transactions: i64 = connection
        .query_row("SELECT COUNT(*) FROM transactions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(transactions, 0);
}
