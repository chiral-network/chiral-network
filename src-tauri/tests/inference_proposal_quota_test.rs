//! Explicit-time contracts for proposal/challenge budgets, capacity and lifecycle retention.
//! Ledger tests assume prior owner/spendability admission; offline signatures are not settlement.
use axum::http::HeaderMap;
use chiral_network::{
    auth::{self, AuthPayload},
    billing_store::{BillingStore, CredentialIssueRequest},
    chain_client::{ChainClient, ValidatedPaymentEnvelope},
    network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    protocol::{self, ModelSpec, NetworkIdentity, PriceSchedule, Quote, Signed, QUOTE_DOMAIN},
    wallet,
};

const NOW: u64 = 120;
const PAYER: &str = "0x2222222222222222222222222222222222222222";
const OTHER_PAYER: &str = "0x4444444444444444444444444444444444444444";
// Public fixture scalar; never funded or used outside isolated test state.
const CHALLENGE_OWNER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000007";

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

// Restore complete, genuinely signed standalone records as a storage-boundary prerequisite.
// These are not store_quote admissions and do not claim to have spent issuance units.
fn restore_standalone(path: &std::path::Path, count: usize, expires_at: u64) -> Vec<Signed<Quote>> {
    let key = format!("{:064x}", 1);
    let provider = wallet::address_from_private_key(&key).unwrap();
    let mut connection = rusqlite::Connection::open(path).unwrap();
    let transaction = connection.transaction().unwrap();
    let mut records = Vec::with_capacity(count);
    for index in 0..count {
        let mut payload = quote_payload(&provider, 100_000 + index as u64, PAYER,
            "test/restored-storage", ("1", "2"));
        payload.expires_at = expires_at;
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        transaction.execute(
            "INSERT INTO quotes(id,owner,expires,record) VALUES(?1,?2,?3,?4)",
            rusqlite::params![signed.payload.id, signed.payload.consumer_wallet,
                signed.payload.expires_at, serde_json::to_string(&signed).unwrap()],
        ).unwrap();
        records.push(signed);
    }
    transaction.commit().unwrap();
    records
}

#[test]
fn same_wallet_quotes_exceed_former_limits_across_terms_and_restart() {
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
    // Neither the ninth live quote nor the thirteenth issuance is a wallet boundary.
    let ninth = quote(1, 100, PAYER, "test/new", ("0", "0"));
    ledger.store_quote(&ninth, NOW).unwrap();
    accepted.push(ninth);
    drop(ledger);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    for (index, rates) in [("0", "0"), ("1", "0"), ("0", "2"), ("1", "2")]
        .into_iter().enumerate() {
        let signed = quote(1, 101 + index as u64, PAYER, "test/new", rates);
        reopened.store_quote(&signed, NOW).unwrap();
        accepted.push(signed);
    }
    let other = quote(1, 200, OTHER_PAYER, "test/alpha", ("1", "2"));
    reopened.store_quote(&other, NOW).unwrap();
    accepted.push(other);
    drop(reopened);
    let persisted = BillingStore::open(&path, &network()).unwrap();
    for signed in &accepted { assert_preserved(&persisted, signed); }
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
        // Actual committed admissions use separate provider-wide minute windows.
        let now = NOW + (index / PER_MINUTE) as u64 * 60;
        let payer = PAYER;
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

#[test]
fn competing_connections_admit_only_one_quote_into_the_final_global_storage_slot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let initial = BillingStore::open(&path, &network()).unwrap();
    let accepted = restore_standalone(&path, 9_999, NOW + 300);
    drop(initial);

    // Pre-open independent connections and sign before releasing the shared start barrier.
    // A fresh rate window leaves admission governed by the final pending slot.
    let race_at = NOW + 60;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers = [8, 9].map(|sequence| {
        let ledger = BillingStore::open(&path, &network()).unwrap();
        let signed = quote(1, sequence, PAYER, "test/race", ("1", "2"));
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let result = ledger.store_quote(&signed, race_at);
            (signed, result)
        })
    });
    let outcomes = workers.map(|worker| worker.join().unwrap());
    assert_eq!(outcomes.iter().filter(|(_, result)| result.is_ok()).count(), 1);

    // All writer connections have closed: inspect durable outcomes through a new connection.
    let reopened = BillingStore::open(&path, &network()).unwrap();
    for (signed, result) in outcomes {
        match result {
            Ok(()) => assert_preserved(&reopened, &signed),
            Err(error) => {
                assert!(error.starts_with("rate_limit:"), "{error}");
                assert!(reopened.quote(&signed.payload.id).unwrap_err().starts_with("not_found:"));
            }
        }
    }
    for signed in &accepted { assert_preserved(&reopened, signed); }
    let excess = quote(1, 10, PAYER, "test/race", ("1", "2"));
    let error = reopened.store_quote(&excess, race_at).unwrap_err();
    assert!(error.starts_with("rate_limit:"), "{error}");
    assert!(reopened.quote(&excess.payload.id).unwrap_err().starts_with("not_found:"));
}

fn rejection_control_pair() -> (tempfile::TempDir, BillingStore, BillingStore, u64) {
    let directory = tempfile::tempdir().unwrap();
    let control_path = directory.path().join("control.sqlite");
    let exercised_path = directory.path().join("exercised.sqlite");
    let control = BillingStore::open(&control_path, &network()).unwrap();
    let exercised = BillingStore::open(&exercised_path, &network()).unwrap();
    let accepted = restore_standalone(&control_path, 10_000, NOW + 1);
    restore_standalone(&exercised_path, 10_000, NOW + 1);
    let rejected = quote(1, 100, PAYER, "test/rollback", ("1", "2"));
    for _ in 0..3 { assert_capacity_rejected(&exercised, &rejected); }
    for signed in &accepted {
        assert_preserved(&control, signed);
        assert_preserved(&exercised, signed);
    }
    (directory, control, exercised, NOW + 1)
}

fn matching_admission(control: &BillingStore, exercised: &BillingStore,
    signed: &Signed<Quote>, now: u64) -> bool {
    let expected = control.store_quote(signed, now);
    let observed = exercised.store_quote(signed, now);
    match expected {
        Ok(()) => {
            observed.expect("quota rejections must not consume later admission allowance");
            assert_preserved(control, signed);
            assert_preserved(exercised, signed);
            true
        }
        Err(error) => {
            assert!(error.starts_with("rate_limit:"), "{error}");
            let observed = observed.unwrap_err();
            assert!(observed.starts_with("rate_limit:"), "{observed}");
            for ledger in [control, exercised] {
                assert!(ledger.quote(&signed.payload.id).unwrap_err().starts_with("not_found:"));
            }
            false
        }
    }
}

#[test]
fn storage_refusals_preserve_all_six_hundred_committed_units_after_expiry() {
    let (_directory, control, exercised, now) = rejection_control_pair();
    // Expiry releases the restored storage prerequisite within the same minute as refusal.
    // Both the previously refused wallet and a different wallet share all 600 remaining units.
    for index in 0..600 {
        let payer = if index % 2 == 0 { PAYER } else { OTHER_PAYER };
        let signed = quote(1, 200 + index, payer, "test/rollback", ("1", "2"));
        assert!(matching_admission(&control, &exercised, &signed, now));
    }
    let excess = quote(1, 800, PAYER, "test/rollback", ("1", "2"));
    assert!(!matching_admission(&control, &exercised, &excess, now));
}

fn assert_rate_rejected_at(ledger: &BillingStore, rejected: &Signed<Quote>, now: u64) {
    let error = ledger.store_quote(rejected, now).unwrap_err();
    assert!(error.starts_with("rate_limit:"), "{error}");
    assert!(ledger.quote(&rejected.payload.id).unwrap_err().starts_with("not_found:"));
}

#[test]
fn duplicate_insertions_preserve_the_last_global_unit_for_the_same_wallet() {
    let ledger = BillingStore::open(std::path::Path::new(":memory:"), &network()).unwrap();
    let key = format!("{:064x}", 1);
    let provider = wallet::address_from_private_key(&key).unwrap();
    for offset in 0..12 {
        let now = NOW + offset;
        let mut payload = quote_payload(&provider, offset + 1, PAYER,
            "test/rate-refusal", ("1", "2"));
        payload.issued_at = now;
        payload.expires_at = now + 1; // Reclaim pending slots, not rate units.
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        ledger.store_quote(&signed, now).unwrap();
    }
    let now = NOW + 12;
    // Twelve payer admissions plus 587 other payers leave one shared unit.
    for index in 0..587 {
        let payer = format!("0x{:040x}", 0x10000 + index);
        let payload = quote_payload(&provider, 1000 + index, &payer,
            "test/rate-refusal", ("1", "2"));
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        ledger.store_quote(&signed, now).unwrap();
    }
    let retained = ledger.quote(&format!("0x{:064x}", 1000)).unwrap();
    // Duplicate IDs fail without spending the last unit, even after twelve same-wallet commits.
    for _ in 0..2 { assert!(ledger.store_quote(&retained, now).is_err()); }
    let last = quote(1, 2001, PAYER, "test/rate-refusal", ("1", "2"));
    ledger.store_quote(&last, now).unwrap();
    assert_preserved(&ledger, &last);
    assert_preserved(&ledger, &retained);
    assert_rate_rejected_at(&ledger,
        &quote(1, 2002, OTHER_PAYER, "test/rate-refusal", ("1", "2")), now);
}

#[test]
fn failed_insertion_rolls_back_rate_windows_counters_and_pending_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let control = BillingStore::open(std::path::Path::new(":memory:"), &network()).unwrap();
    let exercised = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let original = quote(1, 1, PAYER, "test/insertion", ("1", "2"));
    let mut payload = quote_payload(&original.payload.provider_wallet, 2, OTHER_PAYER,
        "test/insertion", ("1", "2"));
    payload.expires_at = NOW + 1;
    let expiring = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
    for store in [&control, &exercised] {
        store.store_quote(&original, NOW).unwrap();
        store.store_quote(&expiring, NOW).unwrap();
    }
    // The original remains valid, but its ID is already stored. Its rejected insertion
    // must not retain a future-minute reset or the tentative deletion of expiring.
    assert!(exercised.store_quote(&original, NOW + 60).is_err());
    assert_preserved(&exercised, &original);
    assert_preserved(&exercised, &expiring);
    drop(exercised);
    let exercised = BillingStore::open(&path, &network()).unwrap();
    assert_preserved(&exercised, &original);
    assert_preserved(&exercised, &expiring);

    // No newer minute was committed: the original two insertions leave exactly 598
    // global units, including for their own wallets. Successful probes may clean expiry.
    let now = NOW + 1;
    for index in 0..598 {
        let payer = if index % 2 == 0 { PAYER } else { OTHER_PAYER };
        let signed = quote(1, 100 + index, payer, "test/insertion", ("1", "2"));
        assert!(matching_admission(&control, &exercised, &signed, now));
    }
    let excess = quote(1, 2000, OTHER_PAYER, "test/insertion", ("1", "2"));
    assert!(!matching_admission(&control, &exercised, &excess, now));
    for store in [&control, &exercised] { assert_preserved(store, &original); }
}

#[test]
fn competing_same_wallet_quotes_cross_the_former_rate_limit_without_extra_charges() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let initial = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let provider = wallet::address_from_private_key(&key).unwrap();
    for offset in 0..11 {
        let now = NOW + offset;
        let mut payload = quote_payload(&provider, offset + 1, PAYER,
            "test/payer-rate-race", ("1", "2"));
        payload.issued_at = now;
        payload.expires_at = now + 1;
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        initial.store_quote(&signed, now).unwrap();
    }
    drop(initial);
    let race_at = NOW + 11;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers = [12, 13].map(|sequence| {
        let ledger = BillingStore::open(&path, &network()).unwrap();
        let signed = quote(1, sequence, PAYER, "test/payer-rate-race", ("1", "2"));
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let result = ledger.store_quote(&signed, race_at);
            (signed, result)
        })
    });
    let outcomes = workers.map(|worker| worker.join().unwrap());
    assert_eq!(outcomes.iter().filter(|(_, result)| result.is_ok()).count(), 2);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    for (signed, result) in outcomes {
        result.unwrap();
        assert_preserved(&reopened, &signed);
    }
    // Thirteen successful commits leave exactly 587 shared units.
    for index in 0..587 {
        let payer = format!("0x{:040x}", 0x10000 + index);
        let payload = quote_payload(&provider, 1000 + index, &payer,
            "test/payer-rate-race", ("1", "2"));
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        reopened.store_quote(&signed, race_at).unwrap();
        assert_preserved(&reopened, &signed);
    }
    assert_rate_rejected_at(&reopened,
        &quote(1, 2000, OTHER_PAYER, "test/payer-rate-race", ("1", "2")), race_at);
}

#[test]
fn competing_connections_admit_only_one_quote_at_the_global_rate_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let initial = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let provider = wallet::address_from_private_key(&key).unwrap();
    for sequence in 1..=599 {
        let payer = format!("0x{:040x}", 0x10000 + sequence);
        let payload = quote_payload(&provider, sequence, &payer,
            "test/global-rate-race", ("1", "2"));
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        initial.store_quote(&signed, NOW).unwrap();
    }
    let retained = initial.quote(&format!("0x{:064x}", 1)).unwrap();
    drop(initial);
    let race_at = NOW + 1;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers = [(600, PAYER), (601, OTHER_PAYER)].map(|(sequence, payer)| {
        let ledger = BillingStore::open(&path, &network()).unwrap();
        let signed = quote(1, sequence, payer, "test/global-rate-race", ("1", "2"));
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let result = ledger.store_quote(&signed, race_at);
            (signed, result)
        })
    });
    let outcomes = workers.map(|worker| worker.join().unwrap());
    assert_eq!(outcomes.iter().filter(|(_, result)| result.is_ok()).count(), 1);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    for (signed, result) in outcomes {
        match result {
            Ok(()) => assert_preserved(&reopened, &signed),
            Err(error) => {
                assert!(error.starts_with("rate_limit:"), "{error}");
                assert!(reopened.quote(&signed.payload.id).unwrap_err().starts_with("not_found:"));
            }
        }
    }
    assert_preserved(&reopened, &retained);
    // Naming either wallet cannot bypass the exhausted provider-wide budget.
    assert_rate_rejected_at(&reopened,
        &quote(1, 602, PAYER, "test/global-rate-race", ("1", "2")), race_at);
}

fn rate_challenge(quote: &Quote, nonce: u64, now: u64) -> chiral_network::auth::AuthPayload {
    chiral_network::auth::AuthPayload {
        network: quote.network.clone(),
        provider_wallet: quote.provider_wallet.clone(),
        wallet: quote.consumer_wallet.clone(),
        method: "GET".into(),
        path: format!("/v1/contracts/{}/balance", quote.id),
        body_hash: protocol::hash_hex(b""),
        nonce: format!("0x{nonce:064x}"),
        expires_at: now + 60,
    }
}

fn assert_challenge_rate_rejected_at(ledger: &BillingStore, challenge: &AuthPayload, now: u64) {
    let error = ledger.create_challenge(challenge, now).unwrap_err();
    assert!(error.starts_with("rate_limit:"), "{error}");
    assert!(ledger.challenge(&challenge.nonce).is_err());
}

fn consume_rate_challenge(ledger: &BillingStore, challenge: &AuthPayload, now: u64) {
    let mut headers = HeaderMap::new();
    headers.insert("x-chiral-owner", challenge.wallet.parse().unwrap());
    headers.insert("x-chiral-nonce", challenge.nonce.parse().unwrap());
    headers.insert("x-chiral-signature", wallet::sign_message(CHALLENGE_OWNER_KEY,
        &auth::challenge_payload(challenge).unwrap()).unwrap().parse().unwrap());
    assert_eq!(auth::authenticate_owner(ledger, &headers, &challenge.network,
        &challenge.provider_wallet, "GET", &challenge.path, b"", now).unwrap(), challenge.wallet);
    assert!(ledger.challenge(&challenge.nonce).is_err());
}

fn assert_read_context(ledger: &BillingStore, credential: &protocol::Credential, terms: &Quote, now: u64) {
    let context = ledger.authenticate(&credential.bearer, now).unwrap();
    assert_eq!(context.credential_id, credential.id);
    assert_eq!(context.scopes, ["read"]);
    assert_eq!(context.account.contract_id, terms.id);
    assert_eq!(context.account.consumer_wallet, terms.consumer_wallet);
    assert_eq!(context.account.model, terms.model);
    assert_eq!(context.account.price, terms.price);
    assert_eq!(context.account.status, "free");
    assert_eq!(context.account.funded_wei, "0");
}

fn race_rate_challenges(path: &std::path::Path, candidates: [&AuthPayload; 2], now: u64) -> usize {
    // Open both connections before the barrier so an initialization failure cannot strand a worker.
    let [first, second] = candidates.map(|candidate| BillingStore::open(path, &candidate.network).unwrap());
    let barrier = std::sync::Barrier::new(2);
    let attempt = |ledger: BillingStore, candidate: &AuthPayload| {
        barrier.wait();
        ledger.create_challenge(candidate, now)
    };
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| attempt(first, candidates[0]));
        let second = scope.spawn(|| attempt(second, candidates[1]));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let winner = results.iter().position(|result| result.is_ok()).unwrap();
    let error = results[1 - winner].as_ref().unwrap_err();
    assert!(error.starts_with("rate_limit:"), "{error}");
    let reopened = BillingStore::open(path, &candidates[0].network).unwrap();
    assert_eq!(serde_json::to_value(reopened.challenge(&candidates[winner].nonce).unwrap()).unwrap(),
        serde_json::to_value(candidates[winner]).unwrap());
    assert!(reopened.challenge(&candidates[1 - winner].nonce).is_err());
    winner
}

#[test]
fn ledger_rate_budget_is_shared_across_terms_but_not_providers_or_control_budgets() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let ledger = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let original = quote(1, 1, PAYER, "test/global-rate-free", ("0", "0"));
    ledger.store_quote(&original, NOW).unwrap();
    let opened = ledger.open_account(&original, None, NOW).unwrap();
    // Challenge issuance must not consume one of the ledger's 600 proposal units.
    let before = rate_challenge(&original.payload, 1, NOW);
    ledger.create_challenge(&before, NOW).unwrap();

    let terms = [
        ("test/global-rate-a", ("0", "0")), ("test/global-rate-b", ("1", "0")),
        ("test/global-rate-a", ("0", "2")), ("test/global-rate-b", ("1", "2")),
    ];
    let mut accepted = Vec::with_capacity(599);
    for sequence in 2..=600 {
        // Distinct payers and only 599 pending records isolate the global rate boundary.
        let payer = format!("0x{:040x}", 0x10000 + sequence);
        let (model, rates) = terms[(sequence as usize - 2) % terms.len()];
        let mut payload = quote_payload(&original.payload.provider_wallet, sequence, &payer,
            model, rates);
        if rates != ("0", "0") { payload.funding_wei = (100 + sequence).to_string(); }
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        ledger.store_quote(&signed, NOW).unwrap();
        accepted.push(signed);
    }
    let rejected: Vec<_> = terms.iter().enumerate().map(|(index, (_, rates))| {
        quote(1, 601 + index as u64, OTHER_PAYER, "test/global-rate-new", *rates)
    }).collect();
    for signed in &rejected { assert_rate_rejected_at(&ledger, signed, NOW); }

    // Exhausted proposal rates must not consume the ability to authenticate or reserve
    // inference on an accepted account. This is admission only: no engine or settlement.
    let after = rate_challenge(&original.payload, 2, NOW);
    ledger.create_challenge(&after, NOW).unwrap();
    for challenge in [&before, &after] {
        assert_eq!(serde_json::to_value(ledger.challenge(&challenge.nonce).unwrap()).unwrap(),
            serde_json::to_value(challenge).unwrap());
    }
    let reservation = ledger.reserve(&original.payload.id, &opened.credential.as_ref().unwrap().id,
        "after-global-proposal-exhaustion", 1, 1, NOW, &original.payload.model).unwrap();
    let request = ledger.request_view(&original.payload.id,
        "after-global-proposal-exhaustion").unwrap();
    assert_eq!(request.id, reservation.id);
    assert_eq!(request.status, "reserved");
    assert_eq!(request.max_cost_wei, "0");

    let independent = BillingStore::open(&directory.path().join("other-provider.sqlite"),
        &network()).unwrap();
    let other_provider = quote(2, 601, OTHER_PAYER, "test/global-rate-new", ("1", "2"));
    independent.store_quote(&other_provider, NOW).unwrap();
    assert_preserved(&independent, &other_provider);

    // Other operations cannot reset the exhausted budget, including after storage reopening.
    drop(ledger);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    for signed in &rejected { assert_rate_rejected_at(&reopened, signed, NOW + 1); }
    for signed in &accepted { assert_preserved(&reopened, signed); }
    let saved = reopened.quote(&original.payload.id).unwrap();
    assert_eq!(saved.payload, original.payload);
    assert_eq!(saved.signature, original.signature);
    let authenticated = reopened.authenticate(&opened.credential.as_ref().unwrap().bearer, NOW + 1).unwrap();
    assert_eq!(authenticated.credential_id, opened.credential.as_ref().unwrap().id);
    assert_eq!(authenticated.account.contract_id, original.payload.id);
    assert_eq!(authenticated.account.funded_wei, "0");
    assert!(reopened.prepare_fee_batches(NOW + 1, true).unwrap().is_empty());
}

#[test]
fn same_wallet_quotes_across_terms_preserve_independent_control_authority() {
    let ledger = BillingStore::open(std::path::Path::new(":memory:"), &network()).unwrap();
    let key = format!("{:064x}", 1);
    let original = quote(1, 1, PAYER, "test/payer-scope-free", ("0", "0"));
    ledger.store_quote(&original, NOW).unwrap();
    let opened = ledger.open_account(&original, None, NOW).unwrap();
    let before = rate_challenge(&original.payload, 1, NOW);
    ledger.create_challenge(&before, NOW).unwrap();
    // A live reservation must neither spend a proposal unit nor prevent fresh proposals.
    let reservation = ledger.reserve(&original.payload.id, &opened.credential.as_ref().unwrap().id,
        "before-more-same-wallet-quotes", 1, 1, NOW, &original.payload.model).unwrap();
    let terms = [
        ("test/payer-scope-a", ("0", "0")), ("test/payer-scope-b", ("1", "0")),
        ("test/payer-scope-a", ("0", "2")), ("test/payer-scope-b", ("1", "2")),
    ];
    for offset in 1..=11 {
        let now = NOW + offset;
        let (model, rates) = terms[(offset as usize - 1) % terms.len()];
        let mut payload = quote_payload(&original.payload.provider_wallet, offset + 1, PAYER,
            model, rates);
        payload.issued_at = now;
        payload.expires_at = now + 1; // Storage fixture, not the native issuance TTL.
        if rates != ("0", "0") { payload.funding_wei = (100 + offset).to_string(); }
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        ledger.store_quote(&signed, now).unwrap();
        assert_preserved(&ledger, &signed);
    }
    let now = NOW + 12;
    let further: Vec<_> = terms.iter().enumerate().map(|(index, (_, rates))| {
        quote(1, 100 + index as u64, PAYER, "test/payer-scope-new", *rates)
    }).collect();
    for signed in &further {
        ledger.store_quote(signed, now).unwrap();
        assert_preserved(&ledger, signed);
    }

    let after = rate_challenge(&original.payload, 2, now);
    ledger.create_challenge(&after, now).unwrap();
    for challenge in [&before, &after] {
        assert_eq!(serde_json::to_value(ledger.challenge(&challenge.nonce).unwrap()).unwrap(),
            serde_json::to_value(challenge).unwrap());
    }
    let other_payer = quote(1, 200, OTHER_PAYER, "test/payer-scope-a", ("1", "2"));
    ledger.store_quote(&other_payer, now).unwrap();
    assert_preserved(&ledger, &other_payer);
    let independent = BillingStore::open(std::path::Path::new(":memory:"), &network()).unwrap();
    let other_provider = quote(2, 100, PAYER, "test/payer-scope-new", ("0", "0"));
    independent.store_quote(&other_provider, now).unwrap();
    assert_preserved(&independent, &other_provider);
    for signed in &further { assert_preserved(&ledger, signed); }
    let request = ledger.request_view(&original.payload.id,
        "before-more-same-wallet-quotes").unwrap();
    assert_eq!(request.id, reservation.id);
    assert_eq!(request.status, "reserved");
    assert_eq!(request.max_cost_wei, "0");
    let authenticated = ledger.authenticate(&opened.credential.as_ref().unwrap().bearer, now).unwrap();
    assert_eq!(authenticated.credential_id, opened.credential.as_ref().unwrap().id);
    assert_eq!(authenticated.account.contract_id, original.payload.id);
    assert_eq!(authenticated.account.funded_wei, "0");
    assert!(ledger.prepare_fee_batches(now, true).unwrap().is_empty());
}

#[test]
fn challenge_ledger_rate_is_independent_of_proposals_providers_and_existing_accounts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let ledger = BillingStore::open(&path, &network()).unwrap();
    let owner = wallet::address_from_private_key(CHALLENGE_OWNER_KEY).unwrap();
    let original = quote(1, 1, &owner, "test/challenge-global", ("0", "0"));
    ledger.store_quote(&original, NOW).unwrap();
    let opened = ledger.open_account(&original, None, NOW).unwrap();
    let read = ledger.issue_credential(&original.payload.id, &CredentialIssueRequest {
        expires_at: NOW + 300, spend_cap_wei: None, max_concurrency: 1, scopes: vec!["read".into()],
    }, NOW).unwrap();
    assert_read_context(&ledger, &read, &original.payload, NOW);

    let mut accepted = Vec::with_capacity(600);
    for sequence in 1..=600 {
        let mut challenge = rate_challenge(&original.payload, sequence, NOW);
        challenge.path = format!("/v1/contracts/{}", original.payload.id);
        if sequence > 1 {
            // One pending record per named wallet; neither wallet rates nor occupancy can mask 600/601.
            challenge.wallet = format!("0x{:040x}", 0x10000 + sequence);
            match sequence % 3 {
                0 => challenge.path = format!("/v1/contracts/{}/requests?before={sequence}", original.payload.id),
                1 => {
                    challenge.method = "POST".into();
                    challenge.path = format!("/v1/contracts/0x{sequence:064x}/credentials");
                    challenge.body_hash = protocol::hash_hex(br#"{"scopes":["read"]}"#);
                }
                _ => {
                    challenge.method = "DELETE".into();
                    challenge.path = format!("/v1/contracts/0x{sequence:064x}/credentials/0x{sequence:064x}");
                }
            }
        }
        ledger.create_challenge(&challenge, NOW).unwrap();
        accepted.push(challenge);
    }
    let mut rejected_owner = accepted[0].clone();
    rejected_owner.nonce = format!("0x{:064x}", 601);
    let mut rejected_other = rejected_owner.clone();
    rejected_other.wallet = OTHER_PAYER.into();
    rejected_other.nonce = format!("0x{:064x}", 602);
    for rejected in [&rejected_owner, &rejected_other] {
        assert_challenge_rate_rejected_at(&ledger, rejected, NOW + 1);
    }

    // The original opening spent one proposal unit; all 599 others remain after 600 challenges.
    let key = format!("{:064x}", 1);
    let mut proposals = Vec::with_capacity(599);
    for sequence in 2..=600 {
        let payer = format!("0x{:040x}", 0x10000 + sequence);
        let signed = protocol::sign(QUOTE_DOMAIN, quote_payload(&original.payload.provider_wallet,
            sequence, &payer, &original.payload.model.id, ("0", "0")), &key).unwrap();
        ledger.store_quote(&signed, NOW + 1).unwrap();
        proposals.push(signed);
    }
    let rejected_quote = quote(1, 601, &owner, "test/challenge-global", ("0", "0"));
    assert_rate_rejected_at(&ledger, &rejected_quote, NOW + 1);
    assert_read_context(&ledger, &read, &original.payload, NOW + 1);
    // Reservation only, not dispatched inference or an invented usage/settlement result.
    let reservation = ledger.reserve(&original.payload.id, &opened.credential.as_ref().unwrap().id,
        "after-both-control-budgets", 1, 1, NOW + 1, &original.payload.model).unwrap();
    for rejected in [&rejected_owner, &rejected_other] {
        assert_challenge_rate_rejected_at(&ledger, rejected, NOW + 1);
    }

    let independent = BillingStore::open(&directory.path().join("other-provider.sqlite"), &network()).unwrap();
    let other_provider = quote(2, 601, &owner, "test/challenge-global", ("0", "0"));
    independent.store_quote(&other_provider, NOW + 1).unwrap();
    let other_challenge = rate_challenge(&other_provider.payload, 601, NOW + 1);
    independent.create_challenge(&other_challenge, NOW + 1).unwrap();
    assert_preserved(&independent, &other_provider);
    assert_eq!(serde_json::to_value(independent.challenge(&other_challenge.nonce).unwrap()).unwrap(),
        serde_json::to_value(&other_challenge).unwrap());

    drop(ledger);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    for rejected in [&rejected_owner, &rejected_other] {
        assert_challenge_rate_rejected_at(&reopened, rejected, NOW + 2);
    }
    assert_rate_rejected_at(&reopened, &rejected_quote, NOW + 2);
    for challenge in &accepted {
        assert_eq!(serde_json::to_value(reopened.challenge(&challenge.nonce).unwrap()).unwrap(),
            serde_json::to_value(challenge).unwrap());
    }
    for proposed in &proposals { assert_preserved(&reopened, proposed); }
    assert_read_context(&reopened, &read, &original.payload, NOW + 2);
    let authenticated = reopened.authenticate(&opened.credential.as_ref().unwrap().bearer, NOW + 2).unwrap();
    assert_eq!(authenticated.credential_id, opened.credential.as_ref().unwrap().id);
    assert!(authenticated.scopes.iter().any(|scope| scope == "infer"));
    let request = reopened.request_view(&original.payload.id, "after-both-control-budgets").unwrap();
    assert_eq!(request.id, reservation.id);
    assert_eq!(request.status, "reserved");
    assert_eq!(request.max_cost_wei, "0");
    consume_rate_challenge(&reopened, &accepted[0], NOW + 2);
    assert_challenge_rate_rejected_at(&reopened, &rejected_owner, NOW + 2);
}

#[test]
fn challenge_wallet_rate_preserves_other_wallets_and_independent_control_authority() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let ledger = BillingStore::open(&path, &network()).unwrap();
    let owner = wallet::address_from_private_key(CHALLENGE_OWNER_KEY).unwrap();
    let original = quote(1, 1, &owner, "test/challenge-wallet", ("0", "0"));
    ledger.store_quote(&original, NOW).unwrap();
    let opened = ledger.open_account(&original, None, NOW).unwrap();
    let read = ledger.issue_credential(&original.payload.id, &CredentialIssueRequest {
        expires_at: NOW + 300, spend_cap_wei: None, max_concurrency: 1, scopes: vec!["read".into()],
    }, NOW).unwrap();
    assert_read_context(&ledger, &read, &original.payload, NOW);
    let reservation = ledger.reserve(&original.payload.id, &opened.credential.as_ref().unwrap().id,
        "busy-before-wallet-challenges", 1, 1, NOW, &original.payload.model).unwrap();
    let saturated = ledger.reserve(&original.payload.id, &opened.credential.as_ref().unwrap().id,
        "saturated-before-challenges", 1, 1, NOW, &original.payload.model).unwrap_err();
    assert!(saturated.starts_with("saturation:"), "{saturated}");
    assert!(ledger.request_view(&original.payload.id, "saturated-before-challenges").is_err());

    // Inference saturation and read authentication must not consume any of the twelve issuance units.
    for nonce in 1..=12 {
        let mut challenge = rate_challenge(&original.payload, nonce, NOW);
        challenge.path = format!("/v1/contracts/{}", original.payload.id);
        ledger.create_challenge(&challenge, NOW).unwrap();
        consume_rate_challenge(&ledger, &challenge, NOW);
    }
    let rejected = rate_challenge(&original.payload, 13, NOW);
    assert_challenge_rate_rejected_at(&ledger, &rejected, NOW);
    let mut other_wallet = rate_challenge(&original.payload, 14, NOW);
    other_wallet.wallet = OTHER_PAYER.into();
    ledger.create_challenge(&other_wallet, NOW).unwrap();

    // Exhausting owner challenges must not reinstate either former same-wallet quote gate.
    for sequence in 2..=13 {
        let signed = quote(1, sequence, &owner, "test/challenge-wallet", ("0", "0"));
        ledger.store_quote(&signed, NOW).unwrap();
        assert_preserved(&ledger, &signed);
    }
    let now = NOW + 12;
    let further_quote = quote(1, 14, &owner, "test/challenge-wallet", ("0", "0"));
    ledger.store_quote(&further_quote, now).unwrap();
    assert_challenge_rate_rejected_at(&ledger, &rejected, now);
    assert_read_context(&ledger, &read, &original.payload, now);

    let independent = BillingStore::open(&directory.path().join("other-provider.sqlite"), &network()).unwrap();
    let other_provider = quote(2, 13, &owner, "test/challenge-wallet", ("0", "0"));
    independent.store_quote(&other_provider, now).unwrap();
    let other_challenge = rate_challenge(&other_provider.payload, 13, now);
    independent.create_challenge(&other_challenge, now).unwrap();
    assert_preserved(&independent, &other_provider);
    assert_eq!(serde_json::to_value(independent.challenge(&other_challenge.nonce).unwrap()).unwrap(),
        serde_json::to_value(&other_challenge).unwrap());

    drop(ledger);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    assert_challenge_rate_rejected_at(&reopened, &rejected, now);
    assert_preserved(&reopened, &further_quote);
    assert_read_context(&reopened, &read, &original.payload, now);
    assert_eq!(serde_json::to_value(reopened.challenge(&other_wallet.nonce).unwrap()).unwrap(),
        serde_json::to_value(&other_wallet).unwrap());
    let authenticated = reopened.authenticate(&opened.credential.as_ref().unwrap().bearer, now).unwrap();
    assert_eq!(authenticated.credential_id, opened.credential.as_ref().unwrap().id);
    assert!(authenticated.scopes.iter().any(|scope| scope == "infer"));
    let request = reopened.request_view(&original.payload.id, "busy-before-wallet-challenges").unwrap();
    assert_eq!(request.id, reservation.id);
    assert_eq!(request.status, "reserved");
    assert_eq!(request.max_cost_wei, "0");
}

#[test]
fn challenge_wallet_refusals_preserve_the_last_global_unit_and_accepted_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let ledger = BillingStore::open(&path, &network()).unwrap();
    let owner = wallet::address_from_private_key(CHALLENGE_OWNER_KEY).unwrap();
    let original = quote(1, 1, &owner, "test/challenge-preservation-free", ("0", "0"));
    ledger.store_quote(&original, NOW).unwrap();
    let opened = ledger.open_account(&original, None, NOW).unwrap();
    let read = ledger.issue_credential(&original.payload.id, &CredentialIssueRequest {
        expires_at: NOW + 300, spend_cap_wei: None, max_concurrency: 1, scopes: vec!["read".into()],
    }, NOW).unwrap();

    // Retain a real signed acknowledgement after the assumed owner/spendability boundary.
    // The envelope is validated offline: no transaction is broadcast, executed or credited.
    let key = format!("{:064x}", 1);
    let payer = wallet::address_from_private_key(&format!("{:064x}", 2)).unwrap();
    let paid = quote(1, 2, &payer, "test/challenge-preservation-paid", ("1", "2"));
    ledger.store_quote(&paid, NOW).unwrap();
    let (raw, envelope) = signed_opening(&paid.payload, directory.path());
    let ack = ledger.register_funding_intent(&paid, &payer, &raw, &envelope.tx_hash,
        envelope.nonce, &key, || Ok(NOW), true, &paid.payload.model).unwrap();
    protocol::verify(protocol::INTENT_DOMAIN, &ack, &paid.payload.provider_wallet).unwrap();
    let protected_state = |store: &BillingStore| serde_json::json!({
        "account": store.account(&original.payload.id).unwrap(),
        "credentials": store.credentials(&original.payload.id).unwrap(),
        "original_terms": store.quote(&original.payload.id).unwrap(),
        "funding_terms": store.quote(&paid.payload.id).unwrap(),
        "funding_intent": store.funding_intent(&paid.payload, &raw).unwrap().unwrap(),
    });
    let before = protected_state(&ledger);
    for nonce in 1..=11 {
        let challenge = rate_challenge(&original.payload, nonce, NOW);
        ledger.create_challenge(&challenge, NOW).unwrap();
        consume_rate_challenge(&ledger, &challenge, NOW);
    }
    // This twelfth insertion is committed without constructing an HTTP reply or consuming its proof.
    let undisclosed = rate_challenge(&original.payload, 12, NOW);
    ledger.create_challenge(&undisclosed, NOW).unwrap();
    for index in 0..587 {
        let mut challenge = rate_challenge(&original.payload, 1000 + index, NOW);
        challenge.wallet = format!("0x{:040x}", 0x10000 + index);
        ledger.create_challenge(&challenge, NOW).unwrap();
    }
    // Twelve owner units plus 587 others leave exactly one ledger unit.
    let rejected = rate_challenge(&original.payload, 2000, NOW);
    for _ in 0..2 { assert_challenge_rate_rejected_at(&ledger, &rejected, NOW + 1); }
    assert_eq!(protected_state(&ledger), before);
    drop(ledger);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    assert_challenge_rate_rejected_at(&reopened, &rejected, NOW + 1);
    let mut last = rate_challenge(&original.payload, 2001, NOW);
    last.wallet = OTHER_PAYER.into();
    reopened.create_challenge(&last, NOW + 1).unwrap();
    let mut excess = last.clone();
    excess.nonce = format!("0x{:064x}", 2002);
    assert_challenge_rate_rejected_at(&reopened, &excess, NOW + 1);
    for challenge in [&undisclosed, &last] {
        assert_eq!(serde_json::to_value(reopened.challenge(&challenge.nonce).unwrap()).unwrap(),
            serde_json::to_value(challenge).unwrap());
    }
    // Cached ledger recovery, not an assertion that exhausted callers can obtain a new owner proof.
    let replay = reopened.register_funding_intent(&paid, &payer, &raw, &envelope.tx_hash,
        envelope.nonce, &key, || Ok(NOW + 1), false, &paid.payload.model).unwrap();
    assert_eq!(replay.payload, ack.payload);
    assert_eq!(replay.signature, ack.signature);
    assert_eq!(protected_state(&reopened), before);
    assert_preserved(&reopened, &paid);
    assert_read_context(&reopened, &read, &original.payload, NOW + 1);
    assert_eq!(reopened.authenticate(&opened.credential.as_ref().unwrap().bearer, NOW + 1).unwrap().credential_id,
        opened.credential.as_ref().unwrap().id);
    assert!(reopened.prepare_fee_batches(NOW + 1, true).unwrap().is_empty());
}

#[test]
fn challenge_wallet_rate_race_commits_one_unit_without_leaking_global_allowance() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let initial = BillingStore::open(&path, &network()).unwrap();
    let owner = wallet::address_from_private_key(CHALLENGE_OWNER_KEY).unwrap();
    let terms = quote(1, 1, &owner, "test/challenge-wallet-race", ("0", "0"));
    initial.store_quote(&terms, NOW).unwrap();
    for nonce in 1..=11 {
        let challenge = rate_challenge(&terms.payload, nonce, NOW);
        initial.create_challenge(&challenge, NOW).unwrap();
        consume_rate_challenge(&initial, &challenge, NOW);
    }
    let candidates = [rate_challenge(&terms.payload, 12, NOW), rate_challenge(&terms.payload, 13, NOW)];
    drop(initial);
    // Pending occupancy is zero, so the two writers compete only for the twelfth wallet unit.
    let winner = race_rate_challenges(&path, [&candidates[0], &candidates[1]], NOW + 1);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    assert_challenge_rate_rejected_at(&reopened, &rate_challenge(&terms.payload, 14, NOW), NOW + 1);
    // The race loser and subsequent refusal must leave all 588 remaining global units intact.
    for index in 0..588 {
        let mut challenge = rate_challenge(&terms.payload, 1000 + index, NOW);
        challenge.wallet = format!("0x{:040x}", 0x10000 + index);
        reopened.create_challenge(&challenge, NOW + 1).unwrap();
    }
    let mut excess = rate_challenge(&terms.payload, 2000, NOW);
    excess.wallet = OTHER_PAYER.into();
    assert_challenge_rate_rejected_at(&reopened, &excess, NOW + 1);
    assert_eq!(serde_json::to_value(reopened.challenge(&candidates[winner].nonce).unwrap()).unwrap(),
        serde_json::to_value(&candidates[winner]).unwrap());
    assert!(reopened.challenge(&candidates[1 - winner].nonce).is_err());
    assert_preserved(&reopened, &terms);
}

#[test]
fn challenge_global_rate_race_commits_only_one_candidate_across_connections() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let initial = BillingStore::open(&path, &network()).unwrap();
    let owner = wallet::address_from_private_key(CHALLENGE_OWNER_KEY).unwrap();
    let terms = quote(1, 1, &owner, "test/challenge-global-race", ("0", "0"));
    initial.store_quote(&terms, NOW).unwrap();
    for sequence in 1..=599 {
        let mut challenge = rate_challenge(&terms.payload, sequence, NOW);
        challenge.wallet = format!("0x{:040x}", 0x10000 + sequence);
        initial.create_challenge(&challenge, NOW).unwrap();
    }
    let retained = initial.challenge(&format!("0x{:064x}", 1)).unwrap();
    let first = rate_challenge(&terms.payload, 600, NOW);
    let mut second = rate_challenge(&terms.payload, 601, NOW);
    second.wallet = OTHER_PAYER.into();
    drop(initial);
    // Both wallets have unused individual budgets; 599 pending records cannot reach the 10,000 ceiling.
    race_rate_challenges(&path, [&first, &second], NOW + 1);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    for (index, wallet) in [owner.as_str(), OTHER_PAYER, PAYER].into_iter().enumerate() {
        let mut excess = rate_challenge(&terms.payload, 602 + index as u64, NOW);
        excess.wallet = wallet.into();
        assert_challenge_rate_rejected_at(&reopened, &excess, NOW + 1);
    }
    assert_eq!(serde_json::to_value(reopened.challenge(&retained.nonce).unwrap()).unwrap(),
        serde_json::to_value(&retained).unwrap());
    assert_preserved(&reopened, &terms);
}

#[test]
fn global_rate_budget_survives_expiry_reopening_and_fixed_minute_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let mut ledger = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let provider = wallet::address_from_private_key(&key).unwrap();
    let fresh = |id, issued_at, expires_at| {
        let mut payload = quote_payload(&provider, id, PAYER, "test/payer-rate", ("1", "2"));
        payload.issued_at = issued_at;
        payload.expires_at = expires_at;
        protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap()
    };

    // Starting halfway through a minute distinguishes UTC buckets from elapsed/rolling time.
    // The last phase skips minutes: unused windows must not accumulate extra allowance.
    for (phase, start) in [NOW + 30, NOW + 60, NOW + 5 * 60].into_iter().enumerate() {
        for offset in 0..600 {
            let now = start + offset / 20;
            // One-second fixtures reclaim storage, never committed issuance units.
            let signed = fresh(phase as u64 * 1000 + offset + 1, now, now + 1);
            ledger.store_quote(&signed, now).unwrap();
            assert_preserved(&ledger, &signed);
            if offset == 299 {
                drop(ledger);
                ledger = BillingStore::open(&path, &network()).unwrap();
            }
        }
        let next_minute = (start / 60 + 1) * 60;
        let rejected = fresh(phase as u64 * 1000 + 601, next_minute - 1, next_minute + 1);
        assert_rate_rejected_at(&ledger, &rejected, next_minute - 1);
        drop(ledger);
        ledger = BillingStore::open(&path, &network()).unwrap();
        // A different control budget may observe a future sample, but cannot erase or
        // advance committed-proposal evidence before an older quote admission arrives.
        let challenge = rate_challenge(&rejected.payload, phase as u64 + 1, next_minute + 60);
        ledger.create_challenge(&challenge, next_minute + 60).unwrap();
        assert_rate_rejected_at(&ledger, &rejected, next_minute - 1);
        // The next phase's first committed quote exercises the exact forward boundary,
        // then a later skipped-to window, with all 600 global units available again.
    }
}

#[test]
fn acknowledgement_neither_refunds_nor_charges_the_global_proposal_rate() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let mut ledger = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let payer = wallet::address_from_private_key(&format!("{:064x}", 2)).unwrap();
    let original = quote(1, 1, &payer, "test/rate-acknowledgement", ("1", "2"));
    ledger.store_quote(&original, NOW).unwrap();
    for offset in 1..=598 {
        let now = NOW + offset / 20;
        let mut payload = quote_payload(&original.payload.provider_wallet, offset + 1, &payer,
            "test/rate-acknowledgement", ("1", "2"));
        payload.issued_at = now;
        payload.expires_at = now + 1; // Reclaim storage capacity without crossing a rate minute.
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        ledger.store_quote(&signed, now).unwrap();
    }
    let now = NOW + 30;
    let (raw, envelope) = signed_opening(&original.payload, directory.path());
    let ack = ledger.register_funding_intent(&original, &payer, &raw, &envelope.tx_hash,
        envelope.nonce, &key, || Ok(now), true, &original.payload.model).unwrap();
    protocol::verify(protocol::INTENT_DOMAIN, &ack, &original.payload.provider_wallet).unwrap();

    // 599 committed quotes leave exactly one rate unit, regardless of the transferred slot.
    let last = quote(1, 600, &payer, "test/rate-acknowledgement", ("1", "2"));
    ledger.store_quote(&last, now).unwrap();
    let rejected = quote(1, 601, &payer, "test/rate-acknowledgement", ("1", "2"));
    for restart in [false, true] {
        if restart {
            drop(ledger);
            ledger = BillingStore::open(&path, &network()).unwrap();
        }
        // Exhausting fresh issuance must not prevent acknowledgement recovery.
        let replay = ledger.register_funding_intent(&original, &payer, &raw,
            &envelope.tx_hash, envelope.nonce, &key, || Ok(now + 1), false,
            &original.payload.model).unwrap();
        assert_eq!(replay.payload, ack.payload);
        assert_eq!(replay.signature, ack.signature);
        assert_rate_rejected_at(&ledger, &rejected, now + 1);
        assert_preserved(&ledger, &original);
        assert_preserved(&ledger, &last);
        assert!(ledger.prepare_fee_batches(now + 1, true).unwrap().is_empty());
    }
}

#[test]
fn free_opening_preserves_rate_allowance_and_credentials_at_exhaustion() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let mut ledger = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let original = quote(1, 1, PAYER, "test/rate-opening", ("0", "0"));
    ledger.store_quote(&original, NOW).unwrap();
    for offset in 1..=598 {
        let now = NOW + offset / 20;
        let mut payload = quote_payload(&original.payload.provider_wallet, offset + 1, PAYER,
            "test/rate-opening", ("0", "0"));
        payload.issued_at = now;
        payload.expires_at = now + 1; // Expiry reclaims storage, not issuance units.
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        ledger.store_quote(&signed, now).unwrap();
    }
    let now = NOW + 30;
    // Ledger-call owner authorization is a precondition; free opening needs no chain evidence.
    let opened = ledger.open_account(&original, None, now).unwrap();
    let last = quote(1, 600, PAYER, "test/rate-opening", ("0", "0"));
    ledger.store_quote(&last, now).unwrap();
    let rejected = quote(1, 601, PAYER, "test/rate-opening", ("0", "0"));
    assert_rate_rejected_at(&ledger, &rejected, now);
    // First opening is still allowed when fresh proposal allowance is already exhausted.
    let last_opened = ledger.open_account(&last, None, now).unwrap();

    for restart in [false, true] {
        if restart {
            drop(ledger);
            ledger = BillingStore::open(&path, &network()).unwrap();
        }
        assert_rate_rejected_at(&ledger, &rejected, now + 1);
        for (signed, account) in [(&original, &opened), (&last, &last_opened)] {
            let saved = ledger.quote(&signed.payload.id).unwrap();
            assert_eq!(saved.payload, signed.payload);
            assert_eq!(saved.signature, signed.signature);
            let authenticated = ledger.authenticate(&account.credential.as_ref().unwrap().bearer, now + 1).unwrap();
            assert_eq!(authenticated.credential_id, account.credential.as_ref().unwrap().id);
            assert_eq!(authenticated.account.contract_id, signed.payload.id);
            assert_eq!(authenticated.account.funded_wei, "0");
            assert_eq!(serde_json::to_value(&authenticated.account).unwrap(),
                serde_json::to_value(&account.account).unwrap());
        }
        assert!(ledger.prepare_fee_batches(now + 1, true).unwrap().is_empty());
    }
}

// An independently encoded EIP-155 opening, signed with public test scalar 2.
// Only offline envelope validation runs: no RPC, spendability observation or broadcast.
fn signed_opening(quote: &Quote, directory: &std::path::Path)
    -> (String, ValidatedPaymentEnvelope) {
    use rlp::RlpStream;
    use secp256k1::{Message, Secp256k1, SecretKey};
    use tiny_keccak::{Hasher, Keccak};

    let data = protocol::opening_data(quote).unwrap();
    let recipient = hex::decode(&quote.provider_wallet[2..]).unwrap();
    let append_terms = |rlp: &mut RlpStream| {
        rlp.append(&0u64); // Original operation's nonce.
        rlp.append(&1_000_000_000u64); // Fixture gas price, not a live estimate.
        rlp.append(&100_000u64); // Covers the fixture's intrinsic gas.
        rlp.append(&recipient.as_slice());
        rlp.append(&quote.funding_wei.parse::<u64>().unwrap());
        rlp.append(&data.as_slice());
    };
    let mut unsigned = RlpStream::new_list(9);
    append_terms(&mut unsigned);
    unsigned.append(&quote.network.chain_id).append(&0u8).append(&0u8);
    let mut digest = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(unsigned.as_raw());
    hasher.finalize(&mut digest);
    let key = SecretKey::from_slice(&hex::decode(format!("{:064x}", 2)).unwrap()).unwrap();
    let (recovery, signature) = Secp256k1::new()
        .sign_ecdsa_recoverable(&Message::from_digest_slice(&digest).unwrap(), &key)
        .serialize_compact();
    let mut signed = RlpStream::new_list(9);
    append_terms(&mut signed);
    signed.append(&(quote.network.chain_id * 2 + 35 + recovery.to_i32() as u64));
    for scalar in signature.chunks_exact(32) {
        let first = scalar.iter().position(|byte| *byte != 0).unwrap();
        signed.append(&&scalar[first..]);
    }
    let raw = format!("0x{}", hex::encode(signed.out()));
    let chain = ChainClient::new(NetworkConfig {
        identity: quote.network.clone(),
        rpc_url: "http://127.0.0.1:9".into(), // Unused by offline validation.
        rpc_bearer_file: None, faucet_url: None, explorer_url: None, policy_url: None,
    }, directory.to_path_buf()).unwrap();
    let envelope = chain.validate_payment_envelope(&raw, &quote.consumer_wallet,
        &quote.provider_wallet, &quote.funding_wei, &data).unwrap();
    assert_eq!(envelope.nonce, 0);
    (raw, envelope)
}

#[test]
fn acknowledgement_frees_exactly_one_slot_and_replay_preserves_its_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let mut ledger = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let payer = wallet::address_from_private_key(&format!("{:064x}", 2)).unwrap();
    let original_quote = quote(1, 1, &payer, "test/acknowledgement", ("1", "2"));
    ledger.store_quote(&original_quote, NOW).unwrap();
    let mut pending = restore_standalone(&path, 9_999, NOW + 300);
    pending.insert(0, original_quote);
    let replacement = quote(1, 9, &payer, "test/replacement", ("0", "0"));
    let excess = quote(1, 10, &payer, "test/excess", ("1", "2"));
    assert_capacity_rejected(&ledger, &replacement);

    let original = &pending[0];
    let (raw, envelope) = signed_opening(&original.payload, directory.path());
    let ack = ledger.register_funding_intent(original, &payer, &raw, &envelope.tx_hash,
        envelope.nonce, &key, || Ok(NOW), true, &original.payload.model).unwrap();
    protocol::verify(protocol::INTENT_DOMAIN, &ack, &original.payload.provider_wallet).unwrap();
    assert_eq!(ack.payload.quote_id, original.payload.id);
    assert_eq!(ack.payload.tx_hash, envelope.tx_hash);
    ledger.store_quote(&replacement, NOW).unwrap();
    assert_capacity_rejected(&ledger, &excess);

    for restart in [false, true] {
        if restart {
            drop(ledger);
            ledger = BillingStore::open(&path, &network()).unwrap();
        }
        // Recovery remains available when fresh funding is disabled. Its later request
        // time must not replace the original signed acknowledgement or consume a slot.
        let replay = ledger.register_funding_intent(original, &payer, &raw,
            &envelope.tx_hash, envelope.nonce, &key, || Ok(NOW + 1), false,
            &original.payload.model).unwrap();
        assert_eq!(replay.payload, ack.payload);
        assert_eq!(replay.signature, ack.signature);
        let recovered = ledger.funding_intent(&original.payload, &raw).unwrap().unwrap();
        assert_eq!(recovered.payload, ack.payload);
        assert_eq!(recovered.signature, ack.signature);
        for signed in &pending { assert_preserved(&ledger, signed); }
        assert_preserved(&ledger, &replacement);
        assert_capacity_rejected(&ledger, &excess);
        assert!(ledger.prepare_fee_batches(NOW + 1, true).unwrap().is_empty());
    }

    // Standalone cleanup and lease expiry cannot discard an accepted original or its nonce.
    let expiry = original.payload.expires_at;
    let mut payload = quote_payload(&original.payload.provider_wallet, 11, &payer,
        "test/after-acknowledgement-expiry", ("1", "2"));
    payload.issued_at = expiry;
    payload.expires_at = expiry + 300;
    let fresh = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
    ledger.store_quote(&fresh, expiry).unwrap();
    drop(ledger);
    ledger = BillingStore::open(&path, &network()).unwrap();
    let replay = ledger.register_funding_intent(original, &payer, &raw,
        &envelope.tx_hash, envelope.nonce, &key, || Ok(expiry), false,
        &original.payload.model).unwrap();
    assert_eq!(replay.payload, ack.payload);
    assert_eq!(replay.signature, ack.signature);
    let (conflicting_raw, conflicting_envelope) = signed_opening(&fresh.payload, directory.path());
    assert!(ledger.register_funding_intent(&fresh, &payer, &conflicting_raw,
        &conflicting_envelope.tx_hash, conflicting_envelope.nonce, &key, || Ok(expiry),
        true, &fresh.payload.model).is_err());
    assert!(ledger.funding_intent(&fresh.payload, &conflicting_raw).unwrap().is_none());
    let recovered = ledger.funding_intent(&original.payload, &raw).unwrap().unwrap();
    assert_eq!(recovered.payload, ack.payload);
    assert_eq!(recovered.signature, ack.signature);
    assert_preserved(&ledger, original);
    assert_preserved(&ledger, &fresh);
    assert!(ledger.prepare_fee_batches(expiry, true).unwrap().is_empty());
}

#[test]
fn rejected_acknowledgements_preserve_pending_capacity_and_allow_original_registration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let mut ledger = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let wrong_key = format!("{:064x}", 3);
    let payer = wallet::address_from_private_key(&format!("{:064x}", 2)).unwrap();
    let original_quote = quote(1, 1, &payer, "test/rejection", ("1", "2"));
    ledger.store_quote(&original_quote, NOW).unwrap();
    let mut pending = restore_standalone(&path, 9_999, NOW + 300);
    pending.insert(0, original_quote);
    let original = &pending[0];
    let replacement = quote(1, 9, &payer, "test/replacement", ("0", "0"));
    let (raw, envelope) = signed_opening(&original.payload, directory.path());
    // Exercise both early admission rejection and failure to authenticate the provider's
    // newly signed acknowledgement, after immutable terms and nonce checks have passed.
    for (accepting, signing_key) in [(false, &key), (true, &wrong_key)] {
        assert!(ledger.register_funding_intent(original, &payer, &raw, &envelope.tx_hash,
            envelope.nonce, signing_key, || Ok(NOW), accepting, &original.payload.model).is_err());
        for signed in &pending { assert_preserved(&ledger, signed); }
        assert!(ledger.funding_intent(&original.payload, &raw).unwrap().is_none());
        assert_capacity_rejected(&ledger, &replacement);
        assert!(ledger.prepare_fee_batches(NOW, true).unwrap().is_empty());
        drop(ledger);
        ledger = BillingStore::open(&path, &network()).unwrap();
        for signed in &pending { assert_preserved(&ledger, signed); }
        assert!(ledger.funding_intent(&original.payload, &raw).unwrap().is_none());
        assert_capacity_rejected(&ledger, &replacement);
    }
    // Failed registration must not poison this original transaction or sender/nonce.
    let ack = ledger.register_funding_intent(original, &payer, &raw, &envelope.tx_hash,
        envelope.nonce, &key, || Ok(NOW), true, &original.payload.model).unwrap();
    protocol::verify(protocol::INTENT_DOMAIN, &ack, &original.payload.provider_wallet).unwrap();
    ledger.store_quote(&replacement, NOW).unwrap();
    assert_capacity_rejected(&ledger,
        &quote(1, 10, &payer, "test/excess", ("1", "2")));
    for signed in &pending { assert_preserved(&ledger, signed); }
    assert_preserved(&ledger, &replacement);
    assert!(ledger.prepare_fee_batches(NOW, true).unwrap().is_empty());
}

#[test]
fn unpaid_first_opening_preserves_pending_capacity_and_signed_terms_after_reopening() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let mut ledger = BillingStore::open(&path, &network()).unwrap();
    let original = quote(1, 1, PAYER, "test/unpaid", ("1", "2"));
    ledger.store_quote(&original, NOW).unwrap();
    let mut pending = restore_standalone(&path, 9_999, NOW + 300);
    pending.insert(0, original);
    let replacement = quote(1, 9, PAYER, "test/replacement", ("0", "0"));
    assert_capacity_rejected(&ledger, &replacement);

    // Owner authorization is a ledger-call precondition; no payment evidence is invented.
    let error = ledger.open_account(&pending[0], None, NOW).unwrap_err();
    assert!(error.starts_with("invalid_payment:"), "{error}");
    for restart in [false, true] {
        if restart {
            drop(ledger);
            ledger = BillingStore::open(&path, &network()).unwrap();
        }
        for signed in &pending { assert_preserved(&ledger, signed); }
        assert_capacity_rejected(&ledger, &replacement);
        assert!(ledger.prepare_fee_batches(NOW, true).unwrap().is_empty());
    }
}

#[test]
fn free_first_opening_frees_one_slot_and_survives_exact_expiry_with_original_credential() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let mut ledger = BillingStore::open(&path, &network()).unwrap();
    let original_quote = quote(1, 1, PAYER, "test/opening", ("0", "0"));
    ledger.store_quote(&original_quote, NOW).unwrap();
    let mut pending = restore_standalone(&path, 9_999, NOW + 300);
    pending.insert(0, original_quote);
    let original = &pending[0];
    let replacement = quote(1, 9, PAYER, "test/replacement", ("1", "2"));
    assert_capacity_rejected(&ledger, &replacement);

    let opened = ledger.open_account(original, None, NOW).unwrap();
    assert_eq!(opened.account.contract_id, original.payload.id);
    assert_eq!(opened.account.model, original.payload.model);
    assert_eq!(opened.account.price, original.payload.price);
    for amount in [&opened.account.funded_wei, &opened.account.fee_wei,
        &opened.account.spent_wei, &opened.account.reserved_wei,
        &opened.account.available_wei, &opened.account.reversed_wei,
        &opened.account.provider_loss_wei, &opened.account.waived_wei] {
        assert_eq!(amount, "0");
    }
    let expected_account = serde_json::to_value(&opened.account).unwrap();
    let original_scopes = ledger.authenticate(&opened.credential.as_ref().unwrap().bearer, NOW).unwrap().scopes;
    let assert_recovery = |store: &BillingStore, now| {
        let saved = store.quote(&original.payload.id).unwrap();
        assert_eq!(saved.payload, original.payload);
        assert_eq!(saved.signature, original.signature);
        let authenticated = store.authenticate(&opened.credential.as_ref().unwrap().bearer, now).unwrap();
        assert_eq!(authenticated.credential_id, opened.credential.as_ref().unwrap().id);
        assert_eq!(authenticated.scopes, original_scopes);
        assert_eq!(serde_json::to_value(authenticated.account).unwrap(), expected_account);
        assert!(store.prepare_fee_batches(now, true).unwrap().is_empty());
    };
    ledger.store_quote(&replacement, NOW).unwrap();
    assert_capacity_rejected(&ledger, &quote(1, 10, PAYER, "test/excess", ("1", "2")));
    for signed in pending.iter().skip(1) { assert_preserved(&ledger, signed); }
    assert_preserved(&ledger, &replacement);
    assert_recovery(&ledger, NOW);

    // Reopen storage and recover using the original exported credential.
    drop(ledger);
    ledger = BillingStore::open(&path, &network()).unwrap();
    assert_recovery(&ledger, NOW + 1);
    let expiry = original.payload.expires_at;
    let provider_key = format!("{:064x}", 1);
    let fresh_quote = |id, issued_at| {
        let mut payload = quote_payload(&original.payload.provider_wallet, id, PAYER,
            "test/cleanup", ("1", "2"));
        payload.issued_at = issued_at;
        payload.expires_at = issued_at + 300;
        protocol::sign(QUOTE_DOMAIN, payload, &provider_key).unwrap()
    };
    let fresh = fresh_quote(11, expiry - 1);
    let error = ledger.store_quote(&fresh, expiry - 1).unwrap_err();
    assert!(error.starts_with("rate_limit:"), "{error}");
    assert!(ledger.quote(&fresh.payload.id).unwrap_err().starts_with("not_found:"));
    for signed in pending.iter().skip(1) { assert_preserved(&ledger, signed); }
    assert_preserved(&ledger, &replacement);
    assert_recovery(&ledger, expiry - 1);

    // Expiry may exclude or delete standalone records. Assert reusable capacity, not deletion.
    ledger.store_quote(&fresh, expiry).unwrap();
    let mut renewed = Vec::with_capacity(600);
    renewed.push(fresh);
    for id in 12..=610 {
        let signed = fresh_quote(id, expiry);
        ledger.store_quote(&signed, expiry).unwrap();
        renewed.push(signed);
    }
    // Reclaimed storage admits 600 actual insertions; issuance, not storage, now rejects 601.
    let excess = fresh_quote(611, expiry);
    let error = ledger.store_quote(&excess, expiry).unwrap_err();
    assert!(error.starts_with("rate_limit:"), "{error}");
    assert!(ledger.quote(&excess.payload.id).unwrap_err().starts_with("not_found:"));
    for expired in pending.iter().skip(1).chain(std::iter::once(&replacement)) {
        assert!(ledger.account(&expired.payload.id).unwrap_err().starts_with("not_found:"));
    }
    for signed in &renewed { assert_preserved(&ledger, signed); }
    assert_recovery(&ledger, expiry);
    drop(ledger);
    ledger = BillingStore::open(&path, &network()).unwrap();
    for signed in &renewed { assert_preserved(&ledger, signed); }
    let error = ledger.store_quote(&excess, expiry + 1).unwrap_err();
    assert!(error.starts_with("rate_limit:"), "{error}");
    assert!(ledger.quote(&excess.payload.id).unwrap_err().starts_with("not_found:"));
    assert_recovery(&ledger, expiry + 1);
}
