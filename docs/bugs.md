# Known bugs, limitations, and qualification gaps

This register applies to the **LLM-only runtime**, not the retired file-sharing application. See the [current book](chiral-book.md) for operating contracts and the [migration plan's evidence/open checklist](llm-serving-migration-plan.md#14-execution-evidence-and-open-release-checklist) for the complete first-public-release gates. A missing qualification is not proof of a defect; a passing unit test is not proof of public readiness.

## Release blockers / unexercised qualification

### Native CFX/performance implementation is not public-release qualification

- **Where:** native wallet/settlement, signed model/offer/quote/receipt contracts, provider provisioning and public deployment.
- **Implemented:** native Conflux eSpace testnet chain **71**, genesis `0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804`; safe-receipt settlement, dynamic pinned HF artifacts, GPU-bound performance evidence, admission limits, inference waivers and durable withdrawal. Chiral-coin is deferred; no mainnet or new pool is authorized.
- **Current llama.cpp evidence:** native Linux CPU/CUDA execution with pinned v0.9.0 / b12110, Qwen Q4_K_M generation and separate Nomic Q8_0 embeddings. Stock Python 3.41.0 / Node 7.45.1 SDKs, generated-token logprobs, echo, both response modes, exact usage, cancellation, disconnect, child failure, replay rejection and same-ledger restart were exercised. Both CPU and zoo-6 passed the 4,096-token context boundary and uncharged overflow rejection. Final new-agreement reservations were zero.
- **Historical evidence:** prior native-CFX vLLM CPU/GPU runs, native Linux desktop generation, top-up recovery and platform-fee arrival remain recorded in the plan. They are not requalification of llama.cpp or permission to convert old agreements.
- **Qualification gap:** zoo-6's 8 GB RTX 4060 remains dedicated to private inference with its miner stopped. Its persistent service now runs managed llama.cpp, but every smoke profile remains `serving:null`. Runtime-bound sustained-arrival qualification, Windows/macOS hardware and installers, larger capacity profiles, Vulkan memory observation and public TLS deployment remain open.
- **Safe operation:** calibration stays loopback-only and unpublished. Do not accept public clients or advertise speed guarantees from synthetic/unit evidence. Preserve historical ledgers and obligations; schema 3 refuses old ledgers without converting credit.

### Full model, SDK, multi-provider, and failure matrix remains open

- **Where:** provider gateway/engine, consumers, settlement, discovery and deployment qualification.
- **Historical slice only:** isolated chiral-coin Qwen chat/completions/SSE and BGE embeddings through stock Python/Node SDKs and native Linux; opening, fee remittance, top-up cancellation/offline recovery; paid concurrency/credential cases; free provider; crash recovery; embedded native and non-root release-container startup. These observations remain valid for that stack, not automatically the new CFX/performance protocol.
- **Remaining constraint:** representative launch models, complete free/paid feedback, multi-provider/adversarial/reorg/fee/failure matrix, GPU containers/public HTTPS, supported-platform installers and security/distributed/load/soak tests. llama.cpp is the only supported adapter; alternate-engine comparisons are optional research, not a retained serving path.
- **Safe operation:** advertise only capabilities actually qualified for the pinned model/engine/tokenizer profile. Dynamic HF download does not support arbitrary remote-code/tokenizer architectures. Retain all gates in the plan; provisioning or defining a CI job closes none.

### Same signing wallet requires shared nonce journal

- **Where:** `src-tauri/src/chain_client.rs`, provider and consumer `data_dir` configuration.
- **Symptom:** independent copies of a signing-wallet journal cannot coordinate nonce selection, funding, cancellation, and fee remittance for the same wallet.
- **Constraint:** locking coordinates a single shared journal, not separate host-local databases. Provider ledger/lock isolation must not be confused with wallet-nonce isolation.
- **Safe operation:** all spenders of the same wallet/network must share the configured journal base or use distinct signing wallets. Do not put the same private key in unrelated datadirs and assume provider deployment separation makes it safe. Back up and restore journals consistently with accepted funding/fee state.

## Deliberate limitations, not silent fallbacks

- **Non-refundable provider credit:** no escrow, withdrawal, automatic balance transfer, or recourse protocol. A dishonest/offline provider can strand funds. Keep deposits small; providers receive signed funding bytes and could broadcast them before an acknowledgement reaches the consumer.
- **Unknown usage:** missing/invalid/lost authoritative usage releases the unsettled reservation at provider expense with an explicit unknown-usage status. Zero charge is not proof that no tokens were generated. Reservation-sized provider loss exposure is not a measured GPU-work bill.
- **Performance remedy:** a qualified miss waives only inference cost, preserving known authoritative usage and idempotent settlement. Funding/platform fees and gas are not refunded. Per-token engine IDs establish the clock; SSE event counts and synthetic tests do not qualify a speed class.
- **Recovery/cancellation:** chain broadcast ambiguity is not failure. Resume the original intent; explicit same-nonce cancellation spends gas and may lose the race. There is no automatic fee bump or transparent paid-generation retry.
- **Model/metadata trust:** signatures attribute offers/receipts but do not attest the claimed model ran or that metering was honest. The provider sees prompts. Testnet feedback is experimental and not strong economic Sybil resistance.
- **Wallet history and migration:** history comes from durable local submissions/receipts, not a complete chain indexer. Legacy files/chain data/history remain on disk but are not automatically imported; no CHI/CHR-to-CFX conversion or reputation carryover exists. Schema-3 refusal must not be bypassed by deleting old state.
- **Browser preview:** Vite preview has no native wallet/payment/inference simulation. Native flows require Tauri or the CLI/SDK.
- **Completion echo/logprobs:** the prompt is preserved exactly, but llama.cpp does not supply input-token probabilities. Prompt-only streamed chunks use `logprobs:null`; non-streaming probability arrays cover generated tokens only. No invented prompt probabilities or silent removal of echo.
- **Token timing:** authoritative final usage can include hidden end-of-generation tokens absent from per-token instrumentation. Billing remains exact, but an incomplete token clock cannot certify decode speed.
- **Embedding dimensions:** native 768-dimensional Nomic output was exercised; arbitrary requested reduction is not silently emulated. Wrong response dimensions fail validation.
- **Native packaging/lifecycle:** the Linux CUDA server archive requires separate compatible CUDA runtime/cuBLAS libraries. The private package binds their verified bytes and licenses. Windows uses kill-on-close jobs; Linux uses parent-death/process-group termination. Abrupt macOS parent `SIGKILL` may orphan the engine; normal shutdown stops it and occupied ports prevent accidental reuse.

## Fixed regressions and hardening with follow-up evidence

The evidence below is from the **historical isolated chiral-coin prototype** unless explicitly labeled otherwise. It records real fixes/results, not requalification of the native CFX/performance cutover.

### Evidence-only qualification renewal preserves existing prepaid accounts

- **Current native-CFX design/fix:** user approved strictly newer evidence for an otherwise identical serving deployment. Original agreements, prices, fees and balances remain unchanged.
- **Reproduced failure:** the regression rejected renewed execution with `performance outcome differs from immutable agreement`.
- **Change:** qualification gates select compatible current evidence; new reservations bind that evidence durably and check its withdrawal latch transactionally. Settlement rejects a substituted report, and crash recovery retains the admitted identity. Existing accepted funding remains recoverable; new admission checks the current report. No ledger reset or credit transfer is involved.
- **Verification:** the full native-CFX headless run passed 186 tests with 1 ignored. Regression coverage includes current-report admission, immutable original terms, substituted-report rejection, durable withdrawal and crash recovery. Hardware qualification and public deployment remain open; synthetic accounting fixtures are not performance certificates.

### Pending top-up displayed the opening transaction hash

- **Where:** `src-tauri/src/consumer.rs::pending_accounts`.
- **Observed current native-CFX failure:** an actual pending 0.01 CFX top-up displayed the account's original opening hash, misleading wallet recovery/status tracking.
- **Change:** derive the displayed top-up hash from its durably saved signed bytes; return no transaction hash before those bytes exist. Opening intents retain their own hash. No journal migration or replacement payment is introduced.
- **Verification:** the extended dropped-broadcast recovery regression failed before the fix; all 25 consumer tests passed afterward. Rebuilt CLI displayed the real top-up hash, resumed the original payment once, preserved the application credential and left no pending operation after repeated resume. Evidence: `topup-pending-corrected.json` and `topup-recovery-evidence.json` in the current native-CFX qualification directory.


### Repeated funding recovery invalidated an exported API credential

- **Where:** `src-tauri/src/consumer.rs`, saved-account `fund`/`resume` behavior.
- **Observed failure:** replayed opening did not duplicate funding, but an unnecessary open rotated the account credential and the previously exported key received HTTP 401.
- **Change:** refresh the already saved open account without issuing another open. Credential rotation remains an explicit user action.
- **Evidence:** 20 targeted consumer tests passed after the fix. Rebuilt CLI actual `resume` and repeated `fund` returned active, funded total stayed `10000000000000000` wei, the exported credential was unchanged and accepted (HTTP 200). Local report: `/var/tmp/chiral-inference-qualification-20261028/credential-recovery-report.json`. This is a resolved regression, not a claim that every credential/funding scenario passed.

### Authenticated opening retries rotated an existing root credential

- **Current native-CFX mismatch:** the ledger treated a fresh owner proof on `POST /v1/contracts/open` as permission to revoke roots and issue another secret. The saved-native-account refresh fix above did not cover direct HTTP replay or a lost first-opening response.
- **Correction:** first creation returns a root secret once; existing-account opening returns the unchanged account with `credential: null`, preserving credential authority and exact funding/fee totals. Providers keep token hashes, not recoverable plaintext secrets.
- **Recovery:** native `resume` saves a recovered account even when the original secret was lost. Explicitly issue a credential to restore a missing default; reopening never revives expired/revoked credentials or replaces existing application keys.
- **Verification:** credential/hold/spend/funding conservation, revoked/expired roots, concurrent one-secret creation, production-router free/paid replay and native post-commit response-loss acceptance pass. Native recovery explicitly issues a new infer/read default without revoking the undisclosed original. All 408 Rust tests and 61 frontend/provisioning tests pass after correcting a paid fixture to supply its original transaction; available builds/checks pass. These isolated tests do not establish live-chain, engine or power-loss qualification.

### Managed embeddings could select a generation chat template

- **Native-CFX mismatch:** inventory validation accepted an embedding artifact's selected template file when its hashes matched, and ignored a non-null template selector. The provisioner already prohibited such selection.
- **Correction:** require explicit `chat_template_name: null` and no inventoried selected template file for embeddings, including zero-length files. Refuse missing/non-null selectors rather than defaulting or discarding them. Keep shared byte/size, model identity and empty-template-digest checks; generation handling is unchanged.
- **Verification:** three native regressions reject correctly bound templates and invalid selectors, admit a template-free inventory, reject same-size model-byte corruption and a mismatched empty-template identity, and restore admission after correction. All 488 available tests, available builds/checks and strict commercial-change validation pass. Synthetic inventory bytes and native executable headers are not executable GGUF, readiness or hardware qualification; these tests launch no engine.

### Native embedding configuration permitted nonzero output prices

- **Native-CFX mismatch:** direct provider configuration checked canonical monetary syntax but did not enforce the input-only embedding rule. Only the optional reference provisioner rejected a nonzero embedding output rate.
- **Correction:** shared native configuration validation requires canonical zero output price on every embedding-capable model, regardless of model/capability position, development mode or managed-runtime selection. Generation pricing is unchanged. Invalid configuration requires explicit correction; no rates, accepted agreements or financial state are rewritten.
- **Verification:** two isolated regressions exercise direct validation and configuration-file loading with free/paid input, zero/nonzero/noncanonical output, multiple models and priced-generation controls. All 485 available tests, available builds/checks and strict commercial-change validation pass. This proves configuration admission, not inventory, startup/readiness or hardware qualification; embedding-template enforcement remains separate.

### Absolute localhost aliases passed advertised-origin validation

- **Native-CFX defect:** HTTPS/443 origins such as `localhost.` and `provider.localhost.` passed static admission because localhost classification omitted terminal DNS root dots. This was a metadata-validation gap, not demonstrated DNS/transport access to a forbidden address.
- **Correction:** the shared origin validator classifies the reserved namespace after URL normalization, stripping terminal dots only for that check. Literal-loopback/exact-localhost development support remains narrow; valid public absolute DNS names and returned URL spellings are preserved. DNS pinning, address filtering and redirect/proxy refusal remain unchanged.
- **Verification:** ten parser-accepted host spellings across three scheme/port combinations and both modes are refused; public absolute names and existing development controls remain accepted. All 483 available tests, available builds/checks and strict commercial-change validation pass. The new regression is isolated syntax/admission evidence, not a DNS, reachability or public-deployment qualification.

### Funding messages accepted positional JSON arrays

- **Native-CFX defect:** derived struct decoding accepted a positional funding request and positional acknowledgement envelope, payload or network. Those response projections retained the original valid signature, so signature verification alone did not enforce the required object schemas.
- **Correction:** object-only decoding lives in the four wire types, preserving all derived missing/type/unknown/duplicate-field checks through a direct map visitor. Valid object serialization and canonical signing bytes are unchanged. No array compatibility alias, payload buffering or persisted-state conversion is introduced.
- **Verification:** the production-router request matrix retains its member cases and rejects a correctly populated array before both new admission and cached recovery. Three new native response cases refuse before chain access or local acknowledgement/start persistence, then recover the exact original from the genuine object reply after reopening. All 482 available tests and available builds/checks pass. Prepared originals and prior acceptance are disclosed fixture prerequisites, not live-chain or process-crash qualification.

### Funding registration could predate signed quote issuance

- **Native-CFX defect:** fresh registration rejected expiry but not a clock sample before quote issuance, permitting a signed acknowledgement that the native consumer's unchanged interval guard rejects.
- **Correction:** enforce `issued_at <= registered_at < expires_at` after acquiring the ledger write transaction. Refuse a pre-issuance sample with `invalid_quote`; do not clamp signed time, change terms or borrow a rate watermark. Previously accepted acknowledgements remain recoverable without a fresh clock or renewed lease.
- **Verification:** two ledger regressions cover exact interval endpoints, unavailable time, exact state conservation/reopening, same-original recovery and real writer contention with deterministic time advancement. A separate writer probe requires the registration transaction to own the write lock when sampling. All 481 available tests and available builds/checks pass. Prior account credit is a disclosed post-verification prerequisite; these are not live-chain, elapsed HTTP/native lost-response or power-loss results.

### Rejected quote insertions did not bound signing work

- **Current native-CFX defect:** the public proposal route signed before storage admission, so its committed-issuance counter did not bound signatures for rejected insertions.
- **Correction:** commit one of 600 provider-ledger-wide signing permits per effective minute before signing, independently of quote insertion. Failures cannot refund work; competing connections and restart share non-regressing persisted evidence. Missing initial evidence exhausts its initialization minute; malformed evidence refuses fresh quoting without resetting financial state.
- **Operator consequence:** the first eligible proposal on a fresh/upgraded ledger may receive 429 until a strictly newer minute. Do not delete rate evidence to clear a refusal or retry an ambiguous paid operation as a new quote.
- **Verification:** seven ledger and three new HTTP regressions pass, with extended actual body/control and accepted-account recovery cases. They cover 600 real signatures despite insertion rollback, signer/persistence/response failures, initialization, corruption, reordered/reopened writers and independent counter conservation. Existing free/paid recovery, explicit credential management, held settlement and request reads preserve exhausted quote budgets. All 479 available tests and available builds/checks pass; no public load, deployment or live-payment qualification is claimed.

### Settled top-up recovery incorrectly required a ready engine

- **Where:** `src-tauri/src/contracts.rs`, existing-agreement top-up settlement.
- **Observed failure:** a previously sent top-up returned 503 after its model was withdrawn, although its immutable account and payment remained valid.
- **Change:** keep readiness gating for new admission, but reconcile an existing agreement's original top-up independently of model readiness.
- **Evidence:** signed-transaction router regression covers pending/accepted/replay and continued rejection of new admission. The rebuilt daemon, still reporting readiness 503, credited the actual original payment once; repeated resume retained funded `20000000000000000` wei, zero reservation, no pending operation and the rotated default credential. Report: `offline-topup-recovery-report.json` in the local qualification directory.

### Policy rollback protection was lost between processes

- **Where:** `src-tauri/src/version.rs`, authenticated policy cache.
- **Observed failure:** one process accepted newer restrictive policy B, while a fresh process could accept older policy A.
- **Change:** persist the authenticated authority/network-scoped rollback watermark in protected storage; reject rollback, corruption and insecure permissions.
- **Evidence:** two rebuilt CLI processes accepted B then rejected A, retaining B on disk. Production permission checks remain enabled; test directories were corrected to satisfy them.

### Scoped file locks could outlive their owning guard

- **Where:** `src-tauri/src/file_lock.rs` and provider, consumer, wallet-journal and policy-cache guards.
- **Observed behavior:** closing the owning descriptor does not release an OS lock while a duplicated or inherited descriptor remains open.
- **Change/evidence:** explicitly unlock on guard drop. A real OS regression retains a duplicate, verifies exclusivity, then verifies a new owner can acquire after the original guard drops; closing the old duplicate does not release the new owner's lock. The full Rust run passed.

### Credential recovery and inference admission boundaries

- **Where:** consumer default-credential selection and `inference_gateway.rs`.
- **Change:** explicit issue can repair a missing usable default without rotating a valid one. Authenticate and acquire bounded body admission before buffering; enforce a five-second read deadline.
- **Evidence:** real explicit recovery, cap concurrency, revoke, expiry and rotation passed. An unauthorized incomplete body received 401 in about 2 ms; an authorized incomplete body with a live engine received 408 after about 5 seconds. Reports: `credential-boundary-report.json` and `security-recovery-report.json`. These checks do not establish public DoS resistance.

### Malformed embedding payloads needed semantic validation

- **Where:** `src-tauri/src/inference_backend.rs`.
- **Change/evidence:** decode and validate base64 f32 vectors, finite components, dimensions, encoding and batch consistency before accepting usage/results. A malformed-response regression failed before the fix and passed afterward. Actual BGE float/base64 batches passed with both stock SDKs; invalid inputs and duplicate request keys did not produce extra charges.

### Provisioning accepted noncanonical network identities

- **Where:** `scripts/provision-provider.py`.
- **Observed failure:** dotted deployment IDs and uppercase genesis hashes passed provisioning validation although the runtime rejects them.
- **Change/evidence:** align with `NetworkConfig`'s lowercase hash and ASCII letter/digit/hyphen/underscore deployment contract. Actual negative fixture processes now reject at identity validation before reading private-key files. This is not proof of a complete public provisioning run.

### Standalone RPC image omitted its required journal argument

- **Where:** `Dockerfile`, `chain-rpc` stage default command; Compose already supplied the argument.
- **Observed failure:** the built image's default startup exited with Clap's missing `--journal-dir` error before loading configuration.
- **Change/evidence:** include the private journal and inbound bearer file in the image default. Rebuilt runtime requires its protected token; with valid mounted configuration/token/state and an intentionally unavailable chain, it reaches chain validation and fails closed. Relay, RPC and CLI image help commands also executed. This does not qualify public RPC ingress.

## Retired historical entries

Folder-child free downloads, Drive-share reachability, CDN reseed timing, legacy issuer-key verdict verification, and the old Diagnostics log cap referred to removed runtime components. They are not current product workarounds or reasons to restore those features. Release builds require a nonzero compile-time public policy key and signed policies for non-development providers/relays. The current runtime separately permits a valid nonzero `CHIRAL_POLICY_PUBLIC_KEY` environment override, cached on first use; it changes the effective authority/cache scope, not the build-time requirement. Protect the service environment and follow the book's [version-policy contract](chiral-book.md#version-policy-and-upgrades). Debug development configuration or an arbitrary replacement authority is not release qualification.

When adding an issue, record the affected current component, observed symptom, evidence, constraint and safe workaround. Separate a reproducible defect from an unexercised gate. Remove or explicitly resolve fixed items rather than leaving stale operator instructions.
