# APAS progress

## 2026-11-14 — Access-list shape and opaque-width acceptance

- Compared the complete read-only book, envelope-bounds specification, shared decoder and existing acceptance. The decoder already enforces the specified shape/width rules; executable coverage was missing. No new author decision or production fix is required.
- Added three `inference_funding_admission_test` groups for task 2.2.2: ten malformed list/entry/field shapes, 0/19/21-byte addresses and 0/31/33-byte keys, for both typed envelopes. Thirty-two isolated refusal/control pairs use real signatures over canonical RLP; the list-valued key stays at depth four so a nesting limit cannot mask its wrong type.
- Every candidate must fail local validation and authenticated HTTP admission without acknowledgement/submission/credit/fees, preserving the quote for its unchanged valid control. Controls use exact-width zero-filled opaque addresses/keys. Existing empty-list, duplicate and count-boundary tests remain intact.
- No runtime/schema, wallet/server, limit or whitepaper change; RPC remains synthetic. Task 2.2.2 and parent 2.2 are complete. Exact occurrence-based gas, local calldata and defensive parser budgets remain in 2.3–2.5.
- Whitepaper/report remained unchanged at entry, with no author answer; **Public proposals can exhaust a named payer's quota** remains open and nonblocking. No new author decision or report entry. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed: all six strict OpenSpec validations, unchanged-whitepaper checksum, frontend type checks (zero errors/warnings), 15 frontend tests/build, 46 Python tests and compilation of CI's seven scripts. The build reports stale Browserslist data; no dependency update was attempted.
- Passed: 360 all-target headless Rust tests (the two existing child-helper tests remain ignored by the ordinary runner) and headless binary build. Total: 421 passing Rust/frontend/Python tests. Parent 2.2's five groups now cover 44 refusal/control pairs; later acceptance tasks and archive stay open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require production clocks, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. Exact native 600/601-second receiver expiry (2.2.3) remains blocked at the private unit-test boundary.
4. Opening-funding-intent 2.1.2 requires object-only request and acknowledgement envelope/payload/network guards; positional decoding is observed, not permitted. Task 2.4 needs the pre-issuance registration guard; 2.7 needs exact native acknowledgement-time boundaries. These fixes/seams remain blocked in owner-untracked parser/protocol/ledger/consumer source. Do not borrow rate clocks for signed time, weaken schemas or commit failing regressions alone.
5. Next unblocked step: envelope-bounds 2.3's occurrence-based structural-gas equality/one-below cases, including repeated addresses/keys whose deduplicated cost would fit, followed by local 4,096/4,097-byte calldata (2.4). Direct private parser-limit work in 2.5 is blocked where owner-untracked native seams are required; the raw 16 KiB equality case is unreachable under the current profile, not permission to weaken it. Then finish opening-funding 2.2's raw/qualification inventory and 2.3, 2.5–2.6, 2.8 recovery work. Parent opening 2.1 and both archives remain open.
6. Challenge-rate 3.1 is complete; its final archive remains blocked by 2.4–2.5. After this bounded pre-credit funding contract, specify safe-credit settlement/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the admission test, envelope-resource design/tasks and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
