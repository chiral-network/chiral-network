## 1. Specify the provider daemon lifecycle

- [x] 1.1 Read the source material:
  - the read-only book section *Native provider provisioning and operations* in full (lines 605–680, every line, including the long lines 609, 613–619, 627–634, 636, 638, 640, 642, 644, 648–652, 656, 667 and 671–679), and line 252, which `inference-offer-discovery` cites for `/readyz`;
  - `docs/provider-qualification.md`, which line 607 points to (lines 195–260 and 450–480);
  - the `CLAUDE.md` rules on the distinct public and loopback operator listeners, the protected operator bearer and `Origin` refusal, the shared base `data_dir` and nonce journal per signing wallet, schema-3 ledgers, and explicit cutover, backups and rollback plans;
  - comparison item 14;
  - the pending `specify-offer-discovery`, `specify-managed-engine-launch`, `specify-inference-request-accounting`, `specify-generation-performance`, `specify-inference-commercial-terms`, `specify-funding-settlement`, `specify-fee-remittance`, `specify-version-policy`, `specify-outbound-http-safeguards`, `specify-account-credentials`, `specify-opening-funding-intent`, `specify-public-inference-api`, `specify-chain-rpc-gateway` and `specify-profile-path-resolution`, and the normative `openspec/specs`;
  - the target-design `specify-desktop-share-online` and `specify-p2p-provider-transport`.

  Searched `openspec/changes` and `openspec/specs` for existing coverage of each rule (`healthz`, `readyz`, `drain`, `resume`, `paused`, `admin/status`, `admin/runtime`, `schema 3`, `file lock`, `backup`, `logging`, `legacy` and `cutover`). Mapped each uncovered whitepaper rule to one requirement, referenced the owners of the rest instead of restating them, and recorded the unowned operator procedures as out of scope with reasons (design *Out of scope, each with a reason*).
  Done 2026-11-20: the book's SHA-256 is `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`.
- [x] 1.2 Wrote the proposal, the design and the `provider-daemon-lifecycle` spec: 12 requirements and 36 scenarios after review, each requirement citing its book lines. Inspected the code:
  - `bin/chiral_daemon.rs`: `Termination`, `main`;
  - `app.rs`: `ProviderState`, `ProviderRuntime::start`, the engine watcher, `next_revision`, `refresh`, `refresh_inner`, `stop_engines`, `drain`, `maintenance`, `withdraw_published`, the routers, `live`, `ready`, `admin_runtime`, `admin_auth`, the `admin_*` handlers and the tests;
  - `inference_gateway.rs`: `Settlement`, `finish`, `Drop`, and the admission checks of `infer`;
  - `contracts.rs`: `accepting`, `MaintenanceReport`, `reconcile`, `reconcile_fees`, `remit_fees`;
  - `billing_store.rs`: `open`, `bind_provider`, `cleanup_pending`, `withdrawn`, `recover_requests`, `admin_summary`, `RequestView`;
  - `file_lock.rs`; `dht.rs` (`start`, `publish`, `publish_local`, `status`, `stop`, `run`); `inference_backend.rs` (`ModelHealth`, `model_health`, `probe_model`); `provider_config.rs` (`runtime_data_dir`); `protocol.rs` (`accepts_execution`, `AccountView`);
  - `scripts/provider-admin.py`, `scripts/provider-preflight.py`, `scripts/provision-provider.py` and `docker-compose.yml`;
  - `tests/daemon_engine_process_test.rs` and `tests/scaled/paid-inference.py`.

  Recorded in `design.md` the coverage table, nine code differences, the implementation values (decision 9), fourteen observations and six author questions. A manual probe of the existing debug `chiral_daemon`, with a scratchpad development configuration, confirmed code differences 2, 3 and 7 (design *Coverage table*).
- [x] 1.3 `openspec validate specify-provider-daemon-lifecycle --strict --no-interactive` passes. The focused runs behind the coverage table passed on 2026-11-20: 10 library tests and 6 integration tests (listed in design *Coverage table*). No code or test was changed, and the whitepaper is unchanged.
  Adversarial review, 2026-11-20: re-read lines 605–680 and the cited code, and re-ran five of the library tests and every `daemon_engine_process_test` and `inference_funding_admission_test` test (all passed). Narrowed code difference 6 to non-colliding listen addresses, and fixed its spec scenario and task 3.8, which used the same configuration and so would fail at the public listener bind. Added code differences 8 and 9, the shutdown no-reopen rule and the content-retention rule (book line 671) to requirements 11 and 12, observations 12–14 and open question 6. Corrected the drain timing ("before it answers"), the ledger-identity recording rule and the Compose line numbers. Strict validation passes again.
- [x] 1.4 Append report entry and mark comparison item 14. For the coordinator. Done 2026-11-20: report entry "Provider daemon lifecycle: drain across restarts, reopening after a settlement failure, startup publication, a lost ledger, automatic restarts and hang-up signals"; item 14 marked "Specified in round 55".
  - append an entry to `docs/apas-worker/report.md` with the nine code differences and the six author questions;
  - mark comparison item 14 in `docs/apas-worker/comparison-2026-11-18.md` as specified by this change.

## 2. Close code differences

- [ ] 2.1 Keep admission closed after a durable settlement failure until the operator acts, and report the failure (requirements 1 and 6, design code difference 1).
  - **Change.** Record a settlement fault in provider state, for example a sanitized message beside `accepting`. Set it where `Settlement::finish` closes admission after a failed `settle_measured`, or after the accounting clock fails (inference_gateway.rs:381–390 and 419–425). Then:
    - make `refresh_inner` include the fault's absence in `accepting` (app.rs:356–362);
    - have `admin_resume` clear the fault before its pass;
    - keep the fault through `admin_refresh` and the periodic pass;
    - report it in `admin_status`.
  - **Regression** in `app::tests`, using `embedding_provider`, which has a free account and an external embedding stub:
    - after start, open the deployment's `ledger.sqlite3` through a second `rusqlite` connection and install a trigger that aborts the settlement's update of `requests`;
    - one embedding request through `public_router` fails with the durable-settlement error, and `accepting` is false;
    - drop the trigger, then run `runtime.refresh()` and `POST /admin/offers/refresh`;
    - a new embedding request is still refused as `provider_paused`, and no request record exists for its key;
    - `/admin/status` names the settlement failure, and `/admin/requests` still lists the first request as `reserved`;
    - after `POST /admin/resume`, the provider admits again, and a new request completes.
  - Keep the trigger until the failed request's answer has arrived, because dropping its settlement retries the write once (design observation 13).
  - **Fails without the change**: after `runtime.refresh()`, `accepting` is true and the new request completes.
- [ ] 2.2 Treat a terminal hang-up and the Windows console events as normal shutdown (requirement 11, design code difference 2).
  - **Change.** In `src-tauri/src/bin/chiral_daemon.rs` `Termination`, install and await `SignalKind::hangup()` on Unix, and `tokio::signal::windows::ctrl_break()` and `ctrl_close()` on Windows, beside the existing requests.
  - **Regression** in `tests/daemon_engine_process_test.rs`:
    - add `sighup_during_engine_startup_stops_the_engine`, which calls the existing `termination_during_engine_startup_stops_the_engine` with `libc::SIGHUP`. That helper already asserts that the daemon exits by itself with a failure, reports "termination requested during startup", never served, and leaves no engine;
    - add a serving variant: a development provider with an unmanaged model and an unavailable RPC, as in the design's manual probe. Wait until `/healthz` answers, send `SIGHUP`, and assert that the daemon exits with code 0 within the shutdown bound, not by the signal.
  - **Fails without the change**: the daemon ends by the signal (`status.code()` is `None`; exit status 129 in a shell). On macOS the startup helper's engine also survives.
  - The Windows events are verified by inspection, unless a console-control test harness is added.
- [ ] 2.3 Report the drain and each model's performance-withdrawal latch in `/admin/status` (requirement 1, design code difference 3).
  - **Change.**
    - Add a `draining` member to `admin_status` (app.rs:588–601).
    - Add a per-model member, for example `performance_withdrawn`, from `ledger.performance_withdrawn` (billing_store.rs:1136), to each model entry.
    - Give a latched model a detail that names the withdrawal, instead of "Ready: …" or "Initial qualification awaits idle engine capacity" (inference_backend.rs:1441–1444 and 1455–1462).
  - **Regression** in `app::tests`:
    - with `embedding_provider` and `ready_chain`: after `POST /admin/drain`, the status has `draining` true and `accepting` false; after `POST /admin/resume`, `draining` false and `accepting` true;
    - with a chain stub whose node status reports another genesis, and no drain: `accepting` false, `draining` false, and `chain_error` naming the rejected identity;
    - with a development provider whose model carries a serving profile and evidence, as the `inference-generation-performance` fixtures use, and a `performance_withdrawals` row for its evidence digest inserted through a second connection: the model entry reports the withdrawal, and its probe result is still reported.
  - **Fails without the change**: the status has no `draining` member and no withdrawal member.
- [ ] 2.4 Keep `/admin/status` answering when the ledger summary cannot be read (requirement 1, design code difference 4).
  - **Change.** In `admin_status`, report a ledger-unavailable member with a fixed category when `admin_summary` fails, instead of returning its error (app.rs:599). Keep HTTP 200 and every other member.
  - **Regression** in `app::tests`:
    - after start, rename the `accounts` table through a second connection, so that `admin_summary` fails;
    - `GET /admin/status` answers 200 with the ledger reported unavailable, and still contains `models`, `accepting`, `funding_ready`, `chain_error`, `maintenance`, `refresh_error` and `dht`;
    - `GET /readyz` answers 503 (`inference-offer-discovery`);
    - restore the table afterwards.
  - **Fails without the change**: the status route answers an error.
- [ ] 2.5 Start, and report, when the starting maintenance pass fails at publication (requirement 7, design code difference 5).
  - **Change.** In `ProviderRuntime::start` (app.rs:219), keep the starting pass's error in `refresh_error` and continue, as the periodic loop does (app.rs:452). Configuration, secret, inventory, launch, lock, ledger-identity and recovery failures stay fatal, because they happen before this pass. The other failures of that pass then behave as in a later pass too: a reconciliation failure leaves admission closed (app.rs:336–341), and a clock failure closes it (349–352).
  - **Regression** in `app::tests`:
    - start a non-development embedding provider, as in `failed_refresh_withdraws_the_offers_it_published`, and open a free account;
    - stop it, write the deployment's `offer-revision` file as `18446744073709551615`, so that `next_revision` fails with "offer revision exhausted", and start again;
    - the start succeeds; `/admin/status` reports the refresh error; `funding_ready` is false; `/admin/offers` and the DHT list no offer; the account's embedding request completes.
  - **Fails without the change**: the second `ProviderRuntime::start` returns an error.
- [ ] 2.6 Take the deployment lock before the DHT node joins under the deployment's identity (requirement 8, design code difference 6).
  - **Change.** In `ProviderRuntime::start`, load the DHT identity under the protected-file rules of `inference-offer-discovery`, for example through a `dht` function that returns the persisted peer ID. Then derive the deployment directory and acquire `provider.lock` before `DhtService::start` (app.rs:128–152). Keep the error "another provider owns this deployment data directory".
  - **Regression** in `app::tests`:
    - bind a plain `TcpListener` on loopback that counts accepted connections;
    - configure it as the only bootstrap address (`/ip4/127.0.0.1/tcp/<port>/p2p/<any valid peer ID>`);
    - start provider `A` and wait for its dial;
    - start a second runtime with the same configuration;
    - the second start fails with the lock error, and the counter does not increase.
  - **Fails without the change**: the second start dials the bootstrap address before it is refused, and the counter increases.
- [ ] 2.7 Report engine liveness in `/admin/runtime` without calling it readiness (requirement 2, design code difference 7).
  - **Change.** In `admin_runtime` (app.rs:508–515), report the process observation as a liveness member, for example `alive`. Then either drop `ready`, or derive it from the model's latest readiness pass together with liveness. Leave unmanaged models without a liveness claim.
  - Update `scripts/provider-preflight.py` (147–149), `scripts/qualify-provider.py` (its `/admin/runtime` reads) and `docs/provider-qualification.md` line 255 to read the liveness member. Read readiness from `/readyz` or `/admin/status`, as preflight already does.
  - **Regression** in `app::tests`: with `embedding_provider`, whose external model is ready, `/admin/runtime` reports the model as unmanaged and does not report it as not ready. With the managed test constructor of `managed-engine-runtime` task 3.10, a live engine whose readiness probe fails is reported alive and not ready.
  - **Regression** in `scripts/test_provider_preflight.py`: preflight accepts a running deployment by the liveness member, and refuses a dead engine.
  - **Fails without the change**: the unmanaged ready model is reported `"ready": false`.

- [ ] 2.8 Report an engine exit in `/admin/status` with its own reason (requirement 1, design code difference 8).
  - **Change.** Where the engine watcher marks a failed model's health entry not ready (app.rs:243–245), also set its detail to the engine-exit reason that `probe_model` uses ("Managed engine exited", inference_backend.rs:1425).
  - **Regression** in `app::tests`, with the managed test constructor of `managed-engine-runtime` task 3.10: after the readiness pass, kill the engine and wait until `/readyz` answers 503; before any further pass, `/admin/status` reports the model `ready` false with a detail that names the engine exit and does not start with "Ready".
  - **Fails without the change**: the detail is still "Ready: private health, tokenizer/template identity and advertised inference probes passed; …".
- [ ] 2.9 Keep the shutdown drain in effect against a racing resume (requirement 11, design code difference 9).
  - **Change.** Give `ProviderRuntime` a shutdown state that the shutdown path sets, under the lifecycle lock, when it drains (chiral_daemon.rs:160). While it is set, `admin_resume` refuses without clearing `draining`, and `refresh_inner` never opens admission or new-funding readiness or publishes. Keep `POST /admin/drain` and the read-only routes answering.
  - **Regression** in `app::tests`, with `embedding_provider` and `ready_chain`: call the new shutdown drain, then send `POST /admin/resume` and `POST /admin/offers/refresh` through `admin_router`; `accepting` and `funding_ready` stay false, `/admin/offers` and the DHT list no offer, and a new embedding request is refused as `provider_paused`.
  - **Fails without the change**: there is no shutdown state, and a `POST /admin/resume` after the shutdown path's `drain` runs a pass that reopens admission and publishes the model's offer.

## 3. Acceptance evidence per requirement

Task 3.N corresponds to requirement N in the spec and to row N of the design coverage table. A task is checked only when existing passing tests prove every statement and scenario of its requirement. An unchecked task names the missing case.

- [ ] 3.1 The operator status reports each lifecycle fact separately.
  - Existing evidence: `app::tests::readiness_records_the_native_embedding_dimension_only_while_the_model_is_ready` (reads `/admin/status`) and `app::tests::a_capability_without_a_probe_on_the_model_endpoints_is_not_ready_or_published` (model detail).
  - Missing, besides tasks 2.1, 2.3 and 2.4: a drained provider with one admitted streaming request, held open by a stub engine, reports one unsettled request (`ledger.active_requests`), and none after the stream completes. A test that reading the status changes no admission, publication or ledger state.
- [ ] 3.2 The runtime observation reports processes, not readiness or a serving profile.
  - Existing evidence: none.
  - Missing, besides task 2.7: a managed engine killed through the test constructor is reported not alive and not ready (shared with `managed-engine-runtime` task 3.10).
- [ ] 3.3 Operator drain stops new admission and new funding while admitted work and recovery continue.
  - Existing evidence: `inference_owner_control_test::funding_intent_authority_is_required_before_new_admission_and_cached_acknowledgement` and `inference_funding_admission_test::fresh_intent_rechecks_provider_funding_gates_after_balance`. Both refuse fresh intents as `provider_draining` and recover cached acknowledgements, with `accepting` set directly, not by a drain.
  - Missing: `app::tests` through `POST /admin/drain`, with a stub chat engine whose stream the test holds open:
    - a new request on the same funded account is refused as `provider_paused`, with no request record for its key;
    - the held stream completes and settles as `completed` with its authoritative usage;
    - the DHT publishes no offer;
    - the owner reads the request status and its receipt, rotates a credential and settles a top-up of the existing agreement while drained, and a new quote proposal is refused;
    - the next pass rechecks a credited funding (its checked time advances);
    - a second drain leaves `list_accounts`, `list_requests`, `fee_summary` and the funding intents byte-identical.
- [ ] 3.4 Resume and offer refresh reopen only through a complete maintenance pass.
  - Existing evidence: `billing_store::tests::withdrawal_survives_outstanding_burst_late_completion_and_restart_per_model_evidence` (the latch survives restart, at the ledger).
  - Missing: `app::tests`:
    - a drained healthy provider, after `POST /admin/resume`, admits, is new-funding ready, publishes offers under a higher revision, and answers `accepting` true with the offer count;
    - a drained provider whose managed engine has exited, after `POST /admin/resume`, ends its drain, still refuses admission, publishes nothing and starts no process (shared with `managed-engine-runtime` task 3.10);
    - a drained provider, after `POST /admin/offers/refresh`, is still draining (task 2.3's member), publishes nothing and answers `accepting` false with zero offers;
    - a latched report stays latched after both actions (shared with `inference-generation-performance`).
- [ ] 3.5 Each maintenance pass observes and reconciles before it reopens or publishes.
  - Existing evidence: `app::tests::failed_refresh_withdraws_the_offers_it_published` and `app::tests::fee_view_lists_confirmed_remittances_and_the_receipt_statement`.
  - Missing:
    - a test-only pause point in `refresh_inner` after it has stored `accepting`; while a pass is paused there, `drain()` is called, and after both return, `accepting` is false and `/admin/offers` and the DHT list no offer;
    - the same with an engine exit through the managed test constructor;
    - a pass while drained and after an engine exit still rechecks a credited funding;
    - an order test: a credited funding whose recheck the ledger quarantines keeps the same pass from publishing.

    The serialization itself holds by inspection of the `lifecycle` lock (app.rs:240, 279 and 431).
- [ ] 3.6 Admission closed by a ledger persistence failure stays closed until the operator acts.
  - Existing evidence: none.
  - Missing, besides task 2.1: after the injected failure, a restart instead of a resume settles the reserved request as `restart_unknown_usage` before the first new request is admitted, and then admits (shared with `inference-request-accounting` task 3.1).
- [ ] 3.7 Startup refuses what it cannot verify, and a publication failure is not a startup failure.
  - Existing evidence: `app::tests::deployments_share_a_wallet_without_sharing_accounts_or_process_locks` (starts with an RPC that answers 503) and `daemon_engine_process_test::start_refuses_an_engine_key_that_repeats_another_credential_before_any_engine_starts`.
  - Missing, besides task 2.5:
    - with an RPC stub whose identity check fails (another chain ID or genesis), `ProviderRuntime::start` fails with "configured RPC identity was rejected";
    - at process level, the daemon exits non-zero without printing its start line;
    - with an RPC that answers 503, `/readyz` reports `funding_ready` false and `/admin/offers` is empty.
- [ ] 3.8 One daemon owns a deployment's state for its lifetime.
  - Existing evidence: `app::tests::deployments_share_a_wallet_without_sharing_accounts_or_process_locks` (a second start fails; a restart finds the account) and `file_lock::tests::exclusion_ends_before_inherited_descriptor_closes`.
  - Missing, besides task 2.6:
    - the second-start assertion names the lock error instead of `is_err()`;
    - in `daemon_engine_process_test`, while the first daemon runs, a second daemon process for the same deployment (the same data directory, signing key and DHT identity file, but other public, administration and DHT listen ports, so that no bind collides) exits non-zero with the lock error, and the stub engine's spawn record shows no second engine. A second process with the same configuration file fails at the public listener bind instead (chiral_daemon.rs:115), which tests nothing about the lock.
- [ ] 3.9 The provider ledger opens only under its own schema, network and provider.
  - Existing evidence: `billing_store::tests::network_scoped_performance_cutover_refuses_old_ledgers_without_rewriting_money`.
  - Missing:
    - `BillingStore::open` of a schema-3 ledger under another network identity fails with `wrong_network`, and its `metadata` rows and accounts are unchanged;
    - `ProviderRuntime::start` with a ledger copied from another signing wallet's deployment fails with `wrong_provider`, and the ledger is unchanged;
    - a start, serve and shutdown with legacy wallet, chain and Drive directories under the base `data_dir` leaves their file digests unchanged.
- [ ] 3.10 Restart and configuration changes never re-term existing agreements.
  - Existing evidence: `app::tests::deployments_share_a_wallet_without_sharing_accounts_or_process_locks` (a restart keeps a free account) and `inference_gateway::tests::renewed_qualification_keeps_existing_model_visible_without_rewriting_agreement`.
  - Missing: `app::tests` with a paid account:
    - a restart with twice the input price reserves and charges at the agreement's rate, and the next offer carries the new price under a higher revision;
    - a restart configured only with another model revision refuses the account's inference before reservation (no request record), keeps its balance, receipts and credentials, and still lets its owner revoke a credential;
    - editing the configuration file of a running provider and running a pass leaves `state.config` and the published offers unchanged.
- [ ] 3.11 Shutdown drains, stops what the daemon owns and preserves durable state.
  - Existing evidence: `daemon_engine_process_test::sigterm_during_engine_startup_stops_the_engine`, `sigint_during_engine_startup_stops_the_engine` and `billing_store::tests::restart_releases_unknown_holds_and_preserves_deduplication`.
  - Missing, besides task 2.2, process tests with a development daemon and an unmanaged stub engine that streams slowly:
    - after `SIGTERM` with one admitted stream, new requests are refused at once, the stream completes, the daemon exits 0, and after a restart the request is `completed`;
    - a stream that outlasts the bound (a test-configurable bound, or a test that waits the full bound) is never recorded as `completed` and is released as `interrupted_unknown_usage` or `restart_unknown_usage`;
    - a public listener failure takes the same path and exits non-zero.
- [ ] 3.12 Logs and operator diagnostics carry no content or credentials.
  - Existing evidence: none; it holds by inspection.
  - Missing: a process test that sends requests whose prompts contain a marker, through completion, an engine failure and cancellation. It collects standard output, standard error and every `GET /admin/*` answer, and asserts that none contains the marker, the account bearer, the administration token, the engine key or the signing key. After the daemon exits, no file in the deployment directory (the ledger with its WAL, `offer-revision`, `provider.lock`) contains the marker. `/admin/requests` lists each request with its identifier, status, usage, cost and timing.

## 4. Qualify and integrate

- [ ] 4.1 Record lifecycle evidence on an isolated, authorized private deployment. Never point `tests/scaled/paid-inference.py` at a shared or live deployment.
  - A drain with an admitted stream that settles.
  - `resume` and `offers/refresh`.
  - `SIGTERM` with an admitted request.
  - Closing the terminal of a macOS daemon after task 2.2.
  - A restart with a changed price.
  - A restore exercise that follows book lines 673–675: a WAL-consistent protected copy of the ledger, acknowledgements, shared journal, keys, configuration and policy cache, restored into a fresh directory and never run beside the original.

  This is not public-testnet or release qualification.
- [ ] 4.2 After tasks 2.x and 3.x are complete, run the following:
  - the full headless test suite under `caffeinate`;
  - `python3 -m unittest discover -s scripts -p 'test_*.py'`;
  - `npm test` and the frontend checks;
  - strict validation.

  Confirm the whitepaper checksum is unchanged.
- [ ] 4.3 Archive with `openspec archive specify-provider-daemon-lifecycle --yes` only after all tasks pass, and after the referenced pending changes are archived, so that the capabilities named in the spec text exist in `openspec/specs`.
  - Those changes are `specify-offer-discovery`, `specify-managed-engine-launch`, `specify-inference-request-accounting`, `specify-generation-performance`, `specify-inference-commercial-terms`, `specify-funding-settlement`, `specify-fee-remittance`, `specify-version-policy`, `specify-outbound-http-safeguards`, `specify-account-credentials`, `specify-opening-funding-intent` and `specify-public-inference-api`.
  - If one of them is abandoned instead, replace the reference before archiving.
  - Never use `--skip-specs`.
