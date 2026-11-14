# APAS progress

## 2026-11-14 — Provider-wide committed proposal rates

- Read the complete whitepaper, report answer and current notes; no new author change. Authority remains `docs/chiral-book.md`, SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. The author-approved non-reserving decision still takes priority over the old wallet quotas.
- Completed task 2.1 of the existing `specify-non-reserving-quotes` change: retargeted the proposal-rate delta, proposal, design and tasks to one 600-per-effective-minute provider-ledger committed budget, with no named-wallet allowance.
- Preserved atomic insertion/global-counter rollback, nonrefundable successful issuance, durable non-regressing time and independent recovery. Specified conservative adoption from the retained global counter: never sum obsolete wallet rows; missing/contradictory totals fail closed; retire obsolete rows only with successful adoption. A separately committed signing-work permit is not refunded by insertion failure.
- Compared native issuance/storage/cleanup and existing global-race/failed-insertion tests. Old wallet ceilings and clock gaps still exist in owner-untracked source. Retained earlier test results as historical evidence and reopened the changed acceptance work; no production code or tests changed. Both changes remain open, and the author report is not yet resolved.

## Verification

- Passed: strict validation of `specify-proposal-rate-budgets` and `specify-non-reserving-quotes`, plus unchanged-whitepaper checksum.
- Passed: 373 all-target headless Rust tests (two existing child helpers ignored by the ordinary runner), headless binary build, frontend type checks (zero errors/warnings), 15 frontend tests/build, 46 Python tests and compilation of CI's seven scripts. Total: 434 passing tests. These checks exercise the existing implementation, not mitigation; known blocked regression failures were not rerun.
- The frontend build reports stale Browserslist data; no dependency update was attempted.
- Entry probes found no configured release policy authority or WebKit desktop dependency. Release/native-desktop, Windows/macOS and live paid/GPU/public qualification remain unrun gates. No installation or deployment; Rust invocations remain sequential.

## Next steps, in order

1. **Next unblocked specification step: non-reserving-quotes 2.2.** Reconcile opening-funding-intent and commercial bounded-proposal wording/tasks with overbookable quotes, provider-wide retained storage and authenticated paid/free capacity. Preserve exact accepted acknowledgements, original deadlines and nonce commitments. Do not archive conflicting old wallet rules as an intermediate step.
2. Implement parent 3.1–3.5 with migrated contract tests, then acceptance 4.1–4.4. **Native cutover is blocked by entirely owner-untracked ledger/handler/consumer source:** do not absorb that migration, add unused helpers or commit expected-failing tests alone. Clear both old wallet gates together; removing only the eight-slot ceiling is insufficient. Revised proposal-rate 2.1–2.3 remain open because historical wallet tests do not prove the new path.
3. Proposal-rate 2.4–2.5 still need the global clock/cleanup/adoption fix and regressions alongside the native cutover. Its global committed counter is separate from parent 3.2's signing-work limiter; initialization and failed-insertion clock semantics must not be conflated.
4. Prioritize any independently committable failing check, then remaining code/spec gaps below. Resume exact-acknowledgement replay/lost-response and native broadcast/recovery acceptance under the new contract, not the superseded payer-slot policy. Prior committed opening rollback task 2.3.1 remains complete; isolated rollback/reopening is not power-loss or financial qualification.
5. After the bounded pre-credit contract, specify safe-credit/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Other retained blockers

- **Envelope-bounds 2.5–2.6:** require RLP byte-string shape before scalar/opaque extraction. Decoder/unit tests remain owner-untracked; `.data()` is not a type check. Pair the regression with an independently committable native fix. Retained reproduction, not rerun: build `PaymentFields::opening(..., 0, 11)`, sign a valid legacy original, then `resign_field(&original, 0, fields.chain_id, &fixture.owner_key, 1, &[0xc1, 0x0b])`. The local `ChainError::Rejected` assertion fails because `field_bytes` extracts the list payload and `uint` treats `[0x0b]` as gas price 11. Failure preceded HTTP admission; no route acceptance, execution or credit was established. Private byte/item/depth isolation remains blocked; the raw 16-KiB equality case is unreachable and this defect cannot manufacture positive controls.
- **Commercial 2.1.4/2.2/2.3:** localhost-alias, embedding output-price and embedding-template fixes are in owner-untracked source. Exact now-relative offer/qualification boundaries (2.1.3.2) need a safe explicit-time seam, not sleeps or weakened requirements.
- **Challenge clocks:** challenge-rate 2.4–2.5 remains independently blocked in owner-untracked source/unit tests. Preserve independent retained evidence; prior failed-future-minute rollback tests do not establish successful-clock non-regression. Challenge 3.1 is complete but its archive remains blocked.
- **Owner-control 2.4.3:** fix HTTP/ledger reopening without rotating an existing root, migrate the old rotation expectation while preserving no-duplicate-credit assertions and prove real-router recovery. Owner-untracked source/tests block the fix; native saved-account recovery already avoids reopening. Exact native 600/601-second receiver expiry (2.2.3) remains blocked at the private unit-test boundary.
- **Opening-funding-intent:** object-only request/acknowledgement envelope/payload/network guards (2.1.2), pre-issuance registration rejection (2.4) and exact acknowledgement-time boundaries (2.7) remain blocked in owner-untracked parser/protocol/ledger/consumer source. Positional decoding is observed, not allowed. Do not borrow rate clocks for signed time or weaken schemas.

## Checkout discipline

Entry had no staged files and extensive owner migration changes. All seven step files were clean at entry. Commit only the proposal-rate change's proposal/design/delta/tasks, the parent change's task list, progress and report; exclude the owner's whitepaper and all source/tests. Preserve the author's complete `Answer:`. No push, stash, reset, restore, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
