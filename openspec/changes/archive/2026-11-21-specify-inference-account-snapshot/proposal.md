## Why

The book requires immutable provider-specific agreements, exact cumulative account accounting and attributable receipts. The wire foundation defines money/network/signature primitives and the pending request-accounting contract defines transitions, but neither specifies the complete `AccountView` payload shared by opening, account reads, top-up responses and signed receipts.

The inspected producer emits a 17-member account object with a two-counter `TokenUsage` object. The current account decoder is derived without `deny_unknown_fields`; both types lack the existing explicit object-only visitor. Define this missing boundary before changing decoder behavior, without confusing a typed provider statement with independently verified funding or execution.

## What Changes

- Specify the existing account members, their exact types and accounting meanings; require a closed named-field object rather than positional aliases or silently discarded statement members.
- Specify the reusable accounting usage object with two exact unsigned 64-bit counters. Preserve the separate OpenAI response usage schema, which may include `total_tokens`.
- Preserve original agreement identity, model/profile, price and concurrency; newer per-request qualification evidence does not replace the account's original model.
- Preserve wide-integer balance validation, non-refundable funding, current reversal/loss semantics, known usage under waivers and the distinction between account loss compensation and request exposure.
- Preserve all existing lifecycle states, including `settlement_pending`, and readable unfamiliar status strings without treating them as active authority or engine readiness.
- Require malformed-response refusal before installing a native account snapshot, credential or receipt watermark, while preserving original payment recovery and remote-outcome ambiguity.

## Capabilities

### New Capabilities

- `inference-account-snapshot`: the shared account payload, accounting usage primitive and context-bound receiver contract.

### Modified Capabilities

None. Existing wire, commercial, owner-control, opening, top-up and request-accounting requirements remain in force.

## Impact

- Planned implementation: reuse `protocol::deserialize_object` for `AccountView` and `TokenUsage`, retaining serialization and stored schema-3 field representation. Keep account acceptance centralized around immutable-term and wide-balance checks.
- Acceptance: raw and pre-parsed JSON, genuine provider output, native opening/read/top-up/receipt boundaries and protected-state recovery. Preserve exact integers at client/export boundaries; do not narrow the wire range to a JavaScript safe-integer subset.
- This turn is specification-only. Full receipt/request/performance/credential envelope schemas, funding/reorg transition policy, hardware qualification, deployments and whitepaper changes are outside scope. The change stays open until its implementation and acceptance tasks pass.
