## Context and scope

The read-only book defines payer-authenticated opening, genuinely free admission without a fabricated transfer, original acknowledged paid evidence and credential-preserving recovery. Existing owner-control and opening-funding-intent changes cover those authorities but not the full `POST /v1/contracts/open` request shape. The wire foundation explicitly leaves each payload's fields to its own contract; its strict envelope/primitive requirements do not define every unsigned control request.

This change specifies that missing foundation before implementation. It does not define top-up or accounting-response schemas, revise the author-approved non-reserving model, or duplicate the complete safe-credit/reorg lifecycle. Pending non-reserving deltas remain authoritative over historical wallet quotas in main specs.

## Decisions

1. **One named-field object, not positional aliases.** Use the object already emitted by native `OpenRequest` serialization, matching the other control-message convention. Require one quote identifier and preserve the optional nullable transaction field. Strict root shape is an explicit boundary to enforce, not a claim that derived decoding already enforces it. Keep existing object behavior; do not add an alternate serializer, compatibility shim or persistent-state conversion.
2. **Optional syntax is not a free-payment choice.** Omission and null both mean no evidence. A supplied string is never silently discarded or converted to absence. Retained commercial terms select free versus paid rules; free rejects any transaction string, and paid requires its acknowledged exact original even on replay. This preserves `check_open_funding` rather than weakening it to make object controls pass.
3. **Keep parsing separate from authority consumption.** The current route parses before owner authentication, then performs lookup, quoted-owner binding and payment checks. Structural failures leave supplied proofs unconsumed; later authenticated business failures consume them. Authenticate original bytes, not normalized decoded fields. Omission and explicit null therefore need different body signatures.
4. **Recovery is not new admission.** After identity/payment checks, an existing account bypasses fresh opening/readiness decisions without creating another credit or credential. First creation still obeys existing free-capacity or paid safe-evidence rules. Do not broaden this into automatic credential issuance or erase earlier accepted evidence.

## Source and acceptance map

| Boundary | Inspected implementation | Existing evidence and missing work |
|---|---|---|
| Request members and writer | `protocol::OpenRequest`; `Consumer::resume_inner` constructs and serializes it before `owner_request` | Required string identifier and nullable optional string are present; native writer emits objects. Derived decoder root-container behavior needs enforcement and real-router regression. |
| Parse/auth/business ordering | `contracts::control`, open branch | Parsing precedes `owner`, then quote lookup and consumer equality. Existing `free_and_paid_opening_require_the_quoted_owner_before_any_account_creation` covers bearer/wrong-owner refusal, genuine free opening and paid no-intent refusal. Full malformed-object/positional and optional-field ordering inventory remains open. |
| Payment absence | `BillingStore::check_open_funding` | Free rejects every `Some(raw)`; fresh free expiry is checked only when no account exists. Paid requires supplied original and `require_funding_intent` before route replay. Add only missing omitted/null/supplied-string cases and protect consumed-proof semantics. |
| Existing-account replay | `contracts::control`, `BillingStore::open_account` | `authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness` preserves financial/credential rows and an admitted hold through reopened providers with both quote budgets exhausted. It checks paid explicit-null refusal and exact-original replay; omitted fields and malformed request shapes are not established by it. |
| Exact signed bytes and safe credit | `inference-opening-funding-intent`, owner-control mutation tests and paid capacity/recovery tests | Reuse their original-evidence, no-broadcast and canonical-safe checks. A decoder probe or post-verification fixture is not live chain evidence. |

## Implementation and acceptance approach

Reuse `protocol::deserialize_object` with a local `deny_unknown_fields` derived decoder, as for proposal and challenge requests. Preserve `Option<String>` semantics and raw duplicate-key detection without buffering into `Value`. No exported field or native caller migration is needed.

Extend existing production-router tests where practical. Supply valid owner proofs bound to malformed raw bytes so an authorization failure cannot mask a decoder failure. Include complete two-element positional forms and the one-element optional-omission form. Preserve missing/mistyped/unknown/duplicate checks, including duplicate null payment members, and successful object controls with omission and explicit null.

Observe untouched quote/account/credential/funding/fee rows and no provider chain access where refusal precedes verification. Distinguish a schema refusal's unconsumed proof from a successful authentication followed by missing-payment/free-payment refusal. Use separate isolated fixtures or bounded case groups so challenge occupancy/rate limits cannot mask schema behavior. No sleeps, near-now assumptions, real payments or changed accepted timestamps.

Reuse existing exact-original paid replay and credential-conservation tests rather than creating a second funding fixture. Keep first opening, already accepted replay and malformed-response handling distinct; this request contract does not establish `OpenResult`/`AccountView` decoding or complete paid/expiry recovery.

## Rollout and evidence limits

The decoder, production-router schema acceptance and payment-presence/body-binding correspondence complete tasks 2.1–2.3. Task 2.4's expiry/readiness/capacity and recovery correspondence remains separate; passing project checks does not complete it. Positional refusal is a wire-shape correction, not evidence of an authentication bypass or signature forgery.

The source migration is already committed and necessary native edits are authorized. Preserve unrelated changes. No author-only decision is required: these request details fill the book's implementation-level gap without changing payment, ownership, refund or release policy. Never edit the whitepaper or archive this change until its implementation and checks are complete.

## Decoder observation (2026-11-15)

A temporary executable decoded raw JSON through the actual `OpenRequest` type. Objects with omitted payment, null payment and string payment decoded; the omitted form serialized back with explicit null. A complete `[quote_id, null]` array also decoded and projected into that object, while the one-element `[quote_id]` array was already rejected. Unknown members and a numeric payment were rejected. The executable was removed before project checks.

These observations established typed decoding and serialization only, not HTTP authorization, account creation or validity of the probe's transaction string. The production-router regression below extends that evidence to opening. The already-rejected short array remains a regression guard, not a newly discovered failure; object omission remains supported.

## Object-only implementation (2026-11-15)

The new production-router regression reproduced HTTP 200 and free-account creation for a correctly signed complete positional request, where the contract requires HTTP 400. `OpenRequest` now uses the existing direct map visitor with a strict derived field decoder. Serialization, nullable/omitted transaction semantics, owner signing bytes and financial guards remain unchanged; no intermediate JSON value or positional compatibility path is introduced.

The regression isolates malformed JSON, root shapes, missing/mistyped members and raw unknown/duplicate members across fresh free opening and existing paid-account replay. Every malformed body has a genuine proof bound to its exact bytes; full snapshots of all fourteen ledger tables must remain unchanged after refusal, including the unconsumed challenge. Positive controls create a usable free account with omitted payment, replay it with explicit null, and replay a paid account using its exact acknowledged original without changing retained accounts, credentials, funding or fees.

Accepted paid credit is an explicit post-verification ledger prerequisite, not an observed chain result. These cases do not complete the separate paid first-credit, payment-presence, exact-body substitution, expiry, readiness/capacity or native recovery inventory. Keep the change open until all remaining tasks and checks are complete.

## Payment-presence and exact-body acceptance (2026-11-15)

The source comparison found the payment-presence and body-binding guards already implemented: `contracts::control` authenticates the original bytes before selecting retained terms, `BillingStore::check_open_funding` rejects supplied free payments and requires paid originals, `require_funding_intent` compares both raw spelling and hash, and `auth::authenticate_owner` checks the exact body hash before consuming its challenge. This slice adds missing acceptance, not a new runtime policy or a claimed reproduced production defect.

Existing coverage remains in place: `free_and_paid_opening_require_the_quoted_owner_before_any_account_creation` covers quoted ownership and unregistered paid opening; `existing_free_and_paid_openings_cannot_be_reentered_using_bearers_or_another_owner` covers replay authority; `authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness` covers exact-original replay and retained holds/credentials. Ledger free-session/unregistered-opening tests and native paid safe-credit tests remain complementary, not replaced by fixture credit.

`opening_payment_presence_and_original_guards_precede_chain_verification` adds fresh/existing free, unregistered/acknowledged unopened paid, and existing paid cases. Empty, empty-hex and malformed strings remain supplied payments; free terms also reject a nonzero signed transfer. Paid omission/null is refused, while alternate hexadecimal spelling of the same bytes and a different valid signed nonce cannot replace a registered original. Every business refusal must consume its genuine proof, reject reuse, preserve retained financial/account rows and leave the observed RPC/engine endpoint untouched.

Positive controls retain useful free creation and exact-original paid replay without new credentials or duplicate credit. For unopened paid accounts, explicit fixture admission of the original lets the actual HTTP path reach an unavailable loopback RPC: it must record only the original verification attempt, preserve the acknowledgement/terms and withhold credit. Admission/spendability and already-accepted paid credit are disclosed prerequisites; no live chain execution, broadcast, qualification or automatic repair is claimed.

`opening_omission_and_null_require_their_own_exact_body_proofs` tests both substitution directions on free and paid accounts. The substituted body must receive 401 with the entire ledger and challenge unchanged. The same proof must still authorize its exact original bytes, producing either credential-preserving free replay or paid `funding_intent_required`, then become unusable. Task 2.4's expiry/readiness/capacity and recovery correspondence remains separate.
