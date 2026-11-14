## 1. Specify commercial terms

- [x] 1.1 Compare the whitepaper and existing foundation with actual schemas, offer/profile validation, provider/provisioning constraints, quote formation and consumer verification; record gaps and existing tests.
- [x] 1.2 Write the proposal, design and `inference-commercial-terms` delta without modifying the whitepaper or runtime sources.
- [x] 1.3 Run `openspec validate specify-inference-commercial-terms --strict --no-interactive`.

## 2. Complete acceptance coverage and native enforcement

- [ ] 2.1 Add missing isolated offer/model/profile schema and consistency boundary tests, retaining existing coverage and avoiding real-time edge flakiness or synthetic qualification claims.
- [ ] 2.2 Add a regression at the direct native provider configuration boundary for embedding zero output price, then enforce the whitepaper's input-only rule there without normalizing invalid rates or relying solely on provisioning. Do not commit the owner's surrounding uncommitted source; if separable ownership is unavailable, record the blocker and leave this task open.
- [ ] 2.3 Exercise the managed embedding inventory boundary and reject a selected generation chat template; retain exact artifact/digest checks and demonstrate a valid template-free embedding inventory without launching a real engine. Apply the same source-ownership restriction as task 2.2.
- [ ] 2.4 Add missing public proposal and native quote-acceptance cases: exact selected terms and requested funding, free/paid boundaries, malformed declarations, readiness refusal, and preservation of existing saved/acknowledged terms.
- [ ] 2.5 Confirm existing evidence-only renewal tests cover the complete unchanged-term requirement and retain historical funding recovery checks; add only missing consumer-visible edge coverage.

## 3. Verify and integrate

- [ ] 3.1 Run the relevant tests and available README/CI checks; fix only this change's failures and record unrelated/platform blockers without weakening checks.
- [ ] 3.2 Strictly revalidate the completed change and confirm all contract and implementation evidence.

After the checklist is complete, archive with `openspec archive specify-inference-commercial-terms --yes`, updating main specs without `--skip-specs`. Do not archive while the native enforcement task is blocked or unimplemented.
