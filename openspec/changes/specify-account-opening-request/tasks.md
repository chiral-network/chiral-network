## 1. Specify the opening request boundary

- [x] 1.1 Compare the read-only whitepaper, foundation and pending owner/commercial/funding contracts with the actual opening decoder, native writer, route, payment guard and existing acceptance.
- [x] 1.2 Write the request shape, optional-payment meaning, exact-body authorization ordering and credential-preserving replay requirements, with source/test correspondence and explicit non-goals.
- [x] 1.3 Validate this change with `openspec validate specify-account-opening-request --strict --no-interactive` and record available README/CI checks without claiming implementation completion. Strict validation, all 435 Rust tests and 61 frontend/provisioning tests, headless/frontend builds, Svelte checks, Python compilation and the protected-book checksum pass. The temporary schema probe is removed; implementation tasks remain open.

## 2. Implement and prove the boundary

- [x] 2.1 Enforce object-only `OpenRequest` decoding through the shared map visitor with a strict derived field decoder. Preserve required `quote_id`, omitted/null/string transaction semantics, raw duplicate-member refusal and unchanged serialization; no positional shim, `Value` round-trip or financial migration.
- [x] 2.2 Extend production HTTP opening acceptance for complete and optional-omission positional arrays, missing/mistyped/unknown/duplicate members and valid omitted/null objects. Use proofs bound to exact raw bytes; require schema refusal before challenge consumption or account/credential/funding/fee mutation and retain useful successful controls.
- [ ] 2.3 Inventory and complete only missing free/paid payment-presence and ordering cases across first opening and replay: free supplied/empty strings refused, paid omitted/null/unregistered/changed originals refused, exact original still recoverable, and omission/null body substitutions cannot reuse authority. Observe business-refusal proof consumption and no premature chain access or financial mutation; reuse existing ownership/safe-credit coverage.
- [ ] 2.4 Complete correspondence for expired free first-opening refusal versus existing-account recovery, paid exact-original replay under unavailable readiness/capacity and credential/hold/funding conservation. Reuse existing explicit-time and reopened-provider tests; add only genuinely missing deterministic cases, without fake chain-credit or full lifecycle claims.

## 3. Verify and integrate

- [ ] 3.1 Run relevant acceptance and available project checks after implementation; preserve unrelated failures/platform blockers without weakening checks or inventing a policy key.
- [ ] 3.2 Strictly validate the complete change and archive with `openspec archive specify-account-opening-request --yes` only after every implementation/acceptance task is complete; never use `--skip-specs`.

No live provider/account operation, deployment, installation or whitepaper edit belongs to these tasks. Top-up and accounting-response schemas remain separate work; existing parent changes retain their remaining paid/expiry, resource and qualification gates.
