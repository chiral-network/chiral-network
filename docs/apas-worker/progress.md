# APAS progress

## 2026-11-14 — Valid-original native journal refusal

- The read-only book, README and CI hashes match their preceding complete reads; no standard whitepaper candidate or new author answer appeared. Book SHA-256 remains `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`.
- Added opening-funding-intent task 2.6.1's worker-owned native acceptance for a valid saved payment whose journal is absent or contains another correctly signed opening at nonce one. Existing synthetic `0x00` coverage alone did not establish this boundary.
- Both `resume` and repeated `fund` run after consumer reopening and must refuse before chain RPC or another acknowledgement, preserving saved bytes, all journal fields and original provider evidence. Explicit isolated-fixture repair provides a same-original progress control against the refusal-only RPC.
- Production source and whitepaper remain untouched. The test does not implement automatic journal repair or prove fresh spendability, successful submission/settlement, physical crash durability, broadcast-start-without-acknowledgement or quota mitigation. Parent tasks and the author report remain open.

## Verification

- Passed strict validation of `specify-opening-funding-intent` and the unchanged-whitepaper checksum; 374 all-target headless Rust tests (two existing child helpers ignored), headless binary build, frontend type checks (zero errors/warnings), 15 frontend tests/build, 46 Python tests and compilation of CI's seven scripts: 435 passing tests total. The new test covers four native journal refusals and two repaired-original progression controls. Stale Browserslist data remains a build warning; no dependency update. Known blocked regressions were not rerun.
- WebKit and the configured release policy authority remain unavailable. Release/native-desktop, Windows/macOS and live paid/GPU/public qualification remain unrun gates; no installation or deployment.

## Next steps, in order

1. Native non-reserving cutover (parent 3.1–3.5) and proposal-rate clock/adoption work (2.4–2.5) remain **blocked by entirely owner-untracked ledger/handler/consumer source**. Do not adopt that migration, introduce unused helpers or commit expected-failing regressions alone. Clear both public wallet gates together and migrate affected tests; keep pre-signing work separate from committed issuance.
2. After verifying 2.6.1, valid-original missing/mismatching-journal refusal is covered through both native entry points. **Next: opening 2.5's saved broadcast-start marker without an acknowledgement**, using the existing worker-owned native fixture and unchanged originals; then missing persistence-failure and ambiguous-submission acceptance. Do not duplicate existing crash checkpoints or adopt private owner source.
3. Then complete independently reachable final-capacity, committed-response-loss and free-opening acceptance (opening 2.3.3–2.3.4, 2.8), and finish the remaining 2.6 inventory. Exact expiry/restart or new resource-gate assertions needing private native seams remain blocked with their fixes. Preserve safe-frontier prerequisites; isolated fixtures do not qualify live settlement or power-loss recovery.
4. Prioritize any independently committable failing check, then retained code/spec gaps below. Earlier cost/nonce/schema/rollback and exact-raw-string evidence remains valid within its recorded limits; native journal refusal does not prove the unimplemented quote-policy cutover.
5. After the bounded pre-credit contract, specify safe-credit/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Other retained blockers

- **Envelope-bounds 2.5–2.6:** require RLP byte-string shape before scalar/opaque extraction. Decoder/unit tests remain owner-untracked; `.data()` is not a type check. Pair the regression with an independently committable native fix. Retained reproduction, not rerun: build `PaymentFields::opening(..., 0, 11)`, sign a valid legacy original, then `resign_field(&original, 0, fields.chain_id, &fixture.owner_key, 1, &[0xc1, 0x0b])`. The local `ChainError::Rejected` assertion fails because `field_bytes` extracts the list payload and `uint` treats `[0x0b]` as gas price 11. Failure preceded HTTP admission; no route acceptance, execution or credit was established. Private byte/item/depth isolation remains blocked; the raw 16-KiB equality case is unreachable and this defect cannot manufacture positive controls.
- **Commercial 2.1.4/2.2/2.3:** localhost-alias, embedding output-price and embedding-template fixes are in owner-untracked source. Exact now-relative offer/qualification boundaries (2.1.3.2) need a safe explicit-time seam, not sleeps or weakened requirements.
- **Challenge clocks:** challenge-rate 2.4–2.5 remains independently blocked in owner-untracked source/unit tests. Preserve independent retained evidence; prior failed-future-minute rollback tests do not establish successful-clock non-regression. Challenge 3.1 is complete but its archive remains blocked.
- **Owner-control 2.4.3:** fix HTTP/ledger reopening without rotating an existing root, migrate the old rotation expectation while preserving no-duplicate-credit assertions and prove real-router recovery. Owner-untracked source/tests block the fix; native saved-account recovery already avoids reopening. Exact native 600/601-second receiver expiry (2.2.3) remains blocked at the private unit-test boundary.
- **Opening-funding-intent:** object-only request/acknowledgement envelope/payload/network guards (2.1.2), pre-issuance registration rejection (2.4) and exact acknowledgement-time boundaries (2.7) remain blocked in owner-untracked parser/protocol/ledger/consumer source. Positional decoding is observed, not allowed. Do not borrow rate clocks for signed time or weaken schemas.

## Checkout discipline

Entry had no staged files and extensive owner migration changes; the candidate test, OpenSpec and APAS files were clean. Commit only this step's `inference_funding_acknowledgement_test.rs`, opening-intent design/tasks and APAS progress/report. Exclude the owner's whitepaper and production migration; preserve the complete author `Answer:` and unrelated changes. No push, stash, reset, restore, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
