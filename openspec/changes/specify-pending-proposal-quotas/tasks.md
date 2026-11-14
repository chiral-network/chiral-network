## 1. Specify the existing policy

- [x] 1.1 Compare the whitepaper, commercial-terms delta and actual quote admission/opening/cleanup code; distinguish existing challenge tests from missing quote tests.
- [x] 1.2 Specify pending capacities, ledger scope, atomic rejection and agreement-preserving reclamation without changing authentication, rates or native quote lifetime.
- [x] 1.3 Record the public named-payer availability exposure for the whitepaper author without modifying the whitepaper.
- [x] 1.4 Strictly validate this specification and run available project checks.

## 2. Add isolated acceptance evidence

- [x] 2.1 Cover the eight-per-payer pending boundary, payer/model/free-paid attribution, independent ledgers and rejection without storing the new proposal.
- [x] 2.2 Cover the 10,000-record storage ceiling with bounded, explicitly storage-level fixtures and controlled time; do not change native TTL/rates, use live databases or fabricate payment evidence.
- [x] 2.3 Cover competing connections at the final pending slot and rollback of tentative rate increments when pending admission rejects.
- [ ] 2.4 Cover exact expiry reclamation and successful/failed first-opening slot transfer with preserved signed agreements. Review and retain existing acknowledged-funding recovery tests; add only missing consumer-visible cases.
- [ ] 2.5 Fix any demonstrated implementation contradiction only where independently committable. Entirely untracked owner source remains blocked; do not absorb it or weaken requirements.

## 3. Verify and integrate

- [ ] 3.1 Run relevant acceptance tests and available README/CI checks; record unavailable platform/release gates without weakening them.
- [ ] 3.2 Strictly validate the completed change and archive with `openspec archive specify-pending-proposal-quotas --yes`, never `--skip-specs`, only once all tasks are complete.

Exact proposal-rate policy/clock semantics remain a separate specification step. The whitepaper disclosure request is nonblocking for testing the current public-proposal contract; any authentication redesign requires the author's design update first.
