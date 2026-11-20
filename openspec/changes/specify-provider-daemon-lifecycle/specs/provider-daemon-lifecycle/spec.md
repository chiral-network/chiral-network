## ADDED Requirements

### Requirement: The operator status reports each lifecycle fact separately

`GET /admin/status`, served only on the administration listener under `outbound-http-safeguards` *Provider administration is a separate loopback-only listener with its own protected bearer*, SHALL report these facts as separate members (book lines 623, 636 and 667: "Distinguish readiness, admission/draining, ledger health, settlement and publication"):

- for each configured model, its readiness from the latest readiness pass or from a later observed engine exit, with a reason that agrees with that readiness when it is not ready, and whether a durable performance-withdrawal latch (`inference-generation-performance` *Withdrawal latches durably per model and evidence digest*) blocks its admission (book lines 632 and 633);
- whether the provider admits new inference, and, as a separate member, whether it is draining (book lines 631 and 667);
- new-funding readiness (book lines 630 and 636);
- the chain observation, including a rejected network identity, and the latest funding and fee reconciliation results (book lines 630 and 636);
- the error of the latest failed maintenance pass, and a durable settlement failure that closed admission under *Admission closed by a ledger persistence failure stays closed until the operator acts*;
- ledger health, and the number of admitted requests that have not yet settled, so that the operator can tell when in-flight work has settled (book lines 631 and 673);
- discovery and publication: the DHT state and the offers the provider currently publishes (book line 636).

When one of these facts cannot be read, the status SHALL report that fact as unavailable and SHALL still report the others. The status is an unsigned operator observation; reading it SHALL NOT change admission, publication, balances or agreements. `GET /healthz` and `GET /readyz` are specified by `inference-offer-discovery` (*Provider liveness and readiness are point-in-time observations, separate from new-funding readiness*); this requirement does not change them.

#### Scenario: A drain is distinguishable from other closed admission

- **GIVEN** a ready provider that admits new inference
- **WHEN** the operator drains it
- **THEN** `GET /admin/status` reports that the provider does not admit new inference and that it is draining
- **AND** when instead the configured RPC reports another genesis and no drain was requested, the status reports that the provider does not admit new inference, that it is not draining, and that the chain identity was rejected

#### Scenario: A latched model is named as performance-withdrawn

- **GIVEN** a profiled model whose readiness probes pass and whose admitted report is latched
- **WHEN** the operator reads `GET /admin/status`
- **THEN** that model is reported as performance-withdrawn and not admissible, and its readiness result is still reported

#### Scenario: An engine exit is reported with its own reason

- **GIVEN** a ready managed model whose readiness probes passed
- **WHEN** its engine exits and the operator reads `GET /admin/status` before the next maintenance pass
- **THEN** the model is reported as not ready, and its reason names the engine exit, not passed readiness probes

#### Scenario: An unreadable ledger does not hide the other facts

- **GIVEN** a provider whose ledger summary cannot be read
- **WHEN** the operator reads `GET /admin/status`
- **THEN** the answer reports the ledger as unavailable
- **AND** it still reports model readiness, admission, drain, new-funding readiness, the chain observation, maintenance results and discovery

#### Scenario: The status shows when in-flight work has settled

- **GIVEN** a drained provider with one admitted streaming request still generating
- **WHEN** the operator reads `GET /admin/status`
- **THEN** it reports one admitted request not yet settled
- **AND** after that request settles, it reports none

### Requirement: The runtime observation reports processes, not readiness or a serving profile

`GET /admin/runtime` SHALL report, for each configured model, whether it runs on a managed engine and, for a managed engine, whether the engine process is alive and the launcher's observation of it (book line 636: it "reports managed-process observations, not a complete signed serving profile"; the observation's contents are specified by `managed-engine-runtime`). It SHALL NOT present engine liveness as model readiness: a live engine whose model fails its readiness probes SHALL NOT be reported as ready, and a model on an operator-owned engine SHALL NOT be reported as not ready merely because it has no managed process (book lines 623 and 628: "A process or `/health` response alone is not qualification"). The observation SHALL NOT be presented as a complete signed serving profile, as qualification evidence or as permission to publish (book line 636).

#### Scenario: A live engine whose model is not ready

- **GIVEN** a managed engine whose process is alive and whose model fails its template probe
- **WHEN** the operator reads `GET /admin/runtime`
- **THEN** the engine is reported alive with its observation
- **AND** the model is not reported as ready

#### Scenario: An exited engine is reported as not alive

- **GIVEN** a managed engine whose process has exited
- **WHEN** the operator reads `GET /admin/runtime`
- **THEN** the engine is reported as not alive, and the model is not reported as ready

#### Scenario: An operator-owned engine has no process observation

- **GIVEN** a model served by an operator-owned external engine
- **WHEN** the operator reads `GET /admin/runtime`
- **THEN** the model is reported as unmanaged, without a process observation, and the answer does not describe its readiness as failed

### Requirement: Operator drain stops new admission and new funding while admitted work and recovery continue

`POST /admin/drain` SHALL at once stop admitting new inference requests and close new-funding readiness, and SHALL withdraw the provider's published offers before it answers (book line 631). Withdrawal of offers and the `/readyz` answer while draining are specified by `inference-offer-discovery`. While the provider is draining:

- a new inference request SHALL be refused before tokenization and reservation, also on an existing funded account (book line 631: "Stop new admission");
- new quote proposals, fresh funding intents and free openings SHALL be refused as their owners specify (`inference-commercial-terms` *Bounded public quote proposal is not owner authorization*, `inference-opening-funding-intent`);
- an opening under an existing funding acknowledgement, top-up settlement of an existing agreement, and their recovery SHALL continue as their owners specify (`inference-opening-funding-intent`; `inference-account-topup-request` *Top-up recovery preserves originals without new admission*; book line 638: draining "does not cancel an accepted funding acknowledgement or move existing credit");
- requests admitted before the drain SHALL run to completion and settle under `inference-request-accounting` (book line 631: "Let in-flight work settle before maintenance");
- request status, receipts, cancellation, credential management (`account-credentials` *Credential management stays reachable without new admission*), and the maintenance pass, including funding monitoring and fee remittance (`inference-fee-remittance`), SHALL continue (book lines 631 and 677).

Draining SHALL NOT refund, cancel, delete or change any agreement, balance, reservation, credential, funding acknowledgement, fee obligation or journal (book lines 631 and 638: "draining is not a refund or deletion of agreements"). Repeating `POST /admin/drain` SHALL have the effect of one drain. A drain is a state of the running daemon: it SHALL remain in effect until `POST /admin/resume` ends it, outside shutdown (*Shutdown drains, stops what the daemon owns and preserves durable state*), or the daemon exits. Drain is the operator's control for stopping new admission, publication and funding ingress for new agreements before maintenance, a model or pricing change, an upgrade or during an incident (book lines 667, 673 and 677).

#### Scenario: Admitted work settles while new work is refused

- **GIVEN** a funded account with one streaming chat request in progress, and a published offer
- **WHEN** the operator drains the provider
- **THEN** a new request on the same account with a new `Idempotency-Key` is refused, and no request is recorded or reserved for that key
- **AND** the streaming request completes and settles with its authoritative usage
- **AND** the offer is no longer published

#### Scenario: Recovery and maintenance continue while draining

- **GIVEN** a drained provider with a funded account and a credited funding
- **WHEN** the account owner reads a request's status and its receipt, rotates a credential and settles a top-up, and the next maintenance pass runs
- **THEN** each account operation answers as it would without the drain, and the top-up is credited to the existing agreement
- **AND** the pass rechecks the credited funding
- **AND** a new quote proposal for the provider's offer is refused, and the account's agreement and fee obligations are unchanged apart from the top-up's own credit and fee

#### Scenario: Drain changes no money

- **GIVEN** a provider with accounts, a reservation, a funding acknowledgement and a pending fee obligation
- **WHEN** the operator drains it twice
- **THEN** new inference is refused
- **AND** every account, reservation, acknowledgement, fee obligation and journal entry is unchanged

### Requirement: Resume and offer refresh reopen only through a complete maintenance pass

`POST /admin/resume` SHALL end a drain and then run one maintenance pass at once. `POST /admin/offers/refresh` SHALL run one maintenance pass at once and SHALL NOT end a drain (book line 667). Neither SHALL reopen admission or new-funding readiness, or publish an offer, except through that pass, under *Each maintenance pass observes and reconciles before it reopens or publishes* (book line 677).

Neither action SHALL:

- restart or replace an engine (`managed-engine-runtime` *An engine exit withdraws service, and nothing restarts the engine*);
- clear a performance-withdrawal latch (book lines 633 and 667; `inference-generation-performance` *Withdrawal latches durably per model and evidence digest*);
- change any agreement, balance or credential.

`POST /admin/offers/refresh` SHALL NOT reopen admission closed under *Admission closed by a ledger persistence failure stays closed until the operator acts*. Each answer SHALL state whether the provider then admits new inference and how many offers it publishes. When the pass fails, the answer SHALL report a failure, and `GET /admin/status` SHALL show the resulting state.

#### Scenario: Resume reopens a healthy provider

- **GIVEN** a drained provider whose engine, chain, ledger and version policy are healthy and whose model is ready
- **WHEN** the operator calls `POST /admin/resume`
- **THEN** the provider admits new inference, is new-funding ready and publishes its offers under a new revision
- **AND** the answer reports that it admits new inference and the number of published offers

#### Scenario: Resume does not pass a failed gate

- **GIVEN** a drained provider whose only managed engine has exited
- **WHEN** the operator calls `POST /admin/resume`
- **THEN** the drain ends, but the provider still does not admit new inference and publishes no offer
- **AND** no engine process is started

#### Scenario: Offer refresh does not end a drain

- **GIVEN** a drained provider
- **WHEN** the operator calls `POST /admin/offers/refresh`
- **THEN** a maintenance pass runs and rechecks funding
- **AND** the provider is still draining, admits no new inference and publishes no offer, and the answer says so

### Requirement: Each maintenance pass observes and reconciles before it reopens or publishes

The daemon SHALL run a maintenance pass once at start, before it serves any request, then periodically while it runs, and for each `POST /admin/resume` and `POST /admin/offers/refresh` (book lines 636 and 638). The periodic cadence and offer lifetime are specified by `inference-offer-discovery` (*The provider publishes fresh offers only while it can accept new funding*). Each pass SHALL take these steps in order (book lines 627–634 and 677):

1. run the readiness probes of every configured model (`public-inference-api` *Readiness verifies the template identity and probes every advertised capability before admission and publication*);
2. observe the configured chain's identity and its `safe` frontier;
3. recheck credited funding and remittance batches, and attempt due remittance (`inference-funding-settlement`, `inference-fee-remittance`);
4. decide admission and new-funding readiness from the gates of book lines 627–634. Admission SHALL be open only when the ledger is healthy (reconciliation in this pass could read and update it; book line 630: "when the serving, ledger and policy gates remain healthy"), the chain identity is not rejected, the provider's own version policy is healthy (`client-version-policy` *A provider whose own version policy is unhealthy admits no new work*), the provider is not draining, no managed engine has failed and at least one model is ready. New-funding readiness SHALL be open only when admission is open, the chain's `safe` frontier is available and funding reconciliation found no unresolved fault;
5. publish or withdraw offers (`inference-offer-discovery`).

A pass SHALL NOT reopen new-funding readiness or publish an offer before its reconciliation step has completed in that pass (book line 677: "Reconcile pending chain submissions and fee batches before resuming"). Passes SHALL keep running while the provider drains, after a managed engine fails, during a chain outage and while a model is latched, so that reconciliation, monitoring and remittance continue (book lines 630–633). A failed pass SHALL leave new-funding readiness closed and nothing published (`inference-offer-discovery`), SHALL be reported in `GET /admin/status`, and SHALL NOT stop later passes.

Passes, drains and the withdrawal after an engine exit SHALL be serialized. Once `POST /admin/drain` has answered, or an engine exit has been observed, no pass that started earlier SHALL leave admission open or an offer published (book lines 631 and 632).

#### Scenario: A drain during a pass leaves nothing published

- **GIVEN** a maintenance pass that has decided admission open and is publishing offers
- **WHEN** the operator drains the provider before the pass completes
- **THEN** after the drain has answered and the pass has completed, the provider admits no new inference and its manifest serves no offer

#### Scenario: An engine exit during a pass leaves nothing published

- **GIVEN** a maintenance pass that has decided admission open and is publishing the offer of a managed model
- **WHEN** that model's engine exits before the pass completes
- **THEN** after the pass has completed, the provider admits no new inference and its manifest serves no offer

#### Scenario: Passes continue while drained and after an engine failure

- **GIVEN** a drained provider whose managed engine has exited, with a credited funding
- **WHEN** the next periodic pass runs
- **THEN** the credited funding is rechecked
- **AND** the provider still admits no new inference and publishes no offer

### Requirement: Admission closed by a ledger persistence failure stays closed until the operator acts

When the provider cannot durably record a request's settlement, it closes admission at once under `inference-request-accounting` (*Terminal request accounting is atomic and at most once*). It SHALL then keep admission closed until the operator acts: no maintenance pass and no `POST /admin/offers/refresh` SHALL reopen it (book line 630: existing credit stays usable "when the serving, ledger and policy gates remain healthy"; book line 677). `GET /admin/status` SHALL report the failure until the operator acts. Only `POST /admin/resume` or a restart SHALL reopen admission, and only through a maintenance pass. At a restart, startup recovery settles the request (`inference-request-accounting` *Restart recovers orphaned reservations before new admission*). The unsettled request SHALL stay recorded for recovery and SHALL NOT be generated again.

#### Scenario: A later pass does not reopen admission

- **GIVEN** a ready provider whose ledger refuses settlement writes
- **WHEN** a request completes and its settlement fails, the ledger then accepts writes again, and the periodic pass and `POST /admin/offers/refresh` both run
- **THEN** new inference is still refused
- **AND** `GET /admin/status` reports the settlement failure, and the request is still recorded as reserved

#### Scenario: Resume or restart reopens admission

- **GIVEN** a provider whose admission was closed by a settlement failure, and whose ledger accepts writes again
- **WHEN** the operator calls `POST /admin/resume`
- **THEN** a maintenance pass runs and, when every gate is open, the provider admits new inference
- **AND** when the operator restarts the daemon instead, startup recovery settles the request as `restart_unknown_usage` before the first new request is admitted

### Requirement: Startup refuses what it cannot verify, and a publication failure is not a startup failure

The daemon SHALL serve no request, publish no offer and exit with an error when any of these fails (book line 627):

- its configuration or a protected secret;
- a managed inventory or engine launch (`managed-engine-runtime`);
- the deployment lock (*One daemon owns a deployment's state for its lifetime*);
- the ledger identity (*The provider ledger opens only under its own schema, network and provider*);
- startup recovery (`inference-request-accounting`).

A temporarily unavailable chain RPC SHALL NOT stop the start. The daemon then starts with new-funding readiness and publication closed, and serves existing accounts when the other gates are healthy (book line 630). A rejected chain identity is different: it SHALL stop the start (book line 630: "proven chain rejection is different").

A failure to sign an offer, persist its revision or publish it during the starting maintenance pass SHALL NOT stop the daemon (book lines 623 and 630: publication is a separate fact from serving admission and ledger health). The daemon SHALL start with new-funding readiness and publication closed. It SHALL serve administration, and serve existing accounts, as it does after the same failure in a later pass. It SHALL report the failure in `GET /admin/status`, and SHALL publish from a later pass once the failure clears.

#### Scenario: An unavailable chain RPC does not stop the start

- **GIVEN** a provider configuration whose chain RPC answers HTTP 503
- **WHEN** the daemon starts
- **THEN** it serves administration and `GET /readyz` with `funding_ready` `false`, and publishes no offer

#### Scenario: A rejected chain identity stops the start

- **GIVEN** a provider configuration whose chain RPC reports another chain identity
- **WHEN** the daemon starts
- **THEN** it exits with an error, and no listener serves a request

#### Scenario: A publication failure at start leaves the daemon running

- **GIVEN** a provider with a funded account, whose offer cannot be published (for example, because the signed offer exceeds the offer size bound of `inference-commercial-terms` *Complete signed offer declaration*)
- **WHEN** the daemon starts
- **THEN** it keeps running, and `GET /admin/status` reports the failed pass
- **AND** it is not new-funding ready, publishes no offer, and admits the funded account's inference when its model is ready

### Requirement: One daemon owns a deployment's state for its lifetime

The daemon SHALL hold an exclusive lock on its deployment directory until it exits. `inference-funding-settlement` (*Every spender of one wallet shares one coordinated nonce journal*) places that directory under the signing wallet and the DHT peer ID. The daemon SHALL acquire the lock before it joins the DHT under that deployment's peer identity, opens the deployment's ledger or starts an engine (book line 673: do not "run two restored provider copies concurrently"). A second daemon for the same deployment SHALL be refused before it serves a request, publishes an offer, starts an engine or connects to any peer under that identity.

The lock SHALL be released explicitly when its owner ends, even while an engine child or another process keeps an inherited descriptor of the lock file (book line 675: "Explicit file-lock release protects process ownership even if another process retains an inherited descriptor"). Different deployments of one wallet keep separate locks and share the wallet journal under `inference-funding-settlement`. This requirement adds no coordination between independent hosts, which need distinct signing wallets (`CLAUDE.md`: "separate local journals are not a nonce coordinator").

#### Scenario: A second daemon is refused before it joins the network

- **GIVEN** a running daemon for deployment `D`
- **WHEN** a second daemon starts for `D`, with the same data directory, signing key and DHT identity file but public, administration and DHT listen addresses that do not collide with the first daemon's
- **THEN** it exits with an error that names the deployment lock
- **AND** no peer receives a connection from `D`'s peer ID made by the second process, and no engine is started

#### Scenario: The lock does not outlive its owner

- **GIVEN** a lock owner whose lock file descriptor is also held by a child process
- **WHEN** the owner releases the lock
- **THEN** another process can acquire it while the child still holds the descriptor

#### Scenario: A restarted daemon serves the same deployment

- **GIVEN** a daemon for deployment `D` with an open account, which then exits
- **WHEN** a daemon starts again with the same configuration
- **THEN** it acquires the lock and finds the account with its balance and status unchanged

### Requirement: The provider ledger opens only under its own schema, network and provider

The daemon SHALL open its deployment ledger only when the ledger records ledger schema 3, the configured complete network identity and the signing wallet's provider address. It SHALL record the schema and network identity when it creates a new ledger and the provider address when it first binds that ledger, and SHALL NOT change a recorded value afterwards (book line 675). It SHALL refuse a ledger that records an older or different schema, another network identity or another provider, and it SHALL then not start (*Startup refuses what it cannot verify, and a publication failure is not a startup failure*).

A refusal SHALL NOT convert, migrate or delete the ledger's agreements, credit, acknowledgements, fee obligations or transaction and nonce ownership, and SHALL NOT change the ledger's recorded schema, network or provider (book line 675: it "refuses older schemas; it does not migrate or convert historical agreements/credit"). The provider lifecycle SHALL NOT import, convert or delete legacy wallet, chain, Drive, hosting or file-transfer data, and SHALL NOT treat a legacy balance as credit (book line 679).

#### Scenario: An older schema is refused without conversion

- **GIVEN** a ledger that records schema 2 and holds a funded account
- **WHEN** the daemon opens it
- **THEN** it refuses and does not start
- **AND** the account record, its signed quote and the recorded schema are unchanged

#### Scenario: A ledger of another network or provider is refused

- **GIVEN** a ledger file copied into the deployment directory from a deployment of another network identity or of another signing wallet
- **WHEN** the daemon starts
- **THEN** it refuses and does not start
- **AND** the ledger's accounts and its recorded network and provider are unchanged

#### Scenario: Legacy data is left in place

- **GIVEN** a base `data_dir` that also holds legacy wallet, chain and Drive directories
- **WHEN** the daemon starts, serves and shuts down
- **THEN** every legacy file is unchanged, and no legacy balance appears in any account

### Requirement: Restart and configuration changes never re-term existing agreements

The daemon SHALL read its configuration only when it starts. A change of model, artifact, runtime, hardware, price, admission setting or any other configured term SHALL take effect only through a restart, never by swapping a model or a setting under a running daemon (book line 640: "configuration-driven, not hot-swapping model weights underneath paid accounts"). The operator drains before such a change (book line 667).

Across a restart, with the same or a changed configuration, every existing account SHALL keep its agreement, balance, credentials, requests, receipts, funding acknowledgements and evidence. Reservations left by the previous process are recovered under `inference-request-accounting` (book line 634). Inference under an existing agreement SHALL be admitted only while a configured deployment accepts that agreement under `inference-commercial-terms` (*Existing agreements permit only evidence-only renewal of identical terms*). Otherwise it SHALL be refused before reservation, and the account's status, receipts, credential management and recovery SHALL stay available (book line 640: "preserve the old accounts and recovery evidence. Do not spend old balances on silently substituted models"). Charges SHALL use the agreement's own rates whatever price is configured (`inference-request-accounting` *Maximum reservation and nominal usage cost round once*). Changed terms reach consumers only through new offers and new agreements (book line 640).

No lifecycle action, startup refusal or error path SHALL delete or rewrite accounts, keys, journals, funding acknowledgements, the offer-revision counter or recovery evidence to clear an error (book lines 634 and 677: "Do not delete accounts or keys to clear an error").

#### Scenario: A price change does not re-price an existing account

- **GIVEN** a funded account whose agreement has input rate `R`
- **WHEN** the operator drains, changes the configured input price to `2R` and restarts the daemon
- **THEN** the account's next request is charged at rate `R`
- **AND** the new offers advertise `2R` under a revision higher than any earlier one

#### Scenario: A replaced model does not spend an old balance

- **GIVEN** a funded account for model revision `M`
- **WHEN** the daemon restarts configured only with model revision `M'`
- **THEN** the account's inference request is refused before reservation, and no hold is created
- **AND** its balance, receipts and credentials are unchanged and readable, and its owner can still revoke a credential

#### Scenario: Editing the configuration of a running daemon changes nothing

- **GIVEN** a running daemon
- **WHEN** its configuration file is edited to another price and model
- **THEN** the daemon keeps serving and publishing the configuration it started with until it is restarted

### Requirement: Shutdown drains, stops what the daemon owns and preserves durable state

On every termination request that it can intercept, the daemon SHALL take its shutdown path. These requests include `SIGTERM`, `SIGINT` and a terminal hang-up (`SIGHUP`) on Unix, and Ctrl-C, Ctrl-Break and closing the console window on Windows (book lines 634 and 642: only an abrupt `SIGKILL` may leave a macOS engine orphan). The shutdown path SHALL take these steps in order (book lines 631, 634 and 673: drain, "let requests settle where possible, stop services cleanly", stop owned children):

1. stop the periodic maintenance pass;
2. drain (*Operator drain stops new admission and new funding while admitted work and recovery continue*);
3. stop accepting new connections;
4. let admitted requests settle within its shutdown bound;
5. stop its managed engines as `managed-engine-runtime` (*Shutdown and parent termination stop the engines the daemon owns*) requires;
6. leave the DHT and exit.

The daemon SHALL also take this path when one of its listeners fails, and then exit with an error. Once the shutdown path has drained, no administration action, including a `POST /admin/resume` received before the listeners stopped accepting, SHALL end that drain, reopen admission or new-funding readiness, or publish an offer (book line 673: "stop services cleanly"). When the operating system ends the process before the path completes, as Windows does a short, system-defined time after a console close, the steps not taken are recovered as after an abrupt end: the engine is stopped as `managed-engine-runtime` provides for an abrupt end of the daemon, and the next start recovers unsettled requests under `inference-request-accounting`. A request that has not settled when the bound ends SHALL be released under the unknown-usage rule, at shutdown or by the next start's recovery (`inference-request-accounting`). It SHALL NOT be charged with an invented token count (book line 634). Shutdown SHALL NOT delete or rewrite keys, the DHT identity, ledgers, journals, funding acknowledgements, the offer-revision counter or accepted terms (book line 634).

#### Scenario: Termination lets an admitted request settle

- **GIVEN** a provider with a published offer and one admitted streaming request
- **WHEN** the daemon receives `SIGTERM`
- **THEN** its offers are withdrawn and new requests are refused at once
- **AND** the admitted request completes and settles before the daemon exits within its bound, and the daemon exits successfully

#### Scenario: A terminal hang-up is a normal shutdown

- **GIVEN** a daemon with a managed engine, started from a terminal, either still starting or serving
- **WHEN** it receives `SIGHUP`
- **THEN** it exits through its own shutdown path, not by the signal's default action
- **AND** its engine process no longer exists, on macOS as on Linux

#### Scenario: A resume during shutdown does not reopen the provider

- **GIVEN** a daemon that has received `SIGTERM` and whose shutdown path has drained it
- **WHEN** a `POST /admin/resume` that the administration listener had already received is handled
- **THEN** the provider still admits no new inference, is not new-funding ready and publishes no offer

#### Scenario: A request that outlasts the bound is released, not charged

- **GIVEN** a request still generating when the shutdown bound ends
- **WHEN** the daemon exits
- **THEN** the request is released under the unknown-usage rule, at shutdown or by the next start's recovery
- **AND** it is never recorded as completed with usage the engine did not report, and its account's hold is released

### Requirement: Logs and operator diagnostics carry no content or credentials

The provider daemon SHALL keep prompt and completion logging off (book line 671). Its process output and logs SHALL NOT contain:

- prompts, completions, embedding inputs or vectors, or tool arguments;
- any credential, key or bearer secret.

Its administration routes SHALL describe requests only by identifiers, statuses, token counts, costs, timings and performance outcomes, not by content (book line 671: "Logs and receipts should contain request IDs, statuses, token counts, costs, and timings, not content or credentials"). The provider daemon discloses no retention of request content, so it SHALL NOT persist prompts, completions, embedding inputs or vectors, or tool arguments in its ledger or any other durable state (book line 671: "Do not treat bearer access, TLS, or signed outputs as permission to retain user content without disclosure"). The managed engine's own output is disabled under `managed-engine-runtime`. Receipt contents are specified by `inference-request-accounting`.

#### Scenario: Neither content nor secrets reach logs or administration

- **GIVEN** requests whose prompts contain a marker string, sent with an account bearer, that complete, fail at the engine and are cancelled
- **WHEN** the daemon's standard output and standard error, every `GET /admin/*` answer and, after the daemon exits, every file in its deployment directory are collected
- **THEN** none of them contains the marker, and none of the output or answers contains the account bearer, the administration token, the engine key or the signing key
- **AND** `GET /admin/requests` lists each request with its identifier, status, usage, cost and timing
