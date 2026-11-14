## 1. Specify commercial terms

- [x] 1.1 Compare the whitepaper and existing foundation with actual schemas, offer/profile validation, provider/provisioning constraints, quote formation and consumer verification; record gaps and existing tests.
- [x] 1.2 Write the proposal, design and `inference-commercial-terms` delta without modifying the whitepaper or runtime sources.
- [x] 1.3 Run `openspec validate specify-inference-commercial-terms --strict --no-interactive`.

## 2. Complete acceptance coverage and native enforcement

- [ ] 2.1 Add missing isolated offer/model/profile schema and consistency boundary tests, retaining existing coverage and avoiding real-time edge flakiness or synthetic qualification claims.
  - [x] 2.1.1 Cover strict offer/model/profile schemas and pure model/profile validation with standalone deterministic integration tests.
  - [x] 2.1.2 Cover pure advertised-origin validation: public HTTPS, explicit loopback development, origin-only syntax, and nonpublic IP-literal/legacy IPv4-spelling rejection without DNS or network access.
  - [ ] 2.1.3 Complete remaining signed offer-receiver admission cases, especially invalid metadata and exact time/size/concurrency boundaries, without flaky wall-clock assumptions or absorbing uncommitted owner source.
    - [x] 2.1.3.1 Cover signed invalid metadata rejection before proposal, inclusive deployment/revision/concurrency boundaries, the complete signed-envelope size bound and the fixed maximum lifetime through the public native consumer.
    - [ ] 2.1.3.2 Complete remaining offer admission cases, especially exact now-relative issuance/expiry/qualification boundaries, using a safe explicit-time seam rather than near-now assertions or sleeps.
  - [ ] 2.1.4 Reject absolute DNS localhost aliases (`localhost.` and `*.localhost.`) in public-origin validation and add a regression. A native probe confirmed acceptance; the source fix is blocked while `offer_registry.rs` remains untracked owner work. Do not claim a DNS/transport bypass or broaden the development exception.
- [ ] 2.2 Add a regression at the direct native provider configuration boundary for embedding zero output price, then enforce the whitepaper's input-only rule there without normalizing invalid rates or relying solely on provisioning. Do not commit the owner's surrounding uncommitted source; if separable ownership is unavailable, record the blocker and leave this task open.
- [ ] 2.3 Exercise the managed embedding inventory boundary and reject a selected generation chat template; retain exact artifact/digest checks and demonstrate a valid template-free embedding inventory without launching a real engine. Apply the same source-ownership restriction as task 2.2.
- [ ] 2.4 Add missing public proposal and native quote-acceptance cases: exact selected terms and requested funding, free/paid boundaries, malformed declarations, readiness refusal, and preservation of existing saved/acknowledged terms.
  - [x] 2.4.1 Exercise fresh quote acceptance through the public native consumer API: complete signed terms, requested funding, free/paid boundaries, signer/domain checks, malformed responses, and evidence-only renewal versus exact quote copying.
  - [ ] 2.4.2 Complete actual provider proposal/readiness and remaining saved-agreement/historical recovery coverage. An isolated response fixture does not verify the production issuer or settlement lifecycle.
- [ ] 2.5 Confirm existing evidence-only renewal tests cover the complete unchanged-term requirement and retain historical funding recovery checks; add only missing consumer-visible edge coverage.

## 3. Verify and integrate

- [ ] 3.1 Run the relevant tests and available README/CI checks; fix only this change's failures and record unrelated/platform blockers without weakening checks.
- [ ] 3.2 Strictly revalidate the completed change and confirm all contract and implementation evidence.

After the checklist is complete, archive with `openspec archive specify-inference-commercial-terms --yes`, updating main specs without `--skip-specs`. Do not archive while the native enforcement task is blocked or unimplemented.
