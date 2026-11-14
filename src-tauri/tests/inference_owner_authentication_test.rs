//! Provider owner-proof acceptance through the production authentication and ledger APIs.
//! Explicit time, isolated disk state and public fixture keys; no HTTP business operation or payment.
use axum::http::HeaderMap;
use chiral_network::{
    auth::{self, AuthPayload},
    billing_store::BillingStore,
    network::{ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    protocol::{self, NetworkIdentity, QUOTE_DOMAIN},
    wallet,
};
use std::{path::PathBuf, sync::Barrier, thread};

// Public test scalars, never funded or used outside these isolated ledgers.
const OWNER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000007";
const PROVIDER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000002";
const OPEN: &str = "/v1/contracts/open";
const BODY: &[u8] = br#"{"quote_id":"0x1111111111111111111111111111111111111111111111111111111111111111","raw_transaction":null}"#;
const RESERIALIZED_BODY: &[u8] = br#"{ "raw_transaction": null, "quote_id": "0x1111111111111111111111111111111111111111111111111111111111111111" }"#;
const READ: &str = "/v1/contracts/0x1111111111111111111111111111111111111111111111111111111111111111/requests";

fn payload() -> AuthPayload {
    AuthPayload {
        network: NetworkIdentity {
            chain_id: ESPACE_TESTNET_CHAIN_ID,
            genesis_hash: ESPACE_TESTNET_GENESIS.into(),
            deployment_id: "owner-authentication-tests".into(),
        },
        provider_wallet: wallet::address_from_private_key(PROVIDER_KEY).unwrap(),
        wallet: wallet::address_from_private_key(OWNER_KEY).unwrap(),
        method: "POST".into(), path: OPEN.into(), body_hash: protocol::hash_hex(BODY),
        nonce: format!("0x{}", "aa".repeat(32)), expires_at: 220,
    }
}

fn proof(payload: &AuthPayload, key: &str) -> HeaderMap {
    let signature = wallet::sign_message(key, &auth::challenge_payload(payload).unwrap()).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("x-chiral-owner", payload.wallet.parse().unwrap());
    headers.insert("x-chiral-nonce", payload.nonce.parse().unwrap());
    headers.insert("x-chiral-signature", signature.parse().unwrap());
    headers
}

struct Fixture {
    ledger: BillingStore,
    payload: AuthPayload,
    path: PathBuf,
    // Drop the connection before removing its directory, including on assertion failure.
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn new(payload: AuthPayload) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite");
        let ledger = BillingStore::open(&path, &payload.network).unwrap();
        ledger.bind_provider(&payload.provider_wallet).unwrap();
        ledger.create_challenge(&payload, 100).unwrap();
        Self { ledger, payload, path, _directory: directory }
    }

    fn authenticate(&self, headers: &HeaderMap, method: &str, path: &str, body: &[u8], now: u64)
        -> Result<String, String> {
        auth::authenticate_owner(&self.ledger, headers, &self.payload.network,
            &self.payload.provider_wallet, method, path, body, now)
    }

    fn assert_pending(&self) {
        assert_eq!(serde_json::to_value(self.ledger.challenge(&self.payload.nonce).unwrap()).unwrap(),
            serde_json::to_value(&self.payload).unwrap());
    }
}

#[test]
fn invalid_control_proofs_preserve_a_usable_challenge() {
    assert_eq!(serde_json::from_slice::<serde_json::Value>(BODY).unwrap(),
        serde_json::from_slice::<serde_json::Value>(RESERIALIZED_BODY).unwrap());
    for case in ["method", "reserialized-body", "malformed-signature", "wrong-signer",
        "wrong-domain", "wrong-nonce-signature", "wrong-expiry-signature", "unknown-nonce"] {
        let fixture = Fixture::new(payload());
        let valid = proof(&fixture.payload, OWNER_KEY);
        let mut headers = valid.clone();
        let mut method = "POST";
        let mut body = BODY;
        match case {
            "method" => method = "DELETE",
            "reserialized-body" => body = RESERIALIZED_BODY,
            "malformed-signature" => {
                headers.insert("x-chiral-signature", format!("0x{}", "gg".repeat(65)).parse().unwrap());
            }
            "wrong-signer" => headers = proof(&fixture.payload, PROVIDER_KEY),
            "wrong-domain" => {
                let bytes = protocol::signing_payload(QUOTE_DOMAIN, &fixture.payload).unwrap();
                let signature = wallet::sign_message(OWNER_KEY, &bytes).unwrap();
                headers.insert("x-chiral-signature", signature.parse().unwrap());
            }
            "wrong-nonce-signature" | "wrong-expiry-signature" | "unknown-nonce" => {
                let mut changed = fixture.payload.clone();
                if case == "wrong-expiry-signature" { changed.expires_at -= 1; }
                else { changed.nonce = format!("0x{}", "bb".repeat(32)); }
                let altered = proof(&changed, OWNER_KEY);
                if case == "unknown-nonce" {
                    // A valid owner signature does not authorize an unissued, unstored nonce.
                    headers = altered;
                } else {
                    headers.insert("x-chiral-signature", altered["x-chiral-signature"].clone());
                }
            }
            _ => unreachable!(),
        }
        assert!(fixture.authenticate(&headers, method, OPEN, body, 101).is_err(), "{case}");
        fixture.assert_pending();
        // A refusal must preserve usable authority, not merely a residual database row.
        let mut valid = valid;
        valid.insert("x-chiral-owner", format!("0x{}", fixture.payload.wallet[2..].to_uppercase())
            .parse().unwrap());
        assert_eq!(fixture.authenticate(&valid, "POST", OPEN, BODY, 102).unwrap(), fixture.payload.wallet);
        assert!(fixture.ledger.challenge(&fixture.payload.nonce).is_err());
    }
}

#[test]
fn owner_proofs_bind_literal_queries_and_empty_read_bodies() {
    let mut intended = payload();
    intended.method = "GET".into();
    intended.path = format!("{READ}?before=0200");
    intended.body_hash = protocol::hash_hex(b"");
    let fixture = Fixture::new(intended);
    let headers = proof(&fixture.payload, OWNER_KEY);
    for target in [READ.to_string(), format!("{READ}?before=200"),
        format!("{READ}?before=%30%32%30%30"), format!("{READ}?before=0201")] {
        assert!(fixture.authenticate(&headers, "GET", &target, b"", 101).is_err(), "{target}");
        fixture.assert_pending();
    }
    assert!(fixture.authenticate(&headers, "GET", &fixture.payload.path, b"{}", 101).is_err());
    fixture.assert_pending();
    assert_eq!(fixture.authenticate(&headers, "GET", &fixture.payload.path, b"", 102).unwrap(),
        fixture.payload.wallet);
    assert!(fixture.ledger.challenge(&fixture.payload.nonce).is_err());
}

#[test]
fn owner_authority_ends_at_the_exact_expiry_second() {
    // Independent ledgers avoid resurrecting a proof by moving one fixture's clock backwards.
    for now in [219, 220, 221] {
        let fixture = Fixture::new(payload());
        let headers = proof(&fixture.payload, OWNER_KEY);
        let result = fixture.authenticate(&headers, "POST", OPEN, BODY, now);
        if now == 219 {
            assert_eq!(result.unwrap(), fixture.payload.wallet);
            assert!(fixture.ledger.challenge(&fixture.payload.nonce).is_err());
        } else {
            assert!(result.is_err(), "authority must end at 220, attempted at {now}");
        }
    }
}

#[test]
fn real_proof_has_one_connection_winner_and_stays_consumed_after_restart() {
    let Fixture { ledger, payload, path, _directory } = Fixture::new(payload());
    let headers = proof(&payload, OWNER_KEY);
    drop(ledger);
    // Reopen before authenticating, then race real proofs on independent connections.
    let first = BillingStore::open(&path, &payload.network).unwrap();
    let second = BillingStore::open(&path, &payload.network).unwrap();
    let barrier = Barrier::new(2);
    let attempt = |store: BillingStore| {
        barrier.wait();
        auth::authenticate_owner(&store, &headers, &payload.network,
            &payload.provider_wallet, "POST", OPEN, BODY, 101)
    };
    let results = thread::scope(|scope| {
        let first = scope.spawn(|| attempt(first));
        let second = scope.spawn(|| attempt(second));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    for owner in results.into_iter().flatten() { assert_eq!(owner, payload.wallet); }
    let reopened = BillingStore::open(&path, &payload.network).unwrap();
    assert!(reopened.challenge(&payload.nonce).is_err());
    assert!(auth::authenticate_owner(&reopened, &headers, &payload.network,
        &payload.provider_wallet, "POST", OPEN, BODY, 102).is_err());
}

#[test]
fn conditional_consumption_compares_every_validated_payload_field() {
    for field in ["chain", "genesis", "deployment", "provider", "owner", "method", "path",
        "body", "nonce", "expiry"] {
        let fixture = Fixture::new(payload());
        let mut expected = fixture.payload.clone();
        match field {
            "chain" => expected.network.chain_id += 1,
            "genesis" => expected.network.genesis_hash = format!("0x{}", "ee".repeat(32)),
            "deployment" => expected.network.deployment_id.push_str("-other"),
            "provider" => expected.provider_wallet = expected.wallet.clone(),
            "owner" => expected.wallet = expected.provider_wallet.clone(),
            "method" => expected.method = "DELETE".into(),
            "path" => expected.path.push_str("?different=1"),
            "body" => expected.body_hash = protocol::hash_hex(b"{}"),
            "nonce" => expected.nonce = format!("0x{}", "bb".repeat(32)),
            "expiry" => expected.expires_at -= 1,
            _ => unreachable!(),
        }
        assert!(fixture.ledger.consume_challenge(&fixture.payload.nonce, &expected, 101).is_err(),
            "must compare {field}, not only nonce and freshness");
        fixture.assert_pending();
        let headers = proof(&fixture.payload, OWNER_KEY);
        assert_eq!(fixture.authenticate(&headers, "POST", OPEN, BODY, 102).unwrap(), fixture.payload.wallet);
    }
}

#[test]
fn stale_validated_proof_cannot_consume_a_replacement_payload() {
    let fixture = Fixture::new(payload());
    let headers = proof(&fixture.payload, OWNER_KEY);
    let validated = fixture.ledger.challenge(&fixture.payload.nonce).unwrap();
    assert_eq!(wallet::recover_signer(&auth::challenge_payload(&validated).unwrap(),
        headers["x-chiral-signature"].to_str().unwrap()).unwrap(), validated.wallet);
    let other = BillingStore::open(&fixture.path, &fixture.payload.network).unwrap();
    other.consume_challenge(&validated.nonce, &validated, 101).unwrap();
    // Deliberately reuse an absent nonce to exercise the consume-time comparison, not RNG collision odds.
    let mut replacement = validated.clone();
    replacement.method = "GET".into();
    replacement.path = format!("{READ}?before=200");
    replacement.body_hash = protocol::hash_hex(b"");
    replacement.expires_at = 222;
    other.create_challenge(&replacement, 102).unwrap();
    assert!(fixture.ledger.consume_challenge(&validated.nonce, &validated, 102).is_err());
    assert!(fixture.authenticate(&headers, "POST", OPEN, BODY, 102).is_err());
    assert_eq!(serde_json::to_value(fixture.ledger.challenge(&replacement.nonce).unwrap()).unwrap(),
        serde_json::to_value(&replacement).unwrap());
    let current_proof = proof(&replacement, OWNER_KEY);
    assert_eq!(fixture.authenticate(&current_proof, "GET", &replacement.path, b"", 103).unwrap(),
        replacement.wallet);
    assert!(fixture.ledger.challenge(&replacement.nonce).is_err());
}
