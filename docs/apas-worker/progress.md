# APAS progress

## 2026-11-14 — Opening and commercial non-reserving alignment

- Authority, README and CI hashes match the preceding complete reads; the report has no new answer. Book SHA-256 remains `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. The author-approved non-reserving decision is read-only.
- Completed task 2.2 of `specify-non-reserving-quotes`: aligned the opening-intent and commercial proposal/design/delta/task files with provider-wide quote resources, authenticated paid/free opening capacity and accepted-original recovery.
- Preserved signed terms, original acknowledgements/transactions/nonces, safe-credit prerequisites and historical evidence. Replaced stale wallet-quota authority and follow-on tasks; added explicit pending acceptance for final-lease competition, definite full refusal, lost-response recovery and authenticated free opening. Earlier eight/twelve-unit tests remain historical exposure, not mitigation.
- Native source/tests remain unchanged. The author report and all incomplete changes stay open for native cutover and acceptance; no new author-only decision is needed.

## Verification

- Passed strict validation of `specify-opening-funding-intent`, `specify-inference-commercial-terms` and `specify-non-reserving-quotes`, plus the unchanged-whitepaper checksum.
- Passed 373 all-target headless Rust tests (two existing child helpers ignored by the ordinary runner), headless binary build, frontend type checks (zero errors/warnings), 15 frontend tests/build, 46 Python tests and compilation of CI's seven scripts: 434 passing tests total. These checks exercise the existing implementation, not mitigation of the revised quote policy; known blocked regression failures were not rerun.
- Stale Browserslist data remains a frontend build warning; no dependency update was attempted.
- Current probes confirm no WebKit desktop dependency or configured release policy authority. Release/native-desktop, Windows/macOS and live paid/GPU/public qualification remain unrun gates. No installation or deployment; keep Rust invocations sequential.

## Next steps, in order

1. Native non-reserving cutover (parent 3.1–3.5) and proposal-rate clock/adoption work (2.4–2.5) remain **blocked by entirely owner-untracked ledger/handler/consumer source**. Do not adopt that migration, introduce unused helpers or commit expected-failing regressions alone. Clear both public wallet gates together and migrate affected tests; keep pre-signing work separate from committed issuance.
2. **Next unblocked acceptance slice: opening-funding-intent 2.3's exact raw-string cached recovery.** Initial uppercase/mixed-case acceptance does not prove that a different spelling of the same decoded signed bytes/hash is refused after registration. Require conflict for alternate spellings and unchanged original acknowledgement recovery after reopening with fresh funding/qualification gates closed and RPC unavailable; preserve unrelated standalone quote storage, nonce/lease evidence and absence of credit. Do not use old payer-slot counts as an oracle.
3. Then complete independently reachable final-capacity, lost-response and native journal/broadcast recovery acceptance (opening 2.3.3–2.3.4, 2.5–2.6, 2.8). Exact expiry/restart or new resource-gate assertions needing private native seams remain blocked with their fixes. Preserve safe-frontier prerequisites; isolated fixtures do not qualify live settlement or power-loss recovery.
4. Prioritize any independently committable failing check, then retained code/spec gaps below. Prior committed cost/nonce/schema-member and rollback evidence remains valid within its recorded limits, not proof of the unimplemented quote-policy cutover.
5. After the bounded pre-credit contract, specify safe-credit/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Other retained blockers

- **Envelope-bounds 2.5–2.6:** require RLP byte-string shape before scalar/opaque extraction. Decoder/unit tests remain owner-untracked; `.data()` is not a type check. Pair the regression with an independently committable native fix. Retained reproduction, not rerun: build `PaymentFields::opening(..., 0, 11)`, sign a valid legacy original, then `resign_field(&original, 0, fields.chain_id, &fixture.owner_key, 1, &[0xc1, 0x0b])`. The local `ChainError::Rejected` assertion fails because `field_bytes` extracts the list payload and `uint` treats `[0x0b]` as gas price 11. Failure preceded HTTP admission; no route acceptance, execution or credit was established. Private byte/item/depth isolation remains blocked; the raw 16-KiB equality case is unreachable and this defect cannot manufacture positive controls.
- **Commercial 2.1.4/2.2/2.3:** localhost-alias, embedding output-price and embedding-template fixes are in owner-untracked source. Exact now-relative offer/qualification boundaries (2.1.3.2) need a safe explicit-time seam, not sleeps or weakened requirements.
- **Challenge clocks:** challenge-rate 2.4–2.5 remains independently blocked in owner-untracked source/unit tests. Preserve independent retained evidence; prior failed-future-minute rollback tests do not establish successful-clock non-regression. Challenge 3.1 is complete but its archive remains blocked.
- **Owner-control 2.4.3:** fix HTTP/ledger reopening without rotating an existing root, migrate the old rotation expectation while preserving no-duplicate-credit assertions and prove real-router recovery. Owner-untracked source/tests block the fix; native saved-account recovery already avoids reopening. Exact native 600/601-second receiver expiry (2.2.3) remains blocked at the private unit-test boundary.
- **Opening-funding-intent:** object-only request/acknowledgement envelope/payload/network guards (2.1.2), pre-issuance registration rejection (2.4) and exact acknowledgement-time boundaries (2.7) remain blocked in owner-untracked parser/protocol/ledger/consumer source. Positional decoding is observed, not allowed. Do not borrow rate clocks for signed time or weaken schemas.

## Checkout discipline

Entry had no staged files and extensive owner migration changes; all candidate OpenSpec and APAS files were clean. Commit only this step's opening/commercial proposal/design/delta/tasks, the parent's task list and APAS progress/report. Exclude the owner's whitepaper and all source/tests; preserve the complete author `Answer:` and unrelated changes. No push, stash, reset, restore, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
