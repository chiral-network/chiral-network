//! Explicit-time contracts for pending-proposal quotas and acknowledgement slot transfer.
//! Ledger tests assume prior owner/spendability admission; offline signatures are not settlement.
use chiral_network::{
    billing_store::BillingStore,
    chain_client::{ChainClient, ValidatedPaymentEnvelope},
    network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
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

#[test]
fn competing_connections_admit_only_one_quote_into_the_final_payer_slot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let initial = BillingStore::open(&path, &network()).unwrap();
    let mut accepted = Vec::with_capacity(7);
    for sequence in 1..=7 {
        let signed = quote(1, sequence, PAYER, "test/race", ("1", "2"));
        initial.store_quote(&signed, NOW).unwrap();
        accepted.push(signed);
    }
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

fn rejection_control_pair() -> (BillingStore, BillingStore, u64) {
    let control = BillingStore::open(std::path::Path::new(":memory:"), &network()).unwrap();
    let exercised = BillingStore::open(std::path::Path::new(":memory:"), &network()).unwrap();
    let key = format!("{:064x}", 1);
    let provider = wallet::address_from_private_key(&key).unwrap();
    let reject_at = NOW + 8 * 60;
    let mut accepted = Vec::with_capacity(8);
    for index in 0..8 {
        let mut payload = quote_payload(&provider, index + 1, PAYER, "test/rollback", ("1", "2"));
        // Storage-only lifetimes keep eight pending records across distinct rate windows.
        // No successful proposal has consumed allowance in the later rejection window.
        payload.issued_at = NOW + index * 60;
        payload.expires_at = reject_at + 1;
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        control.store_quote(&signed, signed.payload.issued_at).unwrap();
        exercised.store_quote(&signed, signed.payload.issued_at).unwrap();
        accepted.push(signed);
    }
    let mut payload = quote_payload(&provider, 100, PAYER, "test/rollback", ("1", "2"));
    payload.issued_at = reject_at;
    payload.expires_at = reject_at + 300;
    let rejected = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
    for _ in 0..1024 {
        let error = exercised.store_quote(&rejected, reject_at).unwrap_err();
        assert!(error.starts_with("rate_limit:"), "{error}");
    }
    assert!(exercised.quote(&rejected.payload.id).unwrap_err().starts_with("not_found:"));
    for signed in &accepted {
        assert_preserved(&control, signed);
        assert_preserved(&exercised, signed);
    }
    (control, exercised, reject_at + 1)
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
fn rejected_pending_proposals_preserve_the_payers_rate_allowance() {
    let (control, exercised, start) = rejection_control_pair();
    let key = format!("{:064x}", 1);
    let provider = wallet::address_from_private_key(&key).unwrap();
    let mut admitted = 0;
    // All probes stay in the rejection window. One-second storage fixtures reclaim pending
    // slots between probes, so the eight-slot cap cannot hide a lost rate allowance.
    for offset in 0..59 {
        let now = start + offset;
        let mut payload = quote_payload(&provider, 200 + offset, PAYER, "test/rollback", ("1", "2"));
        payload.issued_at = now;
        payload.expires_at = now + 1;
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        if !matching_admission(&control, &exercised, &signed, now) { break; }
        admitted += 1;
    }
    assert!(admitted > 0, "the control must have fresh-window rate allowance");
}

#[test]
fn rejected_pending_proposals_preserve_other_payers_shared_rate_allowance() {
    let (control, exercised, now) = rejection_control_pair();
    let key = format!("{:064x}", 1);
    let provider = wallet::address_from_private_key(&key).unwrap();
    let mut admitted = 0;
    let mut refused = false;
    // Distinct payers avoid individual caps. The pending ceiling bounds this trace even if
    // the separate rate policy changes; no numerical rate limit is asserted here.
    for index in 0..=10_000 {
        let payer = format!("0x{:040x}", 0x10000 + index);
        let mut payload = quote_payload(&provider, 200 + index, &payer, "test/rollback", ("1", "2"));
        payload.issued_at = now;
        payload.expires_at = now + 300;
        let signed = protocol::sign(QUOTE_DOMAIN, payload, &key).unwrap();
        if !matching_admission(&control, &exercised, &signed, now) {
            refused = true;
            break;
        }
        admitted += 1;
    }
    assert!(admitted > 0, "the control must admit another payer");
    assert!(refused, "the control must remain bounded");
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
    let pending: Vec<_> = (1..=8)
        .map(|id| quote(1, id, &payer, "test/acknowledgement", ("1", "2"))).collect();
    for signed in &pending { ledger.store_quote(signed, NOW).unwrap(); }
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
}

#[test]
fn rejected_acknowledgements_preserve_pending_capacity_and_allow_original_registration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("billing.sqlite");
    let mut ledger = BillingStore::open(&path, &network()).unwrap();
    let key = format!("{:064x}", 1);
    let wrong_key = format!("{:064x}", 3);
    let payer = wallet::address_from_private_key(&format!("{:064x}", 2)).unwrap();
    let pending: Vec<_> = (1..=8)
        .map(|id| quote(1, id, &payer, "test/rejection", ("1", "2"))).collect();
    for signed in &pending { ledger.store_quote(signed, NOW).unwrap(); }
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
    let pending: Vec<_> = (1..=8)
        .map(|id| quote(1, id, PAYER, "test/unpaid", ("1", "2"))).collect();
    for signed in &pending { ledger.store_quote(signed, NOW).unwrap(); }
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
    let pending: Vec<_> = (1..=8).map(|id| {
        let rates = if id == 1 { ("0", "0") } else { ("1", "2") };
        quote(1, id, PAYER, "test/opening", rates)
    }).collect();
    for signed in &pending { ledger.store_quote(signed, NOW).unwrap(); }
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
    let original_scopes = ledger.authenticate(&opened.credential.bearer, NOW).unwrap().scopes;
    let assert_recovery = |store: &BillingStore, now| {
        let saved = store.quote(&original.payload.id).unwrap();
        assert_eq!(saved.payload, original.payload);
        assert_eq!(saved.signature, original.signature);
        let authenticated = store.authenticate(&opened.credential.bearer, now).unwrap();
        assert_eq!(authenticated.credential_id, opened.credential.id);
        assert_eq!(authenticated.scopes, original_scopes);
        assert_eq!(serde_json::to_value(authenticated.account).unwrap(), expected_account);
        assert!(store.prepare_fee_batches(now, true).unwrap().is_empty());
    };
    ledger.store_quote(&replacement, NOW).unwrap();
    assert_capacity_rejected(&ledger, &quote(1, 10, PAYER, "test/excess", ("1", "2")));
    for signed in pending.iter().skip(1) { assert_preserved(&ledger, signed); }
    assert_preserved(&ledger, &replacement);
    assert_recovery(&ledger, NOW);

    // Reopen storage, not the account: another owner-authorized open command rotates its root.
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
    let mut renewed = Vec::with_capacity(8);
    renewed.push(fresh);
    for id in 12..=18 {
        let signed = fresh_quote(id, expiry);
        ledger.store_quote(&signed, expiry).unwrap();
        renewed.push(signed);
    }
    let excess = fresh_quote(19, expiry);
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
