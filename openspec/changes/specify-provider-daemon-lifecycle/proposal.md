## Why

The read-only whitepaper (`docs/chiral-book.md`) states the provider daemon's operating lifecycle in *Native provider provisioning and operations* (lines 605–680). The relevant parts are:

- the separate lifecycle facts (line 623) and the condition table (lines 627–634): failed startup, engine loaded, ready and accepting, temporary chain unavailability, operator drain, managed child failure, performance withdrawal, and shutdown or restart;
- `/healthz`, `/readyz`, `/admin/status` and `/admin/runtime` (line 636);
- the 30-second offer maintenance, and that expiry or drain cancels no acknowledgement and moves no credit (line 638);
- the configuration-driven lifecycle without hot swapping, and preservation of old accounts (line 640);
- operating-system termination of the engine child (line 642), and that provisioning starts no service (line 644);
- upgrades (lines 646–652), and the listener and secret boundaries, the administration helper and its actions, drain before changes, and "Distinguish readiness, admission/draining, ledger health, settlement and publication" (lines 654–667);
- logging, backup, ledger schema 3, explicit file-lock release, incident response and cutover (lines 669–679).

`docs/provider-qualification.md` (lines 199–225 and 458–475), which line 607 points to, describes the start order, the running preflight, the administration actions and the shutdown behaviour. `CLAUDE.md` adds the following:

- `chiral_daemon --config <provider.json>` has distinct public and loopback-only operator listeners;
- operator routes require their protected bearer and reject browser `Origin` requests;
- processes using the same signing wallet share one base `data_dir` and nonce journal;
- schema-3 provider ledgers refuse older schemas without converting money state;
- retiring live services requires an explicit cutover, backups and a rollback plan.

The comparison pass of 2026-11-18 (`docs/apas-worker/comparison-2026-11-18.md`) lists this lifecycle as missing specification item 14. Existing changes already own much of the section:

- `inference-offer-discovery` (pending `specify-offer-discovery`) owns `/healthz` and `/readyz`, the 30-second refresh and 120-second offers, and withdrawal on drain, on a failed refresh and on a managed engine failure. It defers "health, drain, resume, child failure, shutdown and maintenance cadence as a whole" to this item.
- `managed-engine-runtime` (pending `specify-managed-engine-launch`) owns the following: artifact and execution identity (lines 613–619); the failed launch; the occupied port; observation of an engine exit, with no restart; and engine stop on shutdown, on termination during startup and on parent death (line 642).
- `inference-generation-performance` owns the latch that resume and refresh cannot clear. `inference-commercial-terms` owns evidence-only renewal and the immutable copied terms. `inference-request-accounting` owns restart recovery, settlement-failure admission stop and settlement under drain.
- `inference-funding-settlement` owns chain outages, the shared nonce journal and the preserved deployment identity. `inference-fee-remittance` owns remittance during drain and after restart. `client-version-policy` owns lines 646–652 and the policy cache in backups.
- `outbound-http-safeguards` owns the listener boundaries, the administration bearer and `Origin` refusal, and TLS ingress. `chain-rpc-gateway` owns the RPC gateway's bearer and journal. `account-credentials` and `inference-opening-funding-intent` own credential management and funding recovery under drain.

Nothing specifies the rest:

- what the operator status and runtime observation must distinguish;
- what drain refuses and what it keeps running;
- what resume and offer refresh may reopen;
- the order and serialization of a maintenance pass;
- how long a settlement-failure stop lasts;
- which startup failures are fatal;
- one daemon per deployment;
- the ledger identity check;
- what a restart or configuration change may change for existing accounts;
- the daemon's own shutdown path;
- content-free logs and durable state.

A later change could therefore relax any of these without a failing requirement.

## What Changes

- Add the `provider-daemon-lifecycle` capability for the current runtime: 12 requirements and 36 scenarios. Each requirement cites its whitepaper lines and comes from the whitepaper or `CLAUDE.md`, not from current code. Values that the book does not state are recorded in `design.md` as current implementation values: intervals, bounds, status and error codes, response members and file names.
- Specify the operator status (`/admin/status`) and the runtime observation (`/admin/runtime`) as separate reports of the lifecycle facts.
- Specify operator drain, resume and offer refresh.
- Specify the ordered and serialized maintenance pass.
- Specify the admission stop after a durable settlement failure.
- Specify startup refusals, and that a publication failure is not a startup failure.
- Specify one daemon per deployment directory and the ledger identity check without conversion.
- Specify that restarts and configuration changes never re-term existing agreements.
- Specify the daemon's shutdown path on every interceptable termination request, with no reopening once it has drained, and content-free logs, operator diagnostics and durable state.
- In `design.md`, record a coverage table (requirement, code, existing tests, gaps), nine code differences, observations, six questions for the whitepaper author and the risks. The code differences and the missing acceptance evidence are unchecked tasks.

## Capabilities

### New Capabilities

- `provider-daemon-lifecycle`: The provider daemon's operating lifecycle in the current runtime. It covers the following: the operator status and runtime observation; drain, resume and offer refresh; the maintenance pass; the admission stop after a settlement failure; startup refusals; one daemon per deployment; ledger identity; agreements across restarts and configuration changes; the shutdown path; and content-free logs.

### Modified Capabilities

None. The spec references the pending `inference-offer-discovery`, `managed-engine-runtime`, `inference-request-accounting`, `inference-generation-performance`, `inference-commercial-terms`, `inference-funding-settlement`, `inference-fee-remittance`, `client-version-policy`, `outbound-http-safeguards`, `account-credentials`, `inference-opening-funding-intent` and `public-inference-api`, and the normative `inference-account-topup-request`. It adds no obligation to them and does not change their requirements.

The *Shutdown drains, stops what the daemon owns and preserves durable state* requirement names `SIGHUP` and the Windows Ctrl-Break and console-close events beside the `SIGTERM`, `SIGINT` and Ctrl-C of `managed-engine-runtime` *Shutdown and parent termination stop the engines the daemon owns*. It does so as a daemon obligation, without editing that requirement.

The target-design `specify-desktop-share-online` runs this daemon as a desktop child and adds a start-drained option; this change does not present that as current behaviour.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/bin/chiral_daemon.rs`: `Termination`, `main`;
  - `src-tauri/src/app.rs`: `ProviderState::qualified_model`, `ProviderRuntime::start`, the engine watcher, `refresh`, `refresh_inner`, `drain`, `maintenance`, `withdraw_published`, `ready`, `admin_status`, `admin_runtime`, `admin_drain`, `admin_resume`, `admin_refresh`;
  - `src-tauri/src/inference_gateway.rs`: `Settlement::finish`, `Settlement::drop`, the admission checks in `infer`;
  - `src-tauri/src/contracts.rs`: `accepting`, `reconcile`, `reconcile_fees`;
  - `src-tauri/src/billing_store.rs`: `open`, `bind_provider`, `recover_requests`, `admin_summary`;
  - `src-tauri/src/file_lock.rs`, `src-tauri/src/dht.rs` (`start`, `status`, `stop`), `src-tauri/src/inference_backend.rs` (`ModelHealth`, `probe_model`);
  - `scripts/provider-admin.py`, `scripts/provider-preflight.py` and `docker-compose.yml`.
- Nine code differences were found, two of them in the adversarial review. Tasks 2.1–2.9 track them:
  - admission that a durable settlement failure closed is reopened by the next maintenance pass, and the failure is never reported;
  - `SIGHUP` (and, on Windows, Ctrl-Break and console close) ends the daemon by the default action, without drain or settlement, and on macOS orphans the engine;
  - `/admin/status` does not report whether the provider is draining or whether a model is performance-withdrawn;
  - `/admin/status` fails as a whole when the ledger summary cannot be read;
  - a publication failure in the starting maintenance pass aborts startup, although the same failure later only closes publication;
  - the deployment lock is taken after the DHT node has joined under the deployment's peer identity, for a second daemon of the deployment whose listen addresses do not collide;
  - `/admin/runtime` reports engine process liveness as `ready`;
  - after an engine exit, `/admin/status` reports the model not ready with the detail of its last passed probe until the next pass;
  - a `POST /admin/resume` that races shutdown can end the shutdown drain and republish offers.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.12. No test drives `POST /admin/drain`, `POST /admin/resume` or `POST /admin/offers/refresh`. No test checks a drain with admitted work, a shutdown with admitted work, a resume during shutdown, the serialization of a pass with a drain or an engine exit, a ledger of another network or provider, an agreement after a configuration change, or the absence of content in logs and deployment files.
- Six questions for the whitepaper author are recorded in `design.md`. All six are nonblocking:
  - whether a drain survives a restart;
  - what may reopen admission after a settlement failure;
  - whether a publication failure at start is a failed startup;
  - whether a missing ledger beside existing deployment state should be refused;
  - whether deployment machinery may restart the daemon automatically;
  - whether a terminal hang-up and the Windows console events are normal shutdowns.
- Backup procedure, restore rules, recovery and incident drills, legacy-data cutover procedure, provisioning and the Compose deployment, and the relay's own health and readiness are recorded in `design.md` as out of scope, with reasons.
- No wire format, ledger schema, deployment or whitepaper change is part of this specification step. This change grants no live, public-testnet or release qualification.
