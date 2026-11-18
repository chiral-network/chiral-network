## Why

The read-only whitepaper (`docs/chiral-book.md`) defines the current runtime's reputation in *Payment-gated subjective reputation* (lines 361–446). Line 365 makes it "payer-signed subjective feedback backed by the quote's opening payment". Lines 367–397 fix the two catalog routes, the exact `FeedbackSubmission`, the admission checks, the rule that "Free sessions cannot vote", the exact payment verification at the eSpace `safe` frontier and the native `feedback` operation. Lines 413–415 key one vote per agreement and let a revision replace it only when both its revision and `issued_at` strictly increase. Lines 421–433 state the Elo calculation exactly. Lines 439–443 require revalidation before every snapshot, name the error codes (`invalid_feedback`, `expired_feedback`, `feedback_conflict`, `feedback_busy`, `payment_pending`, `payment_rejected`, `chain_unavailable`, `chain_timeout`) and forbid fabricating a score or blaming a provider for a catalog outage. Line 33 calls it experimental and weak against Sybils. `CLAUDE.md` adds that the relay serves reputation but not prompts or inference, and that "revisions replace one agreement's vote instead of multiplying its weight".

No specification states these rules (`docs/apas-worker/comparison-2026-11-18.md` item 10). `inference-offer-discovery` (pending) specifies only which catalog a consumer reads reputation from. The target-design `provider-launch-reputation` keeps the admission rule and the Elo calculation unchanged, adds `votes_14d`, and says that payment-gated reputation "has no OpenSpec capability yet". A later change could therefore weaken admission, replacement or revalidation without a failing requirement. Comparison item 6 also found that malformed feedback is answered with axum's plain-text 422, 400 or 415 instead of `invalid_feedback`.

## What Changes

- Add the `payment-gated-reputation` capability with 12 requirements and 60 scenarios. Each requirement cites its whitepaper lines. Numeric values that the book does not state are recorded in `design.md` as current implementation values.
- Specify the relay catalog's two reputation routes, that the relay carries no prompts or inference, and that a score is experimental, catalog-specific and gates nothing.
- Specify the exact submission shape, and that every malformed body is HTTP 400 `invalid_feedback` without a verification slot, while a body over 64 KiB stays HTTP 413.
- Specify admission: both signatures and their domains, the complete network identity, the parties, no self-feedback, canonical encodings, the 30-second and 300-second rules and the quote terms.
- Specify that free sessions cannot vote, at the catalog and in the native client.
- Specify exact opening-payment verification under the funding settlement policy, with pending, rejected, unavailable and deadline outcomes, where an RPC identity mismatch or a contradictory receipt is unavailable rather than a rejected payment.
- Specify one vote per agreement and per payment, strict revision replacement, idempotent identical evidence, and preserved first acceptance and amount.
- Specify the network-bound, durable, bounded store, and that a store fault is not reported as a conflict.
- Specify the Elo calculation exactly, with numeric scenarios.
- Specify revalidation before every snapshot, retained evidence, a missing receipt as pending rather than a funding reversal, and 503 instead of a score from unavailable evidence.
- Specify the four shared verification slots and the background pass.
- Specify the native `feedback` operation and the native inspection's reputation result, including reporting catalog codes as catalog answers.
- Record a coverage table, four code differences, observations, two open questions and the risks in `design.md`. Code differences and missing tests are unchecked tasks; this step changes no code and no test.

## Capabilities

### New Capabilities

- `payment-gated-reputation`: The current runtime's payer-signed subjective feedback: the relay catalog's routes and boundary, the exact submission and its strict decoding, admission and the free-session rule, exact opening-payment verification at the eSpace `safe` frontier, one vote per agreement with strict revision replacement, the network-bound bounded store, the exact Elo calculation, revalidation before every snapshot, the four shared verification slots and background pass, and the native feedback and inspection paths.

### Modified Capabilities

None. The spec references the normative `inference-wire-foundation` and the pending `inference-funding-settlement`, `inference-commercial-terms`, `inference-offer-discovery` and `desktop-gui-pages` capabilities by requirement name, and names the target-design change `specify-provider-launch-reputation` where its requirements extend this one. It adds no obligation to any of them.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/rating_api.rs`: `RatingState`, `verify`, `refresh`, `router`, `submit`, `snapshot` and `reconcile`;
  - `src-tauri/src/rating_storage.rs`: `RatingStore::open`, `upsert`, `events`, `set_eligible` and `providers`;
  - `src-tauri/src/reputation.rs`: `Feedback`, `FeedbackSubmission`, `validate_submission` and `compute_reputation`;
  - `src-tauri/src/bin/relay_server.rs`: the store, the router and the reconciliation task;
  - `src-tauri/src/chain_client.rs`: `verify_payment` and its identity and receipt checks;
  - `src-tauri/src/consumer.rs`: `feedback`, the reputation part of `provider_info`, `response_bytes` and `safe_error`;
  - `desktop.rs`, `bin/chiral.rs`, `src/pages/Usage.svelte` and `src/pages/Marketplace.svelte`.
- Four code differences were found. Tasks 2.1–2.4 track them:
  - malformed submissions get axum's plain-text 415, 400 or 422 instead of `invalid_feedback` (comparison item 6), and a valid submission followed by trailing content is accepted;
  - store faults are answered `feedback_conflict` with database text;
  - the native client reports every catalog answer, including a catalog outage, as "Provider operation failed";
  - an RPC identity mismatch or a contradictory receipt is a rejected payment at submission and makes votes ineligible at revalidation, so a snapshot is served instead of 503.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.12. The main gaps are router-level tests over HTTP, rating-handler tests with an RPC fixture, numeric Elo vectors, background reconciliation and the native feedback and inspection paths.
- Live evidence across independently operated relays, providers and consumers is task 4.1. It needs an authorized private deployment and is never satisfied by fixtures.
- Two questions for the whitepaper author are recorded in `design.md` and `docs/apas-worker/report.md`: whether a provider can pre-empt its consumer's vote by reusing the quote ID, and whether every snapshot must revalidate every vote within 30 seconds.
- No code, test, wire format, store schema, deployment or whitepaper change is part of this specification step. It grants no live-chain, public-testnet or release qualification.
