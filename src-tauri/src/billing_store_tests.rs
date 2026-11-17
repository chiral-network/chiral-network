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
    assert_eq!(store.fee_summary().unwrap()["amounts_wei"]["remitted"], "5");
    batch.status = "orphaned".into();
    store.update_fee_batch(&batch).unwrap();
    assert_eq!(store.fee_summary().unwrap()["amounts_wei"]["batched"], "5");
    assert_eq!(
        store.prepare_fee_batches(300, true).unwrap()[0].id,
        batch.id
    );
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
        funding_ready: AtomicBool::new(true),
        qualified_models: std::sync::RwLock::new(
            [q.payload.model.id.clone()].into_iter().collect(),
        ),
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
        assert_eq!(store.fee_summary().unwrap()["amounts_wei"][fee_state], "5");
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
    let now = protocol::now().unwrap();
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
        funding_ready: AtomicBool::new(true),
        qualified_models: std::sync::RwLock::new(Default::default()),
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
    fixture.nonce.store(1, Ordering::Release);
    *fixture.owner_balance.lock().unwrap() = 0;
    fixture.outage.store(true, Ordering::Release);
    state.accepting.store(false, Ordering::Release);
    state.qualified_models.write().unwrap().clear();
    let response = owner_post(&client, &base, &path, &signed.raw_transaction, &ledger).await;
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
        owner_post(&client, &base, &topup_path, &topup.raw_transaction, &ledger)
            .await
            .status(),
        reqwest::StatusCode::CONFLICT
    );
    assert_eq!(ledger.account(&q.payload.id).unwrap().funded_wei, "1000");
    observe(&topup.tx_hash, native_fixture_gas(&protocol::topup_data(&q.payload.id).unwrap()));
    for _ in 0..2 {
        let response =
            owner_post(&client, &base, &topup_path, &topup.raw_transaction, &ledger).await;
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let account: AccountView = response.json().await.unwrap();
        assert_eq!(account.funded_wei, "2000");
        assert_eq!(account.fee_wei, "10");
        assert_eq!(account.available_wei, "1990");
        assert_eq!(account.reserved_wei, "0");
    }
    // Cleanup removed the unacknowledged quote; restore a currently valid offer
    // so this checks the admission gate, not a missing-record rejection.
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
