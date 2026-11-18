# APAS progress

## 2026-11-18 (thirty-fifth round) — Comparison pass: 19 confirmed findings recorded

- In step at `a5aa2b88`; whitepaper unchanged; no new answers.
- With no known gap left, a new comparison ran over the whole book outside Appendix A and the target-design milestones: five slices, each with one finder and one adversarial verifier. 20 findings, 19 confirmed, recorded in `docs/apas-worker/comparison-2026-11-18.md` in priority order. They comprise one spec-versus-whitepaper wording item, six code differences (one high: consumer profile paths resolve against the working directory) and ten missing specifications.
- This round's step (highest priority, spec contradicts whitepaper): book line 483 says the opening transaction carries "intrinsic gas", while the spec and code correctly use the eSpace estimate within the structural minimum and 1,000,000. The spec is right and the whitepaper is read-only, so a report entry proposes new book wording. No spec or code change.

## 2026-11-18 (thirty-fourth round) — Code: no update status from another network survives a logout or profile change

- In step at `e63b9737`; whitepaper unchanged; no new answers.
- Code step (`specify-version-policy` task 2.4, requirement 13; book A.1.8 and A.2). Logout left the process-wide effective policy in place. `get_version_status` without a profile kept reporting the previous network's status, and the webview kept its last status after a failed refresh, so a banner from the previous network stayed visible.
  - `version::reset_effective_policy` and a desktop `end_session` now clear the effective policy on logout.
  - The webview's `resetVersionStatus()` runs on logout, on quit, and before a profile load's refresh.
  - Regressions fail without the change: a desktop-feature Rust test and two rendered webview tests. Supporting tests cover the reset itself. Screenshot checked.
- Every code difference in `specify-version-policy` is now fixed; its author questions, acceptance tasks 3.x and release evidence 4.x remain.

## 2026-11-18 (thirty-third round) — Code: the required-update modal blocks the keyboard, and only HTTPS download URLs are offered

- In step at `fe9b6098`; whitepaper unchanged; no new answers.
- Code step (`specify-version-policy` tasks 2.2 and 2.3, requirement 13; book A.2 line 846).
  - The required-update modal blocked the pointer only. The navigation, page and Log out stayed reachable by keyboard and assistive technology. One `shellBlocked` flag in `App.svelte` now makes the skip link, navigation and `main` inert, and the modal takes focus and keeps Tab inside it. The tray's Quit dialog stays usable in the top layer.
  - *Copy download URL* and the banner's *Update* were offered for any URL. They now appear only for an HTTPS URL without credentials; otherwise the gate says the policy gives no HTTPS download URL.
  - New `tests/updateGate.test.ts`: four tests fail on the previous components, and two guard the other cases. Headless Chrome confirmed the inert shell and that the navbar still sticks; screenshots checked.
- Checks: svelte-check 0/0, 67 frontend tests, build. Rust is unchanged since the full headless suite passed at `fe9b6098` (493 passed, 2 ignored).

## 2026-11-18 (thirty-second round) — Code: the policy authority's key stays in a protected file

- In step at `7dcb5887`; whitepaper unchanged; no new answers.
- Code step (`specify-version-policy` task 2.1, requirement 8; book lines 474 and 648). `chiral-policy-sign sign` took the authority's private key as `--key <hex>` or from `CHIRAL_POLICY_SECRET`, and `keygen` printed `secret=<hex>`. Now `keygen --out <path>` writes the key to a new owner-only file (an existing path is refused) and prints only the public key as JSON. `sign --key-file <path>` reads it through `wallet::open_secret_file`, which refuses symlinks and group- or world-accessible files. A new process-level test, `policy_sign_cli_test.rs`, fails on the previous binary. The only documentation reference (`docs/provider-qualification.md`) uses `verify`, which is unchanged.

## 2026-11-18 (thirty-first round) — Client version policy specified

- In step at `df2240bf`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-version-policy` (capability `client-version-policy`, 14 requirements, 47 scenarios) for the book's version policy (lines 646–652) and the A.2 update gate (846). It covers:
  - a non-placeholder release authority and a once-per-process override;
  - authentic, current policies and the required, recommended and ok statuses;
  - the durable, protected rollback watermark scoped to authority and network, kept through expiry, with fail-closed caches;
  - the placeholder authority for development only, and an offline signing key;
  - configured policy sources and the provider's client-version header check;
  - what a required update refuses (fund, top-up and new inference at minimum), provider self-health, the desktop gate, and preservation across upgrades.
  The cited tests pass (34 `version::`, 2 `network::`, 61 frontend). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1: `chiral-policy-sign` takes the authority's private key on argv or from the environment and prints it on keygen;
  - 2.2: the required-update modal does not trap focus or make the shell inert;
  - 2.3: *Copy download URL* is offered for non-HTTPS URLs;
  - 2.4: the previous network's update status stays visible after logout or a failed check.
- Report: which operations stay available under a required update, including a corrupt policy cache (default: recovery only), and whether an unreachable `policy_url` fails closed (default: yes, as today).

## 2026-11-18 (thirtieth round) — Code: the Settings profile state is stated in text and a load is announced

- In step at `b5a6a5f4`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-pages` tasks 2.3 and 2.4, requirement 13).
  - The Settings *Network profile* entry showed the profile state only as a coloured dot with a tooltip. It now carries visually hidden text.
  - A successful profile load set a page-local notice that the session remount discarded, so it was never shown. The confirmation is now an app-level success notification (`role="status"`) that survives the remount.
  - New `tests/settings.test.ts`, including the first test that mounts the real `App` shell (Tauri window and event modules stubbed): both tests fail on the previous page. Screenshot checked.
- Every code difference in `specify-desktop-gui-pages` is now fixed; its acceptance tasks 3.x and native verification 4.1 remain.
- Checks: svelte-check 0/0, 61 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-ninth round) — Code: credential lifetime enforced and the revealed key copyable while masked

- In step at `128b3db6`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-pages` tasks 2.1 and 2.2, requirement 6, book lines 808 and 930).
  - The lifetime field's bounds were never checked, because the buttons sat outside a form, so 5 s or 60-day credentials reached `credential_issue`. Issue and rotate now refuse anything but a whole number of seconds from 60 to 2,592,000 before any native call.
  - The revealed key sat in a password field that Chromium webviews will not copy. A *Copy key* action now copies it while it stays masked, and reports success or failure in live regions.
  - New `tests/credentials.test.ts`: three tests fail on the previous page, and one guards the default options. Screenshot checked.
- Checks: svelte-check 0/0, 59 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-eighth round) — Desktop GUI page behaviour specified

- In step at `a7d36fa4`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-desktop-gui-pages` (capability `desktop-gui-pages`, 15 requirements, 45 scenarios) for Appendix A.5's required page behaviour. It covers the native operation behind each page action; Marketplace order and gating; offer and serving terms before a quote; Playground limits; Usage accounts, top-up, credentials, request reconciliation, funding recovery and feedback; Wallet file-path key handling with an owner-only backup; Provider loopback administration through a token file; Account, Network, Settings and Diagnostics; and a rule that presentation may change while required elements stay reachable at every width. A.1 rules, funding-settlement, fee-remittance and performance requirements are cited by name. Cited tests pass (55 frontend, 3 Rust). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1: the credential lifetime (60 s–30 days) is not enforced before `credential_issue`;
  - 2.2: the masked API key cannot be copied in Chromium webviews;
  - 2.3: the Settings profile indicator is colour-only;
  - 2.4: a successful profile load is never announced, because the remount discards the notice.
- The review could not reproduce the 390 px Usage overflow in headless Chrome: the tab list scrolls inside its own container. It is recorded as an observation for native task 4.1. No author question.

## 2026-11-18 (twenty-seventh round) — Code: the webview loads nothing from a remote origin

- In step at `cbd3b273`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` task 2.8, requirement 1). `index.html` used to preconnect to, and load a stylesheet from, Google Fonts; the production CSP already blocked the stylesheet. The links are removed. Roboto stays first in the local font stack, which matches the native app's existing rendering; it is not bundled, because this loop may not download a font.
  - With the system font, the browser preview showed that the top bar overflowed by 10 px at exactly 1440 px, as the native app already did. The navbar container now widens to A.3's 1,400 px page frame from 1440 px; there is no overflow at 1280, 1440 or 1536 px.
  - New `tests/noRemoteResources.test.ts` fails on the previous `index.html`. The preview's resource timing shows no request to another origin.
- Every code difference in `specify-desktop-gui-global-rules` is now fixed; its acceptance tasks 3.x and native verification 4.1 remain.
- Checks: svelte-check 0/0, 55 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-sixth round) — Code: accessibility labels, live-region roles and reduced motion

- In step at `a114e665`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` tasks 2.4–2.6, requirement 10, A.1.9).
  - The Usage feedback score field is now labelled.
  - Notifications use `alert` only for errors and warnings, and `status` otherwise. The no-network banner, the Usage funding notices and the Wallet copy confirmation are status messages.
  - A shared `src/lib/motion.ts` combines the system and in-app reduced-motion preferences. The toast transition and the Marketplace scroll respect it; CSS could not reach these script-driven motions.
  - New `tests/accessibility.test.ts`: five behaviour tests fail on the previous components, including one that visits every Usage tab and checks that every field is labelled. A screenshot confirms the feedback form layout is unchanged.
- Checks: svelte-check 0/0, 52 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-fifth round) — Code: canonical wei display and an honest stream-cancel boundary

- In step at `c2392ec7`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` tasks 2.9 and 2.7, requirements 5 and 3), done together because both are `src/lib/native.ts` helpers.
  - `formatCfx` used to display any digit string ("00", "01", 2^128) as an amount. A new `isCanonicalWei` accepts only canonical wei up to 2^128 − 1, and `formatCfx` and the Marketplace lowest-price filter use it.
  - `cancelStream` threw a raw TypeError outside the native runtime; it now refuses with an explanation, like `command` and `stream`.
  - The two extended `tests/native.test.ts` cases fail on the previous helpers.
- Checks: svelte-check 0/0, 46 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-fourth round) — Code: request failures point to recovery and cancellation is labelled possibly billed

- In step at `77914a4b`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` tasks 2.10 and 2.11, requirement 8, book lines 355, 913 and 931).
  - A Playground stream that failed before a request ID was labelled "inspect request status", but there was no request ID to inspect. It is now "Failed — inspect Usage before retrying", with the native error and a note linking to Usage › Requests, where the local request keys are listed. A failure after a request ID keeps *Reconcile*. Nothing is resent.
  - Usage's *Cancel provider request* now states that cancelling does not make generated work free, that the request may still be billed, that cancellation is not a refund, and that *Read status* shows the final accounting.
  - New rendered tests in `tests/requestRecovery.test.ts`: two fail on the previous pages, and one guards the unchanged *Reconcile* path. Screenshots were checked.
- Checks: svelte-check 0/0, 45 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-third round) — Code: profile-dependent actions wait for a loaded profile

- In step at `2e346ae4`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` task 2.2, requirement 4, A.1.3). With no profile loaded, Usage and Playground used to read the native journal on open, and Usage reported "No accounts in this network's local journal". Usage's Refresh and recovery actions, Playground's Refresh accounts and Diagnostics' Collect were enabled; native code refused them. They now read nothing until a profile is loaded, the actions are disabled, and Usage points to Settings. The Diagnostics runtime tile stays. A new rendered test, `tests/profileGate.test.ts`, fails on the previous pages. Screenshots were checked.
- Checks: svelte-check 0/0, 42 frontend tests, build. Rust is unchanged since the full headless suite passed at `2e346ae4` (491 passed, 2 ignored).

## 2026-11-18 (twenty-second round) — Code: the Usage top-up card shows every A.1.6 item

- In step at `c365d384`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` task 2.1, requirement 7). The top-up card used to omit the endpoint, plaintext prompts, the statement that signed metadata does not prove execution, and the fee rate. The webview's account data cannot carry the endpoint or rate, and `AccountView` mirrors the provider wire format. A new read-only native operation `agreement` returns only public terms from the saved signed offer and quote. The card now states every A.1.6 item, with the fee rate exact ("0.5% platform fee (50 basis points)"), and no top-up is offered until those terms are read. The acceptance covers them and resets on account change. A Rust test and two rendered regressions fail without the change. The tests also caught Svelte trimming spaces at `{#if}` boundaries, which is fixed. Screenshots were checked at 1440 px and 390 px.
- Observation, not caused by this change: at 390 px the Usage tab strip makes the page 415 px wide; it does so on the previous page too. This is A.2/A.3 presentation, to consider with the A.5 specification.

## 2026-11-18 (twenty-first round) — Code: no new payment is offered while a journaled payment is unresolved

- In step at `7be73013`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` tasks 2.3 and 2.12; A.1.7 and requirement 1). The Marketplace used to read the funding journal only after its own fund or resume. After a reopen or restart it offered a new quote and payment beside an unresolved one, and a failed journal read replaced the native error with a fixed phrase.
  - The Marketplace now reads `pending_accounts` whenever a profile is loaded. While any paid operation is unresolved, or the journal cannot be read, it lists each one with its stage, transaction hash and *Resume*, and keeps the quote, acceptance and fund controls disabled.
  - Usage disables every account's top-up while another agreement's payment is unresolved and points to it.
  - The new `src/lib/funding.ts` defines unresolved: any stage except `not_prepared`, `cancelled`, `original_failed`, `cancellation_failed` and `free_open_pending`. A saved agreement stays listed until it opens, so terminal or unpaid entries must not block forever.
  - A failed journal read keeps the native text.
  - Three rendered regressions fail on the previous pages, and a fourth test guards against over-blocking. Screenshots were checked at 1440 px and 390 px.
- Checks: svelte-check 0/0, 36 frontend tests, build, 46 script tests. Rust is unchanged since the full headless suite passed at `221212a7` (490 passed, 2 ignored).

## 2026-11-18 (twentieth round) — Desktop GUI global rules specified

- In step at `221212a7`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-desktop-gui-global-rules` (capability `desktop-gui-global-rules`, 10 requirements, 34 scenarios) for Appendix A.1.1–A.1.9: a thin view over the native boundary, secrets staying native except an explicitly revealed key, no simulation, network actions only after an explicit profile load, exact wei, untrusted content as plain text, the remaining A.1.6 disclosures on every opening and top-up, reconcile instead of resend, session scoping, and accessibility. A.1.10 is cited from `desktop-window-lifecycle`, and the reorganization, registration-risk and path disclosures from the funding-settlement and M3 requirements. The cited frontend tests pass (32). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks 2.1–2.12. The main ones:
  - 2.3: the Marketplace offers a new quote and payment while another journaled payment is unresolved after a reopen or restart;
  - 2.1: the Usage top-up card lacks the endpoint, plaintext-prompt, metadata and fee-rate statements, which needs native data;
  - 2.2: Usage, Playground and Diagnostics act before a profile is loaded;
  - 2.10–2.11: request-recovery guidance and the may-still-be-billed label are missing;
  - 2.9: `formatCfx` accepts non-canonical wei;
  - 2.4–2.6: accessibility gaps (an unlabelled input, alert and status roles, motion driven by script);
  - 2.8: a remote Google Fonts stylesheet.
  GUI fixes need screenshots.

## 2026-11-18 (nineteenth round) — Code: the operator fee view shows the book's states, actual remittances and the receipt disclaimer

- In step at `92ec946a`; whitepaper unchanged; no new answers.
- Code step (`specify-fee-remittance` tasks 2.1 and 2.2, requirements 3, 11 and 12). The fee view used to total obligations by stored status, so a broadcast remittance awaiting safe showed as `batched` and the book's `submitted` and `confirmed` never appeared. No route listed actual remittances. `fee_summary` now derives each obligation's state from its own and its batch's status (design decision 4). `amounts_wei` reports pending, batched, submitted and confirmed; cancelled and orphaned amounts are reported separately. The view lists the newest 100 batches, with transaction hash and inclusion evidence and never signed bytes, read in the same snapshot as the totals. `platform_receipt` states that a recorded obligation is not proof the platform received the fee. Stored statuses are unchanged, and a combination the ledger never produces fails closed. Three regressions fail without the change, two existing assertions moved to the reported names and became stricter, and the review added a 100-batch-limit test.
- `amounts_wei.remitted` is now `amounts_wei.confirmed`. Every reader in the repository (`provider-admin.py`, the CLI and desktop `provider_admin`, the Provider page's raw-JSON panel) prints the view unchanged, so no frontend source changed.
- Every `specify-fee-remittance` code difference is now fixed; its acceptance tasks 3.x and live evidence 4.x remain.

## 2026-11-17 (eighteenth round) — Code: every confirmed remittance is rechecked, and aggregation sees every obligation

- In step at `421e103e`; whitepaper unchanged; no new answers.
- Code step (`specify-fee-remittance` tasks 2.4 and 2.3, requirements 4, 5, 6 and 9), done together because both are bounded fee queries in `billing_store.rs` that skipped rows.
  - 2.4: confirmed batches used to be rechecked on a page chosen from `now / 30`. With more than 100 batches and passes 60 s apart, some were never rechecked, so their reorg went undetected. Rechecks are now least-recently checked first, using a `fee-batch-checked:<id>` time in the ledger's `metadata` table (no schema change, sealed records untouched). `reconcile_fees` advances it after every outcome, including an outage or a ledger fault.
  - 2.3: aggregation used to read only the 1,000 oldest pending obligations across all wallets. It now totals every pending obligation of each wallet with checked arithmetic, and seals a qualifying (or forced) wallet completely in 1,000-row chunks within one transaction, splitting only at u128 overflow.
  - Five regressions, plus an ordering test the review added, fail without the change. The review measured the sealing transaction at 37 ms for 10,000 pending obligations and 0.88 s for 100,000 in a release build. The thresholds bound the pending count in practice.

## 2026-11-17 (seventeenth round) — Fee remittance specified

- In step at `5d97430f`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-fee-remittance` (capability `inference-fee-remittance`, 12 requirements, 44 scenarios) for the book's provider-remitted platform fee (line 528 and related lines). It covers:
  - a funding charge once per accepted payment, never per request, with no obligation for free openings;
  - a durable obligation per fee to the agreement's signed `platform_wallet`;
  - forward-only states pending → batched → submitted → confirmed;
  - inclusive automatic aggregation at 10^15 wei or 86,400 s, per platform wallet;
  - immutable sealed batches;
  - a loopback-only `fees/remit` operator action;
  - one transfer per batch through the shared nonce journal, with the provider paying gas;
  - safe-frontier confirmation, with recheck and reorg handling;
  - restart recovery with no fee bump;
  - an operator view that separates obligations from remittances, with the not-trustless and not-proof-of-receipt statements.
  Orphan reversal and restoration are cited from `specify-funding-settlement`. The cited tests were re-run and pass (21 library, 1 integration, 46 script tests). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1: the operator view never shows `submitted` or `confirmed`;
  - 2.2: no route shows batches, transaction hashes or the not-proof-of-receipt statement;
  - 2.3: aggregation reads only the 1,000 oldest pending obligations across all wallets;
  - 2.4: confirmed-batch rechecks page by clock, so with more than 100 batches and passes 60 s apart, some batches are never rechecked and their reorg goes undetected.
- Report: what evidence makes a remittance confirmed (default: the funding safe-frontier policy), and how a failed or displaced remittance is resolved (default: no automatic replacement, reported every pass).

## 2026-11-17 (sixteenth round) — Code: zero TTFT and out-of-boundary qualification samples

- In step at `904cc43e`; whitepaper unchanged; no new answers.
- Code step (`specify-generation-performance` tasks 2.2 and 2.3, requirements 4, 8, 9 and 11), done together because both are evidence predicates in `performance.rs`.
  - A 0 µs TTFT used to count as attained, in native validation, in the runner and at runtime (book line 558: nonpositive timing is not evidence). It is now no evidence of attainment: a qualification sample with it does not pass, and at runtime it counts as a missing first token.
  - `validate_evidence` used to count samples outside every prompt boundary (book line 566). It now rejects any sample outside 120–128, 504–512, 1,016–1,024 and 2,040–2,048.
  - Three Rust regressions and one runner case fail without the change.
- Checks: full headless suite 481 passed, 2 ignored; svelte-check 0/0, 32 frontend tests, 46 script tests, py_compile.

## 2026-11-17 (fifteenth round) — Code: client disconnects no longer count as provider misses

- In step at `505fb9ed`; whitepaper unchanged; no new answers.
- Code step (`specify-generation-performance` task 2.1, requirements 11 and 12). Before this change, a client that disconnected during `engine.dispatch`, or during non-streaming generation, dropped the HTTP handler. The request then settled as `interrupted`, which was waived and counted as a provider miss, so three cheap disconnects could latch a provider's withdrawal (book line 571). After reservation, the gateway now runs dispatch and collection in a task that owns the `Settlement` and answers the handler through a oneshot channel. A closed reply settles `disconnected` (`client_interrupted`, not counted). An already measured miss stays a waived, counted miss. Only interruptions the gateway cannot attribute (a panic or runtime shutdown) remain `interrupted`, now classified `provider_incomplete`. Three real-socket gateway regressions and the updated `outcome` test fail without the change.
- The adversarial review found no defect. Its probes covered cancellation, a streaming disconnect after headers, deadlines in both phases, an engine crash and runtime shutdown, and the public HTTP responses were byte-identical to HEAD apart from ids and timings. It noted that a provider stall after a timely first token, followed by the client leaving, is now `client_interrupted`. That follows the book: a decode miss needs authoritative usage.

## 2026-11-17 (fourteenth round) — Generation performance specified

- In step at `8ea5a5a4`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-generation-performance` (capability `inference-generation-performance`, 12 requirements) for the book's *Performance accounting and withdrawal* contract. It covers profile-bound speed classes, `warm-generation-v1`, the runtime guarantee envelope (requested output, prepared prompt), TTFT and decode in integer microseconds, the private token clock, overload refusal, evidence v2, the qualification sweep and joint attainment, memory counters, per-request classification for waivers, and the durable withdrawal latch. Renewal and the waiver's money effect are cited from `specify-inference-commercial-terms` and `specify-inference-request-accounting`. The cited tests were re-run and pass (20 library, 2 owner-control, 17 runner tests). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1, serious: an unattributed client disconnect is waived and counted as a provider miss, so three cheap disconnects can latch withdrawal;
  - 2.2: a 0 µs TTFT counts as attained;
  - 2.3: qualification samples outside every prompt bucket are counted.
- Report: new entry on whether request-caused failures (for example, an engine-rejected structured-output schema) count as provider-incomplete misses (default: yes, as the code does).

## 2026-11-17 (thirteenth round) — M3: one process per consumer libp2p identity

- In step at `5449f06f`; whitepaper unchanged; no new answers.
- Spec step in `specify-p2p-provider-transport`: new requirement *A consumer libp2p identity runs in one process at a time* (decision 27, tasks 2.19 and 3.16, plus a coverage row). It closes the gap found last round: neither M3 nor any other change said how the desktop and a CLI gateway on one profile share its libp2p identity and fixed `dht.listen` port. An exclusive advisory lock on the identity file admits one node. A second process fails libp2p operations with a fixed error (503 `transport_unavailable` on HTTP surfaces), never switches port or identity, and keeps HTTPS and catalog discovery. A crashed holder's lock is released by the OS. The CLI gateway's *Shared DHT identity* non-goal now cites the requirement. Both changes pass strict validation; no code changed.
- Report: new entry asking whether two processes on one profile should share libp2p instead (default: no).

## 2026-11-17 (twelfth round) — CLI loopback gateway specified

- In step at `d064cbda`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-cli-loopback-gateway` (capability `cli-loopback-gateway`, 12 requirements, about 55 scenarios), the follow-up that milestone 2 named for the book's "the native desktop and CLI therefore run a local loopback gateway" (line 315). `chiral --config <profile> gateway serve --port <port>` is a foreground, loopback-only gateway for funded network accounts (HTTPS or libp2p). It reuses milestone 2's gate, bounds, routing, route-credential, request-identity and recovery requirements by name.
  - CLI-specific rules: a required port with no fallback; signal shutdown that cancels in-flight requests and reports them as possibly charged; owner-only state scoped to network and wallet; a bearer that no command prints; route commands that issue dedicated capped credentials, always show the spending disclosure and need `--accept-spending-risk`; JSON-line events on stdout with the transport path; and no consumer lock held while serving.
  - M2's proposal and design now point at the change. The review fixed six defects, including an issued credential left unmentioned when its route cannot be saved, and an unrequired DHT start. All three affected changes pass strict validation. No code changed; checks are unchanged since round 11.
- Report: new entry asking whether the CLI gateway should also serve local models (default: no).

## 2026-11-17 (eleventh round) — Code: the desktop re-reads the funding journal after every funding attempt

- In step at `820b6567`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.9, requirement 2). `Usage.svelte` used to leave the pending-funding journal stale after a top-up or resume attempt. After a top-up error the *Funding & recovery* tab said nothing was pending until *Refresh*, and a resolved operation could leave a stale notice. `topup()`, `resume()` and the recovery-tab `recover()` now go through an `attempt()` helper that re-reads `pending_accounts` whether the action succeeds or fails. A new frontend regression fails on the previous page. Screenshots show the notice, and the stage and hash, after a failed top-up without *Refresh*. Checks: svelte-check 0/0, 32 frontend tests, build, 46 script tests. Rust is unchanged since the full headless suite passed at `820b6567` (475 passed, 2 ignored).
- `specify-funding-settlement` code differences are now all fixed except 2.4, which waits for the report question. Its remaining open items are acceptance tests (3.x) and live-chain evidence (4.x).

## 2026-11-17 (tenth round) — Code: funding-risk disclosures on the Marketplace, Usage and CLI

- In step at `ea1c7202`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` tasks 2.1–2.3, requirements 2 and 3). Before this step, the Marketplace funding panel and its pending notice said payment is broadcast only after the provider's acknowledgement, and the CLI `fund` text and the Usage top-up card left out parts of the book's disclosures. All three now follow book line 489 and A.1.6. The Marketplace panel has a *Residual registration risk:* paragraph: the client broadcasts only after a durable acknowledgement, the provider holds broadcastable bytes and could broadcast before or without acknowledging, the ordering is not cryptographic escrow, and an error does not prove no money moved. Its acceptance covers that risk and the reorganization statement. The CLI `fund` stderr has the same elements. The Usage top-up card states that reorganizations can freeze or reverse credit and must be accepted before *Sign and top up*, without registration wording. New tests: seven in `tests/fundingDisclosure.test.ts` and one CLI process test; the four that cover new text fail on the old text. Screenshots of the panel, the pending notice, the top-up card and a frozen account (browser preview with a scratchpad-only IPC stub) were checked. Task 3.3 is complete.
- Spec: the review found that `Usage.svelte` does not re-read the funding journal after a top-up or resume attempt, so the journaled stage is not shown after a top-up error until *Refresh*. This is recorded as design code difference 9 and task 2.9.

## 2026-11-17 (ninth round) — Code: a top-up cancellation request is durable before the attempt

- In step at `3ea39335`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.8, requirement 16): `cancel_funding` now saves the top-up's cancellation request together with its validated original bytes before it calls `cancel_payment_once`, as it already did for an opening. Previously, an attempt that failed before journaling a replacement (for example, unaffordable cancellation gas) left the original broadcastable by `resume`, `fund` or `topup` while its nonce was current. From then on those paths never call `send_payment_once` for that operation. They run the read-only reconcile instead: a won original is credited once from its exact bytes, and a resolution without a win clears the intent. An unresolved request still refreshes the account and returns an error that names `funding-recovery` and the explicit retry. `funding-recovery` records a no-replacement win, and the pending view shows `topup_cancellation_requested`. Two regressions fail without the change. The adversarial review found no defect after four extra probes (nonce not current, every settle path after a win, a reorg displacing a recorded win, and a stale win followed by a replacement).
- Known limits, not spec differences: an older binary ignores the new `TopupIntent` field, so a downgrade loses the protection. A no-replacement top-up that resolved `original_failed` at safe is no longer tracked if a deeper reorg later executes it, which matches the existing `cancel_funding` behaviour; explicit `cancel-funding --operation-id` still recovers it.

## 2026-11-17 (eighth round) — Code: a won original is settled by resume without cancellation authority

- In step at `7a8eb7ac`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.6, requirements 16 and 17): before this change, an original that won its cancellation race could be recovered only through `cancel-funding --accept-gas-and-race-risk`, a call that can resubmit the replacement. Now `funding-recovery` and `resume` run the read-only reconcile and durably record each cancelled operation's outcome with the saved agreement (`funding_cancellation`, new `won_topups`). `resume` then opens a won opening, or credits a won top-up once, from the exact journaled original. It never signs, broadcasts or calls the cancellation path, and it withdraws a recorded win that a reorg later displaced. The refusal message no longer points to `cancel-funding`. The adversarial review fixed two gaps: an opening whose cancellation journaled no replacement, and a stale win that could reach `send_payment_once`. Four regressions fail without the change.
- Spec: the review also found that a top-up cancellation request is not saved before the cancellation attempt, so a failed attempt leaves the original broadcastable by `resume`. This is recorded as design code difference 8 and unchecked task 2.8.

## 2026-11-17 (seventh round) — Code: one refused funding recheck no longer stalls monitoring

- In step at `de5eb44e`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.5, requirement 9): `contracts::reconcile` used to abort the whole pass on a ledger refusal without advancing that row, so one payment blocked every later recheck and maintenance stopped accepting. A refused row is now quarantined durably and its `checked` time advanced. Quarantined rows are revisited first; while any remain, new funding and offers stay closed under a fixed category, while existing credit keeps serving. The quarantine is released when an actual chain observation applies. A ledger regression and the extended reconcile integration test both fail without the change.

## 2026-11-17 (sixth round) — Code: opening cancellation no longer blocks account refresh or top-up recovery

- In step at `aa8342c3`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.7, requirements 4 and 16): `Consumer::resume_inner` used to refuse every `resume`/`fund` after an explicit opening cancellation, even on a saved account, which stranded a pending top-up. Now only an unopened cancelled opening is refused. A saved account skips the cancelled opening's payment step, still recovers a pending top-up's exact original bytes and refreshes. Two regressions fail without the change and pass with it.

## 2026-11-17 (fifth round) — Funding settlement specified

- In step at `763ea347`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-funding-settlement` (capability `inference-funding-settlement`, 19 requirements, 65 scenarios). It covers the whitepaper's safe-frontier credit policy, monitoring and reorg/orphan freeze, reversal and loss, restoration, outages, the balance identity, `cancel-funding`, funding recovery, displaced re-inclusion and nonce-journal ownership. Checked tasks cite tests that were re-run and pass; synthetic fixtures are not live-chain evidence.
- Code differences recorded as unchecked tasks 2.1–2.7:
  - desktop and CLI registration-risk disclosure wording;
  - Usage top-up reorganization disclosure;
  - contradictory evidence treated as rejection;
  - one failing recheck stalls monitoring;
  - recovering a won original only through cancellation;
  - a contract-wide opening-cancellation gate.
- Report: three author questions (top-ups to frozen accounts, contradictory evidence vs existing credit, CLI reorganization disclosure).

## 2026-11-17 (fourth round) — M2 session scoping; the comparison's contradictions are all fixed

- In step at `a40d35ea`; whitepaper unchanged; no new answers.
- Spec step:
  - `specify-desktop-local-api-server`: network routes are scoped to network identity and consumer wallet. Another session's routes are hidden on the Local server page (count only, not deleted), and network-route log entries are removed on logout or a profile change (book A.1.8, scoped by A.7). The design now quotes *Use the network* exactly.
  - `specify-desktop-local-models`: its proposal now says five milestones.
  - Both validate strictly; no code changed.
- Every spec-contradicts-whitepaper finding from the 2026-11-17 comparison is now resolved in the specs. What remains is missing specification, then implementation.

## 2026-11-17 (third round) — M4/M5 aligned with the whitepaper

- In step at `0a8e8cf0`; whitepaper unchanged; no new answers.
- Spec step:
  - M4: Auto-price is unavailable for embedding shares, because they cannot hold the valid qualification *Ready to charge* needs; manual pricing stays. This supersedes the earlier embedding-readiness default. The publish disclosure now states the book's prepaid-balance sentence unconditionally.
  - M5: availability sampled at a fixed 300 s period with a random per-identity phase; "24 hours observed" from the exact first counted sample; *Launching — free* only for a generation provider that is free and has never charged; a per-wallet tombstone keeps `sharing_since` after the 180-day retention; no readiness for embedding-only wallets.
  - M1, M2 and M4: stale lines aligned.
  - All five drafts pass strict validation; no code changed.
- Report: new entry on predictable fixed-interval sampling and the *Launching — free* approximation.

## 2026-11-17 (second round) — Durable relay grace evidence and the 30-day deletion bound (M3)

- In step at `e228c76e`; whitepaper unchanged (SHA-256 `eda22ce0…`); no new answers.
- Spec step in `specify-p2p-provider-transport`. Offer evidence is now durable: a protected, owner-only, network-scoped store that fails closed when damaged, with wall-clock caps across restarts and the monotonic cap within a run. A relay restart no longer ends the 24-hour grace. Eviction from the operator-configured evidence maximum (earliest grace end first) is the one disclosed early end, reported in a new loopback-only, bearer-authenticated `relay_server` administration status. Ended free accounts' records, status and receipts now carry the 30-day deletion bound everywhere they were said to stay available. M4's copies of the relayed-grace statement, plus stale lines in M1 and M2, were aligned. All four changes pass strict validation; no code changed.
- Report: new entry proposing whitepaper wording for the evidence-store bound, with the fail-closed and relay-admin-status assumptions.

## 2026-11-17 — Main proposal-admission spec aligned with non-reserving quotes

- Entry clean at `38fc8bea`, in step with `origin/main`; no merge in progress. Whitepaper `docs/chiral-book.md` unchanged (SHA-256 `eda22ce0…`); no new `Answer:` lines.
- Ran a systematic spec-versus-whitepaper comparison (six slices, each finding adversarially verified). Highest confirmed contradiction: the archived main spec `inference-proposal-admission` still required the superseded named-wallet quote gates, which **Approved non-reserving quotes** forbids and the code had already removed.
- Spec step: moved the finished, implemented and tested deltas unchanged from `specify-non-reserving-quotes` into `adopt-provider-wide-quote-retention` and archived it (`+3 ~3 -3`); corrected the main spec Purpose. `specify-non-reserving-quotes` keeps only *Public quotes do not reserve opening capacity* and *Authenticated paid admission preserves accepted commitments* (open tasks 3.3, 4.3, 4.4). No code change. Checks: strict validation of the main spec and of the non-reserving, proposal-rate and opening-intent changes; svelte-check 0/0, 24 frontend tests, frontend build, 46 script tests. Rust code is unchanged since the full headless suite passed (465 tests) at `38fc8bea`.
- Report: worker action under the quota entry (stays open); new entries on the Hugging Face token location, the free-share immutability exception wording, and the assumption that deleting an ended free account frees its wallet's slot.

## Next steps, in order

Code differences, then missing specifications (spec before code):

1. Work through `docs/apas-worker/comparison-2026-11-18.md`: code differences first (item 2, consumer profile paths, is high), then the missing specifications in its order.

Earlier open work, unchanged:

2. Non-reserving 3.3/4.3, snapshot 2.5/3.3–3.4, challenge-clock non-regression, request-accounting 2.2.4/2.3/2.5/3.1–3.3, and the other acceptance items listed in the changes' tasks. Archive `specify-proposal-rate-budgets` once `specify-non-reserving-quotes` completes. The acceptance tasks 3.x of `specify-funding-settlement`, `specify-generation-performance` and `specify-fee-remittance` belong here too. Every code difference in those three changes is fixed except funding-settlement 2.4, which waits for the report question.
3. Milestone implementation M1 → M3 → M2 → M5 → M4. Extend `verus.md` judgments; proofs wait for Verus.

## Blocked

- Verus setup and proofs: open report entry (not installed on this Mac).
- The M1 engine-confinement task waits for the report entry on the Hugging Face token location.
- Real chain, engine, GPU, relay deployment, installers and public-release gates are outside this checkout.

## Checkout discipline

The untracked `src-tauri/gen/schemas/macOS-schema.json` is generated build output and is never committed. The whitepaper is read-only.
