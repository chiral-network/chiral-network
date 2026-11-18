## Why

The read-only whitepaper (`docs/chiral-book.md`) defines the platform fee as a provider-remitted funding charge. Line 528 says that each accepted funding payment owes `floor(funding_wei * 50 / 10000)`, that this is not a per-request charge, and that fee obligations are durable and tracked as pending, batched, submitted and confirmed. It sets automatic aggregation at 10^15 wei or an oldest obligation reaching 86,400 seconds, makes `fees/remit` an explicit local operator action, says that a recorded obligation is not proof that the platform received the fee, and puts remittance gas on the provider. Supporting lines name the signed platform wallet (63, 126, 153), the shared signing-wallet journal for fee remittance (470), the risk that a provider fails to remit (33), the local administration listener (274, 658–667), incident reconciliation of fee batches (677) and the operator fee views (945, 1001). `CLAUDE.md` adds: "Track obligations and actual remittances; collection is not trustless."

Existing specs cover only the edges:

- `inference-funding-settlement` (pending `specify-funding-settlement`) owns the fee formula and credit arithmetic, the balance identity, and what an orphan does to a fee obligation;
- `inference-account-topup-request` (normative) charges the fee once per top-up and tracks a nonzero obligation;
- `inference-commercial-terms` (pending) fixes `fee_bps` at 50 and carries `platform_wallet` in offers and quotes;
- `inference-request-accounting` (pending) keeps waivers from refunding fees;
- `desktop-share-online` (pending) discloses automatic remittance and offers *Remit now* on the future Share dashboard.

None of them specifies the obligation lifecycle, the aggregation trigger, the batch, the remittance transaction, what "confirmed" means, monitoring and reorganization of a remittance, crash recovery, the explicit operator action or the operator fee view. Those rules exist only as prose and code, so a later change could weaken them without a failing requirement.

## What Changes

- Add the `inference-fee-remittance` capability with 12 requirements and 44 scenarios. Each requirement cites its whitepaper lines and comes from the whitepaper, not from current code. Numeric values that the book does not state are recorded in `design.md` as current implementation values.
- Specify that the fee is a funding charge only: one per accepted funding payment, none per request, none for a free opening, and no obligation for a zero-wei fee.
- Specify one durable obligation per nonzero fee, owed to the platform wallet of the accepted signed agreement, never redirected by later configuration.
- Specify the four obligation states and the only transitions between them.
- Specify automatic aggregation per platform wallet: inclusive thresholds of 10^15 wei, summed over every pending obligation of that wallet, or an oldest obligation at least 86,400 seconds old. Aggregation continues while the provider drains.
- Specify sealed-batch immutability, including that orphaned funding never rewrites a sealed batch.
- Specify `POST /admin/fees/remit` and `GET /admin/fees` on the loopback administration listener only, with the administrator bearer and `Origin` refusal, plus the protections of the helper script and the native `provider_admin` command.
- Specify the remittance transaction: one per batch, from the provider wallet, through the shared nonce journal, with gas paid by the provider and never charged to consumers.
- Specify that "confirmed" means the funding settlement policy `canonical-successful-receipt-at-espace-safe-v1`, not finality, and that confirmed remittances are monitored and returned to submitted on a reorganization, recovered only by their original signed bytes.
- Specify restart and ambiguity recovery without fee bumps or replacements, and how a failed remittance is reported.
- Specify the operator fee view: totals per state, the list of actual remittances, and the statements that collection is not trustless and that an obligation is not proof of receipt.
- Record a coverage table in `design.md` (requirement → code → existing tests, or the missing case). Record code differences and missing acceptance evidence as unchecked tasks.

## Capabilities

### New Capabilities

- `inference-fee-remittance`: The platform-fee obligation lifecycle, automatic and explicit aggregation into immutable batches, the provider-paid remittance transaction through the shared nonce journal, confirmation, monitoring and crash recovery of remittances, and the operator fee view.

### Modified Capabilities

None. This capability references the normative `inference-account-topup-request` and the pending `inference-funding-settlement`, `inference-commercial-terms`, `inference-request-accounting`, `inference-account-snapshot` and `desktop-share-online` capabilities. It adds no obligation to them and does not change their requirements.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/billing_store.rs`: `fee_cost`, `credit_funding`, `prepare_fee_batches`, `update_fee_batch`, `fee_batches_to_recheck` and `fee_summary`;
  - `contracts.rs`: `reconcile_fees`, `remit_fees` and `remit_inner`;
  - `app.rs`: the 30-second maintenance loop, the administration router and `admin_auth`;
  - `chain_client.rs`: `send_payment_once`, `verify_payment` and `recheck_payment`;
  - `provider_config.rs`: the loopback administration bind;
  - `consumer.rs` `provider_admin`, `scripts/provider-admin.py` and `src/pages/Provider.svelte`.
- Four code differences were found. Tasks 2.1–2.4 track them:
  - the fee view does not report submitted obligations apart from batched ones, and calls confirmed obligations `remitted`;
  - the fee view lists no remittance transactions;
  - aggregation considers only the 1,000 oldest pending obligations per pass, so the 10^15-wei trigger can be missed and an explicit `fees/remit` can leave older obligations pending;
  - monitoring picks the page of confirmed remittances to recheck from the wall-clock time, so with more than 100 confirmed batches and passes 60 seconds apart, some batches are never rechecked.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.12. The main gaps are contract-level remittance tests with an RPC fixture (submission, confirmation, reorganization, crash between journal and ledger, and failed execution), aggregation boundary tests, and tests of the administration routes and helper.
- Live remittance evidence beyond the single isolated fee transfer recorded in the migration plan is task 4.1. It needs an authorized private deployment and is never satisfied by fixtures.
- Two questions for the whitepaper author are recorded in `design.md` and `docs/apas-worker/report.md`: what evidence "confirmed" requires, and how a remittance that failed or lost its nonce is resolved.
- No ledger schema, wire format, deployment or whitepaper change is part of this specification step. This change grants no live-chain, public-testnet or release qualification.
