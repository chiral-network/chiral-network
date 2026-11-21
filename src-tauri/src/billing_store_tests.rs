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
/// concurrency limit is `max_concurrency`. Verified funding is a ledger prerequisite
/// after the chain boundary, as in `paid`.
fn paid_at(
    store: &BillingStore,
    funding: u128,
    max_concurrency: u32,
    now: u64,
) -> (Signed<Quote>, crate::protocol::OpenResult) {
    let mut payload = quote(funding, false).payload;
    payload.max_concurrency = max_concurrency;
    payload.issued_at = now;
    payload.expires_at = now + 300;
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
        let (agreement, opened) = paid_at(&ledger, funding, max_concurrency, opened_at);
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
