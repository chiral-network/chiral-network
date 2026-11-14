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
|Payload binding and rejection without consumption|`auth::authenticate_owner`, `auth::challenge_payload`|`signature_binds_every_control_field_and_raw_body` compares framed bytes; `intent_owner_challenge_binds_body_payer_network_provider_and_consumes_once` exercises real signatures, altered bindings, later success, and replay. These are distinct levels of evidence.|
|Atomic durable consumption|`BillingStore::challenge`, `BillingStore::consume_challenge`|`challenge_consumption_is_atomic_across_connections_and_restart` reopens a ledger then races two consumers. `issued_challenge_survives_restart_but_public_fields_do_not_authorize_opening` obtains a public response, drops the router/provider, reopens the ledger and authenticates the reconstructed real-key proof. Replay after a second restart remains an acceptance task.|
|Pending capacity and expiry|`BillingStore::create_challenge` and `cleanup_pending`|`wallet_hex_aliases_share_the_same_pending_challenge_allowance` proves both wallet spellings share eight slots without excluding another wallet. Existing `one_use_challenges_are_expiring_and_pending_state_is_bounded` covers expiry cleanup and expired consumption; global/transactional scenarios remain open.|
|Quote/account authority and read scope|`contracts::authorize_read`, `authorize_owner`, opening/funding-intent/top-up/credential branches|The existing public-free-quote case and the new public challenge nonce/owner-without-signature case refuse opening without account creation. Successful authentication alone also leaves the account unopened. The complete read/mutation matrix remains an acceptance task.|

These sources were read; correspondence is not a claim that the unchecked acceptance tasks have passed. Reuse existing tests before adding deterministic consumer-visible regressions. Do not test only wiring or implementation text.

### Task 2.1 acceptance scope

The five tests in `src-tauri/tests/inference_owner_challenge_test.rs` call the production Axum HTTP router with isolated disk-backed `PreparedProvider` fixtures and reserved loopback addresses, not live providers or simulated engine/RPC responses. Payload expectations come from the intended request and provider context; existing independent wire vectors cover the reused framing function. Exact opaque queries, raw-body digests and UTF-8 limits are checked without treating challenge issuance as route validation.

The restart case verifies a persisted response and a valid signature against the reopened ledger at its explicit pre-expiry time. It does not pretend to advance the real server clock, complete account opening, or prove the remaining replay/concurrency matrix. The issuer's 120-second constant and overflow-safe assignment remain source evidence; exact expiry and native receiver boundary acceptance belong to tasks 2.3 and 2.2 respectively. No sleeps or new clock seam were introduced.

### Task 2.2.1 native transport acceptance scope

`src-tauri/tests/inference_owner_consumer_test.rs` uses the public `Consumer` API, protected temporary wallet files, real loopback HTTP transport, and the production control router. Its middleware first obtains a genuine persisted challenge, then changes only its response. Negative cases require the intended challenge request to reach that counterparty and reject any subsequent control request, so a signature leaked to a provider that later rejects it cannot pass merely because account opening failed. Both native accepted-account state and a separate provider-ledger connection must show no account.

The 26 negative variants cover each network component, both wallets, method, path, added query, raw-body digest, payload/response nonce and expiry disagreement, signing domain, additional signed fields, malformed hex/framing/required response fields, empty/257-byte nonces, and expiry zero/`u64::MAX`. Nonce/expiry guard cases supply matching reconstructed bytes so another mismatch cannot mask those guards.

Successful native free opening, account reading and credential revocation are exercised with ordinary replies and with uppercase payload hex plus misleading unsigned extra fields. The tests recover the real wallet signer from locally reconstructed bytes, check transmitted method/target and body digest, require empty GET/DELETE bodies, and verify actual account creation and credential revocation. They do not pin incidental JSON key order or treat unsigned additions as authority.

These observations prove transmission boundaries, not instrumented absence of an internal signing call. Source ordering in `owner_request` places validation before `sign_message`. No runtime/source hook was added. The current native methods do not create query-bearing targets; substitution of a query is rejected, while legitimate provider query authorization remains a separate task. Task 2.2.2 extends nonce boundary coverage below; exact now-relative freshness boundaries remain open rather than being replaced by timeless rejection cases.

### Task 2.2.2 nonce byte-boundary acceptance scope

`native_nonce_byte_boundaries_preserve_proofs_without_granting_provider_authority` checks native acceptance at one, 255 and 256 ASCII bytes, 255/256 bytes containing two-byte UTF-8 characters, and 256 bytes containing three-byte characters. Each case verifies the transmitted nonce bytes, intended opening body/target and recovered wallet signature against the selected agreement. The test records the real provider's HTTP 401 and checks that neither side accepts an account: these substituted nonces are not stored issued challenges, and native plausibility does not relax provider header or ledger authorization.

`native_nonce_limit_counts_utf8_bytes_rather_than_characters` supplies two different 257-byte UTF-8 nonces with fewer than 256 characters and requires rejection before any control request. Existing empty and 257-byte ASCII rejection cases remain intact. The 256-byte UTF-8 cases also prove that lack of a request for the adjacent oversized values is not merely an inability to transport non-ASCII headers. Observations retain raw header bytes rather than using the ASCII-only `HeaderValue::to_str` conversion.

No private consumer hook, clock seam, issuer format change, or provider bypass is introduced. The issuer still generates 32-byte hexadecimal identifiers, and provider authentication still requires its exact stored payload. Exact now-relative expiry remains blocked under task 2.2.3.


## Risks and rollout

No runtime or schema migration is part of this specification step. The source tree contains extensive owner work, including untracked implementation files. A test-only task can proceed in an owned tracked/new test file; a fix requiring edits to an owner-untracked source file must remain blocked until ownership is resolved rather than committing that file wholesale. No acceptance task may require a live provider, real wallet, remote account, or deployment; use isolated local ledgers and fixture keys.

The following financial slice should specify exact signed opening preparation, owner-authorized acknowledgement, persistence before broadcast, and recovery using the same transaction. This change does not weaken those prerequisites or reinterpret proof consumption as payment finality.
