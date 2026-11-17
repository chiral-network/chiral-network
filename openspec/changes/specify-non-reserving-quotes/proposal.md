## Why

The author has added **Approved non-reserving quotes** to `docs/chiral-book.md` and answered the public named-payer quota report. Public proposals must not spend a named wallet's pending-quote or proposal-rate allowance. The former eight-pending/twelve-per-minute wallet gates contradicted that decision; merely disclosing the exposure or raising one limit was not the selected remedy.

## What Changes

- Replace named-wallet quote occupancy with provider-ledger-wide bounded retention of unaccepted signed terms. A quote does not reserve paid or free opening capacity.
- Preserve atomic payer-authenticated paid admission before durable acknowledgement and client broadcast, with permanent accepted-payment recovery independent of subsequent capacity or expiry.
- Make free-account capacity decisions at authenticated opening without a payment. Existing-account, top-up and settlement recovery do not acquire quote-admission gates.
- Bound public body/concurrent work, pre-signing attempts and committed quote retention/rates independently of the named payer. Keep provider-wide refusal honest: this is not global availability or Sybil resistance.
- Retarget the existing proposal-rate and opening-intent changes, migrate conflicting acceptance cases with the native implementation, and preserve historical evidence as historical evidence.

The replacement specifications and provider-only committed-admission cutover are implemented. This step adds the separate durable pre-signing work limit and its acceptance evidence; remaining paid/expiry recovery acceptance keeps the change open. On 2026-11-17 the finished proposal-admission deltas (provider-wide retention, atomic storage, the pre-signing work bound, the three restated retention requirements and the removal of the three named-wallet requirements) moved unchanged to `adopt-provider-wide-quote-retention` and were archived into `openspec/specs/inference-proposal-admission`, so the main spec no longer contradicts the whitepaper. This change now carries only *Public quotes do not reserve opening capacity* and *Authenticated paid admission preserves accepted commitments*, whose acceptance (tasks 3.3, 4.3) is still open. No isolated regression or specification completion is public-release approval.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `inference-proposal-admission`: non-reserving public quotes, wallet-independent resource controls, bounded retained terms and authenticated opening-capacity decisions.

## Impact

The production paths are public proposal issuance in `contracts.rs`, quote/admission/account transactions in `billing_store.rs`, and native funding-intent/broadcast recovery in `consumer.rs`. Proposal-rate, commercial and opening-intent contracts are aligned to the replacement decision. Necessary source incorporation was authorized and the migration is committed; source ownership no longer blocks implementation. This change does not migrate financial state, deploy a provider or edit the whitepaper. Public-release, safe-credit and performance qualification remain separate gates.
