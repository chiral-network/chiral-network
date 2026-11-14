## Why

The author has added **Approved non-reserving quotes** to `docs/chiral-book.md` and answered the public named-payer quota report. Public proposals must not spend a named wallet's pending-quote or proposal-rate allowance. The current eight-pending/twelve-per-minute wallet gates contradict that decision; merely disclosing the exposure or raising one limit is not the selected remedy.

## What Changes

- Replace named-wallet quote occupancy with provider-ledger-wide bounded retention of unaccepted signed terms. A quote does not reserve paid or free opening capacity.
- Preserve atomic payer-authenticated paid admission before durable acknowledgement and client broadcast, with permanent accepted-payment recovery independent of subsequent capacity or expiry.
- Make free-account capacity decisions at authenticated opening without a payment. Existing-account, top-up and settlement recovery do not acquire quote-admission gates.
- Bound public body/concurrent work, pre-signing attempts and committed quote retention/rates independently of the named payer. Keep provider-wide refusal honest: this is not global availability or Sybil resistance.
- Retarget the existing proposal-rate and opening-intent changes, migrate conflicting acceptance cases with the native implementation, and preserve historical evidence as historical evidence.

This turn defines the replacement contract and implementation sequence only. It does not remove the live wallet gates, complete downstream specification alignment, or claim mitigation. The change remains open until implementation and acceptance tasks pass.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `inference-proposal-admission`: non-reserving public quotes, wallet-independent resource controls, bounded retained terms and authenticated opening-capacity decisions.

## Impact

The production paths are public proposal issuance in `contracts.rs`, quote/admission/account transactions in `billing_store.rs`, and native funding-intent/broadcast recovery in `consumer.rs`. Existing proposal-rate, commercial-terms and opening-funding-intent changes and proposal/admission tests need coordinated alignment. The native files are currently owner-untracked; this change does not adopt them, migrate financial state, deploy a provider or edit the whitepaper. Public-release, safe-credit and performance qualification remain separate gates.
