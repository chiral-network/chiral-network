## 1. Correct the draft lifecycle

- [x] 1.1 Compare actual acknowledgement registration, account opening and existing recovery tests with the whitepaper and pending-quota delta; identify the worker's incorrect pending-retention description.
- [x] 1.2 Correct the still-open pending-quota definition/design and add the atomic acknowledgement-transfer requirement without changing implementation or whitepaper.
- [x] 1.3 Strictly validate this change and the corrected pending-quota change; run available project checks.

## 2. Complete acceptance evidence

- [ ] 2.1 Add missing consumer-visible acknowledgement quota coverage: freeing exactly one full-budget slot, replay after replacement, and failed registration preserving pending capacity and signed terms.
- [ ] 2.2 Retain and map existing expiry/restart, original-transaction and sender/nonce recovery tests; add only missing cases, with no fabricated payment success or live qualification claim.
- [ ] 2.3 Resolve any demonstrated implementation contradiction only where independently committable. Preserve the owner's untracked native source and mark inseparable fixes blocked.

## 3. Verify and integrate

- [ ] 3.1 Run relevant acceptance tests and available project checks, recording unavailable platform/release gates without weakening them.
- [ ] 3.2 Strictly validate and archive with `openspec archive specify-acknowledged-proposal-slot-transfer --yes`, never `--skip-specs`, only when every task is complete. Integrate its distinct ADDED requirement alongside the corrected pending-quota requirements.

First-opening slot transfer remains in pending-quota task 2.4. This correction does not approve public deployment or settle the existing author-owned availability disclosure.
