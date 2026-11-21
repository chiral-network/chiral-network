//! Deterministic tests of the ledger AFTER the chain verification boundary.
//! VerifiedPayment values here are unit-test inputs, not chain qualification.
use super::*;
use std::sync::{Arc, Barrier};

const PROVIDER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const OWNER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000002";
fn network() -> NetworkIdentity {
    NetworkIdentity {
        chain_id: crate::network::ESPACE_TESTNET_CHAIN_ID,
        genesis_hash: crate::network::ESPACE_TESTNET_GENESIS.into(),
        deployment_id: "ledger-unit-tests".into(),
    }
}
fn native_fixture_gas(data: &[u8]) -> u64 {
    21000 + data.iter().map(|byte| if *byte == 0 { 10 } else { 40 }).sum::<u64>()
}
fn quote(funding: u128, free: bool) -> Signed<Quote> {
    let q = Quote {
        id: protocol::random_id(),
        network: network(),
        offer_id: protocol::random_id(),
        provider_wallet: crate::wallet::address_from_private_key(PROVIDER_KEY).unwrap(),
        consumer_wallet: crate::wallet::address_from_private_key(OWNER_KEY).unwrap(),
        model: ModelSpec {
            id: "test/model".into(),
            revision: "sha256:test-revision".into(),
            tokenizer_revision: "test-tokenizer".into(),
            chat_template_sha256: "ab".repeat(32),
            quantization: None,
            context_length: 8192,
            max_output_tokens: 4096,
            capabilities: vec!["chat".into()],
            serving: None,
        },
        price: PriceSchedule {
            input_wei_per_1k: if free { "0" } else { "1000" }.into(),
            output_wei_per_1k: if free { "0" } else { "1000" }.into(),
        },
        funding_wei: funding.to_string(),
        platform_wallet: format!("0x{}", "33".repeat(20)),
        fee_bps: 50,
        max_concurrency: 10,
        nonce: protocol::random_id(),
        issued_at: 100,
        expires_at: 400,
    };
    protocol::sign(protocol::QUOTE_DOMAIN, q, PROVIDER_KEY).unwrap()
}
fn payment(q: &Quote, opening: bool, value: u128, tag: u8) -> VerifiedPayment {
    let raw = format!("0x{tag:02x}");
    VerifiedPayment {
        network: q.network.clone(),
        tx_hash: protocol::hash_hex(&[tag]),
        from: q.consumer_wallet.clone(),
        to: q.provider_wallet.clone(),
        value_wei: value.to_string(),
        data: if opening {
            protocol::opening_data(q).unwrap()
        } else {
            protocol::topup_data(&q.id).unwrap()
        },
        block_hash: format!("0x{}", "44".repeat(32)),
        block_number: 10,
        raw_transaction: raw,
    }
}
fn register(
    store: &BillingStore,
    q: &Signed<Quote>,
    p: &VerifiedPayment,
    now: u64,
) -> Signed<FundingIntent> {
    let nonce = if p.raw_transaction.len() == 4 {
        u64::from_str_radix(&p.raw_transaction[2..], 16).unwrap()
    } else {
        crate::chain_client::payment_sender_nonce(&p.raw_transaction, q.payload.network.chain_id)
            .unwrap()
            .1
    };
    store.register_funding_intent(q, &q.payload.consumer_wallet, &p.raw_transaction, &p.tx_hash, nonce, PROVIDER_KEY, || Ok(now), true, &(q).payload.model)
        .unwrap()
}
fn paid(store: &BillingStore) -> (Signed<Quote>, VerifiedPayment, crate::protocol::OpenResult) {
    let quote = quote(1000, false);
    store.store_quote(&quote, 100).unwrap();
    let payment = payment(&quote.payload, true, 1000, 1);
    register(store, &quote, &payment, 100);
    let opened = store.open_account(&quote, Some(&payment), 101).unwrap();
    (quote, payment, opened)
}
fn memory() -> BillingStore {
    BillingStore::open(Path::new(":memory:"), &network()).unwrap()
}
fn app_credential(
    store: &BillingStore,
    id: &str,
    cap: Option<&str>,
    scopes: Vec<String>,
) -> Credential {
    store
        .issue_credential(
            id,
            &CredentialIssueRequest {
                expires_at: 1000,
                spend_cap_wei: cap.map(str::to_string),
                max_concurrency: 2,
                scopes,
            },
            102,
        )
        .unwrap()
}
fn active_intents(store: &BillingStore, now: u64) -> u64 {
    store.db.lock().unwrap().query_row("SELECT COUNT(*) FROM funding_intents WHERE admission_until>?1 AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract)",[now],|r|r.get(0)).unwrap()
}

fn remitted_batch(store: &BillingStore, q: &Quote, now: u64) -> FeeBatch {
    let mut batch = store.prepare_fee_batches(now, true).unwrap().remove(0);
    let raw = "0xab".to_string();
    let hash = protocol::hash_hex(&[0xab]);
    batch.tx_hash = Some(hash.clone());
    batch.raw_transaction = Some(raw.clone());
    batch.status = "accepted".into();
    batch.evidence = Some(VerifiedPayment {
        network: network(),
        tx_hash: hash,
        from: q.provider_wallet.clone(),
        to: batch.platform_wallet.clone(),
        value_wei: batch.amount_wei.clone(),
        data: batch.payment_data().unwrap(),
        block_hash: format!("0x{}", "55".repeat(32)),
        block_number: 100,
        raw_transaction: raw,
    });
    store.update_fee_batch(&batch).unwrap();
    batch
}

#[test]
fn full_width_money_rounds_once_without_float_or_sqlite_integer() {
    let price = PriceSchedule {
        input_wei_per_1k: u128::MAX.to_string(),
        output_wei_per_1k: "0".into(),
    };
    assert_eq!(
        usage_cost(
            &price,
            &TokenUsage {
                prompt_tokens: 1000,
                completion_tokens: 0
            }
        )
        .unwrap(),
        u128::MAX
    );
    assert!(usage_cost(
        &price,
        &TokenUsage {
            prompt_tokens: 1001,
            completion_tokens: 0
        }
    )
    .is_err());
    let price = PriceSchedule {
        input_wei_per_1k: "1".into(),
        output_wei_per_1k: "1".into(),
    };
    assert_eq!(
        usage_cost(
            &price,
            &TokenUsage {
                prompt_tokens: 1,
                completion_tokens: 1
            }
        )
        .unwrap(),
        1
    );
    assert_eq!(fee_cost(199, 50).unwrap(), 0);
    assert_eq!(fee_cost(200, 50).unwrap(), 1);
    assert!(money("00").is_err());
    assert!(money("-1").is_err());
    let store = memory();
    let q = quote(u128::MAX, false);
    store.store_quote(&q, 100).unwrap();
    let p = payment(&q.payload, true, u128::MAX, 1);
    register(&store, &q, &p, 100);
    let open = store.open_account(&q, Some(&p), 101).unwrap();
    assert_eq!(open.account.funded_wei, u128::MAX.to_string());
    assert_eq!(
        money(&open.account.available_wei).unwrap() + money(&open.account.fee_wei).unwrap(),
        u128::MAX
    );
}

#[test]
fn concurrent_connections_cannot_overspend_one_balance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, open) = paid(&store);
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|i| {
            let ledger = BillingStore::open(&path, &network()).unwrap();
            let barrier = barrier.clone();
            let id = q.payload.id.clone();
            let credential = open.credential.as_ref().unwrap().id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                ledger.reserve(&id, &credential, &format!("request-{i}"), 300, 400, 110, &ledger.account(&id).unwrap().model)
            })
        })
        .collect();
    let outcomes: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(outcomes.iter().any(|r| r
        .as_ref()
        .err()
        .is_some_and(|e| e.starts_with("insufficient_funds:"))));
    let account = store.account(&q.payload.id).unwrap();
    assert_eq!(account.reserved_wei, "700");
    assert_eq!(account.available_wei, "295");
}

#[test]
fn open_replay_preserves_credentials_holds_and_spend_without_crediting_twice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, open) = paid(&store);
    let root = open.credential.unwrap();
    let spent = store.reserve(&q.payload.id, &root.id, "spent-before-replay", 100, 100, 110, &q.payload.model).unwrap();
    store.settle(&spent.id, &TokenUsage { prompt_tokens: 100, completion_tokens: 50 }, "completed", 111).unwrap();
    let held = store.reserve(&q.payload.id, &root.id, "held-through-replay", 100, 100, 112, &q.payload.model).unwrap();
    let app = app_credential(&store, &q.payload.id, Some("300"), vec!["read".into()]);
    let account = encode(&store.account(&q.payload.id).unwrap()).unwrap();
    let credentials = encode(&store.credentials(&q.payload.id).unwrap()).unwrap();
    let funding = encode(&store.funding_history(&q.payload.id).unwrap()).unwrap();
    let batches = store.prepare_fee_batches(113, true).unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].amount_wei, "5");
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    for now in [500, 501] {
        // Quote/admission expiry and reopening the database are not credential mutations.
        let reopened = store.open_account(&q, None, now).unwrap();
        assert!(reopened.credential.is_none());
        let document = serde_json::to_value(&reopened).unwrap();
        assert_eq!(document["credential"], serde_json::Value::Null);
        assert!(!document.to_string().contains(&root.bearer));
        assert_eq!(encode(&reopened.account).unwrap(), account);
        assert_eq!(reopened.account.funded_wei, "1000");
        assert_eq!(reopened.account.fee_wei, "5");
        assert_eq!(reopened.account.spent_wei, "150");
        assert_eq!(reopened.account.reserved_wei, "200");
        assert_eq!(reopened.account.available_wei, "645");
        assert_eq!(encode(&store.credentials(&q.payload.id).unwrap()).unwrap(), credentials);
        assert_eq!(encode(&store.funding_history(&q.payload.id).unwrap()).unwrap(), funding);
        assert_eq!(store.funding_history(&q.payload.id).unwrap().len(), 1);
        assert_eq!(encode(&store.prepare_fee_batches(now, true).unwrap()).unwrap(), encode(&batches).unwrap());
        assert_eq!(store.authenticate(&root.bearer, now).unwrap().credential_id, root.id);
        assert!(store.authenticate(&app.bearer, now).is_ok());
        assert_eq!(store.request_view(&q.payload.id, &held.id).unwrap().status, "reserved");
    }
    let options = CredentialIssueRequest {
        expires_at: 1000, spend_cap_wei: None, max_concurrency: 2,
        scopes: vec!["infer".into(), "read".into()],
    };
    let issued = store.issue_credential(&q.payload.id, &options, 502).unwrap();
    assert!(store.authenticate(&root.bearer, 502).is_ok());
    assert!(store.authenticate(&issued.bearer, 502).is_ok());
    let rotated = store.rotate_credential(&q.payload.id, &root.id, &options, 503).unwrap();
    assert!(rotated.bearer != root.bearer);
    assert!(store.authenticate(&root.bearer, 503).is_err());
    assert!(store.authenticate(&rotated.bearer, 503).is_ok());
    assert!(store.authenticate(&issued.bearer, 503).is_ok());
    assert_eq!(encode(&store.account(&q.payload.id).unwrap()).unwrap(), account);
    assert!(store.open_account(&q, None, 504).unwrap().credential.is_none());
    assert!(store.authenticate(&root.bearer, 504).is_err());
    assert!(store.authenticate(&rotated.bearer, 504).is_ok());
    let settled = store.settle(&held.id, &TokenUsage { prompt_tokens: 100, completion_tokens: 50 }, "completed", 505).unwrap();
    assert_eq!(settled.cost_wei, "150");
    assert_eq!(store.account(&q.payload.id).unwrap().reserved_wei, "0");
    assert_eq!(store.account(&q.payload.id).unwrap().spent_wei, "300");
}

#[test]
fn open_replay_never_revives_revoked_or_expired_roots() {
    for revoked in [false, true] {
        let store = memory();
        let (q, _, opened) = paid(&store);
        let root = opened.credential.unwrap();
        let now = if revoked {
            store.revoke_credential(&q.payload.id, &root.id).unwrap();
            500
        } else {
            root.expires_at
        };
        assert!(store.authenticate(&root.bearer, now).is_err());
        let credentials = encode(&store.credentials(&q.payload.id).unwrap()).unwrap();
        let funding = encode(&store.funding_history(&q.payload.id).unwrap()).unwrap();
        for retry in [now, now + 1] {
            let reopened = store.open_account(&q, None, retry).unwrap();
            assert!(reopened.credential.is_none());
            assert_eq!(encode(&reopened.account).unwrap(), encode(&opened.account).unwrap());
            assert_eq!(encode(&store.credentials(&q.payload.id).unwrap()).unwrap(), credentials);
            assert_eq!(encode(&store.funding_history(&q.payload.id).unwrap()).unwrap(), funding);
            assert!(store.authenticate(&root.bearer, retry).is_err());
        }
        let recovered = store.issue_credential(&q.payload.id, &CredentialIssueRequest {
            expires_at: now + 3600, spend_cap_wei: None, max_concurrency: 1,
            scopes: vec!["read".into()],
        }, now + 2).unwrap();
        assert!(store.authenticate(&recovered.bearer, now + 2).is_ok());
        assert!(store.authenticate(&root.bearer, now + 2).is_err());
    }
}

#[test]
fn concurrent_openings_issue_only_one_initial_secret_and_credit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let q = quote(1000, false);
    store.store_quote(&q, 100).unwrap();
    let payment = payment(&q.payload, true, 1000, 1);
    register(&store, &q, &payment, 100);
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2).map(|_| {
        let ledger = BillingStore::open(&path, &network()).unwrap();
        let q = q.clone();
        let payment = payment.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            ledger.open_account(&q, Some(&payment), 101).unwrap()
        })
    }).collect();
    let results: Vec<_> = workers.into_iter().map(|worker| worker.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.credential.is_some()).count(), 1);
    assert_eq!(encode(&results[0].account).unwrap(), encode(&results[1].account).unwrap());
    let root = results.iter().find_map(|result| result.credential.as_ref()).unwrap();
    assert!(store.authenticate(&root.bearer, 102).is_ok());
    assert_eq!(store.credentials(&q.payload.id).unwrap().len(), 1);
    assert_eq!(store.funding_history(&q.payload.id).unwrap().len(), 1);
    let account = store.account(&q.payload.id).unwrap();
    assert_eq!(account.funded_wei, "1000");
    assert_eq!(account.fee_wei, "5");
    assert_eq!(account.available_wei, "995");
    let batches = store.prepare_fee_batches(102, true).unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].amount_wei, "5");
}

#[test]
fn topup_and_cross_account_replays_never_create_second_credit() {
    let store = memory();
    let (q, _, _) = paid(&store);
    let topup = payment(&q.payload, false, 1000, 2);
    assert_eq!(
        store
            .credit_topup(&q.payload.id, &topup, 110)
            .unwrap()
            .funded_wei,
        "2000"
    );
    assert_eq!(
        store
            .credit_topup(&q.payload.id, &topup, 111)
            .unwrap()
            .funded_wei,
        "2000"
    );
    let other = quote(1000, false);
    store.store_quote(&other, 112).unwrap();
    let reused = payment(&other.payload, true, 1000, 2);
    assert!(store
        .register_funding_intent(
            &other,
            &other.payload.consumer_wallet,
            &reused.raw_transaction,
            &reused.tx_hash,
            2,
            PROVIDER_KEY,
            || Ok(113),
            true,
            &other.payload.model
        )
        .unwrap_err()
        .starts_with("funding_replay:"));
    assert!(store.account(&other.payload.id).is_err());
}

#[test]
fn wrong_payment_dimensions_leave_no_account_or_credential() {
    let store = memory();
    let q = quote(1000, false);
    store.store_quote(&q, 100).unwrap();
    let original = payment(&q.payload, true, 1000, 1);
    register(&store, &q, &original, 100);
    let mut variants = Vec::new();
    let mut p = original.clone();
    p.network.deployment_id.push('x');
    variants.push(p);
    let mut p = original.clone();
    p.from = p.to.clone();
    variants.push(p);
    let mut p = original.clone();
    p.to = p.from.clone();
    variants.push(p);
    let mut p = original.clone();
    p.value_wei = "999".into();
    variants.push(p);
    let mut p = original;
    p.data.push(0);
    variants.push(p);
    for p in variants {
        assert!(store.open_account(&q, Some(&p), 101).is_err());
        assert!(store.account(&q.payload.id).is_err());
    }
}

#[test]
fn request_ids_are_scoped_and_terminal_usage_is_idempotent() {
    let store = memory();
    let (q, _, open) = paid(&store);
    let r = store.reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "same-key", 100, 200, 110, &store.account(&q.payload.id).unwrap().model)
        .unwrap();
    assert!(store
        .reserve(
            &q.payload.id,
            &open.credential.as_ref().unwrap().id,
            "same-key",
            100,
            200,
            110,
            &q.payload.model
        )
        .unwrap_err()
        .starts_with("duplicate_request:"));
    let usage = TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
    };
    let settled = store.settle(&r.id, &usage, "completed", 111).unwrap();
    assert_eq!(settled.cost_wei, "150");
    let replay = store
        .settle(
            &r.id,
            &TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 200,
            },
            "completed",
            112,
        )
        .unwrap();
    assert_eq!(replay.cost_wei, "150");
    assert_eq!(store.account(&q.payload.id).unwrap().available_wei, "845");
    let q2 = quote(0, true);
    store.store_quote(&q2, 113).unwrap();
    let o2 = store.open_account(&q2, None, 114).unwrap();
    assert!(store
        .reserve(&q2.payload.id, &o2.credential.as_ref().unwrap().id, "same-key", 100, 200, 115, &q2.payload.model)
        .is_ok());
    assert_eq!(
        store.request_view(&q.payload.id, "same-key").unwrap().id,
        r.id
    );
}

/// account-credentials requirement 1: the options body is one JSON object. The four
/// values listed by position, another JSON type, or a repeated member are not it.
#[test]
fn credential_options_decode_only_from_one_object_without_repeated_members() {
    for body in [
        br#"[1000,null,1,["read"]]"#.as_slice(),
        br#""x""#.as_slice(),
        b"null".as_slice(),
        br#"{"expires_at":1000,"spend_cap_wei":null,"max_concurrency":1,"scopes":["read"],"scopes":["infer"]}"#
            .as_slice(),
    ] {
        assert!(
            serde_json::from_slice::<CredentialIssueRequest>(body).is_err(),
            "{}",
            String::from_utf8_lossy(body)
        );
    }
    let options: CredentialIssueRequest = serde_json::from_slice(
        br#"{"expires_at":1000,"spend_cap_wei":"900","max_concurrency":2,"scopes":["infer","read"]}"#,
    )
    .unwrap();
    assert_eq!(options.expires_at, 1000);
    assert_eq!(options.spend_cap_wei.as_deref(), Some("900"));
    assert_eq!(options.max_concurrency, 2);
    assert_eq!(options.scopes, ["infer", "read"]);
    // The native consumer decodes the same type from a JSON value.
    assert!(serde_json::from_value::<CredentialIssueRequest>(json!([1000, null, 1, ["read"]])).is_err());
    let encoded = serde_json::to_value(&options).unwrap();
    assert_eq!(
        encoded,
        json!({"expires_at":1000,"spend_cap_wei":"900","max_concurrency":2,"scopes":["infer","read"]})
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<CredentialIssueRequest>(encoded.clone()).unwrap()).unwrap(),
        encoded
    );
}

#[test]
fn caps_concurrency_revocation_and_scope_are_atomic() {
    let store = memory();
    let (q, _, _) = paid(&store);
    let app = app_credential(&store, &q.payload.id, Some("300"), vec!["infer".into()]);
    let r = store.reserve(&q.payload.id, &app.id, "one", 100, 100, 110, &store.account(&q.payload.id).unwrap().model)
        .unwrap();
    assert!(store
        .reserve(&q.payload.id, &app.id, "two", 100, 100, 110, &q.payload.model)
        .unwrap_err()
        .starts_with("spend_cap:"));
    store
        .settle(
            &r.id,
            &TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
            },
            "completed",
            111,
        )
        .unwrap();
    assert!(store
        .reserve(&q.payload.id, &app.id, "three", 100, 100, 112, &q.payload.model)
        .is_err());
    let read = app_credential(&store, &q.payload.id, None, vec!["read".into()]);
    assert!(store
        .reserve(&q.payload.id, &read.id, "read-only", 1, 1, 113, &q.payload.model)
        .unwrap_err()
        .starts_with("forbidden:"));
    store.revoke_credential(&q.payload.id, &app.id).unwrap();
    assert!(store.authenticate(&app.bearer, 114).is_err());
    assert!(store.authenticate(&read.bearer, 1000).is_err());
}

#[test]
fn restart_releases_unknown_holds_and_preserves_deduplication() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, open) = paid(&store);
    let r = store.reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "interrupted", 100, 200, 110, &store.account(&q.payload.id).unwrap().model)
        .unwrap();
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    assert_eq!(store.recover_requests(120).unwrap(), 1);
    assert_eq!(store.recover_requests(121).unwrap(), 0);
    let result = store.request_view(&q.payload.id, &r.id).unwrap();
    assert_eq!(result.status, "restart_unknown_usage");
    assert_eq!(result.cost_wei, "0");
    assert_eq!(result.provider_loss_wei, "300");
    assert_eq!(store.account(&q.payload.id).unwrap().available_wei, "995");
    assert!(store
        .reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "interrupted", 1, 1, 122, &q.payload.model)
        .is_err());
    assert_eq!(
        store
            .settle(
                &r.id,
                &TokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 200
                },
                "completed",
                123
            )
            .unwrap()
            .cost_wei,
        "0"
    );
}

#[test]
fn invalid_usage_cannot_charge_over_hold_or_leave_funds_locked() {
    let store = memory();
    let (q, _, open) = paid(&store);
    let r = store.reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "bad-usage", 10, 20, 110, &store.account(&q.payload.id).unwrap().model)
        .unwrap();
    assert!(store
        .settle(
            &r.id,
            &TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 21
            },
            "completed",
            111
        )
        .is_err());
    assert_eq!(store.account(&q.payload.id).unwrap().available_wei, "995");
    assert_eq!(
        store.request_view(&q.payload.id, &r.id).unwrap().status,
        "invalid_usage_unknown_usage"
    );
}

#[test]
fn orphan_credit_freezes_reverses_and_records_consumed_shortfall() {
    let store = memory();
    let (q, p, open) = paid(&store);
    let spent = store.reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "spent", 200, 300, 110, &store.account(&q.payload.id).unwrap().model)
        .unwrap();
    store
        .settle(
            &spent.id,
            &TokenUsage {
                prompt_tokens: 200,
                completion_tokens: 300,
            },
            "completed",
            111,
        )
        .unwrap();
    let held = store.reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "held", 100, 100, 112, &store.account(&q.payload.id).unwrap().model)
        .unwrap();
    store
        .mark_funding_state(&p.tx_hash, "orphaned", 113)
        .unwrap();
    let a = store.account(&q.payload.id).unwrap();
    assert_eq!(a.status, "frozen");
    assert_eq!(a.reversed_wei, "1000");
    assert_eq!(a.provider_loss_wei, "500");
    assert_eq!(a.available_wei, "0");
    assert_eq!(a.reserved_wei, "0");
    assert_eq!(a.fee_wei, "0");
    assert!(store
        .reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "after", 0, 0, 114, &q.payload.model)
        .is_err());
    assert_eq!(
        store
            .settle(
                &held.id,
                &TokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100
                },
                "completed",
                115
            )
            .unwrap()
            .cost_wei,
        "0"
    );
    store
        .mark_funding_state(&p.tx_hash, "orphaned", 116)
        .unwrap();
    assert_eq!(
        store.account(&q.payload.id).unwrap().provider_loss_wei,
        "500"
    );
    assert!(store.prepare_fee_batches(200000, true).unwrap().is_empty());
}

#[test]
fn settlement_pending_freezes_then_recovers_without_recredit() {
    let store = memory();
    let (q, p, open) = paid(&store);
    store
        .mark_funding_state(&p.tx_hash, "pending", 110)
        .unwrap();
    assert!(store
        .reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "blocked", 1, 1, 111, &q.payload.model)
        .is_err());
    store
        .mark_funding_state(&p.tx_hash, "accepted", 112)
        .unwrap();
    assert_eq!(store.account(&q.payload.id).unwrap().funded_wei, "1000");
    assert!(store
        .reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "available", 1, 1, 113, &q.payload.model)
        .is_ok());
}

#[test]
fn free_sessions_authenticate_without_funding_or_fee_evidence() {
    let store = memory();
    let q = quote(0, true);
    store.store_quote(&q, 100).unwrap();
    let opened = store.open_account(&q, None, 101).unwrap();
    assert_eq!(opened.account.status, "free");
    assert!(store.funding_history(&q.payload.id).unwrap().is_empty());
    assert!(store.prepare_fee_batches(200000, true).unwrap().is_empty());
    assert_eq!(store.admin_summary().unwrap()["pending_funding_intents"], 0);
    let p = payment(&q.payload, true, 1, 1);
    assert!(store
        .register_funding_intent(
            &q,
            &q.payload.consumer_wallet,
            &p.raw_transaction,
            &p.tx_hash,
            1,
            PROVIDER_KEY,
            || Ok(102),
            true,
            &q.payload.model
        )
        .unwrap_err()
        .starts_with("invalid_payment:"));
    assert!(store.check_open_funding(&q.payload, None, 1000).is_ok());
    assert!(store
        .check_open_funding(&q.payload, Some(&p.raw_transaction), 102)
        .is_err());
    let r = store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "free", 100, 100, 110, &store.account(&q.payload.id).unwrap().model)
        .unwrap();
    assert_eq!(
        store
            .settle(
                &r.id,
                &TokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100
                },
                "completed",
                111
            )
            .unwrap()
            .cost_wei,
        "0"
    );
    assert!(store
        .credit_topup(&q.payload.id, &payment(&q.payload, false, 1, 3), 112)
        .is_err());
}

#[test]
fn fee_batches_survive_restart_and_have_immutable_submission_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    paid(&store);
    assert!(store.prepare_fee_batches(200, false).unwrap().is_empty());
    let batches = store
        .prepare_fee_batches(101 + FEE_MAX_AGE_SECS, false)
        .unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].amount_wei, "5");
    let id = batches[0].id.clone();
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    let mut batch = store.prepare_fee_batches(200000, true).unwrap().remove(0);
    assert_eq!(batch.id, id);
    batch.tx_hash = Some(protocol::hash_hex(b"signed-fee-boundary"));
    batch.raw_transaction = Some("0xab".into());
    batch.status = "submitted".into();
    store.update_fee_batch(&batch).unwrap();
    let mut changed = batch.clone();
    changed.amount_wei = "6".into();
    assert!(store.update_fee_batch(&changed).is_err());
    changed = batch;
    changed.tx_hash = Some(protocol::random_id());
    assert!(store.update_fee_batch(&changed).is_err());
}

#[test]
fn receipts_are_signed_monotonic_and_network_bound_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, _) = paid(&store);
    let first = store.receipt(&q.payload.id, PROVIDER_KEY, 110).unwrap();
    protocol::verify(protocol::RECEIPT_DOMAIN, &first, &q.payload.provider_wallet).unwrap();
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    let second = store.receipt(&q.payload.id, PROVIDER_KEY, 111).unwrap();
    assert_eq!(second.payload.sequence, first.payload.sequence + 1);
    let mut other = network();
    other.deployment_id.push('x');
    assert!(BillingStore::open(&path, &other).is_err());
    assert!(store.receipt(&q.payload.id, OWNER_KEY, 112).is_err());
}

#[test]
fn preexpiry_funding_intent_preserves_quote_through_settlement_stall_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let q = quote(1000, false);
    store.store_quote(&q, 100).unwrap();
    let p = payment(&q.payload, true, 1000, 1);
    let ack = register(&store, &q, &p, 399);
    protocol::verify(protocol::INTENT_DOMAIN, &ack, &q.payload.provider_wallet).unwrap();
    assert_eq!(ack.payload.registered_at, 399);
    assert!(store.prepare_fee_batches(400, true).unwrap().is_empty());
    store
        .record_funding_attempt(&q.payload.id, &p.raw_transaction, "pending", 400)
        .unwrap();
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    let delayed = 30 * 86_400;
    store
        .record_funding_attempt(&q.payload.id, &p.raw_transaction, "unavailable", delayed)
        .unwrap();
    store
        .transaction(|tx| cleanup_pending(tx, delayed))
        .unwrap();
    assert!(store.quote(&q.payload.id).is_ok());
    assert_eq!(store.admin_summary().unwrap()["pending_funding_intents"], 1);
    let repeated = store.register_funding_intent(&q, &q.payload.consumer_wallet, &p.raw_transaction, &p.tx_hash, 1, PROVIDER_KEY, || Ok(delayed), false, &(&q).payload.model)
        .unwrap();
    assert_eq!(encode(&ack).unwrap(), encode(&repeated).unwrap());
    assert!(store
        .check_open_funding(&q.payload, Some(&p.raw_transaction), delayed)
        .is_ok());
    assert!(store
        .check_open_funding(&q.payload, Some("0x02"), delayed)
        .unwrap_err()
        .starts_with("conflict:"));
    assert!(store.prepare_fee_batches(delayed, true).unwrap().is_empty());
    let opened = store.open_account(&q, Some(&p), delayed).unwrap();
    assert_eq!(opened.account.available_wei, "995");
    assert_eq!(store.admin_summary().unwrap()["pending_funding_intents"], 0);
    assert_eq!(
        store.prepare_fee_batches(delayed, true).unwrap()[0].amount_wei,
        "5"
    );
    let repeated = store.register_funding_intent(&q, &q.payload.consumer_wallet, &p.raw_transaction, &p.tx_hash, 1, PROVIDER_KEY, || Ok(delayed + 1), false, &(&q).payload.model)
        .unwrap();
    assert_eq!(encode(&ack).unwrap(), encode(&repeated).unwrap());
}

#[test]
fn paid_open_never_registers_implicitly_even_before_expiry() {
    let store = memory();
    let q = quote(1000, false);
    store.store_quote(&q, 100).unwrap();
    let p = payment(&q.payload, true, 1000, 1);
    assert!(store
        .check_open_funding(&q.payload, Some(&p.raw_transaction), 101)
        .unwrap_err()
        .starts_with("funding_intent_required:"));
    assert!(store
        .open_account(&q, Some(&p), 101)
        .unwrap_err()
        .starts_with("funding_intent_required:"));
    store
        .record_funding_attempt(&q.payload.id, &p.raw_transaction, "pending", 101)
        .unwrap();
    assert!(store
        .open_account(&q, Some(&p), 102)
        .unwrap_err()
        .starts_with("funding_intent_required:"));
    assert!(store.account(&q.payload.id).is_err());
    assert!(store.prepare_fee_batches(1000, true).unwrap().is_empty());
}

#[test]
fn registration_rejects_expiry_wrong_owner_network_and_replacement() {
    let store = memory();
    let q = quote(1000, false);
    store.store_quote(&q, 100).unwrap();
    let p = payment(&q.payload, true, 1000, 1);
    for now in [400, 401] {
        assert!(store
            .register_funding_intent(
                &q,
                &q.payload.consumer_wallet,
                &p.raw_transaction,
                &p.tx_hash,
                1,
                PROVIDER_KEY,
                || Ok(now),
                true,
                &q.payload.model
            )
            .unwrap_err()
            .starts_with("expired_quote:"));
    }
    assert_eq!(store.admin_summary().unwrap()["pending_funding_intents"], 0);
    assert!(store
        .register_funding_intent(
            &q,
            &q.payload.provider_wallet,
            &p.raw_transaction,
            &p.tx_hash,
            1,
            PROVIDER_KEY,
            || Ok(101),
            true,
            &q.payload.model
        )
        .unwrap_err()
        .starts_with("forbidden:"));
    let mut wrong = q.clone();
    wrong.payload.network.deployment_id.push('x');
    assert!(store
        .register_funding_intent(
            &wrong,
            &q.payload.consumer_wallet,
            &p.raw_transaction,
            &p.tx_hash,
            1,
            PROVIDER_KEY,
            || Ok(101),
            true,
            &wrong.payload.model
        )
        .unwrap_err()
        .starts_with("wrong_network:"));
    assert!(store
        .register_funding_intent(
            &q,
            &q.payload.consumer_wallet,
            &p.raw_transaction,
            &p.tx_hash,
            1,
            PROVIDER_KEY,
            || Ok(101),
            false,
            &q.payload.model
        )
        .unwrap_err()
        .starts_with("provider_draining:"));
    register(&store, &q, &p, 102);
    let other = payment(&q.payload, true, 1000, 2);
    assert!(store
        .register_funding_intent(
            &q,
            &q.payload.consumer_wallet,
            &other.raw_transaction,
            &other.tx_hash,
            2,
            PROVIDER_KEY,
            || Ok(103),
            true,
            &q.payload.model
        )
        .unwrap_err()
        .starts_with("conflict:"));
    store
        .record_funding_attempt(&q.payload.id, &p.raw_transaction, "rejected", 104)
        .unwrap();
    store
        .transaction(|tx| cleanup_pending(tx, 30 * 86_400))
        .unwrap();
    assert!(store
        .check_open_funding(&q.payload, Some(&p.raw_transaction), 30 * 86_400)
        .is_ok());
    assert!(store.open_account(&q, Some(&p), 30 * 86_400).is_ok());
}

#[test]
fn registration_preserves_signed_time_boundaries_and_cached_recovery() {
    for sample in [Ok(99), Ok(100), Ok(399), Ok(400), Ok(401), Err("clock unavailable")] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite");
        let mut store = BillingStore::open(&path, &network()).unwrap();
        paid(&store); // Explicit post-verification history, not observed chain execution.
        store.store_quote(&quote(1000, false), 100).unwrap();
        let q = quote(1000, false);
        store.store_quote(&q, 100).unwrap();
        let data = protocol::opening_data(&q.payload).unwrap();
        let original = crate::chain_client::sign_transaction(
            OWNER_KEY, network().chain_id, 2, &q.payload.provider_wallet, 1000,
            &data, 1, native_fixture_gas(&data),
        ).unwrap();
        let before = quote_work_protected_snapshot(&store);
        let result = store.register_funding_intent(
            &q, &q.payload.consumer_wallet, &original.raw_transaction, &original.tx_hash,
            2, PROVIDER_KEY, || sample.map_err(str::to_owned), true, &q.payload.model,
        );
        let (ack, accepted_at) = match sample {
            Ok(now @ 100..=399) => (result.unwrap(), now),
            _ => {
                let failure = result.unwrap_err();
                match sample {
                    Ok(99) => assert!(failure.starts_with("invalid_quote:"), "{failure}"),
                    Ok(_) => assert!(failure.starts_with("expired_quote:"), "{failure}"),
                    Err(message) => assert_eq!(failure, message),
                }
                assert_eq!(quote_work_protected_snapshot(&store), before);
                drop(store);
                store = BillingStore::open(&path, &network()).unwrap();
                assert_eq!(quote_work_protected_snapshot(&store), before);
                // A refused sample cannot poison this quote or its original nonce.
                let recovered = store.register_funding_intent(
                    &q, &q.payload.consumer_wallet, &original.raw_transaction, &original.tx_hash,
                    2, PROVIDER_KEY, || Ok(100), true, &q.payload.model,
                ).unwrap();
                (recovered, 100)
            }
        };
        protocol::verify(protocol::INTENT_DOMAIN, &ack, &q.payload.provider_wallet).unwrap();
        assert_eq!(serde_json::to_value(&ack.payload).unwrap(), serde_json::json!({
            "network": q.payload.network,
            "provider_wallet": q.payload.provider_wallet,
            "consumer_wallet": q.payload.consumer_wallet,
            "quote_id": q.payload.id,
            "tx_hash": original.tx_hash,
            "registered_at": accepted_at,
        }));
        // Only the matching standalone quote moves to permanent intent retention.
        // All prior money, roots, requests, rates and unrelated terms stay exact.
        let mut expected = before;
        let pending = serde_json::json!([
            q.payload.id, q.payload.consumer_wallet, q.payload.expires_at, encode(&q).unwrap()
        ]).to_string();
        expected[2].retain(|record| record != &pending);
        expected[9].push(serde_json::json!([
            q.payload.id, original.tx_hash, original.raw_transaction, encode(&ack).unwrap(),
            encode(&q).unwrap(), q.payload.consumer_wallet, "2", q.payload.expires_at
        ]).to_string());
        expected[9].sort();
        assert_eq!(quote_work_protected_snapshot(&store), expected);
        drop(store);
        let store = BillingStore::open(&path, &network()).unwrap();
        assert_eq!(quote_work_protected_snapshot(&store), expected);
        for recovery_time in [Ok(99), Ok(400), Err("clock unavailable")] {
            let sampled = std::cell::Cell::new(false);
            let cached = store.register_funding_intent(
                &q, &q.payload.consumer_wallet, &original.raw_transaction, &original.tx_hash,
                2, PROVIDER_KEY,
                || {
                    sampled.set(true);
                    recovery_time.map_err(str::to_owned)
                },
                false, &q.payload.model,
            ).unwrap();
            assert!(!sampled.get(), "cached acceptance must not sample fresh registration time");
            assert_eq!(encode(&cached).unwrap(), encode(&ack).unwrap());
            assert_eq!(quote_work_protected_snapshot(&store), expected);
        }
    }
}

#[test]
fn registration_samples_time_after_waiting_for_the_writer() {
    use std::{
        cell::RefCell,
        sync::{atomic::{AtomicU64, AtomicUsize, Ordering}, mpsc, Arc},
        time::Duration,
    };
    // rusqlite accepts a function pointer. Keep its one-shot handshake local to
    // this dedicated writer thread, never in shared process-wide test state.
    thread_local! {
        static WRITER_WAIT: RefCell<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> =
            const { RefCell::new(None) };
    }
    fn wait_for_release(_: i32) -> bool {
        WRITER_WAIT.with(|slot| match slot.borrow_mut().take() {
            Some((blocked, release)) => blocked.send(()).is_ok()
                && release.recv_timeout(Duration::from_secs(10)).is_ok(),
            None => false,
        })
    }

    for after_wait in [99, 400] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        paid(&store);
        let q = quote(1000, false);
        store.store_quote(&q, 100).unwrap();
        let data = protocol::opening_data(&q.payload).unwrap();
        let original = crate::chain_client::sign_transaction(
            OWNER_KEY, network().chain_id, 2, &q.payload.provider_wallet, 1000,
            &data, 1, native_fixture_gas(&data),
        ).unwrap();
        let before = quote_work_protected_snapshot(&store);
        let writer = BillingStore::open(&path, &network()).unwrap();
        writer.db.lock().unwrap().busy_handler(Some(wait_for_release)).unwrap();
        let mut blocker = Connection::open(&path).unwrap();
        let held = blocker.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        let (blocked_tx, blocked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let clock = Arc::new(AtomicU64::new(100));
        let samples = Arc::new(AtomicUsize::new(0));
        let worker_clock = clock.clone();
        let worker_samples = samples.clone();
        let probe_path = path.clone();
        let worker = std::thread::spawn(move || {
            WRITER_WAIT.with(|slot| *slot.borrow_mut() = Some((blocked_tx, release_rx)));
            writer.register_funding_intent(
                &q, &q.payload.consumer_wallet, &original.raw_transaction, &original.tx_hash,
                2, PROVIDER_KEY,
                || {
                    worker_samples.fetch_add(1, Ordering::SeqCst);
                    // The blocker has been released. Only registration's own
                    // write transaction can now exclude this independent writer.
                    let probe = Connection::open(&probe_path).unwrap();
                    probe.busy_timeout(Duration::ZERO).unwrap();
                    let failure = probe.execute_batch("BEGIN IMMEDIATE").unwrap_err();
                    assert!(matches!(&failure, rusqlite::Error::SqliteFailure(code, _)
                        if code.code == rusqlite::ErrorCode::DatabaseBusy), "{failure}");
                    Ok(worker_clock.load(Ordering::SeqCst))
                },
                true, &q.payload.model,
            )
        });
        blocked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let early_samples = samples.load(Ordering::SeqCst);
        clock.store(after_wait, Ordering::SeqCst);
        held.commit().unwrap();
        release_tx.send(()).unwrap();
        let result = worker.join().unwrap();
        assert_eq!(early_samples, 0, "arrival time must not be sampled before writer admission");
        assert_eq!(samples.load(Ordering::SeqCst), 1);
        let failure = result.unwrap_err();
        let prefix = if after_wait == 99 { "invalid_quote:" } else { "expired_quote:" };
        assert!(failure.starts_with(prefix), "{failure}");
        assert_eq!(quote_work_protected_snapshot(&store), before);
        drop(store);
        let reopened = BillingStore::open(&path, &network()).unwrap();
        assert_eq!(quote_work_protected_snapshot(&reopened), before);
    }
}

#[test]
fn pending_capacity_rejects_new_intents_without_losing_accepted_obligations() {
    let store = memory();
    let q = quote(1000, false);
    store.store_quote(&q, 100).unwrap();
    let p = payment(&q.payload, true, 1000, 1);
    let ack = register(&store, &q, &p, 101);
    let other = quote(1000, false);
    store.store_quote(&other, 102).unwrap();
    let next = payment(&other.payload, true, 1000, 2);
    // Fill capacity directly: this test isolates admission, not envelope validation.
    store.transaction(|tx| {
        for n in 1..MAX_PENDING {
            tx.execute("INSERT INTO funding_intents(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) VALUES(?1,?2,'0x00',?3,?4,?5,'0',400)",params![format!("pending-{n}"),format!("hash-{n}"),encode(&ack)?,encode(&q)?,format!("sender-{n}")]).map_err(sql)?;
        }
        Ok(())
    }).unwrap();
    assert!(store
        .register_funding_intent(
            &other,
            &other.payload.consumer_wallet,
            &next.raw_transaction,
            &next.tx_hash,
            2,
            PROVIDER_KEY,
            || Ok(103),
            true,
            &other.payload.model
        )
        .unwrap_err()
        .starts_with("rate_limit:"));
    assert_eq!(
        store.admin_summary().unwrap()["pending_funding_intents"],
        MAX_PENDING
    );
    store
        .transaction(|tx| cleanup_pending(tx, 30 * 86_400))
        .unwrap();
    assert_eq!(active_intents(&store, 400), 0);
    let mut fresh = quote(1000, false).payload;
    fresh.issued_at = 30 * 86_400;
    fresh.expires_at = fresh.issued_at + 300;
    let fresh = protocol::sign(protocol::QUOTE_DOMAIN, fresh, PROVIDER_KEY).unwrap();
    store.store_quote(&fresh, 30 * 86_400).unwrap();
    register(
        &store,
        &fresh,
        &payment(&fresh.payload, true, 1000, 3),
        30 * 86_400,
    );
    assert_eq!(
        encode(&register(&store, &q, &p, 30 * 86_400)).unwrap(),
        encode(&ack).unwrap()
    );
    store
        .record_funding_attempt(&q.payload.id, &p.raw_transaction, "pending", 30 * 86_400)
        .unwrap();
    assert!(store
        .prepare_fee_batches(30 * 86_400, true)
        .unwrap()
        .is_empty());
    assert!(store.open_account(&q, Some(&p), 30 * 86_400).is_ok());
    assert_eq!(
        store.admin_summary().unwrap()["pending_funding_intents"],
        MAX_PENDING
    );
    assert_eq!(active_intents(&store, 30 * 86_400), 1);
}

/// specify-non-reserving-quotes 3.3/4.3 (requirement "Authenticated paid admission
/// preserves accepted commitments"): the paid bound counts only un-opened originals
/// whose deadline is strictly later than the transaction time; a full refusal leaves
/// no partial acceptance; replaying an accepted original neither allocates, re-counts
/// nor extends its lease, before or after expiry and across reopening. Filler leases
/// are restored prerequisites, not observed admissions; payments are ledger inputs
/// after the chain verification boundary. Would catch counting a lease at its
/// deadline (`>=`), a replay that renews its lease, or cached recovery placed behind
/// the capacity decision.
#[test]
fn paid_lease_bound_is_strict_at_deadlines_and_replay_never_renews_a_lease() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lease-boundary.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    // An opened agreement keeps its intent permanently without occupying a lease.
    let (opened_quote, opened_payment, opened) = paid(&store);
    let original = quote(1000, false); // issued 100, deadline 400
    store.store_quote(&original, 100).unwrap();
    let original_payment = payment(&original.payload, true, 1000, 2);
    let ack = register(&store, &original, &original_payment, 101);
    let candidate = |issued_at: u64, expires_at: u64| {
        let mut terms = quote(1000, false).payload;
        terms.issued_at = issued_at;
        terms.expires_at = expires_at;
        protocol::sign(protocol::QUOTE_DOMAIN, terms, PROVIDER_KEY).unwrap()
    };
    let first = candidate(390, 700);
    let second = candidate(390, 800);
    for pending in [&first, &second] {
        store.store_quote(pending, 390).unwrap();
    }
    let first_payment = payment(&first.payload, true, 1000, 3);
    let second_payment = payment(&second.payload, true, 1000, 4);
    store.transaction(|tx| {
        // 9,998 later deadlines plus one boundary lease ending at 450.
        for (index, deadline) in (0..9_998).map(|n| (n, 500)).chain([(9_998, 450)]) {
            tx.execute("INSERT INTO funding_intents(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) VALUES(?1,?2,'0x00',?3,?4,?5,'0',?6)",
                params![format!("lease-{index}"), format!("lease-hash-{index}"), encode(&ack)?, encode(&original)?,
                    format!("lease-sender-{index}"), deadline]).map_err(sql)?;
        }
        Ok(())
    }).unwrap();
    let attempt = |store: &BillingStore, terms: &Signed<Quote>, funding: &VerifiedPayment, now: u64| {
        let nonce = u64::from_str_radix(&funding.raw_transaction[2..], 16).unwrap();
        store.register_funding_intent(terms, &terms.payload.consumer_wallet, &funding.raw_transaction,
            &funding.tx_hash, nonce, PROVIDER_KEY, || Ok(now), true, &terms.payload.model)
    };
    let replay = |store: &BillingStore| {
        // Cached recovery samples no time and needs no open admission gate.
        let cached = store.register_funding_intent(&original, &original.payload.consumer_wallet,
            &original_payment.raw_transaction, &original_payment.tx_hash, 2, PROVIDER_KEY,
            || panic!("cached recovery must not sample registration time"), false,
            &original.payload.model).unwrap();
        assert_eq!(encode(&cached).unwrap(), encode(&ack).unwrap());
    };
    let lease = |store: &BillingStore, id: &str| -> u64 {
        store.db.lock().unwrap().query_row("SELECT admission_until FROM funding_intents WHERE contract=?1",
            [id], |row| row.get(0)).unwrap()
    };
    assert_eq!(active_intents(&store, 399), MAX_PENDING as u64);

    // Full at 399: refusal creates no intent, nonce, account, credit or quote handoff.
    let before = quote_work_protected_snapshot(&store);
    let refused = attempt(&store, &first, &first_payment, 399).unwrap_err();
    assert!(refused.starts_with("rate_limit:"), "{refused}");
    assert_eq!(quote_work_protected_snapshot(&store), before);
    // Cached replay is not behind the full bound and changes nothing, lease included.
    replay(&store);
    assert_eq!(quote_work_protected_snapshot(&store), before);
    assert_eq!(lease(&store, &original.payload.id), 400);
    drop(store);
    store = BillingStore::open(&path, &network()).unwrap();
    assert_eq!(quote_work_protected_snapshot(&store), before);

    // At its exact deadline the original's lease no longer counts, so the refused
    // original is admitted unchanged; a deadline equal to the time is not active.
    let first_ack = attempt(&store, &first, &first_payment, 400).unwrap();
    assert_eq!(first_ack.payload.registered_at, 400);
    assert_eq!(lease(&store, &first.payload.id), 700);
    assert_eq!(active_intents(&store, 400), MAX_PENDING as u64);
    for _ in 0..2 {
        replay(&store);
        assert_eq!(lease(&store, &original.payload.id), 400, "replay must not renew an expired lease");
        assert_eq!(active_intents(&store, 449), MAX_PENDING as u64);
    }
    drop(store);
    store = BillingStore::open(&path, &network()).unwrap();
    replay(&store);
    let before = quote_work_protected_snapshot(&store);
    let refused = attempt(&store, &second, &second_payment, 449).unwrap_err();
    assert!(refused.starts_with("rate_limit:"), "{refused}");
    assert_eq!(quote_work_protected_snapshot(&store), before);
    // The boundary lease ends at 450; a renewed original would still fill the bound.
    let second_ack = attempt(&store, &second, &second_payment, 450).unwrap();
    assert_eq!(second_ack.payload.registered_at, 450);
    assert_eq!(active_intents(&store, 450), MAX_PENDING as u64);

    // Expired acceptance still opens with its post-verification payment while the
    // bound is full, without a new lease; the earlier opened agreement is untouched.
    replay(&store);
    let credited = store.open_account(&original, Some(&original_payment), 451).unwrap();
    assert_eq!(credited.account.funded_wei, "1000");
    assert_eq!(credited.account.fee_wei, "5");
    assert!(credited.credential.is_some());
    assert_eq!(active_intents(&store, 451), MAX_PENDING as u64);
    replay(&store);
    assert_eq!(encode(&store.open_account(&opened_quote, Some(&opened_payment), 452).unwrap().account).unwrap(),
        encode(&opened.account).unwrap());
    assert_eq!(store.admin_summary().unwrap()["pending_funding_intents"], MAX_PENDING + 1);
}

/// specify-non-reserving-quotes 4.3 (final free slot competition): authenticated free
/// openings decide the owned (8) and ledger-wide (10,000) account bounds atomically
/// across independent connections. Exactly one contender gets an account and root
/// credential; the loser keeps its standalone quote and gains nothing, before and
/// after reopening, and the winner's recovery issues no further credential. Once
/// either bound is full, a later free quote is still retained without an account or
/// credential and its opening is refused. The ledger-wide case restores 9,999 account
/// rows as prerequisites, not openings. Would catch an off-by-one bound, quote storage
/// gated by either full free-account bound, and (by natural race, not a deterministic
/// schedule) a count read without the write lock. A count split into a separately
/// locked transaction from the insert is not distinguishable here; the deterministic
/// `paused_free_opening_holds_the_final_owned_and_ledger_slot_checks_until_its_account_commits`
/// proves that case.
#[test]
fn free_openings_compete_for_the_final_owned_and_ledger_slots_across_connections() {
    for ledger_wide in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("free-slots.sqlite");
        let mut store = BillingStore::open(&path, &network()).unwrap();
        if ledger_wide {
            let template = quote(0, true);
            store.transaction(|tx| {
                for index in 0..MAX_PENDING - 1 {
                    tx.execute("INSERT INTO accounts(id,owner,quote,account,created) VALUES(?1,?2,?3,'{}',100)",
                        params![format!("restored-{index}"), format!("0x{index:040x}"), encode(&template)?]).map_err(sql)?;
                }
                Ok(())
            }).unwrap();
        } else {
            for _ in 0..7 {
                let earlier = quote(0, true);
                store.store_quote(&earlier, 100).unwrap();
                assert!(store.open_account(&earlier, None, 101).unwrap().credential.is_some());
            }
        }
        let contenders = [quote(0, true), quote(0, true)];
        for contender in &contenders {
            store.store_quote(contender, 100).unwrap();
        }
        let before = quote_work_protected_snapshot(&store);
        let barrier = Arc::new(Barrier::new(2));
        let workers = contenders.iter().map(|contender| {
            let ledger = BillingStore::open(&path, &network()).unwrap();
            let contender = contender.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                ledger.open_account(&contender, None, 102)
            })
        }).collect::<Vec<_>>();
        let outcomes = workers.into_iter().map(|worker| worker.join().unwrap()).collect::<Vec<_>>();
        let winner = match (&outcomes[0], &outcomes[1]) {
            (Ok(_), Err(error)) if error.starts_with("rate_limit:") => 0,
            (Err(error), Ok(_)) if error.starts_with("rate_limit:") => 1,
            _ => panic!("exactly one final free slot must commit (ledger_wide={ledger_wide}): {:?}",
                outcomes.iter().map(|outcome| outcome.as_ref().map(|_| "opened").map_err(Clone::clone)).collect::<Vec<_>>()),
        };
        let (won, lost) = (&contenders[winner], &contenders[1 - winner]);
        let opened = outcomes[winner].as_ref().unwrap();
        assert_eq!(opened.account.status, "free");
        let root = opened.credential.as_ref().unwrap();
        let assert_only_winner = |store: &BillingStore| {
            let after = quote_work_protected_snapshot(store);
            for index in [0, 1, 3, 6, 7, 8, 9, 10, 11, 12, 13] {
                assert_eq!(after[index], before[index], "ledger_wide={ledger_wide}: table {index}");
            }
            let mut quotes = before[2].clone();
            quotes.retain(|record| !record.contains(&won.payload.id));
            assert_eq!(after[2], quotes, "only the winner's standalone quote is handed off");
            assert!(after[2].iter().any(|record| record.contains(&lost.payload.id)));
            for (index, table) in [(4, "accounts"), (5, "credentials")] {
                let added = after[index].iter().filter(|row| !before[index].contains(row)).collect::<Vec<_>>();
                assert_eq!(after[index].len(), before[index].len() + 1, "{table}");
                assert_eq!(added.len(), 1, "{table}");
                assert!(added[0].contains(&won.payload.id) && !added[0].contains(&lost.payload.id), "{table}");
            }
        };
        assert_only_winner(&store);
        drop(store);
        store = BillingStore::open(&path, &network()).unwrap();
        assert_only_winner(&store);
        let refused = store.open_account(lost, None, 103).unwrap_err();
        assert!(refused.starts_with("rate_limit:"), "{refused}");
        let recovered = store.open_account(won, None, 103).unwrap();
        assert!(recovered.credential.is_none(), "existing-account recovery issues no credential");
        assert_eq!(encode(&recovered.account).unwrap(), encode(&opened.account).unwrap());
        assert_eq!(store.authenticate(&root.bearer, 103).unwrap().account.contract_id, won.payload.id);
        assert_only_winner(&store);

        // Quoting is not gated by the now-full free-account bound, owned or ledger-wide:
        // a later free quote for the same owner is retained without an account,
        // credential or reservation, and its own authenticated opening is still refused.
        // Compared without printing credential rows on failure.
        let late = quote(0, true);
        let full = quote_work_protected_snapshot(&store);
        store.store_quote(&late, 103).unwrap();
        let quoted = quote_work_protected_snapshot(&store);
        assert!(quoted[4] == full[4] && quoted[5] == full[5],
            "ledger_wide={ledger_wide}: a quote must not allocate an account or credential");
        assert!(quoted[2].iter().any(|record| record.contains(&late.payload.id)));
        let refused = store.open_account(&late, None, 104).unwrap_err();
        assert!(refused.starts_with("rate_limit:"), "{refused}");
        assert!(quote_work_protected_snapshot(&store) == quoted,
            "ledger_wide={ledger_wide}: a refused opening leaves no partial state");
    }
}

/// Arms `store`'s test-only admission pause point `point`: the first receiver resolves once
/// a transaction of `store` has passed that final capacity check, still inside its write
/// transaction, and the transaction continues once the returned sender is used or dropped.
fn pause_next_admission(store: &BillingStore, point: AdmissionPoint)
    -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
    let (reached, reports) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    *store.admission_pause.lock().unwrap() = Some((point, reached, resume));
    (reports, release)
}

/// The deterministic final-slot schedule of specify-non-reserving-quotes 3.3/4.3. Two
/// writers, each its own `BillingStore` connection to the ledger at `path`, are armed at
/// `point`. Writer A runs `first` until it has passed its final capacity check and paused
/// inside its write transaction, before inserting. Only then does writer B start `second`.
/// While A stays paused, B must neither reach its own pause point (it cannot pass its own
/// capacity check) nor return: under the ledger's 5-second busy timeout it waits for A's
/// write lock instead of failing with "database is locked". An independent connection then
/// finds the write lock held. A is released, B's pause point is released with it (so a B
/// that wrongly passes its check later returns instead of blocking the test), and both
/// results are returned; B must not reach its pause point afterwards either. Results are
/// not printed, since an opening's result carries a credential secret.
fn race_paused_admission<T: Send + 'static>(
    path: &Path,
    point: AdmissionPoint,
    first: impl FnOnce(&BillingStore) -> Result<T, String> + Send + 'static,
    second: impl FnOnce(&BillingStore) -> Result<T, String> + Send + 'static,
    context: &str,
) -> (Result<T, String>, Result<T, String>) {
    use std::{sync::mpsc::{self, RecvTimeoutError}, time::Duration};
    let writer_a = BillingStore::open(path, &network()).unwrap();
    let writer_b = BillingStore::open(path, &network()).unwrap();
    let (a_reached, a_release) = pause_next_admission(&writer_a, point);
    let (b_reached, b_release) = pause_next_admission(&writer_b, point);
    let a = std::thread::spawn(move || first(&writer_a));
    if let Err(error) = a_reached.recv_timeout(Duration::from_secs(10)) {
        panic!("{context}: writer A must pass its final capacity check and pause: {error:?}");
    }
    let (started, b_started) = mpsc::channel();
    let b = std::thread::spawn(move || {
        started.send(()).unwrap();
        second(&writer_b)
    });
    b_started.recv_timeout(Duration::from_secs(10)).unwrap();
    // The bounded negative: B is running and A holds the final slot between its check
    // and its insertion. B must not pass its own capacity check in this window.
    match b_reached.recv_timeout(Duration::from_millis(500)) {
        Err(RecvTimeoutError::Timeout) => {}
        Ok(()) => panic!("{context}: writer B passed its own capacity check while writer A \
            was paused between its check and its insertion"),
        Err(RecvTimeoutError::Disconnected) => panic!("{context}: writer B returned while \
            writer A was paused instead of waiting for its write lock: {:?}",
            b.join().unwrap().err()),
    }
    assert!(!b.is_finished(), "{context}: writer B must wait for writer A's write \
        transaction under the busy timeout, not return while A is paused");
    let probe = Connection::open(path).unwrap();
    probe.busy_timeout(Duration::ZERO).unwrap();
    match probe.execute_batch("BEGIN IMMEDIATE") {
        Err(rusqlite::Error::SqliteFailure(code, _))
            if code.code == rusqlite::ErrorCode::DatabaseBusy => {}
        other => panic!("{context}: paused writer A must hold the ledger write lock from \
            its capacity check to its insertion: {other:?}"),
    }
    drop(probe);
    a_release.send(()).unwrap();
    // B's seam is released before joining: a B that passes its capacity check after A's
    // commit (an off-by-one bound, or a count read before the write lock) then returns
    // and is reported below, instead of waiting at its seam on this thread forever.
    drop(b_release);
    let first = a.join().unwrap();
    let second = b.join().unwrap();
    assert!(b_reached.try_recv().is_err(),
        "{context}: writer B must be refused by its capacity check, never pass it");
    (first, second)
}

/// Asserts that the protected ledger snapshot `after` differs from `before` only by the
/// winner's standalone-quote handoff (the loser keeps its quote) and by exactly one added
/// row, naming the winner and not the loser, in each table of `added`. Rows are compared
/// without being printed, since credential rows are protected.
fn assert_only_winner_added(before: &[Vec<String>], after: &[Vec<String>], won: &str,
    lost: &str, added: &[usize], context: &str) {
    assert_eq!(after.len(), before.len());
    for index in 0..before.len() {
        if index == 2 {
            let mut quotes = before[2].clone();
            quotes.retain(|record| !record.contains(won));
            assert!(quotes.len() + 1 == before[2].len() && after[2] == quotes,
                "{context}: only the winner's standalone quote is handed off");
            assert!(after[2].iter().any(|record| record.contains(lost)),
                "{context}: the refused writer keeps its standalone quote");
        } else if added.contains(&index) {
            let previous = before[index].iter().collect::<std::collections::HashSet<_>>();
            let new = after[index].iter().filter(|row| !previous.contains(row)).collect::<Vec<_>>();
            assert!(after[index].len() == before[index].len() + 1 && new.len() == 1
                && new[0].contains(won) && !new[0].contains(lost),
                "{context}: table {index} must gain exactly one row, the winner's");
        } else {
            assert!(after[index] == before[index], "{context}: table {index} changed");
        }
    }
}

/// specify-non-reserving-quotes 3.3/4.3, requirement "Authenticated paid admission preserves
/// accepted commitments": "The final check and accepted-evidence insertion SHALL be atomic
/// across writers", scenario *Writers compete for the last paid admission lease*, by a
/// deterministic schedule rather than a natural race. With exactly one paid lease left
/// (9,999 active leases, restored prerequisites besides one registered original), writer A
/// passes the active-lease check and pauses inside its write transaction before inserting;
/// writer B, an independently qualifying original on another connection, starts and cannot
/// pass its own check while A is paused, and an independent connection finds the write
/// lock held. Released, A commits the only acknowledgement and lease; B gets `rate_limit`
/// after waiting (not a lock error) and leaves no intent, nonce, account or credit, and its
/// quote is retained; a later retry and reopening change nothing. Would catch the count and
/// the insert split into two separately committed transactions, and a count read without
/// the write lock (a deferred transaction), both of which reach B's pause point while A is
/// paused; and a count read before the write lock is taken, or an off-by-one bound, both of
/// which let B pass its check after A's commit (B's seam is released with A, so these fail
/// rather than hang). B's exact refusal shows the active-lease check itself refused it.
#[test]
fn paused_paid_registration_holds_the_final_lease_check_until_its_acknowledgement_commits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("paused-paid-lease.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let seed = quote(1000, false);
    store.store_quote(&seed, 100).unwrap();
    let seed_ack = register(&store, &seed, &payment(&seed.payload, true, 1000, 1), 101);
    store.transaction(|tx| {
        for index in 0..MAX_PENDING - 2 {
            tx.execute("INSERT INTO funding_intents(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) VALUES(?1,?2,'0x00',?3,?4,?5,'0',400)",
                params![format!("lease-{index}"), format!("lease-hash-{index}"), encode(&seed_ack)?,
                    encode(&seed)?, format!("lease-sender-{index}")]).map_err(sql)?;
        }
        Ok(())
    }).unwrap();
    assert_eq!(active_intents(&store, 102), MAX_PENDING as u64 - 1, "exactly one lease left");
    let contenders = [quote(1000, false), quote(1000, false)];
    let payments = [payment(&contenders[0].payload, true, 1000, 2),
        payment(&contenders[1].payload, true, 1000, 3)];
    for contender in &contenders {
        store.store_quote(contender, 100).unwrap();
    }
    let attempt = |index: usize| {
        let (terms, funding) = (contenders[index].clone(), payments[index].clone());
        move |ledger: &BillingStore| ledger.register_funding_intent(&terms,
            &terms.payload.consumer_wallet, &funding.raw_transaction, &funding.tx_hash,
            index as u64 + 2, PROVIDER_KEY, || Ok(102), true, &terms.payload.model)
    };
    let before = quote_work_protected_snapshot(&store);
    let (first, second) = race_paused_admission(&path, AdmissionPoint::PaidLease,
        attempt(0), attempt(1), "final paid lease");
    let acknowledged = first.expect("the paused writer keeps the final lease");
    let refused = second.expect_err("the second writer must be refused");
    // Refused by the active-lease check itself, after A's commit, not by another gate.
    assert_eq!(refused, "rate_limit: active funding intent capacity reached");
    assert_eq!(acknowledged.payload.quote_id, contenders[0].payload.id);
    assert_eq!(acknowledged.payload.tx_hash, payments[0].tx_hash);
    assert_eq!(acknowledged.payload.registered_at, 102);
    assert_eq!(active_intents(&store, 102), MAX_PENDING as u64);
    let after = quote_work_protected_snapshot(&store);
    assert_only_winner_added(&before, &after, &contenders[0].payload.id,
        &contenders[1].payload.id, &[9], "final paid lease");
    let stored = store.funding_intent(&contenders[0].payload, &payments[0].raw_transaction)
        .unwrap().expect("the winner's acknowledgement is durable");
    assert_eq!(encode(&stored).unwrap(), encode(&acknowledged).unwrap());
    assert!(store.funding_intent(&contenders[1].payload, &payments[1].raw_transaction)
        .unwrap().is_none(), "the refused original has no acknowledgement");
    let retried = attempt(1)(&store).expect_err("the refused original stays refused");
    assert_eq!(retried, "rate_limit: active funding intent capacity reached");
    assert!(quote_work_protected_snapshot(&store) == after, "a refused retry leaves no state");
    drop(store);
    store = BillingStore::open(&path, &network()).unwrap();
    assert!(quote_work_protected_snapshot(&store) == after, "reopening preserves exactly one");
}

/// specify-non-reserving-quotes 4.3 (final free slot competition) and the atomic final
/// check of requirement "Authenticated paid admission preserves accepted commitments",
/// applied to authenticated free first openings, by a deterministic schedule rather than a
/// natural race. With one owned free slot left (seven earlier openings) and, separately,
/// one ledger-wide slot left (9,999 restored account rows), writer A passes the owned (8)
/// and ledger-wide (10,000) account checks and pauses inside its write transaction before
/// inserting its account; writer B, another free quote of the same owner on another
/// connection, starts and cannot pass its own checks while A is paused, and an independent
/// connection finds the write lock held. Released, A commits the only account and root
/// credential; B gets `rate_limit` after waiting (not a lock error) with no account or
/// credential and keeps its quote; a later retry changes nothing. Would catch the counts
/// and the insert split into two separately committed transactions, and counts read
/// without the write lock (a deferred transaction), both of which reach B's pause point
/// while A is paused; and an owned or ledger-wide count read before the write lock is taken,
/// or an off-by-one owned bound, which let B pass its checks after A's commit (B's seam is
/// released with A, so these fail rather than hang). B's exact refusal shows the account
/// check itself refused it.
#[test]
fn paused_free_opening_holds_the_final_owned_and_ledger_slot_checks_until_its_account_commits() {
    for ledger_wide in [false, true] {
        let context = if ledger_wide { "final ledger-wide free slot" } else { "final owned free slot" };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paused-free-slot.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        if ledger_wide {
            let template = quote(0, true);
            store.transaction(|tx| {
                for index in 0..MAX_PENDING - 1 {
                    tx.execute("INSERT INTO accounts(id,owner,quote,account,created) VALUES(?1,?2,?3,'{}',100)",
                        params![format!("restored-{index}"), format!("0x{index:040x}"), encode(&template)?]).map_err(sql)?;
                }
                Ok(())
            }).unwrap();
        } else {
            for _ in 0..7 {
                let earlier = quote(0, true);
                store.store_quote(&earlier, 100).unwrap();
                assert!(store.open_account(&earlier, None, 101).unwrap().credential.is_some());
            }
        }
        let contenders = [quote(0, true), quote(0, true)];
        for contender in &contenders {
            store.store_quote(contender, 100).unwrap();
        }
        let attempt = |index: usize| {
            let terms = contenders[index].clone();
            move |ledger: &BillingStore| ledger.open_account(&terms, None, 102)
        };
        let before = quote_work_protected_snapshot(&store);
        let (first, second) = race_paused_admission(&path, AdmissionPoint::FreeAccount,
            attempt(0), attempt(1), context);
        let opened = match first {
            Ok(opened) => opened,
            Err(error) => panic!("{context}: the paused writer keeps the final slot: {error}"),
        };
        let refused = match second {
            Err(error) => error,
            Ok(_) => panic!("{context}: the second writer must be refused"),
        };
        // Refused by the owned/ledger-wide account check itself, after A's commit.
        assert_eq!(refused, "rate_limit: free account limit reached", "{context}");
        assert_eq!(opened.account.contract_id, contenders[0].payload.id);
        assert_eq!(opened.account.status, "free");
        assert!(opened.credential.is_some(), "{context}: the winner gets its root credential");
        let after = quote_work_protected_snapshot(&store);
        assert_only_winner_added(&before, &after, &contenders[0].payload.id,
            &contenders[1].payload.id, &[4, 5], context);
        let retried = match attempt(1)(&store) {
            Err(error) => error,
            Ok(_) => panic!("{context}: the refused opening stays refused"),
        };
        assert_eq!(retried, "rate_limit: free account limit reached", "{context}");
        assert!(quote_work_protected_snapshot(&store) == after,
            "{context}: a refused retry leaves no state");
    }
}

#[test]
fn intent_owner_challenge_binds_body_payer_network_provider_and_consumes_once() {
    let store = memory();
    let wallet = crate::wallet::address_from_private_key(OWNER_KEY).unwrap();
    let provider = crate::wallet::address_from_private_key(PROVIDER_KEY).unwrap();
    let body = br#"{"raw_transaction":"0x01"}"#;
    let path = format!("/v1/contracts/{}/funding-intent", protocol::random_id());
    let payload = AuthPayload {
        network: network(),
        provider_wallet: provider.clone(),
        wallet: wallet.clone(),
        method: "POST".into(),
        path: path.clone(),
        body_hash: protocol::hash_hex(body),
        nonce: protocol::random_id(),
        expires_at: 220,
    };
    store.create_challenge(&payload, 100).unwrap();
    let signature = crate::wallet::sign_message(
        OWNER_KEY,
        &crate::auth::challenge_payload(&payload).unwrap(),
    )
    .unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-chiral-owner", wallet.parse().unwrap());
    headers.insert("x-chiral-nonce", payload.nonce.parse().unwrap());
    headers.insert("x-chiral-signature", signature.parse().unwrap());
    assert!(crate::auth::authenticate_owner(
        &store,
        &headers,
        &network(),
        &provider,
        "POST",
        &path,
        br#"{"raw_transaction":"0x02"}"#,
        101
    )
    .is_err());
    assert!(crate::auth::authenticate_owner(
        &store,
        &headers,
        &network(),
        &provider,
        "POST",
        "/v1/contracts/open",
        body,
        101
    )
    .is_err());
    assert!(crate::auth::authenticate_owner(
        &store,
        &headers,
        &network(),
        &wallet,
        "POST",
        &path,
        body,
        101
    )
    .is_err());
    let mut other_chain = network();
    other_chain.chain_id += 1;
    let mut other_genesis = network();
    other_genesis.genesis_hash.push('x');
    let mut other_deployment = network();
    other_deployment.deployment_id.push('x');
    for changed in [other_chain, other_genesis, other_deployment] {
        assert!(crate::auth::authenticate_owner(
            &store, &headers, &changed, &provider, "POST", &path, body, 101
        )
        .is_err());
    }
    headers.insert("x-chiral-owner", provider.parse().unwrap());
    assert!(crate::auth::authenticate_owner(
        &store,
        &headers,
        &network(),
        &provider,
        "POST",
        &path,
        body,
        101
    )
    .is_err());
    headers.insert("x-chiral-owner", wallet.parse().unwrap());
    assert!(crate::auth::authenticate_owner(
        &store,
        &headers,
        &network(),
        &provider,
        "POST",
        &path,
        body,
        101
    )
    .is_ok());
    assert!(crate::auth::authenticate_owner(
        &store,
        &headers,
        &network(),
        &provider,
        "POST",
        &path,
        body,
        102
    )
    .is_err());
}

#[test]
fn challenge_consumption_is_atomic_across_connections_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let p = AuthPayload {
        network: network(),
        provider_wallet: "provider".into(),
        wallet: "owner".into(),
        method: "POST".into(),
        path: "/v1/contracts/open".into(),
        body_hash: protocol::hash_hex(b"{}"),
        nonce: protocol::random_id(),
        expires_at: 200,
    };
    store.create_challenge(&p, 100).unwrap();
    drop(store);
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let store = BillingStore::open(&path, &network()).unwrap();
            let barrier = barrier.clone();
            let p = p.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.consume_challenge(&p.nonce, &p, 101)
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .filter(Result::is_ok)
            .count(),
        1
    );
}

#[test]
fn application_concurrency_rotation_and_revocation_do_not_release_running_holds() {
    let store = memory();
    let (q, _, _) = paid(&store);
    let options = CredentialIssueRequest {
        expires_at: 1000,
        spend_cap_wei: Some("900".into()),
        max_concurrency: 1,
        scopes: vec!["infer".into(), "read".into()],
    };
    let app = store
        .issue_credential(&q.payload.id, &options, 102)
        .unwrap();
    let reservation = store.reserve(&q.payload.id, &app.id, "running", 10, 20, 110, &store.account(&q.payload.id).unwrap().model)
        .unwrap();
    assert!(store
        .reserve(&q.payload.id, &app.id, "saturated", 10, 20, 111, &q.payload.model)
        .unwrap_err()
        .starts_with("saturation:"));
    let rotated = store
        .rotate_credential(&q.payload.id, &app.id, &options, 112)
        .unwrap();
    assert!(store.authenticate(&app.bearer, 113).is_err());
    assert!(store.authenticate(&rotated.bearer, 113).is_ok());
    assert_eq!(store.account(&q.payload.id).unwrap().reserved_wei, "30");
    assert_eq!(
        store
            .settle(
                &reservation.id,
                &TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 10
                },
                "cancelled",
                114
            )
            .unwrap()
            .cost_wei,
        "20"
    );
    assert_eq!(store.account(&q.payload.id).unwrap().reserved_wei, "0");
}

#[test]
fn fee_obligation_cannot_be_marked_paid_without_matching_chain_evidence() {
    let store = memory();
    let (q, _, _) = paid(&store);
    let mut batch = store.prepare_fee_batches(200, true).unwrap().remove(0);
    batch.status = "accepted".into();
    assert!(store.update_fee_batch(&batch).is_err());
    let raw = "0xab".to_string();
    let hash = protocol::hash_hex(&[0xab]);
    batch.tx_hash = Some(hash.clone());
    batch.raw_transaction = Some(raw.clone());
    let evidence = VerifiedPayment {
        network: network(),
        tx_hash: hash,
        from: q.payload.provider_wallet.clone(),
        to: batch.platform_wallet.clone(),
        value_wei: batch.amount_wei.clone(),
        data: batch.payment_data().unwrap(),
        block_hash: format!("0x{}", "55".repeat(32)),
        block_number: 100,
        raw_transaction: raw,
    };
    let mut wrong = evidence.clone();
    wrong.value_wei = "1".into();
    batch.evidence = Some(wrong);
    assert!(store.update_fee_batch(&batch).is_err());
    batch.evidence = Some(evidence);
    store.update_fee_batch(&batch).unwrap();
    // The fee view reports the book's states (inference-fee-remittance design
    // decision 4): an accepted batch's obligation is confirmed, not "remitted".
    let view = store.fee_summary().unwrap();
    assert_eq!(view["amounts_wei"]["confirmed"], "5");
    assert_eq!(view["amounts_wei"]["batched"], "0");
    assert_eq!(view["batches"][0]["state"], "confirmed");
    assert_eq!(view["batches"][0]["inclusion_block_number"], 100);
    batch.status = "orphaned".into();
    store.update_fee_batch(&batch).unwrap();
    // A remittance proven orphaned keeps its signed bytes, so its obligation is
    // submitted again, not batched, and its stale inclusion is not shown.
    let view = store.fee_summary().unwrap();
    assert_eq!(view["amounts_wei"]["submitted"], "5");
    assert_eq!(view["amounts_wei"]["batched"], "0");
    assert_eq!(view["amounts_wei"]["confirmed"], "0");
    assert_eq!(view["batches"][0]["state"], "submitted");
    assert_eq!(view["batches"][0]["tx_hash"], serde_json::json!(batch.tx_hash));
    assert!(view["batches"][0]["inclusion_block_hash"].is_null());
    assert!(view["batches"][0]["inclusion_block_number"].is_null());
    assert_eq!(
        store.prepare_fee_batches(300, true).unwrap()[0].id,
        batch.id
    );
}

/// A free account that owns fixture-only funding rows for fee obligations.
fn fee_fixture_account(store: &BillingStore) -> Quote {
    let q = quote(0, true);
    store.store_quote(&q, 100).unwrap();
    store.open_account(&q, None, 101).unwrap();
    q.payload
}
/// Fixture-only: records `count` credited payments on `q`'s account, each owing
/// a pending `amount`-wei fee obligation to `platform`, accepted at `created`.
/// The ledger API credits one payment per signed agreement; these rows are
/// inserted directly so aggregation can be exercised beyond 1,000 obligations.
fn pending_fee_obligations(
    store: &BillingStore,
    q: &Quote,
    platform: &str,
    amount: u128,
    created: u64,
    count: usize,
    label: &str,
) {
    let mut db = store.db.lock().unwrap();
    let tx = db.transaction().unwrap();
    for index in 0..count {
        let hash = protocol::hash_hex(format!("{label}:{index}").as_bytes());
        let mut evidence = payment(q, false, amount.saturating_mul(200), 0);
        evidence.tx_hash = hash.clone();
        let funding = FundingView {
            tx_hash: hash.clone(),
            contract_id: q.id.clone(),
            status: "accepted".into(),
            value_wei: Some(evidence.value_wei.clone()),
            fee_wei: Some(amount.to_string()),
            created_at: created,
        };
        tx.execute(
            "INSERT INTO funding(tx_hash,contract,record,evidence,status,checked) VALUES(?1,?2,?3,?4,'accepted',?5)",
            params![hash, q.id, encode(&funding).unwrap(), encode(&evidence).unwrap(), created],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO fee_obligations(tx_hash,platform,amount,created,status) VALUES(?1,?2,?3,?4,'pending')",
            params![hash, platform, amount.to_string(), created],
        )
        .unwrap();
    }
    tx.commit().unwrap();
}
fn fee_obligations_with_status(store: &BillingStore, status: &str) -> u64 {
    store
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM fee_obligations WHERE status=?1",
            [status],
            |r| r.get(0),
        )
        .unwrap()
}
/// Confirms `batch` with a real provider-signed remittance of its exact terms.
fn confirm_fee_batch(store: &BillingStore, mut batch: FeeBatch, nonce: u64) -> FeeBatch {
    let data = batch.payment_data().unwrap();
    let signed = crate::chain_client::sign_transaction(
        PROVIDER_KEY,
        network().chain_id,
        nonce,
        &batch.platform_wallet,
        money(&batch.amount_wei).unwrap(),
        &data,
        1,
        native_fixture_gas(&data),
    )
    .unwrap();
    batch.tx_hash = Some(signed.tx_hash.clone());
    batch.raw_transaction = Some(signed.raw_transaction.clone());
    batch.status = "accepted".into();
    batch.evidence = Some(VerifiedPayment {
        network: network(),
        tx_hash: signed.tx_hash,
        from: crate::wallet::address_from_private_key(PROVIDER_KEY).unwrap(),
        to: batch.platform_wallet.clone(),
        value_wei: batch.amount_wei.clone(),
        data,
        block_hash: format!("0x{}", "55".repeat(32)),
        block_number: 100,
        raw_transaction: signed.raw_transaction,
    });
    store.update_fee_batch(&batch).unwrap();
    batch
}

#[test]
fn fee_view_reports_the_four_book_states_from_obligation_and_batch_state() {
    // inference-fee-remittance requirements 3 and 11 (task 2.1). The ledger
    // stores obligations as `pending`, `batched` or `remitted`; recording a
    // batch's signed remittance changes only the batch, so the submitted state
    // exists only as the batch's state and the stored status alone cannot show
    // it (design decision 4).
    let store = memory();
    let q = fee_fixture_account(&store);
    let platform = format!("0x{}", "33".repeat(20));
    pending_fee_obligations(&store, &q, &platform, 13, 150, 1, "confirmed");
    let confirmed = store.prepare_fee_batches(200, true).unwrap().remove(0);
    let confirmed = confirm_fee_batch(&store, confirmed, 0);
    pending_fee_obligations(&store, &q, &platform, 11, 150, 1, "submitted");
    let mut submitted = store.prepare_fee_batches(201, true).unwrap().remove(0);
    let data = submitted.payment_data().unwrap();
    let signed = crate::chain_client::sign_transaction(
        PROVIDER_KEY,
        network().chain_id,
        1,
        &submitted.platform_wallet,
        11,
        &data,
        1,
        native_fixture_gas(&data),
    )
    .unwrap();
    submitted.tx_hash = Some(signed.tx_hash);
    submitted.raw_transaction = Some(signed.raw_transaction);
    submitted.status = "submitted".into();
    store.update_fee_batch(&submitted).unwrap();
    pending_fee_obligations(&store, &q, &platform, 7, 150, 1, "batched");
    let batched = store
        .prepare_fee_batches(202, true)
        .unwrap()
        .into_iter()
        .find(|batch| batch.status == "prepared")
        .unwrap();
    pending_fee_obligations(&store, &q, &platform, 5, 150, 1, "pending");

    let view = store.fee_summary().unwrap();
    assert_eq!(
        view["amounts_wei"],
        serde_json::json!({"pending":"5","batched":"7","submitted":"11","confirmed":"13"})
    );
    assert_eq!(view["cancelled_wei"], "0");
    assert_eq!(
        view["orphaned_amounts_wei"],
        serde_json::json!({"batched":"0","submitted":"0","confirmed":"0"})
    );
    let listed: Vec<_> = view["batches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|batch| (batch["id"].clone(), batch["state"].clone(), batch["amount_wei"].clone()))
        .collect();
    assert_eq!(
        listed,
        [(batched.id, "batched", "7"), (submitted.id, "submitted", "11"), (confirmed.id, "confirmed", "13")]
            .map(|(id, state, amount)| {
                (serde_json::Value::from(id), serde_json::Value::from(state), serde_json::Value::from(amount))
            })
    );
    // Stored statuses keep their names; orphan restoration depends on them.
    assert_eq!(fee_obligations_with_status(&store, "pending"), 1);
    assert_eq!(fee_obligations_with_status(&store, "batched"), 2);
    assert_eq!(fee_obligations_with_status(&store, "remitted"), 1);
}

#[test]
fn fee_view_lists_only_the_newest_100_batches_but_totals_every_obligation() {
    // inference-fee-remittance requirement 11 (task 2.2): the batch list is
    // bounded and newest first; the totals still cover every obligation.
    let store = memory();
    let q = fee_fixture_account(&store);
    let platform = format!("0x{}", "33".repeat(20));
    let mut sealed: Vec<String> = Vec::new();
    for index in 0..101u64 {
        let label = format!("bound:{index}");
        pending_fee_obligations(&store, &q, &platform, 5, 150, 1, &label);
        store.prepare_fee_batches(200 + index, true).unwrap();
        // The batch that sealed this obligation, read from the ledger itself.
        let batch: String = store
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT batch FROM fee_obligations WHERE tx_hash=?1",
                [protocol::hash_hex(format!("{label}:0").as_bytes())],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!sealed.contains(&batch));
        sealed.push(batch);
    }
    let view = store.fee_summary().unwrap();
    assert_eq!(view["amounts_wei"]["batched"], "505");
    let listed: Vec<_> = view["batches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|batch| batch["id"].as_str().unwrap().to_string())
        .collect();
    let newest: Vec<_> = sealed.iter().rev().take(100).cloned().collect();
    assert_eq!(listed, newest);
    assert!(!listed.contains(&sealed[0]));
}

#[test]
fn amount_threshold_is_evaluated_and_sealed_over_every_pending_obligation() {
    // inference-fee-remittance requirement 4 (task 2.3). The 1,000 oldest of
    // these obligations owe 900,000,000,000,000 wei, below 10^15, but the
    // wallet owes 1,080,000,000,000,000 wei over all 1,200 of them.
    let store = memory();
    let q = fee_fixture_account(&store);
    let now = 10_000;
    let wallet = format!("0x{}", "aa".repeat(20));
    let other = format!("0x{}", "bb".repeat(20));
    pending_fee_obligations(&store, &q, &wallet, 900_000_000_000, now - 3_600, 1_200, "many");
    // Another wallet below both thresholds is neither sealed nor combined.
    pending_fee_obligations(&store, &q, &other, 999_999_999_999_999, now - 3_600, 1, "other");
    let batches = store.prepare_fee_batches(now, false).unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].platform_wallet, wallet);
    assert_eq!(batches[0].amount_wei, "1080000000000000");
    let members: u64 = store
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM fee_obligations WHERE batch=?1 AND status='batched' AND platform=?2",
            params![batches[0].id, wallet],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(members, 1_200);
    assert_eq!(fee_obligations_with_status(&store, "pending"), 1);
    let pending: String = store
        .db
        .lock()
        .unwrap()
        .query_row("SELECT platform FROM fee_obligations WHERE status='pending'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pending, other);
    // A later pass seals nothing new and offers the same immutable batch.
    let again = store.prepare_fee_batches(now + 30, false).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, batches[0].id);
    assert_eq!(again[0].amount_wei, "1080000000000000");
}

#[test]
fn explicit_remittance_seals_every_pending_obligation_but_never_a_cancelled_one() {
    // inference-fee-remittance requirement 6 (task 2.3): `fees/remit` leaves no
    // obligation pending, whatever the count, amount or age.
    let store = memory();
    let (paid_quote, paid_payment, _) = paid(&store);
    // Funding proven orphaned before sealing cancels its pending obligation.
    store
        .mark_funding_state(&paid_payment.tx_hash, "orphaned", 102)
        .unwrap();
    assert_eq!(fee_obligations_with_status(&store, "cancelled"), 1);
    let wallet = paid_quote.payload.platform_wallet.clone();
    let other = format!("0x{}", "bb".repeat(20));
    let q = fee_fixture_account(&store);
    pending_fee_obligations(&store, &q, &wallet, 5, 200, 1_001, "forced");
    pending_fee_obligations(&store, &q, &other, 9, 200, 2, "other");
    let batches = store.prepare_fee_batches(300, true).unwrap();
    assert_eq!(fee_obligations_with_status(&store, "pending"), 0);
    assert_eq!(fee_obligations_with_status(&store, "batched"), 1_003);
    let amounts: std::collections::BTreeMap<_, _> = batches
        .iter()
        .map(|batch| (batch.platform_wallet.clone(), batch.amount_wei.clone()))
        .collect();
    assert_eq!(batches.len(), 2);
    assert_eq!(amounts[&wallet], "5005");
    assert_eq!(amounts[&other], "18");
    let (status, batch): (String, Option<String>) = store
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT status,batch FROM fee_obligations WHERE tx_hash=?1",
            [&paid_payment.tx_hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "cancelled");
    assert!(batch.is_none());
}

#[test]
fn a_wallet_splits_into_several_batches_only_at_u128_overflow() {
    // inference-fee-remittance requirements 4 and 5 (task 2.3).
    let store = memory();
    let q = fee_fixture_account(&store);
    let wallet = format!("0x{}", "cc".repeat(20));
    let half = 1u128 << 127;
    pending_fee_obligations(&store, &q, &wallet, half, 200, 1, "first");
    pending_fee_obligations(&store, &q, &wallet, half - 1, 201, 1, "second");
    pending_fee_obligations(&store, &q, &wallet, half, 202, 1, "third");
    let batches = store.prepare_fee_batches(300, false).unwrap();
    // The two oldest exactly fill one transfer value; only the third overflows.
    assert_eq!(
        batches.iter().map(|batch| batch.amount_wei.clone()).collect::<Vec<_>>(),
        vec![u128::MAX.to_string(), half.to_string()]
    );
    assert_eq!(fee_obligations_with_status(&store, "pending"), 0);
    let first: u64 = store
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM fee_obligations WHERE batch=?1",
            [&batches[0].id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(first, 2);
}

#[test]
fn every_confirmed_fee_batch_is_rechecked_whatever_the_pass_timing() {
    // inference-fee-remittance requirement 9 (task 2.4): passes 60 seconds
    // apart used to read the same page of 100, never the other 50.
    let store = memory();
    let q = fee_fixture_account(&store);
    for index in 0..150u64 {
        pending_fee_obligations(&store, &q, &q.platform_wallet, 5, 100, 1, &format!("confirmed-{index}"));
        let batch = store.prepare_fee_batches(200, true).unwrap().remove(0);
        confirm_fee_batch(&store, batch, index);
    }
    let mut seen = std::collections::BTreeSet::new();
    for now in [0, 60, 120, 180] {
        let page = store.fee_batches_to_recheck(now).unwrap();
        assert_eq!(page.len(), 100);
        for batch in page {
            assert_eq!(batch.status, "accepted");
            store.mark_fee_batch_checked(&batch.id, now).unwrap();
            seen.insert(batch.id);
        }
    }
    assert_eq!(seen.len(), 150);
    // A clock that moved backwards does not starve batches checked "later".
    let mut regressed = std::collections::BTreeSet::new();
    for now in [90, 100] {
        for batch in store.fee_batches_to_recheck(now).unwrap() {
            store.mark_fee_batch_checked(&batch.id, now).unwrap();
            regressed.insert(batch.id);
        }
    }
    assert_eq!(regressed.len(), 150);
    assert!(store
        .mark_fee_batch_checked(&protocol::random_id(), 200)
        .unwrap_err()
        .starts_with("not_found:"));
}

#[test]
fn confirmed_fee_batches_are_rechecked_least_recently_checked_first() {
    // inference-fee-remittance requirement 9 (task 2.4): never-checked batches
    // come first in sealing order, then check times in numeric order (9 before
    // 10, which a text comparison would reverse).
    let store = memory();
    let q = fee_fixture_account(&store);
    let ids: Vec<String> = (0..4u64)
        .map(|index| {
            pending_fee_obligations(&store, &q, &q.platform_wallet, 5, 100, 1, &format!("ordered-{index}"));
            let batch = store.prepare_fee_batches(200, true).unwrap().remove(0);
            confirm_fee_batch(&store, batch, index).id
        })
        .collect();
    store.mark_fee_batch_checked(&ids[0], 10).unwrap();
    store.mark_fee_batch_checked(&ids[1], 9).unwrap();
    let order = |now| {
        store
            .fee_batches_to_recheck(now)
            .unwrap()
            .into_iter()
            .map(|batch| batch.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(order(20), [&ids[2], &ids[3], &ids[1], &ids[0]].map(String::clone));
    // A check time later than `now` (the clock moved backwards) sorts as never
    // checked, in sealing order among the never-checked batches.
    assert_eq!(order(9), [&ids[0], &ids[2], &ids[3], &ids[1]].map(String::clone));
}

#[tokio::test]
async fn fee_recheck_time_advances_after_unavailable_and_faulted_outcomes() {
    // inference-fee-remittance requirement 9 (task 2.4): maintenance advances a
    // confirmed batch's check time after every outcome, so a batch whose
    // recheck keeps failing never starves monitoring of the others.
    use crate::{
        app::ProviderState,
        chain_client::ChainClient,
        contracts,
        network::NetworkConfig,
        offer_registry::OfferRegistry,
        provider_config::{DhtConfig, ProviderConfig},
    };
    use std::sync::atomic::{AtomicBool, AtomicU64};
    let dir = tempfile::tempdir().unwrap();
    let fixture = Arc::new(ContinuityRpc {
        outage: AtomicBool::new(true),
        orphan: AtomicBool::new(false),
        frontier: AtomicU64::new(10),
        nonce: AtomicU64::new(0),
        owner_balance: std::sync::Mutex::new(u128::MAX),
        receipt: std::sync::Mutex::new(serde_json::Value::Null),
        observed_at: protocol::now().unwrap(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let network_config = NetworkConfig {
        identity: network(),
        rpc_url: format!("http://{}", listener.local_addr().unwrap()),
        rpc_bearer_file: None,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    };
    let rpc = axum::Router::new()
        .route("/", axum::routing::post(continuity_rpc))
        .with_state(fixture.clone());
    let rpc_task = tokio::spawn(async move {
        axum::serve(listener, rpc).await.unwrap();
    });
    let ledger = Arc::new(memory());
    let q = fee_fixture_account(&ledger);
    let mut ids = Vec::new();
    for index in 0..2u64 {
        pending_fee_obligations(&ledger, &q, &q.platform_wallet, 5, 100, 1, &format!("monitored-{index}"));
        let batch = ledger.prepare_fee_batches(200, true).unwrap().remove(0);
        ids.push(confirm_fee_batch(&ledger, batch, index).id);
    }
    let (faulted, healthy) = (ids[0].clone(), ids[1].clone());
    // Fixture-only ledger fault: the older confirmed batch loses its evidence.
    let original: String = ledger
        .db
        .lock()
        .unwrap()
        .query_row("SELECT record FROM fee_batches WHERE id=?1", [&faulted], |r| r.get(0))
        .unwrap();
    ledger
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE fee_batches SET record=json_set(record,'$.evidence',json('null')) WHERE id=?1",
            [&faulted],
        )
        .unwrap();
    let config = ProviderConfig {
        network: network_config.clone(),
        data_dir: dir.path().into(),
        signing_key_file: dir.path().join("key"),
        admin_token_file: dir.path().join("admin"),
        public_endpoint: "http://127.0.0.1:9419".into(),
        public_bind: "127.0.0.1:9419".parse().unwrap(),
        admin_bind: "127.0.0.1:9421".parse().unwrap(),
        platform_wallet: q.platform_wallet.clone(),
        fee_bps: 50,
        min_funding_wei: "1000".into(),
        models: vec![],
        dht: DhtConfig {
            listen: "/ip4/127.0.0.1/tcp/0".into(),
            bootstrap: vec![],
            identity_file: dir.path().join("dht"),
        },
        development: true,
    };
    let state = Arc::new(ProviderState {
        model_slots: ProviderState::slots(&config),
        managed_engines: Default::default(),
        config: Arc::new(config),
        chain: Arc::new(ChainClient::new(network_config, dir.path().into()).unwrap()),
        ledger: ledger.clone(),
        private_key: Arc::new(PROVIDER_KEY.into()),
        wallet: q.provider_wallet.clone(),
        offers: Arc::new(OfferRegistry::new(network(), true)),
        accepting: AtomicBool::new(true),
        settlement_fault: Default::default(),
        funding_ready: AtomicBool::new(true),
        qualified_models: std::sync::RwLock::new(Default::default()),
        embedding_dimensions: std::sync::RwLock::new(Default::default()),
    });
    let checked = |id: &str| -> Option<u64> {
        ledger
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM metadata WHERE key=?1",
                [format!("{FEE_BATCH_CHECKED}{id}")],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
    };
    let fault = Some("Accepted fee batch is missing canonical evidence");

    // Pass 1 ends at the faulted batch, but still records its check.
    let before = protocol::now().unwrap();
    let report = contracts::reconcile(state.clone()).await.unwrap();
    let after = protocol::now().unwrap();
    assert_eq!(report.fee_error.as_deref(), fault);
    assert!(checked(&faulted).is_some_and(|time| (before..=after).contains(&time)));
    assert_eq!(checked(&healthy), None);

    // Pass 2 reaches the never-checked batch first; its unavailable recheck is
    // recorded and changes nothing else.
    let before = protocol::now().unwrap();
    let report = contracts::reconcile(state.clone()).await.unwrap();
    let after = protocol::now().unwrap();
    assert_eq!(report.fee_error.as_deref(), fault);
    assert!(checked(&healthy).is_some_and(|time| (before..=after).contains(&time)));
    assert!(checked(&faulted).is_some_and(|time| (before..=after).contains(&time)));

    // With the fault repaired, every batch's unavailable recheck is recorded.
    ledger
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE fee_batches SET record=?2 WHERE id=?1",
            params![faulted, original],
        )
        .unwrap();
    let before = protocol::now().unwrap();
    let report = contracts::reconcile(state.clone()).await.unwrap();
    let after = protocol::now().unwrap();
    assert!(report.fee_error.unwrap().contains("unavailable"));
    for id in [&faulted, &healthy] {
        assert!(checked(id).is_some_and(|time| (before..=after).contains(&time)));
    }
    let batches = ledger.fee_batches().unwrap();
    assert_eq!(batches.len(), 2);
    for batch in batches {
        assert_eq!(batch.status, "accepted");
        assert_eq!(batch.evidence.unwrap().block_number, 100);
    }
    rpc_task.abort();
}

#[test]
fn embedding_batches_have_a_separate_bounded_aggregate_token_limit() {
    let store = memory();
    let mut q = quote(0, true);
    q.payload.model.capabilities = vec!["embeddings".into()];
    q = protocol::sign(protocol::QUOTE_DOMAIN, q.payload, PROVIDER_KEY).unwrap();
    store.store_quote(&q, 100).unwrap();
    let open = store.open_account(&q, None, 101).unwrap();
    let context = q.payload.model.context_length;
    assert!(store
        .reserve(
            &q.payload.id,
            &open.credential.as_ref().unwrap().id,
            "batch",
            context * 32,
            0,
            110,
            &q.payload.model
        )
        .is_ok());
    assert!(store
        .reserve(
            &q.payload.id,
            &open.credential.as_ref().unwrap().id,
            "oversized",
            context * 32 + 1,
            0,
            111,
            &q.payload.model
        )
        .is_err());
    assert!(store
        .reserve(
            &q.payload.id,
            &open.credential.as_ref().unwrap().id,
            "generation",
            context * 2,
            1,
            112,
            &q.payload.model
        )
        .is_err());
}

#[test]
fn one_use_challenges_are_expiring_and_pending_state_is_bounded() {
    let store = memory();
    let mut payload = AuthPayload {
        network: network(),
        provider_wallet: "provider".into(),
        wallet: "owner".into(),
        method: "POST".into(),
        path: "/v1/contracts/open".into(),
        body_hash: protocol::hash_hex(b"{}"),
        nonce: protocol::random_id(),
        expires_at: 220,
    };
    for _ in 0..8 {
        payload.nonce = protocol::random_id();
        store.create_challenge(&payload, 100).unwrap();
    }
    payload.nonce = protocol::random_id();
    assert!(store
        .create_challenge(&payload, 100)
        .unwrap_err()
        .starts_with("rate_limit:"));
    payload.expires_at = 340;
    store.create_challenge(&payload, 220).unwrap();
    assert!(store
        .consume_challenge(&payload.nonce, &payload, 340)
        .is_err());
}

#[test]
fn funding_transport_failure_preserves_last_observation_but_not_a_canonical_freeze() {
    use crate::chain_client::{ChainError, PaymentState};
    let store = memory();
    let (q, p, open) = paid(&store);
    let unavailable = Err(ChainError::Unavailable("connection refused".into()));
    store
        .apply_funding_recheck(&p.tx_hash, &unavailable, 110)
        .unwrap();
    assert_eq!(store.account(&q.payload.id).unwrap().status, "active");
    let request = store.reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "offline", 10, 10, 111, &store.account(&q.payload.id).unwrap().model)
        .unwrap();
    store
        .settle(
            &request.id,
            &TokenUsage::default(),
            "cancelled_unknown_usage",
            112,
        )
        .unwrap();
    store
        .apply_funding_recheck(&p.tx_hash, &Ok(PaymentState::Pending), 113)
        .unwrap();
    store
        .apply_funding_recheck(&p.tx_hash, &unavailable, 114)
        .unwrap();
    assert_eq!(
        store.account(&q.payload.id).unwrap().status,
        "settlement_pending"
    );
    assert!(store
        .reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "pending", 1, 1, 115, &q.payload.model)
        .is_err());
    store
        .apply_funding_recheck(&p.tx_hash, &Ok(PaymentState::Accepted(p.clone())), 116)
        .unwrap();
    assert_eq!(store.account(&q.payload.id).unwrap().status, "active");
    store
        .apply_funding_recheck(&p.tx_hash, &Ok(PaymentState::Orphaned), 117)
        .unwrap();
    let account = store.account(&q.payload.id).unwrap();
    assert_eq!(account.status, "frozen");
    assert_eq!(account.reversed_wei, "1000");
    store
        .apply_funding_recheck(&p.tx_hash, &unavailable, 118)
        .unwrap();
    assert_eq!(store.account(&q.payload.id).unwrap().available_wei, "0");
}

// HTTP fixture exercises ChainClient's signed-payment and canonical verification
// boundary; unlike the ledger-only fixtures above, these are real signed bytes.
struct ContinuityRpc {
    outage: std::sync::atomic::AtomicBool,
    orphan: std::sync::atomic::AtomicBool,
    frontier: std::sync::atomic::AtomicU64,
    nonce: std::sync::atomic::AtomicU64,
    owner_balance: std::sync::Mutex<u128>,
    receipt: std::sync::Mutex<serde_json::Value>,
    observed_at: u64,
}
async fn continuity_rpc(
    axum::extract::State(state): axum::extract::State<Arc<ContinuityRpc>>,
    axum::Json(request): axum::Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use std::sync::atomic::Ordering;
    if state.outage.load(Ordering::Acquire) {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let result = match request["method"].as_str().unwrap() {
        "eth_chainId" => serde_json::json!("0x47"),
        "eth_syncing" => serde_json::json!(false),
        "eth_getBlockByNumber" => {
            assert_eq!(request["params"][1], false);
            let tag = request["params"][0].as_str().unwrap();
            let height = match tag {
                "latest" => 12,
                "safe" => state.frontier.load(Ordering::Acquire),
                "finalized" => 6,
                _ => crate::rpc_client::hex_to_u64(tag).unwrap(),
            };
            let hash = if height == 0 {
                network().genesis_hash
            } else if state.orphan.load(Ordering::Acquire) {
                format!("0x{}", "ff".repeat(32))
            } else {
                format!("0x{}", (if height == 9 { "44" } else if height == 8 { "66" } else { "55" }).repeat(32))
            };
            let timestamp = state.observed_at.saturating_sub(12u64.saturating_sub(height));
            serde_json::json!({"number":format!("0x{height:x}"), "hash":hash, "timestamp":format!("0x{timestamp:x}")})
        }
        "eth_getTransactionCount" => {
            serde_json::json!(format!("0x{:x}", state.nonce.load(Ordering::Acquire)))
        }
        "eth_gasPrice" => serde_json::json!("0x1"),
        "eth_estimateGas" => {
            let data = hex::decode(request["params"][0]["data"].as_str().unwrap().strip_prefix("0x").unwrap()).unwrap();
            serde_json::json!(format!("0x{:x}", native_fixture_gas(&data)))
        }
        "eth_getBalance" => {
            if request["params"][0]
                == crate::wallet::address_from_private_key(PROVIDER_KEY).unwrap()
            {
                serde_json::json!("0x0")
            } else {
                serde_json::json!(format!("0x{:x}", *state.owner_balance.lock().unwrap()))
            }
        }
        "eth_getTransactionReceipt" => {
            if state.orphan.load(Ordering::Acquire) {
                serde_json::Value::Null
            } else {
                state.receipt.lock().unwrap().clone()
            }
        }
        other => panic!("unexpected continuity RPC method: {other}"),
    };
    axum::Json(serde_json::json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
        .into_response()
}

#[tokio::test]
async fn rpc_outage_fee_gas_failure_and_reorg_keep_distinct_admission_states() {
    use crate::{
        app::ProviderState,
        chain_client::ChainClient,
        contracts,
        network::NetworkConfig,
        offer_registry::OfferRegistry,
        provider_config::{DhtConfig, ModelConfig, ProviderConfig},
    };
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let fixture = Arc::new(ContinuityRpc {
        outage: AtomicBool::new(false),
        orphan: AtomicBool::new(false),
        frontier: AtomicU64::new(10),
        nonce: AtomicU64::new(0),
        owner_balance: std::sync::Mutex::new(u128::MAX),
        receipt: std::sync::Mutex::new(serde_json::Value::Null),
        observed_at: protocol::now().unwrap(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let network_config = NetworkConfig {
        identity: network(),
        rpc_url: format!("http://{}", listener.local_addr().unwrap()),
        rpc_bearer_file: None,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    };
    let rpc = axum::Router::new()
        .route("/", axum::routing::post(continuity_rpc))
        .with_state(fixture.clone());
    let rpc_task = tokio::spawn(async move {
        axum::serve(listener, rpc).await.unwrap();
    });
    let chain = Arc::new(ChainClient::new(network_config.clone(), dir.path().into()).unwrap());
    let ledger = Arc::new(memory());
    let mut payload = quote(1000, false).payload;
    let now = protocol::now().unwrap();
    payload.issued_at = now;
    payload.expires_at = now + 300;
    let q = protocol::sign(protocol::QUOTE_DOMAIN, payload, PROVIDER_KEY).unwrap();
    ledger.store_quote(&q, now).unwrap();
    let data = protocol::opening_data(&q.payload).unwrap();
    let signed = chain
        .prepare_payment_once(
            "continuity-opening",
            OWNER_KEY,
            &q.payload.provider_wallet,
            "1000",
            &data,
        )
        .await
        .unwrap();
    *fixture.receipt.lock().unwrap() = serde_json::json!({"transactionHash":signed.tx_hash,"from":q.payload.consumer_wallet,"to":q.payload.provider_wallet,"type":"0x0","gasUsed":format!("0x{:x}", native_fixture_gas(&data)),"status":"0x1","blockHash":format!("0x{}","44".repeat(32)),"blockNumber":"0x9"});
    let payment = chain
        .verify_payment(
            &signed.raw_transaction,
            &q.payload.consumer_wallet,
            &q.payload.provider_wallet,
            Some("1000"),
            &data,
        )
        .await
        .unwrap();
    register(&ledger, &q, &payment, now);
    let opened = ledger.open_account(&q, Some(&payment), now).unwrap();
    let engine_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let engine_url = format!("http://{}", engine_listener.local_addr().unwrap());
    let engine=axum::Router::new()
        .route("/health",axum::routing::get(||async {axum::http::StatusCode::OK}))
        .route("/apply-template",axum::routing::post(||async {axum::Json(serde_json::json!({"prompt":"hello"}))}))
        .route("/tokenize",axum::routing::post(||async {axum::Json(serde_json::json!({"tokens":[1]}))}))
        .route("/v1/chat/completions",axum::routing::post(||async {
            ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], concat!(
                "data: {\"id\":\"continuity-response\",\"object\":\"chat.completion.chunk\",\"model\":\"test/model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"continuity-response\",\"object\":\"chat.completion.chunk\",\"model\":\"test/model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: {\"id\":\"continuity-response\",\"object\":\"chat.completion.chunk\",\"model\":\"test/model\",\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
                "data: [DONE]\n\n"))
        }));
    let engine_task = tokio::spawn(async move {
        axum::serve(engine_listener, engine).await.unwrap();
    });
    let model = ModelConfig {
        model: q.payload.model.clone(),
        price: q.payload.price.clone(),
        engine_url,
        engine_api_key_file: None,
        max_concurrency: 10,
        request_timeout_secs: 5,
        qualification_evidence_file: None,
        runtime: None,
    };
    let config = ProviderConfig {
        network: network_config,
        data_dir: dir.path().into(),
        signing_key_file: dir.path().join("key"),
        admin_token_file: dir.path().join("admin"),
        public_endpoint: "http://127.0.0.1:9419".into(),
        public_bind: "127.0.0.1:9419".parse().unwrap(),
        admin_bind: "127.0.0.1:9421".parse().unwrap(),
        platform_wallet: q.payload.platform_wallet.clone(),
        fee_bps: 50,
        min_funding_wei: "1000".into(),
        models: vec![model],
        dht: DhtConfig {
            listen: "/ip4/127.0.0.1/tcp/0".into(),
            bootstrap: vec![],
            identity_file: dir.path().join("dht"),
        },
        development: true,
    };
    let state = Arc::new(ProviderState {
        model_slots: ProviderState::slots(&config),
        managed_engines: Default::default(),
        config: Arc::new(config),
        chain,
        ledger: ledger.clone(),
        private_key: Arc::new(PROVIDER_KEY.into()),
        wallet: q.payload.provider_wallet.clone(),
        offers: Arc::new(OfferRegistry::new(network(), true)),
        accepting: AtomicBool::new(true),
        settlement_fault: Default::default(),
        funding_ready: AtomicBool::new(true),
        qualified_models: std::sync::RwLock::new(
            [q.payload.model.id.clone()].into_iter().collect(),
        ),
        embedding_dimensions: std::sync::RwLock::new(Default::default()),
    });
    // Force sealing a batch so maintenance actually attempts a gas-starved send.
    ledger.prepare_fee_batches(now, true).unwrap();
    let report = contracts::reconcile(state.clone()).await.unwrap();
    assert!(report.funding_error.is_none());
    assert!(report.fee_error.unwrap().contains("insufficient"));
    assert_eq!(ledger.account(&q.payload.id).unwrap().status, "active");
    assert_eq!(ledger.fee_batches().unwrap()[0].status, "prepared");

    fixture.outage.store(true, Ordering::Release);
    state.funding_ready.store(false, Ordering::Release);
    let report = contracts::reconcile(state.clone()).await.unwrap();
    assert!(report.funding_error.unwrap().contains("unavailable"));
    assert!(!report.funding_rejected);
    assert_eq!(ledger.account(&q.payload.id).unwrap().status, "active");
    assert!(state
        .chain
        .verify_payment(
            &signed.raw_transaction,
            &q.payload.consumer_wallet,
            &q.payload.provider_wallet,
            Some("1000"),
            &data
        )
        .await
        .is_err());
    let reservation = ledger.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "offline-inference", 1, 1, now, &ledger.account(&q.payload.id).unwrap().model)
        .unwrap();
    ledger
        .settle(
            &reservation.id,
            &TokenUsage::default(),
            "cancelled_unknown_usage",
            now,
        )
        .unwrap();

    // No offers exist and new funding is paused, but the exact qualified model
    // remains visible to the paid account. Drain and lost qualification hide it.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router =
        crate::inference_gateway::router(state.clone()).merge(contracts::router(state.clone()));
    let gateway_task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::new();
    let models = client
        .get(format!("{base}/v1/models"))
        .bearer_auth(&opened.credential.as_ref().unwrap().bearer)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(models["data"].as_array().unwrap().len(), 1);
    let inference = serde_json::json!({"model":q.payload.model.id,"messages":[{"role":"user","content":"Hi"}],"max_tokens":1});
    let response = client
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&opened.credential.as_ref().unwrap().bearer)
        .header("idempotency-key", "offline-http-inference")
        .json(&inference)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(ledger.account(&q.payload.id).unwrap().spent_wei, "2");
    assert_eq!(
        client
            .post(format!("{base}/v1/contracts/propose"))
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    );
    state.accepting.store(false, Ordering::Release);
    let models = client
        .get(format!("{base}/v1/models"))
        .bearer_auth(&opened.credential.as_ref().unwrap().bearer)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert!(models["data"].as_array().unwrap().is_empty());
    assert_eq!(
        client
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth(&opened.credential.as_ref().unwrap().bearer)
            .json(&inference)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    );
    state.accepting.store(true, Ordering::Release);
    state.qualified_models.write().unwrap().clear();
    let models = client
        .get(format!("{base}/v1/models"))
        .bearer_auth(&opened.credential.as_ref().unwrap().bearer)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert!(models["data"].as_array().unwrap().is_empty());
    assert_eq!(
        client
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth(&opened.credential.as_ref().unwrap().bearer)
            .json(&inference)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    );
    state
        .qualified_models
        .write()
        .unwrap()
        .insert(q.payload.model.id.clone());
    let mut changed = q.payload.model.clone();
    changed.revision.push_str("-changed");
    assert!(state.qualified_model(&changed).is_none());

    fixture.outage.store(false, Ordering::Release);
    fixture.frontier.store(8, Ordering::Release);
    contracts::reconcile(state.clone()).await.unwrap();
    assert_eq!(
        ledger.account(&q.payload.id).unwrap().status,
        "settlement_pending"
    );
    assert!(ledger
        .reserve(
            &q.payload.id,
            &opened.credential.as_ref().unwrap().id,
            "pending-inference",
            1,
            1,
            now,
            &q.payload.model
        )
        .is_err());
    fixture.frontier.store(10, Ordering::Release);
    contracts::reconcile(state.clone()).await.unwrap();
    assert_eq!(ledger.account(&q.payload.id).unwrap().status, "active");
    fixture.orphan.store(true, Ordering::Release);
    contracts::reconcile(state.clone()).await.unwrap();
    assert_eq!(ledger.account(&q.payload.id).unwrap().status, "frozen");
    assert_eq!(ledger.account(&q.payload.id).unwrap().reversed_wei, "1000");
    assert_eq!(
        ledger.account(&q.payload.id).unwrap().provider_loss_wei,
        "7"
    );
    fixture.orphan.store(false, Ordering::Release);
    fixture.receipt.lock().unwrap()["blockHash"] =
        serde_json::json!(format!("0x{}", "66".repeat(32)));
    fixture.receipt.lock().unwrap()["blockNumber"] = serde_json::json!("0x8");
    for _ in 0..2 {
        contracts::reconcile(state.clone()).await.unwrap();
    }
    let restored = ledger.account(&q.payload.id).unwrap();
    assert_eq!(restored.status, "active");
    assert_eq!(restored.reversed_wei, "0");
    assert_eq!(restored.provider_loss_wei, "0");
    assert_eq!(restored.funded_wei, "1000");
    assert_eq!(restored.spent_wei, "2");
    assert_eq!(restored.available_wei, "993");

    // A recheck the ledger refuses to apply must not abort maintenance
    // (inference-funding-settlement requirement 9, task 2.5). A fixture-only
    // trigger refuses this payment's status transition.
    let tx_hash: String = ledger
        .db
        .lock()
        .unwrap()
        .query_row("SELECT tx_hash FROM funding", [], |row| row.get(0))
        .unwrap();
    ledger
        .db
        .lock()
        .unwrap()
        .execute_batch(&format!(
            "CREATE TRIGGER refuse_status BEFORE UPDATE OF status ON funding WHEN NEW.tx_hash='{tx_hash}' BEGIN SELECT RAISE(FAIL,'fixture refusal'); END;"
        ))
        .unwrap();
    fixture.orphan.store(true, Ordering::Release);
    state.funding_ready.store(true, Ordering::Release);
    let report = contracts::reconcile(state.clone()).await.unwrap();
    assert_eq!(report.funding_rechecks_quarantined, 1);
    assert!(report
        .funding_error
        .as_deref()
        .unwrap()
        .contains("held for operator review"));
    assert!(!state.funding_ready.load(Ordering::Acquire));
    let held = ledger.account(&q.payload.id).unwrap();
    assert_eq!(held.status, "active");
    assert_eq!(held.reversed_wei, "0");
    assert_eq!(held.available_wei, "993");
    // Fail-closed persists while the refusal does, on every later pass.
    let report = contracts::reconcile(state.clone()).await.unwrap();
    assert_eq!(report.funding_rechecks_quarantined, 1);
    assert!(report.funding_error.is_some());
    ledger
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_status;")
        .unwrap();
    fixture.orphan.store(false, Ordering::Release);
    for _ in 0..2 {
        contracts::reconcile(state.clone()).await.unwrap();
    }
    let report = contracts::reconcile(state.clone()).await.unwrap();
    assert_eq!(report.funding_rechecks_quarantined, 0);
    assert!(report.funding_error.is_none());
    assert!(ledger.quarantined_funding_rechecks().unwrap().is_empty());
    assert_eq!(ledger.account(&q.payload.id).unwrap().status, "active");
    gateway_task.abort();
    engine_task.abort();
    rpc_task.abort();
}

#[test]
fn refused_recheck_is_quarantined_without_stalling_other_payments() {
    // inference-funding-settlement requirement 9 (task 2.5).
    use crate::chain_client::PaymentState;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite3");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (q1, p1, _) = paid(&store);
    let q2 = quote(1000, false);
    store.store_quote(&q2, 100).unwrap();
    let p2 = payment(&q2.payload, true, 1000, 2);
    register(&store, &q2, &p2, 100);
    store.open_account(&q2, Some(&p2), 101).unwrap();
    assert_eq!(store.funding_to_recheck().unwrap()[0].tx_hash, p1.tx_hash);
    let before = encode(&store.account(&q1.payload.id).unwrap()).unwrap();
    let mut substituted = p1.clone();
    substituted.value_wei = "2000".into();
    assert!(store
        .apply_funding_recheck(&p1.tx_hash, &Ok(PaymentState::Accepted(substituted)), 110)
        .is_err());
    store.quarantine_funding_recheck(&p1.tx_hash, 110).unwrap();
    // Credit and evidence are untouched, and the scan now reaches the other payment first.
    assert_eq!(encode(&store.account(&q1.payload.id).unwrap()).unwrap(), before);
    assert_eq!(store.funding_to_recheck().unwrap()[0].tx_hash, p2.tx_hash);
    assert_eq!(store.quarantined_funding_rechecks().unwrap(), vec![p1.clone()]);
    drop(store);
    // The quarantine survives reopening the ledger.
    let store = BillingStore::open(&path, &network()).unwrap();
    assert_eq!(store.quarantined_funding_rechecks().unwrap(), vec![p1.clone()]);
    store
        .apply_funding_recheck(&p1.tx_hash, &Ok(PaymentState::Accepted(p1.clone())), 111)
        .unwrap();
    store.clear_funding_recheck_quarantine(&p1.tx_hash).unwrap();
    assert!(store.quarantined_funding_rechecks().unwrap().is_empty());
    assert_eq!(encode(&store.account(&q1.payload.id).unwrap()).unwrap(), before);
    assert!(store
        .quarantine_funding_recheck(&format!("0x{}", "00".repeat(32)), 112)
        .is_err());
}

#[test]
fn identical_reincluded_funding_only_refreshes_verified_inclusion() {
    use crate::chain_client::PaymentState;
    let store = memory();
    let (q, p, _) = paid(&store);
    let before = encode(&store.account(&q.payload.id).unwrap()).unwrap();
    let mut refreshed = p.clone();
    refreshed.block_hash = format!("0x{}", "66".repeat(32));
    refreshed.block_number = 11;
    for now in [110, 111] {
        store
            .apply_funding_recheck(
                &p.tx_hash,
                &Ok(PaymentState::Accepted(refreshed.clone())),
                now,
            )
            .unwrap();
        assert_eq!(
            encode(&store.account(&q.payload.id).unwrap()).unwrap(),
            before
        );
        assert_eq!(store.funding_to_recheck().unwrap(), vec![refreshed.clone()]);
        assert_eq!(store.fee_summary().unwrap()["amounts_wei"]["pending"], "5");
    }
    let mut substituted = refreshed;
    substituted.value_wei = "2000".into();
    assert!(store
        .apply_funding_recheck(&p.tx_hash, &Ok(PaymentState::Accepted(substituted)), 112)
        .is_err());
    assert_eq!(
        encode(&store.account(&q.payload.id).unwrap()).unwrap(),
        before
    );
}

#[test]
fn orphan_restoration_compensates_spending_and_every_fee_state_once_across_restart() {
    use crate::chain_client::{ChainError, PaymentState};
    for fee_state in ["pending", "batched", "remitted"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let (q, p, open) = paid(&store);
        let spent = store.reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "spent", 200, 300, 110, &store.account(&q.payload.id).unwrap().model)
            .unwrap();
        store
            .settle(
                &spent.id,
                &TokenUsage {
                    prompt_tokens: 200,
                    completion_tokens: 300,
                },
                "completed",
                111,
            )
            .unwrap();
        let held = store.reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "held", 100, 100, 112, &store.account(&q.payload.id).unwrap().model)
            .unwrap();
        match fee_state {
            "batched" => {
                store.prepare_fee_batches(112, true).unwrap();
            }
            "remitted" => {
                remitted_batch(&store, &q.payload, 112);
            }
            _ => {}
        }
        for now in [113, 114] {
            store
                .apply_funding_recheck(&p.tx_hash, &Ok(PaymentState::Orphaned), now)
                .unwrap();
        }
        let orphan = store.account(&q.payload.id).unwrap();
        assert_eq!(orphan.funded_wei, "1000");
        assert_eq!(orphan.spent_wei, "500");
        assert_eq!(orphan.reversed_wei, "1000");
        assert_eq!(orphan.available_wei, "0");
        assert_eq!(orphan.reserved_wei, "0");
        assert_eq!(
            orphan.fee_wei,
            if fee_state == "pending" { "0" } else { "5" }
        );
        assert_eq!(
            orphan.provider_loss_wei,
            if fee_state == "pending" { "500" } else { "505" }
        );
        assert_eq!(store.funding_to_recheck().unwrap(), vec![p.clone()]);
        // The fee view reports the book's state names (design decision 4); the
        // stored `remitted` status of an accepted batch's obligation is confirmed.
        let reported = if fee_state == "remitted" { "confirmed" } else { fee_state };
        let zeros = serde_json::json!({"pending":"0","batched":"0","submitted":"0","confirmed":"0"});
        let view = store.fee_summary().unwrap();
        // Orphaned funding's fee is never ordinary debt: an unsealed obligation
        // is cancelled, a sealed one is orphaned by its batch's state.
        assert_eq!(view["amounts_wei"], zeros);
        assert_eq!(view["cancelled_wei"], if fee_state == "pending" { "5" } else { "0" });
        let mut orphaned = serde_json::json!({"batched":"0","submitted":"0","confirmed":"0"});
        if fee_state != "pending" {
            orphaned[reported] = "5".into();
        }
        assert_eq!(view["orphaned_amounts_wei"], orphaned);
        for observation in [
            Ok(PaymentState::Pending),
            Err(ChainError::Pending("above frontier".into())),
            Err(ChainError::Unavailable("offline".into())),
        ] {
            store
                .apply_funding_recheck(&p.tx_hash, &observation, 115)
                .unwrap();
            assert_eq!(
                encode(&store.account(&q.payload.id).unwrap()).unwrap(),
                encode(&orphan).unwrap()
            );
        }
        drop(store);
        let store = BillingStore::open(&path, &network()).unwrap();
        let mut restored = p.clone();
        restored.block_hash = format!("0x{}", "77".repeat(32));
        restored.block_number = 12;
        for now in [116, 117] {
            store
                .apply_funding_recheck(
                    &p.tx_hash,
                    &Ok(PaymentState::Accepted(restored.clone())),
                    now,
                )
                .unwrap();
        }
        let account = store.account(&q.payload.id).unwrap();
        assert_eq!(account.status, "active");
        assert_eq!(account.funded_wei, "1000");
        assert_eq!(account.fee_wei, "5");
        assert_eq!(account.spent_wei, "500");
        assert_eq!(account.available_wei, "495");
        assert_eq!(account.reversed_wei, "0");
        assert_eq!(account.provider_loss_wei, "0");
        let view = store.fee_summary().unwrap();
        let mut ordinary = zeros.clone();
        ordinary[reported] = "5".into();
        assert_eq!(view["amounts_wei"], ordinary);
        assert_eq!(view["cancelled_wei"], "0");
        assert_eq!(
            view["orphaned_amounts_wei"],
            serde_json::json!({"batched":"0","submitted":"0","confirmed":"0"})
        );
        assert_eq!(store.funding_to_recheck().unwrap(), vec![restored]);
        assert_eq!(
            store
                .settle(
                    &held.id,
                    &TokenUsage {
                        prompt_tokens: 100,
                        completion_tokens: 100
                    },
                    "completed",
                    118
                )
                .unwrap()
                .cost_wei,
            "0"
        );
        let count = store.fee_batches().unwrap().len();
        let batches = store.prepare_fee_batches(119, true).unwrap();
        assert_eq!(store.fee_batches().unwrap().len(), count.max(1));
        if fee_state == "remitted" {
            assert!(batches.is_empty());
        }
    }
}

#[test]
fn restoring_one_funding_never_unfreezes_other_pending_or_orphaned_funding() {
    use crate::chain_client::PaymentState;
    for reverse in [false, true] {
        let store = memory();
        let (q, opening, open) = paid(&store);
        let topup = payment(&q.payload, false, 1000, 2);
        store.credit_topup(&q.payload.id, &topup, 102).unwrap();
        let reservation = store.reserve(&q.payload.id, &open.credential.as_ref().unwrap().id, "spent", 750, 750, 110, &store.account(&q.payload.id).unwrap().model)
            .unwrap();
        store
            .settle(
                &reservation.id,
                &TokenUsage {
                    prompt_tokens: 750,
                    completion_tokens: 750,
                },
                "completed",
                111,
            )
            .unwrap();
        let order = if reverse {
            [&topup, &opening]
        } else {
            [&opening, &topup]
        };
        for p in order {
            store
                .apply_funding_recheck(&p.tx_hash, &Ok(PaymentState::Orphaned), 112)
                .unwrap();
        }
        assert_eq!(
            store.account(&q.payload.id).unwrap().provider_loss_wei,
            "1500"
        );
        store
            .apply_funding_recheck(
                &order[0].tx_hash,
                &Ok(PaymentState::Accepted(order[0].clone())),
                113,
            )
            .unwrap();
        let partial = store.account(&q.payload.id).unwrap();
        assert_eq!(partial.status, "frozen");
        assert_eq!(partial.provider_loss_wei, "505");
        assert_eq!(partial.available_wei, "0");
        store
            .apply_funding_recheck(&order[1].tx_hash, &Ok(PaymentState::Pending), 114)
            .unwrap();
        assert_eq!(store.account(&q.payload.id).unwrap().status, "frozen");
        store
            .apply_funding_recheck(&order[0].tx_hash, &Ok(PaymentState::Pending), 115)
            .unwrap();
        store
            .apply_funding_recheck(
                &order[1].tx_hash,
                &Ok(PaymentState::Accepted(order[1].clone())),
                116,
            )
            .unwrap();
        let pending = store.account(&q.payload.id).unwrap();
        assert_eq!(pending.status, "settlement_pending");
        assert_eq!(pending.provider_loss_wei, "0");
        store
            .apply_funding_recheck(
                &order[0].tx_hash,
                &Ok(PaymentState::Accepted(order[0].clone())),
                117,
            )
            .unwrap();
        let account = store.account(&q.payload.id).unwrap();
        assert_eq!(account.status, "active");
        assert_eq!(account.funded_wei, "2000");
        assert_eq!(account.spent_wei, "1500");
        assert_eq!(account.fee_wei, "10");
        assert_eq!(account.available_wei, "490");
        assert_eq!(account.reversed_wei, "0");
        assert_eq!(store.fee_summary().unwrap()["amounts_wei"]["pending"], "10");
    }
}

#[test]
fn equivalent_intents_reuse_ack_and_conflicting_sender_nonce_survives_quota_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let q = quote(1000, false);
    let p = payment(&q.payload, true, 1000, 1);
    store.store_quote(&q, 100).unwrap();
    let ack = register(&store, &q, &p, 101);
    assert_eq!(
        encode(&register(&store, &q, &p, 102)).unwrap(),
        encode(&ack).unwrap()
    );
    assert_eq!(active_intents(&store, 102), 1);
    assert_eq!(
        store
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM quotes", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        0
    );
    let q2 = quote(1000, false);
    let p2 = payment(&q2.payload, true, 1000, 2);
    store.store_quote(&q2, 103).unwrap();
    assert!(store
        .register_funding_intent(
            &q2,
            &q2.payload.consumer_wallet,
            &p2.raw_transaction,
            &p2.tx_hash,
            1,
            PROVIDER_KEY,
            || Ok(104),
            true,
            &q2.payload.model
        )
        .unwrap_err()
        .starts_with("conflict:"));
    store
        .record_funding_attempt(&q.payload.id, &p.raw_transaction, "rejected", 105)
        .unwrap();
    assert_eq!(active_intents(&store, 105), 0);
    assert!(store
        .funding_intent(&q.payload, &p.raw_transaction)
        .unwrap()
        .is_some());
    store.transaction(|tx| cleanup_pending(tx, 400)).unwrap();
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    let mut late = quote(1000, false).payload;
    late.issued_at = 1000;
    late.expires_at = 1300;
    let late = protocol::sign(protocol::QUOTE_DOMAIN, late, PROVIDER_KEY).unwrap();
    store.store_quote(&late, 1000).unwrap();
    let next = payment(&late.payload, true, 1000, 3);
    assert!(store
        .register_funding_intent(
            &late,
            &late.payload.consumer_wallet,
            &next.raw_transaction,
            &next.tx_hash,
            1,
            PROVIDER_KEY,
            || Ok(1001),
            true,
            &late.payload.model
        )
        .unwrap_err()
        .starts_with("conflict:"));
    assert_eq!(
        encode(&store.quote(&q.payload.id).unwrap()).unwrap(),
        encode(&q).unwrap()
    );
    assert_eq!(
        encode(
            &store
                .funding_intent(&q.payload, &p.raw_transaction)
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        encode(&ack).unwrap()
    );
    assert_eq!(
        store
            .open_account(&q, Some(&p), 1002)
            .unwrap()
            .account
            .funded_wei,
        "1000"
    );
    assert_eq!(active_intents(&store, 1002), 0);
    assert!(store
        .register_funding_intent(
            &late,
            &late.payload.consumer_wallet,
            &next.raw_transaction,
            &next.tx_hash,
            1,
            PROVIDER_KEY,
            || Ok(1003),
            true,
            &late.payload.model
        )
        .unwrap_err()
        .starts_with("conflict:"));
}

#[test]
fn network_scoped_performance_cutover_refuses_old_ledgers_without_rewriting_money() {
    for schema in ["1", "2"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preserved-ledger.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let (q, _, opened) = paid(&store);
        let before = encode(&opened.account).unwrap();
        store.db.lock().unwrap().execute("UPDATE metadata SET value=?1 WHERE key='schema'", [schema]).unwrap();
        drop(store);
        let error = BillingStore::open(&path, &network()).err().unwrap();
        assert!(error.contains("preserve the old ledger"));
        let db = Connection::open(&path).unwrap();
        let after: String = db.query_row("SELECT account FROM accounts WHERE id=?1", [&q.payload.id], |row| row.get(0)).unwrap();
        let stored_schema: String = db.query_row("SELECT value FROM metadata WHERE key='schema'", [], |row| row.get(0)).unwrap();
        assert_eq!(after, before);
        assert_eq!(stored_schema, schema);
        let signed_quote: String = db.query_row("SELECT quote FROM accounts WHERE id=?1", [&q.payload.id], |row| row.get(0)).unwrap();
        assert_eq!(signed_quote, encode(&q).unwrap());
    }
}

async fn owner_post(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    raw: &str,
    ledger: &BillingStore,
) -> reqwest::Response {
    owner_post_at(client, base, path, raw, ledger, protocol::now().unwrap()).await
}

/// Issues the owner challenge at `now`, a provider admission sample that a test may
/// advance to its own simulated time; the proof still authenticates at wall-clock time.
async fn owner_post_at(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    raw: &str,
    ledger: &BillingStore,
    now: u64,
) -> reqwest::Response {
    let body = serde_json::to_vec(&json!({"raw_transaction":raw})).unwrap();
    let wallet = crate::wallet::address_from_private_key(OWNER_KEY).unwrap();
    let proof = AuthPayload {
        network: network(),
        provider_wallet: crate::wallet::address_from_private_key(PROVIDER_KEY).unwrap(),
        wallet: wallet.clone(),
        method: "POST".into(),
        path: path.into(),
        body_hash: protocol::hash_hex(&body),
        nonce: protocol::random_id(),
        expires_at: now + 120,
    };
    ledger.create_challenge(&proof, now).unwrap();
    let signature =
        crate::wallet::sign_message(OWNER_KEY, &crate::auth::challenge_payload(&proof).unwrap())
            .unwrap();
    client
        .post(format!("{base}{path}"))
        .header("x-chiral-owner", wallet)
        .header("x-chiral-nonce", proof.nonce)
        .header("x-chiral-signature", signature)
        .body(body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn control_acknowledges_only_spendable_current_nonce_and_recovers_without_rpc_or_broadcast() {
    use crate::{
        app::ProviderState,
        chain_client::{sign_transaction, ChainClient},
        contracts,
        network::NetworkConfig,
        offer_registry::OfferRegistry,
        provider_config::{DhtConfig, ProviderConfig},
    };
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let ledger = Arc::new(memory());
    let fixture = Arc::new(ContinuityRpc {
        outage: AtomicBool::new(false),
        orphan: AtomicBool::new(false),
        frontier: AtomicU64::new(10),
        nonce: AtomicU64::new(0),
        owner_balance: std::sync::Mutex::new(0),
        receipt: std::sync::Mutex::new(Value::Null),
        observed_at: protocol::now().unwrap(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let network_config = NetworkConfig {
        identity: network(),
        rpc_url: format!("http://{}", listener.local_addr().unwrap()),
        rpc_bearer_file: None,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    };
    let admission_pause = Arc::new((
        AtomicBool::new(false), tokio::sync::Notify::new(), tokio::sync::Notify::new(),
    ));
    let rpc_pause = admission_pause.clone();
    let rpc = axum::Router::new()
        .route("/", axum::routing::post(move |state, body| {
            let pause = rpc_pause.clone();
            async move {
                let response = continuity_rpc(state, body).await;
                if pause.0.swap(false, Ordering::AcqRel) {
                    pause.1.notify_one();
                    pause.2.notified().await;
                }
                response
            }
        }))
        .with_state(fixture.clone());
    let rpc_task = tokio::spawn(async move {
        axum::serve(listener, rpc).await.unwrap();
    });
    let chain = Arc::new(ChainClient::new(network_config.clone(), dir.path().into()).unwrap());
    let now = protocol::now().unwrap();
    let mut payload = quote(1000, false).payload;
    payload.issued_at = now;
    payload.expires_at = now + 300;
    let q = protocol::sign(protocol::QUOTE_DOMAIN, payload, PROVIDER_KEY).unwrap();
    ledger.store_quote(&q, now).unwrap();
    let config = ProviderConfig {
        network: network_config,
        data_dir: dir.path().into(),
        signing_key_file: dir.path().join("key"),
        admin_token_file: dir.path().join("admin"),
        public_endpoint: "http://127.0.0.1:9419".into(),
        public_bind: "127.0.0.1:9419".parse().unwrap(),
        admin_bind: "127.0.0.1:9421".parse().unwrap(),
        platform_wallet: q.payload.platform_wallet.clone(),
        fee_bps: 50,
        min_funding_wei: "1000".into(),
        models: vec![crate::provider_config::ModelConfig {
            model: q.payload.model.clone(), price: q.payload.price.clone(),
            engine_url: "http://127.0.0.1:1".into(), engine_api_key_file: None,
            max_concurrency: q.payload.max_concurrency, request_timeout_secs: 1,
            qualification_evidence_file: None,
            runtime: None,
        }],
        dht: DhtConfig {
            listen: "/ip4/127.0.0.1/tcp/0".into(),
            bootstrap: vec![],
            identity_file: dir.path().join("dht"),
        },
        development: true,
    };
    let state = Arc::new(ProviderState {
        model_slots: ProviderState::slots(&config),
        managed_engines: Default::default(),
        config: Arc::new(config),
        chain,
        ledger: ledger.clone(),
        private_key: Arc::new(PROVIDER_KEY.into()),
        wallet: q.payload.provider_wallet.clone(),
        offers: Arc::new(OfferRegistry::new(network(), true)),
        accepting: AtomicBool::new(true),
        settlement_fault: Default::default(),
        funding_ready: AtomicBool::new(true),
        qualified_models: std::sync::RwLock::new(Default::default()),
        embedding_dimensions: std::sync::RwLock::new(Default::default()),
    });
    state.qualified_models.write().unwrap().insert(q.payload.model.id.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = contracts::router(state.clone());
    let gateway_task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::new();
    let path = format!("/v1/contracts/{}/funding-intent", q.payload.id);
    let data = protocol::opening_data(&q.payload).unwrap();
    let signed = sign_transaction(
        OWNER_KEY,
        network().chain_id,
        0,
        &q.payload.provider_wallet,
        1000,
        &data,
        1,
        native_fixture_gas(&data),
    )
    .unwrap();
    for balance in [0, 1000] {
        *fixture.owner_balance.lock().unwrap() = balance;
        assert_eq!(
            owner_post(&client, &base, &path, &signed.raw_transaction, &ledger)
                .await
                .status(),
            reqwest::StatusCode::BAD_REQUEST
        );
        assert!(ledger
            .funding_intent(&q.payload, &signed.raw_transaction)
            .unwrap()
            .is_none());
        assert_eq!(active_intents(&ledger, now), 0);
    }
    *fixture.owner_balance.lock().unwrap() = u128::MAX;
    let future = sign_transaction(
        OWNER_KEY,
        network().chain_id,
        1,
        &q.payload.provider_wallet,
        1000,
        &data,
        1,
        native_fixture_gas(&data),
    )
    .unwrap();
    assert_eq!(
        owner_post(&client, &base, &path, &future.raw_transaction, &ledger)
            .await
            .status(),
        reqwest::StatusCode::BAD_REQUEST
    );
    // A stale quote cannot obtain a NEW acknowledgement while its exact model
    // is unqualified, even when the provider-wide gates remain open.
    state.qualified_models.write().unwrap().clear();
    let response = owner_post(&client, &base, &path, &signed.raw_transaction, &ledger).await;
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.json::<Value>().await.unwrap()["error"]["code"], "model_unqualified");
    assert!(ledger.funding_intent(&q.payload, &signed.raw_transaction).unwrap().is_none());
    state.qualified_models.write().unwrap().insert(q.payload.model.id.clone());
    // Lose qualification while the async spendability check is in flight.
    // Synchronization is at the RPC boundary, not a timing-dependent sleep.
    admission_pause.0.store(true, Ordering::Release);
    let (response, ()) = tokio::join!(
        owner_post(&client, &base, &path, &signed.raw_transaction, &ledger),
        async {
            admission_pause.1.notified().await;
            state.qualified_models.write().unwrap().clear();
            admission_pause.2.notify_one();
        }
    );
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.json::<Value>().await.unwrap()["error"]["code"], "model_unqualified");
    assert!(ledger.funding_intent(&q.payload, &signed.raw_transaction).unwrap().is_none());
    state.qualified_models.write().unwrap().insert(q.payload.model.id.clone());
    let mut mismatched = q.payload.clone();
    mismatched.id = protocol::random_id();
    mismatched.model.revision = "different-pinned-revision".into();
    let mismatched = protocol::sign(protocol::QUOTE_DOMAIN, mismatched, PROVIDER_KEY).unwrap();
    ledger.store_quote(&mismatched, now).unwrap();
    let response = owner_post(
        &client, &base, &format!("/v1/contracts/{}/funding-intent", mismatched.payload.id),
        &signed.raw_transaction, &ledger,
    ).await;
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.json::<Value>().await.unwrap()["error"]["code"], "model_unqualified");
    let response = owner_post(&client, &base, &path, &signed.raw_transaction, &ledger).await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let ack: Signed<FundingIntent> = response.json().await.unwrap();
    assert_eq!(
        ledger.admin_summary().unwrap()["pending_funding_intents"],
        1
    );
    assert_eq!(active_intents(&ledger, now), 1);
    let mut second = q.payload.clone();
    second.id = protocol::random_id();
    second.nonce = protocol::random_id();
    let second = protocol::sign(protocol::QUOTE_DOMAIN, second, PROVIDER_KEY).unwrap();
    ledger.store_quote(&second, now).unwrap();
    let conflict = sign_transaction(
        OWNER_KEY,
        network().chain_id,
        0,
        &second.payload.provider_wallet,
        1000,
        &protocol::opening_data(&second.payload).unwrap(),
        1,
        native_fixture_gas(&protocol::opening_data(&second.payload).unwrap()),
    )
    .unwrap();
    assert_eq!(
        owner_post(
            &client,
            &base,
            &format!("/v1/contracts/{}/funding-intent", second.payload.id),
            &conflict.raw_transaction,
            &ledger
        )
        .await
        .status(),
        reqwest::StatusCode::CONFLICT
    );
    assert!(ledger
        .funding_intent(&second.payload, &conflict.raw_transaction)
        .unwrap()
        .is_none());
    let repriced = sign_transaction(
        OWNER_KEY,
        network().chain_id,
        0,
        &q.payload.provider_wallet,
        1000,
        &data,
        2,
        native_fixture_gas(&data),
    )
    .unwrap();
    assert_eq!(
        owner_post(&client, &base, &path, &repriced.raw_transaction, &ledger)
            .await
            .status(),
        reqwest::StatusCode::CONFLICT
    );
    // Recovery is a local durable read even if the original nonce is spent,
    // admission has expired, the provider is draining, and RPC is unavailable.
    ledger
        .transaction(|tx| cleanup_pending(tx, now + 301))
        .unwrap();
    // That maintenance cannot reset challenge rates, so the next owner proofs are
    // issued at the same simulated time and enter its newer challenge minute.
    let later = now + 301;
    fixture.nonce.store(1, Ordering::Release);
    *fixture.owner_balance.lock().unwrap() = 0;
    fixture.outage.store(true, Ordering::Release);
    state.accepting.store(false, Ordering::Release);
    state.qualified_models.write().unwrap().clear();
    let response = owner_post_at(&client, &base, &path, &signed.raw_transaction, &ledger, later).await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        encode(&response.json::<Signed<FundingIntent>>().await.unwrap()).unwrap(),
        encode(&ack).unwrap()
    );
    assert_eq!(active_intents(&ledger, now + 301), 0);
    assert_eq!(
        state
            .chain
            .history(&q.payload.consumer_wallet)
            .await
            .unwrap()["transactions"],
        json!([])
    );
    // Existing agreements remain settleable after admission stops. Engine readiness
    // cannot reject an already-sent top-up or make its recovery credit it twice.
    fixture.outage.store(false, Ordering::Release);
    state.funding_ready.store(false, Ordering::Release);
    let observe = |hash: &str, gas: u64| {
        *fixture.receipt.lock().unwrap() = json!({
            "transactionHash":hash,"from":q.payload.consumer_wallet,
            "to":q.payload.provider_wallet,"type":"0x0","gasUsed":format!("0x{gas:x}"),
            "status":"0x1","blockHash":format!("0x{}","44".repeat(32)),
            "blockNumber":"0x9"
        });
    };
    observe(&signed.tx_hash, native_fixture_gas(&data));
    let opening = state
        .chain
        .verify_payment(
            &signed.raw_transaction,
            &q.payload.consumer_wallet,
            &q.payload.provider_wallet,
            Some("1000"),
            &data,
        )
        .await
        .unwrap();
    ledger.open_account(&q, Some(&opening), now).unwrap();
    let topup = sign_transaction(
        OWNER_KEY,
        network().chain_id,
        1,
        &q.payload.provider_wallet,
        1000,
        &protocol::topup_data(&q.payload.id).unwrap(),
        1,
        native_fixture_gas(&protocol::topup_data(&q.payload.id).unwrap()),
    )
    .unwrap();
    let topup_path = format!("/v1/contracts/{}/topup", q.payload.id);
    *fixture.receipt.lock().unwrap() = Value::Null;
    assert_eq!(
        owner_post_at(&client, &base, &topup_path, &topup.raw_transaction, &ledger, later)
            .await
            .status(),
        reqwest::StatusCode::CONFLICT
    );
    assert_eq!(ledger.account(&q.payload.id).unwrap().funded_wei, "1000");
    observe(&topup.tx_hash, native_fixture_gas(&protocol::topup_data(&q.payload.id).unwrap()));
    for _ in 0..2 {
        let response =
            owner_post_at(&client, &base, &topup_path, &topup.raw_transaction, &ledger, later).await;
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let account: AccountView = response.json().await.unwrap();
        assert_eq!(account.funded_wei, "2000");
        assert_eq!(account.fee_wei, "10");
        assert_eq!(account.available_wei, "1990");
        assert_eq!(account.reserved_wei, "0");
    }
    // Cleanup removed the unacknowledged quote; restore a currently valid offer
    // so this checks the admission gate, not a missing-record rejection. This
    // wall-clock proof is an older sample spending the later minute's units.
    ledger.store_quote(&second, now).unwrap();
    assert_eq!(
        owner_post(
            &client,
            &base,
            &format!("/v1/contracts/{}/funding-intent", second.payload.id),
            &conflict.raw_transaction,
            &ledger
        )
        .await
        .status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    );
    // The RPC fixture deliberately has no eth_sendRawTransaction handler.
    gateway_task.abort();
    rpc_task.abort();
}

#[test]
fn concurrent_intent_admission_cannot_acknowledge_two_agreements_for_one_nonce() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for tag in [1, 2] {
        let q = quote(1000, false);
        let p = payment(&q.payload, true, 1000, tag);
        store.store_quote(&q, 100).unwrap();
        let ledger = BillingStore::open(&path, &network()).unwrap();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            ledger.register_funding_intent(&q, &q.payload.consumer_wallet, &p.raw_transaction, &p.tx_hash, 7, PROVIDER_KEY, || Ok(101), true, &(&q).payload.model)
        }));
    }
    let outcomes = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(outcomes.iter().any(|result| result
        .as_ref()
        .err()
        .is_some_and(|error| error.starts_with("conflict:"))));
    assert_eq!(store.admin_summary().unwrap()["pending_funding_intents"], 1);
    assert_eq!(active_intents(&store, 101), 1);
}

#[test]
fn performance_waiver_preserves_usage_and_releases_reservation_exactly_once() {
    let store = memory();
    let mut terms = quote(1000, false).payload;
    terms.model.context_length = 4096;
    terms.model.serving = Some(crate::performance::test_profile());
    let q = protocol::sign(protocol::QUOTE_DOMAIN, terms, PROVIDER_KEY).unwrap();
    store.store_quote(&q, 100).unwrap();
    let funding = payment(&q.payload, true, 1000, 91);
    register(&store, &q, &funding, 100);
    let opened = store.open_account(&q, Some(&funding), 101).unwrap();
    let reservation = store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "slow-request", 100, 256, 102, &store.account(&q.payload.id).unwrap().model).unwrap();
    let usage = TokenUsage { prompt_tokens: 100, completion_tokens: 256 };
    let outcome = crate::performance::outcome(q.payload.model.serving.as_ref().unwrap(), true, Some(2_000_001), Some(1_000_000), Some(&usage), "completed", false, Some(3_000_001), &(q.payload.model.serving.as_ref().unwrap()).qualification.evidence_sha256);
    for now in [103, 104] {
        let settled = store.settle_measured(&reservation.id, &usage, "completed", Some(&outcome), now).unwrap();
        assert_eq!(settled.cost_wei, "0");
        assert_eq!(settled.waived_wei, "356");
        assert_eq!(settled.usage.completion_tokens, 256);
        assert_eq!(settled.settled_at, Some(103));
    }
    assert!(store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "slow-request", 100, 256, 105, &q.payload.model).is_err());
    let account = store.account(&q.payload.id).unwrap();
    assert_eq!(account.spent_wei, "0");
    assert_eq!(account.reserved_wei, "0");
    assert_eq!(account.available_wei, "995");
    assert_eq!(account.fee_wei, "5");
    assert_eq!(account.waived_wei, "356");
    assert_eq!(account.usage.prompt_tokens, 100);
    assert_eq!(account.usage.completion_tokens, 256);
    let receipt = store.receipt(&q.payload.id, PROVIDER_KEY, 106).unwrap();
    protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &q.payload.provider_wallet).unwrap();
    assert_eq!(receipt.payload.requests[0].performance.as_ref().unwrap().remedy, "inference_waiver");
    let credential = store.credentials(&q.payload.id).unwrap().into_iter().find(|c| c.id == opened.credential.as_ref().unwrap().id).unwrap();
    assert_eq!(credential.reserved_wei, "0");
    assert_eq!(credential.spent_wei, "0");
}

#[test]
fn restart_performance_waiver_does_not_invent_usage_or_credit() {
    let store = memory();
    let mut terms = quote(1000, false).payload;
    terms.model.context_length = 4096;
    terms.model.serving = Some(crate::performance::test_profile());
    let q = protocol::sign(protocol::QUOTE_DOMAIN, terms, PROVIDER_KEY).unwrap();
    store.store_quote(&q, 100).unwrap();
    let funding = payment(&q.payload, true, 1000, 92);
    register(&store, &q, &funding, 100);
    let opened = store.open_account(&q, Some(&funding), 101).unwrap();
    store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "crash-request", 100, 256, 102, &store.account(&q.payload.id).unwrap().model).unwrap();
    assert_eq!(store.recover_requests(103).unwrap(), 1);
    assert_eq!(store.recover_requests(104).unwrap(), 0);
    let request = store.request_view(&q.payload.id, "crash-request").unwrap();
    let measured = request.performance.unwrap();
    assert_eq!(measured.output_tokens, None);
    assert_eq!(measured.remedy, "inference_waiver");
    assert_eq!(request.waived_wei, "0");
    let account = store.account(&q.payload.id).unwrap();
    assert_eq!(account.available_wei, "995");
    assert_eq!(account.waived_wei, "0");
    assert_eq!(account.usage.completion_tokens, 0);
}

#[test]
fn measured_profile_withdraws_below_ninety_five_percent_without_changing_agreement() {
    let store = memory();
    let mut terms = quote(1000, false).payload;
    terms.model.context_length = 4096;
    terms.model.serving = Some(crate::performance::test_profile());
    let q = protocol::sign(protocol::QUOTE_DOMAIN, terms, PROVIDER_KEY).unwrap();
    store.store_quote(&q, 100).unwrap();
    let funding = payment(&q.payload, true, 1000, 93);
    register(&store, &q, &funding, 100);
    let opened = store.open_account(&q, Some(&funding), 101).unwrap();
    let usage = TokenUsage { prompt_tokens: 1, completion_tokens: 2 };
    for index in 0..21u64 {
        let now = 102 + index;
        let reservation = store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, &format!("window-{index}"), 1, 256, now, &store.account(&q.payload.id).unwrap().model).unwrap();
        let ttft = if index >= 19 { 2_000_001 } else { 1_000_000 };
        let outcome = crate::performance::outcome(q.payload.model.serving.as_ref().unwrap(), true, Some(ttft), Some(50_000), Some(&usage), "completed", false, Some(ttft + 50_000), &(q.payload.model.serving.as_ref().unwrap()).qualification.evidence_sha256);
        store.settle_measured(&reservation.id, &usage, "completed", Some(&outcome), now).unwrap();
        assert_eq!(store.performance_withdrawn(&q.payload.model).unwrap(), index == 20);
    }
    let account = store.account(&q.payload.id).unwrap();
    assert_eq!(account.model, q.payload.model);
    assert_eq!(account.spent_wei, "57");
    assert_eq!(account.waived_wei, "6");
    assert_eq!(account.reserved_wei, "0");
    assert_eq!(account.available_wei, "938");
}

fn performance_account(store: &BillingStore, model: ModelSpec, tag: u8) -> (Signed<Quote>, crate::protocol::OpenResult) {
    let mut terms = quote(100_000, false).payload;
    terms.model = model;
    terms.max_concurrency = 32;
    let q = protocol::sign(protocol::QUOTE_DOMAIN, terms, PROVIDER_KEY).unwrap();
    store.store_quote(&q, 100).unwrap();
    let funding = payment(&q.payload, true, 100_000, tag);
    register(store, &q, &funding, 100);
    let opened = store.open_account(&q, Some(&funding), 101).unwrap();
    (q, opened)
}

fn settle_performance(store: &BillingStore, reservation: &Reservation, missed: bool, short: bool) -> RequestView {
    let usage = TokenUsage { prompt_tokens: 1, completion_tokens: if short { 1 } else { 256 } };
    let ttft = if missed { 2_000_001 } else { 1_000_000 };
    let outcome = crate::performance::outcome(reservation.model.serving.as_ref().unwrap(), true, Some(ttft), Some(1_000_000), Some(&usage), "completed", false, Some(ttft + 1_000_000), &(reservation.model.serving.as_ref().unwrap()).qualification.evidence_sha256);
    // All settlements deliberately share a timestamp: SQL observation sequence
    // must distinguish them without relying on clock granularity or creation.
    store.settle_measured(&reservation.id, &usage, "completed", Some(&outcome), 200).unwrap()
}

#[test]
fn withdrawal_survives_outstanding_burst_late_completion_and_restart_per_model_evidence() {
    for misses_first in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let mut model = quote(0, true).payload.model;
        model.serving = Some(crate::performance::test_profile());
        model.serving.as_mut().unwrap().max_concurrency = 32;
        let (q, opened) = performance_account(&store, model.clone(), 94);
        let mut stale = q.payload.clone();
        stale.id = protocol::random_id();
        stale.nonce = protocol::random_id();
        let stale = protocol::sign(protocol::QUOTE_DOMAIN, stale, PROVIDER_KEY).unwrap();
        store.store_quote(&stale, 100).unwrap();
        let requests: Vec<_> = (0..32).map(|index| store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, &format!("burst-{index}"), 1, 256, 102 + index, &store.account(&q.payload.id).unwrap().model).unwrap()).collect();
        let order: Vec<usize> = if misses_first { (0..32).collect() } else { (3..32).chain(0..3).collect() };
        for index in order {
            let settled = settle_performance(&store, &requests[index], index < 3, false);
            assert_eq!(settled.cost_wei, if index < 3 { "0" } else { "257" });
            assert_eq!(settled.waived_wei, if index < 3 { "257" } else { "0" });
            if misses_first && index == 0 {
                for _ in 0..3 {
                    settle_performance(&store, &requests[index], true, false);
                }
                assert!(!store.performance_withdrawn(&model).unwrap());
            }
            if index == 2 {
                assert!(store.performance_withdrawn(&model).unwrap());
            }
        }
        assert!(store.performance_withdrawn(&model).unwrap());
        let account = store.account(&q.payload.id).unwrap();
        assert_eq!(account.reserved_wei, "0");
        assert_eq!(account.spent_wei, (29 * 257).to_string());
        assert_eq!(account.waived_wei, (3 * 257).to_string());
        assert_eq!(account.fee_wei, "500");
        drop(store);
        let store = BillingStore::open(&path, &network()).unwrap();
        assert!(store.performance_withdrawn(&model).unwrap());
        assert_eq!(store.account(&q.payload.id).unwrap().model, model);
        let stale_payment = payment(&stale.payload, true, 100_000, 98);
        assert!(store.register_funding_intent(
            &stale, &stale.payload.consumer_wallet, &stale_payment.raw_transaction,
            &stale_payment.tx_hash, 98, PROVIDER_KEY, || Ok(201), true,
            &stale.payload.model,
        ).unwrap_err().starts_with("model_unqualified:"));
        assert!(store.funding_intent(&stale.payload, &stale_payment.raw_transaction).unwrap().is_none());
        // Recovery and credited agreement settlement do not depend on the
        // model's current admission eligibility.
        let original_payment = payment(&q.payload, true, 100_000, 94);
        let recovered = register(&store, &q, &original_payment, 201);
        assert_eq!(recovered.payload.quote_id, q.payload.id);
        let reopened = store.open_account(&q, Some(&original_payment), 201).unwrap();
        assert_eq!(reopened.account.funded_wei, "100000");
        assert_eq!(reopened.account.spent_wei, (29 * 257).to_string());
        assert_eq!(reopened.account.fee_wei, "500");
        // A different model with identical evidence and new evidence for the
        // same model each begin their own observation window.
        let mut other_model = model.clone();
        other_model.id = "other/model".into();
        let mut fresh_model = model.clone();
        fresh_model.serving.as_mut().unwrap().qualification.evidence_sha256 = "d".repeat(64);
        for (isolated, tag) in [(other_model, 95), (fresh_model, 96)] {
            assert!(!store.performance_withdrawn(&isolated).unwrap());
            let (q, opened) = performance_account(&store, isolated.clone(), tag);
            let request = store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "independent", 1, 256, 102, &store.account(&q.payload.id).unwrap().model).unwrap();
            settle_performance(&store, &request, true, false);
            assert!(!store.performance_withdrawn(&isolated).unwrap());
            assert!(store.performance_withdrawn(&model).unwrap());
        }
    }
}

#[test]
fn ttft_only_successes_neither_dilute_nor_fill_joint_withdrawal_window() {
    let store = memory();
    let mut model = quote(0, true).payload.model;
    model.serving = Some(crate::performance::test_profile());
    let (q, opened) = performance_account(&store, model.clone(), 97);
    for index in 0..35 {
        let missed = index < 2 || index == 34;
        let request = store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, &format!("short-{index}"), 1, 256, 102, &store.account(&q.payload.id).unwrap().model).unwrap();
        settle_performance(&store, &request, missed, !missed);
        assert_eq!(store.performance_withdrawn(&model).unwrap(), index == 34);
    }
}

#[test]
fn evidence_renewal_preserves_existing_credit_and_request_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("renewal.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let mut model = quote(0, true).payload.model;
    model.serving = Some(crate::performance::test_profile());
    let (q, opened) = performance_account(&store, model.clone(), 99);
    for index in 0..3 {
        let request = store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id,
            &format!("old-miss-{index}"), 1, 256, 102, &model).unwrap();
        settle_performance(&store, &request, true, false);
    }
    assert!(store.performance_withdrawn(&model).unwrap());
    assert!(store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "withdrawn", 1, 256, 201, &model)
        .unwrap_err().starts_with("model_unqualified:"));
    let mut current = model.clone();
    let qualification = &mut current.serving.as_mut().unwrap().qualification;
    qualification.evidence_sha256 = "e".repeat(64);
    qualification.measured_at = 201;
    let request = store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "renewed", 1, 256, 202, &current).unwrap();
    let pending = store.request_view(&q.payload.id, "renewed").unwrap().performance.unwrap();
    assert_eq!(pending.evidence_sha256, "e".repeat(64));
    assert_eq!(pending.eligibility, "pending");
    let usage = TokenUsage { prompt_tokens: 1, completion_tokens: 256 };
    let measured = crate::performance::outcome(current.serving.as_ref().unwrap(), true, Some(1_000_000), Some(1_000_000), Some(&usage), "completed", false, Some(2_000_000), &(current.serving.as_ref().unwrap()).qualification.evidence_sha256);
    let mut forged = measured.clone();
    forged.evidence_sha256 = "f".repeat(64);
    assert!(store.settle_measured(&request.id, &usage, "completed", Some(&forged), 203).is_err());
    let settled = store.settle_measured(&request.id, &usage, "completed", Some(&measured), 203).unwrap();
    assert_eq!(settled.performance.unwrap().evidence_sha256, "e".repeat(64));
    assert_eq!(settled.cost_wei, "257");
    let account = store.account(&q.payload.id).unwrap();
    assert_eq!(account.model, model);
    assert_eq!(account.price, q.payload.price);
    assert_eq!(account.fee_wei, "500");
    assert_eq!(account.available_wei, "99243");
    let mut stale = q.payload.clone();
    stale.id = protocol::random_id();
    stale.nonce = protocol::random_id();
    let stale = protocol::sign(protocol::QUOTE_DOMAIN, stale, PROVIDER_KEY).unwrap();
    store.store_quote(&stale, 203).unwrap();
    let funding = payment(&stale.payload, true, 100_000, 100);
    store.register_funding_intent(&stale, &stale.payload.consumer_wallet,
        &funding.raw_transaction, &funding.tx_hash, 100, PROVIDER_KEY, || Ok(203), true, &current).unwrap();
    assert_eq!(store.quote(&stale.payload.id).unwrap().payload.model, model);
    let mut changed = current.clone();
    changed.serving.as_mut().unwrap().output_tokens_per_second = 50;
    assert!(store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "changed-class", 1, 256, 204, &changed)
        .unwrap_err().starts_with("model_revision_unavailable:"));
    store.reserve(&q.payload.id, &opened.credential.as_ref().unwrap().id, "crashed-renewal", 1, 256, 204, &current).unwrap();
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    assert_eq!(store.recover_requests(205).unwrap(), 1);
    let recovered = store.request_view(&q.payload.id, "crashed-renewal").unwrap();
    assert_eq!(recovered.performance.unwrap().evidence_sha256, "e".repeat(64));
    assert_eq!(recovered.cost_wei, "0");
    assert_eq!(recovered.status, "restart_unknown_usage");
    let receipt = store.receipt(&q.payload.id, PROVIDER_KEY, 206).unwrap();
    protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &q.payload.provider_wallet).unwrap();
    assert_eq!(receipt.payload.account.model, model);
    assert_eq!(receipt.payload.account.available_wei, "99243");
    assert_eq!(receipt.payload.account.reserved_wei, "0");
    assert!(store.performance_withdrawn(&model).unwrap());
    assert!(!store.performance_withdrawn(&current).unwrap());
}

fn quote_rate_snapshot(store: &BillingStore) -> [Vec<String>; 6] {
    let db = store.db.lock().unwrap();
    [
        "SELECT json_array(key,value) FROM metadata ORDER BY key",
        "SELECT json_array(bucket,window,count) FROM rate_limits ORDER BY bucket",
        "SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
        "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
    ].map(|query| db.prepare(query).unwrap().query_map([], |row| row.get(0)).unwrap()
        .collect::<Result<Vec<_>, _>>().unwrap())
}

fn committed_quote_rate(store: &BillingStore) -> (Option<String>, u64, u64, u64) {
    let db = store.db.lock().unwrap();
    let watermark = db.query_row(
        "SELECT value FROM metadata WHERE key='quote_committed_minute'", [], |row| row.get(0),
    ).optional().unwrap();
    let (window, count) = db.query_row(
        "SELECT window,count FROM rate_limits WHERE bucket='quote:global'",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    let legacy = db.query_row(
        "SELECT COUNT(*) FROM rate_limits WHERE bucket GLOB 'quote:*' AND bucket<>'quote:global'",
        [], |row| row.get(0),
    ).unwrap();
    (watermark, window, count, legacy)
}

#[test]
fn quote_rate_adoption_preserves_totals_and_retires_wallet_rows_only_on_commit() {
    for count in [599, 600] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("quote-adoption.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let (agreement, payment, opened) = paid(&store);
        let pending = quote(0, true);
        store.store_quote(&pending, 110).unwrap();
        // Retained old-limiter evidence, not claimed historical public issuances.
        {
            let db = store.db.lock().unwrap();
            db.execute("DELETE FROM metadata WHERE key='quote_committed_minute'", []).unwrap();
            db.execute("UPDATE rate_limits SET window=3,count=?1 WHERE bucket='quote:global'", [count]).unwrap();
            db.execute("INSERT INTO rate_limits VALUES(?1,3,12),(?2,2,12)",
                params![format!("quote:{}", agreement.payload.consumer_wallet), "quote:older-wallet"]).unwrap();
        }
        let before = quote_rate_snapshot(&store);
        // A later minute tentatively adopts/retires evidence, but the duplicate
        // insertion fails. Even legacy row deletion must roll back.
        assert!(store.store_quote(&pending, 240).is_err());
        assert_eq!(quote_rate_snapshot(&store), before);
        let candidate = quote(1000, false);
        if count == 599 {
            store.store_quote(&candidate, 120).unwrap();
            assert_eq!(committed_quote_rate(&store), (Some("3".into()), 3, 600, 0));
            assert_eq!(store.quote(&candidate.payload.id).unwrap().payload, candidate.payload);
        } else {
            assert!(store.store_quote(&candidate, 120).unwrap_err().starts_with("rate_limit:"));
            assert_eq!(quote_rate_snapshot(&store), before);
            assert_eq!(committed_quote_rate(&store), (None, 3, 600, 2));
        }
        drop(store);
        let store = BillingStore::open(&path, &network()).unwrap();
        let retained = committed_quote_rate(&store);
        let challenge = crate::auth::AuthPayload {
            network: network(), provider_wallet: agreement.payload.provider_wallet.clone(),
            wallet: agreement.payload.consumer_wallet.clone(), method: "GET".into(),
            path: "/v1/models".into(), body_hash: protocol::hash_hex(b""),
            nonce: format!("0x{:064x}", 77), expires_at: 360,
        };
        store.create_challenge(&challenge, 240).unwrap();
        assert_eq!(committed_quote_rate(&store), retained,
            "later challenge cleanup cannot delete or advance committed quote evidence");
        for sample in [120, 180, 239] {
            let refused = quote(1000, false);
            let before = quote_rate_snapshot(&store);
            assert!(store.store_quote(&refused, sample).unwrap_err().starts_with("rate_limit:"));
            assert_eq!(quote_rate_snapshot(&store), before);
            assert!(store.quote(&refused.payload.id).unwrap_err().starts_with("not_found:"));
        }
        let fresh = quote(1000, false);
        store.store_quote(&fresh, 240).unwrap();
        assert_eq!(committed_quote_rate(&store), (Some("4".into()), 4, 1, 0));
        assert_eq!(store.quote(&fresh.payload.id).unwrap().payload, fresh.payload);
        assert_eq!(store.quote(&pending.payload.id).unwrap().payload, pending.payload);
        assert_eq!(encode(&store.account(&agreement.payload.id).unwrap()).unwrap(),
            encode(&opened.account).unwrap());
        assert_eq!(store.authenticate(&opened.credential.as_ref().unwrap().bearer, 240).unwrap().credential_id, opened.credential.as_ref().unwrap().id);
        assert_eq!(store.funding_intent(&agreement.payload, &payment.raw_transaction).unwrap().unwrap().payload.quote_id,
            agreement.payload.id);
    }
}

#[test]
fn quote_rate_reordered_writers_share_the_retained_last_unit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("quote-clock.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let first = quote(1000, false);
    store.store_quote(&first, 180).unwrap();
    let delayed = quote(1000, false);
    store.store_quote(&delayed, 120).unwrap();
    assert_eq!(committed_quote_rate(&store), (Some("3".into()), 3, 2, 0));
    // Isolate the last-unit boundary without claiming 599 HTTP signing attempts.
    store.db.lock().unwrap().execute(
        "UPDATE rate_limits SET count=599 WHERE bucket='quote:global'", [],
    ).unwrap();
    let before = quote_rate_snapshot(&store);
    assert!(store.store_quote(&first, 240).is_err());
    assert_eq!(quote_rate_snapshot(&store), before, "failed future insertion must not advance the clock");
    let candidates = [quote(1000, false), quote(1000, false)];
    let writers = [120, 180].map(|sample|
        (BillingStore::open(&path, &network()).unwrap(), sample));
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let [(left, left_sample), (right, right_sample)] = writers;
        let handles = [(left, left_sample, &candidates[0]), (right, right_sample, &candidates[1])]
            .map(|(ledger, sample, candidate)| {
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    ledger.store_quote(candidate, sample)
                })
            });
        handles.map(|handle| handle.join().unwrap())
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let reopened = BillingStore::open(&path, &network()).unwrap();
    assert_eq!(committed_quote_rate(&reopened), (Some("3".into()), 3, 600, 0));
    for (candidate, result) in candidates.iter().zip(results) {
        if result.is_ok() {
            assert_eq!(reopened.quote(&candidate.payload.id).unwrap().payload, candidate.payload);
        } else {
            assert!(result.unwrap_err().starts_with("rate_limit:"));
            assert!(reopened.quote(&candidate.payload.id).unwrap_err().starts_with("not_found:"));
        }
    }
    for existing in [&first, &delayed] {
        assert_eq!(reopened.quote(&existing.payload.id).unwrap().payload, existing.payload);
    }
    for sample in [120, 180] {
        let before = quote_rate_snapshot(&reopened);
        assert!(reopened.store_quote(&quote(1000, false), sample).unwrap_err().starts_with("rate_limit:"));
        assert_eq!(quote_rate_snapshot(&reopened), before);
    }
}

#[test]
fn inconsistent_quote_rate_evidence_refuses_without_erasing_recoverable_accounts() {
    for mutation in [
        "DELETE FROM rate_limits WHERE bucket='quote:global'",
        "DELETE FROM metadata WHERE key='quote_committed_minute'; DELETE FROM rate_limits WHERE bucket='quote:global'; INSERT INTO rate_limits VALUES('quote:legacy',1,1)",
        "DELETE FROM metadata WHERE key='quote_committed_minute'; INSERT INTO rate_limits VALUES('quote:legacy',2,1)",
        "DELETE FROM metadata WHERE key='quote_committed_minute'; INSERT INTO rate_limits VALUES('quote:legacy',1,2)",
        "DELETE FROM metadata WHERE key='quote_committed_minute'; INSERT INTO rate_limits VALUES('quote:legacy',0,13)",
        "DELETE FROM metadata WHERE key='quote_committed_minute'; INSERT INTO rate_limits VALUES('quote:legacy',0,'invalid')",
        "INSERT INTO rate_limits VALUES('quote:legacy',1,1)",
        "UPDATE metadata SET value='invalid' WHERE key='quote_committed_minute'",
        "UPDATE metadata SET value='01' WHERE key='quote_committed_minute'",
        "UPDATE metadata SET value='2' WHERE key='quote_committed_minute'",
        "UPDATE rate_limits SET window=2 WHERE bucket='quote:global'",
        "UPDATE rate_limits SET window=-1 WHERE bucket='quote:global'",
        "UPDATE rate_limits SET window=9223372036854775807 WHERE bucket='quote:global'",
        "UPDATE rate_limits SET count=-1 WHERE bucket='quote:global'",
        "UPDATE rate_limits SET count=0 WHERE bucket='quote:global'",
        "UPDATE rate_limits SET count=601 WHERE bucket='quote:global'",
        "UPDATE rate_limits SET count='invalid' WHERE bucket='quote:global'",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("invalid-quote-rate.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let (agreement, payment, opened) = paid(&store);
        store.db.lock().unwrap().execute_batch(mutation).unwrap();
        let before = quote_rate_snapshot(&store);
        drop(store);
        let store = BillingStore::open(&path, &network()).unwrap();
        let candidate = quote(1000, false);
        assert!(store.store_quote(&candidate, 300).is_err(), "{mutation}");
        assert_eq!(quote_rate_snapshot(&store), before, "{mutation}");
        assert!(store.quote(&candidate.payload.id).unwrap_err().starts_with("not_found:"));
        assert_eq!(encode(&store.account(&agreement.payload.id).unwrap()).unwrap(),
            encode(&opened.account).unwrap());
        assert_eq!(store.authenticate(&opened.credential.as_ref().unwrap().bearer, 300).unwrap().credential_id, opened.credential.as_ref().unwrap().id);
        assert_eq!(store.quote(&agreement.payload.id).unwrap().payload, agreement.payload);
        assert!(store.funding_intent(&agreement.payload, &payment.raw_transaction).unwrap().is_some());
        // Restore the exact valid fixture counter explicitly, not automatic repair.
        store.db.lock().unwrap().execute_batch(
            "DELETE FROM rate_limits WHERE bucket GLOB 'quote:*';
             INSERT INTO rate_limits VALUES('quote:global',1,1);
             UPDATE metadata SET value='1' WHERE key='quote_committed_minute';
             INSERT OR IGNORE INTO metadata VALUES('quote_committed_minute','1');",
        ).unwrap();
        store.store_quote(&candidate, 300).unwrap();
        assert_eq!(committed_quote_rate(&store), (Some("5".into()), 5, 1, 0));
        assert_eq!(store.quote(&candidate.payload.id).unwrap().payload, candidate.payload);
        assert_eq!(encode(&store.account(&agreement.payload.id).unwrap()).unwrap(),
            encode(&opened.account).unwrap());
    }
}

fn quote_work_rate(store: &BillingStore) -> (String, u64, u64) {
    let db = store.db.lock().unwrap();
    db.query_row(
        "SELECT value,window,count FROM metadata,rate_limits
         WHERE key='quote_work_minute' AND bucket='quote-work:global'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap()
}

// Include accepted money, credentials, requests and recovery evidence, not just
// pending-quote counts. Only the independently spent work evidence is excluded.
fn quote_work_protected_snapshot(store: &BillingStore) -> Vec<Vec<String>> {
    let db = store.db.lock().unwrap();
    [
        "SELECT json_array(key,value) FROM metadata WHERE key<>'quote_work_minute' ORDER BY key",
        "SELECT json_array(bucket,window,count) FROM rate_limits WHERE bucket<>'quote-work:global' ORDER BY bucket",
        "SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id",
        "SELECT json_array(nonce,wallet,expires,record) FROM challenges ORDER BY nonce",
        "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
        "SELECT json_array(id,contract,request_key,credential,status,created,record) FROM requests ORDER BY id",
        "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding ORDER BY tx_hash",
        "SELECT json_array(tx_hash,contract,record,raw_transaction,updated) FROM funding_attempts ORDER BY tx_hash",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
        "SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations ORDER BY tx_hash",
        "SELECT json_array(id,status,record) FROM fee_batches ORDER BY id",
        "SELECT json_array(sequence,request,model,evidence,miss) FROM performance_observations ORDER BY sequence",
        "SELECT json_array(model,evidence) FROM performance_withdrawals ORDER BY model,evidence",
    ].into_iter().map(|query| {
        db.prepare(query).unwrap().query_map([], |row| row.get(0)).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap()
    }).collect()
}

#[test]
fn quote_work_bounds_600_real_signatures_when_every_insertion_rolls_back() {
    let store = memory();
    let (agreement, _, opened) = paid(&store);
    let pending = quote(1000, false);
    store.store_quote(&pending, 110).unwrap();
    assert!(store.sign_quote(pending.payload.clone(), PROVIDER_KEY, 100)
        .unwrap_err().starts_with("rate_limit:"));
    let protected = quote_work_protected_snapshot(&store);
    let committed = committed_quote_rate(&store);
    for attempt in 1..=600 {
        let mut payload = pending.payload.clone();
        // Named payers do not partition or replenish provider signing work.
        payload.consumer_wallet = format!("0x{attempt:040x}");
        let signed = store.sign_quote(payload.clone(), PROVIDER_KEY, 120).unwrap();
        protocol::verify(protocol::QUOTE_DOMAIN, &signed, &payload.provider_wallet).unwrap();
        assert_eq!(signed.payload, payload);
        assert!(store.store_quote(&signed, 120).unwrap_err().contains("UNIQUE"));
        assert_eq!(quote_work_rate(&store), ("2".into(), 2, attempt));
        assert_eq!(committed_quote_rate(&store), committed);
        assert_eq!(quote_work_protected_snapshot(&store), protected);
    }
    let before = quote_rate_snapshot(&store);
    assert!(store.sign_quote(pending.payload.clone(), "invalid-key", 120)
        .unwrap_err().starts_with("rate_limit:"));
    assert_eq!(quote_rate_snapshot(&store), before);
    assert_eq!(encode(&store.account(&agreement.payload.id).unwrap()).unwrap(),
        encode(&opened.account).unwrap());
    assert_eq!(store.account(&agreement.payload.id).unwrap().available_wei, "995");
}

#[test]
fn quote_work_failed_signer_spends_last_permit_without_refund() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("signer-failure.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (agreement, _, _) = paid(&store);
    assert!(store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 180).is_err());
    // Explicit restored evidence isolates the last-unit boundary.
    store.db.lock().unwrap().execute(
        "UPDATE rate_limits SET count=599 WHERE bucket='quote-work:global'", [],
    ).unwrap();
    let protected = quote_work_protected_snapshot(&store);
    let error = store.sign_quote(agreement.payload.clone(), "invalid-key", 120).unwrap_err();
    assert_eq!(error,
        protocol::sign(protocol::QUOTE_DOMAIN, agreement.payload.clone(), "invalid-key").unwrap_err());
    assert_eq!(quote_work_rate(&store), ("3".into(), 3, 600));
    assert_eq!(quote_work_protected_snapshot(&store), protected);
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    assert!(store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 180)
        .unwrap_err().starts_with("rate_limit:"));
    assert_eq!(quote_work_rate(&store), ("3".into(), 3, 600));
    let signed = store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 240).unwrap();
    protocol::verify(protocol::QUOTE_DOMAIN, &signed, &agreement.payload.provider_wallet).unwrap();
    assert_eq!(quote_work_rate(&store), ("4".into(), 4, 1));
    assert_eq!(quote_work_protected_snapshot(&store), protected);
}

#[test]
fn quote_work_initialization_restart_and_clock_regression_require_new_minute() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("work-clock.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (agreement, _, _) = paid(&store);
    let protected = quote_work_protected_snapshot(&store);
    assert!(store.sign_quote(agreement.payload.clone(), "invalid-key", 180)
        .unwrap_err().starts_with("rate_limit:"));
    assert_eq!(quote_work_rate(&store), ("3".into(), 3, 600));
    assert_eq!(quote_work_protected_snapshot(&store), protected);
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    for sample in [0, 120, 180, 239] {
        let before = quote_rate_snapshot(&store);
        assert!(store.sign_quote(agreement.payload.clone(), "invalid-key", sample)
            .unwrap_err().starts_with("rate_limit:"));
        assert_eq!(quote_rate_snapshot(&store), before);
    }
    for (sample, spent) in [(240, 1), (120, 2), (299, 3)] {
        let signed = store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, sample).unwrap();
        protocol::verify(protocol::QUOTE_DOMAIN, &signed, &agreement.payload.provider_wallet).unwrap();
        assert_eq!(signed.payload, agreement.payload, "effective minute is not a signed timestamp");
        assert_eq!(quote_work_rate(&store), ("4".into(), 4, spent));
        assert_eq!(quote_work_protected_snapshot(&store), protected);
    }
}

#[test]
fn quote_work_separate_connections_share_the_final_unit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("work-race.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (agreement, _, _) = paid(&store);
    assert!(store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 180).is_err());
    store.db.lock().unwrap().execute(
        "UPDATE rate_limits SET count=599 WHERE bucket='quote-work:global'", [],
    ).unwrap();
    let protected = quote_work_protected_snapshot(&store);
    let writers = [120, 180].map(|sample|
        (BillingStore::open(&path, &network()).unwrap(), sample));
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles = writers.map(|(ledger, sample)| {
            let barrier = &barrier;
            let payload = agreement.payload.clone();
            scope.spawn(move || {
                barrier.wait();
                ledger.sign_quote(payload, PROVIDER_KEY, sample)
            })
        });
        handles.map(|handle| handle.join().unwrap())
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    for result in results {
        match result {
            Ok(signed) => {
                protocol::verify(protocol::QUOTE_DOMAIN, &signed, &agreement.payload.provider_wallet).unwrap();
                assert_eq!(signed.payload, agreement.payload);
            }
            Err(error) => assert!(error.starts_with("rate_limit:"), "{error}"),
        }
    }
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    assert_eq!(quote_work_rate(&store), ("3".into(), 3, 600));
    assert_eq!(quote_work_protected_snapshot(&store), protected);
    assert!(store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 120)
        .unwrap_err().starts_with("rate_limit:"));
}

#[test]
fn quote_work_survives_future_challenge_cleanup_and_legacy_quote_retirement() {
    let store = memory();
    let (agreement, payment, opened) = paid(&store);
    assert!(store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 180).is_err());
    store.db.lock().unwrap().execute_batch(
        "DELETE FROM metadata WHERE key='quote_committed_minute';
         INSERT INTO rate_limits VALUES('quote:legacy-wallet',1,1);",
    ).unwrap();
    let pending = quote(1000, false);
    store.store_quote(&pending, 180).unwrap();
    assert_eq!(committed_quote_rate(&store), (Some("3".into()), 3, 1, 0));
    assert_eq!(quote_work_rate(&store), ("3".into(), 3, 600));
    let challenge = crate::auth::AuthPayload {
        network: network(), provider_wallet: agreement.payload.provider_wallet.clone(),
        wallet: agreement.payload.consumer_wallet.clone(), method: "GET".into(),
        path: "/v1/models".into(), body_hash: protocol::hash_hex(b""),
        nonce: format!("0x{:064x}", 78), expires_at: 720,
    };
    store.create_challenge(&challenge, 600).unwrap();
    assert!(store.quote(&pending.payload.id).is_err(), "cleanup actually ran");
    assert_eq!(quote_work_rate(&store), ("3".into(), 3, 600));
    assert!(store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 239)
        .unwrap_err().starts_with("rate_limit:"));
    store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 240).unwrap();
    assert_eq!(quote_work_rate(&store), ("4".into(), 4, 1));
    assert_eq!(encode(&store.account(&agreement.payload.id).unwrap()).unwrap(),
        encode(&opened.account).unwrap());
    assert_eq!(store.authenticate(&opened.credential.as_ref().unwrap().bearer, 600).unwrap().credential_id,
        opened.credential.as_ref().unwrap().id);
    assert!(store.funding_intent(&agreement.payload, &payment.raw_transaction).unwrap().is_some());
}

#[test]
fn quote_work_persistence_begin_statement_and_commit_failures_return_no_signature() {
    for initialized in [false, true] {
        for fault in ["readonly", "statement", "commit"] {
            let store = memory();
            let (agreement, _, _) = paid(&store);
            if initialized {
                assert!(store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 60).is_err());
            }
            {
                let db = store.db.lock().unwrap();
                match fault {
                    "readonly" => db.execute_batch("PRAGMA query_only=ON;").unwrap(),
                    "statement" => db.execute_batch(
                        "CREATE TRIGGER refuse_quote_work BEFORE INSERT ON metadata
                         WHEN NEW.key='quote_work_minute'
                         BEGIN SELECT RAISE(ABORT,'work statement failure'); END;",
                    ).unwrap(),
                    "commit" => db.execute_batch(
                        "CREATE TABLE work_commit_fault(
                            account TEXT REFERENCES accounts(id) DEFERRABLE INITIALLY DEFERRED);
                         CREATE TRIGGER fail_quote_work_commit BEFORE INSERT ON metadata
                         WHEN NEW.key='quote_work_minute'
                         BEGIN INSERT INTO work_commit_fault VALUES('missing-account'); END;",
                    ).unwrap(),
                    _ => unreachable!(),
                }
            }
            let before = quote_rate_snapshot(&store);
            let protected = quote_work_protected_snapshot(&store);
            let error = store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 120).unwrap_err();
            assert!(error.starts_with("ledger database:"), "{fault}: {error}");
            assert_eq!(quote_rate_snapshot(&store), before, "{fault}, initialized={initialized}");
            assert_eq!(quote_work_protected_snapshot(&store), protected);
            {
                let db = store.db.lock().unwrap();
                match fault {
                    "readonly" => db.execute_batch("PRAGMA query_only=OFF;").unwrap(),
                    "statement" => db.execute_batch("DROP TRIGGER refuse_quote_work;").unwrap(),
                    "commit" => {
                        let rows: u64 = db.query_row("SELECT COUNT(*) FROM work_commit_fault", [], |row| row.get(0)).unwrap();
                        assert_eq!(rows, 0, "failed commit must roll back trigger writes too");
                        db.execute_batch("DROP TRIGGER fail_quote_work_commit;").unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            if initialized {
                let signed = store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 120).unwrap();
                protocol::verify(protocol::QUOTE_DOMAIN, &signed, &agreement.payload.provider_wallet).unwrap();
                assert_eq!(quote_work_rate(&store), ("2".into(), 2, 1));
            } else {
                assert!(store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 120)
                    .unwrap_err().starts_with("rate_limit:"));
                assert_eq!(quote_work_rate(&store), ("2".into(), 2, 600));
            }
            assert_eq!(quote_work_protected_snapshot(&store), protected);
        }
    }
}

#[test]
fn quote_work_invalid_evidence_fails_closed_without_blocking_paid_settlement() {
    for mutation in [
        "DELETE FROM rate_limits WHERE bucket='quote-work:global'",
        "DELETE FROM metadata WHERE key='quote_work_minute'",
        "UPDATE metadata SET value='' WHERE key='quote_work_minute'",
        "UPDATE metadata SET value='invalid' WHERE key='quote_work_minute'",
        "UPDATE metadata SET value='01' WHERE key='quote_work_minute'",
        "UPDATE metadata SET value='+1' WHERE key='quote_work_minute'",
        "UPDATE metadata SET value='1 ' WHERE key='quote_work_minute'",
        "UPDATE metadata SET value='1.0' WHERE key='quote_work_minute'",
        "UPDATE metadata SET value='18446744073709551616' WHERE key='quote_work_minute'",
        "UPDATE metadata SET value='2' WHERE key='quote_work_minute'",
        "UPDATE rate_limits SET window=2 WHERE bucket='quote-work:global'",
        "UPDATE rate_limits SET window=-1 WHERE bucket='quote-work:global'",
        "UPDATE rate_limits SET window='invalid' WHERE bucket='quote-work:global'",
        "UPDATE rate_limits SET window=1.5 WHERE bucket='quote-work:global'",
        "UPDATE rate_limits SET window=9223372036854775807 WHERE bucket='quote-work:global';
         UPDATE metadata SET value='9223372036854775807' WHERE key='quote_work_minute'",
        "UPDATE rate_limits SET count=-1 WHERE bucket='quote-work:global'",
        "UPDATE rate_limits SET count=0 WHERE bucket='quote-work:global'",
        "UPDATE rate_limits SET count=601 WHERE bucket='quote-work:global'",
        "UPDATE rate_limits SET count='invalid' WHERE bucket='quote-work:global'",
        "UPDATE rate_limits SET count=1.5 WHERE bucket='quote-work:global'",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("invalid-work.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let (agreement, payment, opened) = paid(&store);
        let credential = opened.credential.as_ref().unwrap();
        let held = store.reserve(&agreement.payload.id, &credential.id,
            "recoverable-work-fault", 100, 100, 110, &agreement.payload.model).unwrap();
        let account = encode(&store.account(&agreement.payload.id).unwrap()).unwrap();
        assert!(store.sign_quote(agreement.payload.clone(), PROVIDER_KEY, 100).is_err());
        store.db.lock().unwrap().execute_batch(mutation).unwrap();
        let before = quote_rate_snapshot(&store);
        let protected = quote_work_protected_snapshot(&store);
        drop(store);
        let store = BillingStore::open(&path, &network()).unwrap();
        let candidate = quote(1000, false).payload;
        let error = store.sign_quote(candidate.clone(), PROVIDER_KEY, 300).unwrap_err();
        assert!(error.starts_with("ledger "), "{mutation}: {error}");
        assert_eq!(quote_rate_snapshot(&store), before, "{mutation}");
        assert_eq!(quote_work_protected_snapshot(&store), protected, "{mutation}");
        assert!(store.quote(&candidate.id).unwrap_err().starts_with("not_found:"));
        assert_eq!(encode(&store.account(&agreement.payload.id).unwrap()).unwrap(), account);
        assert_eq!(store.authenticate(&credential.bearer, 300).unwrap().credential_id, credential.id);
        assert_eq!(store.quote(&agreement.payload.id).unwrap().payload, agreement.payload);
        assert!(store.funding_intent(&agreement.payload, &payment.raw_transaction).unwrap().is_some());
        let settled = store.settle(&held.id,
            &TokenUsage { prompt_tokens: 100, completion_tokens: 50 }, "completed", 301).unwrap();
        assert_eq!(settled.cost_wei, "150");
        let account = store.account(&agreement.payload.id).unwrap();
        assert_eq!(account.funded_wei, "1000");
        assert_eq!(account.fee_wei, "5");
        assert_eq!(account.spent_wei, "150");
        assert_eq!(account.reserved_wei, "0");
        assert_eq!(account.available_wei, "845");
        assert_eq!(account.reversed_wei, "0");
        assert_eq!(account.provider_loss_wei, "0");
        let after_settlement = quote_work_protected_snapshot(&store);
        assert!(store.sign_quote(candidate, PROVIDER_KEY, 360).is_err());
        assert_eq!(quote_work_protected_snapshot(&store), after_settlement);
    }
}

// account-credentials acceptance evidence (tasks 3.1, 3.2, 3.4, 3.5 and 3.6). Ledger
// tests use logical time; the router tests use current time, because the production
// control and inference routers read the real clock.

/// A paid account opened at `now` on `store`, funded with `funding` wei, whose signed
/// concurrency limit is `max_concurrency` and whose agreement is otherwise `quote`'s, then
/// changed by `edit`. Verified funding is a ledger prerequisite after the chain boundary,
/// as in `paid`.
fn paid_at(
    store: &BillingStore,
    funding: u128,
    max_concurrency: u32,
    now: u64,
    edit: impl FnOnce(&mut Quote),
) -> (Signed<Quote>, crate::protocol::OpenResult) {
    let mut payload = quote(funding, false).payload;
    payload.max_concurrency = max_concurrency;
    payload.issued_at = now;
    payload.expires_at = now + 300;
    edit(&mut payload);
    let agreement = protocol::sign(protocol::QUOTE_DOMAIN, payload, PROVIDER_KEY).unwrap();
    store.store_quote(&agreement, now).unwrap();
    let funding_payment = payment(&agreement.payload, true, funding, 1);
    register(store, &agreement, &funding_payment, now);
    let opened = store.open_account(&agreement, Some(&funding_payment), now).unwrap();
    (agreement, opened)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// The production control router and public inference router over a file-backed
/// ledger holding one paid account (`paid_at`, now unless `opened_at` says otherwise).
/// The provider is accepting, new
/// funding is ready and the agreed model is qualified until a test closes them; no
/// chain RPC or engine answers.
struct CredentialRoutes {
    _directory: tempfile::TempDir,
    path: std::path::PathBuf,
    state: Arc<crate::app::ProviderState>,
    agreement: Signed<Quote>,
    opened: crate::protocol::OpenResult,
}

impl CredentialRoutes {
    fn new(funding: u128, max_concurrency: u32) -> Self {
        Self::opened_at(funding, max_concurrency, protocol::now().unwrap())
    }

    /// `new`, with the account opened at the explicit ledger time `opened_at`. The
    /// routers still read the real clock.
    fn opened_at(funding: u128, max_concurrency: u32, opened_at: u64) -> Self {
        Self::opened_with(funding, max_concurrency, opened_at, |_| {})
    }

    /// `opened_at`, whose agreement, and so the served model and price, is first changed
    /// by `edit`.
    fn opened_with(funding: u128, max_concurrency: u32, opened_at: u64, edit: impl FnOnce(&mut Quote)) -> Self {
        use crate::{
            app::ProviderState,
            chain_client::ChainClient,
            network::NetworkConfig,
            offer_registry::OfferRegistry,
            provider_config::{DhtConfig, ModelConfig, ProviderConfig},
        };
        use std::sync::atomic::AtomicBool;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite");
        let ledger = Arc::new(BillingStore::open(&path, &network()).unwrap());
        let (agreement, opened) = paid_at(&ledger, funding, max_concurrency, opened_at, edit);
        let network_config = NetworkConfig {
            identity: network(),
            rpc_url: "http://127.0.0.1:1".into(),
            rpc_bearer_file: None,
            faucet_url: None,
            explorer_url: None,
            policy_url: None,
        };
        let config = ProviderConfig {
            network: network_config.clone(),
            data_dir: directory.path().into(),
            signing_key_file: directory.path().join("key"),
            admin_token_file: directory.path().join("admin"),
            public_endpoint: "http://127.0.0.1:9419".into(),
            public_bind: "127.0.0.1:9419".parse().unwrap(),
            admin_bind: "127.0.0.1:9421".parse().unwrap(),
            platform_wallet: agreement.payload.platform_wallet.clone(),
            fee_bps: 50,
            min_funding_wei: "1000".into(),
            models: vec![ModelConfig {
                model: agreement.payload.model.clone(),
                price: agreement.payload.price.clone(),
                engine_url: "http://127.0.0.1:1".into(),
                engine_api_key_file: None,
                max_concurrency,
                request_timeout_secs: 1,
                qualification_evidence_file: None,
                runtime: None,
            }],
            dht: DhtConfig {
                listen: "/ip4/127.0.0.1/tcp/0".into(),
                bootstrap: vec![],
                identity_file: directory.path().join("dht"),
            },
            development: true,
        };
        let state = Arc::new(ProviderState {
            model_slots: ProviderState::slots(&config),
            managed_engines: Default::default(),
            config: Arc::new(config),
            chain: Arc::new(ChainClient::new(network_config, directory.path().into()).unwrap()),
            ledger,
            private_key: Arc::new(PROVIDER_KEY.into()),
            wallet: agreement.payload.provider_wallet.clone(),
            offers: Arc::new(OfferRegistry::new(network(), true)),
            accepting: AtomicBool::new(true),
            settlement_fault: Default::default(),
            funding_ready: AtomicBool::new(true),
            qualified_models: std::sync::RwLock::new(
                [agreement.payload.model.id.clone()].into_iter().collect(),
            ),
            embedding_dimensions: std::sync::RwLock::new(Default::default()),
        });
        Self { _directory: directory, path, state, agreement, opened }
    }

    fn id(&self) -> String {
        self.agreement.payload.id.clone()
    }

    fn ledger(&self) -> &BillingStore {
        &self.state.ledger
    }

    /// A credential issued directly in the ledger, valid for an hour, with no cap.
    fn issue(&self, max_concurrency: u32, scopes: &[&str]) -> Credential {
        let now = protocol::now().unwrap();
        self.ledger()
            .issue_credential(
                &self.id(),
                &CredentialIssueRequest {
                    expires_at: now + 3600,
                    spend_cap_wei: None,
                    max_concurrency,
                    scopes: scopes.iter().map(|scope| scope.to_string()).collect(),
                },
                now,
            )
            .unwrap()
    }

    /// The credential authenticates now for this account, as `credential.id`.
    fn authenticates(&self, credential: &Credential) -> CredentialContext {
        let context = self
            .ledger()
            .authenticate(&credential.bearer, protocol::now().unwrap())
            .unwrap();
        assert_eq!(context.account.contract_id, self.id());
        assert_eq!(context.credential_id, credential.id);
        context
    }

    /// Complete credential rows, read through an independent connection.
    fn credential_rows(&self) -> Vec<Value> {
        let db = Connection::open(&self.path).unwrap();
        let mut statement = db
            .prepare("SELECT json_array(id,contract,token_hash,json(record),revoked) FROM credentials ORDER BY id")
            .unwrap();
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
            .collect();
        rows
    }

    /// Sends `body` to the control router with a valid one-use owner proof for exactly
    /// this method, path and body, recorded in the ledger as `owner_post` records it.
    /// Returns the status, the document and the proof's nonce.
    async fn owner(&self, method: &str, path: &str, body: Vec<u8>) -> (axum::http::StatusCode, Value, String) {
        let now = protocol::now().unwrap();
        let wallet = crate::wallet::address_from_private_key(OWNER_KEY).unwrap();
        let proof = AuthPayload {
            network: network(),
            provider_wallet: self.state.wallet.clone(),
            wallet: wallet.clone(),
            method: method.into(),
            path: path.into(),
            body_hash: protocol::hash_hex(&body),
            nonce: protocol::random_id(),
            expires_at: now + 120,
        };
        self.ledger().create_challenge(&proof, now).unwrap();
        let signature =
            crate::wallet::sign_message(OWNER_KEY, &crate::auth::challenge_payload(&proof).unwrap())
                .unwrap();
        let request = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("x-chiral-owner", wallet)
            .header("x-chiral-nonce", &proof.nonce)
            .header("x-chiral-signature", signature)
            .body(axum::body::Body::from(body))
            .unwrap();
        let (status, document) = self.control(request).await;
        (status, document, proof.nonce)
    }

    /// Sends `body` to the control router without any credential.
    async fn anonymous(&self, method: &str, path: &str, body: Vec<u8>) -> (axum::http::StatusCode, Value) {
        let request = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap();
        self.control(request).await
    }

    async fn control(&self, request: axum::http::Request<axum::body::Body>) -> (axum::http::StatusCode, Value) {
        use tower::ServiceExt;
        let response = crate::contracts::router(self.state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// Sends a bodiless request with `bearer` to the public inference router.
    async fn public(&self, method: &str, path: &str, bearer: &str) -> (axum::http::StatusCode, Value) {
        use tower::ServiceExt;
        let request = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {bearer}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let response = crate::inference_gateway::router(self.state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        assert!(!contains(&bytes, bearer.as_bytes()), "a response echoed the bearer");
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    fn entry(&self, credential: &str) -> CredentialView {
        self.ledger()
            .credentials(&self.id())
            .unwrap()
            .into_iter()
            .find(|entry| entry.id == credential)
            .unwrap()
    }
}

/// account-credentials requirement 1 (task 3.1), scenarios *Options outside the contract
/// are refused* (including a scope list that mixes `infer` with an unknown scope) and *A
/// positional array is not the options object*, through the production control router.
/// Each options body is sent with
/// a valid owner proof for exactly that body, once as an issuance and once as a
/// rotation of a valid credential A. Each is refused, A still authenticates, and every
/// credential row and the account view stay exactly as they were. That includes
/// rotation's revocation of A, which happens before the replacement is checked and must
/// roll back: requirement 4 *A refused rotation keeps its predecessor* (concurrency case).
/// The error codes are current implementation values (design decision 4); the
/// requirement says only "refused". Who may send these and the proof's consumption are
/// `inference-owner-control`'s; consumption is checked only as a fixture precondition.
#[tokio::test]
async fn credential_options_outside_the_contract_change_no_credential() {
    let expires_at = protocol::now().unwrap() + 3600;
    let valid = json!({"expires_at": expires_at, "spend_cap_wei": null, "max_concurrency": 1, "scopes": ["infer"]});
    let with = |member: &str, value: Value| {
        let mut body = valid.clone();
        body[member] = value;
        serde_json::to_vec(&body).unwrap()
    };
    let repeated = format!(
        r#"{{"expires_at":{expires_at},"spend_cap_wei":null,"max_concurrency":1,"scopes":["infer"],"scopes":["read"]}}"#
    )
    .into_bytes();
    // The account's signed concurrency limit is 2.
    let cases = [
        ("empty scopes", with("scopes", json!([])), "invalid_credential"),
        ("unknown scope", with("scopes", json!(["admin"])), "invalid_credential"),
        // Every element must be `infer` or `read`, not merely one of them.
        ("known and unknown scope", with("scopes", json!(["infer", "admin"])), "invalid_credential"),
        // Scenario *A positional array is not the options object* (task 2.1's decoder).
        ("positional array", serde_json::to_vec(&json!([expires_at, null, 1, ["read"]])).unwrap(), "invalid_request"),
        ("concurrency above the account's", with("max_concurrency", json!(3)), "invalid_credential"),
        ("leading-zero cap", with("spend_cap_wei", json!("01")), "invalid_amount"),
        ("fractional cap", with("spend_cap_wei", json!("1.5")), "invalid_amount"),
        ("negative cap", with("spend_cap_wei", json!("-1")), "invalid_amount"),
        ("empty cap", with("spend_cap_wei", json!("")), "invalid_amount"),
        ("numeric cap", with("spend_cap_wei", json!(1)), "invalid_request"),
        ("unknown member", with("account", json!("other")), "invalid_request"),
        ("repeated member", repeated, "invalid_request"),
    ];
    for (name, body, code) in cases {
        // A ledger per case keeps the owner within its per-minute challenge budget.
        let routes = CredentialRoutes::new(1000, 2);
        let id = routes.id();
        let named = routes.issue(1, &["infer", "read"]);
        let rows = routes.credential_rows();
        assert_eq!(rows.len(), 2);
        let account = encode(&routes.ledger().account(&id).unwrap()).unwrap();
        for path in [
            format!("/v1/contracts/{id}/credentials"),
            format!("/v1/contracts/{id}/credentials/{}/rotate", named.id),
        ] {
            let (status, document, nonce) = routes.owner("POST", &path, body.clone()).await;
            // Only the error member is printed: an accepted body would carry a secret.
            assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{name} {path}: {}", document["error"]);
            assert_eq!(document["error"]["code"], code, "{name} {path}: {}", document["error"]);
            assert!(routes.ledger().challenge(&nonce).is_err(), "{name}: the proof was not consumed");
            assert_eq!(routes.credential_rows(), rows, "{name} {path}: a credential row changed");
            assert_eq!(encode(&routes.ledger().account(&id).unwrap()).unwrap(), account, "{name}");
            routes.authenticates(&named);
            routes.authenticates(routes.opened.credential.as_ref().unwrap());
        }
    }
}

/// account-credentials requirement 1 (task 3.1), scenario *Valid options issue a
/// credential with exactly those limits*: with an account concurrency limit of 2,
/// issuance and rotation with `{"spend_cap_wei":"900","max_concurrency":2,
/// "scopes":["infer","read"]}` return the credential, and the owner's credential list
/// shows each with exactly that expiry, cap, concurrency and both scopes. The issued
/// `expires_at` is the time from which the credential no longer authenticates. Neither
/// operation changes the account view.
#[tokio::test]
async fn valid_options_issue_and_rotate_credentials_with_exactly_those_limits() {
    let routes = CredentialRoutes::new(1000, 2);
    let id = routes.id();
    let named = routes.issue(1, &["read"]);
    let account = encode(&routes.ledger().account(&id).unwrap()).unwrap();
    let expires_at = protocol::now().unwrap() + 3600;
    let options = json!({"expires_at": expires_at, "spend_cap_wei": "900", "max_concurrency": 2, "scopes": ["infer", "read"]});
    for path in [
        format!("/v1/contracts/{id}/credentials"),
        format!("/v1/contracts/{id}/credentials/{}/rotate", named.id),
    ] {
        let before = protocol::now().unwrap();
        let (status, document, _) = routes.owner("POST", &path, serde_json::to_vec(&options).unwrap()).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{path}: {}", document["error"]);
        let credential: Credential = serde_json::from_value(document).unwrap();
        assert_eq!(credential.contract_id, id);
        assert_eq!(credential.expires_at, expires_at);
        let (status, list, _) = routes.owner("GET", &format!("/v1/contracts/{id}/credentials"), Vec::new()).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{list}");
        let entry = list.as_array().unwrap().iter().find(|entry| entry["id"] == credential.id).unwrap();
        let created_at = entry["created_at"].as_u64().unwrap();
        assert!((before..=protocol::now().unwrap()).contains(&created_at));
        assert_eq!(
            entry,
            &json!({"id": credential.id, "contract_id": id, "expires_at": expires_at,
                "spend_cap_wei": "900", "spent_wei": "0", "reserved_wei": "0",
                "max_concurrency": 2, "scopes": ["infer", "read"], "revoked": false,
                "created_at": created_at, "root": false})
        );
        assert_eq!(routes.authenticates(&credential).scopes, ["infer", "read"]);
        assert!(routes.ledger().authenticate(&credential.bearer, expires_at - 1).is_ok());
        assert!(routes.ledger().authenticate(&credential.bearer, expires_at).unwrap_err().starts_with("unauthorized:"));
        assert_eq!(encode(&routes.ledger().account(&id).unwrap()).unwrap(), account);
    }
    assert!(routes.entry(&named.id).revoked);
    assert!(routes.ledger().authenticate(&named.bearer, protocol::now().unwrap()).is_err());
}

/// account-credentials requirement 2 (task 3.2) on a file-backed ledger. Credentials
/// created by a first opening, an issuance and a rotation:
/// - appear in no ledger file, main database, `-wal`, `-shm` or `-journal`, either
///   whole, as the 64 random hex digits after `chiral_` (lower or upper case) or as the
///   32 bytes they encode, while the ledger is open (the recent pages are in the WAL)
///   and after it is closed;
/// - are stored with `token_hash` = `protocol::hash_hex(secret)` (Keccak-256 is the
///   current implementation value, design decision 4);
/// - have list entries with exactly the `CredentialView` members, so no `token_hash` or
///   secret, and the account, request and receipt views carry no secret either;
/// - have a public identifier that is not the secret, its random part or its hash, and
///   neither the identifier nor the secret contains the other.
#[test]
fn the_ledger_keeps_only_a_hash_of_each_secret_and_an_independent_identifier() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, opened) = paid(&store);
    let id = q.payload.id.clone();
    let root = opened.credential.clone().unwrap();
    let options = CredentialIssueRequest {
        expires_at: 1000,
        spend_cap_wei: Some("900".into()),
        max_concurrency: 2,
        scopes: vec!["infer".into(), "read".into()],
    };
    let issued = store.issue_credential(&id, &options, 102).unwrap();
    let rotated = store.rotate_credential(&id, &issued.id, &options, 103).unwrap();
    let created = [&root, &issued, &rotated];
    let held = store.reserve(&id, &rotated.id, "held", 10, 10, 104, &q.payload.model).unwrap();
    // Each secret is searched for whole, as its 64 random hex digits in either case, and
    // as the 32 random bytes those digits encode (a blob column would hold these).
    let forms: Vec<Vec<u8>> = created
        .iter()
        .flat_map(|credential| {
            let random = &credential.bearer[7..];
            [
                credential.bearer.clone().into_bytes(),
                random.as_bytes().to_vec(),
                random.to_uppercase().into_bytes(),
                hex::decode(random).unwrap(),
            ]
        })
        .collect();
    assert!(forms.iter().all(|form| form.len() >= 32));
    let secrets = |text: &[u8]| forms.iter().any(|form| contains(text, form));
    let search = |stage: &str| {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(directory.path()).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            assert!(name.starts_with("ledger.sqlite"), "{stage}: unexpected file {name}");
            assert!(!secrets(&std::fs::read(entry.path()).unwrap()), "{stage}: {name} holds a secret, as text or as its random bytes");
            names.push(name);
        }
        names.sort();
        names
    };
    // The search covers the write-ahead log, where SQLite keeps the recent pages.
    let open = search("open");
    assert!(open.contains(&"ledger.sqlite".to_string()) && open.contains(&"ledger.sqlite-wal".to_string()), "{open:?}");
    let db = Connection::open(&path).unwrap();
    for credential in created {
        let hash: String = db
            .query_row("SELECT token_hash FROM credentials WHERE id=?1", [&credential.id], |row| row.get(0))
            .unwrap();
        assert_eq!(hash, protocol::hash_hex(credential.bearer.as_bytes()));
        assert_ne!(credential.id, credential.bearer);
        assert!(!credential.bearer.contains(&credential.id) && !credential.id.contains(&credential.bearer));
        assert!(!credential.id.contains(&credential.bearer[7..]) && !credential.bearer.contains(&credential.id[2..]));
        assert_ne!(credential.id, hash);
    }
    drop(db);
    let members = [
        "contract_id", "created_at", "expires_at", "id", "max_concurrency", "reserved_wei",
        "revoked", "root", "scopes", "spend_cap_wei", "spent_wei",
    ];
    let list = serde_json::to_value(store.credentials(&id).unwrap()).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 3);
    for entry in list.as_array().unwrap() {
        let mut keys: Vec<_> = entry.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, members);
        assert!(!secrets(entry.to_string().as_bytes()));
    }
    for view in [
        serde_json::to_value(store.account(&id).unwrap()).unwrap(),
        serde_json::to_value(store.requests(&id, None).unwrap()).unwrap(),
        serde_json::to_value(store.request_view(&id, &held.id).unwrap()).unwrap(),
        serde_json::to_value(store.receipt(&id, PROVIDER_KEY, 105).unwrap()).unwrap(),
        serde_json::to_value(store.open_account(&q, None, 106).unwrap()).unwrap(),
    ] {
        assert!(!secrets(view.to_string().as_bytes()), "a view carries a secret");
    }
    drop(store);
    search("closed");
}

/// account-credentials requirement 5 (task 3.5) on a paid account with 1,000 wei
/// available (1,005 wei funded, 5 wei platform fee):
/// - issuing a credential with cap 10^18 wei leaves the account view, funding history,
///   fee obligations and fee batches unchanged;
/// - that credential's reservation with maximum cost 2,000 wei fails with
///   `insufficient_funds`, not `spend_cap`, and reserves nothing;
/// - revoking a credential with cap 900 wei that has spent 100 wei, rotating another and
///   the expiry of a third create no funding, refund or fee record and change no amount:
///   the 800 wei of unused cap is not money.
#[test]
fn credential_caps_restrict_authority_and_never_create_or_refund_money() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let q = quote(1005, false);
    store.store_quote(&q, 100).unwrap();
    let funding = payment(&q.payload, true, 1005, 1);
    register(&store, &q, &funding, 100);
    store.open_account(&q, Some(&funding), 101).unwrap();
    let id = q.payload.id.clone();
    let model = &q.payload.model;
    let account = store.account(&id).unwrap();
    assert_eq!(
        (account.funded_wei.as_str(), account.fee_wei.as_str(), account.available_wei.as_str()),
        ("1005", "5", "1000")
    );
    let money_state = || {
        let db = Connection::open(&path).unwrap();
        let count = |table: &str| -> i64 {
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)).unwrap()
        };
        json!({
            "account": store.account(&id).unwrap(),
            "agreement": store.quote(&id).unwrap(),
            "funding": store.funding_history(&id).unwrap(),
            "fee_batches": store.fee_batches().unwrap(),
            "fees": store.fee_summary().unwrap(),
            "rows": [count("funding"), count("funding_attempts"), count("fee_obligations"), count("fee_batches")],
        })
    };
    let options = |cap: &str| CredentialIssueRequest {
        expires_at: 1000,
        spend_cap_wei: Some(cap.into()),
        max_concurrency: 2,
        scopes: vec!["infer".into(), "read".into()],
    };
    let before = money_state();
    let large = store.issue_credential(&id, &options("1000000000000000000"), 102).unwrap();
    assert_eq!(money_state(), before);
    // Prices are 1,000 wei per 1,000 tokens each way: 1,000 + 1,000 tokens cost 2,000 wei.
    let error = store.reserve(&id, &large.id, "beyond-balance", 1000, 1000, 103, model).unwrap_err();
    assert!(error.starts_with("insufficient_funds:"), "{error}");
    assert!(store.request_view(&id, "beyond-balance").is_err());
    assert_eq!(money_state(), before);
    assert_eq!(store.credentials(&id).unwrap().iter().find(|c| c.id == large.id).unwrap().reserved_wei, "0");

    let capped = store.issue_credential(&id, &options("900"), 104).unwrap();
    let spend = store.reserve(&id, &capped.id, "spend-100", 50, 50, 105, model).unwrap();
    assert_eq!(spend.max_cost_wei, "100");
    store.settle(&spend.id, &TokenUsage { prompt_tokens: 50, completion_tokens: 50 }, "completed", 106).unwrap();
    let entry = store.credentials(&id).unwrap().into_iter().find(|c| c.id == capped.id).unwrap();
    assert_eq!((entry.spend_cap_wei.as_deref(), entry.spent_wei.as_str(), entry.reserved_wei.as_str()), (Some("900"), "100", "0"));
    let spent = store.account(&id).unwrap();
    assert_eq!(
        (spent.funded_wei.as_str(), spent.fee_wei.as_str(), spent.spent_wei.as_str(), spent.reserved_wei.as_str(), spent.available_wei.as_str()),
        ("1005", "5", "100", "0", "900")
    );
    let before = money_state();
    store.revoke_credential(&id, &capped.id).unwrap();
    assert!(store.authenticate(&capped.bearer, 107).is_err());
    assert_eq!(money_state(), before);
    store.rotate_credential(&id, &large.id, &options("1000000000000000000"), 108).unwrap();
    assert_eq!(money_state(), before);
    let expiring = store.issue_credential(&id, &CredentialIssueRequest { expires_at: 200, ..options("900") }, 109).unwrap();
    assert!(store.authenticate(&expiring.bearer, 200).is_err());
    assert!(store.reserve(&id, &expiring.id, "after-expiry", 1, 1, 200, model).unwrap_err().starts_with("forbidden:"));
    assert_eq!(money_state(), before);
}

/// account-credentials requirement 4 (task 3.4), scenario *An expired or revoked
/// credential can be rotated*, through the production control router. Credential A has
/// expired (a restored record whose `expires_at` has passed, not a sleep), or the owner
/// has revoked it. The owner rotates A with valid options that include `infer`. The
/// replacement authenticates for the account with exactly the requested scopes, also on
/// the public routes; A still does not authenticate and is listed revoked; the account's
/// other credential and its account view are unchanged.
#[tokio::test]
async fn an_expired_or_revoked_credential_rotates_into_a_working_replacement() {
    use axum::http::StatusCode;
    for (case, scopes) in [("expired", json!(["infer"])), ("revoked", json!(["infer", "read"]))] {
        let routes = CredentialRoutes::new(1000, 2);
        let id = routes.id();
        let unusable = routes.issue(1, &["infer", "read"]);
        let other = routes.issue(1, &["infer"]);
        let now = protocol::now().unwrap();
        if case == "expired" {
            let db = Connection::open(&routes.path).unwrap();
            assert_eq!(
                db.execute("UPDATE credentials SET record=json_set(record,'$.expires_at',?1) WHERE id=?2", params![now - 1, unusable.id])
                    .unwrap(),
                1
            );
        } else {
            let path = format!("/v1/contracts/{id}/credentials/{}", unusable.id);
            let (status, document, _) = routes.owner("DELETE", &path, Vec::new()).await;
            assert_eq!(status, StatusCode::OK, "{document}");
            assert_eq!(document, json!({"revoked": true}));
        }
        assert!(routes.ledger().authenticate(&unusable.bearer, now).unwrap_err().starts_with("unauthorized:"));
        let (status, document) = routes.public("GET", "/v1/models", &unusable.bearer).await;
        assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("invalid_api_key")));
        let account = encode(&routes.ledger().account(&id).unwrap()).unwrap();

        let options = json!({"expires_at": now + 3600, "spend_cap_wei": null, "max_concurrency": 1, "scopes": scopes});
        let path = format!("/v1/contracts/{id}/credentials/{}/rotate", unusable.id);
        let (status, document, _) = routes.owner("POST", &path, serde_json::to_vec(&options).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{case}: {}", document["error"]);
        let replacement: Credential = serde_json::from_value(document).unwrap();
        assert_eq!(replacement.contract_id, id);
        assert_ne!(replacement.id, unusable.id);
        assert_ne!(replacement.bearer, unusable.bearer);
        assert_eq!(json!(routes.authenticates(&replacement).scopes), scopes, "{case}");
        let entry = routes.entry(&replacement.id);
        assert_eq!((entry.revoked, entry.max_concurrency, json!(entry.scopes)), (false, 1, scopes.clone()));
        let (status, models) = routes.public("GET", "/v1/models", &replacement.bearer).await;
        assert_eq!(status, StatusCode::OK, "{case}: {models}");
        assert_eq!(models["data"][0]["id"], routes.agreement.payload.model.id);
        let (status, read) = routes.public("GET", "/v1/requests/no-such-request", &replacement.bearer).await;
        let expected = if case == "expired" {
            (StatusCode::FORBIDDEN, "credential_scope")
        } else {
            (StatusCode::NOT_FOUND, "request_not_found")
        };
        assert_eq!((status, read["error"]["code"].as_str().unwrap()), expected, "{case}: {read}");

        assert!(routes.ledger().authenticate(&unusable.bearer, protocol::now().unwrap()).is_err());
        let (status, document) = routes.public("GET", "/v1/models", &unusable.bearer).await;
        assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("invalid_api_key")));
        assert!(routes.entry(&unusable.id).revoked);
        routes.authenticates(&other);
        routes.authenticates(routes.opened.credential.as_ref().unwrap());
        assert_eq!(encode(&routes.ledger().account(&id).unwrap()).unwrap(), account, "{case}");
    }
}

/// account-credentials requirement 4 (task 3.4), scenario *An identifier from another
/// account is refused*, in the library suite through the production control router (the
/// owner-control integration test `credential_identifiers_cannot_escape_the_signed_account_even_for_the_same_owner`
/// covers it end to end). Accounts X (paid) and Y (free) have the same owner. A
/// revocation and a rotation for X naming Y's credential C, each with a valid owner
/// proof for exactly that request, are refused. No credential row of either account
/// changes, neither account view changes, and C still authenticates for Y. As a control,
/// the same rotation addressed to Y succeeds. 404 `not_found` is a current
/// implementation value (design decision 4).
#[tokio::test]
async fn an_identifier_of_another_account_of_the_same_owner_is_refused() {
    use axum::http::StatusCode;
    let routes = CredentialRoutes::new(1000, 2);
    let x = routes.id();
    let now = protocol::now().unwrap();
    let free = {
        let mut payload = quote(0, true).payload;
        payload.issued_at = now;
        payload.expires_at = now + 300;
        protocol::sign(protocol::QUOTE_DOMAIN, payload, PROVIDER_KEY).unwrap()
    };
    routes.ledger().store_quote(&free, now).unwrap();
    let c = routes.ledger().open_account(&free, None, now).unwrap().credential.unwrap();
    let y = free.payload.id.clone();
    let views = || [encode(&routes.ledger().account(&x).unwrap()).unwrap(), encode(&routes.ledger().account(&y).unwrap()).unwrap()];
    let accounts = views();
    assert_eq!(routes.ledger().account(&x).unwrap().consumer_wallet, routes.ledger().account(&y).unwrap().consumer_wallet);
    let rows = routes.credential_rows();
    assert_eq!(rows.len(), 2);
    let c_authenticates = || {
        let context = routes.ledger().authenticate(&c.bearer, protocol::now().unwrap()).unwrap();
        assert_eq!((context.account.contract_id.as_str(), context.credential_id.as_str()), (y.as_str(), c.id.as_str()));
    };
    let options = serde_json::to_vec(&json!({"expires_at": now + 3600, "spend_cap_wei": null, "max_concurrency": 1, "scopes": ["infer", "read"]})).unwrap();
    for (method, path, body) in [
        ("DELETE", format!("/v1/contracts/{x}/credentials/{}", c.id), Vec::new()),
        ("POST", format!("/v1/contracts/{x}/credentials/{}/rotate", c.id), options.clone()),
    ] {
        let (status, document, nonce) = routes.owner(method, &path, body).await;
        assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::NOT_FOUND, Some("not_found")), "{method}: {}", document["error"]);
        assert!(routes.ledger().challenge(&nonce).is_err(), "{method}: the proof was not consumed");
        assert_eq!(routes.credential_rows(), rows, "{method}: a credential row changed");
        assert_eq!(views(), accounts, "{method}");
        c_authenticates();
        routes.authenticates(routes.opened.credential.as_ref().unwrap());
    }
    let (status, document, _) = routes.owner("POST", &format!("/v1/contracts/{y}/credentials/{}/rotate", c.id), options).await;
    assert_eq!(status, StatusCode::OK, "{}", document["error"]);
    assert_eq!(document["contract_id"], y);
    assert!(routes.ledger().authenticate(&c.bearer, protocol::now().unwrap()).is_err());
    routes.authenticates(routes.opened.credential.as_ref().unwrap());
    assert_eq!(views(), accounts);
}

/// account-credentials requirement 6 (task 3.6), scenario *A draining provider still
/// manages credentials*, through the production control router. The provider is not
/// accepting, new-funding readiness is lost, no model is qualified, both quote budgets
/// are exhausted (restored rate state, not 600 observed issuances), opening capacity is
/// exhausted (`MAX_PENDING` restored unopened funding intents with open admission) and
/// no offer is registered; the closed gates are first shown to refuse. Owner-proved listing, issuance, rotation and
/// revocation then each succeed, and the account view (with nonzero funded, fee, spent,
/// reserved and available amounts), the signed agreement, the funding and fee records
/// and the exhausted budgets stay exactly as they were.
#[tokio::test]
async fn credential_management_stays_reachable_while_every_admission_gate_is_closed() {
    use axum::http::StatusCode;
    use std::sync::atomic::Ordering;
    let routes = CredentialRoutes::new(1000, 2);
    let id = routes.id();
    let root = routes.opened.credential.clone().unwrap();
    let model = routes.agreement.payload.model.clone();
    let now = protocol::now().unwrap();
    let spent = routes.ledger().reserve(&id, &root.id, "spent", 10, 10, now, &model).unwrap();
    routes.ledger().settle(&spent.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 5 }, "completed", now).unwrap();
    routes.ledger().reserve(&id, &root.id, "held", 10, 10, now, &model).unwrap();
    let named = routes.issue(1, &["read"]);

    routes.state.accepting.store(false, Ordering::Release);
    routes.state.funding_ready.store(false, Ordering::Release);
    routes.state.qualified_models.write().unwrap().clear();
    let minute = now / 60 + 10;
    let db = Connection::open(&routes.path).unwrap();
    for (bucket, watermark) in [("quote:global", "quote_committed_minute"), ("quote-work:global", "quote_work_minute")] {
        db.execute(
            "INSERT INTO rate_limits(bucket,window,count) VALUES(?1,?2,600) ON CONFLICT(bucket) DO UPDATE SET window=excluded.window,count=excluded.count",
            params![bucket, minute],
        )
        .unwrap();
        db.execute(
            "INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![watermark, minute.to_string()],
        )
        .unwrap();
    }
    // Opening capacity: `MAX_PENDING` funding intents whose admission is still open and
    // whose accounts were never opened (restored rows), so no new opening is admitted.
    db.execute_batch("BEGIN").unwrap();
    {
        let mut insert = db
            .prepare("INSERT INTO funding_intents(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) VALUES(?1,?2,'0x','{}','{}','0xpending',?3,?4)")
            .unwrap();
        for index in 0..MAX_PENDING {
            insert
                .execute(params![format!("pending-opening-{index}"), format!("pending-opening-tx-{index}"), index.to_string(), now + 86_400])
                .unwrap();
        }
    }
    db.execute_batch("COMMIT").unwrap();
    let protected = || {
        let rows = |query: &str| -> Vec<String> {
            let mut statement = db.prepare(query).unwrap();
            let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
            rows.collect::<rusqlite::Result<_>>().unwrap()
        };
        json!({
            "account": routes.ledger().account(&id).unwrap(),
            "agreement": routes.ledger().quote(&id).unwrap(),
            "funding": rows("SELECT json_array(tx_hash,contract,json(record),json(evidence),status,checked) FROM funding ORDER BY tx_hash"),
            "intents": rows("SELECT json_array(contract,tx_hash,json(ack),sender,nonce,admission_until) FROM funding_intents WHERE contract NOT GLOB 'pending-opening-*' ORDER BY contract"),
            "opening_capacity": rows("SELECT json_array(COUNT(*),MIN(admission_until),MAX(admission_until)) FROM funding_intents WHERE contract GLOB 'pending-opening-*'"),
            "fees": rows("SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations ORDER BY tx_hash"),
            "batches": rows("SELECT json_array(id,status,json(record)) FROM fee_batches ORDER BY id"),
            "requests": rows("SELECT json_array(id,status,json(record)) FROM requests ORDER BY id"),
            "budgets": rows("SELECT json_array(bucket,window,count) FROM rate_limits WHERE bucket IN ('quote:global','quote-work:global') ORDER BY bucket"),
        })
    };
    let before = protected();
    let account = &before["account"];
    for (member, amount) in [("funded_wei", "1000"), ("fee_wei", "5"), ("spent_wei", "15"), ("reserved_wei", "20"), ("available_wei", "960")] {
        assert_eq!(account[member], amount, "{member}");
    }
    // Each gate is closed.
    let (status, document) = routes.anonymous("POST", "/v1/contracts/propose", b"{}".to_vec()).await;
    assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::SERVICE_UNAVAILABLE, Some("provider_draining")));
    assert!(routes.state.qualified_model(&model).is_none());
    assert!(routes.state.offers.list().is_empty(), "no live or current offer");
    let (status, models) = routes.public("GET", "/v1/models", &root.bearer).await;
    assert_eq!((status, models), (StatusCode::OK, json!({"object": "list", "data": []})));
    assert!(routes.ledger().sign_quote(quote(1000, false).payload, PROVIDER_KEY, now).unwrap_err().starts_with("rate_limit:"));
    // A new opening is refused for capacity even when accepting and its model are
    // assumed: its quote is restored directly because the quote budgets are exhausted.
    let opening = {
        let mut payload = quote(1000, false).payload;
        payload.issued_at = now;
        payload.expires_at = now + 300;
        protocol::sign(protocol::QUOTE_DOMAIN, payload, PROVIDER_KEY).unwrap()
    };
    db.execute(
        "INSERT INTO quotes(id,owner,expires,record) VALUES(?1,?2,?3,?4)",
        params![opening.payload.id, opening.payload.consumer_wallet, opening.payload.expires_at, encode(&opening).unwrap()],
    )
    .unwrap();
    let opening_payment = payment(&opening.payload, true, 1000, 2);
    let error = routes
        .ledger()
        .register_funding_intent(
            &opening,
            &opening.payload.consumer_wallet,
            &opening_payment.raw_transaction,
            &opening_payment.tx_hash,
            2,
            PROVIDER_KEY,
            || Ok(now),
            true,
            &opening.payload.model,
        )
        .unwrap_err();
    assert!(error.starts_with("rate_limit: active funding intent capacity"), "{error}");
    assert_eq!(protected(), before);

    let list = format!("/v1/contracts/{id}/credentials");
    let (status, listed, _) = routes.owner("GET", &list, Vec::new()).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert!(listed.as_array().unwrap().iter().any(|entry| entry["id"] == named.id && entry["revoked"] == false));
    assert_eq!(protected(), before);
    let options = serde_json::to_vec(&json!({"expires_at": now + 3600, "spend_cap_wei": null, "max_concurrency": 1, "scopes": ["infer", "read"]})).unwrap();
    let (status, document, _) = routes.owner("POST", &list, options.clone()).await;
    assert_eq!(status, StatusCode::OK, "issue: {}", document["error"]);
    let issued: Credential = serde_json::from_value(document).unwrap();
    routes.authenticates(&issued);
    assert_eq!(protected(), before);
    let (status, document, _) = routes.owner("POST", &format!("{list}/{}/rotate", named.id), options).await;
    assert_eq!(status, StatusCode::OK, "rotate: {}", document["error"]);
    let replacement: Credential = serde_json::from_value(document).unwrap();
    routes.authenticates(&replacement);
    assert!(routes.ledger().authenticate(&named.bearer, protocol::now().unwrap()).is_err());
    assert_eq!(protected(), before);
    let (status, document, _) = routes.owner("DELETE", &format!("{list}/{}", issued.id), Vec::new()).await;
    assert_eq!((status, document), (StatusCode::OK, json!({"revoked": true})));
    assert!(routes.ledger().authenticate(&issued.bearer, protocol::now().unwrap()).is_err());
    assert_eq!(protected(), before);
    let (status, listed, _) = routes.owner("GET", &list, Vec::new()).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let listed = listed.as_array().unwrap();
    for (credential, revoked) in [(&named.id, true), (&issued.id, true), (&replacement.id, false), (&root.id, false)] {
        assert!(listed.iter().any(|entry| entry["id"] == *credential && entry["revoked"] == revoked), "{credential}");
    }
    assert!(!routes.state.accepting.load(Ordering::Acquire) && !routes.state.funding_ready.load(Ordering::Acquire));
    assert_eq!(protected(), before);
}

// Challenge issuance keeps its own non-regressing minute clock. These fixtures read
// or restore only challenge-rate rows; they never claim observed public issuances.
fn rate_challenge(wallet: &str, nonce: u64, sample: u64) -> AuthPayload {
    AuthPayload {
        network: network(),
        provider_wallet: crate::wallet::address_from_private_key(PROVIDER_KEY).unwrap(),
        wallet: wallet.into(), method: "GET".into(), path: "/v1/models".into(),
        body_hash: protocol::hash_hex(b""), nonce: format!("0x{nonce:064x}"),
        expires_at: sample + crate::auth::CHALLENGE_TTL_SECS,
    }
}

/// Storage consumption releases occupancy so eight pending slots cannot mask 12/13.
fn issue_consumed(store: &BillingStore, wallet: &str, nonce: u64, sample: u64) -> Result<(), String> {
    let challenge = rate_challenge(wallet, nonce, sample);
    store.create_challenge(&challenge, sample)?;
    store.consume_challenge(&challenge.nonce, &challenge, sample).unwrap();
    Ok(())
}

/// The challenge watermark and ledger counter rows, as (window, count).
fn challenge_clock(store: &BillingStore) -> (Option<(u64, u64)>, Option<(u64, u64)>) {
    let db = store.db.lock().unwrap();
    let row = |bucket: &str| db.query_row(
        "SELECT window,count FROM rate_limits WHERE bucket=?1", [bucket],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().unwrap();
    (row("challenge-minute"), row("challenge:global"))
}

fn challenge_wallet_rows(store: &BillingStore) -> Vec<(String, u64, u64)> {
    let db = store.db.lock().unwrap();
    let mut statement = db.prepare(
        "SELECT bucket,window,count FROM rate_limits
         WHERE bucket GLOB 'challenge:*' AND bucket<>'challenge:global' ORDER BY bucket",
    ).unwrap();
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap().collect::<Result<_, _>>().unwrap();
    rows
}

fn challenge_rate_snapshot(store: &BillingStore) -> Vec<Vec<String>> {
    let db = store.db.lock().unwrap();
    [
        "SELECT json_array(key,value) FROM metadata ORDER BY key",
        "SELECT json_array(bucket,window,count) FROM rate_limits ORDER BY bucket",
        "SELECT json_array(nonce,wallet,expires,record) FROM challenges ORDER BY nonce",
        "SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id",
        "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
        "SELECT json_array(id,contract,request_key,credential,status,created,record) FROM requests ORDER BY id",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
    ].into_iter().map(|query| {
        db.prepare(query).unwrap().query_map([], |row| row.get(0)).unwrap()
            .collect::<Result<Vec<String>, _>>().unwrap()
    }).collect()
}

fn quote_at(funding: u128, free: bool, now: u64) -> Signed<Quote> {
    let mut payload = quote(funding, free).payload;
    payload.issued_at = now;
    payload.expires_at = now + 300;
    protocol::sign(protocol::QUOTE_DOMAIN, payload, PROVIDER_KEY).unwrap()
}

fn assert_accepted_state(store: &BillingStore, agreement: &Signed<Quote>, payment: &VerifiedPayment,
    opened: &crate::protocol::OpenResult, now: u64) {
    assert_eq!(encode(&store.account(&agreement.payload.id).unwrap()).unwrap(), encode(&opened.account).unwrap());
    let root = opened.credential.as_ref().unwrap();
    assert_eq!(store.authenticate(&root.bearer, now).unwrap().credential_id, root.id);
    assert_eq!(store.quote(&agreement.payload.id).unwrap().payload, agreement.payload);
    assert!(store.funding_intent(&agreement.payload, &payment.raw_transaction).unwrap().is_some());
}

#[test]
fn challenge_rate_older_and_reordered_samples_spend_only_the_committed_minute() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("challenge-clock.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let owner = format!("0x{:040x}", 0xa1);
    let other = format!("0x{:040x}", 0xb2);
    // Minute 3 commits first. Delayed samples from minutes 0-2 spend its remaining units.
    issue_consumed(&store, &owner, 1, 180).unwrap();
    for (nonce, sample) in (2..=10).zip([120, 0, 179, 60, 119, 1, 150, 59, 178]) {
        issue_consumed(&store, &owner, nonce, sample).unwrap();
        assert_eq!(challenge_clock(&store), (Some((3, nonce)), Some((3, nonce))), "sample {sample}");
    }
    // A delayed sample keeps its own admission-time lifetime, not the logical minute's.
    let mut delayed = rate_challenge(&owner, 11, 60);
    delayed.expires_at = 181;
    assert!(store.create_challenge(&delayed, 60).unwrap_err().starts_with("invalid_challenge:"));
    delayed.expires_at = 121;
    store.create_challenge(&delayed, 60).unwrap();
    assert!(store.consume_challenge(&delayed.nonce, &delayed, 121).is_err(), "rate minute must not extend expiry");
    assert_eq!(challenge_clock(&store), (Some((3, 11)), Some((3, 11))));
    drop(store);

    // Two writers with older samples compete for the owner's twelfth minute-3 unit.
    let candidates = [(rate_challenge(&owner, 12, 60), 60), (rate_challenge(&owner, 13, 120), 120)];
    let writers = [0, 1].map(|_| BillingStore::open(&path, &network()).unwrap());
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = writers.into_iter().zip(&candidates).map(|(ledger, (candidate, sample))| {
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                ledger.create_challenge(candidate, *sample)
            })
        }).collect();
        handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1, "{results:?}");
    let reopened = BillingStore::open(&path, &network()).unwrap();
    assert_eq!(challenge_clock(&reopened), (Some((3, 12)), Some((3, 12))));
    for ((candidate, _), result) in candidates.iter().zip(&results) {
        match result {
            Ok(()) => assert_eq!(reopened.challenge(&candidate.nonce).unwrap().nonce, candidate.nonce),
            Err(error) => {
                assert!(error.starts_with("rate_limit:"), "{error}");
                assert!(reopened.challenge(&candidate.nonce).is_err());
            }
        }
    }
    // Older, equal-minute and reopened samples all observe the exhausted wallet.
    for (nonce, sample) in [(14, 0), (15, 119), (16, 179)] {
        let before = challenge_rate_snapshot(&reopened);
        assert!(reopened.create_challenge(&rate_challenge(&owner, nonce, sample), sample)
            .unwrap_err().starts_with("rate_limit:"), "sample {sample}");
        assert_eq!(challenge_rate_snapshot(&reopened), before);
    }
    // Another wallet's delayed sample spends the shared minute-3 ledger budget.
    issue_consumed(&reopened, &other, 17, 30).unwrap();
    assert_eq!(challenge_clock(&reopened), (Some((3, 13)), Some((3, 13))));
    assert_eq!(challenge_wallet_rows(&reopened),
        vec![(format!("challenge:{owner}"), 3, 12), (format!("challenge:{other}"), 3, 1)]);
    // Only a strictly newer committed minute reopens the wallet, with normal budgets and pruning.
    issue_consumed(&reopened, &owner, 18, 240).unwrap();
    assert_eq!(challenge_clock(&reopened), (Some((4, 1)), Some((4, 1))));
    assert_eq!(challenge_wallet_rows(&reopened), vec![(format!("challenge:{owner}"), 4, 1)]);
    for nonce in 19..=29 { issue_consumed(&reopened, &owner, nonce, 179).unwrap(); }
    assert_eq!(challenge_clock(&reopened), (Some((4, 12)), Some((4, 12))));
    assert!(reopened.create_challenge(&rate_challenge(&owner, 30, 299), 299)
        .unwrap_err().starts_with("rate_limit:"));
}

#[test]
fn challenge_and_quote_clocks_survive_each_others_later_maintenance() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("independent-clocks.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let (agreement, payment, opened) = paid(&store);
    let owner = format!("0x{:040x}", 0xa1);
    // Minute 2: the owner exhausts twelve units and three other wallets spend one each.
    for nonce in 1..=12 { issue_consumed(&store, &owner, nonce, 120).unwrap(); }
    for nonce in 13..=15 { issue_consumed(&store, &format!("0x{nonce:040x}"), nonce, 125).unwrap(); }
    let held = rate_challenge(&format!("0x{:040x}", 0xd4), 16, 130);
    store.create_challenge(&held, 130).unwrap();
    let challenge_rows = |store: &BillingStore| challenge_rate_snapshot(store)[1].iter()
        .filter(|row| row.contains("\"challenge")).cloned().collect::<Vec<_>>();
    let committed = challenge_rows(&store);
    assert_eq!(challenge_clock(&store), (Some((2, 16)), Some((2, 16))));
    assert_eq!(committed.len(), 7);

    // Proposal storage, quote signing work, inference, credential, fee and proof
    // operations all commit later-minute maintenance without touching challenge rates.
    let root = opened.credential.as_ref().unwrap();
    let proposal = quote_at(1000, false, 600);
    store.store_quote(&proposal, 600).unwrap();
    assert!(store.challenge(&held.nonce).is_err(), "later proposal cleanup actually ran");
    assert_eq!(challenge_rows(&store), committed, "later proposal cleanup reset challenge rates");
    assert!(store.sign_quote(proposal.payload.clone(), PROVIDER_KEY, 600).unwrap_err().starts_with("rate_limit:"));
    let held_request = store.reserve(&agreement.payload.id, &root.id, "later-inference", 10, 10, 610,
        &agreement.payload.model).unwrap();
    store.settle(&held_request.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 5 }, "completed", 620).unwrap();
    app_credential(&store, &agreement.payload.id, None, vec!["read".into()]);
    store.prepare_fee_batches(630, true).unwrap();
    assert_eq!(store.recover_requests(640).unwrap(), 0);
    assert_eq!(challenge_rows(&store), committed);
    assert_eq!(committed_quote_rate(&store), (Some("10".into()), 10, 1, 0));
    assert_eq!(quote_work_rate(&store), ("10".into(), 10, 600));
    let settled_account = encode(&store.account(&agreement.payload.id).unwrap()).unwrap();
    assert_ne!(settled_account, encode(&opened.account).unwrap(), "later inference actually settled");
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    for (nonce, sample) in [(20, 60), (21, 150)] {
        assert!(store.create_challenge(&rate_challenge(&owner, nonce, sample), sample)
            .unwrap_err().starts_with("rate_limit:"), "sample {sample}");
    }
    issue_consumed(&store, &format!("0x{:040x}", 0xc3), 22, 150).unwrap();
    assert_eq!(challenge_clock(&store), (Some((2, 17)), Some((2, 17))));

    // A later challenge minute prunes only superseded challenge wallet rows and
    // neither advances nor resets either quote clock.
    issue_consumed(&store, &owner, 23, 900).unwrap();
    assert_eq!(challenge_clock(&store), (Some((15, 1)), Some((15, 1))));
    assert_eq!(challenge_wallet_rows(&store), vec![(format!("challenge:{owner}"), 15, 1)]);
    assert_eq!(committed_quote_rate(&store), (Some("10".into()), 10, 1, 0));
    assert_eq!(quote_work_rate(&store), ("10".into(), 10, 600));
    let older_quote = quote_at(1000, false, 600);
    store.store_quote(&older_quote, 600).unwrap();
    assert_eq!(committed_quote_rate(&store), (Some("10".into()), 10, 2, 0));
    assert!(store.sign_quote(older_quote.payload.clone(), PROVIDER_KEY, 600).unwrap_err().starts_with("rate_limit:"));
    assert_eq!(quote_work_rate(&store), ("10".into(), 10, 600));

    // A later proposal minute likewise leaves the challenge watermark at minute 15.
    store.store_quote(&quote_at(1000, false, 1200), 1200).unwrap();
    assert_eq!(committed_quote_rate(&store), (Some("20".into()), 20, 1, 0));
    assert_eq!(challenge_clock(&store), (Some((15, 1)), Some((15, 1))));
    issue_consumed(&store, &owner, 24, 899).unwrap();
    assert_eq!(challenge_clock(&store), (Some((15, 2)), Some((15, 2))));
    assert_eq!(challenge_wallet_rows(&store), vec![(format!("challenge:{owner}"), 15, 2)]);
    assert_eq!(encode(&store.account(&agreement.payload.id).unwrap()).unwrap(), settled_account);
    assert!(store.funding_intent(&agreement.payload, &payment.raw_transaction).unwrap().is_some());
    assert_eq!(store.authenticate(&root.bearer, 900).unwrap().credential_id, root.id);
}

#[test]
fn failed_future_minute_challenge_rolls_back_watermark_counters_and_cleanup() {
    for failure in ["collision", "insertion", "capacity"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("challenge-rollback.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let (agreement, payment, opened) = paid(&store);
        let owner = format!("0x{:040x}", 0xa1);
        let busy = format!("0x{:040x}", 0xb2);
        let other = format!("0x{:040x}", 0xc3);
        // Committed minute 2: the owner spends eleven units and keeps one proof
        // until 240, `busy` fills its eight pending slots and one proof expires at 130.
        for nonce in 1..=10 { issue_consumed(&store, &owner, nonce, 120).unwrap(); }
        let kept = rate_challenge(&owner, 11, 120);
        store.create_challenge(&kept, 120).unwrap();
        for nonce in 21..=28 { store.create_challenge(&rate_challenge(&busy, nonce, 120), 120).unwrap(); }
        let mut expiring = rate_challenge(&other, 31, 125);
        expiring.expires_at = 130;
        store.create_challenge(&expiring, 125).unwrap();
        assert_eq!(challenge_clock(&store), (Some((2, 20)), Some((2, 20))));
        let (candidate, sample) = match failure {
            // Unexpired at 200, so the insertion collides after cleanup removed `expiring`.
            "collision" => {
                let mut candidate = rate_challenge(&other, 11, 200);
                candidate.method = "DELETE".into();
                (candidate, 200)
            }
            "insertion" => {
                let candidate = rate_challenge(&other, 41, 600);
                store.db.lock().unwrap().execute_batch(&format!(
                    "CREATE TRIGGER refuse_future_challenge AFTER INSERT ON challenges WHEN NEW.nonce='{}'
                     BEGIN SELECT RAISE(ABORT,'injected future challenge failure'); END;", candidate.nonce,
                )).unwrap();
                (candidate, 600)
            }
            _ => (rate_challenge(&busy, 29, 200), 200),
        };
        let before = challenge_rate_snapshot(&store);
        let error = store.create_challenge(&candidate, sample).unwrap_err();
        match failure {
            "collision" => assert!(error.starts_with("ledger database:"), "{error}"),
            "insertion" => assert!(error.contains("injected future challenge failure"), "{error}"),
            _ => assert!(error.starts_with("rate_limit: too many pending challenges"), "{error}"),
        }
        assert_eq!(challenge_rate_snapshot(&store), before, "{failure}");
        drop(store);
        let store = BillingStore::open(&path, &network()).unwrap();
        assert_eq!(challenge_rate_snapshot(&store), before, "{failure}");
        assert_eq!(challenge_clock(&store), (Some((2, 20)), Some((2, 20))), "{failure}");
        // The tentatively cleaned proof is still pending and consumable at its own time.
        store.consume_challenge(&expiring.nonce, &expiring, 129).unwrap();
        // An older sample sees exactly minute 2's remaining allowance: the owner's twelfth unit.
        store.create_challenge(&rate_challenge(&owner, 12, 60), 60).unwrap();
        for (nonce, sample) in [(13, 100), (14, 179)] {
            assert!(store.create_challenge(&rate_challenge(&owner, nonce, sample), sample)
                .unwrap_err().starts_with("rate_limit:"), "{failure} at {sample}");
        }
        assert_eq!(challenge_clock(&store), (Some((2, 21)), Some((2, 21))));
        assert_accepted_state(&store, &agreement, &payment, &opened, 179);
        // Only a successful later admission enters a newer minute.
        store.create_challenge(&rate_challenge(&other, 51, 240), 240).unwrap();
        assert_eq!(challenge_clock(&store), (Some((4, 1)), Some((4, 1))));
        assert_eq!(challenge_wallet_rows(&store), vec![(format!("challenge:{other}"), 4, 1)]);
    }
}

#[test]
fn retained_challenge_counters_are_adopted_without_fresh_allowance() {
    for spent in [599_u64, 600] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("challenge-adoption.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let (agreement, payment, opened) = paid(&store);
        let owner = format!("0x{:040x}", 0xa1);
        let fresh = format!("0x{:040x}", 0xc3);
        // A real pending proof; its lifetime is independent of the restored counters.
        let held = rate_challenge(&format!("0x{:040x}", 0xd4), 1, 180);
        store.create_challenge(&held, 180).unwrap();
        // Retained old-limiter evidence for minute 3 without a watermark: the owner's
        // twelve units, other wallets filling `spent` and one superseded minute-2 row.
        {
            let db = store.db.lock().unwrap();
            db.execute_batch("DELETE FROM rate_limits WHERE bucket GLOB 'challenge:*' OR bucket='challenge-minute'").unwrap();
            db.execute("INSERT INTO rate_limits VALUES('challenge:global',3,?1)", [spent]).unwrap();
            db.execute("INSERT INTO rate_limits VALUES(?1,3,12)", [format!("challenge:{owner}")]).unwrap();
            let mut remaining = spent - 12;
            for index in 0.. {
                if remaining == 0 { break; }
                let units = remaining.min(12);
                db.execute("INSERT INTO rate_limits VALUES(?1,3,?2)",
                    params![format!("challenge:0x{:040x}", 0x1000 + index), units]).unwrap();
                remaining -= units;
            }
            db.execute("INSERT INTO rate_limits VALUES(?1,2,5)", [format!("challenge:0x{:040x}", 0xee)]).unwrap();
        }
        let before = challenge_rate_snapshot(&store);
        // A later-minute candidate tentatively adopts and advances, then collides: nothing is installed.
        assert!(store.create_challenge(&rate_challenge(&fresh, 1, 240), 240)
            .unwrap_err().starts_with("ledger database:"));
        assert_eq!(challenge_rate_snapshot(&store), before, "{spent}");
        // Older and same-minute samples cannot reopen the owner's retained twelve units.
        for sample in [120, 0, 200] {
            assert!(store.create_challenge(&rate_challenge(&owner, 2 + sample, sample), sample)
                .unwrap_err().starts_with("rate_limit:"), "{spent} at {sample}");
            assert_eq!(challenge_rate_snapshot(&store), before, "{spent} at {sample}");
        }
        let last = rate_challenge(&fresh, 500, 60);
        if spent == 599 {
            store.create_challenge(&last, 60).unwrap();
            assert_eq!(challenge_clock(&store), (Some((3, 600)), Some((3, 600))));
            let rows = challenge_wallet_rows(&store);
            assert!(rows.iter().all(|(_, window, _)| *window == 3), "superseded rows are pruned on commit");
            assert!(rows.contains(&(format!("challenge:{owner}"), 3, 12)));
            assert!(rows.contains(&(format!("challenge:{fresh}"), 3, 1)));
        } else {
            assert!(store.create_challenge(&last, 60).unwrap_err().starts_with("rate_limit:"));
            assert_eq!(challenge_rate_snapshot(&store), before);
            assert_eq!(challenge_clock(&store), (None, Some((3, 600))));
        }
        drop(store);
        let store = BillingStore::open(&path, &network()).unwrap();
        // Adoption alone cannot grant the six-hundred-and-first ledger unit.
        for (wallet, nonce, sample) in [(&fresh, 501, 179), (&format!("0x{:040x}", 0xf5), 502, 0)] {
            assert!(store.create_challenge(&rate_challenge(wallet, nonce, sample), sample)
                .unwrap_err().starts_with("rate_limit:"), "{spent} at {sample}");
        }
        // A strictly newer minute starts normal budgets; retained minute-3 rows are pruned.
        store.create_challenge(&rate_challenge(&owner, 600, 240), 240).unwrap();
        assert_eq!(challenge_clock(&store), (Some((4, 1)), Some((4, 1))));
        assert_eq!(challenge_wallet_rows(&store), vec![(format!("challenge:{owner}"), 4, 1)]);
        store.consume_challenge(&held.nonce, &held, 240).unwrap();
        assert_accepted_state(&store, &agreement, &payment, &opened, 240);
    }
}

#[test]
fn superseded_challenge_wallet_rows_are_not_charges_in_the_adopted_minute() {
    let directory = tempfile::tempdir().unwrap();
    let store = BillingStore::open(&directory.path().join("challenge-superseded.sqlite"), &network()).unwrap();
    let current = format!("0x{:040x}", 0xa1);
    let superseded = format!("0x{:040x}", 0xb2);
    // Retained old-limiter rows without a watermark: minute 3 holds two units for `current`,
    // while `superseded` exhausted minute 2, which is no longer the greatest retained minute.
    store.db.lock().unwrap().execute_batch(&format!(
        "INSERT INTO rate_limits VALUES('challenge:global',3,2),('challenge:{current}',3,2),('challenge:{superseded}',2,12);",
    )).unwrap();
    // An older sample lands in adopted minute 3, where `superseded` has spent nothing:
    // its stale minute-2 count neither refuses it nor carries into minute 3.
    issue_consumed(&store, &superseded, 1, 120).unwrap();
    assert_eq!(challenge_clock(&store), (Some((3, 3)), Some((3, 3))));
    assert_eq!(challenge_wallet_rows(&store), vec![
        (format!("challenge:{current}"), 3, 2), (format!("challenge:{superseded}"), 3, 1)]);
    for nonce in 2..=12 { issue_consumed(&store, &superseded, nonce, 179).unwrap(); }
    assert!(store.create_challenge(&rate_challenge(&superseded, 13, 150), 150)
        .unwrap_err().starts_with("rate_limit:"));
    assert_eq!(challenge_clock(&store), (Some((3, 14)), Some((3, 14))));
}

#[test]
fn inconsistent_challenge_rate_evidence_fails_closed_without_erasing_state() {
    let a = format!("0x{:040x}", 0xa1);
    let b = format!("0x{:040x}", 0xb2);
    let c = format!("0x{:040x}", 0xc3);
    let fresh = format!("0x{:040x}", 0xf5);
    let installed = "bucket IN ('challenge:global','challenge-minute')";
    let mutations = [
        // An installed watermark disagreeing with, or lacking, the ledger counter.
        "DELETE FROM rate_limits WHERE bucket='challenge:global'".to_string(),
        // The watermark alone: its counters were erased, so it must not fall back to a fresh baseline.
        "DELETE FROM rate_limits WHERE bucket GLOB 'challenge:*'".into(),
        "UPDATE rate_limits SET window=4 WHERE bucket='challenge-minute'".into(),
        "UPDATE rate_limits SET window=6 WHERE bucket='challenge-minute'".into(),
        "UPDATE rate_limits SET count=2 WHERE bucket='challenge-minute'".into(),
        "UPDATE rate_limits SET window='invalid' WHERE bucket='challenge-minute'".into(),
        "UPDATE rate_limits SET count=-1 WHERE bucket='challenge-minute'".into(),
        // Ledger units out of range, unreadable or disagreeing with current wallet units.
        format!("UPDATE rate_limits SET count=2 WHERE {installed}"),
        format!("UPDATE rate_limits SET count=4 WHERE {installed}"),
        format!("UPDATE rate_limits SET count=0 WHERE {installed}"),
        format!("UPDATE rate_limits SET count=601 WHERE {installed}"),
        // 601 ledger units that their 52 wallet rows sum to exactly: over budget, not merely mismatched.
        format!("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<50)
             INSERT INTO rate_limits SELECT printf('challenge:0x%040x',4096+i),5,CASE WHEN i=50 THEN 10 ELSE 12 END FROM n;
             UPDATE rate_limits SET count=601 WHERE {installed}"),
        format!("UPDATE rate_limits SET count='invalid' WHERE {installed}"),
        format!("UPDATE rate_limits SET window=-1 WHERE {installed}"),
        format!("UPDATE rate_limits SET window=1.5 WHERE {installed}"),
        "UPDATE rate_limits SET window=9223372036854775807 WHERE bucket GLOB 'challenge:*' OR bucket='challenge-minute'".into(),
        // Wallet units without ledger evidence, ahead of the minute, out of range or unreadable.
        format!("DELETE FROM rate_limits WHERE {installed}"),
        format!("INSERT INTO rate_limits VALUES('challenge:{c}',6,1)"),
        format!("UPDATE rate_limits SET count=13 WHERE bucket='challenge:{a}'; UPDATE rate_limits SET count=14 WHERE {installed}"),
        format!("UPDATE rate_limits SET count=0 WHERE bucket='challenge:{b}'; UPDATE rate_limits SET count=2 WHERE {installed}"),
        format!("UPDATE rate_limits SET count=-1 WHERE bucket='challenge:{b}'"),
        format!("UPDATE rate_limits SET count='invalid' WHERE bucket='challenge:{b}'"),
        format!("UPDATE rate_limits SET window=-1 WHERE bucket='challenge:{b}'"),
        format!("UPDATE rate_limits SET window='invalid' WHERE bucket='challenge:{b}'"),
        // Retained evidence before watermark adoption.
        format!("DELETE FROM rate_limits WHERE bucket='challenge-minute'; INSERT INTO rate_limits VALUES('challenge:{c}',6,1)"),
        "DELETE FROM rate_limits WHERE bucket='challenge-minute'; UPDATE rate_limits SET window=4 WHERE bucket='challenge:global'".into(),
        "DELETE FROM rate_limits WHERE bucket='challenge-minute'; UPDATE rate_limits SET count=4 WHERE bucket='challenge:global'".into(),
        format!("DELETE FROM rate_limits WHERE bucket='challenge-minute'; DELETE FROM rate_limits WHERE bucket IN ('challenge:{a}','challenge:{b}')"),
        format!("DELETE FROM rate_limits WHERE bucket='challenge-minute'; UPDATE rate_limits SET count=13 WHERE bucket='challenge:{a}'; UPDATE rate_limits SET count=14 WHERE bucket='challenge:global'"),
        "DELETE FROM rate_limits WHERE bucket='challenge-minute'; UPDATE rate_limits SET count=601 WHERE bucket='challenge:global'".into(),
    ];
    for mutation in mutations {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("invalid-challenge-rate.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let (agreement, payment, opened) = paid(&store);
        // Committed minute 5: `a` spends two units and holds one proof, `b` spends one.
        issue_consumed(&store, &a, 1, 300).unwrap();
        let held = rate_challenge(&a, 2, 300);
        store.create_challenge(&held, 300).unwrap();
        issue_consumed(&store, &b, 3, 300).unwrap();
        assert_eq!(challenge_clock(&store), (Some((5, 3)), Some((5, 3))));
        store.db.lock().unwrap().execute_batch(&mutation).unwrap();
        let before = challenge_rate_snapshot(&store);
        drop(store);
        let store = BillingStore::open(&path, &network()).unwrap();
        // Neither the same minute nor a strictly newer one invents allowance or repairs evidence.
        for (nonce, sample) in [(10, 300), (11, 900)] {
            let error = store.create_challenge(&rate_challenge(&fresh, nonce, sample), sample).unwrap_err();
            assert!(error.starts_with("ledger "), "{mutation} at {sample}: {error}");
            assert_eq!(challenge_rate_snapshot(&store), before, "{mutation}");
        }
        // Existing proofs and accepted state survive; nothing was deleted.
        store.consume_challenge(&held.nonce, &held, 301).unwrap();
        assert_accepted_state(&store, &agreement, &payment, &opened, 301);
        // Explicit restoration of consistent evidence, not automatic repair, reopens issuance.
        store.db.lock().unwrap().execute_batch(&format!(
            "DELETE FROM rate_limits WHERE bucket GLOB 'challenge:*' OR bucket='challenge-minute';
             INSERT INTO rate_limits VALUES('challenge:global',5,1),('challenge-minute',5,1),('challenge:{a}',5,1);",
        )).unwrap();
        issue_consumed(&store, &fresh, 12, 900).unwrap();
        assert_eq!(challenge_clock(&store), (Some((15, 1)), Some((15, 1))), "{mutation}");
    }
}

// inference-request-accounting task 2.2.4: a credential that expires while one of its
// requests holds prepaid credit. Ledger steps take explicit logical operation times; no
// stored timestamp is rewritten and nothing waits for a clock.

type ExpiryRows = std::collections::BTreeMap<&'static str, Vec<Value>>;

/// Every row of every ledger table, read through an independent connection, with JSON
/// record columns decoded so that expected financial changes can be applied field by
/// field. Fails if the ledger has a table this list does not read.
fn expiry_ledger_rows(path: &Path) -> ExpiryRows {
    const TABLES: [(&str, &str); 14] = [
        ("metadata", "SELECT json_array(key,value) FROM metadata ORDER BY key"),
        ("rate_limits", "SELECT json_array(bucket,window,count) FROM rate_limits ORDER BY bucket"),
        ("quotes", "SELECT json_array(id,owner,expires,json(record)) FROM quotes ORDER BY id"),
        ("challenges", "SELECT json_array(nonce,wallet,expires,json(record)) FROM challenges ORDER BY nonce"),
        ("accounts", "SELECT json_array(id,owner,json(quote),json(account),sequence,created) FROM accounts ORDER BY id"),
        ("credentials", "SELECT json_array(id,contract,token_hash,json(record),revoked) FROM credentials ORDER BY id"),
        ("requests", "SELECT json_array(id,contract,request_key,credential,status,created,json(record)) FROM requests ORDER BY id"),
        ("funding", "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding ORDER BY tx_hash"),
        ("funding_attempts", "SELECT json_array(tx_hash,contract,record,raw_transaction,updated) FROM funding_attempts ORDER BY tx_hash"),
        ("funding_intents", "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract"),
        ("fee_obligations", "SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations ORDER BY tx_hash"),
        ("fee_batches", "SELECT json_array(id,status,record) FROM fee_batches ORDER BY id"),
        ("performance_observations", "SELECT json_array(sequence,request,model,evidence,miss) FROM performance_observations ORDER BY sequence"),
        ("performance_withdrawals", "SELECT json_array(model,evidence) FROM performance_withdrawals ORDER BY model,evidence"),
    ];
    let db = Connection::open(path).unwrap();
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut read: Vec<String> = TABLES.iter().map(|(table, _)| table.to_string()).collect();
    read.sort();
    assert_eq!(tables, read, "the snapshot must read every ledger table");
    TABLES
        .iter()
        .map(|(table, query)| {
            let rows = db
                .prepare(query)
                .unwrap()
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
                .collect();
            (*table, rows)
        })
        .collect()
}

/// The row of `table` whose first column is `id`.
fn expiry_row<'a>(rows: &'a mut ExpiryRows, table: &str, id: &str) -> &'a mut Value {
    rows.get_mut(table)
        .unwrap()
        .iter_mut()
        .find(|row| row[0] == id)
        .unwrap_or_else(|| panic!("no {table} row {id}"))
}

fn expiry_shift(amount: &mut Value, delta: i128) {
    let current: i128 = amount.as_str().unwrap().parse().unwrap();
    *amount = json!((current + delta).to_string());
}

/// Applies to `expected` the terminal accounting of the reserved request `request`: its
/// whole original hold leaves the request's own account and credential, `cost` is spent
/// and the rest returns to available credit. `known` usage is recorded with its cost; a
/// release records zero usage and charge and the reservation-sized exposure. Amounts come
/// from the caller's fixed literals, not from the production cost calculation.
fn expect_terminal(
    expected: &mut ExpiryRows,
    request: &str,
    status: &str,
    known: Option<(u64, u64, i128)>,
    settled_at: u64,
) {
    let row = expiry_row(expected, "requests", request);
    assert_eq!(row[4], "reserved", "only a reserved request settles");
    let (prompt, completion, cost) = known.unwrap_or((0, 0, 0));
    let record = &mut row[6];
    let account = record["contract_id"].as_str().unwrap().to_string();
    let credential = record["credential_id"].as_str().unwrap().to_string();
    let maximum: i128 = record["max_cost_wei"].as_str().unwrap().parse().unwrap();
    record["status"] = json!(status);
    record["usage"] = json!({"prompt_tokens": prompt, "completion_tokens": completion});
    record["cost_wei"] = json!(cost.to_string());
    record["provider_loss_wei"] = json!(if known.is_some() { 0 } else { maximum }.to_string());
    record["settled_at"] = json!(settled_at);
    row[4] = json!(status);
    let account = &mut expiry_row(expected, "accounts", &account)[3];
    expiry_shift(&mut account["reserved_wei"], -maximum);
    expiry_shift(&mut account["available_wei"], maximum - cost);
    expiry_shift(&mut account["spent_wei"], cost);
    for (field, tokens) in [("prompt_tokens", prompt), ("completion_tokens", completion)] {
        account["usage"][field] = json!(account["usage"][field].as_u64().unwrap() + tokens);
    }
    let credential = &mut expiry_row(expected, "credentials", &credential)[3];
    expiry_shift(&mut credential["reserved_wei"], -maximum);
    expiry_shift(&mut credential["spent_wei"], cost);
}

fn expiry_credential(store: &BillingStore, account: &str, credential: &str) -> CredentialView {
    store.credentials(account).unwrap().into_iter().find(|entry| entry.id == credential).unwrap()
}

/// inference-request-accounting *Reservation atomically binds authority limits and
/// credit* (task 2.2.4): the credential must be unexpired at the provider-selected
/// operation time, and account-credentials defines `expires_at` as the time from which
/// it no longer authenticates. With `expires_at` E = 1,000, a reservation at E - 1 is
/// admitted; the identical reservation under a new key at exactly E, at E + 1 and 90
/// days later is refused `forbidden` and the bearer no longer authenticates. Each refusal
/// leaves every ledger row unchanged, also after reopening. The credential still has a
/// free concurrency slot and no cap, and the account's live root admits the refused key
/// and bounds at exactly E, so expiry alone refused it.
#[test]
fn credential_expiry_admits_through_the_last_second_and_refuses_new_holds_from_expires_at() {
    const E: u64 = 1000;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, opened) = paid(&store);
    let id = q.payload.id.clone();
    let model = q.payload.model.clone();
    let expiring = store
        .issue_credential(
            &id,
            &CredentialIssueRequest {
                expires_at: E,
                spend_cap_wei: None,
                max_concurrency: 2,
                scopes: vec!["infer".into(), "read".into()],
            },
            102,
        )
        .unwrap();
    assert_eq!(store.authenticate(&expiring.bearer, E - 1).unwrap().credential_id, expiring.id);
    let admitted = store.reserve(&id, &expiring.id, "last-second", 10, 20, E - 1, &model).unwrap();
    assert_eq!(admitted.max_cost_wei, "30");
    let held = store.request_view(&id, "last-second").unwrap();
    assert_eq!(
        (held.status.as_str(), held.credential_id.as_str(), held.created_at),
        ("reserved", expiring.id.as_str(), E - 1)
    );
    let entry = expiry_credential(&store, &id, &expiring.id);
    assert_eq!((entry.reserved_wei.as_str(), entry.revoked, entry.expires_at), ("30", false, E));
    let account = store.account(&id).unwrap();
    assert_eq!((account.available_wei.as_str(), account.reserved_wei.as_str()), ("965", "30"));
    let before = expiry_ledger_rows(&path);
    for reopened in [false, true] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        for now in [E, E + 1, E + CREDENTIAL_TTL_MAX] {
            let error = store.authenticate(&expiring.bearer, now).unwrap_err();
            assert!(error.starts_with("unauthorized:"), "at {now}: {error}");
            let error = store.reserve(&id, &expiring.id, "from-expiry", 10, 20, now, &model).unwrap_err();
            assert!(error.starts_with("forbidden:"), "at {now}: {error}");
            assert!(store.request_view(&id, "from-expiry").unwrap_err().starts_with("not_found:"));
            assert_eq!(expiry_ledger_rows(&path), before, "reopened {reopened}, refused at {now}");
        }
    }
    let root = opened.credential.unwrap();
    let control = store.reserve(&id, &root.id, "from-expiry", 10, 20, E, &model).unwrap();
    assert_eq!(control.max_cost_wei, "30");
    assert_eq!(store.request_view(&id, "from-expiry").unwrap().credential_id, root.id);
    let account = store.account(&id).unwrap();
    assert_eq!((account.available_wei.as_str(), account.reserved_wei.as_str()), ("935", "60"));
    assert_eq!(expiry_credential(&store, &id, &expiring.id).reserved_wei, "30");
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ExpiredHoldOutcome {
    Known,
    Unknown,
    InvalidUsage,
    Restart,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ExpiredHoldSuccessor {
    Issued,
    Revoked,
    Rotated,
}

/// inference-request-accounting scenario *Revocation during an admitted request*, for
/// expiry (task 2.2.4). Credential C (`expires_at` E = 1,000, cap 50 wei, three wei
/// already spent) reserves 30 wei for client key `expiring-key` at E - 1. The account's
/// root keeps a prior two-wei charge and an unrelated 10-wei hold, and a free account
/// already used the same client key. For each terminal outcome (known usage, explicit
/// unknown usage, usage over the reservation, restart recovery), each successor (a new
/// credential issued for the account; C explicitly revoked, then a new credential issued;
/// or C explicitly rotated) and settlement at exactly E or a day later:
///
/// - at E, C no longer authenticates and its new reservations, also with the held key,
///   are refused with every ledger row unchanged;
/// - the successor S starts with no hold or spend; revocation or rotation marks C revoked
///   without moving its hold or spend; S's reservations with the held key are `duplicate_request`
///   with every row unchanged, also after reopening between expiry and settlement;
/// - settlement after expiry charges or releases exactly C's original hold against C and
///   its account (complete expected rows derived from the fixed literals), leaves S, the
///   root's hold and the other account untouched (restart recovery also releases the
///   root's hold, as it releases every reserved request), and keeps the account's funding
///   loss field unchanged;
/// - later different terminal calls, before and after reopening, return the first result
///   without changing a row; neither S nor C can reserve the key again;
/// - the ledger request/list/receipt reads return the settled request attributed to C, a
///   receipt advances only the receipt sequence, and S remains a live, separate authority
///   whose own one-wei hold is its own.
///
/// Who may perform those reads is the routers' check; see
/// `expired_credential_requests_stay_readable_and_unduplicable_through_the_routers`.
#[test]
fn expired_credential_hold_settles_once_against_its_original_accounting() {
    for outcome in [
        ExpiredHoldOutcome::Known,
        ExpiredHoldOutcome::Unknown,
        ExpiredHoldOutcome::InvalidUsage,
        ExpiredHoldOutcome::Restart,
    ] {
        for successor in [ExpiredHoldSuccessor::Issued, ExpiredHoldSuccessor::Revoked, ExpiredHoldSuccessor::Rotated] {
            for settle_at in [1000, 1000 + 86_400] {
                expired_hold_case(outcome, successor, settle_at);
            }
        }
    }
}

fn expired_hold_case(outcome: ExpiredHoldOutcome, successor_kind: ExpiredHoldSuccessor, settle_at: u64) {
    const E: u64 = 1000;
    let case = format!("{outcome:?}, {successor_kind:?} successor, settled at {settle_at}");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, opened) = paid(&store);
    let id = q.payload.id.clone();
    let model = q.payload.model.clone();
    let root = opened.credential.unwrap();
    let prior = store.reserve(&id, &root.id, "root-prior", 1, 1, 103, &model).unwrap();
    store.settle(&prior.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", 104).unwrap();
    let root_held = store.reserve(&id, &root.id, "root-held", 5, 5, 105, &model).unwrap();
    let free = quote(0, true);
    store.store_quote(&free, 105).unwrap();
    let other = store.open_account(&free, None, 106).unwrap();
    let other_request = store
        .reserve(&free.payload.id, &other.credential.unwrap().id, "expiring-key", 10, 20, 106, &free.payload.model)
        .unwrap();
    store.settle(&other_request.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 5 }, "completed", 106).unwrap();
    let options = |expires_at: u64| CredentialIssueRequest {
        expires_at,
        spend_cap_wei: Some("50".into()),
        max_concurrency: 2,
        scopes: vec!["infer".into(), "read".into()],
    };
    let expiring = store.issue_credential(&id, &options(E), 107).unwrap();
    let spent = store.reserve(&id, &expiring.id, "expiring-prior", 2, 3, 108, &model).unwrap();
    store.settle(&spent.id, &TokenUsage { prompt_tokens: 2, completion_tokens: 1 }, "completed", 109).unwrap();

    assert_eq!(store.authenticate(&expiring.bearer, E - 1).unwrap().credential_id, expiring.id);
    let target = store.reserve(&id, &expiring.id, "expiring-key", 10, 20, E - 1, &model).unwrap();
    assert_eq!(target.max_cost_wei, "30", "{case}");
    let account = store.account(&id).unwrap();
    assert_eq!(
        (
            account.funded_wei.as_str(), account.fee_wei.as_str(), account.available_wei.as_str(),
            account.reserved_wei.as_str(), account.spent_wei.as_str(), account.usage.prompt_tokens,
            account.usage.completion_tokens, account.provider_loss_wei.as_str(),
        ),
        ("1000", "5", "950", "40", "5", 3, 2, "0"),
        "{case}"
    );
    let entry = expiry_credential(&store, &id, &expiring.id);
    assert_eq!((entry.reserved_wei.as_str(), entry.spent_wei.as_str()), ("30", "3"), "{case}");
    let admitted = expiry_ledger_rows(&path);

    // (a) From E, C authorizes no new reservation, not even under the held key.
    assert!(store.authenticate(&expiring.bearer, E).unwrap_err().starts_with("unauthorized:"), "{case}");
    for (key, prompt, output) in [("after-expiry", 1, 1), ("expiring-key", 10, 20)] {
        let error = store.reserve(&id, &expiring.id, key, prompt, output, E, &model).unwrap_err();
        assert!(error.starts_with("forbidden:"), "{case}: {key}: {error}");
    }
    assert!(store.request_view(&id, "after-expiry").is_err(), "{case}");
    assert_eq!(expiry_ledger_rows(&path), admitted, "{case}: a refusal changed the ledger");

    // A successor S for the same account after expiry: issued, issued after C's explicit
    // revocation, or rotated from C.
    let replaced_at = if settle_at == E { E } else { E + 1 };
    let successor = match successor_kind {
        ExpiredHoldSuccessor::Issued => store.issue_credential(&id, &options(replaced_at + CREDENTIAL_TTL_MAX), replaced_at),
        ExpiredHoldSuccessor::Revoked => {
            store.revoke_credential(&id, &expiring.id).unwrap();
            store.issue_credential(&id, &options(replaced_at + CREDENTIAL_TTL_MAX), replaced_at)
        }
        ExpiredHoldSuccessor::Rotated => {
            store.rotate_credential(&id, &expiring.id, &options(replaced_at + CREDENTIAL_TTL_MAX), replaced_at)
        }
    }
    .unwrap();
    let replaced = expiry_ledger_rows(&path);
    let mut expected = admitted.clone();
    let revoked = successor_kind != ExpiredHoldSuccessor::Issued;
    if revoked {
        let row = expiry_row(&mut expected, "credentials", &expiring.id);
        row[3]["revoked"] = json!(true);
        row[4] = json!(1);
    }
    let successor_row = replaced["credentials"].iter().find(|row| row[0] == successor.id).unwrap().clone();
    assert_eq!(
        (&successor_row[1], &successor_row[3], &successor_row[4]),
        (
            &json!(id),
            &json!({
                "id": successor.id, "contract_id": id, "expires_at": replaced_at + CREDENTIAL_TTL_MAX,
                "spend_cap_wei": "50", "spent_wei": "0", "reserved_wei": "0", "max_concurrency": 2,
                "scopes": ["infer", "read"], "revoked": false, "created_at": replaced_at, "root": false,
            }),
            &json!(0),
        ),
        "{case}"
    );
    let credentials = expected.get_mut("credentials").unwrap();
    credentials.push(successor_row);
    credentials.sort_by(|a, b| a[0].as_str().cmp(&b[0].as_str()));
    assert_eq!(replaced, expected, "{case}: the successor moved a hold or spend");

    // (c) S cannot take over or duplicate the held request.
    assert_eq!(store.authenticate(&successor.bearer, replaced_at).unwrap().credential_id, successor.id);
    for (prompt, output) in [(10, 20), (1, 1)] {
        let error = store.reserve(&id, &successor.id, "expiring-key", prompt, output, replaced_at, &model).unwrap_err();
        assert!(error.starts_with("duplicate_request:"), "{case}: {error}");
    }
    assert_eq!(expiry_ledger_rows(&path), replaced, "{case}");

    // (d) Reopening between expiry and settlement.
    drop(store);
    store = BillingStore::open(&path, &network()).unwrap();
    assert_eq!(expiry_ledger_rows(&path), replaced, "{case}: reopening changed the ledger");
    assert!(store.authenticate(&expiring.bearer, settle_at).is_err(), "{case}");
    let error = store.reserve(&id, &expiring.id, "after-expiry", 1, 1, settle_at, &model).unwrap_err();
    assert!(error.starts_with("forbidden:"), "{case}: {error}");
    let error = store.reserve(&id, &successor.id, "expiring-key", 10, 20, settle_at, &model).unwrap_err();
    assert!(error.starts_with("duplicate_request:"), "{case}: {error}");
    assert_eq!(expiry_ledger_rows(&path), replaced, "{case}");

    // (b) Settlement after expiry, against C's original accounting.
    let mut expected = replaced.clone();
    let result = match outcome {
        ExpiredHoldOutcome::Known => {
            expect_terminal(&mut expected, &target.id, "completed", Some((10, 7, 17)), settle_at);
            store.settle(&target.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 7 }, "completed", settle_at)
                .unwrap()
        }
        ExpiredHoldOutcome::Unknown => {
            expect_terminal(&mut expected, &target.id, "cancelled_unknown_usage", None, settle_at);
            store.settle(&target.id, &TokenUsage::default(), "cancelled_unknown_usage", settle_at).unwrap()
        }
        ExpiredHoldOutcome::InvalidUsage => {
            expect_terminal(&mut expected, &target.id, "invalid_usage_unknown_usage", None, settle_at);
            let error = store
                .settle(&target.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 21 }, "completed", settle_at)
                .unwrap_err();
            assert!(error.starts_with("usage_over_reservation:"), "{case}: {error}");
            store.request_view(&id, &target.id).unwrap()
        }
        ExpiredHoldOutcome::Restart => {
            expect_terminal(&mut expected, &target.id, "restart_unknown_usage", None, settle_at);
            expect_terminal(&mut expected, &root_held.id, "restart_unknown_usage", None, settle_at);
            assert_eq!(store.recover_requests(settle_at).unwrap(), 2, "{case}");
            store.request_view(&id, &target.id).unwrap()
        }
    };
    let record = expected["requests"].iter().find(|row| row[0] == target.id).unwrap()[6].clone();
    assert_eq!(serde_json::to_value(&result).unwrap(), record, "{case}");
    assert_eq!(result.credential_id, expiring.id, "{case}");
    let settled = expiry_ledger_rows(&path);
    assert_eq!(settled, expected, "{case}: settlement changed more than C's original accounting");
    let known = outcome == ExpiredHoldOutcome::Known;
    let account = store.account(&id).unwrap();
    let (available, reserved, spent, usage) = match outcome {
        ExpiredHoldOutcome::Known => ("963", "10", "22", (13, 9)),
        ExpiredHoldOutcome::Unknown | ExpiredHoldOutcome::InvalidUsage => ("980", "10", "5", (3, 2)),
        ExpiredHoldOutcome::Restart => ("990", "0", "5", (3, 2)),
    };
    assert_eq!(
        (
            account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str(),
            account.usage.prompt_tokens, account.usage.completion_tokens, account.fee_wei.as_str(),
            account.funded_wei.as_str(), account.provider_loss_wei.as_str(),
        ),
        (available, reserved, spent, usage.0, usage.1, "5", "1000", "0"),
        "{case}"
    );
    assert_eq!(
        (result.cost_wei.as_str(), result.provider_loss_wei.as_str()),
        if known { ("17", "0") } else { ("0", "30") },
        "{case}"
    );
    let entry = expiry_credential(&store, &id, &expiring.id);
    assert_eq!(
        (entry.reserved_wei.as_str(), entry.spent_wei.as_str(), entry.revoked),
        ("0", if known { "20" } else { "3" }, revoked),
        "{case}"
    );
    let entry = expiry_credential(&store, &id, &successor.id);
    assert_eq!((entry.reserved_wei.as_str(), entry.spent_wei.as_str()), ("0", "0"), "{case}");

    // The first terminal result wins, before and after reopening.
    for reopened in [false, true] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        for (prompt, completion, status, at) in [
            (10, 20, "completed", settle_at + 10),
            (0, 0, "cancelled_unknown_usage", settle_at + 11),
            (10, 21, "completed", settle_at + 12),
        ] {
            let replay = store
                .settle(&target.id, &TokenUsage { prompt_tokens: prompt, completion_tokens: completion }, status, at)
                .unwrap();
            assert_eq!(serde_json::to_value(&replay).unwrap(), record, "{case}: replay {status}");
        }
        if outcome == ExpiredHoldOutcome::Restart {
            assert_eq!(store.recover_requests(settle_at + 13).unwrap(), 0, "{case}");
        }
        let error = store.reserve(&id, &successor.id, "expiring-key", 10, 20, settle_at + 14, &model).unwrap_err();
        assert!(error.starts_with("duplicate_request:"), "{case}: {error}");
        let error = store.reserve(&id, &expiring.id, "after-expiry", 1, 1, settle_at + 14, &model).unwrap_err();
        assert!(error.starts_with("forbidden:"), "{case}: {error}");
        assert_eq!(expiry_ledger_rows(&path), settled, "{case}: reopened {reopened}");
    }

    // (e) Reads of the settled request attribute it to C.
    for key in ["expiring-key", target.id.as_str()] {
        assert_eq!(serde_json::to_value(store.request_view(&id, key).unwrap()).unwrap(), record, "{case}");
    }
    let listed = store.requests(&id, None).unwrap();
    assert_eq!(listed.iter().filter(|request| request.id == target.id).count(), 1, "{case}");
    assert!(listed.iter().any(|request| serde_json::to_value(request).unwrap() == record), "{case}");
    assert_eq!(store.request_view(&free.payload.id, "expiring-key").unwrap().id, other_request.id, "{case}");
    let receipt = store.receipt(&id, PROVIDER_KEY, settle_at + 20).unwrap();
    protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &q.payload.provider_wallet).unwrap();
    let bound = receipt.payload.requests.iter().find(|request| request.id == target.id).unwrap();
    assert_eq!(serde_json::to_value(bound).unwrap(), record, "{case}");
    let mut expected = settled.clone();
    let account = expiry_row(&mut expected, "accounts", &id);
    assert_eq!(serde_json::to_value(&receipt.payload.account).unwrap(), account[3], "{case}");
    let sequence = account[4].as_u64().unwrap() + 1;
    assert_eq!(receipt.payload.sequence, sequence, "{case}");
    account[4] = json!(sequence);
    assert_eq!(expiry_ledger_rows(&path), expected, "{case}: a receipt changed more than its sequence");

    // S is a live, separate authority: its own hold is its own, and C's record stays.
    let control = store.reserve(&id, &successor.id, "successor-control", 1, 1, settle_at + 30, &model).unwrap();
    assert_eq!(control.max_cost_wei, "2", "{case}");
    assert_eq!(expiry_credential(&store, &id, &successor.id).reserved_wei, "2", "{case}");
    store.settle(&control.id, &TokenUsage::default(), "cancelled_unknown_usage", settle_at + 31).unwrap();
    let after = expiry_ledger_rows(&path);
    let c_row = |rows: &ExpiryRows| rows["credentials"].iter().find(|row| row[0] == expiring.id).unwrap().clone();
    assert_eq!(c_row(&after), c_row(&settled), "{case}");
    let s_entry = expiry_credential(&store, &id, &successor.id);
    assert_eq!((s_entry.reserved_wei.as_str(), s_entry.spent_wei.as_str()), ("0", "0"), "{case}");
    let account = store.account(&id).unwrap();
    assert_eq!(
        (account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str()),
        (available, reserved, spent),
        "{case}"
    );
}

/// One request to the public inference router (`public`) or the control router, with
/// `bearer` and an optional Idempotency-Key. Returns the status, the JSON document and
/// the `x-request-id` header. No response may echo the bearer.
async fn expiry_route(
    routes: &CredentialRoutes,
    public: bool,
    method: &str,
    path: &str,
    bearer: &str,
    key: Option<&str>,
    body: Vec<u8>,
) -> (axum::http::StatusCode, Value, Option<String>) {
    use tower::ServiceExt;
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {bearer}"));
    if let Some(key) = key {
        request = request.header("idempotency-key", key).header("content-type", "application/json");
    }
    let request = request.body(axum::body::Body::from(body)).unwrap();
    let response = if public {
        crate::inference_gateway::router(routes.state.clone()).oneshot(request).await.unwrap()
    } else {
        crate::contracts::router(routes.state.clone()).oneshot(request).await.unwrap()
    };
    let status = response.status();
    let header = response
        .headers()
        .get("x-request-id")
        .map(|value| value.to_str().unwrap().to_string());
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
    assert!(!contains(&bytes, bearer.as_bytes()), "a response echoed the bearer");
    (status, serde_json::from_slice(&bytes).unwrap(), header)
}

/// Task 2.2.4 through the production control and public inference routers, which read
/// the real clock. The account is opened, and credential C (`expires_at` E) issued and
/// used, at explicit logical ledger times long before that clock, so C has necessarily
/// expired for every route call; no stored timestamp is rewritten and nothing waits.
///
/// - C's bearer is refused with 401 for request status, cancel, control request and
///   receipt reads, and for inference with the held key or a new key, with no financial
///   row changed. *Request and receipt history supports attributable recovery* keeps the
///   existing owner/read-bearer authorization, and an expired credential is not that
///   authority, so reads use the owner's proof or a live `read` credential.
/// - The owner's proof reads the held request and rotates C into a live successor S
///   (the account-credentials remedy for an expired credential). Rotation moves neither
///   C's hold nor its spend.
/// - S's inference with the held Idempotency-Key gets 409 `duplicate_request` naming the
///   original request and status, before tokenization or any engine call (the configured
///   engine is unreachable), and adds no request or hold; S's cancel of that key is a
///   read. S's `read` scope and the owner's proof read the original request, attributed
///   to C, and S obtains a signed receipt binding it.
/// - Known settlement at an explicit time after E charges C and its account, not S; the
///   same reads then return the settled request, the duplicate reports `completed`, and
///   C's bearer is still refused.
#[tokio::test]
async fn expired_credential_requests_stay_readable_and_unduplicable_through_the_routers() {
    use axum::http::StatusCode;
    const OPENED_AT: u64 = 1_000_000;
    const E: u64 = OPENED_AT + 1000;
    let routes = CredentialRoutes::opened_at(1000, 2, OPENED_AT);
    let id = routes.id();
    let model = routes.agreement.payload.model.clone();
    let expiring = routes
        .ledger()
        .issue_credential(
            &id,
            &CredentialIssueRequest {
                expires_at: E,
                spend_cap_wei: Some("50".into()),
                max_concurrency: 1,
                scopes: vec!["infer".into(), "read".into()],
            },
            OPENED_AT + 1,
        )
        .unwrap();
    let held = routes.ledger().reserve(&id, &expiring.id, "route-held", 10, 20, E - 1, &model).unwrap();
    assert_eq!(held.max_cost_wei, "30");
    assert!(protocol::now().unwrap() >= E, "the routers' real clock must be after the logical expiry");
    // Owner proofs legitimately spend challenge evidence and receipts advance only the
    // receipt sequence (checked separately); every other row is compared.
    let financial = || {
        let mut rows = expiry_ledger_rows(&routes.path);
        rows.retain(|table, _| !matches!(*table, "metadata" | "rate_limits" | "challenges"));
        for account in rows.get_mut("accounts").unwrap() {
            account[4] = Value::Null;
        }
        rows
    };
    let sequence = || -> u64 {
        Connection::open(&routes.path)
            .unwrap()
            .query_row("SELECT sequence FROM accounts WHERE id=?1", [&id], |row| row.get(0))
            .unwrap()
    };
    let reserved = serde_json::to_value(routes.ledger().request_view(&id, &held.id).unwrap()).unwrap();
    assert_eq!((reserved["status"].as_str(), reserved["credential_id"].as_str()), (Some("reserved"), Some(expiring.id.as_str())));
    let chat = serde_json::to_vec(&json!({
        "model": model.id, "messages": [{"role": "user", "content": "hello"}], "max_tokens": 20,
    }))
    .unwrap();
    let status_path = "/v1/requests/route-held";
    let cancel_path = "/v1/requests/route-held/cancel";
    let control_path = format!("/v1/contracts/{id}/requests/{}", held.id);
    let receipt_path = format!("/v1/contracts/{id}/receipt");

    let refuse_expired = |stage: &'static str| {
        let (routes, expiring, financial, chat) = (&routes, &expiring, &financial, &chat);
        let (control_path, receipt_path) = (&control_path, &receipt_path);
        async move {
            let before = financial();
            for (public, method, path, key) in [
                (true, "GET", status_path, None),
                (true, "POST", cancel_path, None),
                (true, "POST", "/v1/chat/completions", Some("route-held")),
                (true, "POST", "/v1/chat/completions", Some("route-new")),
                (false, "GET", control_path.as_str(), None),
                (false, "GET", receipt_path.as_str(), None),
            ] {
                let body = if key.is_some() { chat.clone() } else { Vec::new() };
                let (status, document, _) = expiry_route(routes, public, method, path, &expiring.bearer, key, body).await;
                let code = if public { "invalid_api_key" } else { "unauthorized" };
                assert_eq!(
                    (status, document["error"]["code"].as_str()),
                    (StatusCode::UNAUTHORIZED, Some(code)),
                    "{stage}: expired bearer {method} {path}: {document}"
                );
            }
            assert!(routes.ledger().request_view(&routes.id(), "route-new").is_err(), "{stage}");
            assert_eq!(financial(), before, "{stage}: an expired bearer changed the ledger");
        }
    };
    refuse_expired("held").await;
    let admitted = financial();

    let (status, document, _) = routes.owner("GET", &control_path, Vec::new()).await;
    assert_eq!((status, &document), (StatusCode::OK, &reserved));

    // The owner rotates C into a live successor S at the routers' clock.
    let now = protocol::now().unwrap();
    let options = json!({"expires_at": now + 3600, "spend_cap_wei": "50", "max_concurrency": 1, "scopes": ["infer", "read"]});
    let rotate = format!("/v1/contracts/{id}/credentials/{}/rotate", expiring.id);
    let (status, document, _) = routes.owner("POST", &rotate, serde_json::to_vec(&options).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{}", document["error"]);
    let successor: Credential = serde_json::from_value(document).unwrap();
    routes.authenticates(&successor);
    let rotated = financial();
    let mut expected = admitted.clone();
    let row = expiry_row(&mut expected, "credentials", &expiring.id);
    row[3]["revoked"] = json!(true);
    row[4] = json!(1);
    let successor_row = rotated["credentials"].iter().find(|row| row[0] == successor.id).unwrap().clone();
    assert_eq!(
        (successor_row[3]["reserved_wei"].as_str(), successor_row[3]["spent_wei"].as_str()),
        (Some("0"), Some("0"))
    );
    let credentials = expected.get_mut("credentials").unwrap();
    credentials.push(successor_row);
    credentials.sort_by(|a, b| a[0].as_str().cmp(&b[0].as_str()));
    assert_eq!(rotated, expected, "rotation moved a hold or spend");
    let c_entry = routes.entry(&expiring.id);
    assert_eq!((c_entry.reserved_wei.as_str(), c_entry.spent_wei.as_str(), c_entry.revoked), ("30", "0", true));

    // S neither duplicates nor takes over the held request; its reads attribute it to C.
    let successor_reads = |request: Value, request_status: &'static str| {
        let (routes, successor, held, financial, sequence, chat) = (&routes, &successor, &held, &financial, &sequence, &chat);
        let (control_path, receipt_path) = (&control_path, &receipt_path);
        async move {
            let before = financial();
            let (status, document, header) = expiry_route(
                routes, true, "POST", "/v1/chat/completions", &successor.bearer, Some("route-held"), chat.clone(),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT, "{document}");
            assert_eq!(
                (
                    document["error"]["code"].as_str(),
                    document["error"]["request_id"].as_str(),
                    document["error"]["request_status"].as_str(),
                    header.as_deref(),
                ),
                (Some("duplicate_request"), Some(held.id.as_str()), Some(request_status), Some(held.id.as_str()))
            );
            for (public, method, path) in [
                (true, "GET", status_path),
                (true, "POST", cancel_path),
                (false, "GET", control_path.as_str()),
            ] {
                let (status, document, _) = expiry_route(routes, public, method, path, &successor.bearer, None, Vec::new()).await;
                assert_eq!((status, &document), (StatusCode::OK, &request), "successor {method} {path}");
            }
            let (status, document, _) = routes.owner("GET", control_path, Vec::new()).await;
            assert_eq!((status, &document), (StatusCode::OK, &request), "owner read");
            // Both the successor's `read` scope and the owner's proof obtain a signed
            // receipt binding the original request.
            for reader in ["successor", "owner"] {
                let sequence_before = sequence();
                let (status, document) = if reader == "owner" {
                    let (status, document, _) = routes.owner("GET", receipt_path, Vec::new()).await;
                    (status, document)
                } else {
                    let (status, document, _) =
                        expiry_route(routes, false, "GET", receipt_path, &successor.bearer, None, Vec::new()).await;
                    (status, document)
                };
                assert_eq!(status, StatusCode::OK, "{reader} receipt: {document}");
                let receipt: Signed<UsageReceipt> = serde_json::from_value(document).unwrap();
                protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &routes.state.wallet).unwrap();
                assert_eq!(receipt.payload.sequence, sequence_before + 1, "{reader} receipt");
                assert_eq!(sequence(), sequence_before + 1, "{reader} receipt");
                let bound = receipt.payload.requests.iter().find(|bound| bound.id == held.id).unwrap();
                assert_eq!(serde_json::to_value(bound).unwrap(), request, "{reader} receipt");
                assert_eq!(
                    serde_json::to_value(&receipt.payload.account).unwrap(),
                    serde_json::to_value(routes.ledger().account(&routes.id()).unwrap()).unwrap(),
                    "{reader} receipt"
                );
            }
            assert_eq!(financial(), before, "successor reads or duplicates changed the ledger");
        }
    };
    successor_reads(reserved.clone(), "reserved").await;
    assert_eq!(financial(), rotated);

    // Known settlement at an explicit time after E, against C's original accounting.
    let settled = routes
        .ledger()
        .settle(&held.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 7 }, "completed", E + 5)
        .unwrap();
    let mut expected = rotated.clone();
    expect_terminal(&mut expected, &held.id, "completed", Some((10, 7, 17)), E + 5);
    let record = expected["requests"].iter().find(|row| row[0] == held.id).unwrap()[6].clone();
    assert_eq!(serde_json::to_value(&settled).unwrap(), record);
    assert_eq!(financial(), expected);
    let account = routes.ledger().account(&id).unwrap();
    assert_eq!(
        (account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str()),
        ("978", "0", "17")
    );
    let c_entry = routes.entry(&expiring.id);
    assert_eq!((c_entry.reserved_wei.as_str(), c_entry.spent_wei.as_str()), ("0", "17"));
    let s_entry = routes.entry(&successor.id);
    assert_eq!((s_entry.reserved_wei.as_str(), s_entry.spent_wei.as_str()), ("0", "0"));
    successor_reads(record, "completed").await;
    refuse_expired("settled").await;
    assert_eq!(financial(), expected);
}

// inference-request-accounting task 2.5: the remaining ledger edges recorded by 2.1.
// Every step passes an explicit logical operation time through the public ledger
// arguments; no stored timestamp is rewritten, nothing waits for a clock, and no signed
// time or quote/challenge clock stands in for the inference admission sample.

/// The refusal of either inference admission budget (*Inference abuse budgets remain
/// independent*), distinct from the quote, challenge and free-account rate limits.
const INFERENCE_BUDGET_REFUSAL: &str = "rate_limit: inference admission limit reached";

/// How `budget_records` ends each request it creates.
#[derive(Clone, Copy, Debug)]
enum BudgetEnd {
    /// Known (1, 0) usage under this terminal status.
    Known(&'static str),
    /// An explicit release under this unknown-usage status.
    Unknown(&'static str),
    /// Known usage with one output token over its zero-token bound, which commits
    /// `invalid_usage_unknown_usage`.
    Invalid,
}

/// Creates `count` request records `{prefix}-{n}` for `credential` at logical time `at`.
/// Each reserves one prepared input token and no output (a one-wei hold on the paid
/// fixture, none on a free account) and ends at the same time as `end` says.
fn budget_records(
    store: &BillingStore,
    account: &str,
    credential: &str,
    prefix: &str,
    count: usize,
    at: u64,
    end: BudgetEnd,
) {
    let model = store.account(account).unwrap().model;
    for n in 0..count {
        let key = format!("{prefix}-{n}");
        let held = store
            .reserve(account, credential, &key, 1, 0, at, &model)
            .unwrap_or_else(|error| panic!("{key} at {at}: {error}"));
        let (status, view) = match end {
            BudgetEnd::Known(status) => {
                let usage = TokenUsage { prompt_tokens: 1, completion_tokens: 0 };
                (status, store.settle(&held.id, &usage, status, at).unwrap())
            }
            BudgetEnd::Unknown(status) => (status, store.settle(&held.id, &TokenUsage::default(), status, at).unwrap()),
            BudgetEnd::Invalid => {
                let usage = TokenUsage { prompt_tokens: 1, completion_tokens: 1 };
                let error = store.settle(&held.id, &usage, "completed", at).unwrap_err();
                assert!(error.starts_with("usage_over_reservation:"), "{key}: {error}");
                ("invalid_usage_unknown_usage", store.request_view(account, &held.id).unwrap())
            }
        };
        assert_eq!((view.status.as_str(), view.created_at, view.settled_at), (status, at, Some(at)), "{key}");
    }
}

/// The fixture's own count of retained request records, from every row read through an
/// independent connection: records of `credential` created at or after `credential_from`,
/// and `*_unknown_usage` records of `account` created at or after `unknown_from`. It states
/// a fixture precondition; admission outcomes are the evidence.
fn budget_counts(path: &Path, account: &str, credential: &str, credential_from: u64, unknown_from: u64) -> (usize, usize) {
    let rows = expiry_ledger_rows(path);
    let requests = &rows["requests"];
    let created = |row: &Value| row[5].as_u64().unwrap();
    (
        requests.iter().filter(|row| row[3] == credential && created(row) >= credential_from).count(),
        requests
            .iter()
            .filter(|row| {
                row[1] == account
                    && row[4].as_str().unwrap().ends_with("_unknown_usage")
                    && created(row) >= unknown_from
            })
            .count(),
    )
}

/// A fresh one-token reservation by `credential` under `key` at `now` is refused by an
/// inference admission budget, leaves every ledger row unchanged and does not occupy `key`.
fn assert_budget_refused(store: &BillingStore, path: &Path, account: &str, credential: &str, key: &str, now: u64, case: &str) {
    let before = expiry_ledger_rows(path);
    let model = store.account(account).unwrap().model;
    let error = store.reserve(account, credential, key, 1, 0, now, &model).unwrap_err();
    assert_eq!(error, INFERENCE_BUDGET_REFUSAL, "{case}: {key} at {now}");
    assert!(store.request_view(account, key).unwrap_err().starts_with("not_found:"), "{case}: {key}");
    assert_eq!(expiry_ledger_rows(path), before, "{case}: the refusal of {key} at {now} changed the ledger");
}

/// inference-request-accounting scenario *Prompt or output mismatch* and requirement
/// *Unknown usage releases the hold without inventing execution* (task 2.5): authoritative
/// input usage must equal the exact prepared count. Credential C (100-wei cap) reserves 30
/// wei for 10 prepared input and 20 maximum output tokens, beside a prior two-wei root
/// charge and an unrelated 10-wei root hold. Known usage that changes only the input count
/// (one fewer (9, 5), one more (11, 5) or none (0, 0)) would cost 14, 16 or zero wei, all
/// within the hold. Each instead commits one `invalid_usage_unknown_usage` release: zero
/// charge and recorded usage, the whole 30 wei back to available credit and off both
/// holds, 30 wei of request exposure, and unchanged spend, cumulative usage and account
/// funding-loss field. The invalid-usage error is returned after that commit. Later valid,
/// over-bound, unknown and repeated mismatched calls, before and after reopening, return
/// the first result without changing a row, and the key stays occupied for C and the root.
/// A control with the same bounds and the exact prepared count is charged its 15 wei.
#[test]
fn prompt_count_mismatch_commits_one_invalid_usage_release_of_the_whole_hold() {
    for (case, prompt, completion) in [("one fewer", 9, 5), ("one more", 11, 5), ("none", 0, 0)] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite");
        let mut store = BillingStore::open(&path, &network()).unwrap();
        let (q, _, opened) = paid(&store);
        let id = q.payload.id.clone();
        let model = q.payload.model.clone();
        let root = opened.credential.unwrap();
        let prior = store.reserve(&id, &root.id, "prior", 1, 1, 110, &model).unwrap();
        store.settle(&prior.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", 111).unwrap();
        store.reserve(&id, &root.id, "unrelated", 5, 5, 112, &model).unwrap();
        let c = app_credential(&store, &id, Some("100"), vec!["infer".into(), "read".into()]);
        let target = store.reserve(&id, &c.id, "target", 10, 20, 120, &model).unwrap();
        assert_eq!(target.max_cost_wei, "30", "{case}");

        let mut expected = expiry_ledger_rows(&path);
        expect_terminal(&mut expected, &target.id, "invalid_usage_unknown_usage", None, 130);
        let error = store
            .settle(&target.id, &TokenUsage { prompt_tokens: prompt, completion_tokens: completion }, "completed", 130)
            .unwrap_err();
        assert!(error.starts_with("usage_over_reservation:"), "{case}: {error}");
        let settled = expiry_ledger_rows(&path);
        assert_eq!(settled, expected, "{case}: the mismatch did not commit exactly one whole release");
        let record = expected["requests"].iter().find(|row| row[0] == target.id).unwrap()[6].clone();
        let view = store.request_view(&id, "target").unwrap();
        assert_eq!(serde_json::to_value(&view).unwrap(), record, "{case}");
        assert_eq!(
            (
                view.status.as_str(), view.usage.prompt_tokens, view.usage.completion_tokens, view.cost_wei.as_str(),
                view.provider_loss_wei.as_str(), view.prompt_tokens_reserved, view.max_output_tokens, view.settled_at,
            ),
            ("invalid_usage_unknown_usage", 0, 0, "0", "30", 10, 20, Some(130)),
            "{case}"
        );
        let account = store.account(&id).unwrap();
        assert_eq!(
            (
                account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str(),
                account.usage.prompt_tokens, account.usage.completion_tokens, account.provider_loss_wei.as_str(),
                account.fee_wei.as_str(), account.funded_wei.as_str(),
            ),
            ("983", "10", "2", 1, 1, "0", "5", "1000"),
            "{case}"
        );
        let entry = expiry_credential(&store, &id, &c.id);
        assert_eq!((entry.reserved_wei.as_str(), entry.spent_wei.as_str()), ("0", "0"), "{case}");

        for reopened in [false, true] {
            if reopened {
                drop(store);
                store = BillingStore::open(&path, &network()).unwrap();
            }
            for (late_prompt, late_completion, status) in [
                (10, 5, "completed"),
                (10, 20, "completed"),
                (10, 21, "completed"),
                (prompt, completion, "completed"),
                (0, 0, "cancelled_unknown_usage"),
            ] {
                let late = TokenUsage { prompt_tokens: late_prompt, completion_tokens: late_completion };
                let replay = store.settle(&target.id, &late, status, 140).unwrap();
                assert_eq!(
                    serde_json::to_value(&replay).unwrap(),
                    record,
                    "{case}: late {status} ({late_prompt}, {late_completion})"
                );
            }
            for credential in [&c.id, &root.id] {
                let error = store.reserve(&id, credential, "target", 10, 20, 141, &model).unwrap_err();
                assert!(error.starts_with("duplicate_request:"), "{case}: {error}");
            }
            assert_eq!(expiry_ledger_rows(&path), settled, "{case}: reopened {reopened}");
        }

        // Control: the same bounds with the exact prepared count are known, chargeable usage.
        let control = store.reserve(&id, &c.id, "control", 10, 20, 150, &model).unwrap();
        let mut expected = expiry_ledger_rows(&path);
        expect_terminal(&mut expected, &control.id, "completed", Some((10, 5, 15)), 151);
        let charged = store
            .settle(&control.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 5 }, "completed", 151)
            .unwrap();
        assert_eq!((charged.status.as_str(), charged.cost_wei.as_str()), ("completed", "15"), "{case}");
        assert_eq!(expiry_ledger_rows(&path), expected, "{case}: control");
    }
}

/// A paid agreement whose signed model admits any u64 token count (context and output
/// limits u64::MAX) at the fixture's one wei per token, credited with `funding` wei through
/// an injected post-verification payment `tag` (a ledger input, not chain evidence).
fn wide_account(store: &BillingStore, funding: u128, tag: u8) -> (Signed<Quote>, Credential) {
    let mut payload = quote(funding, false).payload;
    payload.model.context_length = u64::MAX;
    payload.model.max_output_tokens = u64::MAX;
    let agreement = protocol::sign(protocol::QUOTE_DOMAIN, payload, PROVIDER_KEY).unwrap();
    store.store_quote(&agreement, 100).unwrap();
    let funding_payment = payment(&agreement.payload, true, funding, tag);
    register(store, &agreement, &funding_payment, 100);
    let opened = store.open_account(&agreement, Some(&funding_payment), 101).unwrap();
    (agreement, opened.credential.unwrap())
}

/// inference-request-accounting requirement *Unknown usage releases the hold without
/// inventing execution* (task 2.5): the account's cumulative prompt and completion usage
/// counters are u64 and must not overflow. Credentials carry spend and hold amounts but no
/// token counters. Two known root requests bring both counters to u64::MAX - 100 under a
/// model whose signed limits admit any u64 count, and an unrelated seven-wei root hold
/// stays reserved. Credential C then reserves and settles, in order: input usage 101 with
/// three output tokens (only the input counter would pass u64::MAX), seven input with 101
/// output (only the output counter), 101 of each, exactly 100 of each (both counters reach
/// u64::MAX), then one input token and one output token past it. Each over-maximum usage
/// matches its prepared input and output bound, yet commits one `invalid_usage_unknown_usage`
/// release: its whole hold returns with equal request exposure and zero charge, neither
/// counter changes (no partial update and no wraparound), and the invalid-usage error is
/// returned after the commit. Reaching exactly u64::MAX is charged as known usage. Complete
/// expected rows come from fixed literals. Later calls, before and after reopening, return
/// each first result and the keys stay occupied. A second account with zero counters settles
/// each refused shape as known usage, so the cumulative counters alone caused the release.
#[test]
fn cumulative_usage_overflow_commits_an_invalid_usage_release_without_wrapping() {
    const FUNDING: u128 = 100_000_000_000_000_000_000;
    const FEE: u128 = FUNDING / 200;
    const NEAR: u64 = u64::MAX - 100;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let (agreement, root) = wide_account(&store, FUNDING, 1);
    let (control, control_root) = wide_account(&store, FUNDING, 2);
    let id = agreement.payload.id.clone();
    let model = agreement.payload.model.clone();
    for (key, prompt, output) in [("seed-input", NEAR, 0), ("seed-output", 0, NEAR)] {
        let seed = store.reserve(&id, &root.id, key, prompt, output, 110, &model).unwrap();
        assert_eq!(seed.max_cost_wei, NEAR.to_string(), "{key}");
        let usage = TokenUsage { prompt_tokens: prompt, completion_tokens: output };
        assert_eq!(store.settle(&seed.id, &usage, "completed", 111).unwrap().cost_wei, NEAR.to_string(), "{key}");
    }
    let unrelated = store.reserve(&id, &root.id, "unrelated", 3, 4, 112, &model).unwrap();
    assert_eq!(unrelated.max_cost_wei, "7");
    let c = app_credential(&store, &id, None, vec!["infer".into(), "read".into()]);
    let seeded = 2 * u128::from(NEAR);
    let account = store.account(&id).unwrap();
    assert_eq!(
        (
            account.usage.prompt_tokens, account.usage.completion_tokens, account.fee_wei.clone(),
            account.spent_wei.clone(), account.reserved_wei.clone(), account.available_wei.clone(),
        ),
        (NEAR, NEAR, FEE.to_string(), seeded.to_string(), "7".to_string(), (FUNDING - FEE - seeded - 7).to_string())
    );

    // (key, prepared input, maximum output, supplied usage, hold, charge when valid, counters after)
    let steps: [(&str, u64, u64, (u64, u64), u128, Option<u128>, (u64, u64)); 6] = [
        ("input-over", 101, 10, (101, 3), 111, None, (NEAR, NEAR)),
        ("output-over", 7, 101, (7, 101), 108, None, (NEAR, NEAR)),
        ("both-over", 101, 101, (101, 101), 202, None, (NEAR, NEAR)),
        ("exact-maximum", 100, 100, (100, 100), 200, Some(200), (u64::MAX, u64::MAX)),
        ("input-past-maximum", 1, 0, (1, 0), 1, None, (u64::MAX, u64::MAX)),
        ("output-past-maximum", 0, 1, (0, 1), 1, None, (u64::MAX, u64::MAX)),
    ];
    let mut results = Vec::new();
    for (step, (key, prompt, output, usage, hold, charge, counters)) in steps.into_iter().enumerate() {
        let at = 120 + 10 * step as u64;
        let held = store.reserve(&id, &c.id, key, prompt, output, at, &model).unwrap();
        assert_eq!(held.max_cost_wei, hold.to_string(), "{key}");
        let supplied = TokenUsage { prompt_tokens: usage.0, completion_tokens: usage.1 };
        let mut expected = expiry_ledger_rows(&path);
        let result = match charge {
            Some(cost) => {
                expect_terminal(&mut expected, &held.id, "completed", Some((usage.0, usage.1, cost as i128)), at + 1);
                store.settle(&held.id, &supplied, "completed", at + 1).unwrap()
            }
            None => {
                expect_terminal(&mut expected, &held.id, "invalid_usage_unknown_usage", None, at + 1);
                let error = store.settle(&held.id, &supplied, "completed", at + 1).unwrap_err();
                assert!(error.starts_with("usage_over_reservation:"), "{key}: {error}");
                store.request_view(&id, key).unwrap()
            }
        };
        assert_eq!(expiry_ledger_rows(&path), expected, "{key}: settlement changed more than its own accounting");
        let record = expected["requests"].iter().find(|row| row[0] == held.id).unwrap()[6].clone();
        assert_eq!(serde_json::to_value(&result).unwrap(), record, "{key}");
        let (cost, exposure) = match charge {
            Some(cost) => (cost.to_string(), "0".to_string()),
            None => ("0".to_string(), hold.to_string()),
        };
        assert_eq!((result.cost_wei.clone(), result.provider_loss_wei.clone()), (cost, exposure), "{key}");
        let account = store.account(&id).unwrap();
        assert_eq!((account.usage.prompt_tokens, account.usage.completion_tokens), counters, "{key}");
        assert_eq!(account.provider_loss_wei, "0", "{key}");
        results.push((key, held.id, record, supplied));
    }
    let account = store.account(&id).unwrap();
    assert_eq!(
        (
            account.spent_wei.clone(), account.reserved_wei.clone(), account.available_wei.clone(),
            account.fee_wei.clone(), account.funded_wei.clone(),
        ),
        (
            (seeded + 200).to_string(), "7".to_string(), (FUNDING - FEE - seeded - 200 - 7).to_string(),
            FEE.to_string(), FUNDING.to_string(),
        )
    );
    let entry = expiry_credential(&store, &id, &c.id);
    assert_eq!((entry.reserved_wei.as_str(), entry.spent_wei.as_str()), ("0", "200"));

    let finished = expiry_ledger_rows(&path);
    for reopened in [false, true] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        for (key, request, record, supplied) in &results {
            for (late, status) in [
                (supplied.clone(), "completed"),
                (TokenUsage::default(), "completed"),
                (TokenUsage::default(), "cancelled_unknown_usage"),
            ] {
                let replay = store.settle(request, &late, status, 300).unwrap();
                assert_eq!(serde_json::to_value(&replay).unwrap(), *record, "{key}: late {status}");
            }
            let error = store.reserve(&id, &c.id, key, 1, 0, 301, &model).unwrap_err();
            assert!(error.starts_with("duplicate_request:"), "{key}: {error}");
        }
        assert_eq!(expiry_ledger_rows(&path), finished, "reopened {reopened}");
    }

    // Control: with zero cumulative usage, every released shape is valid known usage.
    let control_model = control.payload.model.clone();
    for (key, prompt, output, usage, hold, charge, _) in steps {
        if charge.is_some() {
            continue;
        }
        let held = store.reserve(&control.payload.id, &control_root.id, key, prompt, output, 400, &control_model).unwrap();
        assert_eq!(held.max_cost_wei, hold.to_string(), "control {key}");
        let supplied = TokenUsage { prompt_tokens: usage.0, completion_tokens: usage.1 };
        let settled = store.settle(&held.id, &supplied, "completed", 401).unwrap();
        assert_eq!(
            (settled.status.as_str(), settled.cost_wei.clone()),
            ("completed", (usage.0 + usage.1).to_string()),
            "control {key}"
        );
    }
    assert_eq!(expiry_ledger_rows(&path)["accounts"].iter().find(|row| row[0] == id), finished["accounts"].iter().find(|row| row[0] == id));
}

/// inference-request-accounting requirement *Inference abuse budgets remain independent*
/// (task 2.5), credential budget: a reservation is refused when its credential already has
/// at least 120 request records created at or after max(0, now - 60). With T = 1,000,
/// credential C has one known record at T - 61, one created at T - 61 but settled only at
/// T, one at exactly T - 60, and in (T - 60, T] 90 known records, a request still held
/// since T - 30, 10 explicit unknown releases, five invalid-usage releases and 12 known
/// `failed` records: 119 counted records. At T the 120th reservation is admitted and the
/// 121st refused with every ledger row unchanged, also after reopening; so the record at
/// T - 60 counts, and neither record created at T - 61 does, although one of them was
/// settled inside the window: the window uses creation time, not settlement time.
/// Settling the held request and the 120th (terminal status changes) deletes no identity
/// and restores no allowance. The root credential of the same account is admitted at T: the
/// budget is per credential. At T + 1 the record created at T - 60 leaves the window and
/// exactly one more reservation is admitted.
#[test]
fn credential_inference_budget_admits_the_120th_record_from_an_inclusive_60_second_cutoff() {
    const T: u64 = 1_000;
    let case = "credential budget";
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, opened) = paid(&store);
    let id = q.payload.id.clone();
    let model = q.payload.model.clone();
    let root = opened.credential.unwrap();
    let options = CredentialIssueRequest {
        expires_at: 100_000,
        spend_cap_wei: None,
        max_concurrency: 3,
        scopes: vec!["infer".into(), "read".into()],
    };
    let c = store.issue_credential(&id, &options, 102).unwrap();
    budget_records(&store, &id, &c.id, "outside", 1, T - 61, BudgetEnd::Known("completed"));
    let late = store.reserve(&id, &c.id, "outside-settled-late", 1, 0, T - 61, &model).unwrap();
    budget_records(&store, &id, &c.id, "edge", 1, T - 60, BudgetEnd::Known("completed"));
    budget_records(&store, &id, &c.id, "known", 90, T - 59, BudgetEnd::Known("completed"));
    let held = store.reserve(&id, &c.id, "held", 10, 20, T - 30, &model).unwrap();
    budget_records(&store, &id, &c.id, "cancelled", 10, T - 20, BudgetEnd::Unknown("cancelled_unknown_usage"));
    budget_records(&store, &id, &c.id, "invalid", 5, T - 10, BudgetEnd::Invalid);
    budget_records(&store, &id, &c.id, "failed", 12, T, BudgetEnd::Known("failed"));
    let late = store.settle(&late.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 0 }, "completed", T).unwrap();
    assert_eq!((late.created_at, late.settled_at), (T - 61, Some(T)));
    assert_eq!(budget_counts(&path, &id, &c.id, T - 60, 0), (119, 15));
    assert_eq!(budget_counts(&path, &id, &c.id, T - 61, 0).0, 121);

    let admitted = store.reserve(&id, &c.id, "admitted-120th", 1, 0, T, &model).unwrap();
    for reopened in [false, true] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        assert_budget_refused(&store, &path, &id, &c.id, "refused-121st", T, case);
    }
    // Terminal status changes keep every counted identity, so no allowance returns.
    store.settle(&held.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 7 }, "completed", T).unwrap();
    store.settle(&admitted.id, &TokenUsage::default(), "cancelled_unknown_usage", T).unwrap();
    assert_eq!(budget_counts(&path, &id, &c.id, T - 60, 0), (120, 16));
    assert_budget_refused(&store, &path, &id, &c.id, "refused-121st", T, case);
    // Per credential: the account's root still reserves at T.
    let control = store.reserve(&id, &root.id, "root-control", 1, 0, T, &model).unwrap();
    store.settle(&control.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 0 }, "completed", T).unwrap();
    // At T + 1 the record created at T - 60 is older than the window: exactly one more.
    store.reserve(&id, &c.id, "admitted-after-edge", 1, 0, T + 1, &model).unwrap();
    assert_budget_refused(&store, &path, &id, &c.id, "refused-after-edge", T + 1, case);
    let account = store.account(&id).unwrap();
    // 106 one-wei known records (105 of C, the root's control) plus the held request's 17
    // wei; the one-wei hold admitted at T + 1 remains.
    assert_eq!(
        (account.spent_wei.as_str(), account.reserved_wei.as_str(), account.available_wei.as_str()),
        ("123", "1", "871")
    );
}

/// inference-request-accounting requirement *Inference abuse budgets remain independent*
/// (task 2.5): the credential cutoff max(0, now - 60) saturates near time zero instead of
/// wrapping or dropping the window. A free account opened at logical time 0 (zero-price
/// requests keep their admission accounting) has one record at 0 and 118 at 30. At 30
/// (cutoff 0) the 120th is admitted and the 121st refused; at 60 (cutoff exactly 0) the
/// record at 0 still counts; at 61 it does not, so exactly one more is admitted.
#[test]
fn credential_inference_budget_cutoff_saturates_at_time_zero() {
    let case = "near time zero";
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let agreement = quote_at(0, true, 0);
    store.store_quote(&agreement, 0).unwrap();
    let opened = store.open_account(&agreement, None, 0).unwrap();
    let id = agreement.payload.id.clone();
    let model = agreement.payload.model.clone();
    let root = opened.credential.unwrap();
    budget_records(&store, &id, &root.id, "zero", 1, 0, BudgetEnd::Known("completed"));
    budget_records(&store, &id, &root.id, "thirty", 118, 30, BudgetEnd::Known("completed"));
    assert_eq!(budget_counts(&path, &id, &root.id, 0, 0), (119, 0));
    let admitted = store.reserve(&id, &root.id, "admitted-120th", 1, 0, 30, &model).unwrap();
    assert_eq!(admitted.max_cost_wei, "0");
    assert_budget_refused(&store, &path, &id, &root.id, "refused", 30, case);
    assert_budget_refused(&store, &path, &id, &root.id, "refused", 60, case);
    store.reserve(&id, &root.id, "admitted-at-61", 1, 0, 61, &model).unwrap();
    assert_budget_refused(&store, &path, &id, &root.id, "refused", 61, case);
}

/// inference-request-accounting requirement *Inference abuse budgets remain independent*
/// (task 2.5): the account cutoff max(0, now - 3600) also saturates near time zero rather
/// than falling back to `now` or dropping the window. A free account opened at logical time
/// 0 has one explicit unknown-usage release created at 0 and 28 at 1,800. At 1,800 (cutoff
/// 0) the 30th is admitted and, once released as unknown, the account is refused; at 3,600
/// (cutoff exactly 0) the release created at 0 still counts; at 3,601 it does not, so
/// exactly one more is admitted before its own release refuses the account again. Every
/// refusal leaves all rows unchanged. The root's 60-second credential budget stays far
/// below 120 throughout, so each refusal is the account's unknown-usage budget.
#[test]
fn account_unknown_usage_budget_cutoff_saturates_at_time_zero() {
    let case = "unknown-usage budget near time zero";
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let store = BillingStore::open(&path, &network()).unwrap();
    let agreement = quote_at(0, true, 0);
    store.store_quote(&agreement, 0).unwrap();
    let opened = store.open_account(&agreement, None, 0).unwrap();
    let id = agreement.payload.id.clone();
    let root = opened.credential.unwrap();
    let unknown = BudgetEnd::Unknown("cancelled_unknown_usage");
    budget_records(&store, &id, &root.id, "zero", 1, 0, unknown);
    budget_records(&store, &id, &root.id, "half-hour", 28, 1_800, unknown);
    assert_eq!(budget_counts(&path, &id, &root.id, 0, 0), (29, 29));
    assert_eq!(budget_counts(&path, &id, &root.id, 1_740, 1), (28, 28));
    budget_records(&store, &id, &root.id, "admitted-30th", 1, 1_800, BudgetEnd::Unknown("failed_unknown_usage"));
    assert_budget_refused(&store, &path, &id, &root.id, "refused", 1_800, case);
    assert_budget_refused(&store, &path, &id, &root.id, "refused", 3_600, case);
    budget_records(&store, &id, &root.id, "admitted-at-3601", 1, 3_601, unknown);
    assert_budget_refused(&store, &path, &id, &root.id, "refused", 3_601, case);
}

/// inference-request-accounting requirement *Inference abuse budgets remain independent*
/// (task 2.5), account unknown-usage budget: a reservation is refused when its account
/// already has at least 30 `*_unknown_usage` request records created at or after
/// max(0, now - 3600). With T = 10,000, account A's credentials C1, C2 and root R create: at
/// T - 3601 a request that stays reserved until restart recovery at T - 100 (an unknown
/// record created outside the window though settled inside it); at exactly T - 3600 an
/// explicit unknown release; 42 known records under `completed`, `cancelled`, `failed`,
/// `disconnected`, `timed_out` and `completed_unknownxusage` (a known status the ledger
/// charges, which merely resembles an unknown one); 27 releases across
/// `cancelled_unknown_usage`, `failed_unknown_usage`, `disconnected_unknown_usage`,
/// `timed_out_unknown_usage` and invalid usage; one more restart-recovered request; and a
/// request still held: 29 counted records. At T the 30th is admitted, then released as
/// unknown, and every credential of A is refused with all rows unchanged, also after
/// reopening. A late known result cannot turn that record back into allowance, the held
/// request still settles, and another account is admitted. At T + 1 the record created at
/// T - 3600 leaves the window: one reservation is admitted and settled as known usage, which
/// does not count, so another is admitted; its unknown release refuses the account again.
#[test]
fn account_unknown_usage_budget_counts_only_unknown_records_by_creation_time() {
    const T: u64 = 10_000;
    let case = "unknown-usage budget";
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, opened) = paid(&store);
    let id = q.payload.id.clone();
    let model = q.payload.model.clone();
    let root = opened.credential.unwrap();
    let free = quote(0, true);
    store.store_quote(&free, 105).unwrap();
    let other = store.open_account(&free, None, 106).unwrap().credential.unwrap();
    let options = CredentialIssueRequest {
        expires_at: 100_000,
        spend_cap_wei: None,
        max_concurrency: 3,
        scopes: vec!["infer".into(), "read".into()],
    };
    let c1 = store.issue_credential(&id, &options, 107).unwrap();
    let c2 = store.issue_credential(&id, &options, 107).unwrap();

    let old = store.reserve(&id, &c1.id, "old", 1, 0, T - 3601, &model).unwrap();
    budget_records(&store, &id, &c1.id, "edge", 1, T - 3600, BudgetEnd::Unknown("cancelled_unknown_usage"));
    for status in ["completed", "cancelled", "failed", "disconnected", "timed_out", "completed_unknownxusage"] {
        budget_records(&store, &id, &c2.id, status, 7, T - 3000, BudgetEnd::Known(status));
    }
    for (credential, prefix, count, end) in [
        (&c1.id, "cancelled-unknown", 7, BudgetEnd::Unknown("cancelled_unknown_usage")),
        (&c2.id, "failed-unknown", 6, BudgetEnd::Unknown("failed_unknown_usage")),
        (&root.id, "disconnected-unknown", 6, BudgetEnd::Unknown("disconnected_unknown_usage")),
        (&c1.id, "timed-out-unknown", 4, BudgetEnd::Unknown("timed_out_unknown_usage")),
        (&c2.id, "invalid", 4, BudgetEnd::Invalid),
    ] {
        budget_records(&store, &id, credential, prefix, count, T - 2000, end);
    }
    store.reserve(&id, &root.id, "restart", 1, 0, T - 100, &model).unwrap();
    assert_eq!(store.recover_requests(T - 100).unwrap(), 2);
    let recovered = store.request_view(&id, &old.id).unwrap();
    assert_eq!(
        (recovered.status.as_str(), recovered.created_at, recovered.settled_at),
        ("restart_unknown_usage", T - 3601, Some(T - 100))
    );
    // Until T - 1 the window still holds the recovered request created at T - 3601.
    let held = store.reserve(&id, &c2.id, "held", 10, 20, T, &model).unwrap();
    assert_eq!(budget_counts(&path, &id, &c1.id, 0, T - 3600).1, 29);
    assert_eq!(budget_counts(&path, &id, &c1.id, 0, T - 3601).1, 30);
    assert_eq!(budget_counts(&path, &id, &c1.id, 0, 0).1, 30);

    let thirtieth = store.reserve(&id, &c1.id, "thirtieth", 1, 0, T, &model).unwrap();
    store.settle(&thirtieth.id, &TokenUsage::default(), "failed_unknown_usage", T).unwrap();
    for reopened in [false, true] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        for credential in [&c1.id, &c2.id, &root.id] {
            assert_budget_refused(&store, &path, &id, credential, "refused", T, case);
        }
    }
    // A late known result keeps the first unknown result; the held request still settles.
    let replay = store
        .settle(&thirtieth.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 0 }, "completed", T)
        .unwrap();
    assert_eq!((replay.status.as_str(), replay.cost_wei.as_str()), ("failed_unknown_usage", "0"));
    let settled = store
        .settle(&held.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 7 }, "completed", T)
        .unwrap();
    assert_eq!((settled.status.as_str(), settled.cost_wei.as_str()), ("completed", "17"));
    for credential in [&c1.id, &c2.id, &root.id] {
        assert_budget_refused(&store, &path, &id, credential, "refused", T, case);
    }
    // Per account: another account reserves at T.
    store.reserve(&free.payload.id, &other.id, "refused", 1, 0, T, &free.payload.model).unwrap();
    // At T + 1 the record created at T - 3600 is older than the window.
    let after = store.reserve(&id, &c2.id, "after-edge", 1, 0, T + 1, &model).unwrap();
    store.settle(&after.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 0 }, "completed", T + 1).unwrap();
    let last = store.reserve(&id, &root.id, "after-known", 1, 0, T + 1, &model).unwrap();
    store.settle(&last.id, &TokenUsage::default(), "cancelled_unknown_usage", T + 1).unwrap();
    for credential in [&c1.id, &c2.id, &root.id] {
        assert_budget_refused(&store, &path, &id, credential, "refused-after-edge", T + 1, case);
    }
}

/// inference-request-accounting requirement *Inference abuse budgets remain independent*
/// (task 2.5): `now` is the provider-selected operation-time sample, and retained records
/// created after a regressed sample are not omitted. Credential budget: credential C has
/// 120 known records created at T = 20,000. Unknown-usage budget, in its own ledger: C has
/// 30 explicit unknown releases created at T and the account's root has no record. For
/// samples T, T - 1, T - 500, 0 and the last second of the window (T + 60, respectively
/// T + 3600), before and after reopening, C (respectively the root) is refused with every
/// row unchanged. One second later the records are older than the window and one
/// reservation is admitted.
#[test]
fn inference_budgets_count_future_dated_records_after_clock_regression_and_reopening() {
    const T: u64 = 20_000;
    for unknown in [false, true] {
        let case = if unknown { "unknown-usage budget" } else { "credential budget" };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite");
        let mut store = BillingStore::open(&path, &network()).unwrap();
        let (q, _, opened) = paid(&store);
        let id = q.payload.id.clone();
        let model = q.payload.model.clone();
        let root = opened.credential.unwrap();
        let options = CredentialIssueRequest {
            expires_at: 100_000,
            spend_cap_wei: None,
            max_concurrency: 3,
            scopes: vec!["infer".into(), "read".into()],
        };
        let c = store.issue_credential(&id, &options, 102).unwrap();
        let (subject, last_second) = if unknown {
            budget_records(&store, &id, &c.id, "unknown", 30, T, BudgetEnd::Unknown("cancelled_unknown_usage"));
            assert_eq!(budget_counts(&path, &id, &root.id, 0, T), (0, 30));
            (root.id.clone(), T + 3600)
        } else {
            budget_records(&store, &id, &c.id, "known", 120, T, BudgetEnd::Known("completed"));
            assert_eq!(budget_counts(&path, &id, &c.id, T, 0), (120, 0));
            (c.id.clone(), T + 60)
        };
        for reopened in [false, true] {
            if reopened {
                drop(store);
                store = BillingStore::open(&path, &network()).unwrap();
            }
            for now in [T, T - 1, T - 500, 0, last_second] {
                assert_budget_refused(&store, &path, &id, &subject, "refused", now, case);
            }
        }
        store.reserve(&id, &subject, "admitted", 1, 0, last_second + 1, &model).unwrap();
    }
}

/// inference-request-accounting requirement *Inference abuse budgets remain independent*
/// (task 2.5): each window reaches back from the provider-selected sample itself, not from
/// a non-regressing watermark such as the latest retained creation time. With T = 20,000,
/// credential budget: credential C has 60 known records created at T and then, after the
/// sample regressed, 60 created at T - 100. At T - 100 and at T - 40 (cutoff exactly
/// T - 100) all 120 count and C is refused with every row unchanged, before and after
/// reopening; at T - 39 the 60 records created at T - 100 leave the window and C is
/// admitted. Unknown-usage budget, in its own ledger: C has 15 explicit unknown-usage
/// releases created at T and then 15 at T - 4,000. The account's root, which has no
/// record, is refused at T - 4,000 and at T - 400 (cutoff exactly T - 4,000), before and
/// after reopening, and admitted at T - 399. A window anchored at the latest record (T)
/// would admit at every refused sample.
#[test]
fn inference_budget_windows_reach_back_from_the_regressed_sample_not_the_latest_record() {
    const T: u64 = 20_000;
    for unknown in [false, true] {
        let case = if unknown { "unknown-usage budget" } else { "credential budget" };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite");
        let mut store = BillingStore::open(&path, &network()).unwrap();
        let (q, _, opened) = paid(&store);
        let id = q.payload.id.clone();
        let model = q.payload.model.clone();
        let root = opened.credential.unwrap();
        let options = CredentialIssueRequest {
            expires_at: 100_000,
            spend_cap_wei: None,
            max_concurrency: 3,
            scopes: vec!["infer".into(), "read".into()],
        };
        let c = store.issue_credential(&id, &options, 102).unwrap();
        let (subject, earlier, edge) = if unknown {
            let end = BudgetEnd::Unknown("cancelled_unknown_usage");
            budget_records(&store, &id, &c.id, "later", 15, T, end);
            budget_records(&store, &id, &c.id, "earlier", 15, T - 4_000, end);
            assert_eq!(budget_counts(&path, &id, &root.id, 0, T - 4_000), (0, 30));
            assert_eq!(budget_counts(&path, &id, &root.id, 0, T - 3_999), (0, 15));
            (root.id.clone(), T - 4_000, T - 400)
        } else {
            let end = BudgetEnd::Known("completed");
            budget_records(&store, &id, &c.id, "later", 60, T, end);
            budget_records(&store, &id, &c.id, "earlier", 60, T - 100, end);
            assert_eq!(budget_counts(&path, &id, &c.id, T - 100, 0), (120, 0));
            assert_eq!(budget_counts(&path, &id, &c.id, T - 99, 0), (60, 0));
            (c.id.clone(), T - 100, T - 40)
        };
        for reopened in [false, true] {
            if reopened {
                drop(store);
                store = BillingStore::open(&path, &network()).unwrap();
            }
            for now in [earlier, edge] {
                assert_budget_refused(&store, &path, &id, &subject, "refused", now, case);
            }
        }
        let admitted = store.reserve(&id, &subject, "admitted", 1, 0, edge + 1, &model).unwrap();
        assert_eq!(store.request_view(&id, &admitted.id).unwrap().created_at, edge + 1, "{case}");
    }
}

/// inference-request-accounting scenario *Exhausted inference budget does not strand a
/// hold* (task 2.5). Credential C (five slots) holds four 30-wei requests admitted at
/// T - 10 (T = 5,000). At T - 5 it then reaches both fresh allowances: 86 known one-wei
/// records and 30 explicit unknown releases make 120 records of C in the last 60 seconds
/// and 30 unknown-usage records of the account in the last hour. A pending signed quote and
/// an issued challenge leave quote and challenge rate state. Fresh reservations by C and by
/// the account's root are refused with every row unchanged. While both allowances stay
/// exhausted, the held requests still settle at T: known (10, 7) usage is charged 17 wei; an
/// explicit unknown release and invalid (10, 21) usage return their whole 30 wei with 30
/// wei of exposure; and after reopening restart recovery releases the last hold. Each
/// result changes exactly that request's original accounting (complete expected rows from
/// fixed literals). Quote, challenge, rate and metadata rows are neither charged nor
/// replenished, first results survive later calls and reopening, and fresh reservations
/// stay refused.
#[test]
fn exhausted_inference_budgets_do_not_strand_admitted_holds() {
    const T: u64 = 5_000;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, opened) = paid(&store);
    let id = q.payload.id.clone();
    let model = q.payload.model.clone();
    let root = opened.credential.unwrap();
    let pending = quote_at(1000, false, T - 20);
    store.store_quote(&pending, T - 20).unwrap();
    let owner = crate::wallet::address_from_private_key(OWNER_KEY).unwrap();
    store.create_challenge(&rate_challenge(&owner, 1, T - 20), T - 20).unwrap();
    let options = CredentialIssueRequest {
        expires_at: 100_000,
        spend_cap_wei: None,
        max_concurrency: 5,
        scopes: vec!["infer".into(), "read".into()],
    };
    let c = store.issue_credential(&id, &options, 102).unwrap();
    let holds = ["held-known", "held-unknown", "held-invalid", "held-restart"]
        .map(|key| store.reserve(&id, &c.id, key, 10, 20, T - 10, &model).unwrap());
    budget_records(&store, &id, &c.id, "known", 86, T - 5, BudgetEnd::Known("completed"));
    budget_records(&store, &id, &c.id, "unknown", 30, T - 5, BudgetEnd::Unknown("cancelled_unknown_usage"));
    assert_eq!(budget_counts(&path, &id, &c.id, T - 60, T - 3600), (120, 30));
    assert_eq!(budget_counts(&path, &id, &root.id, T - 60, T - 3600).0, 0);
    let unrelated = |rows: &ExpiryRows| {
        ["metadata", "rate_limits", "quotes", "challenges"].map(|table| rows[table].clone())
    };
    let allowance = unrelated(&expiry_ledger_rows(&path));
    assert!(allowance[1..].iter().all(|rows| !rows.is_empty()), "the fixture must hold quote and challenge state");
    let account = store.account(&id).unwrap();
    assert_eq!(
        (account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str()),
        ("789", "120", "86")
    );
    let exhausted = |store: &BillingStore, case: &str| {
        for credential in [&c.id, &root.id] {
            assert_budget_refused(store, &path, &id, credential, "fresh", T, case);
        }
    };
    exhausted(&store, "before settlement");

    let mut expected = expiry_ledger_rows(&path);
    expect_terminal(&mut expected, &holds[0].id, "completed", Some((10, 7, 17)), T);
    let known = store
        .settle(&holds[0].id, &TokenUsage { prompt_tokens: 10, completion_tokens: 7 }, "completed", T)
        .unwrap();
    assert_eq!(known.cost_wei, "17");
    assert_eq!(expiry_ledger_rows(&path), expected, "known settlement");
    exhausted(&store, "after known settlement");

    expect_terminal(&mut expected, &holds[1].id, "cancelled_unknown_usage", None, T);
    store.settle(&holds[1].id, &TokenUsage::default(), "cancelled_unknown_usage", T).unwrap();
    assert_eq!(expiry_ledger_rows(&path), expected, "unknown release");
    exhausted(&store, "after unknown release");

    expect_terminal(&mut expected, &holds[2].id, "invalid_usage_unknown_usage", None, T);
    let error = store
        .settle(&holds[2].id, &TokenUsage { prompt_tokens: 10, completion_tokens: 21 }, "completed", T)
        .unwrap_err();
    assert!(error.starts_with("usage_over_reservation:"), "{error}");
    assert_eq!(expiry_ledger_rows(&path), expected, "invalid-usage release");
    exhausted(&store, "after invalid-usage release");

    drop(store);
    store = BillingStore::open(&path, &network()).unwrap();
    exhausted(&store, "reopened");
    expect_terminal(&mut expected, &holds[3].id, "restart_unknown_usage", None, T);
    assert_eq!(store.recover_requests(T).unwrap(), 1);
    assert_eq!(expiry_ledger_rows(&path), expected, "restart recovery");
    let account = store.account(&id).unwrap();
    assert_eq!(
        (
            account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str(),
            account.usage.prompt_tokens, account.usage.completion_tokens, account.provider_loss_wei.as_str(),
            account.fee_wei.as_str(), account.funded_wei.as_str(),
        ),
        ("892", "0", "103", 96, 7, "0", "5", "1000")
    );
    let entry = expiry_credential(&store, &id, &c.id);
    assert_eq!((entry.reserved_wei.as_str(), entry.spent_wei.as_str()), ("0", "103"));

    let records: Vec<Value> = holds
        .iter()
        .map(|hold| expected["requests"].iter().find(|row| row[0] == hold.id).unwrap()[6].clone())
        .collect();
    for reopened in [false, true] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        for (hold, record) in holds.iter().zip(&records) {
            for (late, status) in [
                (TokenUsage { prompt_tokens: 10, completion_tokens: 20 }, "completed"),
                (TokenUsage::default(), "failed_unknown_usage"),
            ] {
                let replay = store.settle(&hold.id, &late, status, T + 1).unwrap();
                assert_eq!(serde_json::to_value(&replay).unwrap(), *record, "{}: late {status}", hold.id);
            }
        }
        assert_eq!(store.recover_requests(T + 1).unwrap(), 0);
        assert_eq!(expiry_ledger_rows(&path), expected, "reopened {reopened}");
        exhausted(&store, "after every settlement");
    }
    assert_eq!(unrelated(&expiry_ledger_rows(&path)), allowance, "quote or challenge allowance changed");
}

// inference-request-accounting task 2.3: attributable request and receipt recovery.
// Ledger steps take explicit logical operation times. The router tests use the
// CredentialRoutes fixture, whose production routers read the real clock, with ledger
// fixtures at logical times before that clock. Post-verification funding and supplied
// usage are ledger prerequisites, not chain or engine evidence.

/// The members of a request record: accounting metadata only, no prompt or output.
const REQUEST_MEMBERS: [&str; 15] = [
    "contract_id", "cost_wei", "created_at", "credential_id", "id", "max_cost_wei",
    "max_output_tokens", "performance", "prompt_tokens_reserved", "provider_loss_wei",
    "request_id", "settled_at", "status", "usage", "waived_wei",
];

/// The members of a signed receipt's payload.
const RECEIPT_MEMBERS: [&str; 6] = ["account", "issued_at", "network", "provider_wallet", "requests", "sequence"];

fn members(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

/// Whether `id` has the ledger's opaque identifier form: `0x` and 64 lowercase hex digits.
fn opaque_request_id(id: &str) -> bool {
    id.len() == 66
        && id.starts_with("0x")
        && id[2..].bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The stored record of request `id` in complete ledger rows.
fn request_record(rows: &ExpiryRows, id: &str) -> Value {
    rows["requests"]
        .iter()
        .find(|row| row[0] == id)
        .unwrap_or_else(|| panic!("no request row {id}"))[6]
        .clone()
}

/// What a request keeps for its lifetime: its opaque and client identifiers, account and
/// credential attribution, original token bounds, maximum hold and creation time.
fn request_identity(record: &Value) -> Value {
    json!({
        "id": record["id"], "request_id": record["request_id"], "contract_id": record["contract_id"],
        "credential_id": record["credential_id"], "prompt_tokens_reserved": record["prompt_tokens_reserved"],
        "max_output_tokens": record["max_output_tokens"], "max_cost_wei": record["max_cost_wei"],
        "created_at": record["created_at"],
    })
}

/// inference-request-accounting *Request identity prevents automatic paid replay*, scenarios
/// *Duplicate reserved or terminal request* and *Account-scoped keys and absent keys*, at
/// the ledger (task 2.3). Paid account A (one wei per token) reserves four client keys
/// through its root and an `infer` application credential; free account B reserves A's
/// first key. Each reservation persists an opaque `0x` + 64-hex identifier, distinct from
/// its key and from every other request, B's for the same key included. Either identifier
/// reads the same record, and neither account can read the other's.
///
/// While reserved, after known, explicit unknown and invalid-usage settlement and after
/// restart recovery, before and after reopening, each identity (both identifiers, account,
/// credential, token bounds, maximum hold, creation time) equals the fixed literals of its
/// reservation, and the complete rows equal the terminal accounting derived from those
/// literals. Every duplicate, through either credential, with the original or changed
/// bounds, and spelled as the key or as the request's opaque identifier, returns
/// `duplicate_request` with every ledger row unchanged (no second request, hold or charge),
/// and the key still names the original. Later terminal calls return the first result.
/// B admits A's opaque identifier as its own key, and A a fresh key: identity is per account.
#[test]
fn request_identity_and_attribution_survive_settlement_reopening_and_duplicates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, opened) = paid(&store);
    let a = q.payload.id.clone();
    let model = q.payload.model.clone();
    let root = opened.credential.unwrap();
    let app = app_credential(&store, &a, None, vec!["infer".into()]);
    let free = quote(0, true);
    store.store_quote(&free, 105).unwrap();
    let b = free.payload.id.clone();
    let b_root = store.open_account(&free, None, 106).unwrap().credential.unwrap();
    let shapes: [(&str, &str, u64, u64, u64); 4] = [
        ("known-key", app.id.as_str(), 10, 20, 110),
        ("unknown-key", root.id.as_str(), 5, 5, 111),
        ("invalid-key", app.id.as_str(), 3, 4, 112),
        ("restart-key", root.id.as_str(), 2, 2, 113),
    ];
    let mut ids = Vec::new();
    let mut identities = Vec::new();
    for (key, credential, prompt, output, at) in shapes {
        let held = store.reserve(&a, credential, key, prompt, output, at, &model).unwrap();
        identities.push(json!({
            "id": held.id, "request_id": key, "contract_id": a, "credential_id": credential,
            "prompt_tokens_reserved": prompt, "max_output_tokens": output,
            "max_cost_wei": (prompt + output).to_string(), "created_at": at,
        }));
        ids.push(held.id);
    }
    let b_held = store.reserve(&b, &b_root.id, "known-key", 10, 20, 114, &free.payload.model).unwrap();
    store.settle(&b_held.id, &TokenUsage { prompt_tokens: 10, completion_tokens: 5 }, "completed", 115).unwrap();
    let mut all: Vec<&str> = ids.iter().map(String::as_str).collect();
    all.push(&b_held.id);
    for (index, id) in all.iter().enumerate() {
        assert!(opaque_request_id(id), "{id} is not an opaque request identifier");
        assert!(all[index + 1..].iter().all(|other| other != id), "request identifiers repeat");
        assert!(shapes.iter().all(|(key, ..)| key != id), "an identifier equals its client key");
    }

    // Identity and account-scoped reads; returns the complete rows.
    let reads = |store: &BillingStore, stage: &str| {
        let rows = expiry_ledger_rows(&path);
        for ((key, ..), (id, identity)) in shapes.iter().zip(ids.iter().zip(&identities)) {
            let record = request_record(&rows, id);
            assert_eq!(request_identity(&record), *identity, "{stage}: {key}");
            for lookup in [*key, id.as_str()] {
                let read = serde_json::to_value(store.request_view(&a, lookup).unwrap()).unwrap();
                assert_eq!(read, record, "{stage}: {lookup}");
            }
            assert!(store.request_view(&b, id).unwrap_err().starts_with("not_found:"), "{stage}: B read A's {key}");
        }
        assert_eq!(store.request_view(&b, "known-key").unwrap().id, b_held.id, "{stage}");
        assert!(store.request_view(&a, &b_held.id).unwrap_err().starts_with("not_found:"), "{stage}");
        rows
    };
    // Every duplicate is refused without a row changing, and the key still names the original.
    let duplicates = |store: &BillingStore, at: u64, stage: &str| {
        let before = expiry_ledger_rows(&path);
        for ((key, ..), id) in shapes.iter().zip(&ids) {
            for credential in [&root.id, &app.id] {
                for (prompt, output) in [(1, 1), (10, 20)] {
                    for spelling in [*key, id.as_str()] {
                        let error = store.reserve(&a, credential, spelling, prompt, output, at, &model).unwrap_err();
                        assert!(error.starts_with("duplicate_request:"), "{stage}: {key} as {spelling}: {error}");
                    }
                }
            }
            assert_eq!(store.request_view(&a, key).unwrap().id, *id, "{stage}: {key} names another request");
        }
        assert_eq!(expiry_ledger_rows(&path), before, "{stage}: a duplicate changed the ledger");
    };

    let reserved = reads(&store, "reserved");
    duplicates(&store, 116, "reserved");

    // Terminal outcomes, expected from the fixed literals (one wei per token).
    let mut expected = reserved;
    expect_terminal(&mut expected, &ids[0], "completed", Some((10, 7, 17)), 120);
    store.settle(&ids[0], &TokenUsage { prompt_tokens: 10, completion_tokens: 7 }, "completed", 120).unwrap();
    expect_terminal(&mut expected, &ids[1], "cancelled_unknown_usage", None, 121);
    store.settle(&ids[1], &TokenUsage::default(), "cancelled_unknown_usage", 121).unwrap();
    expect_terminal(&mut expected, &ids[2], "invalid_usage_unknown_usage", None, 122);
    let error = store
        .settle(&ids[2], &TokenUsage { prompt_tokens: 3, completion_tokens: 5 }, "completed", 122)
        .unwrap_err();
    assert!(error.starts_with("usage_over_reservation:"), "{error}");
    drop(store);
    store = BillingStore::open(&path, &network()).unwrap();
    expect_terminal(&mut expected, &ids[3], "restart_unknown_usage", None, 130);
    assert_eq!(store.recover_requests(130).unwrap(), 1);
    assert_eq!(expiry_ledger_rows(&path), expected, "terminal accounting");
    let account = store.account(&a).unwrap();
    assert_eq!(
        (account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str()),
        ("978", "0", "17")
    );

    for reopened in [false, true] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        let stage = if reopened { "terminal, reopened" } else { "terminal" };
        assert_eq!(reads(&store, stage), expected, "{stage}");
        duplicates(&store, 131, stage);
        for id in &ids {
            let replay = store
                .settle(id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", 132)
                .unwrap();
            assert_eq!(serde_json::to_value(replay).unwrap(), request_record(&expected, id), "{stage}");
        }
        assert_eq!(store.recover_requests(133).unwrap(), 0, "{stage}");
        assert_eq!(expiry_ledger_rows(&path), expected, "{stage}");
    }

    // The identifier namespace is the account's: B admits A's opaque identifier as its own
    // key, which reads B's request in B and still A's request in A.
    let spelled = store.reserve(&b, &b_root.id, &ids[0], 1, 1, 140, &free.payload.model).unwrap();
    assert!(opaque_request_id(&spelled.id) && spelled.id != ids[0]);
    assert_eq!(store.request_view(&b, &ids[0]).unwrap().id, spelled.id);
    assert_eq!(store.request_view(&a, &ids[0]).unwrap().request_id, "known-key");
    let fresh = store.reserve(&a, &app.id, "fresh-key", 1, 1, 141, &model).unwrap();
    assert_eq!(fresh.max_cost_wei, "2");
    assert_eq!(store.request_view(&a, "fresh-key").unwrap().credential_id, app.id);
}

/// Task 2.3, requirement *Request and receipt history supports attributable recovery*:
/// "Request lists SHALL be account-scoped bounded pages of at most 100, ordered by
/// descending creation time with descending insertion order for ties", and a receipt binds
/// "a consistent account/request snapshot and at most the 100 most recent account
/// requests". Paid account A's root reserves 103 one-wei requests whose creation times are
/// out of insertion order, with two- and three-way ties (`250 + 37n mod 50`); free account
/// B reserves every tenth key at the same time. The fixture computes the expected order from
/// those times and the insertion order, not from the ledger's query.
///
/// At 100 records the page holds all of them; at 101 and 103 it holds exactly the 100
/// newest, the omitted oldest stay individually readable, and the page never contains B's
/// requests (B's own page holds only B's). A receipt's payload has exactly the members
/// `{network, provider_wallet, account, requests, sequence, issued_at}`, binds the complete
/// network and provider wallet, the account exactly as stored and that same 100-request
/// page, each request with only the accounting members; it changes only the receipt
/// sequence, also after reopening. B's receipt binds only B's requests.
#[test]
fn request_pages_and_receipts_hold_the_100_newest_account_requests_newest_first() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, opened) = paid(&store);
    let a = q.payload.id.clone();
    let model = q.payload.model.clone();
    let root = opened.credential.unwrap();
    let free = quote(0, true);
    store.store_quote(&free, 105).unwrap();
    let b = free.payload.id.clone();
    let b_root = store.open_account(&free, None, 106).unwrap().credential.unwrap();
    let created = |n: u64| 250 + (n * 37) % 50;
    // The fixture's own newest-first order: creation time, then insertion order, descending.
    let newest_first = |inserted: &[(u64, u64, String)]| {
        let mut order = inserted.to_vec();
        order.sort_by(|x, y| (y.0, y.1).cmp(&(x.0, x.1)));
        order.into_iter().map(|(_, _, id)| id).collect::<Vec<_>>()
    };
    let page = |store: &BillingStore, account: &str| -> Vec<Value> {
        store.requests(account, None).unwrap().iter().map(|request| serde_json::to_value(request).unwrap()).collect()
    };
    let mut inserted: Vec<(u64, u64, String)> = Vec::new();
    let mut b_inserted: Vec<(u64, u64, String)> = Vec::new();
    for n in 0..103u64 {
        let at = created(n);
        let key = format!("page-{n}");
        let held = store.reserve(&a, &root.id, &key, 1, 0, at, &model).unwrap();
        if n < 101 {
            if n % 4 == 0 {
                store.settle(&held.id, &TokenUsage::default(), "cancelled_unknown_usage", at).unwrap();
            } else {
                store.settle(&held.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 0 }, "completed", at).unwrap();
            }
        }
        inserted.push((at, n, held.id));
        if n % 10 == 0 {
            let other = store.reserve(&b, &b_root.id, &key, 1, 0, at, &free.payload.model).unwrap();
            store.settle(&other.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 0 }, "completed", at).unwrap();
            b_inserted.push((at, n, other.id));
        }
        if matches!(n, 99 | 100 | 102) {
            let records = n as usize + 1;
            let rows = expiry_ledger_rows(&path);
            let order = newest_first(&inserted);
            let expected: Vec<Value> = order.iter().take(100).map(|id| request_record(&rows, id)).collect();
            let listed = page(&store, &a);
            assert_eq!(listed.len(), 100, "{records} records");
            assert_eq!(listed, expected, "{records} records");
            assert_eq!(order.len() - 100, records - 100, "{records} records");
            for omitted in &order[100..] {
                let read = serde_json::to_value(store.request_view(&a, omitted).unwrap()).unwrap();
                assert_eq!(read, request_record(&rows, omitted), "{records} records: an omitted request");
            }
            assert!(listed.iter().all(|request| request["contract_id"] == a), "{records} records");
        }
    }
    // The three omitted requests are the oldest: the three-way tie at 250, newest insertion first.
    let order = newest_first(&inserted);
    assert_eq!(order[100..], [inserted[100].2.clone(), inserted[50].2.clone(), inserted[0].2.clone()]);
    let b_page = page(&store, &b);
    let b_order = newest_first(&b_inserted);
    assert_eq!(b_page.iter().map(|request| request["id"].as_str().unwrap().to_string()).collect::<Vec<_>>(), b_order);
    assert!(b_page.iter().all(|request| request["contract_id"] == b));
    let account = store.account(&a).unwrap();
    assert_eq!(
        (account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str()),
        ("918", "2", "75")
    );

    for (reopened, sequence) in [(false, 1u64), (true, 2)] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        let rows = expiry_ledger_rows(&path);
        let expected_page: Vec<Value> = order.iter().take(100).map(|id| request_record(&rows, id)).collect();
        let issued_at = 400 + sequence;
        let receipt = store.receipt(&a, PROVIDER_KEY, issued_at).unwrap();
        protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &q.payload.provider_wallet).unwrap();
        let payload = serde_json::to_value(&receipt.payload).unwrap();
        assert_eq!(members(&payload), RECEIPT_MEMBERS);
        let mut expected_rows = rows.clone();
        let account_row = expiry_row(&mut expected_rows, "accounts", &a);
        assert_eq!(payload["account"], account_row[3], "reopened {reopened}");
        account_row[4] = json!(sequence);
        assert_eq!(payload["requests"], Value::Array(expected_page), "reopened {reopened}");
        assert_eq!(
            (&payload["network"], &payload["provider_wallet"], &payload["sequence"], &payload["issued_at"]),
            (&serde_json::to_value(network()).unwrap(), &json!(q.payload.provider_wallet), &json!(sequence), &json!(issued_at))
        );
        for request in payload["requests"].as_array().unwrap() {
            assert_eq!(members(request), REQUEST_MEMBERS);
        }
        assert_eq!(expiry_ledger_rows(&path), expected_rows, "a receipt changed more than its sequence");
    }
    let b_receipt = store.receipt(&b, PROVIDER_KEY, 410).unwrap();
    assert_eq!(b_receipt.payload.account.contract_id, b);
    assert_eq!(b_receipt.payload.requests.iter().map(|request| request.id.clone()).collect::<Vec<_>>(), b_order);
}

/// Task 2.3: "an optional `before` timestamp SHALL exclude requests created at or after that
/// timestamp. This timestamp paging SHALL NOT be advertised as an exhaustive tie-safe
/// cursor". The documented limit, not a defect: one account holds four requests at
/// 1,000-1,003, six tied at T = 1,100 and 96 at T + 1 to T + 96. The 100-request first page
/// ends inside the tie, at its four newest insertions. A client continuing with
/// `before = T` (the last item's creation time) gets only the four older requests, so the
/// two remaining T requests appear on neither page; continuing with `before = T + 1` instead
/// repeats the four tied requests already listed. The omitted requests are retained and
/// readable by key, and a receipt's 100 newest omit them too.
#[test]
fn timestamp_before_pages_can_omit_requests_sharing_the_boundary_timestamp() {
    const T: u64 = 1100;
    let store = memory();
    let (q, _, opened) = paid(&store);
    let a = q.payload.id.clone();
    let model = q.payload.model.clone();
    let root = opened.credential.unwrap();
    let make = |key: String, at: u64| {
        let held = store.reserve(&a, &root.id, &key, 1, 0, at, &model).unwrap();
        store.settle(&held.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 0 }, "completed", at).unwrap();
        held.id
    };
    let older: Vec<String> = (0..4).map(|n| make(format!("older-{n}"), 1000 + n)).collect();
    let tied: Vec<String> = (0..6).map(|n| make(format!("tied-{n}"), T)).collect();
    let newer: Vec<String> = (0..96).map(|n| make(format!("newer-{n}"), T + 1 + n)).collect();
    let ids = |page: Vec<RequestView>| page.into_iter().map(|request| request.id).collect::<Vec<_>>();
    let reversed = |ids: &[String]| ids.iter().rev().cloned().collect::<Vec<_>>();

    let first = ids(store.requests(&a, None).unwrap());
    let mut expected = reversed(&newer);
    expected.extend(reversed(&tied[2..]));
    assert_eq!(first, expected);
    assert_eq!(store.request_view(&a, first.last().unwrap()).unwrap().created_at, T);

    let exclusive = ids(store.requests(&a, Some(T)).unwrap());
    assert_eq!(exclusive, reversed(&older));
    let listed: std::collections::HashSet<&String> = first.iter().chain(&exclusive).collect();
    let omitted: Vec<&String> = older.iter().chain(&tied).chain(&newer).filter(|id| !listed.contains(id)).collect();
    assert_eq!(omitted, [&tied[0], &tied[1]], "the boundary tie's unseen requests");

    let inclusive = ids(store.requests(&a, Some(T + 1)).unwrap());
    let mut expected = reversed(&tied);
    expected.extend(reversed(&older));
    assert_eq!(inclusive, expected);
    assert_eq!(inclusive[..4], first[96..], "the inclusive cursor repeats listed requests");

    for (n, id) in tied[..2].iter().enumerate() {
        let view = store.request_view(&a, &format!("tied-{n}")).unwrap();
        assert_eq!((view.id.as_str(), view.created_at, view.status.as_str()), (id.as_str(), T, "completed"));
    }
    let receipt = store.receipt(&a, PROVIDER_KEY, 1300).unwrap();
    assert_eq!(receipt.payload.requests.iter().map(|request| request.id.clone()).collect::<Vec<_>>(), first);
}

/// Task 2.3: "The positive receipt sequence SHALL increase durably per account across
/// restart; signing or persistence failure SHALL not return a successful uncommitted
/// receipt", and receipt issuance "may advance only its receipt sequence". Receipts for paid
/// account A and free accounts B and C, interleaved across two reopenings, number 1, 2, ...
/// per account independently; each verifies, binds the stored account and changes only its
/// own account's sequence (complete rows).
///
/// Each failure returns no receipt and leaves every row unchanged: another signer
/// (`wrong_provider`), a malformed signing key, an A-scoped `AFTER UPDATE OF sequence`
/// trigger raising `RAISE(FAIL)` (before and after reopening, while B still advances), and a
/// deferred foreign-key witness that refuses the commit after the sequence write (before
/// and after reopening; no witness survives). After the fixtures are removed, A's next
/// receipt takes exactly the next number: no gap, no reuse. With C's stored sequence
/// restored to `i64::MAX - 1` (issuing 2^63 receipts is not feasible), C's next receipt
/// takes `i64::MAX` and the following one is refused as exhausted with no row changed, also
/// after reopening, while A and B continue.
#[test]
fn receipt_sequences_are_durable_per_account_and_failed_issuance_commits_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.sqlite");
    let mut store = BillingStore::open(&path, &network()).unwrap();
    let (q, _, opened) = paid(&store);
    let a = q.payload.id.clone();
    let model = q.payload.model.clone();
    let root = opened.credential.unwrap();
    let settled = store.reserve(&a, &root.id, "receipt-settled", 1, 1, 103, &model).unwrap();
    store.settle(&settled.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", 104).unwrap();
    store.reserve(&a, &root.id, "receipt-held", 10, 20, 105, &model).unwrap();
    let [b, c] = [0, 1].map(|_| {
        let free = quote(0, true);
        store.store_quote(&free, 106).unwrap();
        store.open_account(&free, None, 106).unwrap();
        free.payload.id
    });
    let provider = q.payload.provider_wallet.clone();
    let issue = |store: &BillingStore, account: &str, at: u64, sequence: u64| {
        let mut expected = expiry_ledger_rows(&path);
        let receipt = store.receipt(account, PROVIDER_KEY, at).unwrap();
        protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &provider).unwrap();
        let row = expiry_row(&mut expected, "accounts", account);
        assert_eq!(serde_json::to_value(&receipt.payload.account).unwrap(), row[3], "{account}");
        assert_eq!((receipt.payload.sequence, receipt.payload.issued_at), (sequence, at), "{account}");
        row[4] = json!(sequence);
        assert_eq!(expiry_ledger_rows(&path), expected, "a receipt changed more than its own sequence");
    };
    let refused = |store: &BillingStore, account: &str, key: &str, expected_error: &str, stage: &str| {
        let before = expiry_ledger_rows(&path);
        let error = store.receipt(account, key, 200).unwrap_err();
        assert!(error.contains(expected_error), "{stage}: {error}");
        assert_eq!(expiry_ledger_rows(&path), before, "{stage}: a failed receipt changed the ledger");
    };

    issue(&store, &a, 110, 1);
    issue(&store, &b, 111, 1);
    issue(&store, &a, 112, 2);
    drop(store);
    store = BillingStore::open(&path, &network()).unwrap();
    issue(&store, &b, 113, 2);
    issue(&store, &a, 114, 3);
    drop(store);
    store = BillingStore::open(&path, &network()).unwrap();
    issue(&store, &c, 115, 1);
    issue(&store, &a, 116, 4);

    refused(&store, &a, OWNER_KEY, "wrong_provider:", "another signer");
    refused(&store, &a, "not-a-signing-key", "", "a malformed signing key");

    let fixture = Connection::open(&path).unwrap();
    fixture
        .execute_batch(&format!(
            "CREATE TRIGGER receipt_sequence_fault AFTER UPDATE OF sequence ON accounts WHEN NEW.id='{a}'
             BEGIN SELECT RAISE(FAIL,'injected receipt sequence failure'); END;"
        ))
        .unwrap();
    refused(&store, &a, PROVIDER_KEY, "injected receipt sequence failure", "sequence write failure");
    issue(&store, &b, 120, 3);
    drop(store);
    store = BillingStore::open(&path, &network()).unwrap();
    refused(&store, &a, PROVIDER_KEY, "injected receipt sequence failure", "sequence write failure, reopened");
    fixture.execute_batch("DROP TRIGGER receipt_sequence_fault").unwrap();

    let before = expiry_ledger_rows(&path);
    fixture
        .execute_batch(&format!(
            "CREATE TABLE receipt_commit_parent(id TEXT PRIMARY KEY);
             CREATE TABLE receipt_commit_witness(account TEXT NOT NULL
                 REFERENCES receipt_commit_parent(id) DEFERRABLE INITIALLY DEFERRED);
             CREATE TRIGGER receipt_commit_fault AFTER UPDATE OF sequence ON accounts WHEN NEW.id='{a}'
             BEGIN INSERT INTO receipt_commit_witness VALUES(NEW.id); END;"
        ))
        .unwrap();
    for reopened in [false, true] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        let error = store.receipt(&a, PROVIDER_KEY, 121).unwrap_err();
        assert!(error.contains("FOREIGN KEY constraint failed"), "reopened {reopened}: {error}");
        let (witnesses, sequence): (u64, u64) = fixture
            .query_row(
                "SELECT (SELECT COUNT(*) FROM receipt_commit_witness),(SELECT sequence FROM accounts WHERE id=?1)",
                [&a],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((witnesses, sequence), (0, 4), "reopened {reopened}: a refused commit persisted");
    }
    fixture
        .execute_batch("DROP TRIGGER receipt_commit_fault; DROP TABLE receipt_commit_witness; DROP TABLE receipt_commit_parent;")
        .unwrap();
    assert_eq!(expiry_ledger_rows(&path), before, "a refused commit changed the ledger");
    issue(&store, &a, 122, 5);

    fixture
        .execute("UPDATE accounts SET sequence=?1 WHERE id=?2", params![i64::MAX - 1, c])
        .unwrap();
    issue(&store, &c, 123, i64::MAX as u64);
    for reopened in [false, true] {
        if reopened {
            drop(store);
            store = BillingStore::open(&path, &network()).unwrap();
        }
        let before = expiry_ledger_rows(&path);
        assert_eq!(store.receipt(&c, PROVIDER_KEY, 124).unwrap_err(), "receipt sequence exhausted");
        assert_eq!(expiry_ledger_rows(&path), before, "reopened {reopened}: exhaustion changed the ledger");
    }
    issue(&store, &a, 125, 6);
    issue(&store, &b, 126, 4);
}

/// Task 2.3: a receipt binds "a consistent account/request snapshot" when another writer
/// competes, by a deterministic schedule. A competing connection applies the production
/// settlement transition (`settle_request`) of a 30-wei hold inside its own uncommitted
/// write transaction. A receipt writer on an independent connection starts and must wait
/// for the write lock (its busy handler reports the wait and holds it there). The competing
/// transaction then commits, or rolls back, and the receipt continues.
///
/// The receipt binds exactly the committed state: the settled request and account (17 wei
/// charged, 966 available, 10 reserved) after a commit, the original hold (953 available,
/// 40 reserved) after a rollback, never a mixture, and only its sequence changes. Two
/// further receipt writers waiting behind one competing writer then take sequences 3 and 4,
/// never the same number. Would catch a request page or sequence read outside the receipt's
/// write transaction.
#[test]
fn a_receipt_waits_for_a_competing_settlement_and_binds_only_committed_state() {
    use std::{cell::RefCell, sync::mpsc, time::Duration};
    thread_local! {
        static RECEIPT_WAIT: RefCell<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> =
            const { RefCell::new(None) };
    }
    // rusqlite accepts a function pointer. The first wait for the write lock is reported
    // and held until released; later waits retry every millisecond for up to ten seconds.
    fn wait_for_release(attempts: i32) -> bool {
        RECEIPT_WAIT.with(|slot| match slot.borrow_mut().take() {
            Some((blocked, release)) => {
                blocked.send(()).is_ok() && release.recv_timeout(Duration::from_secs(10)).is_ok()
            }
            None => {
                std::thread::sleep(Duration::from_millis(1));
                attempts < 10_000
            }
        })
    }
    /// A receipt writer's own connection, opened before any competing writer holds the lock.
    fn receipt_writer(path: &Path) -> BillingStore {
        let writer = BillingStore::open(path, &network()).unwrap();
        writer.db.lock().unwrap().busy_handler(Some(wait_for_release)).unwrap();
        writer
    }
    type Started = (mpsc::Receiver<()>, mpsc::Sender<()>, std::thread::JoinHandle<Result<Signed<UsageReceipt>, String>>);
    fn start_receipt(writer: BillingStore, account: &str, issued_at: u64) -> Started {
        let (blocked_tx, blocked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let account = account.to_string();
        let worker = std::thread::spawn(move || {
            RECEIPT_WAIT.with(|slot| *slot.borrow_mut() = Some((blocked_tx, release_rx)));
            writer.receipt(&account, PROVIDER_KEY, issued_at)
        });
        (blocked_rx, release_tx, worker)
    }

    for commit in [true, false] {
        let case = if commit { "committed" } else { "rolled back" };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite");
        let store = BillingStore::open(&path, &network()).unwrap();
        let (q, _, opened) = paid(&store);
        let id = q.payload.id.clone();
        let model = q.payload.model.clone();
        let root = opened.credential.unwrap();
        let prior = store.reserve(&id, &root.id, "receipt-prior", 1, 1, 103, &model).unwrap();
        store.settle(&prior.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", 104).unwrap();
        let contended = store.reserve(&id, &root.id, "receipt-contended", 10, 20, 105, &model).unwrap();
        let held = store.reserve(&id, &root.id, "receipt-held", 5, 5, 106, &model).unwrap();
        assert_eq!(store.receipt(&id, PROVIDER_KEY, 107).unwrap().payload.sequence, 1);
        let mut expected = expiry_ledger_rows(&path);
        if commit {
            expect_terminal(&mut expected, &contended.id, "completed", Some((10, 7, 17)), 110);
        }
        expiry_row(&mut expected, "accounts", &id)[4] = json!(2);

        let writer = receipt_writer(&path);
        let mut competitor = Connection::open(&path).unwrap();
        let transaction = competitor.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        let (settled, invalid) = settle_request(
            &transaction,
            &contended.id,
            &TokenUsage { prompt_tokens: 10, completion_tokens: 7 },
            "completed",
            None,
            110,
        )
        .unwrap();
        assert_eq!((settled.status.as_str(), settled.cost_wei.as_str(), invalid), ("completed", "17", false));
        let (blocked, release, worker) = start_receipt(writer, &id, 111);
        blocked
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("{case}: the receipt must wait for the competing write lock"));
        assert!(!worker.is_finished(), "{case}");
        if commit {
            transaction.commit().unwrap();
        } else {
            transaction.rollback().unwrap();
        }
        release.send(()).unwrap();
        let receipt = worker.join().unwrap().unwrap();
        protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &q.payload.provider_wallet).unwrap();
        assert_eq!(expiry_ledger_rows(&path), expected, "{case}");
        assert_eq!(receipt.payload.sequence, 2, "{case}");
        let account_row = expiry_row(&mut expected, "accounts", &id)[3].clone();
        assert_eq!(serde_json::to_value(&receipt.payload.account).unwrap(), account_row, "{case}");
        let records: Vec<Value> = [&held.id, &contended.id, &prior.id].iter().map(|request| request_record(&expected, request)).collect();
        assert_eq!(serde_json::to_value(&receipt.payload.requests).unwrap(), Value::Array(records), "{case}");
        let account = &receipt.payload.account;
        assert_eq!(
            (account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str()),
            if commit { ("966", "10", "19") } else { ("953", "40", "2") },
            "{case}"
        );

        // Two receipt writers waiting behind one competing writer take distinct numbers.
        let writers = [receipt_writer(&path), receipt_writer(&path)];
        let mut competitor = Connection::open(&path).unwrap();
        let transaction = competitor.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        let started: Vec<Started> = writers
            .into_iter()
            .zip([112, 113])
            .map(|(writer, issued_at)| start_receipt(writer, &id, issued_at))
            .collect();
        for (blocked, _, _) in &started {
            blocked
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| panic!("{case}: each receipt must wait for the competing write lock"));
        }
        transaction.commit().unwrap();
        for (_, release, _) in &started {
            release.send(()).unwrap();
        }
        let mut sequences: Vec<u64> = started
            .into_iter()
            .map(|(_, _, worker)| {
                let receipt = worker.join().unwrap().unwrap();
                protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &q.payload.provider_wallet).unwrap();
                assert_eq!(serde_json::to_value(&receipt.payload.account).unwrap(), account_row, "{case}");
                receipt.payload.sequence
            })
            .collect();
        sequences.sort();
        assert_eq!(sequences, [3, 4], "{case}: concurrent receipts reused a sequence number");
        expiry_row(&mut expected, "accounts", &id)[4] = json!(4);
        assert_eq!(expiry_ledger_rows(&path), expected, "{case}");
    }
}

/// One bodiless GET to the control router or, when `public`, the public inference router,
/// with `bearer` when given. Returns the status and document; no response echoes the bearer.
async fn read_route(
    routes: &CredentialRoutes,
    public: bool,
    path: &str,
    bearer: Option<&str>,
) -> (axum::http::StatusCode, Value) {
    use tower::ServiceExt;
    let mut request = axum::http::Request::builder().method("GET").uri(path);
    if let Some(bearer) = bearer {
        request = request.header("authorization", format!("Bearer {bearer}"));
    }
    let request = request.body(axum::body::Body::empty()).unwrap();
    let response = if public {
        crate::inference_gateway::router(routes.state.clone()).oneshot(request).await.unwrap()
    } else {
        crate::contracts::router(routes.state.clone()).oneshot(request).await.unwrap()
    };
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
    if let Some(bearer) = bearer {
        assert!(!contains(&bytes, bearer.as_bytes()), "a response echoed the bearer");
    }
    (status, serde_json::from_slice(&bytes).unwrap())
}

/// Decodes and verifies a signed receipt document from `routes`' provider and returns its
/// payload, which must have exactly the receipt members and the complete network/provider.
fn routed_receipt(routes: &CredentialRoutes, document: Value) -> Value {
    let receipt: Signed<UsageReceipt> = serde_json::from_value(document).unwrap();
    protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &routes.state.wallet).unwrap();
    let payload = serde_json::to_value(&receipt.payload).unwrap();
    assert_eq!(members(&payload), RECEIPT_MEMBERS);
    assert_eq!(
        (&payload["network"], &payload["provider_wallet"]),
        (&serde_json::to_value(network()).unwrap(), &json!(routes.state.wallet))
    );
    payload
}

/// Task 2.3 through the production control and public inference routers: "Request status
/// and receipt access SHALL retain account isolation and existing read authorization", and
/// "Reads SHALL preserve financial amounts and original request outcomes; receipt issuance
/// may advance only its receipt sequence". Paid account A holds 101 one-wei requests at
/// distinct logical times through its root and an `infer`-only credential (known, explicit
/// unknown and one still reserved); free account B, same owner, holds a request under A's
/// newest key.
///
/// A `read`-only bearer, the root bearer and owner proofs read the 100-request list (the
/// 100 newest, newest first), its `before` continuation (the oldest), the held request by
/// opaque identifier and by key, the oldest by key, and (bearers) the public status of the
/// key, each equal to its stored record; every ledger row stays identical (owner proofs
/// consume only their own challenge, leaving an unrelated pending proof of the same wallet,
/// and change only the challenge issuance budget). Each receipt binds the stored account and the same
/// 100 records and changes only A's receipt sequence. No credential, an `infer`-only bearer,
/// a revoked bearer, an expired bearer (a restored past `expires_at`; nothing waits) and
/// B's bearer are refused on every control read, and all but B's on the public status,
/// with every row unchanged. B's bearer reads only B: the key names B's own request on the
/// public route, A's opaque identifier is not found there, and B's receipt binds only B.
#[tokio::test]
async fn request_and_receipt_reads_are_account_scoped_and_change_no_financial_row() {
    use axum::http::StatusCode;
    let routes = CredentialRoutes::new(1000, 4);
    let a = routes.id();
    let model = routes.agreement.payload.model.clone();
    let root = routes.opened.credential.clone().unwrap();
    let now = protocol::now().unwrap();
    let reader = routes.issue(1, &["read"]);
    let infer_only = routes.issue(2, &["infer"]);
    let revoked = routes.issue(1, &["infer", "read"]);
    routes.ledger().revoke_credential(&a, &revoked.id).unwrap();
    let expired = routes.issue(1, &["infer", "read"]);
    let restored = Connection::open(&routes.path)
        .unwrap()
        .execute(
            "UPDATE credentials SET record=json_set(record,'$.expires_at',?1) WHERE id=?2",
            params![now - 1, expired.id],
        )
        .unwrap();
    assert_eq!(restored, 1);
    let free = {
        let mut payload = quote(0, true).payload;
        payload.issued_at = now;
        payload.expires_at = now + 300;
        protocol::sign(protocol::QUOTE_DOMAIN, payload, PROVIDER_KEY).unwrap()
    };
    routes.ledger().store_quote(&free, now).unwrap();
    let b = free.payload.id.clone();
    let foreign = routes.ledger().open_account(&free, None, now).unwrap().credential.unwrap();

    let base = now - 1000;
    let mut ids = Vec::new();
    for n in 0..101u64 {
        let credential = if n % 2 == 0 { &root.id } else { &infer_only.id };
        let held = routes.ledger().reserve(&a, credential, &format!("page-{n}"), 1, 0, base + n, &model).unwrap();
        if n < 100 {
            let (usage, status) = if n % 5 == 0 {
                (TokenUsage::default(), "cancelled_unknown_usage")
            } else {
                (TokenUsage { prompt_tokens: 1, completion_tokens: 0 }, "completed")
            };
            routes.ledger().settle(&held.id, &usage, status, base + n).unwrap();
        }
        ids.push(held.id);
    }
    let held_id = ids[100].clone();
    let b_request = routes.ledger().reserve(&b, &foreign.id, "page-100", 1, 0, base + 100, &free.payload.model).unwrap();
    // The first control request binds the provider identity; later ones change nothing.
    let (status, _) = read_route(&routes, false, &format!("/v1/contracts/{a}"), Some(&reader.bearer)).await;
    assert_eq!(status, StatusCode::OK);

    let mut baseline = expiry_ledger_rows(&routes.path);
    let account = routes.ledger().account(&a).unwrap();
    assert_eq!(
        (account.available_wei.as_str(), account.reserved_wei.as_str(), account.spent_wei.as_str()),
        ("914", "1", "80")
    );
    let page: Vec<Value> = ids[1..].iter().rev().map(|id| request_record(&baseline, id)).collect();
    let oldest = request_record(&baseline, &ids[0]);
    let held = request_record(&baseline, &held_id);
    let list = format!("/v1/contracts/{a}/requests");
    let continuation = format!("{list}?before={}", base + 1);
    let by_id = format!("{list}/{held_id}");
    let by_key = format!("{list}/page-100");
    let oldest_by_key = format!("{list}/page-0");
    let receipt_path = format!("/v1/contracts/{a}/receipt");
    let status_path = "/v1/requests/page-100";
    let reads: [(bool, &str, Value); 6] = [
        (false, &list, Value::Array(page.clone())),
        (false, &continuation, json!([oldest])),
        (false, &by_id, held.clone()),
        (false, &by_key, held.clone()),
        (false, &oldest_by_key, oldest.clone()),
        (true, status_path, held.clone()),
    ];
    // Owner proofs may change only the challenge issuance budget (`challenge*` buckets);
    // every other row, the challenges table included, is compared.
    let owner_view = |rows: &ExpiryRows| {
        let mut rows = rows.clone();
        rows.get_mut("rate_limits").unwrap().retain(|row| !row[0].as_str().unwrap().starts_with("challenge"));
        rows
    };
    let mut sequence = 0;
    let mut check_receipt = |document: Value, baseline: &mut ExpiryRows, reader: &str| {
        let payload = routed_receipt(&routes, document);
        let account = expiry_row(baseline, "accounts", &a);
        assert_eq!(payload["account"], account[3], "{reader}");
        assert_eq!(payload["requests"], Value::Array(page.clone()), "{reader}");
        sequence += 1;
        assert_eq!(payload["sequence"], json!(sequence), "{reader}");
        account[4] = json!(sequence);
    };

    for (reader_name, bearer) in [("read bearer", &reader.bearer), ("root bearer", &root.bearer)] {
        for (public, path, expected) in &reads {
            let (status, document) = read_route(&routes, *public, path, Some(bearer)).await;
            assert_eq!((status, &document), (StatusCode::OK, expected), "{reader_name}: {path}");
            if path.contains("/requests/") {
                assert_eq!(members(&document), REQUEST_MEMBERS);
            }
        }
        assert_eq!(expiry_ledger_rows(&routes.path), baseline, "{reader_name}: a request read changed the ledger");
        let (status, document) = read_route(&routes, false, &receipt_path, Some(bearer)).await;
        assert_eq!(status, StatusCode::OK, "{reader_name}: {document}");
        check_receipt(document, &mut baseline, reader_name);
        assert_eq!(expiry_ledger_rows(&routes.path), baseline, "{reader_name}: a receipt changed more than its sequence");
    }
    // An unrelated pending proof of the same owner wallet: each owner read consumes only its
    // own proof, so this one stays pending.
    let pending = AuthPayload {
        network: network(),
        provider_wallet: routes.state.wallet.clone(),
        wallet: crate::wallet::address_from_private_key(OWNER_KEY).unwrap(),
        method: "GET".into(),
        path: format!("/v1/contracts/{a}/credentials"),
        body_hash: protocol::hash_hex(b""),
        nonce: protocol::random_id(),
        expires_at: protocol::now().unwrap() + 120,
    };
    routes.ledger().create_challenge(&pending, protocol::now().unwrap()).unwrap();
    let mut owner_baseline = expiry_ledger_rows(&routes.path);
    assert_eq!(owner_baseline["challenges"].len(), 1);
    for (public, path, expected) in &reads {
        if *public {
            continue;
        }
        let (status, document, nonce) = routes.owner("GET", path, Vec::new()).await;
        assert_eq!((status, &document), (StatusCode::OK, expected), "owner: {path}");
        assert!(routes.ledger().challenge(&nonce).is_err(), "owner: {path}: the proof was not consumed");
    }
    assert_eq!(owner_view(&expiry_ledger_rows(&routes.path)), owner_view(&owner_baseline), "owner reads changed the ledger");
    let (status, document, _) = routes.owner("GET", &receipt_path, Vec::new()).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    check_receipt(document, &mut owner_baseline, "owner");
    assert_eq!(owner_view(&expiry_ledger_rows(&routes.path)), owner_view(&owner_baseline), "an owner receipt changed more than its sequence");
    assert_eq!(routes.ledger().challenge(&pending.nonce).unwrap().path, pending.path, "an owner read consumed another proof");

    // Refused authority, with every row unchanged.
    let baseline = expiry_ledger_rows(&routes.path);
    let control_reads = [list.as_str(), continuation.as_str(), by_id.as_str(), by_key.as_str(), receipt_path.as_str()];
    for (who, bearer, status, control_code, public_code) in [
        ("no credential", None, StatusCode::UNAUTHORIZED, "unauthorized", Some("invalid_api_key")),
        ("an infer-only bearer", Some(infer_only.bearer.as_str()), StatusCode::FORBIDDEN, "forbidden", Some("credential_scope")),
        ("a revoked bearer", Some(revoked.bearer.as_str()), StatusCode::UNAUTHORIZED, "unauthorized", Some("invalid_api_key")),
        ("an expired bearer", Some(expired.bearer.as_str()), StatusCode::UNAUTHORIZED, "unauthorized", Some("invalid_api_key")),
        ("another account's bearer", Some(foreign.bearer.as_str()), StatusCode::FORBIDDEN, "forbidden", None),
    ] {
        for path in control_reads {
            let (code, document) = read_route(&routes, false, path, bearer).await;
            assert_eq!((code, document["error"]["code"].as_str()), (status, Some(control_code)), "{who}: {path}: {document}");
        }
        if let Some(public_code) = public_code {
            let (code, document) = read_route(&routes, true, status_path, bearer).await;
            assert_eq!((code, document["error"]["code"].as_str()), (status, Some(public_code)), "{who}: {document}");
        }
        assert_eq!(expiry_ledger_rows(&routes.path), baseline, "{who}: a refused read changed the ledger");
    }
    // Another account's bearer resolves keys and identifiers only within its own account.
    let (code, document) = read_route(&routes, true, status_path, Some(&foreign.bearer)).await;
    assert_eq!((code, &document), (StatusCode::OK, &request_record(&baseline, &b_request.id)));
    assert_eq!(document["contract_id"], b);
    let (code, document) = read_route(&routes, true, &format!("/v1/requests/{held_id}"), Some(&foreign.bearer)).await;
    assert_eq!((code, document["error"]["code"].as_str()), (StatusCode::NOT_FOUND, Some("request_not_found")));
    assert_eq!(expiry_ledger_rows(&routes.path), baseline);
    let (code, document) = read_route(&routes, false, &format!("/v1/contracts/{b}/receipt"), Some(&foreign.bearer)).await;
    assert_eq!(code, StatusCode::OK, "{document}");
    let payload = routed_receipt(&routes, document);
    assert_eq!(payload["requests"], json!([request_record(&baseline, &b_request.id)]));
    assert_eq!(payload["account"]["contract_id"], b);
    for credential in [&root, &reader] {
        routes.authenticates(credential);
    }
}

const CONTENT_PROMPT: &str = "zebra-quartz-prompt-7319";
const CONTENT_RENDERED: &str = "rendered-marker-5521";
const CONTENT_OUTPUT: [&str; 3] = ["Opal", "ine", " Lantern-4417"];

/// A private llama.cpp-shaped engine for the paid fixture's chat agreement on a loopback
/// listener, recording every call as `METHOD path`. Its template renders any prompt to
/// `CONTENT_RENDERED`, tokenization prepares two tokens, and a private stream with final
/// usage generates the three `CONTENT_OUTPUT` pieces, a `stop` finish and usage of two
/// prompt and three completion tokens. Any other generation request is answered 400.
async fn content_engine(log: Arc<Mutex<Vec<String>>>) -> (String, tokio::task::JoinHandle<()>) {
    use axum::{
        response::IntoResponse,
        routing::{get, post},
        Json, Router,
    };
    let generate = |Json(body): Json<Value>| async move {
        if body["stream"] != true || body["stream_options"] != json!({"include_usage": true}) {
            return axum::http::StatusCode::BAD_REQUEST.into_response();
        }
        let chunk = |choices: Value| {
            json!({"id": "engine-response", "object": "chat.completion.chunk", "model": "test/model", "created": 1, "choices": choices})
        };
        let mut events: Vec<Value> = CONTENT_OUTPUT
            .iter()
            .map(|piece| chunk(json!([{"index": 0, "delta": {"content": piece}, "finish_reason": null}])))
            .collect();
        events.push(chunk(json!([{"index": 0, "delta": {}, "finish_reason": "stop"}])));
        let mut usage = chunk(json!([]));
        usage["usage"] = json!({"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5});
        events.push(usage);
        let mut text: String = events.iter().map(|event| format!("data: {event}\n\n")).collect();
        text.push_str("data: [DONE]\n\n");
        ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], text).into_response()
    };
    let engine = Router::new()
        .route("/health", get(|| async { axum::http::StatusCode::OK }))
        .route("/apply-template", post(|| async { Json(json!({"prompt": CONTENT_RENDERED})) }))
        .route("/tokenize", post(|| async { Json(json!({"tokens": [10, 11]})) }))
        .route("/v1/chat/completions", post(generate))
        .fallback(|| async { axum::http::StatusCode::NOT_FOUND })
        .layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let log = log.clone();
                async move {
                    log.lock().unwrap().push(format!("{} {}", request.method(), request.uri().path()));
                    next.run(request).await
                }
            },
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    (base, tokio::spawn(async move { axum::serve(listener, engine).await.unwrap() }))
}

/// One chat request to the public inference router with `bearer`, each of `keys` as an
/// `Idempotency-Key` header, and `body`. Returns the status, the answer and `x-request-id`.
async fn chat_route(
    routes: &CredentialRoutes,
    bearer: &str,
    keys: &[&[u8]],
    body: &Value,
) -> (axum::http::StatusCode, Value, Option<String>) {
    use tower::ServiceExt;
    let mut request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json");
    for key in keys {
        request = request.header("idempotency-key", axum::http::HeaderValue::from_bytes(key).unwrap());
    }
    let request = request.body(axum::body::Body::from(body.to_string())).unwrap();
    let response = crate::inference_gateway::router(routes.state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let header = response.headers().get("x-request-id").map(|value| value.to_str().unwrap().to_string());
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
    assert!(!contains(&bytes, bearer.as_bytes()), "a response echoed the bearer");
    (status, serde_json::from_slice(&bytes).unwrap(), header)
}

/// Every file under `directory`, recursively.
fn files_under(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// Task 2.3, requirement *Request identity prevents automatic paid replay*: "A public
/// duplicate SHALL return HTTP 409 with `duplicate_request`, the original request identifier
/// and its current status, rather than replay generation or retained output", with the
/// key syntax and absent-key rule, through the production public router and a controlled
/// private engine (not model qualification or observed llama.cpp usage).
///
/// A paid chat request under `content-key` dispatches once (health, template,
/// tokenization, generation) and is charged its five wei. Repeating the key (non-streaming,
/// streaming, a different prompt, another `infer` credential) gets exactly the 409 body
/// naming the original identifier and `completed`, with that `x-request-id`: no engine call
/// and no ledger row changes. Two requests without a key each get a fresh version-4 UUID key
/// and their own dispatch and charge. Two keys, a non-ASCII key, an empty key, a path
/// character, a space, each of the other 28 printable ASCII characters outside the alphabet
/// and 129 characters are refused 400 before any engine call or row change; 128 characters
/// of the whole allowed alphabet are admitted under that exact key.
///
/// Neither status, list, request and receipt reads (bearer and owner proof) nor any file
/// under the provider's data directory contain the prompt, the rendered template or the
/// generated text; request records have only the accounting members.
#[tokio::test]
async fn completed_inference_retains_no_content_and_duplicates_never_replay_it() {
    use axum::http::StatusCode;
    let mut routes = CredentialRoutes::new(1000, 2);
    let log: Arc<Mutex<Vec<String>>> = Default::default();
    let (engine_url, engine) = content_engine(log.clone()).await;
    {
        let state = Arc::get_mut(&mut routes.state).unwrap();
        let served = &mut Arc::get_mut(&mut state.config).unwrap().models[0];
        served.engine_url = engine_url;
        // The served fixture's deadline, so a loaded test machine cannot time the engine out.
        served.request_timeout_secs = 60;
    }
    let a = routes.id();
    let root = routes.opened.credential.clone().unwrap();
    let application = routes.issue(1, &["infer"]);
    let calls = || log.lock().unwrap().clone();
    let model_id = routes.agreement.payload.model.id.clone();
    let body = |content: &str, stream: bool| {
        json!({"model": model_id, "messages": [{"role": "user", "content": content}], "max_tokens": 4, "stream": stream})
    };
    let prompt = format!("Please remember {CONTENT_PROMPT}");
    let dispatch = ["GET /health", "POST /apply-template", "POST /tokenize", "POST /v1/chat/completions"];

    let (status, answer, header) = chat_route(&routes, &root.bearer, &[b"content-key"], &body(&prompt, false)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["choices"][0]["message"]["content"], CONTENT_OUTPUT.concat());
    assert_eq!(calls(), dispatch);
    let original = routes.ledger().request_view(&a, "content-key").unwrap();
    assert_eq!(header.as_deref(), Some(original.id.as_str()));
    assert!(opaque_request_id(&original.id));
    assert_eq!(
        (
            original.status.as_str(), original.credential_id.as_str(), original.usage.prompt_tokens,
            original.usage.completion_tokens, original.cost_wei.as_str(), original.max_cost_wei.as_str(),
        ),
        ("completed", root.id.as_str(), 2, 3, "5", "6")
    );

    // Duplicates name the original and never reach the engine.
    let rows = expiry_ledger_rows(&routes.path);
    let conflict = json!({"error": {
        "message": "Request already exists and will not be replayed. Retrieve its status; response content is not retained.",
        "type": "invalid_request_error", "param": null, "code": "duplicate_request",
        "request_id": original.id, "request_status": "completed",
    }});
    for (bearer, content, stream) in [
        (&root.bearer, prompt.as_str(), false),
        (&root.bearer, prompt.as_str(), true),
        (&root.bearer, "a different prompt", false),
        (&application.bearer, prompt.as_str(), false),
    ] {
        let (status, answer, header) = chat_route(&routes, bearer, &[b"content-key"], &body(content, stream)).await;
        assert_eq!((status, &answer), (StatusCode::CONFLICT, &conflict), "stream {stream}");
        assert_eq!(header.as_deref(), Some(original.id.as_str()));
    }
    assert_eq!(calls(), dispatch, "a duplicate reached the engine");
    assert_eq!(expiry_ledger_rows(&routes.path), rows, "a duplicate changed the ledger");

    // Without a key, each request is a new operation under a fresh UUID.
    let mut fresh = Vec::new();
    for _ in 0..2 {
        let (status, answer, header) = chat_route(&routes, &root.bearer, &[], &body(&prompt, false)).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let view = routes.ledger().request_view(&a, header.as_deref().unwrap()).unwrap();
        assert_eq!(uuid::Uuid::parse_str(&view.request_id).unwrap().get_version_num(), 4);
        assert_eq!((view.status.as_str(), view.cost_wei.as_str()), ("completed", "5"));
        fresh.push(view);
    }
    assert!(fresh[0].request_id != fresh[1].request_id && fresh[0].id != fresh[1].id);
    assert!(fresh.iter().all(|view| view.id != original.id && view.request_id != "content-key"));
    assert_eq!(calls().len(), 3 * dispatch.len());
    let account = routes.ledger().account(&a).unwrap();
    assert_eq!((account.available_wei.as_str(), account.spent_wei.as_str()), ("980", "15"));

    // Key syntax is checked before any engine call or ledger change.
    let rows = expiry_ledger_rows(&routes.path);
    let seen = calls();
    let too_long = "k".repeat(129);
    for (keys, code) in [
        (vec![b"first".as_slice(), b"second".as_slice()], "invalid_idempotency_key"),
        (vec![b"\xc3\xa9".as_slice()], "invalid_idempotency_key"),
        (vec![b"".as_slice()], "invalid_request_id"),
        (vec![b"bad/key".as_slice()], "invalid_request_id"),
        (vec![b"bad key".as_slice()], "invalid_request_id"),
        (vec![too_long.as_bytes()], "invalid_request_id"),
    ] {
        let (status, answer, _) = chat_route(&routes, &root.bearer, &keys, &body(&prompt, false)).await;
        assert_eq!((status, answer["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some(code)), "{answer}");
    }
    // Each of the other printable ASCII characters is outside the key alphabet, also
    // inside an otherwise valid key.
    let alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._:-";
    let outside: Vec<[u8; 3]> =
        (0x21u8..=0x7e).filter(|byte| !alphabet.as_bytes().contains(byte)).map(|byte| [b'k', byte, b'k']).collect();
    assert_eq!(outside.len(), 94 - alphabet.len());
    for key in &outside {
        let (status, answer, _) = chat_route(&routes, &root.bearer, &[key.as_slice()], &body(&prompt, false)).await;
        assert_eq!(
            (status, answer["error"]["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_request_id")),
            "{}: {answer}",
            String::from_utf8_lossy(key)
        );
    }
    assert_eq!(calls(), seen, "an invalid key reached the engine");
    assert_eq!(expiry_ledger_rows(&routes.path), rows, "an invalid key changed the ledger");
    let longest = format!("{alphabet}{}", &alphabet[..128 - alphabet.len()]);
    assert_eq!(longest.len(), 128);
    let (status, answer, header) = chat_route(&routes, &root.bearer, &[longest.as_bytes()], &body(&prompt, false)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let admitted = routes.ledger().request_view(&a, &longest).unwrap();
    assert_eq!((admitted.request_id.as_str(), header.as_deref()), (longest.as_str(), Some(admitted.id.as_str())));

    // No read or stored file retains the prompt, the rendered template or the output.
    let markers = [CONTENT_PROMPT, CONTENT_RENDERED, "Opaline", "Lantern-4417"];
    let list = format!("/v1/contracts/{a}/requests");
    let request = format!("{list}/{}", original.id);
    let receipt = format!("/v1/contracts/{a}/receipt");
    let mut documents = Vec::new();
    for (public, path) in [(true, "/v1/requests/content-key"), (false, list.as_str()), (false, request.as_str()), (false, receipt.as_str())] {
        let (status, document) = read_route(&routes, public, path, Some(&root.bearer)).await;
        assert_eq!(status, StatusCode::OK, "{path}: {document}");
        documents.push(document);
    }
    for path in [&list, &request, &receipt] {
        let (status, document, _) = routes.owner("GET", path, Vec::new()).await;
        assert_eq!(status, StatusCode::OK, "owner {path}: {document}");
        documents.push(document);
    }
    for document in &documents {
        let text = document.to_string();
        for marker in markers {
            assert!(!text.contains(marker), "a read returned content {marker}");
        }
    }
    let records: Vec<&Value> = [&documents[0], &documents[2], &documents[5]]
        .into_iter()
        .chain(documents[1].as_array().unwrap())
        .chain(documents[3]["payload"]["requests"].as_array().unwrap())
        .collect();
    assert_eq!(records.len(), 3 + 4 + 4);
    for record in records {
        assert_eq!(members(record), REQUEST_MEMBERS);
    }
    for file in files_under(routes._directory.path()) {
        let bytes = std::fs::read(&file).unwrap();
        for marker in markers {
            assert!(!contains(&bytes, marker.as_bytes()), "{} retains {marker}", file.display());
        }
    }
    engine.abort();
}

// inference-request-accounting task 3.1: the production public gateway, driven through its
// router over the paid `CredentialRoutes` ledger (one wei per token). Its private engine is
// a controlled loopback stub: explicit test evidence of what the gateway prepares,
// reserves, dispatches and settles, never a runtime fallback, model qualification or
// observed llama.cpp tokenization and usage. The gateway reads the real clock, so each
// ledger time is bounded by clock samples taken around the call; nothing waits for a clock
// except where a credential must expire on it.

/// The controlled tokenizer's ID of `word`: deterministic and below 1,000,003, except the
/// word `huge-token`, whose ID 2^31 is beyond any engine token ID.
fn controlled_token(word: &str) -> u64 {
    if word == "huge-token" {
        return 1 << 31;
    }
    word.bytes().fold(17, |hash, byte| (hash * 131 + u64::from(byte)) % 1_000_003)
}

/// The controlled tokenization of `text`: one token per whitespace-separated word.
fn controlled_tokens(text: &str) -> Vec<u64> {
    text.split_whitespace().map(controlled_token).collect()
}

/// The controlled chat template: the first message between two marker words, so a chat
/// message of n words is prepared into n + 2 tokens.
fn controlled_rendered(content: &str) -> String {
    format!("<user> {content} <assistant>")
}

/// The value of the word `name=value` in `text`, the controlled engine's instructions.
fn controlled_directive<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.split_whitespace().find_map(|word| word.strip_prefix(name)?.strip_prefix('='))
}

/// What the controlled engine records, its gates and its event channel.
struct ControlledShared {
    calls: Mutex<Vec<String>>,
    templated: Mutex<Vec<Value>>,
    tokenized: Mutex<Vec<Value>>,
    generated: Mutex<Vec<Value>>,
    snapshots: Mutex<Vec<(String, ExpiryRows)>>,
    observe: Option<std::path::PathBuf>,
    gates: Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>,
    events: tokio::sync::mpsc::UnboundedSender<String>,
}

impl ControlledShared {
    fn gate(&self, name: &str) -> Arc<tokio::sync::Semaphore> {
        self.gates.lock().unwrap().entry(name.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(0))).clone()
    }
    /// Records every ledger row as the engine receives `call`, when observing.
    fn observe(&self, call: String) {
        if let Some(path) = &self.observe {
            self.snapshots.lock().unwrap().push((call, expiry_ledger_rows(path)));
        }
    }
    /// Reports `held <gate>`, then waits until the test releases `gate`.
    async fn hold(&self, gate: &str) {
        let _ = self.events.send(format!("held {gate}"));
        self.gate(gate).acquire().await.unwrap().forget();
    }
    /// Reports `held <gate>`, then never continues.
    async fn stall(&self, gate: &str) {
        let _ = self.events.send(format!("held {gate}"));
        std::future::pending::<()>().await;
    }
}

/// Reports `dropped <gate>` when the engine answer it marks is abandoned before its end.
struct ControlledInFlight {
    gate: String,
    events: tokio::sync::mpsc::UnboundedSender<String>,
    finished: std::cell::Cell<bool>,
}

impl Drop for ControlledInFlight {
    fn drop(&mut self) {
        if !self.finished.get() {
            let _ = self.events.send(format!("dropped {}", self.gate));
        }
    }
}

enum ControlledStep {
    Send(String),
    Wait(String),
    Stall(String),
    /// The engine ends its answer here, cleanly, without the rest of the stream.
    End,
}

/// The controlled engine's chat generation, an SSE stream for the gateway's private stream
/// with final usage. The prompt count is the rendered message's token count; the message
/// words instruct the engine:
/// - `out=N`: N output tokens `w0 `, `w1 `, … (default 3), then a `stop` finish;
/// - `usage=exact` (default), `prompt+1`, `prompt-1`, `over` (one more completion token
///   than `max_tokens`), `total+1`, `no-total`, `text` (a string prompt count) or `missing`
///   (no usage event): the final usage event;
/// - `wait=G:P` or `stall=G:P`, with P `before-headers`, `after-output` or `after-usage`:
///   at P, report `held G` and wait for the test to release G, or never continue;
/// - `end=P`, with P `after-output` or `after-usage`: the stream ends at P, without the
///   finish, usage or `[DONE]` that would follow;
/// - `generation-fails`: the generation is refused with a 500 before any output.
///
/// An answer abandoned before its end reports `dropped G`.
async fn controlled_chat(shared: Arc<ControlledShared>, headers: axum::http::HeaderMap, body: Value)
    -> axum::response::Response {
    use axum::response::IntoResponse;
    let id = headers.get("x-request-id").map(|value| value.to_str().unwrap().to_owned()).unwrap_or_default();
    shared.observe(format!("POST /v1/chat/completions id={id}"));
    shared.generated.lock().unwrap().push(body.clone());
    let content = body["messages"][0]["content"].as_str().unwrap_or_default().to_owned();
    if content.split_whitespace().any(|word| word == "generation-fails") {
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"error": {"code": 500, "message": "controlled engine failure"}}))).into_response();
    }
    let prompt = controlled_tokens(&controlled_rendered(&content)).len() as u64;
    let maximum = body["max_tokens"].as_u64().unwrap_or_default();
    let output: u64 = controlled_directive(&content, "out").map_or(3, |count| count.parse().unwrap());
    let (mode, gate, point) = match (controlled_directive(&content, "wait"), controlled_directive(&content, "stall")) {
        (Some(plan), None) => ("wait", plan.split_once(':').unwrap().0, plan.split_once(':').unwrap().1),
        (None, Some(plan)) => ("stall", plan.split_once(':').unwrap().0, plan.split_once(':').unwrap().1),
        (None, None) => ("none", "unnamed", "none"),
        _ => panic!("one hold per request"),
    };
    let marker = ControlledInFlight { gate: gate.to_owned(), events: shared.events.clone(), finished: Default::default() };
    if point == "before-headers" {
        if mode == "stall" {
            shared.stall(gate).await;
        }
        shared.hold(gate).await;
    }
    let chunk = |choices: Value| format!("data: {}\n\n", json!({"id": "engine-response",
        "object": "chat.completion.chunk", "model": body["model"], "created": 1, "choices": choices}));
    let hold = |at: &str| match (mode, point == at) {
        ("wait", true) => Some(ControlledStep::Wait(gate.to_owned())),
        ("stall", true) => Some(ControlledStep::Stall(gate.to_owned())),
        _ => None,
    };
    let end = |at: &str| (controlled_directive(&content, "end") == Some(at)).then_some(ControlledStep::End);
    let mut steps: Vec<ControlledStep> = (0..output)
        .map(|index| ControlledStep::Send(chunk(json!([{"index": 0, "delta": {"content": format!("w{index} ")},
            "finish_reason": null}]))))
        .collect();
    steps.extend(hold("after-output"));
    steps.extend(end("after-output"));
    steps.push(ControlledStep::Send(chunk(json!([{"index": 0, "delta": {}, "finish_reason": "stop"}]))));
    let usage = match controlled_directive(&content, "usage").unwrap_or("exact") {
        "exact" => Some(json!({"prompt_tokens": prompt, "completion_tokens": output, "total_tokens": prompt + output})),
        "prompt+1" => Some(json!({"prompt_tokens": prompt + 1, "completion_tokens": output, "total_tokens": prompt + 1 + output})),
        "prompt-1" => Some(json!({"prompt_tokens": prompt - 1, "completion_tokens": output, "total_tokens": prompt - 1 + output})),
        "over" => Some(json!({"prompt_tokens": prompt, "completion_tokens": maximum + 1, "total_tokens": prompt + maximum + 1})),
        "total+1" => Some(json!({"prompt_tokens": prompt, "completion_tokens": output, "total_tokens": prompt + output + 1})),
        "no-total" => Some(json!({"prompt_tokens": prompt, "completion_tokens": output})),
        "text" => Some(json!({"prompt_tokens": prompt.to_string(), "completion_tokens": output, "total_tokens": prompt + output})),
        "missing" => None,
        other => panic!("unknown usage instruction {other}"),
    };
    if let Some(usage) = usage {
        let mut event = json!({"id": "engine-response", "object": "chat.completion.chunk", "model": body["model"],
            "created": 1, "choices": []});
        event["usage"] = usage;
        steps.push(ControlledStep::Send(format!("data: {event}\n\n")));
    }
    steps.extend(hold("after-usage"));
    steps.extend(end("after-usage"));
    steps.push(ControlledStep::Send("data: [DONE]\n\n".into()));
    let stream = futures_util::stream::unfold((steps.into_iter(), shared, marker), |(mut steps, shared, marker)| async move {
        loop {
            match steps.next() {
                None | Some(ControlledStep::End) => {
                    marker.finished.set(true);
                    return None;
                }
                Some(ControlledStep::Send(text)) => {
                    return Some((Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(text)), (steps, shared, marker)))
                }
                Some(ControlledStep::Wait(gate)) => shared.hold(&gate).await,
                Some(ControlledStep::Stall(gate)) => shared.stall(&gate).await,
            }
        }
    });
    ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], axum::body::Body::from_stream(stream)).into_response()
}

/// The controlled engine's text completion of the prompt's token IDs: three output tokens
/// with their IDs, then a `stop` finish carrying exact usage.
async fn controlled_completion(shared: Arc<ControlledShared>, headers: axum::http::HeaderMap, body: Value)
    -> axum::response::Response {
    use axum::response::IntoResponse;
    let id = headers.get("x-request-id").map(|value| value.to_str().unwrap().to_owned()).unwrap_or_default();
    shared.observe(format!("POST /v1/completions id={id}"));
    shared.generated.lock().unwrap().push(body.clone());
    let prompt = body["prompt"].as_array().map_or(0, Vec::len) as u64;
    let chunk = |text: &str, finish: Value, tokens: Value| json!({"id": "engine-response", "object": "text_completion",
        "model": body["model"], "created": 1, "choices": [{"index": 0, "text": text, "finish_reason": finish, "logprobs": null}],
        "__verbose": {"tokens": tokens}});
    let mut text: String = (0..3u64)
        .map(|index| format!("data: {}\n\n", chunk(&format!("w{index} "), Value::Null, json!([500 + index]))))
        .collect();
    let mut finish = chunk("", json!("stop"), json!([]));
    finish["__verbose"]["truncated"] = json!(false);
    finish["usage"] = json!({"prompt_tokens": prompt, "completion_tokens": 3, "total_tokens": prompt + 3});
    text.push_str(&format!("data: {finish}\n\ndata: [DONE]\n\n"));
    ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], text).into_response()
}

/// The controlled engine's embeddings: one vector per token-ID input and usage counting
/// every input token, or one more when an input holds the token of `embedding-miscount`.
async fn controlled_embedding(shared: Arc<ControlledShared>, headers: axum::http::HeaderMap, body: Value)
    -> axum::response::Response {
    use axum::response::IntoResponse;
    let id = headers.get("x-request-id").map(|value| value.to_str().unwrap().to_owned()).unwrap_or_default();
    shared.observe(format!("POST /v1/embeddings id={id}"));
    shared.generated.lock().unwrap().push(body.clone());
    let inputs = body["input"].as_array().cloned().unwrap_or_default();
    let mut tokens: u64 = inputs.iter().map(|input| input.as_array().map_or(0, Vec::len) as u64).sum();
    let miscount = controlled_token("embedding-miscount");
    if inputs.iter().filter_map(Value::as_array).flatten().any(|token| token.as_u64() == Some(miscount)) {
        tokens += 1;
    }
    let data: Vec<Value> = (0..inputs.len())
        .map(|index| json!({"index": index, "object": "embedding", "embedding": [0.5]}))
        .collect();
    axum::Json(json!({"model": body["model"], "object": "list", "data": data,
        "usage": {"prompt_tokens": tokens, "total_tokens": tokens}})).into_response()
}

/// The records an engine call left, taken by `ControlledEngine::take`.
#[derive(Debug, PartialEq)]
struct ControlledRecords {
    calls: Vec<String>,
    templated: Vec<Value>,
    tokenized: Vec<Value>,
    generated: Vec<Value>,
    snapshots: Vec<(String, ExpiryRows)>,
}

/// A controlled llama.cpp-shaped private engine on a loopback listener. It records every
/// call as `METHOD path` (generation calls with ` id=<X-Request-Id>`), the template,
/// tokenization and generation bodies and, when observing, every ledger row at each
/// tokenization and generation call. `/apply-template` renders with `controlled_rendered`,
/// or answers 500 for a message containing the word `template-fails`. `/tokenize` prepares
/// with `controlled_tokens`, answers 500 for `tokenize-fails`, and for `tokenize-wait=G`
/// reports `held G` and waits for G first. `template-delay-ms=N` and `tokenize-delay-ms=N`
/// delay those answers by N ms. Generation follows `controlled_chat`,
/// `controlled_completion` and `controlled_embedding`.
struct ControlledEngine {
    base: String,
    shared: Arc<ControlledShared>,
    events: tokio::sync::mpsc::UnboundedReceiver<String>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for ControlledEngine {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ControlledEngine {
    async fn start(observe: Option<std::path::PathBuf>) -> Self {
        use axum::{response::IntoResponse, routing::{get, post}, Json, Router};
        let (events, receiver) = tokio::sync::mpsc::unbounded_channel();
        let shared = Arc::new(ControlledShared {
            calls: Default::default(), templated: Default::default(), tokenized: Default::default(),
            generated: Default::default(), snapshots: Default::default(), observe,
            gates: Default::default(), events,
        });
        let failure = || (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"code": 500, "message": "controlled engine failure"}}))).into_response();
        let (template, tokenize, chat, completion, embedding, recorded) =
            (shared.clone(), shared.clone(), shared.clone(), shared.clone(), shared.clone(), shared.clone());
        let engine = Router::new()
            .route("/health", get(|| async { axum::http::StatusCode::OK }))
            .route("/apply-template", post(move |Json(body): Json<Value>| {
                let shared = template.clone();
                async move {
                    shared.templated.lock().unwrap().push(body.clone());
                    let content = body["messages"][0]["content"].as_str().unwrap_or_default();
                    if content.split_whitespace().any(|word| word == "template-fails") {
                        return failure();
                    }
                    if let Some(delay) = controlled_directive(content, "template-delay-ms") {
                        tokio::time::sleep(Duration::from_millis(delay.parse().unwrap())).await;
                    }
                    Json(json!({"prompt": controlled_rendered(content)})).into_response()
                }
            }))
            .route("/tokenize", post(move |Json(body): Json<Value>| {
                let shared = tokenize.clone();
                async move {
                    shared.observe("POST /tokenize".into());
                    shared.tokenized.lock().unwrap().push(body.clone());
                    let text = body["content"].as_str().unwrap_or_default().to_owned();
                    if text.split_whitespace().any(|word| word == "tokenize-fails") {
                        return failure();
                    }
                    if let Some(delay) = controlled_directive(&text, "tokenize-delay-ms") {
                        tokio::time::sleep(Duration::from_millis(delay.parse().unwrap())).await;
                    }
                    if let Some(gate) = controlled_directive(&text, "tokenize-wait") {
                        shared.hold(gate).await;
                    }
                    Json(json!({"tokens": controlled_tokens(&text)})).into_response()
                }
            }))
            .route("/v1/chat/completions", post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                controlled_chat(chat.clone(), headers, body)
            }))
            .route("/v1/completions", post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                controlled_completion(completion.clone(), headers, body)
            }))
            .route("/v1/embeddings", post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                controlled_embedding(embedding.clone(), headers, body)
            }))
            .fallback(|| async { axum::http::StatusCode::NOT_FOUND })
            .layer(axum::middleware::from_fn(move |request: axum::extract::Request, next: axum::middleware::Next| {
                let shared = recorded.clone();
                async move {
                    let path = request.uri().path().to_owned();
                    let mut call = format!("{} {path}", request.method());
                    if path.starts_with("/v1/") {
                        let id = request.headers().get("x-request-id").map(|value| value.to_str().unwrap().to_owned());
                        call.push_str(&format!(" id={}", id.unwrap_or_default()));
                    }
                    shared.calls.lock().unwrap().push(call);
                    next.run(request).await
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, engine).await.unwrap() });
        Self { base, shared, events: receiver, task }
    }

    /// Takes everything recorded since the last take.
    fn take(&self) -> ControlledRecords {
        let take = |values: &Mutex<Vec<Value>>| std::mem::take(&mut *values.lock().unwrap());
        ControlledRecords {
            calls: std::mem::take(&mut *self.shared.calls.lock().unwrap()),
            templated: take(&self.shared.templated),
            tokenized: take(&self.shared.tokenized),
            generated: take(&self.shared.generated),
            snapshots: std::mem::take(&mut *self.shared.snapshots.lock().unwrap()),
        }
    }

    /// Asserts that nothing reached the engine since the last take.
    fn untouched(&self, context: &str) {
        let records = self.take();
        assert!(records.calls.is_empty(), "{context}: the engine received {:?}", records.calls);
    }

    /// Lets the request held at `gate` continue.
    fn release(&self, gate: &str) {
        self.shared.gate(gate).add_permits(1);
    }

    /// The next engine event, within 30 seconds.
    async fn event(&mut self) -> String {
        tokio::time::timeout(Duration::from_secs(30), self.events.recv())
            .await
            .expect("no engine event within 30 s")
            .unwrap()
    }

    /// The next `count` engine events, sorted.
    async fn events(&mut self, count: usize) -> Vec<String> {
        let mut events = Vec::new();
        for _ in 0..count {
            events.push(self.event().await);
        }
        events.sort();
        events
    }
}

/// The paid `CredentialRoutes` fixture (1,000 wei funded, 995 available, one wei per token,
/// four concurrent requests) whose agreement advertises chat, completions and embeddings,
/// served by a `ControlledEngine` with a generation deadline of `deadline_secs`, and one
/// public router for the whole test, so that cancellation reaches its in-flight requests.
struct GatewayFixture {
    routes: CredentialRoutes,
    app: axum::Router,
    engine: ControlledEngine,
    root: Credential,
}

async fn gateway_fixture(observe: bool, deadline_secs: u64) -> GatewayFixture {
    let mut routes = CredentialRoutes::opened_with(1000, 4, protocol::now().unwrap(), |agreement| {
        agreement.model.capabilities = vec!["chat".into(), "completions".into(), "embeddings".into()];
    });
    let engine = ControlledEngine::start(observe.then(|| routes.path.clone())).await;
    {
        let state = Arc::get_mut(&mut routes.state).unwrap();
        let served = &mut Arc::get_mut(&mut state.config).unwrap().models[0];
        served.engine_url = engine.base.clone();
        served.request_timeout_secs = deadline_secs;
    }
    let app = crate::inference_gateway::router(routes.state.clone());
    let root = routes.opened.credential.clone().unwrap();
    GatewayFixture { routes, app, engine, root }
}

/// A request to the public router with `bearer`, an optional `Idempotency-Key` and an
/// optional JSON body.
fn gateway_request(method: &str, path: &str, bearer: &str, key: Option<&str>, body: Option<&Value>)
    -> axum::http::Request<axum::body::Body> {
    let mut request = axum::http::Request::builder().method(method).uri(path)
        .header("authorization", format!("Bearer {bearer}"));
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    match body {
        Some(body) => request.header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string())).unwrap(),
        None => request.body(axum::body::Body::empty()).unwrap(),
    }
}

/// A chat request for the fixture's model with `content` as its one message.
fn gateway_chat(bearer: &str, key: &str, content: &str, max_tokens: u64, stream: bool)
    -> axum::http::Request<axum::body::Body> {
    gateway_request("POST", "/v1/chat/completions", bearer, Some(key), Some(&json!({"model": "test/model",
        "messages": [{"role": "user", "content": content}], "max_tokens": max_tokens, "stream": stream})))
}

/// `gateway_chat`, streaming with `stream_options.include_usage`, so that the client sees
/// the final usage event once the gateway has read it.
fn gateway_usage_stream(bearer: &str, key: &str, content: &str) -> axum::http::Request<axum::body::Body> {
    gateway_request("POST", "/v1/chat/completions", bearer, Some(key), Some(&json!({"model": "test/model",
        "messages": [{"role": "user", "content": content}], "max_tokens": 4, "stream": true,
        "stream_options": {"include_usage": true}})))
}

/// The status, `x-request-id` and text of a complete answer, which never holds a bearer.
async fn gateway_answer(response: axum::response::Response) -> (axum::http::StatusCode, Option<String>, String) {
    let status = response.status();
    let header = response.headers().get("x-request-id").map(|value| value.to_str().unwrap().to_owned());
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains("chiral_"), "an answer held a bearer");
    (status, header, text)
}

/// The text of a streamed answer up to its end, and whether it ended with an error.
async fn gateway_stream_end(body: &mut axum::body::BodyDataStream, text: &mut String) -> bool {
    use futures_util::StreamExt;
    loop {
        match tokio::time::timeout(Duration::from_secs(30), body.next()).await.expect("the stream neither ended nor failed") {
            Some(Ok(bytes)) => text.push_str(std::str::from_utf8(&bytes).unwrap()),
            Some(Err(_)) => return true,
            None => return false,
        }
    }
}

/// Reads a streamed answer until its text contains `marker`.
async fn gateway_stream_until(body: &mut axum::body::BodyDataStream, text: &mut String, marker: &str) {
    use futures_util::StreamExt;
    while !text.contains(marker) {
        let bytes = tokio::time::timeout(Duration::from_secs(30), body.next()).await
            .unwrap_or_else(|_| panic!("no {marker} within 30 s: {text}"))
            .unwrap_or_else(|| panic!("the stream ended before {marker}: {text}"))
            .unwrap();
        text.push_str(std::str::from_utf8(&bytes).unwrap());
    }
}

/// The JSON events of a public SSE text, without `[DONE]`.
fn gateway_events(text: &str) -> Vec<Value> {
    text.split("\n\n").filter_map(|event| event.strip_prefix("data: ")).filter(|data| *data != "[DONE]")
        .map(|data| serde_json::from_str(data).unwrap()).collect()
}

impl GatewayFixture {
    fn id(&self) -> String {
        self.routes.id()
    }
    fn rows(&self) -> ExpiryRows {
        expiry_ledger_rows(&self.routes.path)
    }
    fn view(&self, key: &str) -> RequestView {
        self.routes.ledger().request_view(&self.id(), key).unwrap()
    }
    fn recorded(&self, key: &str) -> bool {
        self.routes.ledger().request_view(&self.id(), key).is_ok()
    }
    async fn call(&self, request: axum::http::Request<axum::body::Body>) -> axum::response::Response {
        use tower::ServiceExt;
        self.app.clone().oneshot(request).await.unwrap()
    }
    /// Sends `request` from a task, as a client that may go away.
    fn spawn(&self, request: axum::http::Request<axum::body::Body>) -> tokio::task::JoinHandle<axum::response::Response> {
        use tower::ServiceExt;
        let app = self.app.clone();
        tokio::spawn(async move { app.oneshot(request).await.unwrap() })
    }
    /// The request of `key` once it has left `reserved`, within 30 seconds.
    async fn settled(&self, key: &str) -> RequestView {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let view = self.view(key);
            if view.status != "reserved" {
                return view;
            }
            assert!(tokio::time::Instant::now() < deadline, "{key} never settled");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    /// Sends the cancellation of `target` with `bearer`: status, `x-request-id` and answer.
    async fn cancel(&self, bearer: &str, target: &str) -> (axum::http::StatusCode, Option<String>, Value) {
        let (status, header, text) = gateway_answer(self.call(gateway_request("POST",
            &format!("/v1/requests/{target}/cancel"), bearer, None, None)).await).await;
        (status, header, serde_json::from_str(&text).unwrap())
    }
    /// `GET /v1/requests/<target>` with `bearer`: status and answer.
    async fn status(&self, bearer: &str, target: &str) -> (axum::http::StatusCode, Value) {
        let (status, _, text) = gateway_answer(self.call(gateway_request("GET",
            &format!("/v1/requests/{target}"), bearer, None, None)).await).await;
        (status, serde_json::from_str(&text).unwrap())
    }
    /// Sends a chat request of `key` with each bearer, once non-streaming with `content`, and
    /// with the first bearer once streaming with another message, and asserts that each is
    /// answered with exactly the 409 naming the key's request and its current status, and
    /// that neither the engine nor any ledger row saw anything of it.
    async fn duplicates(&self, key: &str, content: &str, bearers: &[&str]) {
        let view = self.view(key);
        let conflict = json!({"error": {
            "message": "Request already exists and will not be replayed. Retrieve its status; response content is not retained.",
            "type": "invalid_request_error", "param": null, "code": "duplicate_request",
            "request_id": view.id, "request_status": view.status,
        }});
        let rows = self.rows();
        self.engine.untouched(key);
        let mut attempts: Vec<(&str, String, bool)> = bearers.iter().map(|bearer| (*bearer, content.to_owned(), false)).collect();
        attempts.push((bearers[0], format!("another message for {key}"), true));
        for (bearer, content, stream) in attempts {
            let (status, header, text) = gateway_answer(self.call(gateway_chat(bearer, key, &content, 4, stream)).await).await;
            assert_eq!((status, serde_json::from_str::<Value>(&text).unwrap()), (axum::http::StatusCode::CONFLICT, conflict.clone()),
                "{key}: stream {stream}");
            assert_eq!(header.as_deref(), Some(view.id.as_str()), "{key}");
        }
        self.engine.untouched(key);
        assert_eq!(self.rows(), rows, "{key}: a duplicate changed the ledger");
    }
}

/// `before` with the gateway's reservation of `key` by `credential` added: exactly a
/// reservation of `prompt` input and `output` maximum output tokens holding `maximum` wei,
/// whose opaque identifier, account and creation time are read from the request's row in
/// `after`, where its identity must equal those literals; the creation time must lie within
/// the clock samples `window`. `maximum` moves from the account's available credit to the
/// account's and the credential's holds. Amounts are the caller's fixed literals.
#[allow(clippy::too_many_arguments)]
fn gateway_reserved(before: &ExpiryRows, after: &ExpiryRows, key: &str, credential: &str,
    prompt: u64, output: u64, maximum: i128, window: (u64, u64)) -> ExpiryRows {
    let found = after["requests"].iter().find(|row| row[2] == key)
        .unwrap_or_else(|| panic!("{key} was not reserved"));
    let id = found[0].as_str().unwrap().to_owned();
    let contract = found[1].as_str().unwrap().to_owned();
    let created = found[5].as_u64().unwrap();
    assert!(window.0 <= created && created <= window.1, "{key}: created at {created}, outside {window:?}");
    assert!(opaque_request_id(&id), "{key}");
    let row = json!([id, contract, key, credential, "reserved", created, {
        "id": id, "request_id": key, "contract_id": contract, "credential_id": credential, "status": "reserved",
        "usage": {"prompt_tokens": 0, "completion_tokens": 0}, "cost_wei": "0", "max_cost_wei": maximum.to_string(),
        "provider_loss_wei": "0", "prompt_tokens_reserved": prompt, "max_output_tokens": output,
        "created_at": created, "settled_at": null, "performance": null, "waived_wei": "0"}]);
    assert_eq!((&found[3], request_identity(&found[6])), (&row[3], request_identity(&row[6])), "{key}: reserved identity");
    let mut expected = before.clone();
    let requests = expected.get_mut("requests").unwrap();
    requests.push(row);
    requests.sort_by(|a, b| a[0].as_str().cmp(&b[0].as_str()));
    let account = &mut expiry_row(&mut expected, "accounts", &contract)[3];
    expiry_shift(&mut account["available_wei"], -maximum);
    expiry_shift(&mut account["reserved_wei"], maximum);
    expiry_shift(&mut expiry_row(&mut expected, "credentials", credential)[3]["reserved_wei"], maximum);
    expected
}

/// `before`, in which request `id` is reserved, with the gateway's settlement as `status`
/// applied by `expect_terminal` from the caller's literals: `known` usage and charge, or a
/// whole release. The settlement time read from `after` must lie within the clock samples
/// `window`. The gateway's unqualified performance record is copied from `after` once its
/// members other than the three measured durations are checked.
fn gateway_settled(before: &ExpiryRows, after: &ExpiryRows, id: &str, status: &str,
    known: Option<(u64, u64, i128)>, window: (u64, u64)) -> ExpiryRows {
    let record = &after["requests"].iter().find(|row| row[0] == id)
        .unwrap_or_else(|| panic!("no request {id}"))[6];
    let settled_at = record["settled_at"].as_u64().unwrap_or_else(|| panic!("{id} is not settled"));
    assert!(window.0 <= settled_at && settled_at <= window.1, "{id}: settled at {settled_at}, outside {window:?}");
    let performance = record["performance"].clone();
    let mut outcome = performance.clone();
    for duration in ["ttft_us", "decode_us", "elapsed_us"] {
        assert!(outcome[duration].is_null() || outcome[duration].is_u64(), "{id}: {performance}");
        outcome[duration] = Value::Null;
    }
    assert_eq!(outcome, json!({"evidence_sha256": "", "policy": "unqualified", "eligibility": "unqualified",
        "ttft_us": null, "decode_us": null, "elapsed_us": null, "output_tokens": known.map(|(_, output, _)| output),
        "attained": null, "remedy": "none"}), "{id}");
    let mut expected = before.clone();
    expect_terminal(&mut expected, id, status, known, settled_at);
    expiry_row(&mut expected, "requests", id)[6]["performance"] = performance;
    expected
}

/// Task 3.1 (a), the gateway's exact preparation. *Maximum reservation and nominal usage
/// cost round once*: "Exact prompt/template tokenization and supported bounded options SHALL
/// precede reservation"; *Reservation atomically binds authority limits and credit*: "Before
/// paid engine dispatch the provider SHALL durably create the request and reserve M" and "A
/// definite reservation refusal or failed reservation transaction SHALL leave no partial
/// request or hold and SHALL NOT dispatch inference".
///
/// The controlled engine records every ledger row when tokenization and generation reach
/// it. A chat message of five words is rendered by the engine's template and tokenized into
/// exactly seven tokens; while it is tokenized no row has changed, and when its generation
/// arrives, carrying the opaque identifier, the ledger already holds exactly that request
/// with seven prepared input and four output tokens and an 11-wei hold, nothing else. A text
/// completion of three words dispatches exactly the three token IDs the tokenizer returned
/// (an 8-wei hold); an embedding batch of two inputs dispatches each input's IDs and holds
/// its five input tokens only (5 wei, no output budget). Each then settles at its validated
/// usage (10, 6 and 5 wei) with complete rows derived from fixed literals.
///
/// A failing template, a failing tokenizer, a token ID beyond the engine's range, a model
/// whose qualification is withdrawn while its prompt is tokenized, admission closed while
/// it is tokenized and a tokenization that outlives the deadline (504 `tokenization_timeout`
/// or, when the engine client's equal timeout ends it first, 502) each answer an error object
/// with no request recorded, no ledger row changed and no generation dispatched; only the
/// preparation steps named reach the engine. A template and a tokenization that each answer
/// within the engine client's timeout but together outlive the deadline are refused by the
/// gateway's deadline alone: 504 `tokenization_timeout`, nothing recorded or dispatched.
#[tokio::test]
async fn gateway_reserves_and_dispatches_exactly_the_prepared_prompt_and_failed_preparation_holds_nothing() {
    use axum::http::StatusCode;
    use std::sync::atomic::Ordering;
    let mut fixture = gateway_fixture(true, 60).await;
    let root = fixture.root.clone();
    let model = fixture.routes.agreement.payload.model.id.clone();

    // Chat: the template's rendering is tokenized into seven tokens, then reserved.
    let content = "alpha beta gamma delta epsilon";
    let rendered = controlled_rendered(content);
    assert_eq!(controlled_tokens(&rendered).len(), 7);
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let (status, header, text) = gateway_answer(fixture.call(gateway_chat(&root.bearer, "prepared-chat", content, 4, false)).await).await;
    let end = protocol::now().unwrap();
    assert_eq!(status, StatusCode::OK, "{text}");
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer["choices"][0]["message"]["content"], "w0 w1 w2 ");
    assert_eq!(answer["usage"], json!({"prompt_tokens": 7, "completion_tokens": 3, "total_tokens": 10}));
    let id = fixture.view("prepared-chat").id;
    assert_eq!(header.as_deref(), Some(id.as_str()));
    let records = fixture.engine.take();
    let dispatch = format!("POST /v1/chat/completions id={id}");
    assert_eq!(records.calls, ["GET /health", "POST /apply-template", "POST /tokenize", dispatch.as_str()]);
    assert_eq!(records.templated.len(), 1);
    assert_eq!(records.templated[0]["messages"], json!([{"role": "user", "content": content}]));
    assert_eq!(records.tokenized, [json!({"content": rendered, "add_special": true, "parse_special": true})]);
    assert_eq!(records.generated.len(), 1);
    let generated = &records.generated[0];
    assert_eq!((&generated["model"], &generated["messages"], &generated["max_tokens"], &generated["n"], &generated["stream"],
        &generated["stream_options"]), (&json!(model), &records.templated[0]["messages"], &json!(4), &json!(1), &json!(true),
        &json!({"include_usage": true})));
    let reserved = gateway_reserved(&rows, &records.snapshots[1].1, "prepared-chat", &root.id, 7, 4, 11, (start, end));
    assert_eq!(records.snapshots, [("POST /tokenize".to_owned(), rows.clone()), (dispatch, reserved.clone())],
        "the ledger was not exactly the prepared reservation at tokenization and dispatch");
    assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &id, "completed", Some((7, 3, 10)), (start, end)));

    // Text completion: exactly the tokenizer's IDs are reserved and dispatched.
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let body = json!({"model": model, "prompt": "one two three", "max_tokens": 5});
    let (status, header, text) = gateway_answer(fixture.call(gateway_request("POST", "/v1/completions", &root.bearer,
        Some("prepared-completion"), Some(&body))).await).await;
    let end = protocol::now().unwrap();
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["choices"][0]["text"], "w0 w1 w2 ");
    let id = fixture.view("prepared-completion").id;
    assert_eq!(header.as_deref(), Some(id.as_str()));
    let records = fixture.engine.take();
    let dispatch = format!("POST /v1/completions id={id}");
    assert_eq!(records.calls, ["GET /health", "POST /tokenize", dispatch.as_str()]);
    assert_eq!(records.tokenized, [json!({"content": "one two three", "add_special": true, "parse_special": true})]);
    assert_eq!(records.generated.len(), 1);
    assert_eq!(records.generated[0]["prompt"], json!(controlled_tokens("one two three")), "the dispatched prompt");
    let reserved = gateway_reserved(&rows, &records.snapshots[1].1, "prepared-completion", &root.id, 3, 5, 8, (start, end));
    assert_eq!(records.snapshots, [("POST /tokenize".to_owned(), rows.clone()), (dispatch, reserved.clone())]);
    assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &id, "completed", Some((3, 3, 6)), (start, end)));

    // Embeddings: each input's IDs are dispatched; only their five input tokens are held.
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let inputs = ["red green", "blue cyan magenta"];
    let body = json!({"model": model, "input": inputs});
    let (status, header, text) = gateway_answer(fixture.call(gateway_request("POST", "/v1/embeddings", &root.bearer,
        Some("prepared-embedding"), Some(&body))).await).await;
    let end = protocol::now().unwrap();
    assert_eq!(status, StatusCode::OK, "{text}");
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!((answer["data"].as_array().unwrap().len(), &answer["usage"]),
        (2, &json!({"prompt_tokens": 5, "completion_tokens": 0, "total_tokens": 5})));
    let id = fixture.view("prepared-embedding").id;
    assert_eq!(header.as_deref(), Some(id.as_str()));
    let records = fixture.engine.take();
    let dispatch = format!("POST /v1/embeddings id={id}");
    assert_eq!(records.calls, ["GET /health", "POST /tokenize", "POST /tokenize", dispatch.as_str()]);
    assert_eq!(records.tokenized, inputs.map(|input| json!({"content": input, "add_special": true, "parse_special": true})));
    assert_eq!(records.generated.len(), 1);
    assert_eq!(records.generated[0]["input"], json!(inputs.map(controlled_tokens)), "the dispatched inputs");
    let reserved = gateway_reserved(&rows, &records.snapshots[2].1, "prepared-embedding", &root.id, 5, 0, 5, (start, end));
    assert_eq!(records.snapshots, [("POST /tokenize".to_owned(), rows.clone()), ("POST /tokenize".to_owned(), rows.clone()),
        (dispatch, reserved.clone())]);
    assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &id, "completed", Some((5, 0, 5)), (start, end)));

    // Failed preparation reserves and dispatches nothing.
    let refused = |records: &ControlledRecords, rows: &ExpiryRows, key: &str, calls: &[&str]| {
        assert_eq!(records.calls, calls, "{key}");
        assert!(records.generated.is_empty(), "{key}: dispatched");
        assert!(records.snapshots.iter().all(|(_, observed)| observed == rows), "{key}: the ledger changed during preparation");
    };
    for (key, path, body, status, code, calls) in [
        ("template-failure", "/v1/chat/completions",
            json!({"model": model, "messages": [{"role": "user", "content": "template-fails here"}], "max_tokens": 4}),
            StatusCode::BAD_GATEWAY, "engine_error", &["GET /health", "POST /apply-template"][..]),
        ("tokenizer-failure", "/v1/chat/completions",
            json!({"model": model, "messages": [{"role": "user", "content": "tokenize-fails here"}], "max_tokens": 4}),
            StatusCode::BAD_GATEWAY, "engine_error", &["GET /health", "POST /apply-template", "POST /tokenize"][..]),
        ("token-out-of-range", "/v1/completions", json!({"model": model, "prompt": "huge-token here", "max_tokens": 4}),
            StatusCode::BAD_GATEWAY, "engine_error", &["GET /health", "POST /tokenize"][..]),
    ] {
        let rows = fixture.rows();
        let (actual, _, text) = gateway_answer(fixture.call(gateway_request("POST", path, &root.bearer, Some(key), Some(&body))).await).await;
        let answer: Value = serde_json::from_str(&text).unwrap();
        assert_eq!((actual, answer["error"]["code"].as_str()), (status, Some(code)), "{key}: {text}");
        assert_eq!(answer.as_object().unwrap().keys().collect::<Vec<_>>(), ["error"], "{key}");
        refused(&fixture.engine.take(), &rows, key, calls);
        assert!(!fixture.recorded(key), "{key}: recorded");
        assert_eq!(fixture.rows(), rows, "{key}: the ledger changed");
    }
    let state = fixture.routes.state.clone();
    for (key, status, code) in [
        ("withdrawn-while-tokenized", StatusCode::SERVICE_UNAVAILABLE, "model_unqualified"),
        ("paused-while-tokenized", StatusCode::SERVICE_UNAVAILABLE, "provider_paused"),
    ] {
        let rows = fixture.rows();
        let pending = fixture.spawn(gateway_chat(&root.bearer, key, &format!("waiting tokenize-wait={key}"), 4, false));
        assert_eq!(fixture.engine.event().await, format!("held {key}"));
        if code == "model_unqualified" {
            state.qualified_models.write().unwrap().remove(&model);
        } else {
            state.accepting.store(false, Ordering::Release);
        }
        fixture.engine.release(key);
        let (actual, _, text) = gateway_answer(pending.await.unwrap()).await;
        assert_eq!((actual, serde_json::from_str::<Value>(&text).unwrap()["error"]["code"].as_str()), (status, Some(code)), "{key}: {text}");
        refused(&fixture.engine.take(), &rows, key, &["GET /health", "POST /apply-template", "POST /tokenize"]);
        assert!(!fixture.recorded(key), "{key}: recorded");
        assert_eq!(fixture.rows(), rows, "{key}: the ledger changed");
        state.qualified_models.write().unwrap().insert(model.clone());
        state.accepting.store(true, Ordering::Release);
    }

    // A tokenization that outlives the one-second deadline. The engine client's own request
    // timeout equals that deadline, so whichever ends the stalled tokenization first answers.
    let fixture = gateway_fixture(true, 1).await;
    let rows = fixture.rows();
    let (status, _, text) = gateway_answer(fixture.call(gateway_chat(&fixture.root.bearer, "tokenization-deadline",
        "waiting tokenize-wait=never", 4, false)).await).await;
    let outcome = (status, serde_json::from_str::<Value>(&text).unwrap()["error"]["code"].as_str().map(str::to_owned));
    assert!([(StatusCode::GATEWAY_TIMEOUT, Some("tokenization_timeout".to_owned())),
        (StatusCode::BAD_GATEWAY, Some("engine_error".to_owned()))].contains(&outcome), "{text}");
    refused(&fixture.engine.take(), &rows, "tokenization-deadline", &["GET /health", "POST /apply-template", "POST /tokenize"]);
    assert!(!fixture.recorded("tokenization-deadline"));
    assert_eq!(fixture.rows(), rows);

    // Review addition: the gateway's own deadline, not the engine client's. The template and
    // the tokenizer each answer after 1.2 s, within the engine client's two-second timeout, but
    // together outlive the gateway's two-second deadline, which alone refuses the request.
    let fixture = gateway_fixture(true, 2).await;
    let rows = fixture.rows();
    let (status, _, text) = gateway_answer(fixture.call(gateway_chat(&fixture.root.bearer, "slow-preparation",
        "template-delay-ms=1200 tokenize-delay-ms=1200", 4, false)).await).await;
    assert_eq!((status, serde_json::from_str::<Value>(&text).unwrap()["error"]["code"].as_str()),
        (StatusCode::GATEWAY_TIMEOUT, Some("tokenization_timeout")), "{text}");
    refused(&fixture.engine.take(), &rows, "slow-preparation", &["GET /health", "POST /apply-template", "POST /tokenize"]);
    assert!(!fixture.recorded("slow-preparation"));
    assert_eq!(fixture.rows(), rows);
}

/// Task 3.1 (b), *Request identity prevents automatic paid replay* at the gateway: "Once
/// reserved, an account/key SHALL NOT create another execution or hold", and "A public
/// duplicate SHALL return HTTP 409 with `duplicate_request`, the original request identifier
/// and its current status". Each duplicate below is sent by every `infer` bearer of the
/// account, non-streaming with the original message and streaming with another, and gets
/// exactly that 409 with the original `x-request-id`, no engine call and no row change.
///
/// - In flight: a request held after its output is duplicated as `reserved`, then completes
///   once (7 of 8 wei) and is duplicated as `completed`.
/// - Racing: two requests with one key (root and application credentials, different
///   prompts) are both past the gateway's duplicate check and held at tokenization; once
///   released, exactly one is reserved and dispatched, and the other gets the ledger's
///   refusal as the same 409 naming the winner as `reserved`, with no hold of its own.
/// - Lost replies: a client that leaves after the final usage was read (streaming, once it
///   saw its usage event; non-streaming, which sees nothing before its answer, after a 500 ms
///   pause) settles `disconnected` at its known usage (7 wei); one that leaves during output settles
///   `disconnected_unknown_usage`, releasing its whole 8-wei hold as exposure; a reply dropped
///   after completion stays `completed`. The engine sees each abandoned generation dropped,
///   the key's status reads the terminal record, and each retry under the key is the 409.
///
/// Every step's complete rows come from fixed literals; seven operations dispatch exactly
/// seven generations.
#[tokio::test]
async fn gateway_duplicates_in_flight_racing_or_after_a_lost_reply_never_dispatch_or_hold_again() {
    use axum::http::StatusCode;
    let mut fixture = gateway_fixture(false, 60).await;
    let root = fixture.root.clone();
    let application = fixture.routes.issue(4, &["infer", "read"]);
    let bearers = [root.bearer.as_str(), application.bearer.as_str()];
    let mut generations = 0;

    // In flight, then completed.
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let content = "held wait=in-flight:after-output";
    let held = fixture.spawn(gateway_chat(&root.bearer, "in-flight", content, 4, false));
    assert_eq!(fixture.engine.event().await, "held in-flight");
    let reserved = gateway_reserved(&rows, &fixture.rows(), "in-flight", &root.id, 4, 4, 8, (start, protocol::now().unwrap()));
    assert_eq!(fixture.rows(), reserved);
    let id = fixture.view("in-flight").id;
    let records = fixture.engine.take();
    assert_eq!(records.calls.last().map(String::as_str), Some(format!("POST /v1/chat/completions id={id}").as_str()));
    generations += records.generated.len();
    fixture.duplicates("in-flight", content, &bearers).await;
    fixture.engine.release("in-flight");
    let (status, header, text) = gateway_answer(held.await.unwrap()).await;
    let end = protocol::now().unwrap();
    assert_eq!((status, header.as_deref()), (StatusCode::OK, Some(id.as_str())), "{text}");
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["choices"][0]["message"]["content"], "w0 w1 w2 ");
    assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &id, "completed", Some((4, 3, 7)), (start, end)));
    fixture.duplicates("in-flight", content, &bearers).await;

    // Racing under one key: both are prepared, one is reserved.
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let contents = [(&root, "racing tokenize-wait=race wait=race-won:after-output", 5),
        (&application, "racing slowly tokenize-wait=race wait=race-won:after-output", 6)];
    let racers: Vec<_> = contents.iter()
        .map(|(credential, content, _)| fixture.spawn(gateway_chat(&credential.bearer, "raced", content, 4, false)))
        .collect();
    assert_eq!(fixture.engine.events(2).await, ["held race", "held race"]);
    assert!(!fixture.recorded("raced"), "reserved before both were prepared");
    fixture.engine.release("race");
    fixture.engine.release("race");
    assert_eq!(fixture.engine.event().await, "held race-won");
    let winner = fixture.view("raced");
    let (credential, content, prompt) = contents.iter().find(|(credential, _, _)| credential.id == winner.credential_id).unwrap();
    let mut racers = racers.into_iter();
    let (a, b) = (racers.next().unwrap(), racers.next().unwrap());
    let (loser, first) = match futures_util::future::select(a, b).await {
        futures_util::future::Either::Left((answer, other)) | futures_util::future::Either::Right((answer, other)) => (answer, other),
    };
    let (status, header, text) = gateway_answer(loser.unwrap()).await;
    assert_eq!((status, serde_json::from_str::<Value>(&text).unwrap()), (StatusCode::CONFLICT, json!({"error": {
        "message": "Request already exists and will not be replayed. Retrieve its status; response content is not retained.",
        "type": "invalid_request_error", "param": null, "code": "duplicate_request",
        "request_id": winner.id, "request_status": "reserved"}})), "the losing racer");
    assert_eq!(header.as_deref(), Some(winner.id.as_str()));
    let reserved = gateway_reserved(&rows, &fixture.rows(), "raced", &credential.id, *prompt, 4, *prompt as i128 + 4,
        (start, protocol::now().unwrap()));
    assert_eq!(fixture.rows(), reserved, "the losing racer held or recorded something");
    let records = fixture.engine.take();
    assert_eq!(records.calls.iter().filter(|call| call.as_str() == "POST /tokenize").count(), 2);
    assert_eq!(records.generated.len(), 1, "both racers were dispatched");
    generations += 1;
    fixture.engine.release("race-won");
    let (status, _, text) = gateway_answer(first.await.unwrap()).await;
    let end = protocol::now().unwrap();
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &winner.id, "completed",
        Some((*prompt, 3, *prompt as i128 + 3)), (start, end)));
    fixture.duplicates("raced", content, &bearers).await;

    // Lost replies: (key, message, streaming, terminal status, known usage). A streaming
    // client asks for the usage event, so it leaves only once the gateway has read the final
    // usage; a non-streaming client sees nothing before its answer, so it leaves after a
    // 500 ms pause in which the gateway reads the usage the engine has already sent.
    for (key, content, stream, terminal, known) in [
        ("lost-after-usage", "lost wait=lost-after-usage:after-usage", false, "disconnected", Some((4, 3, 7))),
        ("lost-during-output", "lost stall=lost-during-output:after-output", false, "disconnected_unknown_usage", None),
        ("lost-stream-after-usage", "lost wait=lost-stream-after-usage:after-usage", true, "disconnected", Some((4, 3, 7))),
        ("lost-stream-during-output", "lost stall=lost-stream-during-output:after-output", true,
            "disconnected_unknown_usage", None),
    ] {
        let rows = fixture.rows();
        let start = protocol::now().unwrap();
        if stream {
            let response = fixture.call(gateway_usage_stream(&root.bearer, key, content)).await;
            assert_eq!(response.status(), StatusCode::OK, "{key}");
            let mut body = response.into_body().into_data_stream();
            let mut text = String::new();
            gateway_stream_until(&mut body, &mut text, if known.is_some() { "\"usage\"" } else { "w2 " }).await;
            assert_eq!(fixture.engine.event().await, format!("held {key}"));
            assert_eq!(fixture.view(key).status, "reserved", "{key}");
            drop(body);
        } else {
            let client = fixture.spawn(gateway_chat(&root.bearer, key, content, 4, false));
            assert_eq!(fixture.engine.event().await, format!("held {key}"));
            if known.is_some() {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            assert_eq!(fixture.view(key).status, "reserved", "{key}");
            client.abort();
            assert!(client.await.unwrap_err().is_cancelled(), "{key}");
        }
        assert_eq!(fixture.engine.event().await, format!("dropped {key}"), "the abandoned generation went on");
        let view = fixture.settled(key).await;
        let end = protocol::now().unwrap();
        assert_eq!(view.status, terminal, "{key}");
        let reserved = gateway_reserved(&rows, &fixture.rows(), key, &root.id, 4, 4, 8, (start, end));
        assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &view.id, terminal, known, (start, end)), "{key}");
        assert_eq!(fixture.status(&root.bearer, key).await, (StatusCode::OK, serde_json::to_value(&view).unwrap()), "{key}");
        generations += fixture.engine.take().generated.len();
        fixture.duplicates(key, content, &bearers).await;
    }

    // A reply dropped after completion: the request completed before its answer was sent.
    let response = fixture.call(gateway_chat(&root.bearer, "lost-after-completion", "lost", 4, false)).await;
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);
    assert_eq!(fixture.view("lost-after-completion").status, "completed");
    generations += fixture.engine.take().generated.len();
    fixture.duplicates("lost-after-completion", "lost", &bearers).await;
    assert_eq!(generations, 7, "each operation was dispatched exactly once");
}

/// Task 3.1 (c), cancellation at the gateway. *Billable usage is authoritative and matches
/// the reservation*: "Known usage after cancellation ... SHALL remain eligible for its exact
/// charge. Receiving a cancellation acknowledgement such as 202 `cancelling` ... SHALL NOT
/// establish a free execution"; *Unknown usage releases the hold without inventing
/// execution*: "Absent ... usage SHALL resolve an unsettled request with zero charge, its
/// whole hold returned, and an explicit `*_unknown_usage` terminal status".
///
/// Each request holds 8 wei (four prepared input and four output tokens) and is cancelled
/// with its owner's root bearer, by key or by the opaque identifier, while the engine holds
/// it: `POST /v1/requests/<id>/cancel` answers 202 `cancelling` naming it, the engine sees the
/// generation dropped, and the request's own answer is a 409 `request_cancelled` error object
/// (or, streaming, a final error event of that code) with no output after it.
/// - Known: once the final usage was read (a streaming client saw its usage event; a
///   non-streaming one after a 500 ms pause), the request settles `cancelled` and is charged
///   its 7 wei of known usage.
/// - Unknown: cancelled during output (both modes) or before the engine answered at all, it
///   settles `cancelled_unknown_usage`: no charge or usage, the whole hold returned to the
///   account and the credential, and 8 wei of request exposure.
///
/// Cancelling any settled request, by key or opaque identifier, and a completed control
/// answers 200 with exactly its stored record, signals nothing and changes no row; an
/// unknown key is 404. Complete rows come from fixed literals.
#[tokio::test]
async fn gateway_cancellation_charges_known_usage_and_releases_unknown_holds_at_provider_expense() {
    use axum::http::StatusCode;
    let mut fixture = gateway_fixture(false, 60).await;
    let root = fixture.root.clone();
    let mut settled = Vec::new();
    // (key, message, streaming, cancel by opaque identifier, terminal status, known usage)
    for (key, content, stream, by_id, terminal, known) in [
        ("cancel-known", "cancel wait=cancel-known:after-usage", false, false, "cancelled", Some((4, 3, 7))),
        ("cancel-known-stream", "cancel wait=cancel-known-stream:after-usage", true, true, "cancelled", Some((4, 3, 7))),
        ("cancel-unknown", "cancel stall=cancel-unknown:after-output", false, true, "cancelled_unknown_usage", None),
        ("cancel-unknown-stream", "cancel stall=cancel-unknown-stream:after-output", true, false,
            "cancelled_unknown_usage", None),
        ("cancel-before-answer", "cancel stall=cancel-before-answer:before-headers", false, false,
            "cancelled_unknown_usage", None),
    ] {
        let rows = fixture.rows();
        let start = protocol::now().unwrap();
        let (client, mut stream_body, mut text) = if stream {
            let response = fixture.call(gateway_usage_stream(&root.bearer, key, content)).await;
            assert_eq!(response.status(), StatusCode::OK, "{key}");
            let mut body = response.into_body().into_data_stream();
            let mut text = String::new();
            gateway_stream_until(&mut body, &mut text, if known.is_some() { "\"usage\"" } else { "w2 " }).await;
            (None, Some(body), text)
        } else {
            (Some(fixture.spawn(gateway_chat(&root.bearer, key, content, 4, false))), None, String::new())
        };
        assert_eq!(fixture.engine.event().await, format!("held {key}"));
        if known.is_some() && !stream {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let held = fixture.view(key);
        assert_eq!(held.status, "reserved", "{key}");
        let target = if by_id { held.id.clone() } else { key.to_owned() };
        let (status, header, answer) = fixture.cancel(&root.bearer, &target).await;
        assert_eq!((status, header.as_deref(), answer), (StatusCode::ACCEPTED, Some(held.id.as_str()),
            json!({"id": held.id, "status": "cancelling"})), "{key}");
        assert_eq!(fixture.engine.event().await, format!("dropped {key}"), "the cancelled generation went on");
        if let Some(client) = client {
            let (status, header, text) = gateway_answer(client.await.unwrap()).await;
            let answer: Value = serde_json::from_str(&text).unwrap();
            assert_eq!((status, header.as_deref(), answer["error"]["code"].as_str()),
                (StatusCode::CONFLICT, Some(held.id.as_str()), Some("request_cancelled")), "{key}: {text}");
            assert_eq!(answer.as_object().unwrap().keys().collect::<Vec<_>>(), ["error"], "{key}");
        } else {
            let failed = gateway_stream_end(stream_body.as_mut().unwrap(), &mut text).await;
            let events = gateway_events(&text);
            assert!(!failed, "{key}: {text}");
            let last = events.last().unwrap();
            assert_eq!((&last["error"]["code"], events.iter().filter(|event| event.get("error").is_some()).count()),
                (&json!("request_cancelled"), 1), "{key}: {text}");
        }
        let view = fixture.settled(key).await;
        let end = protocol::now().unwrap();
        assert_eq!(view.status, terminal, "{key}");
        let reserved = gateway_reserved(&rows, &fixture.rows(), key, &root.id, 4, 4, 8, (start, end));
        assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &view.id, terminal, known, (start, end)), "{key}");
        let records = fixture.engine.take();
        assert_eq!(records.generated.len(), 1, "{key}");
        settled.push(view);
    }

    // A completed control, then cancellations of settled requests: a read only.
    let (status, _, _) = gateway_answer(fixture.call(gateway_chat(&root.bearer, "cancel-completed", "done", 4, false)).await).await;
    assert_eq!(status, StatusCode::OK);
    settled.push(fixture.view("cancel-completed"));
    fixture.engine.take();
    for view in &settled {
        for target in [view.request_id.as_str(), view.id.as_str()] {
            let rows = fixture.rows();
            let (status, header, answer) = fixture.cancel(&root.bearer, target).await;
            assert_eq!((status, header.as_deref(), answer), (StatusCode::OK, Some(view.id.as_str()),
                serde_json::to_value(view).unwrap()), "{target}");
            assert_eq!(fixture.rows(), rows, "{target}: cancelling a settled request changed the ledger");
            fixture.engine.untouched(target);
        }
    }
    let rows = fixture.rows();
    let (status, _, answer) = fixture.cancel(&root.bearer, "never-sent").await;
    assert_eq!((status, answer["error"]["code"].as_str()), (StatusCode::NOT_FOUND, Some("request_not_found")));
    assert_eq!(fixture.rows(), rows);
}

/// Task 3.1 (c), a dispatched request that ends in an engine failure or at the generation
/// deadline instead of a cancellation. *Billable usage is authoritative and matches the
/// reservation*: "Known usage after cancellation, disconnection or a failed response SHALL
/// remain eligible for its exact charge" and "Absence of validated usage SHALL follow the
/// explicit unknown-usage rule instead"; *Unknown usage releases the hold without inventing
/// execution*.
///
/// Each request holds 8 wei (four prepared input and four output tokens) under a two-second
/// generation deadline, and the engine sends two output tokens unless it fails first. Then
/// the engine's stream ends without `[DONE]` after the final usage, or before the finish and
/// usage; or the engine stalls after the final usage, during output or before answering,
/// until the gateway's deadline drops the generation; or it refuses the generation with a
/// 500. Non-streaming, each answers an error object only, with none of the output: 502
/// `engine_error` for an ended stream or a refusal, 504 `engine_timeout` at the deadline.
/// Streaming, the output ends with one final error event of that code before the only
/// `[DONE]`. With the final usage read, the request settles `failed` or `timed_out` and is
/// charged its known 6 wei. Without it, it settles `failed_unknown_usage` or
/// `timed_out_unknown_usage`: no charge or usage, the whole 8 wei returned to the account and
/// the credential, and 8 wei of request exposure. The engine sees each stalled generation
/// dropped, each key is the 409 afterwards with no second dispatch, and no settlement failure
/// is recorded. Complete rows come from fixed literals.
#[tokio::test]
async fn gateway_failed_and_timed_out_responses_charge_known_usage_and_release_unknown_holds() {
    use axum::http::StatusCode;
    use std::sync::atomic::Ordering;
    let mut fixture = gateway_fixture(false, 2).await;
    let root = fixture.root.clone();
    let state = fixture.routes.state.clone();
    let (ended, late) = ((StatusCode::BAD_GATEWAY, "engine_error"), (StatusCode::GATEWAY_TIMEOUT, "engine_timeout"));
    // (key, message, streaming, answer, terminal status, known usage, stalled by the engine)
    for (key, content, stream, (code_status, code), terminal, known, stalled) in [
        ("ended-after-usage", "out=2 end=after-usage", false, ended, "failed", Some((4, 2, 6)), false),
        ("ended-after-usage-stream", "out=2 end=after-usage", true, ended, "failed", Some((4, 2, 6)), false),
        ("ended-during-output", "out=2 end=after-output", false, ended, "failed_unknown_usage", None, false),
        ("ended-during-output-stream", "out=2 end=after-output", true, ended, "failed_unknown_usage", None, false),
        ("refused-generation", "refused generation-fails", false, ended, "failed_unknown_usage", None, false),
        ("late-after-usage", "out=2 stall=late-after-usage:after-usage", false, late, "timed_out", Some((4, 2, 6)), true),
        ("late-after-usage-stream", "out=2 stall=late-after-usage-stream:after-usage", true, late, "timed_out",
            Some((4, 2, 6)), true),
        ("late-during-output", "out=2 stall=late-during-output:after-output", false, late, "timed_out_unknown_usage",
            None, true),
        ("late-before-answer", "late stall=late-before-answer:before-headers", false, late, "timed_out_unknown_usage",
            None, true),
    ] {
        let rows = fixture.rows();
        let start = protocol::now().unwrap();
        let response = fixture.call(gateway_chat(&root.bearer, key, content, 4, stream)).await;
        let header = response.headers().get("x-request-id").map(|value| value.to_str().unwrap().to_owned());
        if stream {
            assert_eq!(response.status(), StatusCode::OK, "{key}");
            let mut body = response.into_body().into_data_stream();
            let mut text = String::new();
            // Two output events and the finish leave the bounded queue room for the error event.
            assert!(!gateway_stream_end(&mut body, &mut text).await, "{key}: {text}");
            let events = gateway_events(&text);
            let errors: Vec<&Value> = events.iter().filter(|event| event.get("error").is_some()).collect();
            assert_eq!(errors.len(), 1, "{key}: {text}");
            assert_eq!((&errors[0]["error"]["code"], &errors[0]["error"]["type"]), (&json!(code), &json!("server_error")), "{key}");
            assert!(events.last().unwrap().get("error").is_some(), "{key}: output after the error: {text}");
            assert!(text.contains("w0 ") && text.contains("w1 "), "{key}: {text}");
            assert_eq!(text.matches("data: [DONE]").count(), 1, "{key}: {text}");
            assert!(text.ends_with("data: [DONE]\n\n"), "{key}: {text}");
        } else {
            let (status, _, text) = gateway_answer(response).await;
            let answer: Value = serde_json::from_str(&text).unwrap();
            assert_eq!((status, answer["error"]["code"].as_str()), (code_status, Some(code)), "{key}: {text}");
            assert_eq!(answer.as_object().unwrap().keys().collect::<Vec<_>>(), ["error"], "{key}");
            assert!(!text.contains("w0"), "{key}: output was returned: {text}");
        }
        let view = fixture.settled(key).await;
        let end = protocol::now().unwrap();
        assert_eq!((view.status.as_str(), header.as_deref()), (terminal, Some(view.id.as_str())), "{key}");
        let reserved = gateway_reserved(&rows, &fixture.rows(), key, &root.id, 4, 4, 8, (start, end));
        assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &view.id, terminal, known, (start, end)), "{key}");
        if stalled {
            assert_eq!(fixture.engine.events(2).await, [format!("dropped {key}"), format!("held {key}")],
                "{key}: the generation outlived its deadline");
        }
        assert!(fixture.engine.events.try_recv().is_err(), "{key}: an unexpected engine event");
        assert_eq!(fixture.engine.take().generated.len(), 1, "{key}");
        assert_eq!(fixture.status(&root.bearer, key).await, (StatusCode::OK, serde_json::to_value(&view).unwrap()), "{key}");
        fixture.duplicates(key, content, &[&root.bearer]).await;
        assert!(state.settlement_fault().is_none() && state.accepting.load(Ordering::Acquire), "{key}");
    }
}

/// Task 3.1 (d), a durable settlement failure at the gateway. *Terminal request accounting
/// is atomic and at most once*: "Financial persistence failure SHALL NOT be acknowledged as
/// successful durable settlement; the gateway SHALL stop new admission and retain the
/// original request for recovery", scenario *Persistence failure cannot acknowledge an
/// unrecorded debit*; *Quote pressure and new-admission gates cannot strand settlement*:
/// settlement and authorized reads of existing requests continue.
///
/// Four requests are in progress: an application credential's request held after its
/// output, two root requests held after their output (one streaming), and an application
/// request held while it is tokenized. An `AFTER UPDATE` trigger with `RAISE(FAIL)` then
/// refuses the two root requests' terminal writes; `FAIL` keeps the statement's own work in
/// the transaction, so the production transaction must roll back the account and credential
/// writes before it. Released, the non-streaming one answers 500 `internal_error` with no
/// output and the stream ends with one final `internal_error` event, not a completion. Both
/// stay `reserved` with every row exactly as reserved (the gateway's retries fail alike),
/// and the provider records `DURABLE_SETTLEMENT_FAILED`, closing admission and new funding.
///
/// While admission is closed, the request being tokenized and new requests of both
/// credentials are refused 503 `provider_paused` (the first after tokenization, the others
/// before any engine call) with nothing recorded, and the application's earlier request
/// still completes at its 7 wei. Status, cancellation (a read, as nothing is in flight),
/// duplicate (409 `reserved`, no engine call), control-plane request and account reads and a
/// signed receipt binding both reserved requests stay available; only the receipt sequence
/// changes. Explicit startup recovery over the reopened ledger, once the trigger is gone,
/// then releases both 8-wei holds as `restart_unknown_usage` with 8 wei of exposure each and
/// no charge, leaving every other row unchanged. (Daemon startup ordering is
/// `a_paid_settlement_failure_keeps_its_hold_until_a_restart_releases_it_before_admission`.)
#[tokio::test]
async fn gateway_settlement_failure_keeps_holds_stops_admission_and_keeps_existing_reads() {
    use axum::http::StatusCode;
    use std::sync::atomic::Ordering;
    let mut fixture = gateway_fixture(false, 60).await;
    let root = fixture.root.clone();
    let application = fixture.routes.issue(2, &["infer", "read"]);
    let contract = fixture.id();
    let state = fixture.routes.state.clone();
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let admitted = fixture.spawn(gateway_chat(&application.bearer, "admitted-before", "admitted wait=admitted:after-output", 4, false));
    let failing = fixture.spawn(gateway_chat(&root.bearer, "settle-fault", "failing wait=failing:after-output", 4, false));
    let streaming = fixture.call(gateway_chat(&root.bearer, "settle-fault-stream", "failing wait=failing-stream:after-output", 4, true)).await;
    assert_eq!(streaming.status(), StatusCode::OK);
    let mut stream_body = streaming.into_body().into_data_stream();
    let mut stream_text = String::new();
    gateway_stream_until(&mut stream_body, &mut stream_text, "w2 ").await;
    let preparing = fixture.spawn(gateway_chat(&application.bearer, "prepared-during-failure", "preparing tokenize-wait=preparing", 4, false));
    assert_eq!(fixture.engine.events(4).await, ["held admitted", "held failing", "held failing-stream", "held preparing"]);
    let window = (start, protocol::now().unwrap());
    let mut held = gateway_reserved(&rows, &fixture.rows(), "admitted-before", &application.id, 4, 4, 8, window);
    held = gateway_reserved(&held, &fixture.rows(), "settle-fault", &root.id, 4, 4, 8, window);
    held = gateway_reserved(&held, &fixture.rows(), "settle-fault-stream", &root.id, 4, 4, 8, window);
    assert_eq!(fixture.rows(), held);
    let faults = Connection::open(&fixture.routes.path).unwrap();
    faults.busy_timeout(Duration::from_secs(5)).unwrap();
    faults.execute_batch("CREATE TRIGGER gateway_settlement_fault AFTER UPDATE ON requests
        WHEN OLD.request_key IN ('settle-fault','settle-fault-stream') AND NEW.status<>'reserved'
        BEGIN SELECT RAISE(FAIL,'injected gateway settlement failure'); END;").unwrap();
    fixture.engine.take();

    // Neither failed settlement is acknowledged; both holds stay exactly as reserved.
    fixture.engine.release("failing");
    let (status, header, text) = gateway_answer(failing.await.unwrap()).await;
    let failed = fixture.view("settle-fault");
    assert_eq!((status, header.as_deref()), (StatusCode::INTERNAL_SERVER_ERROR, Some(failed.id.as_str())), "{text}");
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer, json!({"error": {"message": "The provider could not complete this operation",
        "type": "server_error", "param": null, "code": "internal_error"}}));
    assert_eq!(state.settlement_fault(), Some(crate::inference_gateway::DURABLE_SETTLEMENT_FAILED));
    assert!(!state.accepting.load(Ordering::Acquire) && !state.funding_ready.load(Ordering::Acquire));
    assert_eq!(fixture.rows(), held, "a failed settlement left a partial or acknowledged debit");
    fixture.engine.release("failing-stream");
    let ended_with_error = gateway_stream_end(&mut stream_body, &mut stream_text).await;
    let events = gateway_events(&stream_text);
    let errors: Vec<&Value> = events.iter().filter(|event| event.get("error").is_some()).collect();
    assert!(!ended_with_error && errors.len() == 1 && events.last().unwrap().get("error").is_some(),
        "the failed stream ended like a completion: {stream_text}");
    assert_eq!(stream_text.matches("data: [DONE]").count(), 1, "{stream_text}");
    assert!(stream_text.find("\"internal_error\"") < stream_text.find("data: [DONE]"),
        "the stream was completed before its settlement failed: {stream_text}");
    assert_eq!(errors[0], &json!({"error": {"message": "The provider could not complete this operation",
        "type": "server_error", "param": null, "code": "internal_error"}}));
    assert_eq!(fixture.rows(), held, "a failed streaming settlement left a partial or acknowledged debit");
    assert_eq!(fixture.view("settle-fault-stream").status, "reserved");

    // Admission is closed: a request being prepared and new requests reserve nothing.
    fixture.engine.take();
    fixture.engine.release("preparing");
    let (status, _, text) = gateway_answer(preparing.await.unwrap()).await;
    assert_eq!((status, serde_json::from_str::<Value>(&text).unwrap()["error"]["code"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("provider_paused")), "{text}");
    assert!(fixture.engine.take().generated.is_empty(), "the prepared request was dispatched");
    for (bearer, key) in [(&root.bearer, "new-after-failure"), (&application.bearer, "new-after-failure-application")] {
        let (status, _, text) = gateway_answer(fixture.call(gateway_chat(bearer, key, "new", 4, false)).await).await;
        assert_eq!((status, serde_json::from_str::<Value>(&text).unwrap()["error"]["code"].as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, Some("provider_paused")), "{key}: {text}");
        fixture.engine.untouched(key);
        assert!(!fixture.recorded(key), "{key}");
    }
    for key in ["prepared-during-failure", "new-after-failure", "new-after-failure-application"] {
        assert!(!fixture.recorded(key), "{key}");
    }
    assert_eq!(fixture.rows(), held);

    // The request admitted before the failure still settles while admission stays closed.
    fixture.engine.release("admitted");
    let (status, _, text) = gateway_answer(admitted.await.unwrap()).await;
    let end = protocol::now().unwrap();
    assert_eq!(status, StatusCode::OK, "{text}");
    let completed = gateway_settled(&held, &fixture.rows(), &fixture.view("admitted-before").id, "completed",
        Some((4, 3, 7)), (start, end));
    assert_eq!(fixture.rows(), completed);
    assert_eq!(state.settlement_fault(), Some(crate::inference_gateway::DURABLE_SETTLEMENT_FAILED));
    assert!(!state.accepting.load(Ordering::Acquire));

    // Existing reads stay available and change nothing but the receipt sequence.
    fixture.engine.take();
    let streamed = fixture.view("settle-fault-stream");
    for (key, view) in [("settle-fault", &failed), ("settle-fault-stream", &streamed)] {
        let record = serde_json::to_value(view).unwrap();
        assert_eq!(view.status, "reserved");
        for target in [key, view.id.as_str()] {
            assert_eq!(fixture.status(&root.bearer, target).await, (StatusCode::OK, record.clone()), "{target}");
            assert_eq!(fixture.cancel(&root.bearer, target).await, (StatusCode::OK, Some(view.id.clone()), record.clone()),
                "{target}: a cancellation found it in flight");
        }
        let (status, document) = read_route(&fixture.routes, false, &format!("/v1/contracts/{contract}/requests/{key}"),
            Some(&root.bearer)).await;
        assert_eq!((status, document), (StatusCode::OK, record), "{key}");
    }
    fixture.duplicates("settle-fault", "failing", &[&root.bearer, &application.bearer]).await;
    assert_eq!(fixture.rows(), completed);
    let account = serde_json::to_value(fixture.routes.ledger().account(&contract).unwrap()).unwrap();
    assert_eq!((&account["reserved_wei"], &account["available_wei"]), (&json!("16"), &json!("972")));
    let (status, document) = read_route(&fixture.routes, false, &format!("/v1/contracts/{contract}"), Some(&root.bearer)).await;
    assert_eq!((status, document), (StatusCode::OK, account.clone()));
    let (status, document) = read_route(&fixture.routes, false, &format!("/v1/contracts/{contract}/receipt"), Some(&root.bearer)).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let payload = routed_receipt(&fixture.routes, document);
    assert_eq!(payload["account"], account);
    for view in [&failed, &streamed] {
        assert!(payload["requests"].as_array().unwrap().contains(&serde_json::to_value(view).unwrap()), "{}", view.request_id);
    }
    let mut expected = completed.clone();
    let sequence = &mut expiry_row(&mut expected, "accounts", &contract)[4];
    *sequence = json!(sequence.as_u64().unwrap() + 1);
    assert_eq!(fixture.rows(), expected, "a read changed more than the receipt sequence");
    fixture.engine.untouched("reads");

    // Startup recovery over the reopened ledger releases both holds once, without a charge.
    faults.execute_batch("DROP TRIGGER gateway_settlement_fault").unwrap();
    let recovered_at = protocol::now().unwrap();
    let reopened = BillingStore::open(&fixture.routes.path, &network()).unwrap();
    assert_eq!(reopened.recover_requests(recovered_at).unwrap(), 2);
    for view in [&failed, &streamed] {
        expect_terminal(&mut expected, &view.id, "restart_unknown_usage", None, recovered_at);
    }
    assert_eq!(fixture.rows(), expected);
    let account = &expected["accounts"].iter().find(|row| row[0] == contract).unwrap()[3];
    assert_eq!((&account["reserved_wei"], &account["available_wei"], &account["spent_wei"]),
        (&json!("0"), &json!("988"), &json!("7")));
}

/// Task 3.1 (e), the gateway's own usage validation. *Billable usage is authoritative and
/// matches the reservation*: "any reported total SHALL agree with their checked sum ...
/// Input usage SHALL equal the exact prepared count and output usage SHALL not exceed the
/// authorized maximum. Missing, malformed, inconsistent ... usage SHALL NOT be replaced",
/// scenario *Prompt or output mismatch*: "the request is not charged from those counts" and
/// "its unsettled hold is resolved under the explicit unknown-usage policy rather than left
/// locked or charged an estimate".
///
/// Each chat request holds 9 wei (five prepared input and four output tokens) while the
/// engine streams two output tokens and then final usage with one more or one fewer
/// prompt token than prepared, five completion tokens (over the four authorized), a total
/// one above the sum, no total, a prompt count as a string, or no usage event. Non-streaming
/// it answers 502 with an error object only (`engine_usage_mismatch` for the counts that
/// contradict the reservation, `engine_error` for malformed or missing usage) and none of
/// the output; streaming, its output ends with one final error event of that code. Each
/// settles `failed_unknown_usage`: no charge or usage, the whole 9 wei returned to the account
/// and the credential, and 9 wei of request exposure, from fixed-literal complete rows. The
/// gateway's validation precedes the ledger's, so none reaches the ledger's own
/// `invalid_usage_unknown_usage` release or stops admission. An embedding batch prepared into
/// two tokens whose engine usage counts three is released the same way (2 wei). A control
/// with exact usage is charged its 7 wei.
#[tokio::test]
async fn gateway_usage_validation_releases_the_whole_hold_without_inventing_tokens() {
    use axum::http::StatusCode;
    use std::sync::atomic::Ordering;
    let fixture = gateway_fixture(false, 60).await;
    let root = fixture.root.clone();
    let state = fixture.routes.state.clone();
    for stream in [false, true] {
        for (case, instruction, code) in [
            ("prompt-over", "usage=prompt+1", "engine_usage_mismatch"),
            ("prompt-under", "usage=prompt-1", "engine_usage_mismatch"),
            ("output-over", "usage=over", "engine_usage_mismatch"),
            ("false-total", "usage=total+1", "engine_error"),
            ("no-total", "usage=no-total", "engine_error"),
            ("text-count", "usage=text", "engine_error"),
            ("no-usage", "usage=missing", "engine_error"),
        ] {
            let key = format!("{case}-{stream}");
            let rows = fixture.rows();
            let start = protocol::now().unwrap();
            // Two output events and the finish leave the bounded queue room for the error event.
            let response = fixture.call(gateway_chat(&root.bearer, &key, &format!("checked out=2 {instruction}"), 4, stream)).await;
            if stream {
                assert_eq!(response.status(), StatusCode::OK, "{key}");
                let mut body = response.into_body().into_data_stream();
                let mut text = String::new();
                assert!(!gateway_stream_end(&mut body, &mut text).await, "{key}: {text}");
                let events = gateway_events(&text);
                let errors: Vec<&Value> = events.iter().filter(|event| event.get("error").is_some()).collect();
                assert_eq!(errors.len(), 1, "{key}: {text}");
                assert_eq!((&errors[0]["error"]["code"], &errors[0]["error"]["type"]), (&json!(code), &json!("server_error")), "{key}");
                assert!(events.last().unwrap().get("error").is_some(), "{key}: output after the error: {text}");
                assert!(!text.contains("\"usage\""), "{key}: {text}");
            } else {
                let (status, _, text) = gateway_answer(response).await;
                let answer: Value = serde_json::from_str(&text).unwrap();
                assert_eq!((status, answer["error"]["code"].as_str()), (StatusCode::BAD_GATEWAY, Some(code)), "{key}: {text}");
                assert_eq!(answer.as_object().unwrap().keys().collect::<Vec<_>>(), ["error"], "{key}");
                assert!(!text.contains("w0"), "{key}: output was returned: {text}");
            }
            let view = fixture.settled(&key).await;
            let end = protocol::now().unwrap();
            assert_eq!(view.status, "failed_unknown_usage", "{key}");
            let reserved = gateway_reserved(&rows, &fixture.rows(), &key, &root.id, 5, 4, 9, (start, end));
            assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &view.id, "failed_unknown_usage", None,
                (start, end)), "{key}");
            assert_eq!(fixture.rows(), rows.clone().into_iter().map(|(table, mut entries)| {
                if table == "requests" {
                    entries = fixture.rows()["requests"].clone();
                }
                (table, entries)
            }).collect::<ExpiryRows>(), "{key}: the release changed more than its own request record");
            assert_eq!(fixture.engine.take().generated.len(), 1, "{key}");
            assert!(state.settlement_fault().is_none() && state.accepting.load(Ordering::Acquire), "{key}");
        }
    }

    // An embedding batch whose engine usage contradicts its two prepared tokens.
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let body = json!({"model": "test/model", "input": ["embedding-miscount here"]});
    let (status, _, text) = gateway_answer(fixture.call(gateway_request("POST", "/v1/embeddings", &root.bearer,
        Some("embedding-miscount"), Some(&body))).await).await;
    let end = protocol::now().unwrap();
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!((status, answer["error"]["code"].as_str()), (StatusCode::BAD_GATEWAY, Some("engine_usage_mismatch")), "{text}");
    assert_eq!(answer.as_object().unwrap().keys().collect::<Vec<_>>(), ["error"]);
    let view = fixture.view("embedding-miscount");
    let reserved = gateway_reserved(&rows, &fixture.rows(), "embedding-miscount", &root.id, 2, 0, 2, (start, end));
    assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &view.id, "failed_unknown_usage", None, (start, end)));

    // Control: exact usage is charged.
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let (status, _, text) = gateway_answer(fixture.call(gateway_chat(&root.bearer, "exact-usage", "checked usage=exact", 4, false)).await).await;
    let end = protocol::now().unwrap();
    assert_eq!(status, StatusCode::OK, "{text}");
    let view = fixture.view("exact-usage");
    let reserved = gateway_reserved(&rows, &fixture.rows(), "exact-usage", &root.id, 4, 4, 8, (start, end));
    assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &view.id, "completed", Some((4, 3, 7)), (start, end)));
}

/// Task 3.1 (f), scenario *Revocation during an admitted request* at the gateway: "WHEN its
/// credential expires, is revoked or is explicitly rotated before settlement THEN the original
/// hold survives and the request settles against that original credential's accounting AND no
/// replacement credential gains its hold or authorizes a duplicate execution".
///
/// Three application credentials of the account each have one request in generation, 8 wei
/// held (four prepared input and four output tokens): E's credential expires three seconds
/// after issuance on the provider's real clock, R's is revoked and T's is rotated into a
/// successor while their requests run. The test polls the clock until E's `expires_at`; no
/// product time is substituted and no stored timestamp rewritten. Then every ended bearer is
/// refused 401 for its request's status and cancellation and for new inference, with no
/// engine call and no row change; the successor holds nothing, its duplicate of T's key is
/// the 409 naming T as `reserved` with no engine call, and its status read attributes T to
/// the rotated credential. E then completes and is charged 7 wei against its expired
/// credential (spend cap 50), R is cancelled by the root after its final usage and charged
/// 7 wei against its revoked credential, and T is cancelled during output and releases its
/// whole hold from the rotated credential with 8 wei of exposure. The successor's spend and
/// hold stay zero. Complete rows come from fixed literals.
#[tokio::test]
async fn credentials_ending_during_generation_settle_against_their_original_accounting() {
    use axum::http::StatusCode;
    let mut fixture = gateway_fixture(false, 60).await;
    let root = fixture.root.clone();
    let contract = fixture.id();
    let ledger = fixture.routes.ledger();
    let issued_at = protocol::now().unwrap();
    let expires_at = issued_at + 3;
    let expiring = ledger.issue_credential(&contract, &CredentialIssueRequest {
        expires_at, spend_cap_wei: Some("50".into()), max_concurrency: 1, scopes: vec!["infer".into(), "read".into()],
    }, issued_at).unwrap();
    let revoked = fixture.routes.issue(1, &["infer", "read"]);
    let rotated = fixture.routes.issue(1, &["infer", "read"]);
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let completing = fixture.spawn(gateway_chat(&expiring.bearer, "expiring", "ending wait=expiring:after-output", 4, false));
    let response = fixture.call(gateway_usage_stream(&revoked.bearer, "revoking", "ending wait=revoking:after-usage")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut revoking = response.into_body().into_data_stream();
    let mut revoking_text = String::new();
    gateway_stream_until(&mut revoking, &mut revoking_text, "\"usage\"").await;
    let cancelled = fixture.spawn(gateway_chat(&rotated.bearer, "rotating", "ending stall=rotating:after-output", 4, false));
    assert_eq!(fixture.engine.events(3).await, ["held expiring", "held revoking", "held rotating"]);
    let window = (start, protocol::now().unwrap());
    assert!(window.1 < expires_at, "the expiring request was not admitted before its credential's expiry");
    let mut expected = gateway_reserved(&rows, &fixture.rows(), "expiring", &expiring.id, 4, 4, 8, window);
    expected = gateway_reserved(&expected, &fixture.rows(), "revoking", &revoked.id, 4, 4, 8, window);
    expected = gateway_reserved(&expected, &fixture.rows(), "rotating", &rotated.id, 4, 4, 8, window);
    assert_eq!(fixture.rows(), expected);

    // Revocation and rotation while generating; then E's expiry on the real clock.
    ledger.revoke_credential(&contract, &revoked.id).unwrap();
    let now = protocol::now().unwrap();
    let successor = ledger.rotate_credential(&contract, &rotated.id, &CredentialIssueRequest {
        expires_at: now + 3600, spend_cap_wei: None, max_concurrency: 1, scopes: vec!["infer".into(), "read".into()],
    }, now).unwrap();
    for credential in [&revoked.id, &rotated.id] {
        let row = expiry_row(&mut expected, "credentials", credential);
        row[3]["revoked"] = json!(true);
        row[4] = json!(1);
    }
    let after = fixture.rows();
    let successor_row = after["credentials"].iter().find(|row| row[0] == successor.id).unwrap().clone();
    assert_eq!((&successor_row[3]["spent_wei"], &successor_row[3]["reserved_wei"], &successor_row[3]["revoked"]),
        (&json!("0"), &json!("0"), &json!(false)));
    let credentials = expected.get_mut("credentials").unwrap();
    credentials.push(successor_row);
    credentials.sort_by(|a, b| a[0].as_str().cmp(&b[0].as_str()));
    assert_eq!(after, expected, "revocation or rotation moved a running hold");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while protocol::now().unwrap() < expires_at {
        assert!(tokio::time::Instant::now() < deadline, "the clock did not reach the expiry");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Ended bearers are refused everywhere; the successor holds nothing and cannot duplicate.
    fixture.engine.take();
    for (bearer, key) in [(&expiring.bearer, "expiring"), (&revoked.bearer, "revoking"), (&rotated.bearer, "rotating")] {
        let (status, answer) = fixture.status(bearer, key).await;
        assert_eq!((status, answer["error"]["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("invalid_api_key")), "{key}");
        let (status, _, answer) = fixture.cancel(bearer, key).await;
        assert_eq!((status, answer["error"]["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("invalid_api_key")), "{key}");
        let (status, _, text) = gateway_answer(fixture.call(gateway_chat(bearer, &format!("{key}-again"), "again", 4, false)).await).await;
        assert_eq!((status, serde_json::from_str::<Value>(&text).unwrap()["error"]["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("invalid_api_key")), "{key}");
    }
    fixture.engine.untouched("ended bearers");
    assert_eq!(fixture.rows(), expected);
    fixture.duplicates("rotating", "ending stall=rotating:after-output", &[&successor.bearer]).await;
    let (status, answer) = fixture.status(&successor.bearer, "rotating").await;
    assert_eq!((status, &answer["credential_id"], &answer["status"]), (StatusCode::OK, &json!(rotated.id), &json!("reserved")));

    // Each request settles against its original credential.
    fixture.engine.release("expiring");
    let (status, _, text) = gateway_answer(completing.await.unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["choices"][0]["message"]["content"], "w0 w1 w2 ");
    let (status, _, _) = fixture.cancel(&root.bearer, "revoking").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let failed = gateway_stream_end(&mut revoking, &mut revoking_text).await;
    assert!(!failed && gateway_events(&revoking_text).last().unwrap()["error"]["code"] == "request_cancelled", "{revoking_text}");
    let (status, _, _) = fixture.cancel(&root.bearer, "rotating").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, _, text) = gateway_answer(cancelled.await.unwrap()).await;
    assert_eq!((status, serde_json::from_str::<Value>(&text).unwrap()["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("request_cancelled")));
    let end = protocol::now().unwrap();
    for (key, status, known) in [("expiring", "completed", Some((4, 3, 7))), ("revoking", "cancelled", Some((4, 3, 7))),
        ("rotating", "cancelled_unknown_usage", None)] {
        let view = fixture.settled(key).await;
        assert_eq!(view.status, status, "{key}");
        expected = gateway_settled(&expected, &fixture.rows(), &view.id, status, known, (start, end));
    }
    assert_eq!(fixture.rows(), expected);
    let credential = |id: &str| expiry_credential(fixture.routes.ledger(), &contract, id);
    for (id, spent) in [(&expiring.id, "7"), (&revoked.id, "7"), (&rotated.id, "0"), (&successor.id, "0")] {
        let view = credential(id);
        assert_eq!((view.spent_wei.as_str(), view.reserved_wei.as_str()), (spent, "0"));
    }
    fixture.duplicates("rotating", "ending", &[&successor.bearer, &root.bearer]).await;
}

/// Task 3.1 (e), the ledger's own invalid-usage release reached through the gateway.
/// *Unknown usage releases the hold without inventing execution*: "cumulative usage-counter
/// overflow SHALL commit an `invalid_usage_unknown_usage` release and surface the invalid-usage
/// error rather than leave the hold locked"; *Terminal request accounting is atomic and at most
/// once*: "An explicit invalid-usage terminal release is distinct from a transaction failure",
/// and only a settlement the provider "cannot durably record" closes admission
/// (provider-daemon-lifecycle requirement 6).
///
/// The agreement's signed limits admit any u64 token count, and a seeded known request brings
/// the account's cumulative completion counter to u64::MAX - 2 (its usage and charge are ledger
/// inputs). A chat request holding 8 wei then completes with valid usage of four prompt and
/// three completion tokens, which the counter cannot take. The ledger commits the release: no
/// charge or usage, the whole hold back and 8 wei of exposure, both counters unchanged. The
/// client gets a 500 `usage_not_accounted` error object, not the output and not a success, but
/// no settlement failure is recorded and admission and new funding stay open. The next request
/// (two completion tokens) brings the counter to exactly u64::MAX and is charged its 6 wei,
/// streaming. A streaming request whose two completion tokens the counter can then no longer
/// take ends with one `usage_not_accounted` event before its only `[DONE]`, with the same
/// committed release and admission still open. Complete rows come from fixed literals; the
/// gateway's performance record keeps its measured output tokens beside the zeroed usage.
#[tokio::test]
async fn gateway_cumulative_usage_overflow_is_an_invalid_usage_release_not_a_settlement_failure() {
    use axum::http::StatusCode;
    use std::sync::atomic::Ordering;
    const NEAR: u64 = u64::MAX - 2;
    const FUNDING: u128 = 100_000_000_000_000_000_000;
    let mut routes = CredentialRoutes::opened_with(FUNDING, 4, protocol::now().unwrap(), |agreement| {
        agreement.model.context_length = u64::MAX;
        agreement.model.max_output_tokens = u64::MAX;
    });
    let engine = ControlledEngine::start(None).await;
    {
        let state = Arc::get_mut(&mut routes.state).unwrap();
        let served = &mut Arc::get_mut(&mut state.config).unwrap().models[0];
        served.engine_url = engine.base.clone();
        served.request_timeout_secs = 60;
    }
    let app = crate::inference_gateway::router(routes.state.clone());
    let root = routes.opened.credential.clone().unwrap();
    let fixture = GatewayFixture { routes, app, engine, root: root.clone() };
    let contract = fixture.id();
    let model = fixture.routes.agreement.payload.model.clone();
    let now = protocol::now().unwrap();
    let seed = fixture.routes.ledger().reserve(&contract, &root.id, "seed", 0, NEAR, now, &model).unwrap();
    fixture.routes.ledger().settle(&seed.id, &TokenUsage { prompt_tokens: 0, completion_tokens: NEAR }, "completed", now).unwrap();
    let state = fixture.routes.state.clone();

    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let (status, header, text) = gateway_answer(fixture.call(gateway_chat(&root.bearer, "overflowing", "overflow out=3", 4, false)).await).await;
    let end = protocol::now().unwrap();
    let view = fixture.view("overflowing");
    assert_eq!(state.settlement_fault(), None, "a committed invalid-usage release was reported as a settlement failure");
    assert!(state.accepting.load(Ordering::Acquire) && state.funding_ready.load(Ordering::Acquire));
    assert_eq!((status, header.as_deref()), (StatusCode::INTERNAL_SERVER_ERROR, Some(view.id.as_str())), "{text}");
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer.as_object().unwrap().keys().collect::<Vec<_>>(), ["error"], "{text}");
    assert_eq!(answer["error"]["code"], "usage_not_accounted", "{text}");
    let mut expected = gateway_reserved(&rows, &fixture.rows(), "overflowing", &root.id, 4, 4, 8, (start, end));
    let record = fixture.rows()["requests"].iter().find(|row| row[0] == view.id).unwrap()[6].clone();
    let settled_at = record["settled_at"].as_u64().unwrap();
    assert!(start <= settled_at && settled_at <= end);
    assert_eq!((&record["performance"]["output_tokens"], &record["performance"]["policy"]), (&json!(3), &json!("unqualified")));
    expect_terminal(&mut expected, &view.id, "invalid_usage_unknown_usage", None, settled_at);
    expiry_row(&mut expected, "requests", &view.id)[6]["performance"] = record["performance"].clone();
    assert_eq!(fixture.rows(), expected);
    let account = fixture.routes.ledger().account(&contract).unwrap();
    assert_eq!((account.usage.prompt_tokens, account.usage.completion_tokens), (0, NEAR));

    // Admission stayed open: the next request reaches exactly u64::MAX and is charged.
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let response = fixture.call(gateway_chat(&root.bearer, "reaching-the-maximum", "fits out=2", 4, true)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let mut text = String::new();
    assert!(!gateway_stream_end(&mut body, &mut text).await, "{text}");
    assert!(text.ends_with("data: [DONE]\n\n") && !text.contains("\"error\""), "{text}");
    let view = fixture.settled("reaching-the-maximum").await;
    let end = protocol::now().unwrap();
    let reserved = gateway_reserved(&rows, &fixture.rows(), "reaching-the-maximum", &root.id, 4, 4, 8, (start, end));
    assert_eq!(fixture.rows(), gateway_settled(&reserved, &fixture.rows(), &view.id, "completed", Some((4, 2, 6)), (start, end)));
    let account = fixture.routes.ledger().account(&contract).unwrap();
    assert_eq!((account.usage.prompt_tokens, account.usage.completion_tokens), (4, u64::MAX));
    assert_eq!(state.settlement_fault(), None);

    // Review addition: a stream overflowing the counter, now at u64::MAX, ends with one
    // `usage_not_accounted` event before its only `[DONE]`; the release is committed and
    // admission stays open. Two output events and the finish leave the queue room for it.
    let rows = fixture.rows();
    let start = protocol::now().unwrap();
    let response = fixture.call(gateway_chat(&root.bearer, "overflowing-stream", "overflow out=2", 4, true)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let mut text = String::new();
    assert!(!gateway_stream_end(&mut body, &mut text).await, "{text}");
    let end = protocol::now().unwrap();
    let events = gateway_events(&text);
    let errors: Vec<&Value> = events.iter().filter(|event| event.get("error").is_some()).collect();
    assert_eq!(errors.len(), 1, "{text}");
    assert_eq!((&errors[0]["error"]["code"], &errors[0]["error"]["type"]), (&json!("usage_not_accounted"), &json!("server_error")));
    assert!(events.last().unwrap().get("error").is_some() && text.ends_with("data: [DONE]\n\n"), "{text}");
    assert_eq!(text.matches("data: [DONE]").count(), 1, "{text}");
    assert_eq!(state.settlement_fault(), None, "a committed invalid-usage release was reported as a settlement failure");
    assert!(state.accepting.load(Ordering::Acquire) && state.funding_ready.load(Ordering::Acquire));
    let view = fixture.view("overflowing-stream");
    let mut expected = gateway_reserved(&rows, &fixture.rows(), "overflowing-stream", &root.id, 4, 4, 8, (start, end));
    let record = fixture.rows()["requests"].iter().find(|row| row[0] == view.id).unwrap()[6].clone();
    let settled_at = record["settled_at"].as_u64().unwrap();
    assert!(start <= settled_at && settled_at <= end);
    assert_eq!((&record["performance"]["output_tokens"], &record["performance"]["policy"]), (&json!(2), &json!("unqualified")));
    expect_terminal(&mut expected, &view.id, "invalid_usage_unknown_usage", None, settled_at);
    expiry_row(&mut expected, "requests", &view.id)[6]["performance"] = record["performance"].clone();
    assert_eq!(fixture.rows(), expected);
    let account = fixture.routes.ledger().account(&contract).unwrap();
    assert_eq!((account.usage.prompt_tokens, account.usage.completion_tokens), (4, u64::MAX));
}

// inference-request-accounting task 3.2: admitted evidence through settlement. Ledger steps
// take explicit logical operation times. The reports are synthetic parser inputs persisted as
// files, not hardware qualification, timing eligibility or withdrawal classification.

/// The synthetic profiled chat model of `performance::evidence_tests` with its report measured
/// at `measured_at`, persisted as `name` in `directory`: the model, whose qualification is that
/// file's SHA-256 digest and measurement time, and the file.
fn measured_report(directory: &Path, name: &str, measured_at: u64) -> (ModelSpec, std::path::PathBuf) {
    let (mut model, mut report) = crate::performance::evidence_tests::fixture();
    model.serving.as_mut().unwrap().qualification.measured_at = measured_at;
    report["measured_at"] = json!(measured_at);
    let path = directory.join(name);
    crate::performance::evidence_tests::persist(&mut model, &mut report, &path);
    (model, path)
}

/// The measurement time recorded by the one retained report among `reports` whose SHA-256
/// digest is `digest`: what a bound digest identifies.
fn report_measured_at(reports: &[&std::path::Path], digest: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let matching: Vec<u64> = reports.iter().filter_map(|path| {
        let bytes = std::fs::read(path).unwrap();
        (hex::encode(Sha256::digest(&bytes)) == digest)
            .then(|| serde_json::from_slice::<Value>(&bytes).unwrap()["measured_at"].as_u64().unwrap())
    }).collect();
    assert_eq!(matching.len(), 1, "exactly one retained report has digest {digest}");
    matching[0]
}

fn evidence_of(model: &ModelSpec) -> String {
    model.serving.as_ref().unwrap().qualification.evidence_sha256.clone()
}

/// The performance record a reservation binds before dispatch: the admitted report's digest
/// and policy, nothing measured yet.
fn pending_performance(evidence: &str) -> Value {
    json!({"evidence_sha256": evidence, "policy": crate::performance::POLICY, "eligibility": "pending",
        "ttft_us": null, "decode_us": null, "elapsed_us": null, "output_tokens": null,
        "attained": null, "remedy": "none"})
}

/// `expected` with the ledger's reservation of `key` on `account` by `credential` added at
/// logical time `created`: `prompt` input and `output` maximum output tokens holding `maximum`
/// wei, bound to the report whose digest is `evidence`. Only the opaque identifier is read from
/// `after`; every other member is the caller's literal. Returns that identifier.
#[allow(clippy::too_many_arguments)]
fn bound_reservation(expected: &mut ExpiryRows, after: &ExpiryRows, account: &str, credential: &str, key: &str,
    prompt: u64, output: u64, maximum: i128, created: u64, evidence: &str) -> String {
    let id = after["requests"].iter().find(|row| row[1] == account && row[2] == key)
        .unwrap_or_else(|| panic!("{key} was not reserved"))[0].as_str().unwrap().to_owned();
    let requests = expected.get_mut("requests").unwrap();
    requests.push(json!([id, account, key, credential, "reserved", created, {
        "id": id, "request_id": key, "contract_id": account, "credential_id": credential, "status": "reserved",
        "usage": {"prompt_tokens": 0, "completion_tokens": 0}, "cost_wei": "0", "max_cost_wei": maximum.to_string(),
        "provider_loss_wei": "0", "prompt_tokens_reserved": prompt, "max_output_tokens": output,
        "created_at": created, "settled_at": null, "performance": pending_performance(evidence), "waived_wei": "0"}]));
    requests.sort_by(|a, b| a[0].as_str().cmp(&b[0].as_str()));
    let held = &mut expiry_row(expected, "accounts", account)[3];
    expiry_shift(&mut held["available_wei"], -maximum);
    expiry_shift(&mut held["reserved_wei"], maximum);
    expiry_shift(&mut expiry_row(expected, "credentials", credential)[3]["reserved_wei"], maximum);
    id
}

/// `expect_terminal` for request `id` with its stored performance record `performance` and
/// `waived` wei of nominal inference cost foregone: recorded on the request and accumulated once
/// on its account's waived total, never returned to available credit or spent.
fn expect_bound_terminal(expected: &mut ExpiryRows, id: &str, status: &str, known: Option<(u64, u64, i128)>,
    settled_at: u64, performance: Value, waived: i128) {
    expect_terminal(expected, id, status, known, settled_at);
    let row = expiry_row(expected, "requests", id);
    row[6]["performance"] = performance;
    row[6]["waived_wei"] = json!(waived.to_string());
    let account = row[6]["contract_id"].as_str().unwrap().to_owned();
    expiry_shift(&mut expiry_row(expected, "accounts", &account)[3]["waived_wei"], waived);
}

/// `expected` with the next performance observation: request `id` of model `model` against the
/// report whose digest is `evidence`, a miss or not.
fn expect_observation(expected: &mut ExpiryRows, id: &str, evidence: &str, miss: bool) {
    let observations = expected.get_mut("performance_observations").unwrap();
    let sequence = observations.last().map_or(1, |row| row[0].as_u64().unwrap() + 1);
    observations.push(json!([sequence, id, "model", evidence, u8::from(miss)]));
}

/// inference-request-accounting task 3.2, *Performance remedies preserve usage and admitted
/// evidence*: "Terminal performance accounting SHALL bind the reservation's admitted report and
/// policy, not silently substitute a later report, and the binding SHALL survive settlement and
/// unknown-usage restart recovery"; an eligible waiver "SHALL set actual inference charge to zero,
/// release the hold, retain known authoritative usage and record the nominal waived inference
/// amount once" and "SHALL NOT refund funding/platform fees or chain gas".
///
/// An account is opened under report R0 (measured at 100). Three synthetic report files of
/// otherwise identical terms are persisted and parse as reports: R0, R1 (150) and R2 (200), each
/// bound by its file's SHA-256 digest. Four requests are admitted under the renewal R1 and held.
/// While they are held, three other R1 misses latch R1's withdrawal, eight changed or
/// not-strictly-newer reports are refused as renewals with every row unchanged, and the renewal
/// R2 is admitted for four more requests. Each request keeps the report it was admitted under,
/// distinct from both the agreement's report and the later one:
/// - known settlement, a known-usage inference waiver and an explicit unknown release each refuse
///   a performance outcome computed against the later report (or the agreement's) with every row
///   unchanged, then commit against the admitted report, with its observation in that report's
///   own window;
/// - the waiver charges nothing, keeps the known usage, records the 17-wei nominal cost once and
///   returns only the hold; funding, the 500-wei platform fee and its obligation are unchanged;
/// - after reopening, restart recovery binds each still-reserved request to its admitted report,
///   not to the later renewal, and counts its provider-incomplete observation in that window;
/// - late replays offering the other report or other usage return the first result unchanged;
/// - a signed receipt carries each request's admitted digest beside the unchanged agreement
///   (R0's digest and measurement time); the bound digest identifies exactly one retained report,
///   whose measurement time is the admitted one.
///
/// R1 stays withdrawn and R2 keeps its own window. Every step compares complete rows of every
/// ledger table against fixed literals.
#[test]
fn admitted_reports_stay_bound_through_settlement_waiver_and_restart_across_a_renewal() {
    use crate::performance::{outcome, validate_evidence, POLICY};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("renewal.sqlite");
    let (r0, r0_file) = measured_report(directory.path(), "r0.json", 100);
    let (r1, r1_file) = measured_report(directory.path(), "r1.json", 150);
    let (r2, r2_file) = measured_report(directory.path(), "r2.json", 200);
    let reports = [r0_file.as_path(), r1_file.as_path(), r2_file.as_path()];
    for (model, file) in [(&r0, &r0_file), (&r1, &r1_file), (&r2, &r2_file)] {
        validate_evidence(model, file, 300).unwrap();
    }
    let (d0, d1, d2) = (evidence_of(&r0), evidence_of(&r1), evidence_of(&r2));
    assert!(d0 != d1 && d1 != d2 && d0 != d2);
    assert!(r0.accepts_execution(&r1) && r0.accepts_execution(&r2));

    let store = BillingStore::open(&path, &network()).unwrap();
    let (q, opened) = performance_account(&store, r0.clone(), 121);
    let account = q.payload.id.clone();
    let root = opened.credential.as_ref().unwrap().id.clone();
    let mut expected = expiry_ledger_rows(&path);
    let rows = || expiry_ledger_rows(&path);

    // Four requests admitted under the renewal R1: 10 input and 20 maximum output tokens at one
    // wei per token hold 30 wei each. The reservation carries R1's digest and measurement time;
    // the stored request binds R1's digest and policy; the agreement stays R0.
    let mut ids = std::collections::BTreeMap::new();
    for key in ["r1-known", "r1-waived", "r1-unknown", "r1-restart"] {
        let reservation = store.reserve(&account, &root, key, 10, 20, 200, &r1).unwrap();
        assert_eq!(reservation.model, r1, "{key}: the reservation carries the admitted report");
        assert_eq!((reservation.price, reservation.max_cost_wei.as_str()), (q.payload.price.clone(), "30"));
        let after = rows();
        ids.insert(key, bound_reservation(&mut expected, &after, &account, &root, key, 10, 20, 30, 200, &d1));
        assert_eq!(after, expected, "{key}");
    }
    assert_eq!(store.account(&account).unwrap().model, r0);

    // While they are held, three other R1 misses latch R1's withdrawal. Each is a known-usage
    // waiver of its own 3-wei nominal cost.
    let known = TokenUsage { prompt_tokens: 10, completion_tokens: 7 };
    let measured = |model: &ModelSpec, usage: &TokenUsage, miss: bool| {
        let profile = model.serving.as_ref().unwrap();
        outcome(profile, true, Some(if miss { 2_000_001 } else { 1_000_000 }), Some(50_000), Some(usage),
            "completed", false, Some(2_100_000), &profile.qualification.evidence_sha256)
    };
    let latch_usage = TokenUsage { prompt_tokens: 1, completion_tokens: 2 };
    for index in 0..3 {
        let key = format!("r1-miss-{index}");
        let reservation = store.reserve(&account, &root, &key, 1, 2, 201, &r1).unwrap();
        let after = rows();
        let id = bound_reservation(&mut expected, &after, &account, &root, &key, 1, 2, 3, 201, &d1);
        assert_eq!(reservation.id, id);
        let miss = measured(&r1, &latch_usage, true);
        store.settle_measured(&id, &latch_usage, "completed", Some(&miss), 201).unwrap();
        expect_bound_terminal(&mut expected, &id, "completed", Some((1, 2, 0)), 201, serde_json::to_value(&miss).unwrap(), 3);
        expect_observation(&mut expected, &id, &d1, true);
        if index == 2 {
            expected.get_mut("performance_withdrawals").unwrap().push(json!(["model", d1]));
        }
        assert_eq!(rows(), expected, "{key}");
    }
    assert!(store.performance_withdrawn(&r1).unwrap());
    assert!(store.reserve(&account, &root, "r1-after-its-latch", 10, 20, 201, &r1).unwrap_err().starts_with("model_unqualified:"));
    assert_eq!(rows(), expected, "a refused reservation under the latched report");

    // Not an otherwise-identical, strictly newer renewal: each is refused with every row unchanged.
    let changed = |change: &dyn Fn(&mut protocol::ServingProfile)| {
        let mut candidate = r2.clone();
        change(candidate.serving.as_mut().unwrap());
        candidate
    };
    for (case, candidate) in [
        ("the agreement's measurement time", changed(&|p| p.qualification.measured_at = 100)),
        ("an older measurement time", changed(&|p| p.qualification.measured_at = 99)),
        ("hardware", changed(&|p| p.devices[0].memory_mib += 1)),
        ("artifact", changed(&|p| p.artifact_manifest_sha256 = "e".repeat(64))),
        ("engine", changed(&|p| p.engine_version = "another-engine".into())),
        ("admitted concurrency", changed(&|p| p.max_concurrency += 1)),
        ("admitted arrival rate", changed(&|p| p.arrival_milli_rps += 1)),
        ("speed class", changed(&|p| p.output_tokens_per_second = 50)),
    ] {
        let refused = store.reserve(&account, &root, "not-a-renewal", 10, 20, 202, &candidate);
        assert!(refused.as_ref().is_err_and(|error| error.starts_with("model_revision_unavailable:")), "{case}: {refused:?}");
        assert_eq!(rows(), expected, "{case}");
        assert!(!r0.accepts_execution(&candidate), "{case}");
    }

    // The renewal R2 is admitted while the R1 requests are still held.
    for key in ["r2-known", "r2-waived", "r2-unknown", "r2-restart"] {
        let reservation = store.reserve(&account, &root, key, 10, 20, 203, &r2).unwrap();
        assert_eq!(reservation.model, r2, "{key}");
        let after = rows();
        ids.insert(key, bound_reservation(&mut expected, &after, &account, &root, key, 10, 20, 30, 203, &d2));
        assert_eq!(after, expected, "{key}");
    }
    assert_eq!(store.account(&account).unwrap().model, r0);

    // Settlement of each request: an outcome computed against any report other than the admitted
    // one is refused with every row unchanged; the admitted report's outcome then commits.
    let unknown = |model: &ModelSpec| {
        let profile = model.serving.as_ref().unwrap();
        outcome(profile, true, Some(1_000_000), None, None, "cancelled", false, Some(1_500_000),
            &profile.qualification.evidence_sha256)
    };
    for (admitted, evidence, others, prefix) in [(&r1, &d1, [&r2, &r0], "r1"), (&r2, &d2, [&r1, &r0], "r2")] {
        let id = ids[format!("{prefix}-known").as_str()].clone();
        for other in others {
            assert!(store.settle_measured(&id, &known, "completed", Some(&measured(other, &known, false)), 204)
                .unwrap_err().contains("differs from admitted execution evidence"), "{prefix}-known");
            assert_eq!(rows(), expected, "{prefix}-known: a refused outcome");
        }
        let attained = measured(admitted, &known, false);
        let settled = store.settle_measured(&id, &known, "completed", Some(&attained), 204).unwrap();
        assert_eq!((settled.cost_wei.as_str(), settled.waived_wei.as_str()), ("17", "0"));
        expect_bound_terminal(&mut expected, &id, "completed", Some((10, 7, 17)), 204, serde_json::to_value(&attained).unwrap(), 0);
        expect_observation(&mut expected, &id, evidence, false);
        assert_eq!(rows(), expected, "{prefix}-known");

        let id = ids[format!("{prefix}-waived").as_str()].clone();
        for other in others {
            assert!(store.settle_measured(&id, &known, "completed", Some(&measured(other, &known, true)), 204)
                .unwrap_err().contains("differs from admitted execution evidence"), "{prefix}-waived");
            assert_eq!(rows(), expected, "{prefix}-waived: a refused outcome");
        }
        let missed = measured(admitted, &known, true);
        assert_eq!((missed.attained, missed.remedy.as_str()), (Some(false), "inference_waiver"));
        let settled = store.settle_measured(&id, &known, "completed", Some(&missed), 204).unwrap();
        assert_eq!((settled.cost_wei.as_str(), settled.waived_wei.as_str()), ("0", "17"));
        assert_eq!((settled.usage.prompt_tokens, settled.usage.completion_tokens), (10, 7));
        expect_bound_terminal(&mut expected, &id, "completed", Some((10, 7, 0)), 204, serde_json::to_value(&missed).unwrap(), 17);
        expect_observation(&mut expected, &id, evidence, true);
        assert_eq!(rows(), expected, "{prefix}-waived");

        let id = ids[format!("{prefix}-unknown").as_str()].clone();
        for other in others {
            assert!(store.settle_measured(&id, &TokenUsage::default(), "cancelled_unknown_usage", Some(&unknown(other)), 204)
                .unwrap_err().contains("differs from admitted execution evidence"), "{prefix}-unknown");
            assert_eq!(rows(), expected, "{prefix}-unknown: a refused outcome");
        }
        let released = unknown(admitted);
        let settled = store.settle_measured(&id, &TokenUsage::default(), "cancelled_unknown_usage", Some(&released), 204).unwrap();
        assert_eq!((settled.cost_wei.as_str(), settled.provider_loss_wei.as_str(), settled.waived_wei.as_str()), ("0", "30", "0"));
        expect_bound_terminal(&mut expected, &id, "cancelled_unknown_usage", None, 204, serde_json::to_value(&released).unwrap(), 0);
        assert_eq!(rows(), expected, "{prefix}-unknown");
    }

    // Restart recovery after reopening binds each still-reserved request to its admitted report,
    // not the later renewal, with no invented usage or waived amount.
    drop(store);
    let store = BillingStore::open(&path, &network()).unwrap();
    assert_eq!(rows(), expected, "reopened");
    assert_eq!(store.recover_requests(205).unwrap(), 2);
    for (key, evidence) in [("r1-restart", &d1), ("r2-restart", &d2)] {
        let id = &ids[key];
        expect_bound_terminal(&mut expected, id, "restart_unknown_usage", None, 205, json!({
            "evidence_sha256": evidence, "policy": POLICY, "eligibility": "provider_incomplete",
            "ttft_us": null, "decode_us": null, "elapsed_us": null, "output_tokens": null,
            "attained": false, "remedy": "inference_waiver"}), 0);
        expect_observation(&mut expected, id, evidence, true);
    }
    assert_eq!(rows(), expected, "restart recovery");
    assert_eq!(store.recover_requests(206).unwrap(), 0);
    assert_eq!(rows(), expected, "repeated recovery");
    // R1 stays withdrawn; R2's own window holds one attained and two missed observations.
    assert_eq!(expected["performance_withdrawals"], vec![json!(["model", d1])]);
    assert!(store.performance_withdrawn(&r1).unwrap() && !store.performance_withdrawn(&r2).unwrap());
    assert!(!store.performance_withdrawn(&r0).unwrap());

    // Late replays offering the other report, other usage or another status return the first
    // result and change no row.
    for (key, id) in &ids {
        let (admitted, other) = if key.starts_with("r1") { (&r1, &r2) } else { (&r2, &r1) };
        let first = store.request_view(&account, key).unwrap();
        let late = TokenUsage { prompt_tokens: 10, completion_tokens: 3 };
        for (usage, status, performance) in [
            (&late, "completed", Some(measured(other, &late, true))),
            (&known, "completed", Some(measured(admitted, &known, false))),
            (&TokenUsage::default(), "restart_unknown_usage", None),
        ] {
            let replay = store.settle_measured(id, usage, status, performance.as_ref(), 207).unwrap();
            assert_eq!(serde_json::to_value(&replay).unwrap(), serde_json::to_value(&first).unwrap(), "{key}");
        }
        assert!(store.reserve(&account, &root, key, 10, 20, 207, &r2).unwrap_err().starts_with("duplicate_request:"), "{key}");
        assert_eq!(rows(), expected, "{key}: late replays");
    }

    // Only the hold returned: the waiver is foregone inference cost, never a balance credit,
    // and funding, the platform fee and its obligation are unchanged.
    let held = store.account(&account).unwrap();
    assert_eq!((held.funded_wei.as_str(), held.fee_wei.as_str(), held.available_wei.as_str(), held.reserved_wei.as_str(),
        held.spent_wei.as_str(), held.waived_wei.as_str()), ("100000", "500", "99466", "0", "34", "43"));
    assert_eq!((held.usage.prompt_tokens, held.usage.completion_tokens), (43, 34));
    assert_eq!(expected["fee_obligations"].len(), 1);

    // A signed receipt binds each request's admitted digest beside the unchanged agreement; each
    // bound digest identifies exactly one retained report and its measurement time.
    let receipt = store.receipt(&account, PROVIDER_KEY, 208).unwrap();
    protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &q.payload.provider_wallet).unwrap();
    expiry_row(&mut expected, "accounts", &account)[4] = json!(1);
    assert_eq!(rows(), expected, "a receipt advances only its sequence");
    assert_eq!(receipt.payload.account.model, r0);
    assert_eq!(report_measured_at(&reports, &d0), 100);
    assert_eq!(receipt.payload.requests.len(), 11);
    for request in &receipt.payload.requests {
        let performance = request.performance.as_ref().unwrap();
        let (evidence, measured_at) = if request.request_id.starts_with("r1") { (&d1, 150) } else { (&d2, 200) };
        assert_eq!((&performance.evidence_sha256, performance.policy.as_str()), (evidence, POLICY), "{}", request.request_id);
        assert_eq!(report_measured_at(&reports, &performance.evidence_sha256), measured_at, "{}", request.request_id);
        let stored = expiry_row(&mut expected, "requests", &request.id)[6].clone();
        assert_eq!(serde_json::to_value(request).unwrap(), stored, "{}", request.request_id);
    }
}
