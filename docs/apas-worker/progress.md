# APAS progress

## 2026-11-17 (twelfth round) — CLI loopback gateway specified

- In step at `d064cbda`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-cli-loopback-gateway` (capability `cli-loopback-gateway`, 12 requirements, about 55 scenarios), the follow-up that milestone 2 named for the book's "the native desktop and CLI therefore run a local loopback gateway" (line 315). `chiral --config <profile> gateway serve --port <port>` is a foreground, loopback-only gateway for funded network accounts (HTTPS or libp2p). It reuses milestone 2's gate, bounds, routing, route-credential, request-identity and recovery requirements by name.
  - CLI-specific rules: a required port with no fallback; signal shutdown that cancels in-flight requests and reports them as possibly charged; owner-only state scoped to network and wallet; a bearer that no command prints; route commands that issue dedicated capped credentials, always show the spending disclosure and need `--accept-spending-risk`; JSON-line events on stdout with the transport path; and no consumer lock held while serving.
  - M2's proposal and design now point at the change. The review fixed six defects, including an issued credential left unmentioned when its route cannot be saved, and an unrequired DHT start. All three affected changes pass strict validation. No code changed; checks are unchanged since round 11.
- Report: new entry asking whether the CLI gateway should also serve local models (default: no).

## 2026-11-17 (eleventh round) — Code: the desktop re-reads the funding journal after every funding attempt

- In step at `820b6567`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.9, requirement 2). `Usage.svelte` used to leave the pending-funding journal stale after a top-up or resume attempt. After a top-up error the *Funding & recovery* tab said nothing was pending until *Refresh*, and a resolved operation could leave a stale notice. `topup()`, `resume()` and the recovery-tab `recover()` now go through an `attempt()` helper that re-reads `pending_accounts` whether the action succeeds or fails. A new frontend regression fails on the previous page. Screenshots show the notice, and the stage and hash, after a failed top-up without *Refresh*. Checks: svelte-check 0/0, 32 frontend tests, build, 46 script tests. Rust is unchanged since the full headless suite passed at `820b6567` (475 passed, 2 ignored).
- `specify-funding-settlement` code differences are now all fixed except 2.4, which waits for the report question. Its remaining open items are acceptance tests (3.x) and live-chain evidence (4.x).

## 2026-11-17 (tenth round) — Code: funding-risk disclosures on the Marketplace, Usage and CLI

- In step at `ea1c7202`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` tasks 2.1–2.3, requirements 2 and 3). Before this step, the Marketplace funding panel and its pending notice said payment is broadcast only after the provider's acknowledgement, and the CLI `fund` text and the Usage top-up card left out parts of the book's disclosures. All three now follow book line 489 and A.1.6. The Marketplace panel has a *Residual registration risk:* paragraph: the client broadcasts only after a durable acknowledgement, the provider holds broadcastable bytes and could broadcast before or without acknowledging, the ordering is not cryptographic escrow, and an error does not prove no money moved. Its acceptance covers that risk and the reorganization statement. The CLI `fund` stderr has the same elements. The Usage top-up card states that reorganizations can freeze or reverse credit and must be accepted before *Sign and top up*, without registration wording. New tests: seven in `tests/fundingDisclosure.test.ts` and one CLI process test; the four that cover new text fail on the old text. Screenshots of the panel, the pending notice, the top-up card and a frozen account (browser preview with a scratchpad-only IPC stub) were checked. Task 3.3 is complete.
- Spec: the review found that `Usage.svelte` does not re-read the funding journal after a top-up or resume attempt, so the journaled stage is not shown after a top-up error until *Refresh*. This is recorded as design code difference 9 and task 2.9.

## 2026-11-17 (ninth round) — Code: a top-up cancellation request is durable before the attempt

- In step at `3ea39335`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.8, requirement 16): `cancel_funding` now saves the top-up's cancellation request together with its validated original bytes before it calls `cancel_payment_once`, as it already did for an opening. Previously, an attempt that failed before journaling a replacement (for example, unaffordable cancellation gas) left the original broadcastable by `resume`, `fund` or `topup` while its nonce was current. From then on those paths never call `send_payment_once` for that operation. They run the read-only reconcile instead: a won original is credited once from its exact bytes, and a resolution without a win clears the intent. An unresolved request still refreshes the account and returns an error that names `funding-recovery` and the explicit retry. `funding-recovery` records a no-replacement win, and the pending view shows `topup_cancellation_requested`. Two regressions fail without the change. The adversarial review found no defect after four extra probes (nonce not current, every settle path after a win, a reorg displacing a recorded win, and a stale win followed by a replacement).
- Known limits, not spec differences: an older binary ignores the new `TopupIntent` field, so a downgrade loses the protection. A no-replacement top-up that resolved `original_failed` at safe is no longer tracked if a deeper reorg later executes it, which matches the existing `cancel_funding` behaviour; explicit `cancel-funding --operation-id` still recovers it.

## 2026-11-17 (eighth round) — Code: a won original is settled by resume without cancellation authority

- In step at `7a8eb7ac`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.6, requirements 16 and 17): before this change, an original that won its cancellation race could be recovered only through `cancel-funding --accept-gas-and-race-risk`, a call that can resubmit the replacement. Now `funding-recovery` and `resume` run the read-only reconcile and durably record each cancelled operation's outcome with the saved agreement (`funding_cancellation`, new `won_topups`). `resume` then opens a won opening, or credits a won top-up once, from the exact journaled original. It never signs, broadcasts or calls the cancellation path, and it withdraws a recorded win that a reorg later displaced. The refusal message no longer points to `cancel-funding`. The adversarial review fixed two gaps: an opening whose cancellation journaled no replacement, and a stale win that could reach `send_payment_once`. Four regressions fail without the change.
- Spec: the review also found that a top-up cancellation request is not saved before the cancellation attempt, so a failed attempt leaves the original broadcastable by `resume`. This is recorded as design code difference 8 and unchecked task 2.8.

## 2026-11-17 (seventh round) — Code: one refused funding recheck no longer stalls monitoring

- In step at `de5eb44e`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.5, requirement 9): `contracts::reconcile` used to abort the whole pass on a ledger refusal without advancing that row, so one payment blocked every later recheck and maintenance stopped accepting. A refused row is now quarantined durably and its `checked` time advanced. Quarantined rows are revisited first; while any remain, new funding and offers stay closed under a fixed category, while existing credit keeps serving. The quarantine is released when an actual chain observation applies. A ledger regression and the extended reconcile integration test both fail without the change.

## 2026-11-17 (sixth round) — Code: opening cancellation no longer blocks account refresh or top-up recovery

- In step at `aa8342c3`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.7, requirements 4 and 16): `Consumer::resume_inner` used to refuse every `resume`/`fund` after an explicit opening cancellation, even on a saved account, which stranded a pending top-up. Now only an unopened cancelled opening is refused. A saved account skips the cancelled opening's payment step, still recovers a pending top-up's exact original bytes and refreshes. Two regressions fail without the change and pass with it.

## 2026-11-17 (fifth round) — Funding settlement specified

- In step at `763ea347`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-funding-settlement` (capability `inference-funding-settlement`, 19 requirements, 65 scenarios). It covers the whitepaper's safe-frontier credit policy, monitoring and reorg/orphan freeze, reversal and loss, restoration, outages, the balance identity, `cancel-funding`, funding recovery, displaced re-inclusion and nonce-journal ownership. Checked tasks cite tests that were re-run and pass; synthetic fixtures are not live-chain evidence.
- Code differences recorded as unchecked tasks 2.1–2.7:
  - desktop and CLI registration-risk disclosure wording;
  - Usage top-up reorganization disclosure;
  - contradictory evidence treated as rejection;
  - one failing recheck stalls monitoring;
  - recovering a won original only through cancellation;
  - a contract-wide opening-cancellation gate.
- Report: three author questions (top-ups to frozen accounts, contradictory evidence vs existing credit, CLI reorganization disclosure).

## 2026-11-17 (fourth round) — M2 session scoping; the comparison's contradictions are all fixed

- In step at `a40d35ea`; whitepaper unchanged; no new answers.
- Spec step:
  - `specify-desktop-local-api-server`: network routes are scoped to network identity and consumer wallet. Another session's routes are hidden on the Local server page (count only, not deleted), and network-route log entries are removed on logout or a profile change (book A.1.8, scoped by A.7). The design now quotes *Use the network* exactly.
  - `specify-desktop-local-models`: its proposal now says five milestones.
  - Both validate strictly; no code changed.
- Every spec-contradicts-whitepaper finding from the 2026-11-17 comparison is now resolved in the specs. What remains is missing specification, then implementation.

## 2026-11-17 (third round) — M4/M5 aligned with the whitepaper

- In step at `0a8e8cf0`; whitepaper unchanged; no new answers.
- Spec step:
  - M4: Auto-price is unavailable for embedding shares, because they cannot hold the valid qualification *Ready to charge* needs; manual pricing stays. This supersedes the earlier embedding-readiness default. The publish disclosure now states the book's prepaid-balance sentence unconditionally.
  - M5: availability sampled at a fixed 300 s period with a random per-identity phase; "24 hours observed" from the exact first counted sample; *Launching — free* only for a generation provider that is free and has never charged; a per-wallet tombstone keeps `sharing_since` after the 180-day retention; no readiness for embedding-only wallets.
  - M1, M2 and M4: stale lines aligned.
  - All five drafts pass strict validation; no code changed.
- Report: new entry on predictable fixed-interval sampling and the *Launching — free* approximation.

## 2026-11-17 (second round) — Durable relay grace evidence and the 30-day deletion bound (M3)

- In step at `e228c76e`; whitepaper unchanged (SHA-256 `eda22ce0…`); no new answers.
- Spec step in `specify-p2p-provider-transport`. Offer evidence is now durable: a protected, owner-only, network-scoped store that fails closed when damaged, with wall-clock caps across restarts and the monotonic cap within a run. A relay restart no longer ends the 24-hour grace. Eviction from the operator-configured evidence maximum (earliest grace end first) is the one disclosed early end, reported in a new loopback-only, bearer-authenticated `relay_server` administration status. Ended free accounts' records, status and receipts now carry the 30-day deletion bound everywhere they were said to stay available. M4's copies of the relayed-grace statement, plus stale lines in M1 and M2, were aligned. All four changes pass strict validation; no code changed.
- Report: new entry proposing whitepaper wording for the evidence-store bound, with the fail-closed and relay-admin-status assumptions.

## 2026-11-17 — Main proposal-admission spec aligned with non-reserving quotes

- Entry clean at `38fc8bea`, in step with `origin/main`; no merge in progress. Whitepaper `docs/chiral-book.md` unchanged (SHA-256 `eda22ce0…`); no new `Answer:` lines.
- Ran a systematic spec-versus-whitepaper comparison (six slices, each finding adversarially verified). Highest confirmed contradiction: the archived main spec `inference-proposal-admission` still required the superseded named-wallet quote gates, which **Approved non-reserving quotes** forbids and the code had already removed.
- Spec step: moved the finished, implemented and tested deltas unchanged from `specify-non-reserving-quotes` into `adopt-provider-wide-quote-retention` and archived it (`+3 ~3 -3`); corrected the main spec Purpose. `specify-non-reserving-quotes` keeps only *Public quotes do not reserve opening capacity* and *Authenticated paid admission preserves accepted commitments* (open tasks 3.3, 4.3, 4.4). No code change. Checks: strict validation of the main spec and of the non-reserving, proposal-rate and opening-intent changes; svelte-check 0/0, 24 frontend tests, frontend build, 46 script tests. Rust code is unchanged since the full headless suite passed (465 tests) at `38fc8bea`.
- Report: worker action under the quota entry (stays open); new entries on the Hugging Face token location, the free-share immutability exception wording, and the assumption that deleting an ended free account frees its wallet's slot.

## Next steps, in order

Missing specifications (spec before code):

1. Milestone 3 gap found while specifying the CLI gateway: neither `specify-p2p-provider-transport` nor any other change says how two processes on one consumer profile (the desktop and a CLI gateway) share its DHT identity and fixed `dht.listen` port. Specify it in M3.
2. Performance remedies (warm-generation-v1 envelope, TTFT/decode definitions, waiver eligibility, withdrawal latch) and fee remittance (obligation states, aggregation thresholds, remittance gas) have no SHALL requirements. (Funding settlement is `specify-funding-settlement`: every code difference except 2.4 is fixed, and 2.4 waits for the report question. Its missing acceptance tests are listed in tasks 3.x.)
3. Appendix A required GUI behavior (A.1 global rules and the required parts of A.5) has no capability except `desktop-window-lifecycle`. The behavior exists in the code; specify it, then confirm with screenshots.

Earlier open work, unchanged:

4. Non-reserving 3.3/4.3, snapshot 2.5/3.3–3.4, challenge-clock non-regression, request-accounting 2.2.4/2.3/2.5/3.1–3.3, and the other acceptance items listed in the changes' tasks. Archive `specify-proposal-rate-budgets` once `specify-non-reserving-quotes` completes.
5. Milestone implementation M1 → M3 → M2 → M5 → M4. Extend `verus.md` judgments; proofs wait for Verus.

## Blocked

- Verus setup and proofs: open report entry (not installed on this Mac).
- The M1 engine-confinement task waits for the report entry on the Hugging Face token location.
- Real chain, engine, GPU, relay deployment, installers and public-release gates are outside this checkout.

## Checkout discipline

The untracked `src-tauri/gen/schemas/macOS-schema.json` is generated build output and is never committed. The whitepaper is read-only.
