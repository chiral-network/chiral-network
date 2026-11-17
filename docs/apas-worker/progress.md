# APAS progress

## 2026-11-16 — Fix the macOS-only provisioning test failure

- Entry was clean at `685b44aa`, in step with `origin/main`; no merge/rebase in progress. Read the complete README-designated `docs/chiral-book.md` (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`) and the report. No new `Answer:` lines; both open entries (named-payer quota implementation, Verus availability) are unchanged.
- Since the previous worker turn the author's session added: the desktop GUI restyle, Appendix A (GUI text reference), close-to-taskbar and the tray (`desktop-window-lifecycle`, archived), the adopted target design (local-first desktop, libp2p transport) with eleven *Author decisions*, and five spec-only OpenSpec changes for its milestones: `specify-desktop-local-models` (M1), `specify-desktop-local-api-server` (M2), `specify-p2p-provider-transport` (M3), `specify-desktop-share-online` (M4), `specify-provider-launch-reputation` (M5). Their implementation tasks are all unchecked.
- Failing check found and fixed (implementation layer, test only): `python -m unittest discover -s scripts -p "test_*.py"` (a CI `native` job step, also on `macos-14`) failed three subtests of `test_native_generation_uses_managed_runtime_and_private_host_paths` on macOS. `provision-provider.py` deliberately records `model_dir`/`runtime_dir` as `Path.resolve(strict=True)` canonical paths; the test expected the unresolved temporary path, and macOS temporary directories sit behind `/var -> /private/var`. The expectation now uses the canonical paths; the asserted property is unchanged and the production script is untouched.

## Verification

- This Mac (macOS, Apple Silicon): Svelte check 0 errors/0 warnings, 24 frontend tests, frontend build, CI Python compilation and 46 script unit tests passed; headless Rust tests passed (`cargo test --locked --no-default-features --all-targets`: 465 passed, 0 failed, 2 existing ignored, 21 suites). The native-feature `cargo test`/`tauri:build` CI job was not rerun this turn.
- Verus: `verus --version` still fails here (command not found). Proof steps remain blocked by the open report entry **2026-11-15 — Verus is unavailable**; nothing installed.

## Next steps, in order

1. Systematic spec-versus-whitepaper comparison after the author's 2026-11-16 changes: current-runtime main specs and the older pending changes against the unchanged Part II text, and the five milestone drafts against the target-design sections, *Author decisions* 1–11 and Appendix A.7. Fix any contradiction in the specs first.
2. Missing specification for **Appendix A** required GUI behavior: A.1 global rules (native boundary, no simulation, explicit network, exact money, untrusted output, disclosure before funding, recovery over retry, session scoping, accessibility) and the required parts of A.5 pages have no OpenSpec capability; only `desktop-window-lifecycle` exists. Specify before changing GUI code; GUI steps need a screenshot check (the CDP headless-Chrome harness and native `screencapture` work on this Mac).
3. Earlier open work, unchanged: snapshot **2.5 / 3.3–3.4**; challenge-clock non-regression (**2.4–2.5**) and private time/fault acceptance; accepted-paid-intent/elapsed-expiry recovery (**non-reserving 3.3/4.3**, opening **2.3.4.3/2.8.2**); request-accounting **2.2.4, 2.3, 2.5, 3.1–3.3**; safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment.
4. Milestone implementation, in dependency order: M1 (download/library/runtime/local chat), M3 (transport and offer schema v2; after `specify-inference-commercial-terms`), M2, M5, then M4. Each is large; take a first useful slice per turn.
5. Extend the Rust-test judgment inventory in `verus.md`; proofs wait for Verus.

## Blocked

- Verus setup and proofs: open report entry (Verus unavailable here).
- Real chain, engine, GPU, relay deployment, installers and public-release gates: outside this checkout.

## Checkout discipline

This turn owns `scripts/test_qualify_provider.py` and `docs/apas-worker/progress.md`. The untracked `src-tauri/gen/schemas/macOS-schema.json` is generated build output and is never committed. The whitepaper is read-only.
