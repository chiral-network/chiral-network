## Why

The read-only whitepaper (`docs/chiral-book.md`) describes each desktop page in Appendix A.5 (book lines 871–967): Marketplace, Playground, Usage & API keys, Wallet, Provider, Account, Network, Settings and Diagnostics. Each page lists its purpose, layout, native operations and required states (line 873). The appendix introduction says that required statements restate contracts from elsewhere in the book, while layout, wording, color and iconography are current presentation (lines 799–802). A.6 adds that a replacement GUI must keep every required disclosure, acceptance checkbox and recovery path in A.5: a different layout is fine, a missing warning or an extra unconfirmed payment path is not (line 971).

The global rules A.1.1–A.1.9 are specified by the pending `specify-desktop-gui-global-rules` change, and A.1.10 by the normative `desktop-window-lifecycle` capability. Pending changes cover the funding disclosures and recovery (`inference-funding-settlement`), the operator fee view (`inference-fee-remittance`), serving-profile semantics (`inference-generation-performance`, `inference-commercial-terms`) and the target-design pages of A.7. Nothing specifies the pages themselves: which native operation each action invokes, which states each page must show, where each disclosure, acceptance and recovery path sits, and how each page handles key files, token files and the revealed API key. A redesign could drop any of these without a failing requirement.

## What Changes

- Add the `desktop-gui-pages` capability with 15 requirements and 45 scenarios. Each requirement cites its book lines and can be tested against a replacement GUI, not only the current Svelte pages:
  1. each page action invokes its documented native operation, and opening a page only reads (the mapping table for A.5.1–A.5.9);
  2. the Marketplace discovers, inspects and funds in one gated order (A.5.1);
  3. offer and serving terms are shown wherever a page offers funding or use (A.4 as A.5.1–A.5.3 use it);
  4. the Playground uses a funded account within its advertised terms (A.5.2);
  5. Usage shows each account and tops it up only through the disclosed form (A.5.3 overview);
  6. API credentials are managed as metadata, with a 60-second to 30-day lifetime, and revealed only on request, in a form the user can copy for SDK use (A.5.3 API keys);
  7. Usage reconciles requests and recovers journaled funding, with or without a selected account (A.5.3 requests and recovery);
  8. feedback is payment-backed and revisable (A.5.3 feedback);
  9. the Wallet page manages keys only through native file paths (A.5.4);
  10. the Provider page administers a loopback operator listener through a token file (A.5.5);
  11. the Account page lists provider accounts without combining them (A.5.6);
  12. the Network page reports the settlement frontier of the configured chain (A.5.7);
  13. Settings loads a profile by path and states, as text, whether one is loaded (A.5.8);
  14. Diagnostics reports runtime state without secrets (A.5.9);
  15. presentation may change, but every required element stays present and reachable in every layout and at phone widths (A.2, A.3, A.6).
- Reference, without restating: `desktop-gui-global-rules` for every A.1 rule; `desktop-window-lifecycle` for A.1.10 and Quit; `inference-funding-settlement` for the funding disclosures, the pending-funding and recovery behavior and cancellation; `inference-fee-remittance` for the native operator rules and the fee view; `inference-generation-performance` and `inference-commercial-terms` for serving-profile semantics; `inference-account-snapshot` and `inference-request-accounting` for account and receipt wording; `p2p-provider-transport`, `provider-launch-reputation`, `desktop-share-online`, `desktop-local-workbench` and `desktop-local-chat` for the target design.
- Record a coverage table in `design.md` (requirement, book lines, implementing code, existing tests, differences). Record each code difference as an unchecked task in section 2 and each missing acceptance test in section 3. Native desktop verification is task 4.1.

## Capabilities

### New Capabilities

- `desktop-gui-pages`: The required behavior of each desktop page: the native operation behind every action, the states each page shows, the placement of each disclosure, acceptance control and recovery path, the page-level handling of key files, token files and the revealed API key, and the rule that presentation may change while required elements stay reachable.

### Modified Capabilities

None. This capability references the normative `desktop-window-lifecycle` capability and the pending `desktop-gui-global-rules`, `inference-funding-settlement`, `inference-fee-remittance`, `inference-generation-performance`, `inference-commercial-terms`, `inference-account-snapshot`, `inference-request-accounting`, `p2p-provider-transport`, `provider-launch-reputation`, `desktop-share-online`, `desktop-local-workbench` and `desktop-local-chat` capabilities. It adds no obligation to them and does not change their requirements.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements: every page in `src/pages/`, `src/App.svelte` (routing and remount), `src/lib/native.ts`, `src/lib/funding.ts`, `src/lib/stores.ts`, `src/lib/toastStore.ts`, `src/lib/components/ServingProfile.svelte`, `Result.svelte` and `ui/`, `src-tauri/src/desktop.rs` (operation dispatch) and the public results of `src-tauri/src/consumer.rs`.
- Four code differences were found. Tasks 2.1–2.4 track them:
  - the credential lifetime field declares 60 seconds to 30 days but is not enforced, so 5 seconds or 60 days reaches `credential_issue` (the provider accepts up to 90 days);
  - the revealed API key cannot be copied: it sits in a password field, from which Chromium does not copy, and the page offers no copy action;
  - the Settings profile indicator is a colored dot whose state is not in its accessible name;
  - the Settings success message after a profile load is discarded by the remount, so a successful load is never announced.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.15, one per requirement. No requirement is fully proven by existing tests; the passing frontend tests and the three focused Rust tests cited in `design.md` prove parts of requirements 1, 2, 4, 5, 6, 7, 8, 9, 13 and 14.
- The known observation that the Usage tab strip made the page 415 pixels wide at 390 pixels was not reproduced in Chromium, and every Usage section stayed reachable. It is recorded as a presentation observation, with a WebKit check in task 4.1.
- No question for the whitepaper author was found, so no report entry is added.
- No code, test, whitepaper, deployment or native verification change is part of this specification step.
