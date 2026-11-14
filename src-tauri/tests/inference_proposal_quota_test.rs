//! Explicit-time ledger contracts for specify-pending-proposal-quotas tasks 2.1 and 2.2.
//! Storage-level signed fixtures, not native issuance, chain evidence or engine qualification.
use chiral_network::{
    billing_store::BillingStore,
    network::{ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    protocol::{self, ModelSpec, NetworkIdentity, PriceSchedule, Quote, Signed, QUOTE_DOMAIN},
    wallet,
};

const NOW: u64 = 120;
const PAYER: &str = "0x2222222222222222222222222222222222222222";
const OTHER_PAYER: &str = "0x4444444444444444444444444444444444444444";

fn network() -> NetworkIdentity {
    NetworkIdentity {
        chain_id: ESPACE_TESTNET_CHAIN_ID,
        genesis_hash: ESPACE_TESTNET_GENESIS.into(),
        deployment_id: "proposal-quota-tests".into(),
    }
}

fn quote(provider: u64, sequence: u64, payer: &str, model: &str,
    rates: (&str, &str)) -> Signed<Quote> {
    // Public test scalars only; never an operator key or a funded wallet.
    let key = format!("{provider:064x}");
    let provider_wallet = wallet::address_from_private_key(&key).unwrap();
    protocol::sign(QUOTE_DOMAIN,
        quote_payload(&provider_wallet, sequence, payer, model, rates), &key).unwrap()
}

fn quote_payload(provider_wallet: &str, sequence: u64, payer: &str, model: &str,
    rates: (&str, &str)) -> Quote {
    Quote {
        id: format!("0x{sequence:064x}"),
        network: network(),
        offer_id: format!("0x{:064x}", 1000 + sequence),
        provider_wallet: provider_wallet.into(),
        consumer_wallet: payer.into(),
        model: ModelSpec {
            id: model.into(), revision: "ab".repeat(20), tokenizer_revision: "ab".repeat(20),
            chat_template_sha256: "cd".repeat(32), quantization: None,
            context_length: 4096, max_output_tokens: 256,
            capabilities: vec!["completions".into()], serving: None,
        },
        price: PriceSchedule {
            input_wei_per_1k: rates.0.into(), output_wei_per_1k: rates.1.into(),
        },
        funding_wei: if rates == ("0", "0") { "0" } else { "100" }.into(),
        platform_wallet: "0x3333333333333333333333333333333333333333".into(),
        fee_bps: 50, max_concurrency: 1,
        nonce: format!("0x{:064x}", 2000 + sequence),
        issued_at: NOW, expires_at: NOW + 300,
    }
}

fn assert_preserved(ledger: &BillingStore, expected: &Signed<Quote>) {
    let saved = ledger.quote(&expected.payload.id).unwrap();
    assert_eq!(saved.payload, expected.payload);
    assert_eq!(saved.signature, expected.signature);
    assert!(ledger.account(&expected.payload.id).unwrap_err().starts_with("not_found:"));
}

fn assert_capacity_rejected(ledger: &BillingStore, rejected: &Signed<Quote>) {
    let error = ledger.store_quote(rejected, NOW).unwrap_err();
    assert!(error.starts_with("rate_limit:"), "{error}");
    let missing = ledger.quote(&rejected.payload.id).unwrap_err();
    assert!(missing.starts_with("not_found:"), "{missing}");
}

#[test]
fn pending_capacity_is_shared_across_models_offers_and_free_paid_terms_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let ledger = BillingStore::open(&path, &network()).unwrap();
    let terms = [
        ("test/alpha", ("0", "0")), ("test/beta", ("1", "2")),
        ("test/alpha", ("1", "0")), ("test/beta", ("0", "2")),
        ("test/gamma", ("0", "0")), ("test/gamma", ("1", "2")),
        ("test/alpha", ("1", "2")), ("test/beta", ("0", "0")),
    ];
    let mut accepted = Vec::new();
    for (index, (model, rates)) in terms.into_iter().enumerate() {
        let signed = quote(1, index as u64 + 1, PAYER, model, rates);
        ledger.store_quote(&signed, NOW).unwrap();
        accepted.push(signed);
    }
    // Check before and after reopening so neither live nor restored admission can evade the cap.
    assert_capacity_rejected(&ledger, &quote(1, 100, PAYER, "test/new", ("0", "0")));
    drop(ledger);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    for (index, rates) in [("0", "0"), ("1", "0"), ("0", "2"), ("1", "2")]
        .into_iter().enumerate() {
        assert_capacity_rejected(&reopened,
            &quote(1, 101 + index as u64, PAYER, "test/new", rates));
    }
    // This is a payer budget, not an eight-record global or model budget.
    for index in 0..8 {
        let signed = quote(1, 200 + index, OTHER_PAYER, "test/alpha", ("1", "2"));
        reopened.store_quote(&signed, NOW).unwrap();
        accepted.push(signed);
    }
    assert_capacity_rejected(&reopened,
        &quote(1, 300, OTHER_PAYER, "test/alpha", ("1", "2")));
    drop(reopened);
    let persisted = BillingStore::open(&path, &network()).unwrap();
    for signed in &accepted { assert_preserved(&persisted, signed); }
    for sequence in [100, 101, 102, 103, 104, 300] {
        assert!(persisted.quote(&format!("0x{sequence:064x}"))
            .unwrap_err().starts_with("not_found:"));
    }
}

#[test]
fn one_provider_ledger_cannot_consume_another_providers_pending_allowance() {
    let directory = tempfile::tempdir().unwrap();
    let first = BillingStore::open(&directory.path().join("first.sqlite"), &network()).unwrap();
    let second = BillingStore::open(&directory.path().join("second.sqlite"), &network()).unwrap();
    let mut accepted = Vec::new();
    for (provider, ledger) in [(1, &first), (2, &second)] {
        for sequence in 1..=8 {
            let signed = quote(provider, sequence, PAYER, "test/alpha", ("1", "2"));
            ledger.store_quote(&signed, NOW).unwrap();
            accepted.push((ledger, signed));
        }
        assert_capacity_rejected(ledger,
            &quote(provider, 9, PAYER, "test/alpha", ("1", "2")));
    }
    // The same IDs in independent ledgers still recover each provider's own signed terms.
    for (ledger, signed) in accepted { assert_preserved(ledger, &signed); }
}

#[test]
fn ledger_wide_pending_ceiling_rejects_new_payers_without_evicting_signed_terms() {
    const CAPACITY: usize = 10_000;
    const PER_MINUTE: usize = 500;
    const EXPIRY: u64 = NOW + 3600;
    // Private SQLite connections avoid disk fsync load and never touch an existing ledger.
    let ledger = BillingStore::open(std::path::Path::new(":memory:"), &network()).unwrap();
    let key = format!("{:064x}", 1);
    let provider = wallet::address_from_private_key(&key).unwrap();
    let mut accepted = Vec::with_capacity(CAPACITY);
    for index in 0..CAPACITY {
        // Stay below the current rate ceiling, with at most eight proposals for each payer.
        let now = NOW + (index / PER_MINUTE) as u64 * 60;
        let payer = format!("0x{:040x}", 0x10000 + index / 8);
        let (model, rates) = if index % 2 == 0 {
            ("test/global-free", ("0", "0"))
        } else {
            ("test/global-paid", ("1", "2"))
        };
        let mut payload = quote_payload(&provider, index as u64 + 1, &payer, model, rates);
        payload.issued_at = now;
        // Storage-level fixture lifetime only; the native issuer still uses 300 seconds.
        payload.expires_at = EXPIRY;
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        ledger.store_quote(&signed, now).unwrap();
        accepted.push(signed);
    }

    // Use a fresh rate window and a new payer/model, so rejection must not rely on their budgets.
    let after_fill = NOW + (CAPACITY / PER_MINUTE) as u64 * 60;
    let mut payload = quote_payload(&provider, CAPACITY as u64 + 1, OTHER_PAYER,
        "test/global-third", ("1", "2"));
    payload.issued_at = after_fill;
    payload.expires_at = EXPIRY;
    let rejected = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
    for now in [after_fill, after_fill + 60] {
        let error = ledger.store_quote(&rejected, now).unwrap_err();
        assert!(error.starts_with("rate_limit:"), "{error}");
        let missing = ledger.quote(&rejected.payload.id).unwrap_err();
        assert!(missing.starts_with("not_found:"), "{missing}");
    }

    // The exact rejected bytes are admissible in an independent ledger with capacity.
    let independent = BillingStore::open(std::path::Path::new(":memory:"), &network()).unwrap();
    independent.store_quote(&rejected, after_fill + 60).unwrap();
    assert_preserved(&independent, &rejected);
    // Check every original record, including its signature: no live quote may be silently evicted.
    for signed in &accepted { assert_preserved(&ledger, signed); }
}
