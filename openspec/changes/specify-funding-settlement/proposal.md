## Why

The read-only whitepaper (`docs/chiral-book.md`) defines a complete funding settlement and recovery contract (book lines 468–472, 476–495, 510–518 and 522–540). The provider credits only after canonical successful execution at the healthy eSpace `safe` frontier. Credited funding is monitored. A proven orphan freezes and reverses credit with explicit loss, and outages never invent loss. Explicit cancellation is a fixed-fee same-nonce self-transfer that is not a refund. Every process spending one wallet shares one nonce journal. The `CLAUDE.md` financial invariants restate those rules.

Existing normative specs cover adjacent boundaries only. `inference-wire-foundation` defines money, identities and commitments. `inference-account-opening-request` and `inference-account-topup-request` define the opening and top-up requests; the top-up spec also owns top-up recovery. Pending changes define:

- opening funding intents (`specify-opening-funding-intent`);
- commercial terms (`specify-inference-commercial-terms`);
- owner control (`specify-owner-control-authorization`);
- account snapshots (`specify-inference-account-snapshot`);
- signed-envelope resource bounds (`specify-payment-envelope-resource-bounds`);
- non-reserving quotes (`specify-non-reserving-quotes`);
- request accounting (`specify-inference-request-accounting`);
- desktop sharing (`specify-desktop-share-online`).

None of them specifies the settlement policy itself, the classification of chain evidence, the bounded chain RPC transport, funding monitoring, orphan reversal and restoration, outage behavior, the extended balance identity, explicit cancellation, recovery targeting, the separation of resume and cancellation, displaced-payment reconciliation, or journal coordination for every spender. Those rules exist only as prose and code, so a later change could weaken them without a failing requirement.

## What Changes

- Add the `inference-funding-settlement` capability with 19 requirements and 65 scenarios. Each requirement cites its whitepaper lines and comes from the whitepaper, not from current code. Numeric limits that the book does not state are recorded in `design.md` as current implementation values.
- Specify step 6 of the funding order: provider credit only through authenticated open or top-up after independent signed-byte and settlement-policy checks, with its response codes. Steps 1–5, acknowledged-term survival and top-up recovery are referenced, not restated.
- Specify which native surfaces carry the registration-risk disclosure for openings, ambiguity guidance for top-ups, the desktop reorganization disclosure (A.1.6) and the frozen-account display (A.5.3).
- Specify the `fund` refusal of different terms and explicit credential issuance that restores only a missing default.
- Specify chain-evidence verification against the configured identity-checked RPC. Only signed-byte mismatches and canonical failed execution are payment rejections; contradictory or untrustworthy chain evidence is unavailable.
- Specify the bounded JSON-RPC transport: 64 KiB requests, ordinary responses and errors; 8 MiB only for hash-only `eth_getBlockByNumber`.
- Specify the `canonical-successful-receipt-at-espace-safe-v1` policy, with two consistent observations, chain health, bounded head and safe freshness, and distinct pending, executed, safe-accepted and finalized stages.
- Specify funding monitoring with five outcomes, the rule that a missing receipt is not proof of a reorg, proven-orphan freeze/reversal/loss journaling, exact restoration, outage handling and the extended balance identity.
- Specify explicit `cancel-funding`: authorization, the fixed replacement price `P + floor(P/8) + 1` wei, no automatic bump, outcome conditions and the meaning of "not a refund". Also specify default targeting versus the exact `--operation-id`, `funding-recovery`, a cancellation block scoped to its own operation, recovery of a won original without cancellation authority, journal retention, and reconciliation of a displaced payment that becomes canonical again.
- Specify journal path derivation, deployment separation, journal file protection and locking for every spender of a wallet, and sender/nonce ownership through terminal status and chain regression.
- Record a coverage table in `design.md` (requirement → code → existing tests, or the missing case). Record code differences and missing acceptance evidence as unchecked tasks.

## Capabilities

### New Capabilities

- `inference-funding-settlement`: Provider credit, chain evidence and RPC bounds, the safe-frontier settlement policy, monitoring, orphan and restoration accounting, outage behavior, funding disclosures, explicit cancellation and recovery, and wallet-journal coordination for paid inference funding.

### Modified Capabilities

None. This capability references `inference-wire-foundation`, `inference-account-opening-request` and `inference-account-topup-request`. It also references `inference-opening-funding-intent`, `inference-payment-envelope-bounds` and `inference-account-snapshot`, which were pending when this was written and are now main specifications (archived 2026-11-22, 2026-11-22 and 2026-11-21). It also references the still-pending `inference-commercial-terms`, `inference-request-accounting` and `desktop-share-online` capabilities. It adds no obligation to them and does not change their requirements. Native retention of a pending top-up after any refusal is already required by `inference-account-topup-request` ("retaining pending local evidence until successful acceptance is confirmed and persisted").

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements: `src-tauri/src/chain_client.rs`, `rpc_client.rs`, `billing_store.rs`, `contracts.rs` (provider credit and maintenance), `app.rs` (monitoring cadence and deployment layout), `consumer.rs` (`fund`, `resume`, `topup`, `cancel_funding`, `funding_recovery`), `bin/chiral.rs` and `desktop.rs` (disclosure and explicit cancellation authorization), plus `src/pages/Marketplace.svelte` and `Usage.svelte` (disclosures and recovery).
- Seven code differences were found. Tasks 2.1–2.7 track them:
  - the desktop Marketplace overstates the acknowledgement ordering;
  - the CLI `fund` disclosure is incomplete;
  - the desktop top-up card does not say that reorganizations can freeze or reverse credit;
  - contradictory chain evidence is reported as a payment rejection and withholds accepted credit;
  - one failing payment stalls funding monitoring;
  - a won original is recoverable only through the cancellation command;
  - an opening cancellation blocks every later `resume` of the agreement.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.10 and 3.12–3.18. It covers:
  - disclosure and guidance text on the CLI and desktop (3.2, 3.3);
  - `fund` refusing different terms (3.4);
  - the 503 response for untrustworthy chain evidence (3.1), and receipt-field and identity mismatches at the provider (3.5);
  - RPC redirect refusal (3.6);
  - safe-only staleness (3.7);
  - the settlement-policy identifier and finality wording (3.8);
  - multi-batch, poison-row and contradictory monitoring (3.9);
  - a contradictory observation after an orphan (3.10);
  - readiness closing after an outage (3.12);
  - direct refusal of an unbalanced write (3.13);
  - refusal of unauthorized cancellation (3.14);
  - foreign operation IDs and unprepared top-ups (3.15);
  - recovery without cancellation authority and the opening-cancellation scope (3.16, 3.17);
  - journal sharing across deployments and journal file protection (3.18).
- Three questions for the whitepaper author are recorded in `design.md`: top-ups to frozen accounts, spendability under contradictory evidence, and a CLI reorganization disclosure.
- No ledger schema, wire format, deployment or whitepaper change is part of the specification step. This change grants no live-chain reorg, public-testnet or release qualification.
