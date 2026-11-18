## Why

The read-only whitepaper (`docs/chiral-book.md`) ends with Appendix A, a text version of the native desktop GUI that a redesign or another toolkit must be able to reproduce (book lines 797–805). Its global rules A.1.1–A.1.9 (lines 808–816) are marked *required*: they restate contracts from the book's listener and secret boundaries, configuration and state, product and trust model, settlement and recovery risks, and agreements and exact funding order sections. A.6 (lines 969–975) says that any replacement or extension must keep them. The appendix introduction (line 804) adds that the GUI is a thin view: it never computes money, signs, verifies chain evidence or decides admission.

Only part of A.1 is in OpenSpec. A.1.10 (closing is not quitting) is the archived `desktop-window-lifecycle` capability. Pending changes cover pieces of A.1.6 and A.1.7: `specify-funding-settlement` owns the reorganization statement, the opening registration risk, funding recovery and cancellation wording, and `specify-p2p-provider-transport` owns the libp2p path disclosure. `specify-desktop-local-api-server` owns the target-design local API server bearer that A.1.1 names as its second exception, and `specify-desktop-local-models` owns the A.7 scoping of A.1.3 and A.1.8. Nothing specifies the native boundary itself, the secret rules and the API-key reveal, the browser preview, explicit network selection, exact money handling, untrusted rendering, the remaining A.1.6 disclosure items, request reconciliation, session scoping or accessibility. These exist only as appendix prose and Svelte code, so a redesign could drop them without a failing requirement.

## What Changes

- Add the `desktop-gui-global-rules` capability with 10 requirements and 34 scenarios. Each requirement cites its book lines and is written so that it can be tested against a replacement GUI, not only the current Svelte pages:
  1. the GUI is a thin view over the native command boundary, shows native errors verbatim, and its rules bind every page and any replacement (book lines 797–804, 859, 873, 969–974);
  2. secrets stay native, with the explicitly revealed API key as the one current exception: masked, kept out of storage and cleared on hide, account or credential change, leaving the page, logout or profile change (A.1.1);
  3. the browser preview simulates nothing (A.1.2);
  4. network actions wait for an explicitly loaded profile (A.1.3);
  5. money crosses the native boundary as exact wei (A.1.4);
  6. untrusted content renders as plain text (A.1.5);
  7. every opening and top-up surface discloses that funds are non-refundable and usable only with that provider and endpoint, that the provider sees prompts in plaintext, that signed metadata does not prove execution, the platform fee rate and that gas is extra, and requires active acceptance (the A.1.6 items no other capability owns);
  8. ambiguous paid requests are reconciled, never resent (the request part of A.1.7);
  9. logout and a profile change end the previous session's view (A.1.8);
  10. the GUI is accessible (A.1.9).
- Reference, without restating: `desktop-window-lifecycle` for A.1.10; `inference-funding-settlement` for the A.1.6 reorganization and registration-risk statements and the funding and cancellation parts of A.1.7; `p2p-provider-transport` for the A.1.6 libp2p path disclosure; `desktop-local-api-server` for the A.1.1 local server bearer; `desktop-local-workbench`, `desktop-local-chat`, `desktop-model-acquisition` and `desktop-share-online` for the target-design pages and the A.7 scoping.
- Record a coverage table in `design.md` (requirement, book lines, implementing code, existing tests, gaps). Record each code difference as an unchecked task in section 2 and each missing acceptance test in section 3. Native desktop verification is task 4.1.

## Capabilities

### New Capabilities

- `desktop-gui-global-rules`: The global rules every desktop GUI page, added page and replacement GUI must keep: the native command boundary, secret handling and the API-key reveal, the browser preview, explicit network selection, exact money, untrusted rendering, the product-risk disclosure before funding, request reconciliation, session scoping and accessibility.

### Modified Capabilities

None. This capability references the normative `desktop-window-lifecycle` and `inference-wire-foundation` capabilities and the pending `inference-funding-settlement`, `p2p-provider-transport`, `desktop-local-api-server`, `desktop-local-workbench`, `desktop-local-chat`, `desktop-model-acquisition`, `desktop-share-online` and `inference-request-accounting` capabilities. It adds no obligation to them and does not change their requirements.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements: `src/App.svelte`, `src/lib/native.ts`, `src/lib/stores.ts`, `src/lib/components/` (including `Toast.svelte`, `ConnectionPill.svelte`, `Navbar.svelte`, `Sidebar.svelte`, `Result.svelte` and `ui/Callout.svelte`), every page in `src/pages/`, `src/app.css`, `src-tauri/capabilities/default.json`, `src-tauri/tauri.conf.json` (CSP), `src-tauri/src/desktop.rs` and the public results of `src-tauri/src/consumer.rs`.
- Twelve code differences were found. Tasks 2.1–2.12 track them:
  - the Usage top-up card omits the endpoint, the plaintext-prompts statement, the signed-metadata statement and the platform fee rate, and the native account data it receives carries neither the agreed fee rate nor the endpoint;
  - Usage, Playground and Diagnostics enable, and Usage and Playground automatically start, native operations that need a profile before one is loaded;
  - the Marketplace and the Usage top-up form still offer a new payment while a journaled payment of another agreement is unresolved (the rule itself is `inference-funding-settlement` requirement *Funding ambiguity is disclosed and recovered on native surfaces*; native code already refuses the second payment);
  - the feedback score input has no associated label;
  - every notification uses `role="alert"`, and several status messages have no `role="status"`;
  - notifications slide and the Marketplace scrolls smoothly even when reduced motion is requested;
  - the stream-cancel helper does not refuse with an explanation outside the native runtime (not reachable from the current pages);
  - `index.html` preconnects to and requests a stylesheet from Google Fonts, which the CSP blocks;
  - CFX display shows non-canonical wei strings such as `"00"` and out-of-range values as numbers;
  - the Playground labels a stream that failed before any request ID as a stream error, with no status action and no pointer to Usage;
  - cancelling a request by its ID on the Usage page is not presented as possibly billed;
  - the Marketplace replaces a failed funding-journal read's native error with a fixed phrase.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.10, one per requirement. No requirement is fully proven by existing tests; the passing frontend and focused Rust tests cited in `design.md` prove parts of requirements 1, 2, 3, 5, 6, 7 and 9.
- No question for the whitepaper author was found, so no report entry is added.
- No code, test, whitepaper, deployment or native verification change is part of this specification step.
