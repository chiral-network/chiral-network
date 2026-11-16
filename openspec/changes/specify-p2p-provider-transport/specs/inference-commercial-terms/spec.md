## MODIFIED Requirements

### Requirement: Complete signed offer declaration

An offer SHALL use the signed envelope, complete network identity, canonical wallet/money representations and the offer domain of its schema version defined by `inference-wire-foundation`. The current offer payload schema (v2) SHALL allow exactly these fields and SHALL reject unknown fields, including within its model and serving-profile objects:

| Field | Contract |
|---|---|
| `network` | Complete selected network identity |
| `provider_wallet` | Canonical provider wallet and required offer signer |
| `peer_id` | Syntactically valid libp2p PeerId; authenticated manifest retrieval additionally binds it to the responding peer, and a `/chiral/inference/http/1` serving stream binds it to the Noise-authenticated remote peer (`p2p-provider-transport`) |
| `deployment_id` | Provider deployment namespace, 1–128 ASCII letters, digits, `_` or `-`, distinct from `network.deployment_id` |
| `transports` | Array of one or two unique members of `https` and `libp2p`; member order is signed but carries no preference |
| `endpoint` | Present in every payload: a provider HTTP origin satisfying the origin requirement below when `transports` contains `https`, otherwise JSON `null` |
| `model` | Complete model declaration below |
| `price` | Canonical `input_wei_per_1k` and `output_wei_per_1k` strings |
| `min_funding_wei` | Canonical minimum opening value for paid service; in a v2 offer, exactly `"0"` when both token rates are zero |
| `free_allowance` | Present in every payload: the free allowance object below when both token rates are zero, otherwise JSON `null` |
| `platform_wallet` | Canonical provider-remitted platform-fee destination |
| `fee_bps` | Unsigned 16-bit integer, exactly `50` |
| `max_concurrency` | Unsigned 32-bit integer, 1–256; equals profile concurrency when a profile is present |
| `revision` | Positive unsigned 64-bit integer in the provider-wallet/deployment/model revision stream |
| `issued_at`, `expires_at` | Unsigned 64-bit Unix-second timestamps |

A v2 offer is free when both of its token rates are exactly zero. A free v2 offer SHALL declare `min_funding_wei` as `"0"` and `free_allowance` as an allowance object. A v2 offer with any nonzero token rate SHALL declare `free_allowance` as JSON `null`. The allowance object SHALL contain exactly two members, each an unsigned integer JSON number:

- `tokens`: the most tokens that one account opened from the offer may use, counting input and output tokens together, in the inclusive range 1,000 through 10,000,000;
- `lifetime_secs`: the number of seconds after the account's opening at which the account stops being served, in the inclusive range 3,600 through 2,592,000.

A missing, null, string, Boolean, negative, fractional, exponent-form, out-of-range, duplicate or unknown allowance member SHALL be rejected. The object SHALL be canonicalized like every other signed object under `inference-wire-foundation`, so its canonical form is `{"lifetime_secs":<integer>,"tokens":<integer>}`. The allowance applies to each account separately; it SHALL NOT be read as a limit across accounts, payers or offer revisions. Native publication SHALL emit `{"lifetime_secs":86400,"tokens":20000}` for a free offer unless the provider configured other values within these bounds. Service within the allowance is governed by *Free accounts are served only within their signed allowance*.

The legacy offer payload schema (v1) SHALL allow exactly the same fields except `transports` and `free_allowance`, SHALL require a non-null `endpoint` string, and SHALL be interpreted as declaring exactly `https`. A payload containing `transports` SHALL be decoded and verified only as v2 under the v2 offer domain; a payload without `transports` SHALL be decoded and verified only as v1 under the v1 offer domain; no payload SHALL be accepted under both schemas or both domains. v1 never shipped publicly and SHALL be retired at once, with no acceptance window: a native node implementing v2 SHALL NOT sign, publish or list a v1 offer, and SHALL use v1 payloads only for the historical verification and service of retained agreements that the freshness requirement below keeps. The revision stream per provider wallet, deployment and model SHALL hold only v2 offers: a higher revision supersedes, a lower revision is rejected and an equal revision with different content is equivocation. A change of price, including a change between free and priced terms and one made by the share's automatic pricing (Auto-price, milestone 4), SHALL be an ordinary new revision in this stream with no other protocol change; it SHALL govern only new discovery, quotes and accounts, and SHALL NOT alter an agreement accepted under an earlier revision. An upgraded native publisher SHALL continue its one persisted revision counter, so that each v2 revision it issues is higher than every v1 revision it issued. Native publication SHALL publish only v2 offers, declaring `https` only for a configured advertised origin and `libp2p` only when libp2p serving is enabled for the persisted identity named by `peer_id`, on the manifest protocol that `p2p-provider-transport` defines.

The receiver SHALL reject a serialized signed offer exceeding 16 KiB, a wrong network or signer, malformed fields, an invalid model/profile, zero revision, unsupported fee, invalid concurrency, an empty, duplicate or unknown transport, an `https` declaration without an endpoint, an endpoint without an `https` declaration, a free v2 offer with nonzero minimum funding or without a free allowance, a priced v2 offer with a free allowance, or a malformed or out-of-range allowance. These free-allowance and zero-minimum rules apply to v2 offers only; a retained v1 offer, which has no `free_allowance` field, SHALL be verified under its own schema without them. Validation SHALL NOT accept a malformed offer merely because its signature is valid. A valid declaration SHALL NOT by itself prove health, slot availability, reachability over a declared transport, execution of the advertised artifact, truthful metering or completed qualification.

#### Scenario: Attributable but invalid commercial metadata

- **WHEN** a provider signs an offer with an invalid fee, concurrency, model, peer identity or provider deployment identifier
- **THEN** offer acceptance rejects it despite a valid provider signature

#### Scenario: Oversized or extended declaration

- **WHEN** a signed offer exceeds 16 KiB or includes an unknown offer/model/profile field
- **THEN** it is rejected rather than storing a truncated or projected commercial declaration

#### Scenario: Separate network and provider deployment namespaces

- **WHEN** two offers share a provider deployment string but differ in the selected network tuple
- **THEN** the foreign-network offer is not accepted as the selected network's offer

#### Scenario: Transport declarations

- **WHEN** an otherwise valid v2 offer declares `["libp2p"]` with `endpoint` `null`
- **THEN** it is accepted as a libp2p-only declaration with no advertised origin
- **AND** `["https"]` with a `null` endpoint, `["libp2p"]` with an endpoint string, `[]`, `["libp2p","libp2p"]` or an unknown member such as `"quic"` is rejected despite a valid signature

#### Scenario: Schema and domain are bound together

- **WHEN** a payload containing `transports` is signed under the v1 offer domain, or a payload without `transports` is signed under the v2 offer domain
- **THEN** offer acceptance rejects it
- **AND** a valid v1 payload of a retained agreement verifies only as an `https`-only declaration of its endpoint

#### Scenario: Upgraded publisher continues one revision stream

- **GIVEN** a provider whose last v1 offer for a provider wallet, deployment and model had revision 7
- **WHEN** it publishes after upgrading to v2
- **THEN** it publishes only v2 offers for that provider wallet, deployment and model, at revisions greater than 7
- **AND** a receiver rejects a different v2 payload at an already accepted v2 revision as equivocation

#### Scenario: Free allowance is declared exactly by free offers

- **GIVEN** an otherwise valid v2 offer whose two token rates and `min_funding_wei` are `"0"`
- **WHEN** it declares `free_allowance` as `{"lifetime_secs":86400,"tokens":20000}`
- **THEN** offer acceptance accepts it as a free offer with that per-account allowance
- **AND** the same offer with a `null` allowance or with `min_funding_wei` `"1"`, a v2 offer with a nonzero output rate and an allowance object, and an allowance with `tokens` 999 or 10,000,001, `lifetime_secs` 3,599 or 2,592,001, `"20000"`, `2e4` or an extra member are rejected despite a valid signature
- **AND** a retained v1 free offer, which has no allowance, still verifies under the v1 schema for the historical service of its retained agreements

#### Scenario: A price change is an ordinary revision

- **GIVEN** a provider whose free v2 offer at revision 10 carries the allowance `{"lifetime_secs":86400,"tokens":20000}`, and an account opened from it
- **WHEN** the provider's automatic pricing publishes revision 11 with nonzero rates and a `null` allowance, and later revision 12 that is free again with a different allowance
- **THEN** receivers accept each as an ordinary higher revision that supersedes the previous one for new discovery, quotes and accounts
- **AND** the account opened from revision 10 keeps its free terms and its original allowance, and an account opened from revision 11 keeps revision 11's prices

### Requirement: Offer freshness and advertised origin

Current offer acceptance SHALL require `expires_at > issued_at`, `expires_at > now`, issuance no more than 30 seconds in the future, and lifetime no greater than 3,600 seconds. Native publication SHALL normally issue 120-second offers; that shorter issuer lifetime SHALL NOT replace the receiver's 3,600-second bound. Offer expiry SHALL prevent fresh selection/proposal without erasing a previously acknowledged agreement.

Schema v1 SHALL be retired at once, with no acceptance window, cutoff, configuration, environment value or received object that re-enables it. A node implementing v2 SHALL NOT accept a v1 offer for any fresh use: discovery, registry insertion, catalog listing, fresh selection, quote proposal or quote acceptance, or opening a new account from it by preparing a new opening transaction, registering a funding intent or creating a free first opening. It SHALL reject such a use with a reason that identifies the retired schema. A provider SHALL treat a quote it issued against a v1 offer, for which it holds no durable funding-intent acknowledgement and no account, as new funding of a v1 offer and SHALL refuse its funding-intent registration or free opening with that reason, without acknowledgement, accepted intent, account, credit or submission; as for any refused registration, the refused original SHALL NOT be broadcast.

Retirement SHALL govern only new discovery, quotes and accounts; top-ups, requests and recovery of an existing account SHALL continue under its saved terms. v1 offers, and the quotes, funding-intent acknowledgements and receipts that reference them, persisted in a saved consumer account or a provider ledger SHALL remain parseable and verifiable under their original domains for the life of the account, without a time limit: network, signature, party and term checks for historical verification and recovery SHALL continue under the v1 offer domain. Recovery of an acknowledged opening with its original evidence, and the account's inference, request status, receipts, cancellation, top-ups and settlement, SHALL continue under their existing contracts, which do not depend on current offer publication, and SHALL NOT be refused because the agreement references a v1 offer.

When an offer declares `https`, its advertised remote origin SHALL use HTTPS on port 443, with no credentials, query, fragment or non-root path. Forbidden literal addresses, loopback/localhost names and non-public remote origins SHALL be rejected. Explicit development mode SHALL permit HTTP(S) and non-443 ports only for literal loopback or exact `localhost`, not arbitrary private LAN or metadata endpoints. Consumers SHALL not append an SDK path before validating the origin. An offer that declares only `libp2p` has no advertised origin; its reachability SHALL be governed by `p2p-provider-transport` and SHALL NOT be represented as an HTTPS origin. Static declaration acceptance SHALL NOT be represented as successful DNS/reachability, health or qualification verification; outbound transport safeguards remain additionally required.

#### Scenario: Inclusive issuance and lifetime bounds

- **WHEN** an otherwise valid offer is issued at `now + 30` with a lifetime of exactly 3,600 seconds
- **THEN** it satisfies these time bounds
- **AND** issuance at `now + 31`, lifetime of 3,601 seconds, or expiry at `now` is rejected

#### Scenario: Provider path or private-network origin

- **WHEN** an offer that declares `https` advertises a path-prefixed origin, remote cleartext HTTP, remote port 8443, a private address or a metadata endpoint
- **THEN** origin validation rejects it
- **AND** enabling development does not authorize the private LAN/metadata/remote cases

#### Scenario: A v1 offer is refused for discovery, quote and new funding

- **GIVEN** a node implementing v2
- **WHEN** an otherwise valid, unexpired v1 offer arrives in a manifest page or catalog entry, is offered for selection, or is named in a quote proposal
- **THEN** it is not inserted, listed, selected or quoted, and no new account is opened from it
- **AND** the rejection identifies the retired schema, and no configuration, environment value or received object makes it acceptable

#### Scenario: Unacknowledged pre-upgrade v1 quote

- **GIVEN** a quote that the provider issued against a v1 offer before upgrading, with no durable funding-intent acknowledgement and no account
- **WHEN** the payer registers a funding intent or requests a free first opening for it after the upgrade
- **THEN** the provider refuses it as the retired schema, with no acknowledgement, accepted intent, account, credit or submission
- **AND** the consumer does not broadcast the prepared opening transaction

#### Scenario: A saved v1 account still verifies for recovery

- **GIVEN** a saved account whose retained offer and quote are v1 and whose opening was acknowledged before the consumer and provider upgraded
- **WHEN** the consumer recovers the opening, reads the account, sends requests or tops it up after the upgrade
- **THEN** the retained offer, quote, acknowledgement and receipts verify under their original domains with no time limit
- **AND** the provider serves and credits the account under its retained v1 terms, without a new quote or funding intent

#### Scenario: libp2p-only offer has no origin

- **WHEN** a valid v2 offer declares only `libp2p`
- **THEN** no HTTPS origin is validated, displayed or used for it
- **AND** its reachability is established only by the peer-to-peer provider transport

### Requirement: Complete signed quote and immutable copied terms

A quote SHALL use the quote domain of its schema version defined by `inference-wire-foundation`. The current quote payload schema (v2) SHALL allow exactly these required fields, rejecting unknown fields:

| Fields | Contract |
|---|---|
| `id` | Quote/account identifier under the wire foundation |
| `network`, `offer_id`, `provider_wallet` | Selected full network, exact offer content ID and provider signer |
| `consumer_wallet` | Canonical intended payer |
| `model`, `price`, `platform_wallet`, `fee_bps`, `max_concurrency` | Exact selected model/profile, rates, fee destination/rate and concurrency; no implicit substitution |
| `free_allowance` | Exact copy of the selected offer's `free_allowance`: its allowance object for a free offer, otherwise JSON `null` |
| `funding_wei` | Canonical requested opening amount |
| `nonce` | Provider-supplied signed string; native issuance uses a fresh cryptographically random 32-byte `0x`-prefixed hexadecimal nonce |
| `issued_at`, `expires_at` | Unsigned 64-bit Unix-second timestamps; native issuance expires 300 seconds after issuance |

The legacy quote payload schema (v1) SHALL allow exactly the same fields except `free_allowance`. A payload containing `free_allowance` SHALL be decoded and verified only as v2 under the v2 quote domain, and a payload without it only as v1 under the v1 quote domain; no payload SHALL be accepted under both schemas or both domains. A v2 quote SHALL name a v2 offer in `offer_id`. v1 quotes SHALL be retired with v1 offers, with no acceptance window: a native issuer SHALL NOT sign a v1 quote, and v1 quotes SHALL be used only for the historical verification and service of retained agreements that *Offer freshness and advertised origin* keeps.

The native issuer SHALL create a fresh random quote/account identifier and nonce and sign and durably record the complete quote before returning it. A quote SHALL be an overbookable commercial agreement proposal, not an opening-capacity reservation, admission acknowledgement, guarantee of later admission, on-chain smart contract, spendable credit or proof of payment. Every copied field, including the complete optional serving profile and the free allowance, SHALL remain covered by the quote signature and opening commitment. An accepted agreement SHALL bind its quote's free allowance for the life of the account.

#### Scenario: Quote copies the selected commercial proposal

- **WHEN** a valid request selects an available local offer and an allowed funding amount
- **THEN** the provider returns a signed v2 quote with the exact selected terms, including the offer's free allowance or `null`, named payer, requested amount, fresh identifier/nonce and native 300-second validity window
- **AND** the quote is recorded before the successful response

#### Scenario: Unknown or substituted quote term

- **WHEN** a quote contains an unknown field or changes a copied commercial term despite a valid provider signature
- **THEN** the native consumer does not accept it as the selected agreement

#### Scenario: Free allowance is copied, not substituted

- **GIVEN** a free v2 offer whose allowance is `{"lifetime_secs":86400,"tokens":20000}`
- **WHEN** the provider issues a quote for it
- **THEN** the quote carries exactly that allowance, zero rates and zero funding
- **AND** a quote for a priced v2 offer carries a `null` allowance, and a quote whose allowance differs from its offer's, or that carries an allowance for a priced offer, is not accepted as the selected agreement

#### Scenario: Quote schema and domain are bound together

- **GIVEN** a consumer with a saved account whose retained quote is v1
- **WHEN** a payload containing `free_allowance` is signed under the v1 quote domain, or a payload without it is signed under the v2 quote domain
- **THEN** quote acceptance rejects it
- **AND** the retained v1 quote still verifies under the v1 quote domain for recovery of that account, and no native issuer signs a new v1 quote

### Requirement: Native quote verification binds the chosen provider and payer

Before new onboarding, the native consumer SHALL validate the selected offer, which SHALL be a v2 offer, and verify both offer and quote signatures against that offer's canonical provider wallet; the quote SHALL be a v2 quote. It SHALL require the selected network to match the offer and quote, the quote's offer ID to equal the exact selected offer content ID, the quote payer to equal the active wallet, and exact equality of model/profile, rates, free allowance, fee wallet/rate and concurrency. It SHALL validate the quote/account identifier and the transport that returned the quote before a payment can be prepared/broadcast: for `https`, the advertised origin, rejecting unsafe outbound destinations; for `libp2p`, a Noise-authenticated remote peer ID equal to the offer's `peer_id`.

Quote acceptance SHALL reject issuance more than 30 seconds in the future or expiry not later than issuance. Fresh onboarding SHALL additionally require both the offer and quote to remain unexpired. The native issuer's 300-second policy SHALL not be confused with these receiver checks or used to destroy accepted historical terms. Historical verification SHALL retain network/signature/party/term checks while allowing expiry to be evaluated by the separate durable funding/recovery state machine.

A locally saved agreement SHALL NOT be overwritten by a different signed quote under the same account identifier. Offer refresh, endpoint or transport-path observations and current model availability SHALL NOT renegotiate an accepted agreement or transfer a provider-specific balance.

#### Scenario: Re-signed quote for another payer or terms

- **WHEN** the provider validly signs a quote for another payer/network/offer, different prices, a different or missing free allowance, another fee destination, or changed model/profile/concurrency
- **THEN** the native consumer rejects it for the original selection

#### Scenario: Fresh onboarding versus historical recovery

- **WHEN** offer or quote expiry has been reached
- **THEN** fresh onboarding is refused
- **AND** expiry alone does not discard an acknowledged agreement or bypass the signature/network/party checks during recovery

#### Scenario: Reusing an account identifier with changed terms

- **WHEN** a local saved agreement exists and a subsequent funding call supplies different signed quote data under that identifier
- **THEN** the call rejects the conflict and preserves the saved agreement

#### Scenario: Quote returned over libp2p

- **WHEN** a quote for a libp2p-only offer is returned over a stream whose authenticated remote peer ID equals the offer's `peer_id`
- **THEN** the native consumer may prepare payment after the remaining quote checks pass
- **AND** a quote returned over a connection authenticated as any other peer is never used to prepare payment

#### Scenario: Path change does not renegotiate

- **GIVEN** an accepted agreement first used over a Relayed path
- **WHEN** later requests travel over a Hole-punched path or over the offer's HTTPS origin
- **THEN** the agreement's terms, payee and balance are unchanged

### Requirement: Authenticated opening admission preserves original acceptance

Fresh paid admission SHALL occur at payer-authenticated funding-intent registration after exact quote/payment validation and before durable acknowledgement or client broadcast. The final capacity check and accepted-evidence insertion SHALL be atomic across competing writers. At most 10,000 original un-opened intent leases whose original quote-expiry deadlines exceed transaction time SHALL be active per ledger, independently of standalone quote retention; replay SHALL NOT extend those deadlines. Full admission SHALL return 429/`rate_limit` without acknowledgement, accepted intent, account, credit or submission. An honest provider SHALL NOT broadcast the refused original. The consumer SHALL validate and durably persist a valid acknowledgement before submitting the same prepared transaction bytes.

Once acknowledged, the exact original quote, acknowledgement, transaction and sender/nonce ownership SHALL survive later capacity pressure, quote/lease expiry and restart. Lease expiry SHALL release capacity only, never accepted evidence. Credit SHALL still require verified canonical successful execution at the healthy safe frontier. A lost reply or timeout SHALL be ambiguous, not proof of refusal: recovery SHALL use the original intent/acknowledgement/payment without automatic replacement. The provider receives broadcastable signed bytes; this residual trust boundary SHALL remain disclosed.

Free first-opening admission SHALL instead occur atomically after payer authentication, requiring fewer than eight accounts owned by that authenticated wallet and fewer than 10,000 counted ledger accounts immediately before creation. Every account SHALL count toward the per-wallet limit. Every account SHALL count toward the ledger limit except an ended free account: a free account whose agreement is a v2 quote with a signed `free_allowance`, which has no unsettled request and no allowance token hold, and whose allowance has ended, because the provider's wall clock in integer Unix seconds reads at least `opened_at + lifetime_secs` or its used allowance has reached the allowance's `tokens`. Whether an account is ended SHALL be evaluated within the same creation transaction. An ended free account SHALL keep its records, which remain available as for any other account, and SHALL be counted again only if it acquires an unsettled request, which its ended allowance never admits. A free account whose retained agreement is a v1 quote has no allowance, never ends, and SHALL keep counting toward both limits. It SHALL NOT fabricate a payment or paid intent. Cached accepted-original, existing-account, top-up and settlement recovery SHALL NOT acquire fresh quote storage/work/issuance or opening-capacity gates, rotate an existing exported credential, or rewrite accepted terms.

An ended free account with no unsettled request or token hold SHALL be deleted from the provider ledger 30 days after it ended — that is, 30 days after its allowance lifetime passed or its allowance was fully used, whichever came first — and SHALL NOT be deleted earlier. Deletion SHALL remove its record, status and receipts; later status or receipt requests for it SHALL be answered as for an unknown account. Deletion SHALL NOT change any other account, any paid account or the per-wallet count of accounts that still exist. (Decided 2026-11-16 by the author in chat; book *Wire change*.)

#### Scenario: Paid writers compete for final admission

- **GIVEN** one paid lease remains and two authenticated original payments otherwise qualify
- **WHEN** their registrations compete
- **THEN** at most one durable acknowledgement and lease are created
- **AND** the refused original creates no accepted intent, account, credit or broadcast

#### Scenario: Lost acknowledgement survives later pressure

- **GIVEN** an original acknowledgement committed but its reply was lost
- **WHEN** restart, quote/lease expiry and later capacity exhaustion precede authenticated recovery
- **THEN** the exact original acknowledgement and transaction remain recoverable without a new quote or lease
- **AND** the valid original remains eligible for safe-frontier credit without replacement payment

#### Scenario: Free opening capacity is authenticated

- **GIVEN** two authorized first free openings compete for the final applicable account slot
- **WHEN** their creation transactions execute
- **THEN** at most one account is created and the other is refused without partial account creation or quote removal
- **AND** neither path fabricates payment, while existing-account recovery remains usable without fresh capacity admission

#### Scenario: Ended free accounts leave the ledger limit but not the wallet limit

- **GIVEN** a ledger holding 10,000 accounts, of which 300 are free v2 accounts whose allowance lifetime has passed or whose allowance is fully used and which have no unsettled request, and a payer wallet that owns seven accounts, two of them ended free accounts
- **WHEN** that payer's authenticated first free opening executes its creation transaction
- **THEN** the ledger counts 9,700 accounts and the wallet seven, so the opening is admitted and the ended accounts' records, status and receipts remain available
- **AND** a later free opening by the same wallet, now owning eight accounts, is refused even though some of them are ended, and a free account with a request still unsettled after its lifetime passed keeps counting toward the ledger limit until that request settles

#### Scenario: Ended free accounts are deleted after 30 days

- **GIVEN** a free v2 account whose allowance lifetime passed 29 days ago, with no unsettled request, and another whose allowance was fully used 31 days ago, with no unsettled request
- **WHEN** the provider ledger applies retention
- **THEN** the first account's record, status and receipts remain available, and the second account is deleted, so a status request for it is answered as for an unknown account
- **AND** no paid account and no account with an unsettled request is deleted

## ADDED Requirements

### Requirement: Free accounts are served only within their signed allowance

A free account is an account whose agreement is a v2 quote with both token rates zero. The provider SHALL serve a free account only within that quote's `free_allowance`, which SHALL be part of the agreement's immutable terms.

The provider SHALL record the account's opening time, `opened_at`, in integer Unix seconds from its own wall clock, durably and in the same transaction that creates the free account. Replay or recovery of the opening, a restart and later offer revisions SHALL NOT change it. The allowance ends at `opened_at + lifetime_secs`.

Allowance tokens SHALL count input and output tokens together. A request uses the prompt (input) tokens plus the completion (output) tokens of its validated authoritative usage, as `inference-request-accounting` defines that usage; an embeddings request uses its input tokens only. The used allowance SHALL be the account's settled known usage, its accounting `prompt_tokens` plus `completion_tokens`, including usage whose charge is waived. The reserved allowance SHALL be the sum of the token holds of the account's unsettled requests.

Before dispatching inference on a free account, after authentication, the provider SHALL perform these checks and updates within the reservation transaction of `inference-request-accounting`, together with that transaction's credential, concurrency and request-identity checks:

- its wall clock, in integer Unix seconds, SHALL read less than the allowance end;
- the used allowance plus the reserved allowance plus `P + G` SHALL be at most the allowance's `tokens`, where `P` is the request's exact prepared input count and `G` its authorized maximum output count (zero for embeddings), so that the worst case is reserved before dispatch as in paid accounting;
- it SHALL add `P + G` to the reserved allowance as that request's token hold.

Competing requests SHALL NOT together reserve more than the allowance. A refusal SHALL leave no request record or hold and SHALL NOT dispatch inference. It SHALL be HTTP 402 with error code `free_allowance_expired` when the allowance end has passed, and otherwise HTTP 402 with error code `free_allowance_exhausted` when the remaining allowance cannot cover `P + G`. Each code SHALL carry a fixed message stating that the account's free allowance has ended, or cannot cover this request's maximum usage, and that further service needs a new account.

Terminal accounting of a request on a free account SHALL, in one transaction, add its validated usage to the account's usage counters and release its whole token hold. Validated usage has exactly `P` input tokens and at most `G` output tokens, so the used allowance SHALL never exceed `tokens`; usage that fails that validation SHALL follow the invalid-usage rule of `inference-request-accounting`. An unknown-usage outcome SHALL release the token hold without adding to the used allowance, at the provider's expense, and SHALL NOT invent a token count. A request admitted before the allowance end SHALL complete and settle normally after it: the end governs admission only.

An ended or used allowance SHALL NOT erase the account or its records: authorized account reads, request status, receipts and cancellation SHALL remain available as for any other account, and no credential SHALL be rotated because of it. The allowance SHALL NOT be extended or renewed. Free accounts remain ineligible for top-up, and no offer revision, including a change made by automatic pricing, SHALL change an opened account's allowance. Further free service SHALL require a new account under the then-current offer, subject to ordinary free-opening admission, and paid service SHALL require a new paid account. A free account whose retained agreement is a v1 quote has no signed allowance and SHALL continue under its saved terms.

Wherever the native desktop shows a consumer a free v2 offer, including the Marketplace offer card (book A.4, "a free offer's allowance"), it SHALL show that offer's signed allowance: its token count, stated as input and output tokens together, and its lifetime, stated as running from the account's opening. Before a free opening, the native consumer SHALL show the allowance from the verified quote: its token count, stated as input and output tokens together; its lifetime, stated as running from the account's opening; and that the account then stops serving and further use needs a new account. It SHALL record, with the saved agreement, its own clock reading when it first sent the opening request. After opening, it SHALL show the remaining tokens, computed as the allowance's `tokens` less the account's usage counters, and an end time estimated as that recorded reading plus `lifetime_secs` and labeled as an estimate. On `free_allowance_exhausted` or `free_allowance_expired` it SHALL report which applies, SHALL NOT retry the request automatically, and SHALL NOT fund a paid account or open another free account without the user's explicit action.

A provider in a qualification pause (`desktop-share-online`, milestone 4) answers new inference requests and new openings with HTTP 503 and error code `qualification_in_progress`. The native consumer, including desktop Chat, the Playground and the CLI, SHALL classify that response, on either transport, as retryable and not admitted: it SHALL treat the request or opening as never dispatched and never charged, leave the account's balance, holds and allowance display unchanged, and SHALL NOT reconcile it as an ambiguous outcome. It SHALL NOT retry the request or opening automatically, as a paid request or otherwise, and SHALL NOT broadcast a prepared opening transaction for which it holds no durably persisted acknowledgement. Desktop surfaces SHALL show "Provider is qualifying — try again later", and the CLI SHALL report the error code in its machine-readable output. A valid `Retry-After` value MAY be shown as a hint; it SHALL NOT start an automatic retry. A later request or opening SHALL be sent only on the user's explicit action.

#### Scenario: Worst-case tokens are reserved against the allowance

- **GIVEN** a free account whose allowance is 20,000 tokens, which has used 19,000 tokens and has no unsettled request
- **WHEN** a chat request prepares 600 input tokens with an authorized maximum of 500 output tokens
- **THEN** the provider refuses it with HTTP 402 and `free_allowance_exhausted`, with no request record or hold and no dispatch
- **AND** the same request with a maximum of 400 output tokens is admitted with a 1,000-token hold

#### Scenario: Competing requests cannot exceed the allowance

- **GIVEN** a free account with 1,000 tokens of allowance remaining and no unsettled request
- **WHEN** two requests whose `P + G` is 600 tokens each compete for reservation
- **THEN** at most one is admitted
- **AND** the other is refused with `free_allowance_exhausted` and leaves no request record or hold

#### Scenario: Settlement counts input and output and releases the hold

- **GIVEN** a request admitted on a free account with `P` 600 and `G` 400, after 19,000 tokens were used
- **WHEN** its validated usage is 600 input and 150 output tokens
- **THEN** the used allowance becomes 19,750 and the request's 1,000-token hold is released in the same transaction
- **AND** had its usage been unknown, the hold would be released with the used allowance still 19,000 and no token count invented

#### Scenario: The lifetime ends admission, not admitted requests

- **GIVEN** a free account opened at provider time `T` with `lifetime_secs` 86,400, and a request admitted at `T` plus 86,000 seconds that is still streaming
- **WHEN** a new request arrives at `T` plus 86,400 seconds
- **THEN** the provider refuses it with HTTP 402 and `free_allowance_expired`
- **AND** the earlier request completes and settles its known usage, and the account's status, receipts and cancellation remain available

#### Scenario: Opening replay does not restart the lifetime

- **GIVEN** a free account opened at provider time `T`
- **WHEN** its owner replays the opening at `T` plus 50,000 seconds after losing the reply
- **THEN** the provider returns the same account with `opened_at` still `T`
- **AND** its allowance still ends at `T` plus `lifetime_secs`

#### Scenario: A price change does not alter an opened allowance

- **GIVEN** a free account opened from a revision whose allowance is 20,000 tokens and 86,400 seconds
- **WHEN** the provider publishes a priced revision and later a free revision with a 50,000-token allowance
- **THEN** the account keeps its 20,000-token, 86,400-second allowance
- **AND** more service needs a new account: a free one under the current free revision, subject to free-opening admission, or a paid one

#### Scenario: The offer card shows a free offer's allowance

- **GIVEN** a verified free v2 offer whose allowance is `{"lifetime_secs":86400,"tokens":20000}`
- **WHEN** the Marketplace shows its offer card
- **THEN** the card shows 20,000 input and output tokens and 24 hours from opening as the offer's per-account allowance
- **AND** a priced offer's card shows no allowance

#### Scenario: The consumer sees the allowance before opening

- **GIVEN** a selected free v2 offer whose verified quote carries the allowance `{"lifetime_secs":86400,"tokens":20000}`
- **WHEN** the native consumer asks the user to confirm the free opening
- **THEN** it shows 20,000 input and output tokens, 24 hours from opening, and that the account then stops serving
- **AND** after opening it shows the remaining tokens and an estimated end time, and on an allowance refusal it neither retries, funds a paid account nor opens another free account without the user's action

#### Scenario: A retained v1 free account keeps its saved terms

- **GIVEN** a free account whose retained agreement is a v1 quote, opened before the provider upgraded
- **WHEN** its owner sends requests after the upgrade
- **THEN** the provider serves it under its saved terms, with no allowance check
- **AND** no allowance is inferred or added to its agreement

#### Scenario: A qualifying provider's 503 is retryable and not admitted

- **GIVEN** a free account with remaining allowance at a provider that has entered a qualification pause
- **WHEN** desktop Chat sends a request and receives HTTP 503 `qualification_in_progress` with `Retry-After: 600`
- **THEN** Chat shows "Provider is qualifying — try again later", records nothing as charged or ambiguous, and the account's remaining tokens are unchanged
- **AND** neither Chat, the Playground nor the CLI re-sends the request or any paid request automatically, and a paid opening refused the same way at funding-intent registration is never broadcast
