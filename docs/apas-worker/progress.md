# APAS progress

## 2026-11-17 — Main proposal-admission spec aligned with non-reserving quotes

- Entry clean at `38fc8bea`, in step with `origin/main`; no merge in progress. Whitepaper `docs/chiral-book.md` unchanged (SHA-256 `eda22ce0…`); no new `Answer:` lines.
- Ran a systematic spec-versus-whitepaper comparison (six slices, each finding adversarially verified). Highest confirmed contradiction: the archived main spec `inference-proposal-admission` still required the superseded named-wallet quote gates, which **Approved non-reserving quotes** forbids and the code had already removed.
- Spec step: moved the finished, implemented and tested deltas unchanged from `specify-non-reserving-quotes` into `adopt-provider-wide-quote-retention` and archived it (`+3 ~3 -3`); corrected the main spec Purpose. `specify-non-reserving-quotes` keeps only *Public quotes do not reserve opening capacity* and *Authenticated paid admission preserves accepted commitments* (open tasks 3.3, 4.3, 4.4). No code change. Checks: strict validation of the main spec and of the non-reserving, proposal-rate and opening-intent changes; svelte-check 0/0, 24 frontend tests, frontend build, 46 script tests. Rust code is unchanged since the full headless suite passed (465 tests) at `38fc8bea`.
- Report: worker action under the quota entry (stays open); new entries on the Hugging Face token location, the free-share immutability exception wording, and the assumption that deleting an ended free account frees its wallet's slot.

## Next steps, in order

Remaining verified spec contradictions (fix in the specs, in this order):
1. M3 `specify-p2p-provider-transport`: (a) the 24-hour relay grace can end early on relay restart or evidence eviction, while the whitepaper bounds it only by per-provider quotas. Persist bounded per-relay offer evidence so a restart keeps the grace, and state eviction honestly; if no honest bounded design avoids an early end, ask the author. (b) The MODIFIED opening-admission and allowance requirements keep ended free accounts' status and receipts available, contradicting their deletion 30 days after ending (*Wire change*); add the deletion bound at spec.md:220/:279 and correct design decision 25.
2. M4/M5: Auto-price for embedding shares contradicts *Ready to charge*, which needs a valid qualification embeddings cannot hold. Make Auto-price unavailable for embedding shares (manual pricing stays) and drop the embedding-readiness criteria from M5.
3. M2 `specify-desktop-local-api-server`: network routes and network log entries must follow the loaded session (A.1.8, A.7). Record the wallet with each, and hide them after logout or a profile change.
4. M4 publish disclosure: state unconditionally that once the share charges, consumers prepay non-refundable balances only this machine can serve.
5. M5: sample availability at a fixed 300 s period with a random per-identity phase (book: "fixed interval"). Base "24 hours observed" on the exact earliest counted sample, not an hour-rounded `counted_since`. Show *Launching — free* only before the wallet's first charged period. Keep a per-wallet tombstone so the 180-day retention cannot restart `sharing_since`.
6. M1 proposal says "four milestones", should be five; M2 design misquotes A.1.6's "keeping every existing disclosure and adding the transport-path … disclosure".

Missing specifications (spec before code):

7. `specify-cli-loopback-gateway`: the book says the desktop **and CLI** run the loopback gateway. M2 and M3 both defer the CLI part to this change, which does not exist yet.
8. Performance remedies (warm-generation-v1 envelope, TTFT/decode definitions, waiver eligibility, withdrawal latch), fee remittance (obligation states, aggregation thresholds, remittance gas) and funding settlement/reorg (`canonical-successful-receipt-at-espace-safe-v1`, orphan freeze/reversal/loss, cancel-funding) have no SHALL requirements.
9. Appendix A required GUI behavior (A.1 global rules and the required parts of A.5) has no capability except `desktop-window-lifecycle`. The behavior exists in the code; specify it, then confirm with screenshots.

Earlier open work, unchanged:

10. Non-reserving 3.3/4.3, snapshot 2.5/3.3–3.4, challenge-clock non-regression, request-accounting 2.2.4/2.3/2.5/3.1–3.3, and the other acceptance items listed in the changes' tasks. Archive `specify-proposal-rate-budgets` once `specify-non-reserving-quotes` completes.
11. Milestone implementation M1 → M3 → M2 → M5 → M4. Extend `verus.md` judgments; proofs wait for Verus.

## Blocked

- Verus setup and proofs: open report entry (not installed on this Mac).
- The M1 engine-confinement task waits for the report entry on the Hugging Face token location.
- Real chain, engine, GPU, relay deployment, installers and public-release gates are outside this checkout.

## Checkout discipline

The untracked `src-tauri/gen/schemas/macOS-schema.json` is generated build output and is never committed. The whitepaper is read-only.
