## Authority and scope

`docs/chiral-book.md` is the authority and stays read-only. Its SHA-256 was `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933` when this change was written. The requirements come from *Native provider provisioning and operations* (lines 605–680), which was read in full:

- 607 and 609: the qualification guide, the configuration file, and the external endpoint "for calibration, not a substitute for managed runtime evidence".
- *Artifact and execution identity* (613–619): owned by `managed-engine-runtime`, and read here only for what the lifecycle depends on.
- *Provider lifecycle and immutable agreements* (621–644):
  - 623: "Engine liveness, model readiness, serving admission, new-funding readiness, publication and ledger health are separate facts".
  - 627–634: the condition table:
    - 627: failed startup serves and advertises nothing;
    - 628: probes gate readiness;
    - 629: ready and accepting;
    - 630: a chain outage withholds new credit and publication; existing credit stays usable "when the serving, ledger and policy gates remain healthy"; "proven chain rejection is different";
    - 631: drain;
    - 632: child failure; "No automatic generation replay or engine restart. Reconcile requests before operator restart";
    - 633: the latch survives resume and refresh;
    - 634: shutdown and restart preserve state, and orphaned reservations are recovered.
  - 636: `/healthz`, `/readyz`, `/admin/status` ("model, chain, maintenance, ledger and discovery diagnostics") and `/admin/runtime` ("managed-process observations, not a complete signed serving profile").
  - 638: the 30-second refresh and 120-second offers; withdrawal is not instant; expiry or drain cancels no acknowledgement and moves no credit.
  - 640: "configuration-driven, not hot-swapping model weights underneath paid accounts"; changed terms need new offers and distinct agreements; "preserve the old accounts and recovery evidence. Do not spend old balances on silently substituted models".
  - 642: Linux parent-death signal, Windows job object, macOS orphan only after `SIGKILL`, "Never restart over an occupied engine port".
  - 644: provisioning starts no service; Compose is deployment machinery without a standalone engine service, with read-only mounts; the RPC gateway's own bearer and journal.
- *Version policy and upgrades* (646–652): owned by `client-version-policy`.
- *Listener and secret boundaries* (654–667):
  - 656 and 658: the topology and the local-only administration bearer, owned by `outbound-http-safeguards`;
  - 660–667: the helper commands and the other actions. Line 667 adds: "Drain before maintenance or model/pricing changes. Preserve existing agreements' exact terms … `resume` and `offers/refresh` do not clear a performance-withdrawal latch. Distinguish readiness, admission/draining, ledger health, settlement and publication."
- *Backup, incident response, and cutover* (669–679):
  - 671: logging off, and logs and receipts carry identifiers, statuses, counts, costs and timings;
  - 673: drain, settle, stop, take a consistent backup, restore rules, practice recovery;
  - 675: the policy cache in backups, ledger schema 3 refusing older schemas, preserving old ledgers, "A fresh network-scoped ledger is not authority to erase obligations", explicit file-lock release;
  - 677: incident response;
  - 679: cutover without legacy deletion, migration or automatic import.
- `docs/provider-qualification.md`, the guide that line 607 points to:
  - lines 199–225: `--check`, the start order, "Do **not** … restart over an occupied port", and the running preflight;
  - lines 458–475: drain, reconcile and back up before cutover; the administration actions; "`resume` and `offers/refresh` are explicit later actions"; preservation across restart; the shutdown behaviour.
- `CLAUDE.md`:
  - "`chiral_daemon --config <provider.json>` has distinct public and loopback-only operator listeners. Operator routes require their protected bearer and reject browser `Origin` requests";
  - "Processes using the **same signing wallet must share the same base `data_dir`/nonce journal** … Independent hosts without shared journal storage need distinct signing wallets";
  - "Schema-3 provider ledgers refuse older schemas without converting money state";
  - "Retiring/replacing live services requires an explicit cutover, backups, and an operational rollback plan".

Where the code differs, the requirement follows the whitepaper, and an unchecked task records the code change. Requirements are never weakened to match the code. Values that the book does not state are recorded under decision 9 as current implementation values, not as normative constants.

### Out of scope, each with a reason

- **Owned by pending changes**, and referenced instead of restated (see the next section):
  - `/healthz` and `/readyz`;
  - publication, refresh cadence and withdrawal;
  - engine launch, exit and stop;
  - the performance latch;
  - evidence-only renewal;
  - restart recovery of reservations;
  - chain outages and the nonce journal;
  - fee remittance;
  - version policy and upgrades;
  - listener and ingress boundaries;
  - the RPC gateway;
  - credential management;
  - funding recovery.
- **Backup procedure, restore rules and drills** (lines 673, 675 and 677, and the `CLAUDE.md` cutover rule). The following are operator procedures that no daemon code path performs or can verify:
  - a consistent protected backup, including a WAL-consistent SQLite copy;
  - a restore that preserves exact transaction bytes and deduplication;
  - not rolling a stale journal or ledger onto the live chain;
  - not resetting nonce state;
  - not advertising an old signed offer revision as new;
  - practising recovery, and incident and restore drills.

  The book lists the drills as release gates ("Public incident and restore drills remain release gates"), and `docs/llm-serving-migration-plan.md` tracks them. This change specifies only the parts that the daemon enforces: one daemon per deployment (requirement 8), ledger identity (requirement 9), and preservation of state across restart (requirement 10). The offer-revision counter's monotonicity is `inference-offer-discovery`'s. Task 4.1 records a private drain, restart and restore exercise as evidence, not as a requirement.
- **Legacy cutover procedure** (line 679). No importer, bridge or migration exists in the code. Requirement 9 states the daemon's part: it neither imports nor converts nor deletes legacy data. The desktop and consumer handling of legacy data is outside the provider daemon.
- **Provisioning and the Compose deployment** (line 644). These are deployment tooling, outside the daemon's lifecycle. By inspection:
  - `scripts/provision-provider.py` starts no service; its only subprocesses are `whoami`, `icacls` and PowerShell for access-control lists (provision-provider.py:39–60);
  - `docker-compose.yml` has no engine service and mounts `/model` and `/runtime` read-only (docker-compose.yml:35–36).

  The automatic restart policy is open question 5.
- **The relay's own `/api/health` and `/api/ready`** (relay_server.rs). `inference-offer-discovery` deferred them to "relay lifecycle (comparison item 14)", but the book section defines no relay probe semantics. The running preflight only requires both to answer 200 (provider-preflight.py:166–168; `docs/provider-qualification.md` line 225). Specifying them here would invent behaviour. They are left to a relay-lifecycle change.
- **Target design.** `desktop-share-online` runs this daemon as a desktop child, adds a start-drained option (its design decision 3), adds a qualification pause, and changes a running share's token rates, minimum funding or free allowance through an operator-only administration action without a restart. `p2p-provider-transport` adds a relayed grace after offers lapse. When those are implemented, they build on requirements 3 to 5 and 11, and the running repricing action must modify requirement 10's "read its configuration only when it starts" for those values, keeping its rule that existing agreements are never re-termed. This change does not present them as current.

## Relationship to existing and pending specifications

| Specification | Owns | Referenced from |
|---|---|---|
| `inference-offer-discovery` (pending `specify-offer-discovery`) | *The provider publishes fresh offers only while it can accept new funding* (30-second refresh, 120-second offers, withdrawal on drain, failed refresh and engine failure, the revision counter); *Provider liveness and readiness are point-in-time observations, separate from new-funding readiness* (`/healthz`, `/readyz`, `ready` false while draining, after chain rejection, after engine failure, with no ready model or an unreadable ledger) | Requirements 1, 3, 5 and 7. Its open question 1 (a provider whose every model is latched) stays there |
| `managed-engine-runtime` (pending `specify-managed-engine-launch`) | *Startup is bounded, and a failed launch serves and advertises nothing*; *An engine exit withdraws service, and nothing restarts the engine* (including after `POST /admin/resume` and maintenance); *Shutdown and parent termination stop the engines the daemon owns* (`SIGTERM`, `SIGINT`, Ctrl-C, drop, Linux parent-death signal, Windows job, macOS orphan, no adoption); the runtime observation's contents | Requirements 2, 4, 7 and 11. Requirement 11 names further termination requests as a daemon obligation, without changing that requirement (decision 7) |
| `inference-request-accounting` (pending `specify-inference-request-accounting`) | *Terminal request accounting is atomic and at most once* (a persistence failure stops new admission); *Restart recovers orphaned reservations before new admission*; *Quote pressure and new-admission gates cannot strand settlement* (drain and withdrawal do not erase holds); *Maximum reservation and nominal usage cost round once* (agreement rates) | Requirements 3, 6, 7, 10 and 11. Requirement 6 adds how long the stop lasts and what reopens it |
| `inference-generation-performance` (pending `specify-generation-performance`) | *Withdrawal latches durably per model and evidence digest* (resume, refresh and restart cannot clear it) | Requirements 1 and 4 |
| `inference-commercial-terms` (pending) | *Existing agreements permit only evidence-only renewal of identical terms*; *Bounded public quote proposal is not owner authorization* (refused while draining); *Complete signed offer declaration* (size bound) | Requirements 3, 7 and 10 |
| `inference-funding-settlement` (pending) | *Every spender of one wallet shares one coordinated nonce journal* (deployment directory, preserved identity); *Chain outages withhold new credit without inventing loss or switching networks*; credited-funding monitoring | Requirements 5 and 8 |
| `inference-fee-remittance` (pending) | Remittance continues while draining; restart recovers the original remittance; `POST /admin/fees/remit`, and the `provider-admin.py` helper's protected token file, loopback target and disabled proxies | Requirements 3 and 5 |
| `client-version-policy` (pending `specify-version-policy`) | *A provider whose own version policy is unhealthy admits no new work*; *Upgrades preserve accepted state and the policy watermark* (lines 646–652 and the policy cache in backups) | Requirement 5 |
| `outbound-http-safeguards` (pending) | *Provider administration is a separate loopback-only listener with its own protected bearer* (bearer, `Origin` refusal, no `/admin/*` on the public listener, the native administration client); *TLS ingress fronts only the public listener* (preflight's private-route and listener checks) | Requirement 1 |
| `account-credentials` (pending) | *Credential management stays reachable without new admission* (including while draining) | Requirement 3 |
| `inference-opening-funding-intent` (pending), `inference-account-topup-request` (normative) | Fresh intents refused while draining; acknowledged recovery and top-up recovery without new admission | Requirement 3 |
| `public-inference-api` (pending) | *Readiness verifies the template identity and probes every advertised capability before admission and publication* | Requirement 5 |
| `chain-rpc-gateway` (pending) | The gateway's bearer, journal and probes (line 644) | Not referenced from the spec text |
| `profile-path-resolution` (pending) | Relative provider configuration paths | Not referenced from the spec text |
| `desktop-share-online`, `p2p-provider-transport` (target design) | Desktop child daemon, start drained, qualification pause, relayed grace | Not referenced from the spec text |

This capability adds only what those specifications do not state:

- what the operator status and runtime observation must distinguish;
- what drain refuses and what it keeps running, and how long it lasts;
- what resume and offer refresh may reopen;
- the order of a maintenance pass, and its serialization with drain and engine failure;
- how long a settlement-failure stop lasts, and what reopens it;
- which startup failures are fatal;
- one daemon per deployment directory, before it joins the network;
- the ledger's schema, network and provider identity without conversion;
- that configuration changes take effect only by restart and never re-term existing agreements;
- the daemon's own shutdown path on every termination request that it can intercept;
- content-free logs and operator diagnostics.

## Decisions

1. **One capability for the current daemon.** `provider-daemon-lifecycle` covers what `chiral_daemon` does between start and exit, apart from the engine child (`managed-engine-runtime`) and the offers (`inference-offer-discovery`).
2. **Reference the owners, do not restate them.** Each requirement names the owning requirement and adds only the missing obligation. Examples:
   - requirement 3 lists what drain refuses by owner;
   - requirement 6 starts from `inference-request-accounting`'s "SHALL stop new admission" and adds its duration;
   - requirement 11 names the engine stop of `managed-engine-runtime`.
3. **Drain closes inference for existing accounts too.** Line 631 says "Stop new admission", and line 623 counts "serving admission" as one fact. The code refuses every new inference request while draining (`provider_paused`, inference_gateway.rs:456–462 and 591–597), and admitted requests continue. Requirement 3 states this behaviour. Drain closes funding only for new agreements. Quote proposals, fresh funding intents and free openings pass the `accepting` gate (contracts.rs:246, 323 and 383). An opening under an existing acknowledgement and a top-up of an existing agreement do not (contracts.rs:313–352 and 475–504), as `inference-opening-funding-intent`, `inference-account-topup-request` and book lines 495 and 638 require. "Funding ingress" in line 677 is therefore read as funding for new agreements.
4. **A drain lasts for the running daemon.** The book makes drain an operator action "before maintenance or model/pricing changes" (667), followed by a restart with the new configuration (673). It does not say that a drain survives a restart. The code keeps it in memory (app.rs:108 and 210). Requirement 3 adopts that reading. Open question 1 asks whether it should be durable. The target-design start-drained option of `desktop-share-online` is a separate addition.
5. **The maintenance pass order is normative; its implementation is not.** Line 677 requires reconciliation "before resuming", and lines 630 and 638 tie publication to verifiable funding. Requirement 5 therefore fixes the order: probes, then chain observation, then reconciliation, then admission and funding readiness, then publication. The code follows it (app.rs:290–421). How each step is implemented belongs to its owner.
6. **Settlement-failure stops wait for the operator.** `inference-request-accounting` requires the gateway to "stop new admission and retain the original request for recovery rather than suppress the accounting failure", without saying for how long. Lines 630 and 677 make a healthy ledger gate a condition of serving, and reconciliation a step before resuming. Requirement 6 therefore keeps admission closed until `POST /admin/resume` or a restart. This is open question 2's proposed default.
7. **Every interceptable termination request is a normal shutdown.** Line 642 allows a macOS orphan only after "abrupt parent `SIGKILL`", and line 634 requires that shutdown stop owned children and preserve state. A request that the daemon could intercept but leaves to the default action ends it as abruptly as `SIGKILL`. The provider guide tells operators to "Start each following process in its own terminal" (`docs/provider-qualification.md` line 199), so closing that terminal sends `SIGHUP`. Requirement 11 therefore lists `SIGHUP` and the Windows Ctrl-Break and console-close events beside the requests that `managed-engine-runtime` names. `SIGQUIT` is left to its default action (a core dump on request, for debugging; observation 9).
8. **Publication failures are not startup failures.** Line 623 separates publication from serving admission and ledger health, and line 630 lets existing credit be used while the serving, ledger and policy gates are healthy. The code already treats a publication failure in a later pass that way (app.rs:278–288 and 442–455). Requirement 7 applies the same rule to the first pass, leaving configuration, secret, inventory, launch, lock, ledger and recovery failures fatal (line 627). This is open question 3's proposed default.
9. **Current implementation values (not normative).** Each value may change without a spec change if the requirements still hold.
   - **Cadence and bounds.**
     - The periodic pass runs every 30 seconds after the starting pass. The first interval tick is consumed, and missed ticks are skipped (app.rs:442–455).
     - The engine watcher polls every 100 ms (app.rs:223).
     - The shutdown join bound is 75 seconds (chiral_daemon.rs:163). Compose gives the provider a 90-second stop grace (docker-compose.yml:14).
     - The ledger's SQLite busy timeout is 5 seconds (billing_store.rs:308).
   - **Refusal codes.**
     - New inference while not admitting: 503 `provider_paused` (inference_gateway.rs:456–462, and again before reservation at 591–597).
     - New funding: 503 `provider_draining` while not admitting, and 503 `settlement_unavailable` while not new-funding ready (contracts.rs:160–176).
     - A durable settlement failure: 500 with "Durable settlement failed; provider admission has been stopped" (inference_gateway.rs:419–425).
     - Administration without the token or with `Origin`: 401 (app.rs:575–579).
   - **Administration answers.**
     - `POST /admin/drain` answers `{"accepting":false,"offers_published":0}` (app.rs:627–630).
     - `POST /admin/resume` and `POST /admin/offers/refresh` answer `{"accepting":…,"offers_published":n}`, or 500 with the pass's error (app.rs:631–647).
     - `GET /admin/status` has the members `accepting`, `funding_ready`, `chain_error`, `maintenance` (`funding_error`, `fee_error`, `funding_rejected`, `funding_rechecks_quarantined`), `refresh_error`, `network`, `provider_wallet`, `models` (`model_id`, `ready`, `detail`), `dht` (`running`, `peer_id`, `connected_peers`, `listening_addresses`, `published_offers`, `verified_offers`) and `ledger` (app.rs:588–601; contracts.rs:583–590; dht.rs:613).
     - The `ledger` member has `accounts`, `active_requests` (requests still `reserved`), `requests`, `frozen_accounts`, the funding-intent counts and fee policy values (billing_store.rs:1636–1657).
     - `GET /admin/runtime` answers `{"models":[{"model_id","managed","ready","observation"}]}` (app.rs:508–515).
   - **Files.**
     - The deployment directory is `<data_dir>/<hex digest of the canonical network identity>/<wallet>/<peer-id>/` (app.rs:137; provider_config.rs:189–192). It holds `provider.lock` (mode 0600, opened with `O_NOFOLLOW|O_CLOEXEC`, locked with `fs2` `try_lock_exclusive`), `ledger.sqlite3` and `offer-revision` (app.rs:139–152, 166–172).
     - The policy watermark is under `<data_dir>/inference/<namespace>/version-policy` (chiral_daemon.rs:73–77).
   - **Ledger.**
     - SQLite runs with `journal_mode=WAL` and `synchronous=FULL`. The metadata rows are `network`, `schema` (`"3"`) and `provider` (billing_store.rs:313–366 and 400–421).
     - The refusal messages are "performance protocol requires a fresh network-scoped ledger; preserve the old ledger and agreements without conversion", `wrong_network: …` and `wrong_provider: …`.
   - **Termination requests intercepted today**: `SIGTERM` and `SIGINT` on Unix, Ctrl-C on Windows (chiral_daemon.rs:36–66).
   - **Deployment tooling.**
     - Compose: `restart: unless-stopped`, and a healthcheck of `/healthz` every 15 seconds (docker-compose.yml:10 and 45–49).
     - Running preflight: `/admin/runtime` must show one managed model with `ready` and an observation, and `/healthz` and `/readyz` must answer 200 (provider-preflight.py:147–165).
     - `provider-admin.py` sends `POST` for `drain`, `resume`, `offers/refresh` and `fees/remit`, to `127.0.0.1` with proxies disabled (provider-admin.py:17–29).
10. **Status codes and members are not normative.** Requirements 1–4 name facts and outcomes. The codes and JSON members above are implementation values. Tasks 2.3, 2.4 and 2.7 add or rename members; scripts that parse them must be updated in the same change.

## Coverage table

Every test named below passed on 2026-11-20 in focused runs (`cargo test --offline --locked --no-default-features --manifest-path src-tauri/Cargo.toml`):

- **Library**: `app::tests::deployments_share_a_wallet_without_sharing_accounts_or_process_locks`, `app::tests::failed_refresh_withdraws_the_offers_it_published`, `app::tests::readiness_records_the_native_embedding_dimension_only_while_the_model_is_ready`, `app::tests::a_capability_without_a_probe_on_the_model_endpoints_is_not_ready_or_published`, `app::tests::fee_view_lists_confirmed_remittances_and_the_receipt_statement`, `file_lock::tests::exclusion_ends_before_inherited_descriptor_closes`, `billing_store::tests::network_scoped_performance_cutover_refuses_old_ledgers_without_rewriting_money`, `billing_store::tests::restart_releases_unknown_holds_and_preserves_deduplication`, `billing_store::tests::withdrawal_survives_outstanding_burst_late_completion_and_restart_per_model_evidence` and `inference_gateway::tests::renewed_qualification_keeps_existing_model_visible_without_rewriting_agreement`: 10 passed.
- **Integration**: all four tests of `daemon_engine_process_test` (including `sigterm_during_engine_startup_stops_the_engine` and `sigint_during_engine_startup_stops_the_engine`), `inference_owner_control_test::funding_intent_authority_is_required_before_new_admission_and_cached_acknowledgement` and `inference_funding_admission_test::fresh_intent_rechecks_provider_funding_gates_after_balance`: 6 passed.

A manual probe of the existing debug `chiral_daemon` (2026-11-20, scratchpad development configuration with an unmanaged model and an unavailable RPC) showed the following:

- `SIGTERM` exits with status 0;
- `SIGHUP` ends the process with status 129, the signal's default action;
- after `POST /admin/drain`, `/admin/status` has no drain member;
- `/admin/runtime` reports the unmanaged model as `"ready":false`.

All of this is loopback evidence. None of it is deployment evidence.

| # | Requirement (book lines) | Code path | Existing tests | Gaps / differences |
|---|---|---|---|---|
| 1 | The operator status reports each lifecycle fact separately (623, 631, 632, 633, 636, 667, 673) | `admin_status` (app.rs:588–601); engine watcher (221–248); `ModelHealth` (inference_backend.rs:1379–1387); `probe_model` details (1423–1465); `admin_summary` (billing_store.rs:1636–1657); `dht.status` (dht.rs:476–482, 613) | `readiness_records_the_native_embedding_dimension_only_while_the_model_is_ready` (reads `/admin/status`; no `dimension`); `a_capability_without_a_probe_on_the_model_endpoints_is_not_ready_or_published` (model detail); `failed_refresh_withdraws_the_offers_it_published` (`refresh_error` set, read from the runtime, not the route) | **Code differs** (code differences 1, 3, 4 and 8). No route test of the status after a drain, a chain rejection, a latch, an engine exit, an unreadable ledger, or with an admitted request |
| 2 | The runtime observation reports processes, not readiness or a serving profile (623, 628, 636) | `admin_runtime` (app.rs:508–515); preflight (provider-preflight.py:147–149) | None in Rust; `managed-engine-runtime` task 3.10 expects "not ready" after an exit | **Code differs** (code difference 7). No test of a live engine with a failing probe or of an unmanaged model |
| 3 | Operator drain stops new admission and new funding while admitted work and recovery continue (631, 638, 667, 673, 677) | `drain` (app.rs:427–440); `admin_drain` (627–630); `infer` (inference_gateway.rs:456–462, 591–597); funding gate (contracts.rs:160–176, applied at 246, 323 and 383); top-up and acknowledged opening outside the gate (313–352, 475–504); periodic pass while draining (app.rs:290–421) | `funding_intent_authority_is_required_before_new_admission_and_cached_acknowledgement` and `fresh_intent_rechecks_provider_funding_gates_after_balance` (fresh intents refused as `provider_draining` and cached acknowledgements recovered, with the flag set directly, not by `POST /admin/drain`) | No test calls `ProviderRuntime::drain` or `POST /admin/drain`. Missing: new inference refused before reservation with an admitted stream completing; account control, a top-up and a pass while drained; a repeated drain changing no money |
| 4 | Resume and offer refresh reopen only through a complete maintenance pass (633, 667, 677) | `admin_resume` (app.rs:631–639); `admin_refresh` (640–647); `refresh` (278–288) | `withdrawal_survives_outstanding_burst_late_completion_and_restart_per_model_evidence` (latch survives restart, at the ledger) | No test calls `POST /admin/resume` or `POST /admin/offers/refresh` |
| 5 | Each maintenance pass observes and reconciles before it reopens or publishes (627–634, 636, 638, 677) | `refresh_inner` order (app.rs:290–421); `lifecycle` lock (111, 279, 431, 240); `maintenance` (442–455); first pass at start (219); `reconcile` (contracts.rs:603–653) | `failed_refresh_withdraws_the_offers_it_published` (a failed pass withdraws everything it published); `fee_view_lists_confirmed_remittances_and_the_receipt_statement` (remittance through the administration route) | No test of a drain or an engine exit overlapping a pass; no test that a pass runs while drained and after an engine failure; serialization holds by inspection |
| 6 | Admission closed by a ledger persistence failure stays closed until the operator acts (630, 677) | `Settlement::finish` (inference_gateway.rs:406–427); `refresh_inner` recomputes `accepting` (app.rs:356–362) | None | **Code differs** (code difference 1). `inference-request-accounting` task 3.1 lists the stop itself as missing evidence |
| 7 | Startup refuses what it cannot verify, and a publication failure is not a startup failure (623, 627, 630, 632) | `ProviderRuntime::start` (app.rs:118–250, first pass at 219); identity check (162–165); `main` (chiral_daemon.rs:121–126) | `deployments_share_a_wallet_without_sharing_accounts_or_process_locks` (starts with RPC 503); `start_refuses_an_engine_key_that_repeats_another_credential_before_any_engine_starts` | **Code differs** (code difference 5). No test of a rejected chain identity at start, or of a publication failure at start |
| 8 | One daemon owns a deployment's state for its lifetime (673, 675; `CLAUDE.md`) | DHT start (app.rs:128–135; dht.rs:411–424 dials bootstrap peers); lock (app.rs:139–152); `ExclusiveFileLock` (file_lock.rs:1–24) | `deployments_share_a_wallet_without_sharing_accounts_or_process_locks` (a second start on the same deployment fails; a restart finds the account); `exclusion_ends_before_inherited_descriptor_closes` | **Code differs** (code difference 6). The second-start test asserts only `is_err()`, not the lock error, and not the absence of a DHT connection |
| 9 | The provider ledger opens only under its own schema, network and provider (675, 679) | `BillingStore::open` (billing_store.rs:303–385); `bind_provider` (400–421) | `network_scoped_performance_cutover_refuses_old_ledgers_without_rewriting_money` (schemas 1 and 2 refused; account, quote and schema unchanged) | No test of a ledger of another network or provider wallet at open; legacy data untouched holds by inspection (the daemon writes only under the deployment directory and `inference/<namespace>`) |
| 10 | Restart and configuration changes never re-term existing agreements (634, 640, 652, 667, 675, 677) | `ProviderConfig::load` once in `main` (chiral_daemon.rs:71); `Arc<ProviderConfig>` (app.rs:40); `qualified_model` and `accepts_execution` (app.rs:90–97; protocol.rs:136–152); reservation at the account's price (billing_store.rs:1079) | `deployments_share_a_wallet_without_sharing_accounts_or_process_locks` (restart keeps a free account); `renewed_qualification_keeps_existing_model_visible_without_rewriting_agreement` | No test of a price change across restart, a replaced model, or account control for an agreement no deployment accepts. No hot reload holds by inspection (the configuration is read once) |
| 11 | Shutdown drains, stops what the daemon owns and preserves durable state (631, 634, 642, 673) | `Termination` (chiral_daemon.rs:23–66); shutdown path (153–181); `admin_resume` (app.rs:631–639); `Settlement::drop` (inference_gateway.rs:430–439) | `sigterm_during_engine_startup_stops_the_engine`, `sigint_during_engine_startup_stops_the_engine` (termination during startup); `restart_releases_unknown_holds_and_preserves_deduplication` (restart recovery at the ledger); `tests/scaled/paid-inference.py` (SIGKILL and restart, against an authorized isolated deployment only, not run) | **Code differs** (code differences 2 and 9). No test of `SIGTERM` while serving with an admitted request, of a request that outlasts the bound, or of a resume during shutdown |
| 12 | Logs and operator diagnostics carry no content or credentials (671) | No request logging in `app.rs`, `inference_gateway.rs`, `inference_backend.rs`, `contracts.rs` or `billing_store.rs` (no `println!`, `eprintln!` or logger); `RequestView` (billing_store.rs:100) and `AccountView` (protocol.rs:567–590) | None | Holds by inspection, including the ledger's request records (billing_store.rs:100–116). No test collects the daemon's output, administration answers and deployment files for a marker |

## Code differences

Each difference has an unchecked task in section 2 of `tasks.md`.

1. **Admission closed by a durable settlement failure reopens at the next maintenance pass, and the failure is never reported** (requirements 1 and 6; book lines 630 and 677; `inference-request-accounting` *Terminal request accounting is atomic and at most once*).
   - When `settle_measured` fails, `Settlement::finish` stores `accepting = false` and answers 500 (inference_gateway.rs:406–427).
   - Every maintenance pass recomputes `accepting` from chain rejection, version policy, drain, engine liveness and model readiness only (app.rs:356–362). The next periodic pass, at most 30 seconds later, therefore reopens admission when those gates are open, and so do `POST /admin/offers/refresh` and `POST /admin/resume` (631–647).
   - Nothing records the failure for the operator. `refresh_error` holds only pass errors (app.rs:281), and `/admin/status` has no member for it (588–601). The only trace is a request left `reserved`, seen in `/admin/requests` and in `ledger.active_requests`.
   - The same pattern applies to a clock failure during settlement (inference_gateway.rs:381–390).
   - Severity medium: a recurring persistence fault (for example, a full disk) admits new paid requests every 30 seconds. Each one reserves and dispatches before its settlement fails, and leaves another hold that only a restart releases. No money is lost or invented.
   - Verified by reading; no test injects a settlement failure. Task 2.1.
   - **Fixed 2026-11-20** (task 2.1): `Settlement::finish` records a fixed reason in `ProviderState::settlement_fault` when it closes admission, also when the drop retry then records the request (observation 13). `refresh_inner` stores admission through `ProviderState::store_admission`, which the fault overrides under the same lock, so neither the periodic pass nor `POST /admin/offers/refresh` reopens it. Only `POST /admin/resume` (`ProviderRuntime::resume`, which clears the fault under the lifecycle lock before its pass; since task 2.10 it clears only the failures recorded before the resume request arrived) or a restart reopens admission. `/admin/status` reports the fault as `settlement_error`. The recording also closes new-funding readiness at once, and the pass stores its new-funding readiness under the same lock, so `funding_ready` is never reported true while the fault holds admission closed. Regression: `app::tests::a_settlement_failure_keeps_admission_closed_until_the_operator_resumes`.
2. **`SIGHUP`, and on Windows Ctrl-Break and console close, end the daemon by the default action** (requirement 11; book lines 634 and 642).
   - `Termination` registers only `SIGTERM` and `SIGINT` on Unix and Ctrl-C on Windows (chiral_daemon.rs:36–66). Any other termination request ends the process without the shutdown path: no drain, no withdrawal, no settlement within the bound, no `stop_engines`, no DHT stop (chiral_daemon.rs:153–181).
   - The provider guide starts each process "in its own terminal" (`docs/provider-qualification.md` line 199), so closing that terminal ends the daemon this way.
   - On macOS the engine child, which runs in its own process group (`managed-engine-runtime` decision 9), is not stopped. It stays on its port, and the next start fails with an occupied port, as after `SIGKILL`. On Linux the parent-death signal, and on Windows the job object, stop the engine.
   - Admitted requests stay `reserved` until the next start recovers them, and published offers stay in remote caches until they expire.
   - Severity medium-low.
   - Reproduced on this host on 2026-11-20: the existing debug daemon answered `SIGTERM` with exit status 0, and `SIGHUP` with status 129, the default action. Task 2.2.
   - **Fixed 2026-11-20** (task 2.2): `Termination` also creates a `SIGHUP` stream on Unix, and Ctrl-Break and console-close streams on Windows, before startup. Each takes the same path as `SIGTERM`: during startup the engines already started are stopped and the daemon exits with status 1 without serving; while serving, the shutdown path runs and the daemon exits with status 0. Regressions in `tests/daemon_engine_process_test.rs`: `sighup_during_engine_startup_stops_the_engine` and `sighup_while_serving_stops_the_engine_and_keeps_the_deployment`, with the control `sigterm_while_serving_stops_the_engine_and_keeps_the_deployment`. Without the change the daemon ends by the signal and, on macOS, the engine survives on its port. The Windows events are verified by reading only. A `SIGHUP` that the daemon started ignoring, as under `nohup`, stays ignored (Risks): no hang-up stream is created then, because tokio's handler would replace that disposition. Regression `sighup_stays_ignored_when_the_daemon_starts_ignoring_it`.
3. **`/admin/status` does not report whether the provider is draining, or whether a model is performance-withdrawn** (requirement 1; book lines 633 and 667).
   - The status reports `accepting` (app.rs:592), but the `draining` flag (app.rs:108) is never reported. A drain is indistinguishable from:
     - a rejected chain identity (only `chain_error` distinguishes that);
     - an unhealthy version policy;
     - a failed engine;
     - no ready model;
     - a settlement failure (code difference 1).
   - The per-model entries are `ModelHealth` (`model_id`, `ready`, `detail`; inference_backend.rs:1379–1387). A latched model whose probes pass is reported `ready: true` with the detail "Ready: …", although `qualified_model` refuses it (app.rs:95). A latched model whose slots are busy is reported as "Initial qualification awaits idle engine capacity" (inference_backend.rs:1441–1444).
   - After a failed `POST /admin/resume`, which ends the drain before its pass (app.rs:634), the operator cannot tell from the status whether the drain ended.
   - Severity low. Confirmed by the manual probe: after `POST /admin/drain`, the members were `accepting`, `chain_error`, `dht`, `funding_ready`, `ledger`, `maintenance`, `models`, `network`, `provider_wallet` and `refresh_error`. Task 2.3.
   - **Fixed 2026-11-20** (task 2.3): `/admin/status` adds `draining` and, for each model, `performance_withdrawn`, read from the ledger when the status is read (`null` when it cannot be read). A latched model's `detail` names the withdrawal first and then gives the latest pass's readiness reason, and `ready` stays the probe result. Regressions: `app::tests::admin_status_distinguishes_a_drain_from_a_rejected_chain_identity` and `app::tests::admin_status_names_a_performance_withdrawn_model_and_keeps_its_readiness`.
4. **`/admin/status` fails as a whole when the ledger summary cannot be read** (requirement 1; book lines 636 and 667).
   - The member `"ledger": runtime.state.ledger.admin_summary()?` (app.rs:599) turns any ledger read error into an error answer for the whole route. Model readiness, admission, funding readiness, chain, maintenance, refresh and discovery diagnostics are then unavailable exactly when the operator needs to tell ledger health from the rest.
   - Severity low. Verified by reading. Task 2.4.
   - **Fixed 2026-11-20** (task 2.4): a failing `admin_summary` is reported as `"ledger": null` with `"ledger_error": "Ledger summary unavailable"`, and the route still answers 200 with every other member. `/readyz` is unchanged. Regression: `app::tests::admin_status_reports_an_unreadable_ledger_and_every_other_fact`.
5. **A publication failure in the starting maintenance pass aborts startup** (requirement 7; book lines 623 and 630).
   - `ProviderRuntime::start` ends with `runtime.refresh().await?` (app.rs:219). Every publication failure of that first pass therefore makes the start fail, and the daemon exits (chiral_daemon.rs:121–126). These failures are:
     - a revision that cannot be persisted (app.rs:369–372);
     - an offer that cannot be signed or identified (397–402);
     - a registry refusal, for example an offer above the size bound (offer_registry.rs:279–281);
     - a DHT advertisement refused because the provider record store is full (dht.rs:555–580).
   - The same failure in a later pass only closes new-funding readiness and withdraws offers. `accepting` was already stored before publication (app.rs:362), the error is kept in `refresh_error` (281), and the periodic loop continues (452).
   - A restart after a crash can therefore leave existing funded accounts unserved and administration unavailable, because of a transient DHT condition.
   - Severity low. Verified by reading; `failed_refresh_withdraws_the_offers_it_published` injects the failure only after start. Task 2.5.
6. **The deployment lock is taken after the DHT node has joined under the deployment's peer identity** (requirement 8; book line 673).
   - `ProviderRuntime::start` starts the `DhtService` (app.rs:128–135), which listens and dials every bootstrap peer at once (dht.rs:411–424). Only then does it derive the deployment directory from the peer ID (app.rs:137) and try the lock (139–152).
   - The window needs listen addresses that do not collide. A second process with the *same* configuration file fails earlier: `chiral_daemon` binds the public and administration listeners before `ProviderRuntime::start` (chiral_daemon.rs:115–116), and a fixed DHT port already in use fails `listen_on` synchronously (dht.rs:400–407; libp2p-tcp 0.41.0 binds in `listen_on`, and port reuse is off under `tcp::Config::default()`, dht.rs:380), before any dial. Provisioned providers use fixed ports (provision-provider.py:246). The window therefore opens for a second daemon of the same deployment, with the same data directory, signing key and DHT identity file, whose listeners and DHT listen address differ: an ephemeral `/tcp/0` DHT listen address, as in the `app::tests` fixtures (app.rs:719), or a restored copy configured with other ports on the same host, which is the case line 673 forbids.
   - Such a daemon connects to its bootstrap peers under the live deployment's peer ID before it is refused. The reference bootstrap peer is the relay. In that window, a peer that asks this identity for its manifest can receive an empty, complete manifest. `inference-offer-discovery` *Only a complete, valid, single-snapshot manifest withdraws that peer's cached offers* then withdraws the live provider's cached offers until the next complete search.
   - Severity low: the window lasts from the DHT start to the lock refusal, and needs a non-colliding copy of the configuration.
   - Verified by reading; `deployments_share_a_wallet_without_sharing_accounts_or_process_locks` (`/tcp/0`) asserts only that the second start fails. Task 2.6.
7. **`/admin/runtime` reports engine process liveness as `ready`** (requirement 2; book lines 623, 628 and 636).
   - `admin_runtime` sets `"ready": engine.is_some_and(|engine| engine.is_alive())` (app.rs:512). A live managed engine whose model fails its readiness probes is reported `ready: true`. A model on an operator-owned engine, whose readiness probes may pass, is always reported `ready: false`; the manual probe showed this.
   - The running preflight treats the field as "managed runtime not healthy/observed" (provider-preflight.py:148–149). The qualification recipe asserts `rows[0]['ready']` (`docs/provider-qualification.md` line 255). Both mean liveness.
   - Severity low. Task 2.7.
8. **After an engine exit, `/admin/status` reports the model not ready with the detail of its last passed probe** (requirement 1; book lines 623, 632 and 636). Found in review.
   - The engine watcher sets `ready` to false on the failed model's health entry and leaves `detail` unchanged (app.rs:243–245).
   - Until the next maintenance pass replaces the entry with "Managed engine exited" (inference_backend.rs:1424–1426), `/admin/status` reports `"ready": false` with the detail "Ready: private health, tokenizer/template identity and advertised inference probes passed; …" (inference_backend.rs:1458–1459).
   - Severity low: the window lasts until the next periodic pass, and `/readyz` and `/admin/runtime` already report the exit. Verified by reading. Task 2.8.
   - **Fixed 2026-11-20** (task 2.8): the engine watcher sets the failed model's `detail` to `inference_backend::ENGINE_EXITED` ("Managed engine exited", shared with `probe_model`) in the same write that sets `ready` false. Regression: `app::tests::admin_status_reports_an_engine_exit_with_its_own_reason`, which uses the managed test constructor `engine_runtime::ManagedEngine::helper_for_test` (`managed-engine-runtime` task 3.10).
9. **A `POST /admin/resume` that races shutdown can end the shutdown drain** (requirement 11; book lines 631, 634 and 673). Found in review.
   - The shutdown path drains (chiral_daemon.rs:160) and only then tells the listeners to stop accepting (161). Graceful shutdown lets requests on connections already accepted finish (138–144 and 163–171).
   - `admin_resume` clears `draining` without any shutdown check, then runs a pass (app.rs:631–639). `drain` sets `draining` and takes the lifecycle lock in one poll when the lock is free (app.rs:427–431). A resume whose handler clears `draining` after that waits for the lock, and its pass then runs after the shutdown drain has returned. It finds the provider not draining, so it reopens admission and new-funding readiness and publishes offers (app.rs:356–413) while the daemon shuts down.
   - The listeners no longer accept connections, but the published offers stay in remote caches until they expire, after the DHT stops (chiral_daemon.rs:177).
   - Severity low: it needs an operator resume concurrent with a termination request. Verified by reading. Task 2.9.
   - **Fixed 2026-11-20** (task 2.9): the shutdown path calls `ProviderRuntime::drain_for_shutdown`, which sets a shutdown drain state, which only rises, before it takes the lifecycle lock, and then closes admission and withdraws the offers under the lock. A resume ends only an operator drain. It is refused (409 `shutting_down`) without a pass when the shutdown state is set on its arrival or once it holds the lifecycle lock, and a pass admits only when no drain is in effect. Regression: `app::tests::a_resume_during_shutdown_does_not_reopen_the_provider`.

No other difference from the whitepaper was found. The following match the requirements:

- drain's refusals, its continuing settlement, account control and maintenance, and its withdrawal before it answers;
- the order of the maintenance pass, its ledger gate (a reconciliation failure closes admission, app.rs:336–341), and its serialization with drain and engine failure by the `lifecycle` lock (app.rs:240, 279 and 431);
- passes that keep running after a failed pass (`let _ = runtime.refresh()`, app.rs:452), while drained, after an engine failure and during a chain outage;
- resume and offer refresh through one pass, without restarting an engine or clearing a latch;
- startup refusal of configuration, secrets, inventories, launch, lock, ledger and chain rejection, and the start with an unavailable RPC;
- the ledger's schema, network and provider checks without conversion;
- configuration read once, and agreements executed only when accepted and charged at their own rates (billing_store.rs:1066 and 1079);
- apart from code differences 2 and 9, the `SIGTERM` and `SIGINT` shutdown path, including requests still running when the bound ends: once the server tasks are aborted, `stop_engines` ends a managed engine's streams, and the runtime's exit drops the remaining request tasks, whose `Settlement::drop` records them as `interrupted` (inference_gateway.rs:430–439), or leaves them to startup recovery;
- no `/admin/*` route on the public router (app.rs:464–477);
- no request content in logs, administration answers or the ledger's request records (`RequestView`, billing_store.rs:100–116).

## Observations

1. **Liveness is not answered during startup.** The listeners are bound before `ProviderRuntime::start` but served only after it returns (chiral_daemon.rs:115–144). While engines load, `/healthz` connections wait in the backlog. A Compose healthcheck (docker-compose.yml:45–49, with no start period) can mark a loading provider unhealthy. The book's "`GET /healthz` reports daemon liveness" (line 636) does not say whether it answers during startup. `inference-offer-discovery` owns `/healthz`, so no requirement or task is added here.
2. **A failed resume has already ended the drain.** `admin_resume` clears `draining` before its pass (app.rs:634). When the pass fails at publication, admission is open for existing accounts while the answer is 500. Requirement 4 allows this, because the answer reports a failure and the status shows the state once task 2.3 adds the drain member.
3. **`--check` does not open the ledger or take the lock** (chiral_daemon.rs:94–111). A ledger that the start would refuse (requirement 9) passes `--check`. The guide says `--check` validates "config, credentials and managed inventories, **not live inference**" (`docs/provider-qualification.md` line 199), so this is not a difference.
4. **The ledger is touched before its network and provider are checked.** `BillingStore::open` creates missing tables, indexes and triggers (billing_store.rs:319–343 and 367–380) before it compares the recorded network (354–357). `bind_provider` runs after `open` has committed (app.rs:166–170). For a ledger written by the current code these statements change nothing. For an older schema-3 ledger they add schema objects, but never money state, before the refusal. Requirement 9 forbids changing agreements, credit, acknowledgements, obligations, ownership and the recorded identity, which holds.
5. **Ledger expiry sweeps are not part of the maintenance pass.** Quotes, challenges, rate windows and old rejected funding attempts are pruned inside ledger transactions (`cleanup_pending`, billing_store.rs:568–577), and owned by the proposal-admission specifications. The pass's own steps are those of requirement 5.
6. **Shutdown aborts a pass in progress** (chiral_daemon.rs:158). A remittance or funding recheck interrupted at an await point is recovered from the journal under `inference-fee-remittance` and `inference-funding-settlement`. An offer published by the interrupted pass but not yet recorded in `published` (app.rs:407–411) is withdrawn only when the DHT stops at the end of shutdown, and is refused for proposals because admission is closed.
7. **A second termination request during the shutdown wait is not observed** (chiral_daemon.rs:153–176). Shutdown always waits up to its bound, and only `SIGKILL` shortens it. The book does not ask for a faster second stop.
8. **The engine watcher repeats its withdrawal** every 100 ms after an engine has failed (app.rs:221–248). This is harmless; it keeps admission closed against a racing pass.
9. **`SIGQUIT`** keeps its default action, a core dump on request (decision 7).
10. **Daemon output is text, not JSON.** The start line and the `--check` line go to standard output as text (chiral_daemon.rs:109 and 145–149). The `CLAUDE.md` rule "CLI stdout is machine-readable JSON or explicit streaming events" names the CLI, and the book section does not address the service's output, so no task is added.
11. **A start that fails after the DHT has started** drops its `DhtService`, which ends the swarm when its command channel closes (dht.rs:597–616), and drops started engines (`managed-engine-runtime`). Nothing is served, because the listeners are served only after a successful start.
12. **A ledger file without a `metadata` table is treated as new.** `BillingStore::open` checks the schema only when a `metadata` table exists (billing_store.rs:309–318), then creates the missing tables and records schema 3 and the configured network (319–366), whatever other tables the file holds. Every ledger written since the baseline commit has that table, and the deployment directory is scoped by network digest, wallet and peer ID (app.rs:137), so no ledger that this code wrote can reach this path. Requirement 9 refuses ledgers that *record* another schema, which holds. No task is added; a later change that meets pre-metadata ledgers should refuse them.
13. **A failed settlement is retried once when its owner is dropped.** `answer` drops the `Settlement` right after `finish` (inference_gateway.rs:767–769), and `Drop` calls `finish("interrupted")` while the request is unsettled (430–434). If the ledger accepts that second write, the request settles as `interrupted` with its known usage instead of staying `reserved`, and admission stays closed under requirement 6. The status and charge of that retry belong to `inference-request-accounting`. The regression of task 2.1 keeps the ledger refusing writes until after the retry.
14. **The engine key file is read again for every readiness probe.** `probe_model` builds a `Backend` for each pass (inference_backend.rs:1445), and `Backend::new` reads `engine_api_key_file` (524–538), while the gateway keeps the backend it built at start (inference_gateway.rs:59). Replacing that file under a running daemon changes what the probes send, not any configured term, so requirement 10 holds. Engine key handling is `managed-engine-runtime`'s.

## Open questions for the whitepaper author

All six are nonblocking: no requirement or task waits on an answer, and the spec adopts the proposed default.

1. **Does an operator drain survive a restart?** Lines 631, 667 and 673 describe drain before maintenance, changes and upgrades, and do not say that it lasts beyond the running daemon. The options are:
   - (a) drain is a state of the running daemon, and a restart serves again after startup recovery, as now;
   - (b) a durable drain that only `resume` clears;
   - (c) a start-drained option, as the target-design `desktop-share-online` adds.

   Proposed default, which is what the spec adopts: the book text, (a). Requirement 3 says the drain lasts until resume or exit.
2. **What may reopen admission after a durable settlement failure?** `inference-request-accounting` stops admission and does not say for how long. Lines 630 and 677 make a healthy ledger gate and reconciliation conditions of serving and resuming. The options are:
   - (a) only `POST /admin/resume` or a restart;
   - (b) a maintenance pass that proves the ledger can be written again;
   - (c) any maintenance pass, as now.

   Proposed default, which is what the spec adopts: (a), from lines 630 and 677. Requirement 6 and task 2.1 follow it.
3. **Is a publication failure in the starting pass a failed startup?** Line 627 forbids serving after "failed startup". Line 623 separates publication from serving admission. Line 630 keeps existing credit usable while the serving, ledger and policy gates are healthy. The options are:
   - (a) start, serve existing accounts and report the failure, as after the same failure in a later pass;
   - (b) exit, as now.

   Proposed default, which is what the spec adopts: (a), from lines 623 and 630. Requirement 7 and task 2.5 follow it.
4. **Should a missing ledger beside existing deployment state be refused?** Line 675 says "A fresh network-scoped ledger is not authority to erase obligations". If `ledger.sqlite3` is lost while `offer-revision` and the DHT identity remain, the daemon creates an empty ledger and serves again, and earlier consumers' bearers no longer authenticate. The options are:
   - (a) refuse to create a ledger in a deployment directory that already holds provider state, unless the operator explicitly confirms;
   - (b) create it, as now, with the rule left to operator procedure.

   Proposed default, which is what the spec adopts: the book text, (b). Line 675 states an operator rule, not a detection. No requirement or task is added.
5. **May deployment machinery restart the daemon automatically?** The reference Compose service has `restart: unless-stopped` (docker-compose.yml:10). Line 632 forbids "automatic … engine restart" after a child failure while the daemon stays alive, and the daemon does not exit then. Line 634 allows a restart that recovers orphaned reservations. The options are:
   - (a) automatic restart after the daemon exits is a restart under line 634, as now;
   - (b) no automatic restart, so that the operator reconciles first ("Reconcile requests before operator restart", line 632).

   Proposed default, which is what the spec adopts: the book text, (a). A start that keeps failing serves nothing (requirement 7), and each restart recovers reservations before admission.
6. **Is a terminal hang-up or a Windows console event a normal shutdown?** Line 642 names only an abrupt parent `SIGKILL` as leaving a macOS orphan, and line 634 requires shutdown to stop owned children and preserve state. `managed-engine-runtime` names `SIGTERM`, `SIGINT` and Ctrl-C. The options are:
   - (a) every termination request that the daemon can intercept takes the shutdown path, including `SIGHUP`, Ctrl-Break and console close, as requirement 11 states;
   - (b) only the requests that `managed-engine-runtime` names, with operators told to run the daemon under a service manager or `nohup`, as now.

   Proposed default, which is what the spec adopts: the book text, (a), because line 642 allows an orphan only after `SIGKILL`. Requirement 11 and task 2.2 follow it. Windows ends a process a short, system-defined time after a console close, so on Windows the path can be cut short; requirement 11 then relies on the job object and startup recovery.

## Risks

- **Task 2.1 turns a transient ledger fault into an operator-attended outage.** That is the fail-closed direction that `inference-request-accounting` and line 630 ask for, but it needs the status member of task 2.3 or 2.4 so that the operator can see why.
- **Task 2.2 changes what `SIGHUP` does.** Operators who closed a terminal to stop the daemon will now see an orderly stop, bounded like `SIGTERM`. Processes started under `nohup` keep ignoring `SIGHUP`.
- **Tasks 2.3, 2.4 and 2.7 change administration JSON.** `provider-admin.py`, the desktop Provider page and the CLI print it raw. The running preflight and the qualification recipe parse `/admin/runtime`'s `ready`; update `scripts/provider-preflight.py`, `scripts/qualify-provider.py` and `docs/provider-qualification.md` line 255 in the same change. Keep `/readyz` unchanged.
- **Task 2.5 lets a daemon run without ever publishing.** A configuration whose offer always fails to publish now runs indefinitely with `refresh_error` set, instead of exiting. Operators must watch `/admin/status`.
- **Task 2.6 changes the start order.** The DHT identity is loaded, and the deployment directory derived, before the swarm starts. The identity file rules of `inference-offer-discovery` *DHT indexes point to candidate peers within one network, under a stable peer identity* must still apply.
- **Task 2.9 makes resume refuse during shutdown.** An operator who sends `resume` while the daemon stops now gets a refusal instead of a short reopening; restart the daemon to serve again.
- **Timing-sensitive fixtures.** The serialization tests of task 3.5, and the shutdown tests of tasks 2.2 and 3.11, depend on stub engines and deadlines. Use generous margins.
- **What the evidence is.** All executable evidence is unit, loopback or process-level testing on one host. A private drain, resume, restart, shutdown and restore exercise is task 4.1. No evidence here is public-release qualification.
