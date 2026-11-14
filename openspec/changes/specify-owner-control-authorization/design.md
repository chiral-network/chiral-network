## Context

The authority is `docs/chiral-book.md`; it is unchanged. Existing wire requirements already define the complete network identity, canonical JSON, framed `chiral-inference-auth-v1` bytes, personal signatures, and payer signer. Existing proposal requirements explicitly withhold account authority from a public quote. This change supplies the missing authorization contract between those foundations and financial operations.

## Goals and non-goals

Specify owner proof issuance, client verification, durable one-use consumption, pending capacity, and account control authorization. Preserve existing public proposal/challenge behavior and the separate read-bearer path.

Do not implement or claim acceptance of the full funding-intent/payment state machine, credential lifecycle, inference authentication, version policy, or endpoint/DNS policy. Those remain independent capabilities. Challenge issuance rate windows need a separate specification/acceptance slice; existing 12-per-named-wallet and 600-per-ledger minute limits are not removed. The shared `rate_limit`/`cleanup_pending` clock-regression concern already recorded by `specify-proposal-rate-budgets` also affects challenge issuance and must not be described as solved here.

## Decisions

### 1. Public name selection is not ownership

The request's `wallet` names a future signer. Normalize hexadecimal aliases before storing or counting challenges, but do not authenticate challenge issuance or public proposal. A challenge alone authorizes nothing. Pending limits constrain storage; an unauthenticated third party can still consume a named wallet's challenge capacity. They are not an availability guarantee or Sybil defense. The existing public-proposal availability report remains nonblocking and is not an instruction to add authentication here.

### 2. Bind exact bytes and target, not JSON meaning

Use the existing `AuthPayload` and `challenge_payload`, not a second signing convention. Hash the outgoing bytes once and transmit those bytes after signing. Include the raw query spelling in the request target; server authorization already uses `Uri::path_and_query`. Current native `owner_request` builds path-only URLs, so this requirement does not invent a native pagination feature. Any future query-bearing caller must bind the complete target.

The provider supplies its network and signing identity. The client reconstructs those from its selected context instead of trusting the returned hex. Response-only additive fields cannot affect the locally reconstructed payload. The strict input and signed payload schemas are distinct from the unsigned response decoder, which currently tolerates unknown members.

### 3. Separate issuer lifetime from receiver plausibility

`CHALLENGE_TTL_SECS` is 120; `BillingStore::create_challenge` independently enforces that bound at admission. The native `validate_challenge` receiver requires a nonempty nonce of at most 256 bytes and an expiry strictly in the future but no more than 600 seconds ahead of its own clock. Preserve these different roles instead of silently changing interoperable behavior: the receiver guard is not permission for a provider to issue a 600-second challenge, nor a promise of clock synchronization. Actual provider authentication still requires an unexpired stored challenge and exact payload equality. The response's `0x`-prefixed hex is decoded before comparing bytes; hex letter case alone does not change signing bytes.

### 4. Authenticate once, then apply ownership and business rules

`authenticate_owner` validates all bindings and the recovered signer before `consume_challenge`. The ledger deletion compares nonce, serialized expected payload, and expiry in one transaction. Its success is the authority linearization point, not the financial-operation commit. A later 403/409/503 can therefore consume a proof without crediting funds or changing credentials. Missing responses remain ambiguous; obtain fresh proof for recovery, never infer another payment is needed.

Reads deliberately choose the owner branch whenever `X-Chiral-Owner` is present. Do not mask an invalid owner proof by accepting a simultaneously supplied bearer. Bearers remain confined to their account and declared read scope on the listed GET routes. Opening and funding-intent ownership comes from the quote; established-account mutations use its saved consumer wallet.

### 5. Keep durable state and capacity in the existing ledger

Persist before returning a challenge, and never accept stateless signature-only owner authorization. Pending challenge quotas are independent of quotes: eight per canonical named wallet and 10,000 per ledger. The existing transaction combines expiry cleanup, rate accounting, pending-capacity checks, and insertion; a failure rolls back the transaction. Consumption releases one pending slot without making an account or funding decision. Cleanup must continue respecting archived acknowledged-funding retention requirements.

## Source and existing evidence

|Contract area|Current implementation|Existing evidence and limits|
|---|---|---|
|Strict challenge request and payload|`protocol::ChallengeRequest`, `auth::AuthPayload`, `auth::validate_challenge_request`, challenge branch of `contracts::control`|`inference_owner_challenge_test` exercises missing, mistyped, extra and duplicate fields; supported/unsupported methods; wallet/digest aliases and invalid encodings; path prefix, controls, fragment and 512-byte ASCII/UTF-8 boundaries; exact provider/network/request binding.|
|Local reconstruction and safe signing|`consumer::owner_request`, `consumer::validate_challenge`|`inference_owner_consumer_test` exercises public native calls with altered real-provider challenge replies and verifies actual transmitted signatures/bodies for POST, GET and DELETE. Dedicated cases cover nonce byte boundaries; existing helper tests retain selected freshness coverage and exact remaining time boundaries stay in task 2.2.3.|
|Payload binding and rejection without consumption|`auth::authenticate_owner`, `auth::challenge_payload`|`signature_binds_every_control_field_and_raw_body` compares framed bytes; the existing funding-intent test exercises real signatures and identity mismatches. `inference_owner_authentication_test` adds actual method/query/body and signature rejection, unknown nonces, subsequent usable authority, and exact expiry.|
|Atomic durable consumption|`BillingStore::challenge`, `BillingStore::consume_challenge`|The existing two-connection test races direct consumption; the public-issuer test recovers an unconsumed real proof after restart. `inference_owner_authentication_test` additionally races full signature authentication, checks consumed replay after reopening, and verifies complete-payload comparison and a stale-reader replacement interleaving.|
|Pending capacity and expiry|`BillingStore::create_challenge` and `cleanup_pending`|`wallet_hex_aliases_share_the_same_pending_challenge_allowance` proves both wallet spellings share eight slots without excluding another wallet. Existing `one_use_challenges_are_expiring_and_pending_state_is_bounded` covers expiry cleanup and expired consumption; global/transactional scenarios remain open.|
|Quote/account authority and read scope|`contracts::authorize_read`, `authorize_owner`, opening/funding-intent/top-up/credential branches|`inference_owner_control_test` covers all six GET routes, credential mutation authorization and the free/paid opening, acknowledgement and top-up authority matrix. Wrong-owner and business refusals consume proofs without granting balances or credentials. Existing-root preservation on an authenticated opening retry remains blocked task 2.4.3.|

These sources were read; correspondence is not a claim that the unchecked acceptance tasks have passed. Reuse existing tests before adding deterministic consumer-visible regressions. Do not test only wiring or implementation text.

### Task 2.1 acceptance scope

The five tests in `src-tauri/tests/inference_owner_challenge_test.rs` call the production Axum HTTP router with isolated disk-backed `PreparedProvider` fixtures and reserved loopback addresses, not live providers or simulated engine/RPC responses. Payload expectations come from the intended request and provider context; existing independent wire vectors cover the reused framing function. Exact opaque queries, raw-body digests and UTF-8 limits are checked without treating challenge issuance as route validation.

The restart case verifies a persisted response and a valid signature against the reopened ledger at its explicit pre-expiry time. It does not pretend to advance the real server clock, complete account opening, or prove the remaining replay/concurrency matrix. The issuer's 120-second constant and overflow-safe assignment remain source evidence; exact expiry and native receiver boundary acceptance belong to tasks 2.3 and 2.2 respectively. No sleeps or new clock seam were introduced.

### Task 2.2.1 native transport acceptance scope

`src-tauri/tests/inference_owner_consumer_test.rs` uses the public `Consumer` API, protected temporary wallet files, real loopback HTTP transport, and the production control router. Its middleware first obtains a genuine persisted challenge, then changes only its response. Negative cases require the intended challenge request to reach that counterparty and reject any subsequent control request, so a signature leaked to a provider that later rejects it cannot pass merely because account opening failed. Both native accepted-account state and a separate provider-ledger connection must show no account.

The 26 negative variants cover each network component, both wallets, method, path, added query, raw-body digest, payload/response nonce and expiry disagreement, signing domain, additional signed fields, malformed hex/framing/required response fields, empty/257-byte nonces, and expiry zero/`u64::MAX`. Nonce/expiry guard cases supply matching reconstructed bytes so another mismatch cannot mask those guards.

Successful native free opening, account reading and credential revocation are exercised with ordinary replies and with uppercase payload hex plus misleading unsigned extra fields. The tests recover the real wallet signer from locally reconstructed bytes, check transmitted method/target and body digest, require empty GET/DELETE bodies, and verify actual account creation and credential revocation. They do not pin incidental JSON key order or treat unsigned additions as authority.

These observations prove transmission boundaries, not instrumented absence of an internal signing call. Source ordering in `owner_request` places validation before `sign_message`. No runtime/source hook was added. The current native methods do not create query-bearing targets; substitution of a query is rejected, while provider query authorization is covered separately in task 2.4.1. Task 2.2.2 extends nonce boundary coverage below; exact now-relative freshness boundaries remain open rather than being replaced by timeless rejection cases.

### Task 2.2.2 nonce byte-boundary acceptance scope

`native_nonce_byte_boundaries_preserve_proofs_without_granting_provider_authority` checks native acceptance at one, 255 and 256 ASCII bytes, 255/256 bytes containing two-byte UTF-8 characters, and 256 bytes containing three-byte characters. Each case verifies the transmitted nonce bytes, intended opening body/target and recovered wallet signature against the selected agreement. The test records the real provider's HTTP 401 and checks that neither side accepts an account: these substituted nonces are not stored issued challenges, and native plausibility does not relax provider header or ledger authorization.

`native_nonce_limit_counts_utf8_bytes_rather_than_characters` supplies two different 257-byte UTF-8 nonces with fewer than 256 characters and requires rejection before any control request. Existing empty and 257-byte ASCII rejection cases remain intact. The 256-byte UTF-8 cases also prove that lack of a request for the adjacent oversized values is not merely an inability to transport non-ASCII headers. Observations retain raw header bytes rather than using the ASCII-only `HeaderValue::to_str` conversion.

No private consumer hook, clock seam, issuer format change, or provider bypass is introduced. The issuer still generates 32-byte hexadecimal identifiers, and provider authentication still requires its exact stored payload. Exact now-relative expiry remains blocked under task 2.2.3.

### Task 2.3 provider proof acceptance scope

The six tests in `src-tauri/tests/inference_owner_authentication_test.rs` exercise production authentication and disk-backed ledger APIs with explicit timestamps and canonical network/provider identities. They do not execute an account operation, simulate an RPC/engine, or claim HTTP route/business acceptance. Existing real-signature tests retain coverage of each network component, provider, claimed owner and changed path/body; the new cases fill remaining gaps rather than replacing that evidence.

Invalid method, semantically equivalent reserialized JSON, malformed/wrong-signer/wrong-domain signatures, signatures binding a different nonce/expiry, and a valid signature for an unstored nonce must fail without burning the original challenge. Each case then acquires authority with the original proof, including canonicalization of uppercase owner hex. Read proofs distinguish a missing, numerically equivalent, percent-encoded or changed cursor from the exact signed query, and an empty body from `{}`. HTTP read-route/cursor validation and account ownership are covered separately in task 2.4.1.

Separate fixtures accept a real proof one second before expiry and reject it exactly at and after expiry; no test rewinds one ledger's clock. The concurrency case closes the issuing ledger, races complete authentication through two reopened connections, requires one owner-authority winner, closes both connections and proves that a further reopen does not restore the consumed proof. This is a two-connection thread race, not a new subprocess or live-provider test.

Conditional-consumption cases alter each of the ten scalar signing fields, including all three network components, and require refusal while preserving usable original authority. A separate deterministic interleaving loads and validates an old payload, consumes it through another connection, inserts a different payload under the now-absent nonce, and requires stale consumption and the old proof to fail while the new proof succeeds. Deliberate nonce reuse here tests the storage comparison, not random-issuer collision probability or permission to replace a still-pending challenge.

### Task 2.4.1 HTTP account-read acceptance scope

The six read tests in `src-tauri/tests/inference_owner_control_test.rs` send requests through the production Axum control router. Each isolated provider opens two genuine free accounts over HTTP for the same wallet, so a foreign-account bearer cannot pass merely because its wallet matches. Read-only and inference-only application credentials are issued through the ledger as prerequisites; request history is populated by real reservation/cancelled-unknown-usage settlement calls, without engine output or fabricated paid funding. The same fixture supports HTTP credential mutation acceptance in task 2.4.2.1.

Account, receipt, funding, request-list, individual-request and credential-list routes must accept the account owner and its read-capable bearers, reject anonymous/inference-only/foreign-account access, and never downgrade an explicit incomplete, consumed or wrong-owner proof to an otherwise valid bearer. Successful reads check account-specific contents, provider-signed receipts, cancelled request identity/status and credential metadata without bearer disclosure. Wrong-owner business refusals consume their valid proofs; subsequent bearer-only access confirms that the fallback credential remains usable.

All six GET routes reject nonempty bodies with both bearer authority and a proof binding those exact bytes. Request lists reject missing, negative, duplicate, unsupported, percent-encoded and out-of-range cursor values. Accepted zero/`i64::MAX` bounds and the stored request's exact creation second exercise real filtering without sleeps; a leading-zero cursor must remain byte-identical to its signed target even when its numeric value is unchanged.

A foreign global request identifier must not escape the authorized account, even when both accounts share a client request key and wallet. The HTTP 404 after valid authentication durably consumes the nonce, as checked through an independent ledger connection, preserves the other account's request, rejects replay, and allows a fresh proof for the correct request. Exact expiry is already covered at the authentication API in task 2.3; these HTTP tests neither manipulate clocks nor add a runtime seam. Parent task 2.4 stays open for existing-root preservation on opening recovery.

### Task 2.4.2.1 HTTP credential-mutation acceptance scope

The combined control-route test target adds three cases for issuance, revocation and rotation without duplicating the account-read fixture. Every mutation rejects anonymous callers, root/read/inference-only bearers, same-wallet foreign-account bearers, incomplete owner proofs and an authenticated different wallet even when a valid bearer is also supplied. Refusals preserve account and credential state. Fresh authority from the actual owner succeeds; issued/rotated tokens authenticate for the intended account with the requested read scope, revoked predecessors cannot authenticate, and unrelated credentials remain usable. Consumed successful proofs cannot mutate again through bearer fallback.

Revocation and rotation must not follow a credential identifier out of the signed account target, even when both accounts share an owner. These 404 refusals durably consume authority without changing either account; a fresh proof for the credential's actual account succeeds. Authorized issuance/rotation with zero concurrency and revocation with a nonempty body likewise consume proofs while preserving credentials. An independent ledger connection verifies consumption and unchanged credential records, including rollback of rotation's attempted predecessor revocation. Corrected requests require fresh proofs and then produce the intended credential effect.

Existing `control_acknowledges_only_spendable_current_nonce_and_recovers_without_rpc_or_broadcast` covers successful signed acknowledgement, identical acknowledgement recovery while draining/RPC-unavailable, paid top-up settlement and duplicate-credit prevention using isolated chain evidence. Task 2.4.2.2 adds the HTTP bearer/wrong-owner matrix below rather than duplicating those settlement tests. No runtime signing API is widened and no live chain, engine or deployment is used.

### Task 2.4.2.2 HTTP payment-authorization acceptance scope

Four additional control-router tests cover first and existing free/paid opening, new/cached funding-intent acknowledgement, and free/active-paid account top-ups. Root, read-only, inference-only and same-wallet foreign-account bearers cannot submit these operations. Anonymous/incomplete-proof requests fail; a genuine different-wallet proof fails and remains durably consumed even with a valid bearer present. Account, credential and existing funding records are preserved by these refusals.

Transactions are locally signed EIP-155 fixtures and checked with the public offline envelope validator for chain, sender, recipient, amount and commitment. A higher-revision paid offer supplies new HTTP quotes while existing free agreements provide valid same-provider bearers. For active-paid-account prerequisites only, tests follow the existing ledger-unit-test boundary: register an admitted intent and open from an explicitly synthetic `VerifiedPayment` block observation after verification. These inputs are not chain/spendability qualification or observed funds; no RPC, broadcast, engine or production signer API is used.

The actual quoted owner opens a new free account. A valid paid-opening proof and matching signed transaction without an acknowledgement produce `funding_intent_required`, not implicit registration or credit; retries preserve the same bytes and need new proofs. Cached acknowledgements remain recoverable with fresh owner proofs while draining, returning the identical provider-signed acknowledgement without opening an account. New intent admission while draining refuses after consumption and creates no acknowledgement.

Top-up refusals distinguish authorization from business validity. Missing/wrong authority cannot submit even a correctly signed account-bound payment or create a funding-attempt record. With correct authority, free accounts refuse funding, and active-paid accounts reject a signed commitment for another account before RPC. Fresh-proof retries use those same bytes; only a rejected attempt is recorded, with no value/fee credited and existing balances, funding and credentials unchanged. Existing successful settlement/duplicate-credit tests supply the separate payment-verification evidence.

### Task 2.4.3 existing-root recovery mismatch

`contracts::control` sends authenticated repeated `/v1/contracts/open` calls to `BillingStore::open_account`. Its existing-account path then revokes all active root credentials and issues another root (`billing_store.rs`, opening transaction, currently lines 605–646). This contradicts the prohibition on authentication retries themselves authorizing credential rotation and the book's stable exported-credential recovery contract. The new tests prove unauthorized re-entry cannot trigger it; they deliberately do not bless rotation on an authorized retry.

Fixing that behavior and covering native/HTTP recovery must preserve accepted balances and leave explicit issuance/rotation reachable. The ledger and dependent native implementation are owner-untracked work, so this fix remains blocked rather than being absorbed into a test commit. Parent 2.4 cannot be closed or the change archived while this mismatch remains.


## Risks and rollout

No runtime or schema migration is part of this specification step. The source tree contains extensive owner work, including untracked implementation files. A test-only task can proceed in an owned tracked/new test file; a fix requiring edits to an owner-untracked source file must remain blocked until ownership is resolved rather than committing that file wholesale. No acceptance task may require a live provider, real wallet, remote account, or deployment; use isolated local ledgers and fixture keys.

The following financial slice should specify exact signed opening preparation, owner-authorized acknowledgement, persistence before broadcast, and recovery using the same transaction. This change does not weaken those prerequisites or reinterpret proof consumption as payment finality.
