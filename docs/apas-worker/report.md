Answer an entry with a line starting `Answer:` under that entry. The worker never edits the whitepaper.

## 2026-11-14 — Public proposals can exhaust a named payer's quota

Status: open

Where: `docs/chiral-book.md`, authorization matrix row for `POST /v1/contracts/propose` (currently line 270).

Current text: “Public proposal for an unexpired local offer, subject to provider funding/admission rules. **This endpoint is not owner-authenticated.** The resulting quote binds its named consumer.”

Evidence: `contracts.rs::control` accepts the caller's canonical named payer without owner proof, and `BillingStore::store_quote` limits that payer to eight pending quotes. The isolated loopback regression `public_pending_quota_follows_the_canonical_named_payer_not_caller_authority` now demonstrates eight unauthenticated proposals for one payer, shared capacity across address-case aliases, HTTP 429 / `rate_limit` on subsequent proposals and continued admission for another payer. Issued signed agreements remain intact, with no account creation or transaction submission.

A caller can therefore exhaust another payer's fresh-quote allowance without paying or owning the wallet. Replenishing expired proposals to sustain that obstruction remains a source-derived risk, not a tested live-deployment claim. This does not grant access to an account, credentials or funds; existing owner-authorization tests continue to pass. No live attack was performed.

Proposed addition after the authorization matrix:

> Public quote issuance does not prove control of the named payer. A caller can consume that payer's pending-proposal allowance and repeatedly obstruct new quote issuance while keeping the allowance full. These quotas are availability controls, not payer ownership proofs or Sybil resistance. Quota expiry must not erase acknowledged funding terms or settled accounts. Public-rollout security and load review must account for this availability exposure.

Why: the current public-proposal design and its per-payer bounds have a concrete availability tradeoff that should not be mistaken for authenticated per-caller protection.

Author action requested: add an equivalent disclosure, or specify a different public-proposal design if this exposure is unacceptable. Only the author can change that whitepaper boundary. Acceptance of this proposed text does not authorize the worker to edit it; the entry remains open until the whitepaper reflects the decision.

Nonblocking for specification/testing of the current pending-quota contract. No authentication change, mitigation, live attack or public rollout is performed or claimed here.

Answer: The author selected non-reserving, overbookable quotes rather than wallet authentication for public quote issuance. Providers may issue more quotes than they can accept. Make the paid admission decision at payer-authenticated funding-intent, before acknowledgement and client broadcast: atomically accept available capacity with a durable acknowledgement, or reject when full. Never reject an already acknowledged valid payment merely because capacity later fills. Public proposals must not consume a named wallet's pending-quote or proposal-rate allowance; preserve bounded provider-wide/request-source resource controls. Free openings decide capacity at authenticated open without a payment. Preserve accepted terms, nonce ownership, ambiguity recovery and safe-frontier credit checks.

The author-approved contract is now in `docs/chiral-book.md`, **Approved non-reserving quotes**. The earlier authorization-matrix quotation and disclosure-only proposal above record the finding before this decision; they are not the selected remedy. The worker need not edit the whitepaper. Align conflicting pending-proposal, proposal-rate, acknowledged-slot and opening-intent specifications and implementation with this decision without rewriting historical evidence as if it tested the new behavior. The current quota regression remains evidence of the old exposure, not proof of mitigation. Keep implementation and acceptance gaps open; no production code or tests were changed by this answer, and no deployment is authorized.

Answer: You may incorporate and commit the pre-existing source needed for this implementation; preserve unrelated work.

Worker acknowledgement (2026-11-14): This source-incorporation answer arrived during the terminal-race acceptance step. It removes the ownership blocker for source actually needed by the non-reserving implementation, not permission to absorb unrelated migration work. Preserve and commit this answer with the APAS notes; the next step must act on it by identifying the necessary source/dependencies and implementing the approved cutover. The entry remains open until implementation and acceptance align; the whitepaper stays read-only.

Worker action (2026-11-14): Read the author's updated book and complete answer, then created `specify-non-reserving-quotes` with replacement proposal-admission deltas, resource-control decisions and implementation/acceptance tasks. The earlier quota/disclosure assessment above is historical, not the selected policy. This first specification slice removes named-wallet quote allowances from the replacement contract, distinguishes quote retention from authenticated paid/free admission, and preserves accepted-original recovery. It also specifies bounded pre-signing work separately from committed quote issuance.

After that first slice, downstream rate/opening/commercial alignment, native wallet-gate removal and mitigation acceptance remained incomplete. The new change stays open; passing old-policy tests is not mitigation evidence. The author has made the whitepaper change; the worker has neither edited nor committed the whitepaper.

Worker action continued (2026-11-14): Completed non-reserving-quotes task 2.1 by retargeting the proposal-rate delta, proposal, design and tasks to one provider-ledger-wide committed-issuance budget. Both affected changes pass strict validation. The contract now forbids named-wallet rate charging, preserves the global counter and its non-regressing clock, refuses missing/contradictory retained totals, and distinguishes insertion rollback from an already spent pre-signing work permit. Previous wallet-ceiling tests remain historical evidence; their replacement acceptance tasks are open and no production code/tests were changed.

Worker action continued (2026-11-14): Completed task 2.2 by aligning opening-intent and commercial specifications, proposals, designs, source mappings and remaining tasks with non-reserving quotes. The two changes and their parent pass strict validation. Paid capacity is decided atomically at authenticated funding-intent; free capacity is decided at authenticated first opening. Definite full refusal forbids acknowledgement and honest-provider/client submission, while ambiguous replies require original-operation recovery. Existing accepted terms and safe-credit conditions remain protected against later capacity pressure or expiry.

Native implementation and mitigation evidence remain incomplete, so this entry stays open without another author question. The preceding specification steps changed neither runtime nor tests and left the whitepaper unchanged. Native cutover still requires independently committable source; old wallet-quota tests are not mitigation.

Worker action continued (2026-11-14): Extended worker-owned opening-intent acceptance (task 2.3.5) for exact raw-string acknowledgement recovery. Equivalent alternate hexadecimal spellings must conflict while the verbatim original remains recoverable after ledger/router/chain reopening with fresh-funding gates closed, qualification cleared and RPC stopped. Verification is recorded in progress; this bounded recovery coverage does not remove the public wallet quotas, exercise their replacement, or complete expiry/full-capacity/lost-response/native-broadcast acceptance. No production source or whitepaper edit and no new author decision.

Worker action continued (2026-11-14): Added worker-owned native recovery acceptance (opening-intent task 2.6.1) for a valid saved original with a missing or different valid journal original. Both native recovery entry points must preserve the saved agreement and refuse replacement before chain RPC; explicit fixture repair is only a positive control, not an automatic product recovery policy. Verification is recorded in progress. This does not change public quote gates or finish their mitigation; production source and whitepaper remain untouched, and no new author decision is needed.

Worker action continued (2026-11-14): Added worker-owned acceptance for opening-intent task 2.5.1: a saved broadcast-start marker cannot substitute for an omitted/null provider acknowledgement. Both native recovery entry points must fail closed while preserving the genuine original and journal; restoring the genuine fixture state is only a positive control. Verification is recorded in progress. Production source, public quote policy and the whitepaper are unchanged; the mitigation entry remains open without another author question.

Worker action continued (2026-11-14): Added opening-intent task 2.5.2's native acknowledgement commit-failure acceptance using a deterministic isolated filesystem obstruction and genuine cached replies. Original terms/journal evidence must survive and explicit obstruction removal must recover the same acknowledgement/payment. The later start-state-only write fault remains separately blocked on a deterministic independently committable seam (2.5.3); no policy question is needed. Verification is recorded in progress. Production source, the whitepaper and public quote mitigation remain unchanged.

Worker action continued (2026-11-14): Added opening-intent task 2.6.2's native ambiguous-submission acceptance for remote-error, mismatching-hash and disconnected RPC replies, with reopened `resume`/repeated `fund`, durable prerequisite snapshots, blocked conflicting preparation and same-original retries. Synthetic pending-chain observations never establish execution or credit; verification is recorded in progress. Production source, the whitepaper and public quote mitigation remain unchanged, and no new author decision is needed.

Worker action continued (2026-11-14): Added opening-intent task 2.3.3.1's provider-side final-paid-lease competition across independent ledger/chain/router instances, with distinct authenticated originals, complete signed retained prerequisites and full-capacity recovery after reopening with fresh gates/RPC unavailable. Verification is recorded in progress. Native-client full-refusal/submission coverage remains separate in 2.3.3.2; synthetic spendability and restored historical records are not live admission or credit evidence. Production source, the whitepaper and public quote mitigation remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Extended opening-intent task 2.3.3's final-lease scenario with the losing payer's native initial `fund`, reopened `resume` and repeated `fund` at full paid capacity (2.3.3.2). The assertions observe genuine HTTP refusals and separate native/provider RPC boundaries, preserving the exact original journal, pending local state and retained provider evidence. Verification is recorded in progress. Definite full refusal is not lost-acceptance-response recovery, live-chain evidence or mitigation of the blocked public quote gates. Production source and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added opening-intent task 2.3.4.1's genuine fresh acknowledgement-response loss and reopened provider/native recovery, rather than treating loss of a preexisting cached reply as fresh acceptance. The native original is a restored preparation prerequisite; provider owner admission, synthetic spendability, ledger commit and response-body disconnection are exercised. Verification is recorded in progress. Later full-resource pressure and deterministic expiry remain separate tasks 2.3.4.2–3, so parent 2.3.4 stays open. Production source and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Extended opening-intent task 2.3.4.2 with later full paid-intent and standalone quote resources after genuine acknowledgement commit/loss. Complete signed bulk records are explicit storage prerequisites, not claimed live admissions or public issuances. The assertions require genuine fresh-capacity refusals and exact reopened native recovery without rewriting any retained record or lease; verification is recorded in progress. Deterministic expiry remains separate in 2.3.4.3, and the blocked non-reserving quote cutover is still not mitigated. Production source and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added opening-intent task 2.8.1.1's final owned free-account slot acceptance with seven real owner accounts, an unrelated owner's real account and two genuine competing quotes across independent provider ledger/chain instances. Assertions require exactly one owner-authenticated success, intact losing/pending records, no partial account/credential and preservation of issued bearers through store reopening, with no provider RPC or payment records. Verification is recorded in progress. Ledger-wide capacity, transactional fault rollback and native free-opening observations remain separate tasks 2.8.1.2–4; this does not mitigate the blocked public quote gates or resolve accepted-account root rotation. Production source and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added opening-intent task 2.8.1.2's ledger-wide final free-account slot acceptance, reusing the prior independent-writer race with two different eligible owners. The 9,997 complete signed account/root records are restored prerequisites, not claimed historical admissions; the competing owner-authenticated requests and later public quote/refusal controls are genuine. Counted streaming digests preserve all account/credential columns, and actual issued plus representative restored bearers exercise reopened authentication/account reads. Verification is recorded in progress. Transactional fault rollback, native preparation/submission, root-rotation recovery and public quote-policy mitigation remain separate unfinished work; production source and the whitepaper are unchanged, with no new author decision needed.

Worker action continued (2026-11-14): Added opening-intent task 2.8.1.3's first-free-opening rollback acceptance at account insertion, root-credential insertion and pending-quote deletion. Candidate-scoped `AFTER`/`RAISE(FAIL)` triggers require actual transaction rollback, with repeated genuine owner-authenticated failures after provider reopening, consumed-proof replay refusal, unchanged original/unrelated records and usable prior bearers. Explicit fault removal then opens the same quote and preserves its new credential through reopening; verification is recorded in progress. Statement faults do not establish commit/fsync or power-loss durability, native payment preparation, root-rotation recovery or public quote-policy mitigation. Production source and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added opening-intent task 2.8.1.4's native free-opening/full-refusal acceptance against the production provider router. Genuine native openings, initial/reopened refusal paths and saved-account refresh observe both wallet journals, a separate native RPC endpoint, exact control requests, pending local state and exported credentials; verification is recorded in progress. This is distinct from provider-only admission tests and from lost-unsaved-response/root-rotation recovery. The acknowledged-paid-opening exclusion from free bounds remains in task 2.8.2, so parent 2.8.1 stays open. Production source, public quote-policy mitigation and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added task 2.8.2.1's post-verification paid-ledger opening acceptance at exhausted owned and global free-account bounds. An earlier durable acknowledgement survives later capacity pressure, missing-evidence refusal and reopening; exact credited state, fee obligation, prior credentials and permanent intent ownership are checked. The global bulk records and accepted-block input are explicit prerequisites, not observed historical admissions or healthy-safe execution. Task 2.8.2.2 retains the paid HTTP/native boundary and parent 2.8.1 remains open; verification is recorded in progress. Production source, public quote policy and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Completed task 2.8.2.2's native/HTTP paid-opening acceptance at both free-account bounds. Fresh authenticated acknowledgement, separate client/provider chain views, safe-credit refusals, terminal lease release with permanent evidence retention, eventual production-verified credit and reopened saved-account credential preservation are observed. Native preparation and bulk account history remain explicit restored prerequisites; synthetic RPC does not qualify live execution, broadcast or power-loss durability. Verification passed as recorded in progress. The free-bound acceptance closes parent 2.8.1 without closing quote-resource/top-up/settlement recovery, lost-unsaved-response root rotation or the production cutover. Production source, public quote policy and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Completed task 2.8.2.3's provider-side top-up recovery acceptance at full standalone quote storage. A genuine public quote refusal, unchanged owner authentication, independent safe-credit observations, pending recovery, exact top-up/fee arithmetic and reopened idempotence are verified without changing retained terms or exported bearer authority. Initial paid opening and bulk quote history are explicit restored/post-verification prerequisites; synthetic RPC is not live-chain, native top-up or remittance evidence. Verification passed as recorded in progress. Other quote-work/issuance and inference-settlement cases remain open; production source, the whitepaper and the public quote-policy blockers remain unchanged, with no new author question.

Worker action continued (2026-11-14): Completed task 2.8.2.4's native top-up original-ownership acceptance before broader resource-pressure cases. Missing and internally consistent different-nonce journal originals stop reopened `resume`, repeated `topup` and repeated `fund` before RPC/provider access, preserving saved evidence, journal/provider records and exported credentials. Explicit fixture-only restoration of the exact original unblocks reconciliation to an unavailable RPC without replacement or submission. Paid-opening/native state is a disclosed prerequisite; this is not automatic repair, capacity exhaustion, successful top-up settlement or live-chain evidence. Verification passed as recorded in progress. The production source, whitepaper and public quote-policy blockers remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Completed task 2.8.2.5's native top-up workflow at full quote storage, including fresh intent/preparation/submission, independent provider credit checks and a lost genuine credit response followed by same-original recovery. Original paid-opening/native and bulk quote history are explicit restored prerequisites; the top-up is produced by the native workflow. The test preserves old local state during ambiguous acceptance, original journal ownership, exact credit/fee arithmetic and exported credentials. Verification passed as recorded in progress. Scripted RPC is not live-chain, power-loss or fee-remittance evidence; other resource/settlement cases and production-source/whitepaper/public quote-policy blockers remain unchanged. No new author decision is needed.

Worker action continued (2026-11-14): The remaining settlement-pressure inventory found a missing specification foundation: quote-capacity exclusions did not define request reservation/terminal accounting. Created `specify-inference-request-accounting` for the book's exact holds/charges, account/credential limits, authoritative and unknown usage, restart, performance-remedy binding and attributable recovery history, with source/test correspondence and open acceptance tasks. Existing creation-time inference budgets and bounded timestamp-only pages are preserved without claiming an exhaustive history or a quote-style effective-minute clock. Request unknown-usage exposure is explicitly distinct from actual measured loss and account funding-orphan compensation. This specification-only step changes no runtime, tests, whitepaper or public quote-policy implementation; verification is recorded in progress. No author-only decision is needed.

Worker action continued (2026-11-14): Completed the request-accounting ledger/read coverage map and added task 2.4's full-quote-storage regression. It exercises real reservations, known settlement, unknown restart, unchanged duplicate/cap/scope/concurrency guards and actual owner/read-bearer request/receipt recovery across ledger/router reopening. Complete signed bulk quote history and post-verification opening credit are disclosed prerequisites; supplied usage is not engine evidence. Strict/project verification is recorded in progress. This does not implement public quote-policy mitigation, exercise separate signing-work/issuance exhaustion or establish daemon/process-kill or live-chain evidence. Production source and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added request-accounting task 2.2.1's reservation write-failure acceptance at request insertion and both account/credential hold updates. Candidate-scoped `AFTER`/`RAISE(FAIL)` triggers require production transaction rollback of actual mutations, with complete ledger snapshots, retained prior spend/holds/authority, same-key isolation and successful explicit fixture repair across reopening. Post-verification opening credit and supplied prior usage are disclosed prerequisites. Verification is recorded in progress; this is not terminal-failure, competing-writer, gateway non-dispatch, commit/fsync or power-loss evidence. Those tasks remain open. Production source, public quote policy and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added request-accounting task 2.2.2.1's unprofiled terminal write-failure matrix for known, unknown and invalid usage at account, credential and request updates. Complete snapshots distinguish rolled-back persistence errors from a deliberately committed invalid-usage release/error, preserve earlier holds/financial authority, and require first-terminal-result idempotence through reopening after explicit fixture repair. The existing reservation regression shares the unchanged snapshot queries and retains every assertion. Verification is recorded in progress. Commit refusal, performance observation/withdrawal writes, gateway shutdown and physical durability remain separate open tasks; setup is post-verification credit and supplied usage, not chain/engine evidence. Production source, public quote policy and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added request-accounting task 2.2.2.2's terminal commit-refusal acceptance for known, unknown and invalid usage. A fixture-only deferred foreign key rejects commit after settlement writes; complete ledger snapshots and an absent transactional witness require rollback before and after reopening. Explicit parent insertion leaves the trigger active for same-original reconciliation, whose exact account/credential/request witness and first terminal outcome must survive later retries and reopening. Existing statement-fault assertions remain intact in the shared matrix. Verification is recorded in progress. Constraint rejection is not physical fsync/power-loss, gateway shutdown or performance-latch evidence; those boundaries remain open. Production source, public quote policy and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added request-accounting task 2.2.2.3's bound performance failure matrix: known/unknown/invalid usage at observation insertion, withdrawal insertion and post-latch commit refusal. Complete snapshots preserve original terms/fees, prior spend/waivers, old withdrawal and both holds across reopening, then require one target reconciliation with the admitted report and new latch. Late outcomes cannot rewrite that result; an earlier live request can settle after withdrawal without clearing either latch. Verification is recorded in progress. Signed synthetic profiles, post-verification credit and supplied usage are ledger prerequisites, not enrollment, hardware or engine evidence. Gateway shutdown, broader performance classification and physical durability remain separate. Production source, public quote policy and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added request-accounting task 2.2.3.1's same-key independent-writer acceptance with shared and distinct credentials and known/unknown later settlement. Exactly one original token-bound reservation may commit; complete snapshots preserve its hold, losing credential authority, account isolation and reserved/terminal identities through reopened competing retries. A distinct-key concurrent control admits both original shapes and releases their holds without rewriting the first charge. Verification is recorded in progress. Limit-refusal and first-terminal-outcome races remain separate in 2.2.3.2–3; no winner schedule, engine dispatch, live-chain or process-kill claim is made. Production source, public quote policy and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added request-accounting task 2.2.3.2's independent reservation-limit races for spend cap, credential concurrency and the final account slot, each with known/unknown later settlement. Exact snapshots preserve prior spend, original bounds, credentials and any earlier root hold; independent controls distinguish the actual limiting checks. The refused key remains absent until allowance is released, then can be admitted without a new key, while late terminal replay cannot release its new hold. Verification is recorded in progress. Competing terminal outcomes remain in 2.2.3.3; supplied usage and post-verification credit do not establish engine, live-chain or process-kill evidence. Production source, public quote policy and the whitepaper remain unchanged; no new author decision is needed.

Worker action continued (2026-11-14): Added request-accounting task 2.2.3.3's pairwise first-terminal-result races on independently opened connections: six unprofiled known/unknown/invalid pairs and ten profiled pairs including inference waivers. Exact snapshots and return values preserve one complete winning charge/release, usage, status, timestamp and admitted report across reopened competing replays. Explicit independent control keys establish each candidate's effect; prior fees, credential authority, old withdrawal and a separate running hold survive. Verification is recorded in progress. This is scoped ledger acceptance, not actual engine usage, qualification, live chain, exhaustive scheduling or process-kill durability. The existing performance-failure fixture setup is shared without changing its assertions; production source, public quote policy and the whitepaper remain unchanged. No new author decision is needed.

Worker action continued (2026-11-15): Acted on the source-incorporation Answer. The checkout entered clean at `7536493a` with the migration already committed, removing the former ownership blocker. Implemented the provider-only committed-admission slice of `specify-non-reserving-quotes` 3.1/3.5 and related rate-clock/adoption work: remove both public named-wallet gates together, preserve the 10,000 stored-quote and 600 committed-issuance bounds, retain a non-regressing committed minute, adopt valid global evidence without inventing wallet totals, and retire legacy wallet rows only with a successful quote transaction. Regression coverage migrates old wallet refusals to public/native acceptance and provider-global retention, rollback, concurrency and recovery assertions; validation results are recorded in progress. The public handler still signs before storage admission, so bounded pre-signing work and remaining opening/recovery acceptance remain explicitly open. The author decision is settled; no new answer, whitepaper edit, deployment or public-release authorization is inferred.

Worker action continued (2026-11-15): Completed non-reserving parent 3.4 and owner-control 2.4.3's existing-account credential preservation. Authenticated reopening now returns the account with `credential: null` without revoking, reissuing or disclosing secrets; native recovery after a genuinely lost first-opening reply saves the account and leaves missing-default repair to explicit issuance. Regressions retain original paid-evidence guards, funding/fee deduplication, credential spend/holds, revoked/expired authority, one-secret concurrent creation and real HTTP/native recovery. All 469 available tests and available builds/checks pass as recorded in progress. Bounded pre-signing work and remaining paid/expiry acceptance are still open, so this author decision's implementation entry remains open without another question. No whitepaper edit or deployment.

Worker action continued (2026-11-15): Completed parent 3.2/4.2 and rate 2.3/2.5/3.1. The actual public proposal route now commits independent provider-wide signing-work allowance before invoking the real signer: 600 attempts per non-regressing effective minute, no refund after signing/insertion/reply failure, durable conservative initialization and fail-closed inconsistent evidence. Seven ledger and three new HTTP regressions plus extended body/control/recovery cases pass; all 479 available tests and available builds/checks pass as recorded in progress. Both exhausted quote budgets preserve existing free/paid accounts, explicit credential management, admitted-hold settlement and authorized request reads. Remaining paid/expiry and broader top-up/settlement acceptance keeps the change and this entry open; no new author decision or deployment is required.

Nonblocking factual update proposed for the author: in **Approved non-reserving quotes**, the final “Implementation gap and required evidence” paragraph still describes the old eight-slot and named-payer rate gates as current. Replace that present-tense description with: “The earlier isolated regression demonstrated the former named-wallet quota exposure. Native proposal issuance now uses provider-wide quote retention, committed-issuance and durable pre-signing-work bounds without named-wallet quote allowances. Remaining paid/expiry recovery acceptance and the full public-release gates are still open; isolated checks are not deployment qualification.” The worker leaves this factual whitepaper update to the author; it does not change the approved design or block remaining implementation acceptance.

Worker action continued (2026-11-17): A systematic spec-versus-whitepaper comparison found that the archived main spec `inference-proposal-admission` still required the superseded named-wallet gates (eight pending proposals per named wallet, per-payer rate atomicity and named-payer quota attribution), contradicting **Approved non-reserving quotes** and the already-migrated code. The finished, implemented and tested deltas moved unchanged from `specify-non-reserving-quotes` into `adopt-provider-wide-quote-retention`, which was archived normally; the main spec now has provider-wide unaccepted-quote retention, atomic storage admission and the pre-signing work bound, and its Purpose no longer describes named-payer attribution. `specify-non-reserving-quotes` keeps *Public quotes do not reserve opening capacity* and *Authenticated paid admission preserves accepted commitments*, whose acceptance (tasks 3.3, 4.3) is still open, so this entry stays open. No code or whitepaper change; no new author decision is needed.

Worker action continued (2026-11-21): The implementation and its acceptance now cover every item of the book's "Implementation gap and required evidence" list. `specify-non-reserving-quotes` and `specify-proposal-rate-budgets` are archived (round 77). Mitigation evidence:
- Unrelated public proposals consume no named-wallet allowance: both wallet gates are removed (non-reserving 3.1, proposal-rate 2.4), and 24 same-owner proposals beyond both former ceilings are admitted (4.1).
- Quote resources stay bounded provider-wide: 10,000 retained quotes, 600 committed insertions and 600 pre-signing work units per non-regressing minute (3.2, 4.2, proposal-rate 2.2–2.5).
- Competing funding intents at the capacity boundary: exactly one acknowledgement for the final paid lease, and deterministic two-writer tests show the capacity check and insertion share one write transaction (3.3, 4.3, round 76).
- Full capacity is refused with no acknowledgement, intent, account, credit or submission, and quotes issued while capacity is full reserve nothing (round 76).
- Accepted intents are recovered across lost replies, restart and quote/lease expiry, without a new lease or replacement, up to safe-frontier credit (round 76; opening-intent 2.3.4).

Remaining gaps, by design or outside this checkout:
- Provider-wide exhaustion still affects every caller; these are availability bounds, not Sybil resistance.
- Fresh owner proofs are limited to 12 per named wallet per minute, so a third party can still delay a wallet's owner operations. This is disclosed in the `inference-challenge-rate-budgets` spec.
- No public load, security review or live-chain qualification has been done.

Revised nonblocking factual update for the author, replacing the one above. In **Approved non-reserving quotes**, line 508 still describes the eight-slot and named-payer rate gates as current. A replacement: "The earlier isolated regression demonstrated the former named-wallet quota exposure. Native proposal issuance now uses provider-wide quote retention, committed-issuance and durable pre-signing-work bounds without named-wallet quote allowances, and isolated acceptance covers competing funding intents at the capacity boundary, full-capacity refusal without acknowledgement or submission, and recovery of accepted intents across lost replies, restart and expiry. Public-release security and load gates remain open; isolated checks are not deployment qualification." The entry stays open only for this factual update, which only the author can make.

## 2026-11-15 — Verus is unavailable

Status: open

`verus --version` fails here with exit 127 (`verus: command not found`). No existing Verus/vstd integration or project verification command was found in the README, CI, Cargo configuration, source, tests, scripts or OpenSpec files. The worker has not installed anything.

Question: Please make Verus available in this environment, or answer explicitly that the worker should keep tests instead of applying the Verus rules.

All Verus setup/proof steps wait for this entry. Once Verus runs, the worker can pin the available version/vstd and add the required README/CI verification command before proving code. File, network, third-party-crate and unmodeled concurrency acceptance remains test-based; those unblocked steps can continue. No test has been deleted or treated as proved, and no unchecked proof assumption has been introduced. Test judgments are recorded in `docs/apas-worker/verus.md`.

The question has not been asked interactively because an unblocked I/O acceptance step remains. No default or implied answer is assumed.

## 2026-11-17 — Where does the Hugging Face token live for engine confinement?

Status: open

Where: `docs/chiral-book.md`, *Desktop local-client workflow (target design)* → *My models* (currently line 711) and *Storage layout* (line 743).

What it says now: *My models* says an exploited engine "could read the wallet key, provider signing key, local bearer and Hugging Face token under `network/` and `local-server/`", and makes "OS confinement that denies the engine those directories" a release-gate item. *Storage layout* lists `local-server/` as "(bearer and settings)" and `network/` as the shared `data_dir`, wallet journal, provider ledger and DHT identity files; it gives the Hugging Face token no location, and *Discover and download models* says only that it is "stored as a protected native file".

Why it matters: milestone 1 (`specify-desktop-local-models`) lets the user choose the token file's path, so the token may sit outside the confined directories and the release gate as worded would not protect it.

Question: should the token have a fixed protected location inside the confined set (for example `<app data>/local-server/hf-token`, or a `secrets/` directory added to *Storage layout*), or should the gate be restated as "deny the engine every secret file", wherever it lives? Either wording is fine for the specs; only the author can choose. Until then the milestone-1 drafts keep the user-chosen path and the confinement task stays blocked.

## 2026-11-17 — Agreement-immutability text should name the free-share exception

Status: open

Where: `docs/chiral-book.md` lines 256 (*Immutable terms versus current availability*: "For existing accounts the sole supported evidence substitution is a strictly newer qualification measurement…"), 575 (*Approved evidence-only renewal*: "Changed terms require a distinct agreement, not silent migration") and 640 (*Provider lifecycle and immutable agreements*: "Evidence-only renewal is the narrowly defined exception above, not a general migration facility").

What changed: *Share online* step 4 (target design, author decision 2026-11-16) adds a second exception: after a launch-window qualification passes, the share "keeps serving the free accounts opened before it under their allowance — same model and settings, no speed promise", although a serving profile was added.

Proposed wording (append to each of the three sentences): "In the target design, *Share online* step 4 adds one more narrow exception: free agreements opened before a share's launch-window qualification stay servable, under their allowance and with no speed promise, after the same deployment gains its qualified profile."

Why: the three passages call evidence-only renewal the sole exception; the milestone-4 draft (`specify-desktop-share-online`, MODIFIED *Existing agreements permit only evidence-only renewal of identical terms*) follows step 4 and cites it. This is a wording proposal; it does not block the specs. The worker does not edit the whitepaper.

## 2026-11-17 — Assumption: deleting an ended free account frees its wallet's slot

Status: open

Where: `docs/chiral-book.md` *Wire change* (line 319): an ended free account "no longer counts toward the provider's ledger-wide account cap (the per-consumer cap still counts it) and is deleted 30 days after it ended".

Ambiguity: the whitepaper does not say whether the per-consumer cap (eight accounts per wallet) still counts an ended free account after it has been deleted. The milestone-3 draft (`specify-p2p-provider-transport`, MODIFIED *Authenticated opening admission preserves original acceptance*) counts only accounts that still exist, so 30 days after deletion a wallet regains that slot. The worker keeps that reading, because a deleted record is gone and a tombstone counter would be new retained state the whitepaper does not ask for.

Answer only if you want the other reading (deleted ended free accounts keep counting against the wallet's eight), which would need a compact per-wallet tombstone count.


## 2026-11-17 — Relay grace and the relay's bounded evidence store

Status: open

Where: `docs/chiral-book.md`, *Relays carry ciphertext, under explicit limits* (currently line 305): the 24-hour grace "is bounded by the same per-provider quotas", and *Author decisions* item 6.

What the specs now do: `specify-p2p-provider-transport` keeps each relay's offer evidence (the most recent qualifying offer it verified per destination) in a protected durable store, so a relay restart no longer ends a grace period. The store is bounded by an operator-configured maximum (at least the relay's reservation maximum). When it is full, the relay evicts the record with the earliest grace end. That destination then gets only coordination-sized circuits until it publishes again, and relay administration status reports the eviction. Any finite relay must bound this per-destination state, so a relay flooded with distinct publishing peers can end some grace periods early. The provider-facing disclosures say so.

Proposed wording (append to the grace sentence): "A relay keeps this evidence durably, so a restart does not end the grace, but in a bounded store: a relay whose evidence store is full may end the grace of the destination whose grace would end soonest, and reports it."

Why: the whitepaper names only per-provider quotas as the bound, so the eviction case is a resource limit it does not mention. The alternative of never evicting would refuse serving-grade circuits to newly published providers while the store is full. That trades one provider's grace for another's service and is not clearly better. Answer if you prefer that alternative, or a different bound. Until then the worker keeps the disclosed eviction.

Two related assumptions in the same change, for your awareness (answer only to change them):
- A missing, unreadable, corrupt or insecure evidence store fails closed. The relay then grants serving-grade limits only up to a destination's current offer expiry, with no grace. It reports the condition, never invents or reconstructs records, and never overwrites a damaged store. A missing store is treated like a first start and replaced by an empty one.
- To report the store's condition and evictions, `relay_server` gains a loopback-only, bearer-authenticated administration status, like `chiral_daemon`'s operator listener. This is a new operator surface with its own protected bearer file; the public relay routes are unchanged.

## 2026-11-17 — Launch reputation: predictable sampling and the "Launching — free" label

Status: open

Where: `docs/chiral-book.md`, *Launch reputation: service score and track record* (availability bullet, currently line 405) and A.4 *Offer card* (line 867).

1. **Fixed interval.** The whitepaper samples availability "at a fixed interval". `specify-provider-launch-reputation` now does exactly that: every 300 s, with a random phase drawn once per identity. A provider that recognizes the observer can therefore predict sample times and be reachable only around them. Proposed wording: replace "at a fixed interval" with "at random times averaging a fixed interval, so a provider cannot predict them". The earlier draft used that schedule (exponential intervals with a 300 s mean).
2. **"Launching — free".** A.4 shows *Launching — free* "during its launch window". The launch window is a desktop concept (Auto-price on, first free period, not yet ready to charge), and the catalog cannot see Auto-price. The spec therefore shows the label for a generation provider that is free now and has never completed a charged period. A share with Auto-price off, or one whose qualification has not passed, can read *Launching — free* without being in a launch window. Proposed wording for A.4: "*Launching — free* while the provider is free and has never charged".

Neither blocks the specs. Answer to choose the wording; without an answer the specs keep the whitepaper's current text.

## 2026-11-17 — Funding settlement: three questions from the new specification

Status: open

Context: the new change `specify-funding-settlement` specifies the whitepaper's settlement and recovery contract (*Agreements and exact funding order*, *Settlement and recovery risks*, *Durable accounting and fees*). Three points need the author:

1. **Top-ups to an account frozen by an orphan.** Suppose a top-up payment executed before its account became `frozen`, and the opening's orphan is permanent. The top-up contract then refuses settlement, and the provider keeps on-chain funds that can never be credited. That conflicts with line 495 ("Settlement of an existing top-up remains available…") and with recovery that does not abandon accepted balances. Should such a top-up be credited despite the freeze, held until the opening is restored, or recorded as provider loss? The spec currently leaves the existing refusal in place and records the case as a question.
2. **Contradictory chain evidence and existing credit.** Line 514 groups a missing receipt with "unavailable/contradictory evidence", and lines 252 and 630 say existing settled credit can remain usable during chain/RPC unavailability ("proven chain rejection is different"). The spec reads this as: an RPC that fails its identity check, or a receipt that contradicts the signed transaction, closes new admission and new credit but keeps already-accepted credit spendable. The code instead moves accepted credit to `settlement_pending` (no spending) and maps the case to a payment rejection; this is recorded as code-difference task 2.4. Confirm the spec's reading, or say that contradictory evidence should withhold spending.
3. **CLI reorganization disclosure.** A.1.6 requires the desktop to show, and the user to accept, that reorganizations can freeze or reverse credit before any opening or top-up. The book states no CLI equivalent, and the CLI `fund`/`topup` commands do not show it. Should the CLI carry the same disclosure and acceptance?

None blocks the specification; questions 1 and 2 block the matching code tasks.

## 2026-11-17 — Should the CLI loopback gateway also serve local models?

Status: open

Where: `docs/chiral-book.md`, *Peer-to-peer provider transport → SDK and tool compatibility* (line 315) and *Desktop local-client workflow → Discover and download models*, "One artifact contract" (line 702). Spec: the new change `specify-cli-loopback-gateway` (capability `cli-loopback-gateway`), design.md open question 1.

What it says now: line 315 says "The native desktop and CLI therefore run a **local loopback gateway** (`http://127.0.0.1:<port>/v1`, the desktop's local API server) that forwards requests for a funded network account to its provider". Line 702 says a model downloaded in the desktop "is therefore usable unchanged by the CLI, the daemon and qualification". The desktop's local API server also routes names to loaded local models (line 725), through milestone 1's load, eject and per-model engine, which exist only in the desktop.

Question: should `chiral gateway serve` also serve local models? That would need a CLI way to load and eject a model on the managed runtime.

Proposed default (what the spec adopts): no. The CLI gateway serves funded network accounts only, over HTTPS or libp2p. It refuses local targets and never starts an engine. Answer only if you want local routes in the CLI; that would be a later change that adds a CLI runtime surface and applies milestone 2's local-route rules.

Related choices in the same change, for your awareness (answer only to change them):
- No command prints the gateway's bearer. `gateway serve` and `gateway show` print only the path of its owner-only file, and regenerating it requires stopping the gateway first.
- `--port` is required and has no default, following "on a user-chosen port" (line 723).
- At most one gateway serves a profile (network plus wallet) at a time. Its state is kept apart from the desktop's `local-server/`. It never holds the consumer lock, so funding and credential commands keep working while it runs.

Not asked again: whether CLI funding should carry the A.1.6 reorganization disclosure is item 3 of *Funding settlement: three questions*. The gateway funds nothing, so either answer leaves this change unchanged.

Nonblocking: no requirement or task waits on this answer.

## 2026-11-17 — Two processes on one consumer profile and its libp2p identity

Status: open

Where: `docs/chiral-book.md`, *Peer-to-peer provider transport* → *SDK and tool compatibility* (line 315: "The native desktop and CLI therefore run a local loopback gateway"). Spec: `specify-p2p-provider-transport` requirement *A consumer libp2p identity runs in one process at a time*, design decision 27 and open question 5; `specify-cli-loopback-gateway` non-goal *Shared DHT identity*.

What it says now: the book gives each consumer one stable, protected libp2p identity and lets both the desktop and the CLI run a loopback gateway. It does not say what happens when both run on the same profile at once. Two live nodes with one peer ID would split identify, Kademlia and hole-punching state, and a fixed `dht.listen` port cannot be bound twice.

Question: while one process (for example the desktop) runs the profile's libp2p node, may a second process (a CLI gateway or `chiral discover`) use libp2p on the same profile?

Proposed default (what the spec adopts): no. An exclusive lock on the identity file admits one node at a time. The second process fails its libp2p operations with a fixed error (503 `transport_unavailable` on HTTP surfaces) and never switches port or identity. HTTPS forwarding and catalog discovery keep working. Answer only to ask for sharing one node between processes, or for a separate identity per process, in a later change.

Nonblocking: no current code depends on it; implementation tasks M3 2.19 and 3.16 follow the default.

## 2026-11-17 — Generation performance: do failures caused by the request count against the provider?

Status: open

Where: `docs/chiral-book.md`, *Performance accounting and withdrawal* (line 571: "Provider-incomplete execution cannot masquerade as success") and the withdrawal latch at line 573. Spec: the new change `specify-generation-performance` (capability `inference-generation-performance`), design decision 3 and open question 1.

What it says now: the whitepaper separates client interruption (not an automatic waiver, line 571) from provider-incomplete execution, which cannot count as success. It does not say how to classify an in-envelope request that fails after admission because of something in the request itself, for example a structured-output schema that the gateway's validation admitted but the engine cannot compile. The code counts every such failure as provider-incomplete: a waived miss in the withdrawal window (`performance.rs:304`). Usage of a failed request is usually unknown, so it costs the consumer nothing. If consumers can trigger such failures, one funded consumer could latch withdrawal of a provider's report with three requests.

Question: should a failure attributable to the request count as a provider miss, be excluded from the window like a client interruption, or be prevented by stricter validation before reservation?

Proposed default (what the spec adopts): it counts as provider-incomplete. The gateway admitted the request and the provider can validate options before reservation; excluding failures would let a provider turn slow requests into failures to avoid withdrawal. Answer only to choose exclusion or a different rule.

Separately, the spec records as a code difference, not a question, that a client disconnect is now waived and counted as a provider miss (task 2.1). Line 571 already says that interruption before a measured miss is not an automatic waiver.

Nonblocking: no requirement or task waits on this answer.

## 2026-11-17 — Fee remittance: what "confirmed" means, and what happens to a failed remittance

Status: open

Where: `docs/chiral-book.md`, *Durable accounting and fees* (line 528: fee obligations are "separately tracked as pending/batched/submitted/confirmed"). Spec: the new change `specify-fee-remittance` (capability `inference-fee-remittance`), design decisions 5 and 7 and open questions 1 and 2.

What it says now: line 528 names the confirmed state and says that recording an obligation is not proof that the platform received it. It does not say what evidence confirms a remittance, or what happens when a remittance cannot be confirmed. `CLAUDE.md` forbids automatic fee bumps and replacements.

Questions:

1. **Confirmation evidence.** Should a remittance count as confirmed under the funding policy `canonical-successful-receipt-at-espace-safe-v1` (successful canonical execution at the healthy `safe` frontier, observed twice, matching the batch's sender, recipient, value and calldata), or does it need something else, such as PoS `finalized`? Proposed default (what the spec adopts): the funding policy, as the code already does. Safe acceptance is not finality, and a confirmed remittance returns to submitted if a reorganization undoes it.
2. **Failed or displaced remittance.** A remittance can fail canonically (for example, a platform wallet that is a contract rejecting the transfer), or its nonce can be consumed without its receipt (`nonce_conflict`). Today its obligations stay unconfirmed in their batch forever and every maintenance pass reports a fee failure; a `nonce_conflict` also blocks every later original of that wallet, including consumer funding that shares the journal, until the receipt returns. Should an explicit operator action re-seal those obligations into a new batch and send a new transaction (paying gas again) once the failure is canonical at `safe`, should they stay as permanently unremitted debt shown in the fee view, or something else? Proposed default (what the spec adopts): no automatic replacement, a failure reported on every pass, and no recovery action until you answer.

Separately, the spec records four code differences, not questions: the fee view reports no "submitted" state and calls confirmed obligations `remitted`; it lists no remittance transactions and omits the "not proof of receipt" statement; aggregation considers only the 1,000 oldest pending obligations per pass; and monitoring can skip some confirmed remittances indefinitely once there are more than 100 (tasks 2.1–2.4).

Nonblocking: no requirement or task waits on these answers; question 2 would add a recovery task.

## 2026-11-18 — Version policy: which operations stay available while an update is required

Status: open

Where: `docs/chiral-book.md`, *Version policy and upgrades* (lines 646–652) and A.2 *Version gate* (line 846: "a *required* update shows a full-screen blocking modal"). Spec: the new change `specify-version-policy` (capability `client-version-policy`), design decisions 4–6 and open questions 1–3. This picks up `specify-desktop-gui-pages` design observation 9.

What it says now: the book says what an upgrade must preserve (line 652) and that a required update blocks the desktop (line 846). It does not say which native operations are refused, or which recovery paths stay open, while an update is required. `CLAUDE.md` requires that "historical funding recovery must remain reachable without abandoning accepted balances", and A.1.7 (line 814) offers pending journaled funding as *Resume*.

Questions:

1. **Recovery under a required update.** Today the desktop's native layer refuses `discover`, `quote`, `fund`, `topup`, `resume`, credential issuance and rotation, `feedback` and inference (`desktop.rs:46–52`, `152–154`, `219`), and keeps funding recovery, funding cancellation, request status and cancellation, credential revocation and local views. The modal covers every page, so a desktop user reaches none of these, cannot log out to load another network's profile, and can only update or quit from the tray. The CLI refuses only `fund`, `topup` and inference (`consumer.rs:984`, `1567`, `2074`), so `chiral resume` still recovers pending funding. A provider refuses every request whose stated version is below its minimum, including owner-authenticated account reads, funding-intent recovery and credential revocation (`app.rs:506–527`). A related case: a consumer profile whose policy cache is corrupt or insecure does not load at all (`consumer.rs:462`), so no recovery runs for that network until the cache is repaired, and deleting the file to repair it discards the rollback watermark. Which operations should stay available, in the clients and at providers? Proposed default: recovery-only operations stay available everywhere: the pending view, funding recovery, `resume` of an original whose signed bytes are already journaled (rebroadcast or settle only, never sign a new payment), funding cancellation, account, usage, receipt and request reads, request cancellation and credential revocation. The modal links to *Funding & recovery* and offers Log out and Quit. New quotes, new funding and top-ups, credential issuance and rotation, feedback and new inference are refused. A profile with a corrupt cache loads with the same recovery-only operations and never rewrites or deletes the cache. The spec adopts only the part both clients already meet (new funding, top-ups and native inference refused; the client's own check never refuses funding recovery, funding cancellation, request status, revocation or local views) and leaves the rest to your answer.
2. **No current policy available.** When the consumer's `policy_url` does not answer, or answers with an expired or rejected policy, the consumer refuses `fund`, `topup` and inference and fetches again on each call; the desktop shows no gate or error. A provider whose own policy has expired admits nothing and answers 400 `invalid_version` to native requests. A relay outage therefore stops native funding and inference with every provider. Should the client (a) fail closed, as today, or (b) keep using its stored authenticated policy until that policy's `valid_until` while the URL is unreachable? Proposed default (what the spec adopts): (a), with recovery as in question 1.
3. **Network binding.** The signed policy fields carry no network identity, so a policy issued for one network verifies on every network that trusts the same authority, and a relay of network B could make network A's newer policy B's watermark. Should a signed policy name its network? Proposed default (what the spec adopts): no; the policy is a statement by the project-wide authority (line 63), and storage keeps each network's watermark separate (line 650). Yes would need a new signed format.

Separately, the spec records four code differences, not questions: the policy signer takes the authority's private key on the command line or from the environment and prints a new key to standard output (book line 474); the required modal does not block the keyboard or assistive technology; "Copy download URL" is offered for any URL, and its fallback points to a non-HTTPS URL; and the gate keeps showing the previous network's status after logout (tasks 2.1–2.4).

Nonblocking: no requirement or task waits on these answers; question 1 would add tasks for both clients, the provider middleware and the desktop gate.

## 2026-11-18 — Opening transaction gas: "intrinsic gas" versus the eSpace estimate

Status: open

Where: `docs/chiral-book.md`, *Agreements and exact funding order*, step 2 (line 483: the journaled opening transaction includes its "terms commitment in calldata, nonce, value, intrinsic gas, and hash"). Spec: `inference-opening-funding-intent` (archived 2026-11-22 from `specify-opening-funding-intent`), requirement *Native preparation durably owns one exact opening transaction* (`openspec/specs/inference-opening-funding-intent/spec.md:10`) and the archived change's `design.md:15`. Code: `src-tauri/src/chain_client.rs:633–654` (`estimate_gas`) and the payment preparation path.

What it says now: line 483 says the signed opening carries intrinsic gas. The spec instead requires the chain's eSpace estimate for the actual payment, at least the transaction's structural minimum and at most 1,000,000 gas, "without treating Ethereum intrinsic gas as a complete eSpace cost estimate". The code follows the spec: it signs `eth_estimateGas` within those bounds. eSpace can charge at least three quarters of the signed limit, so intrinsic gas would understate the cost. Line 491 already requires balance for the full gas limit, so this is wording, not a money-path defect. The wording appears to come from the older migration plan ("London intrinsic gas … when no estimation RPC exists").

Question: may line 483 say "its signed gas limit (the chain's eSpace estimate, at least the structural minimum and at most 1,000,000 gas)" in place of "intrinsic gas"?

Proposed default (what the spec and code already do): the eSpace estimate within those bounds. No spec or code change is planned; only the whitepaper wording would change.

Nonblocking: no requirement or task waits on this answer.

## 2026-11-18 — Profile paths: wording of line 449 and state at the old working-directory location

Status: open

Where: `docs/chiral-book.md`, *Configuration and state* (line 449: "Paths in consumer/provider configurations are resolved relative to their configuration file") and line 472 ("Changing paths or restoring an older layout without deliberate state preservation is not an upgrade strategy"). Spec: the new change `specify-profile-path-resolution` (capability `profile-path-resolution`), design decisions 3 and 7 and open questions 1 and 2.

What it says now: the consumer resolved its relative `data_dir`, RPC bearer file and DHT identity file against the working directory, unlike the provider, relay and RPC gateway. The consumer now resolves them against the profile. A profile whose state exists only at the old working-directory location is refused, with both paths and both remedies named, and nothing is moved.

Questions:

1. **Should line 449 name the relay and RPC-gateway configurations and the command-line exception?** Proposed wording: "Paths inside consumer, provider, relay and RPC-gateway configuration files are resolved relative to that file; paths given as command-line arguments follow the working directory." The spec adopts this reading.
2. **Should a client warn when the old working-directory location also holds state?** When both locations hold this network's state, the profile's own location is used and the other is left untouched without a message. Proposed default (what the spec adopts): no warning, because the user has already acted deliberately.

Nonblocking: no requirement or task waits on these answers.

## 2026-11-18 — May a priced, unprofiled embedding deployment use an external engine?

Status: open

Where: `docs/chiral-book.md`, *Why llama.cpp* (line 73: Rust "starts and supervises a pinned private `llama-server` subprocess … and verifies artifacts before use"), *Native provider provisioning and operations* (line 609: an operator-owned external private llama.cpp endpoint "is available for calibration, not a substitute for managed runtime evidence") and *Listener and secret boundaries* (line 656: an engine endpoint "may be a literal private IP for a protected trusted-network deployment"). Spec: `specify-inference-commercial-terms`, requirement *Native deployment constraints cannot be bypassed by declaration validity*, and its task 2.6.

What it says now: a qualified generation profile now requires a Rust-managed runtime outside development, and an external engine is refused. An embedding deployment carries no speed profile, and the configuration still accepts a priced embedding model on an external engine (loopback or a literal private IP) without a managed runtime. In that case no runtime, artifact or device inventory is verified before the offer is published; only readiness probes run. Book line 656 allows a private-IP engine, but a managed engine is loopback-only, so a private-IP engine can never be managed.

Question: may a non-development, priced, unprofiled embedding deployment be served by an external engine (lines 656 and 609 read as allowing a trusted-network engine), or must every non-development priced model run on a Rust-managed runtime (line 73)?

Proposed default (what the code does today): allow it, because line 656 permits a private-IP engine and embeddings carry no performance claim. If you choose the managed rule, the refusal extends to every priced model outside development.

Nonblocking: no requirement or task waits on this answer.

## 2026-11-18 — Is a provider whose every model is performance-withdrawn ready?

Status: open

Where: `docs/chiral-book.md`, *Immutable terms versus current availability* (line 252: `/readyz` returns its status "based on serving readiness", and "durable performance withdrawal can prevent admission") and *Provider lifecycle and immutable agreements* (lines 633 and 636). Spec: the new change `specify-offer-discovery` (capability `inference-offer-discovery`), requirement *Provider liveness and readiness are point-in-time observations, separate from new-funding readiness*, design decision 7 and open question 1. Code: `app.rs:335–345` and `477–483`; `inference_backend.rs:1370–1397`; `app.rs:63–70` and `353`.

What it says now: the book says `/readyz` reports serving readiness, separate from new-funding readiness, and lists performance withdrawal among the things that "can prevent admission". It does not say whether a provider with no admissible model is serving-ready. Today `ready` is `true`, and `funding_ready` can be `true`, when every configured model passes its readiness probe but carries a durable performance-withdrawal latch: `accepting` requires only that some model passed its probe, and the probe consults the latch only while the engine is busy. Requests and new quotes for those models are refused, and no offer is published for them.

Question: should `ready` (a) stay provider-wide and leave performance withdrawal to admission and publication, as today, or (b) be `false` when no configured model is admissible, counting the latch?

Proposed default (what the spec adopts): (a), by not listing performance withdrawal among the conditions that close `ready`. Option (b) would add that condition and one implementation task. The target-design launch reputation counts a sample as available only with a qualifying published offer, so either answer leaves its samples unchanged.

Separately, the spec records three code differences, not questions: a manifest assembled from pages of different snapshots counts as complete and can withdraw a live offer (comparison item 4); a refresh that fails leaves the offers it published earlier in that pass served; and the DHT identity key file is not a protected secret on Windows and is read through symbolic links on Unix (tasks 2.1–2.3).

Nonblocking: no requirement or task waits on this answer.

## 2026-11-18 — Chain RPC gateway: the relay's unauthenticated RPC route, and the wording of lines 644 and 667

Status: open

Where: `docs/chiral-book.md`, *Native provider provisioning and operations* (line 644: "The standalone RPC gateway has its own inbound bearer and persistent journal, distinct from engine/admin credentials") and *Listener and secret boundaries* (line 656: Caddy TLS ingress in front of "the bounded authenticated chain RPC gateway (`127.0.0.1:8547`)"; line 667: "an exact `eth_*` method allowlist"). Spec: the new change `specify-chain-rpc-gateway` (capability `chain-rpc-gateway`), design decisions 2, 4, 5 and 9 and open questions 1 and 2. Code: `src-tauri/src/bin/chiral_rpc_gateway.rs`, `src-tauri/src/chain_rpc_api.rs:25–38` and `src-tauri/src/bin/relay_server.rs:232`.

What it says now: the gateway served every request without a bearer when bound to loopback, although the generated Caddyfile forwards the public RPC domain to that loopback port (comparison item 7). It now refuses to start without `--bearer-file` on any bind address, with no development exception, and requires an owner-only file of at least 32 visible ASCII characters that differs from the upstream RPC bearer. Requests carrying `Origin` stay refused, and `net_version` was removed from the allowlist because it is not an `eth_*` method and no Chiral client calls it. The relay still mounts the same RPC route on its loopback HTTP listener with no bearer; the reference Caddyfile does not forward it.

Questions:

1. **Should the relay keep its unauthenticated `/api/chain/rpc` route?** Options: (a) remove it, since provider and relay already call the standalone gateway; (b) require a protected bearer and refuse `Origin` there too; (c) keep it as a loopback-only convenience. Proposed default (what this change does): the relay is unchanged until you answer.
2. **May lines 644 and 667 state the bearer rule and the method list?** Proposed wording for line 644: "The standalone RPC gateway requires its own protected inbound bearer file on every bind address, including loopback (at least 32 visible ASCII characters, distinct from its upstream RPC bearer and from engine/admin credentials), and keeps its own persistent journal." For line 667: "exposes exactly `eth_chainId`, `eth_blockNumber`, `eth_gasPrice`, `eth_syncing`, `eth_getBalance`, `eth_getTransactionCount`, `eth_getBlockByNumber`, `eth_getTransactionReceipt`, `eth_estimateGas` and `eth_sendRawTransaction`, with fixed parameter shapes, not arbitrary node administration". This also confirms that `net_version` stays excluded.

Nonblocking: no requirement or task waits on these answers.

## 2026-11-18 — Managed engine launch: what "before startup" requires of fit, and CPU headroom outside development

Status: open

Where: `docs/chiral-book.md`, *Hardware participation and platform status* (line 90: "at least 4,096 MiB headroom" for shared-memory operation versus "at least 256 MiB per dedicated device"; line 100: CPU is a development path) and *Native provider provisioning and operations* (line 617: "Model fit/headroom is checked after actual loading as well as before startup"). Spec: the new change `specify-managed-engine-launch` (capability `managed-engine-runtime`), requirement *Physical devices are observed independently, and headroom is enforced before startup and after loading*, design decisions 6 and 7 and open questions 1 and 2. Code: `src-tauri/src/engine_runtime.rs:32–54` (`EngineRuntimeConfig::validate`), `253–284` (`ManagedEngine::start`) and `349–416` (`observe_devices`).

What it says now: the provider daemon's managed launch is now specified for the current runtime (comparison item 8). Before spawning the engine, the daemon checks that each selected device has free memory of at least the reserve. It does not estimate whether weights, KV cache and buffers fit in free memory minus the reserve. With automatic fitting off, the actual load decides fit, and after the engine reports health the daemon observes the devices again and stops the engine if headroom was lost. A CPU runtime observes no device, has no reserve floor and no memory check, and the configuration accepts it outside development for an unprofiled embedding model.

Questions:

1. **What does "before startup" require of fit?** Options: (a) the headroom check before startup, as today, with the actual load and the post-load check deciding fit; (b) also an estimate of weights, KV cache and buffers against free memory minus the reserve, refused before spawning, as the desktop target design (`desktop-local-runtime`, *Fit is checked before and after loading*) does. Proposed default (what the spec adopts): (a).
2. **What headroom applies to a CPU runtime outside development?** Options: (a) none, as today: CPU carries no capacity tier, headroom or speed claim; (b) a system-memory reserve, for example 4,096 MiB as for unified memory, checked before and after loading; (c) no CPU managed runtime outside development. Proposed default (what the spec adopts): (a). This is related to, but separate from, the entry of 2026-11-18 about priced, unprofiled embedding deployments on external engines.

Separately, the spec records three code differences, not questions:

- the engine key file is not checked against the provider signing key, administration token or RPC bearer, so a configuration naming the signing key file hands the wallet key to the engine (book line 474);
- the occupied-port probe uses address reuse, so on macOS a wildcard listener on the engine port is not detected (book lines 627 and 642). Simply dropping address reuse would also refuse a prompt restart while the old engine's connections are in `TIME_WAIT`;
- the daemon installs its termination handlers only after startup, so on macOS a `SIGTERM` or `SIGINT` while an engine loads orphans the engine (book lines 634 and 642).

These are tasks 2.1–2.3.

A fourth code difference was found on 2026-11-20 by the device-observation tests of task 3.7: a device selector listed twice by the engine, or two `nvidia-smi` rows with one PCI bus ID, were accepted by taking the first match, where the spec requires ambiguous output to fail the launch. It was fixed the same day (design code difference 4).

Nonblocking: no requirement or task waits on these answers; option (b) of either question would add one task.

## 2026-11-18 — Payment-gated reputation: a provider can pre-empt its consumer's vote, and snapshots cannot revalidate many votes in 30 seconds

Status: open

Where: `docs/chiral-book.md`, *Payment-gated subjective reputation*, *Replacement, deduplication, and bounded persistence* (line 413: "one row per quote/account (`quote_id` primary key)") and *RPC failures, reorgs, and what "unavailable" means* (line 439: "Before returning a provider's snapshot, the catalog revalidates stored provider/payer signatures and the opening payments"; line 441: "a 30-second verification/revalidation deadline produces 503 `chain_timeout`"). Spec: the new change `specify-payment-gated-reputation` (capability `payment-gated-reputation`), design decisions 9 and 10 and open questions 1 and 2. Code: `src-tauri/src/protocol.rs:411–413`, `src-tauri/src/rating_storage.rs:74–77`, `src-tauri/src/rating_api.rs:53–83` and `148–175`, `src-tauri/src/chain_client.rs:23`, `540–548`, `561–575`, `608–624`, `808–855` and `918–930`, `src-tauri/src/chain_rpc_api.rs:328–335`, `src-tauri/src/bin/relay_server.rs:195–198` and `210–232`.

What it says now: the current runtime's payment-gated reputation is now specified (comparison item 10). A vote is keyed by the agreement's `quote_id`, and a vote for an agreement that already has one must carry the same opening payment, or it is refused as `feedback_conflict`. Every snapshot revalidates every stored vote of the provider, one after another, within a single 30-second deadline.

Questions:

1. **Quote-ID reuse.** A quote's `id` is a random value that the provider chooses and signs. The provider of an agreement can sign a second quote with the same `id` for a wallet it controls, pay itself a 1-wei opening from that wallet, and vote for that quote as soon as the payment is safe. When the real payer votes later, the stored row has another payment hash and the vote is refused as `feedback_conflict`. Unlike the Sybil limits that line 365 discloses, this removes a real payer's vote, at the cost of gas. Options: (a) keep `quote_id` as the key, as today, and disclose the limitation; (b) key a vote by the quote commitment `Keccak256(P_quote)` that the opening calldata binds, or by `(quote_id, consumer_wallet)`, so that only the funded agreement's own payer occupies its row; (c) another rule. Proposed default (what the spec adopts): (a), because it is the book's text.
2. **Revalidation within one deadline.** Each vote takes two complete chain observations of six RPC calls each, so twelve sequential calls. Eight of them, the block and receipt reads, wait for a 300 ms interval that the relay's whole chain client shares. One vote therefore takes at least about 2.1 seconds even against an instant RPC, and a snapshot can revalidate at most 12 votes with canonical payments within 30 seconds. A provider with 13 or more such votes (the store allows 1,000) can never be read: every snapshot answers 503 `chain_timeout`, and the background pass restarts from the first vote on each attempt, so its later votes are never revalidated. The relay's unauthenticated `/api/chain/rpc` proxy uses the same chain client and pacing and admits 32 concurrent reads, so its callers can push every submission and snapshot past the deadline. Snapshot reads are unauthenticated and each holds one of the four shared verification slots for up to 30 seconds, so four concurrent reads also make submissions answer 429 `feedback_busy`. Options: (a) as today; (b) serve a snapshot from each vote's most recent revalidation when every vote was revalidated successfully within a stated bound (for example by a background pass that revalidates vote by vote and keeps its progress), and answer 503 only when one is older than the bound or was unavailable; (c) another rule. Proposed default (what the spec adopts): (a), because it is the book's text.

Separately, the spec records four code differences, not questions: malformed submissions get axum's plain-text 415, 400 or 422 instead of `invalid_feedback`, and a valid submission followed by trailing content is accepted (comparison item 6); store faults are answered `feedback_conflict` with database text; the native client reports every catalog answer, including a catalog outage, as "Provider operation failed"; and an RPC identity mismatch or a receipt that contradicts the signed transaction counts as a rejected payment, so revalidation marks votes ineligible and serves a snapshot instead of 503 (tasks 2.1–2.4).

Nonblocking: no requirement or task waits on these answers; option (b) of either question would add one task, and option (b) of question 1 a store migration.

## 2026-11-19 — Outbound HTTP safeguards: chain RPC proxies, and whether the origin rule governs the policy source

Status: open

Where: `docs/chiral-book.md`, *HTTP origins and trust boundaries* (lines 262–279), and the configuration rows for `network.rpc_url` (line 458) and `policy_url` (line 460). Spec: the new change `specify-outbound-http-safeguards` (capability `outbound-http-safeguards`), design *Authority and scope* and open questions 1 and 2. Code: `src-tauri/src/rpc_client.rs:11–19`, `src-tauri/src/network.rs:66–72` and `110–131`, `src-tauri/src/consumer.rs:775–788`.

What it says now: the current runtime's outbound HTTP boundary is now specified (comparison item 11). It has eight requirements: origin validation, the development exception, origins as advertised endpoints, the per-origin DNS-pinned client with no proxies or redirects, the exact public-address predicate, the loopback administration listener, the private engine origin, and TLS ingress. Line 264 names provider and catalog origins. Line 268 disables environment proxies for them, and line 275 disables them for the private engine. Line 458 gives the chain RPC only a scheme rule, and line 460 gives the policy source the profile's endpoint rules.

Questions:

1. **Should the chain RPC client ignore environment proxies?** Today `rpc_client` honours them. With a literal-loopback `http://` RPC URL and `HTTP_PROXY` set, every JSON-RPC request goes to the proxy in cleartext, including the `rpc_bearer_file` bearer. Options: (a) keep honouring proxies, since an operator may need one to reach a remote HTTPS node; (b) ignore environment proxies for all chain RPC, as for provider and catalog origins; (c) ignore them at least for literal-loopback HTTP RPC. Proposed default (what the spec adopts): (a). The new capability does not cover the chain RPC client; if (b) or (c) is chosen, it belongs to `inference-funding-settlement` *Chain RPC transport is bounded*.
2. **Does the provider/catalog origin rule also govern the version-policy source?** The code fetches the policy through the pinned client of its origin, so a policy source must already be public HTTPS on port 443 (or loopback in development). Profile load, however, accepts any HTTPS URL with a path. Options: (a) make the origin rule and the pinned client normative for the policy source, as a change to `client-version-policy` *Policies come only from configured sources*; (b) leave it as current behaviour without a requirement. Proposed default (what the spec adopts): (b).

Separately, the spec records two code differences, not questions (tasks 2.1 and 2.2):
- a development `localhost` origin accepts a non-loopback DNS answer and pins it (`offer_registry.rs:216–217`), so a resolver that answers `localhost` with a remote address receives cleartext HTTP;
- credential export derives the SDK `base_url` without the origin validator (`consumer.rs:2222–2226`), so an account saved under rules that no longer validate it still returns its API secret with a `base_url` that every native request would refuse.

Nonblocking: no requirement or task waits on these answers; option (b) or (c) of question 1, or option (a) of question 2, would add one task to another change.

## 2026-11-19 — Account credentials: omitted spend cap, rotation restoring a missing default, and listing completeness

Status: open

Where: `docs/chiral-book.md`, *Consumer control, credentials, and request lifecycle* (lines 325–360, chiefly line 345 on issuance options and storage) and the recovery line 495 ("issue or rotate"). Spec: the new change `specify-account-credentials` (capability `account-credentials`), design open questions 1–3. Code: `src-tauri/src/billing_store.rs:43–50` and `839–846`, `src-tauri/src/consumer.rs:2122–2138` and `2168–2171`.

What it says now: credential issuance and secret storage in the current runtime are now specified (comparison item 12). Nine requirements cover:
- the four-member options object;
- hash-only provider storage with a public identifier separate from the secret;
- bearer scopes and lifetime on the inference routes;
- revocation and rotation of one named credential;
- caps as limits on the same balance, never credit;
- credential management that stays reachable without new admission;
- the protected native store;
- metadata-only command output except explicit export;
- default selection on rotation.

Questions:

1. **May `spend_cap_wei` be omitted?** Today a missing member decodes as `null`, meaning no cap. Options: (a) require all four members, with `null` the only way to ask for no cap; (b) accept omission as `null`. Proposed default (what the spec adopts): the book text. Requirement 1 names the four members and allows `null` without deciding omission.
2. **Does rotating a credential that is not the recorded default restore a missing default?** After a native revocation of the default, the default pointer is cleared. Rotating that revoked credential then leaves no default, while an issuance would restore one, although line 495 offers "issue or rotate" as equal remedies. Options: (a) a rotation with `infer` restores a missing default exactly as issuance does; (b) only rotating the recorded default does (current behaviour). Proposed default (what the spec adopts): (b), because requirement 9 covers rotation of the recorded default only.
3. **Must `GET /credentials` list every credential that can still authenticate?** Today it returns the 100 newest records in any state. An older active credential, such as one whose secret was lost, can drop out of the list, and its owner then cannot find it to revoke it. Options: (a) list every active credential (at most 64), then recent inactive ones; (b) keep the 100 newest records. Proposed default (what the spec adopts): the book text, which states no bound or completeness rule.

Separately, the review notes that requirement 1 refuses unknown and repeated members, reading the book's `{expires_at, spend_cap_wei, max_concurrency, scopes}` as a strict object. That follows the precedent of the opening-request and owner-challenge specs. The spec also records two code differences, not questions (tasks 2.1 and 2.2):
- the options body also decodes from a positional JSON array, unlike the sibling bodies that use `protocol::deserialize_object`;
- an explicit native issuance can replace a still-valid default once that default falls outside the provider's bounded list of the 100 newest records (task 2.2 is needed whichever answer question 3 gets).

Nonblocking: no requirement or task waits on these answers. Option (a) of question 1 or 2, or option (a) of question 3, would add one task each.

## 2026-11-19 — Public inference API: the `streaming` capability, tools on managed deployments, embedding dimensions and busy-model probes

Status: open

Where: `docs/chiral-book.md`, *API compatibility and model qualification* (lines 579–601), with lines 530, 591 and 628. Spec: the new change `specify-public-inference-api` (capability `public-inference-api`), design open questions 1–4. Code: `src-tauri/src/inference_backend.rs:250–265`, `317–319`, `738–795` and `1370–1397`, `src-tauri/src/engine_runtime.rs:152`, `src-tauri/src/inference_gateway.rs:578–598` and `825–852`.

What it says now: the provider's public OpenAI-compatible surface and per-capability qualification are now specified (comparison item 13). Twelve requirements cover:
- the implemented endpoint set, with no engine or out-of-scope route;
- both modes for stock clients;
- credential-scoped listing;
- admission only within the contracted model's capabilities;
- body bounds;
- honest failures;
- GGUF capability derivation;
- readiness probing every advertised capability;
- exact echo with no invented prompt probabilities;
- matching embedding dimensions;
- tools and structured output as data that is never executed;
- whole-model text inference only.

Questions:

1. **Does SSE on a generation route require the `streaming` capability?** Options: (a) serve SSE only for models advertising `streaming`, derived by provisioning; (b) every chat or completion model serves SSE, and `streaming` is informational, as today. Proposed default (what the spec adopts): the book text. Lines 587–588 require both modes for chat and completions, and line 595 requires an advertised `streaming` to be probed.
2. **How may `tools` or `structured_output` be offered on a managed deployment?** Launch requires the configured capabilities to equal the artifact's derived ones, so neither can be added today. Options: (a) a separate qualification step that extends the signed capabilities with probe evidence; (b) not offered in this release. Proposed default (what the spec adopts): the book text. Line 601 requires separate qualification, never a model card, but specifies no procedure.
3. **Should the native embedding dimension be signed or listed?** Options: (a) add it to the signed model description (a wire change); (b) report it in `/v1/models` after readiness measures it; (c) neither, as today. Proposed default (what the spec adopts): (c). Line 599 does not advertise it, and task 2.2 learns it from readiness.
4. **Must readiness repeat inference probes while a model stays busy?** Today a busy ready model repeats only identity and template checks. Options: (a) reserve or schedule probe capacity within a bound; (b) as now. Proposed default (what the spec adopts): the book text. Probes gate readiness (lines 591 and 628), but no cadence is stated.

Separately, the spec records four code differences, not questions (tasks 2.1–2.4):
- an advertised `streaming` capability that no probe exercises still lets an operator-owned embedding deployment become ready and publish;
- a requested embedding `dimensions` the deployment does not produce is refused only after dispatch, as an engine error at the provider's loss, and counts toward the account's hourly unknown-usage bound;
- a failed stream whose error event cannot be queued, because the client stopped reading, ends like a normal stream;
- a prompt the engine tokenizes to nothing is refused as `context_length_exceeded`.

Difference 2 depends partly on question 3. The review also notes that the code may hash the raw engine template while provisioning uses the normalized digest; only a pinned-engine run can settle that (task 4.1).

Two more code differences were found on 2026-11-20 by the acceptance tests of tasks 3.8 and 3.9 and fixed the same day (design code differences 5 and 6): the qualification runner's paid echo probe accepted log-probabilities covering the prompt or offsets from the prompt's start, and readiness accepted a streamed probe that returned none of the log-probabilities it asked for. Whether the pinned engine returns them for the probes' one generated token is part of task 4.1.

Nonblocking: no requirement or task waits on these answers. Option (a) of questions 1, 2 or 4, or option (a) or (b) of question 3, would add a task.

## 2026-11-20 — Provider daemon lifecycle: drain across restarts, reopening after a settlement failure, startup publication, a lost ledger, automatic restarts and hang-up signals

Status: open

Where: `docs/chiral-book.md`, *Native provider provisioning and operations* (lines 605–680), chiefly lines 623, 627, 630–634, 642, 667, 673 and 675. Spec: the new change `specify-provider-daemon-lifecycle` (capability `provider-daemon-lifecycle`), design open questions 1–6. Code: `src-tauri/src/app.rs:128–152`, `219`, `356–362`, `588–601` and `631–647`; `src-tauri/src/inference_gateway.rs:381–427`; `src-tauri/src/bin/chiral_daemon.rs:36–66` and `153–181`; `docker-compose.yml:10`.

What it says now: the provider daemon's lifecycle is now specified (comparison item 14). Its twelve requirements cover:
- separate status facts;
- runtime observation as liveness, not readiness;
- operator drain;
- resume and refresh only through a complete maintenance pass;
- the ordered maintenance pass;
- a settlement-failure stop that waits for the operator;
- startup refusals;
- one daemon per deployment;
- the ledger's own schema, network and provider;
- no re-terming of agreements on restart or configuration change;
- an orderly shutdown;
- content-free logs and diagnostics.

Backup and cutover procedures are recorded as operator procedures, not daemon behaviour.

Questions (the spec adopts each proposed default):

1. **Does an operator drain survive a restart?** Options: (a) no, it is state of the running daemon (as now); (b) a durable drain that only `resume` clears; (c) a start-drained option. Proposed default: (a), the book text.
2. **What may reopen admission after a durable settlement failure?** Options: (a) only `POST /admin/resume` or a restart; (b) a pass that proves the ledger can be written; (c) any pass (as now). Proposed default: (a), from lines 630 and 677. This is task 2.1.
3. **Is a publication failure in the starting pass a failed startup?** Options: (a) no: start, serve existing accounts and report it, as after the same failure later; (b) yes, exit (as now). Proposed default: (a), from lines 623 and 630. This is task 2.5.
4. **Should a missing ledger be refused when other deployment state remains?** If `ledger.sqlite3` is lost while the offer revision and DHT identity remain, the daemon creates an empty ledger and earlier consumers' bearers stop working. Options: (a) refuse unless the operator explicitly confirms; (b) create it, leaving line 675 to operator procedure (as now). Proposed default: (b), the book text.
5. **May deployment machinery restart the daemon automatically?** The reference Compose service uses `restart: unless-stopped`. Options: (a) yes, as a restart under line 634 with startup recovery (as now); (b) no, the operator reconciles first (line 632). Proposed default: (a).
6. **Is a terminal hang-up or a Windows console event a normal shutdown?** Options: (a) every interceptable termination request takes the shutdown path, including `SIGHUP`, Ctrl-Break and console close; (b) only `SIGTERM`, `SIGINT` and Ctrl-C (as now). Proposed default: (a), because line 642 allows a macOS engine orphan only after `SIGKILL`. This is task 2.2.

Separately, the spec records nine code differences, not questions (tasks 2.1–2.9):
- a settlement failure closes admission only until the next pass and is never reported;
- `SIGHUP`, Ctrl-Break and console close skip the shutdown path, so on macOS the engine is orphaned;
- `/admin/status` omits the drain and the performance latch;
- `/admin/status` fails as a whole when the ledger summary cannot be read;
- a publication failure in the starting pass aborts startup;
- the deployment lock is taken after the DHT node joins under the deployment's identity;
- `/admin/runtime` reports engine liveness as `ready`;
- an engine exit keeps a "Ready" status detail until the next pass;
- a resume racing shutdown can reopen admission after the shutdown drain.

A tenth code difference was found on 2026-11-20 by the acceptance tests of task 3.1: while a maintenance pass held the lifecycle lock, `/admin/status` still reported a model whose managed engine had exited as ready. It was fixed the same day (design code difference 10).

Nonblocking: no requirement or task waits on these answers. A different answer to question 2, 3 or 6 would change task 2.1, 2.5 or 2.2. Option (a) of question 4, or (b) or (c) of question 1, would add a task.

## 2026-11-20 — Application startup: may the desktop ever start at login if it starts no node or miner?

Status: open

Where: `docs/chiral-book.md`, Part I line 31 ("The application never starts a node or miner on login"), with lines 17, 777 and 793. Spec: the new change `specify-no-background-start` (capability `application-startup-boundaries`), design open question 1. Code: none differs (see below).

What it says now: the application's startup boundaries are now specified (comparison item 16). Five requirements cover:
- the application runs no chain node or miner;
- login and session start launch no node, miner or legacy service;
- legacy data stays in place and is never imported automatically;
- Core-space funds become usable only through the user's explicit transfer to eSpace;
- native provisioning starts no service.

The code search found no difference. There is no autostart plugin, launch agent, Run key, XDG autostart entry, systemd unit or scheduled task. The desktop setup only installs the tray. The only process spawns are the provider daemon's engine launch and device observation. The chain client sends only `eth_*` methods.

Question:

1. **May the application register itself to start at login when that start launches no node, miner or legacy service?** Line 31 forbids only starting a node or miner on login. Today nothing is registered, and the archived window-lifecycle and tray specs, like the target-design `desktop-share-online`, leave start-at-login out of scope. Options: (a) follow the book text: only a login start of a node, miner or legacy service is forbidden, and any desktop start-at-login needs its own change; (b) allow no start-at-login entry of any kind in this release. Proposed default (what the spec adopts): (a).

Nonblocking: no requirement or task waits on the answer; option (b) would add one requirement. There are no code differences; acceptance evidence is tasks 3.1–3.5 and 4.1.

## 2026-11-20 — Owner-challenge rate: should a ledger left inconsistent by the old limiter be repaired automatically?

Status: open

Where: `openspec/specs/inference-challenge-rate-budgets/spec.md` (archived from `specify-challenge-rate-budgets` in round 75), requirement 2 ("Unreadable or inconsistent retained rate evidence SHALL refuse fresh issuance") and design decision "Use conservative validation before issuing"; code `billing_store.rs::challenge_rate_limit` (round 74, tasks 2.4 and 2.5). The book (`POST /v1/auth/challenge`, "Public bounded challenge issuance") does not fix this.

What happens now: owner challenges have a durable, non-regressing minute, like the quote clocks. Retained counters are adopted, and evidence that contradicts itself refuses fresh issuance with an internal `ledger invariant` error, not 429. Existing proofs, bearers and accounts keep working.

The review found one way the old limiter itself could leave such rows. It sampled time before taking the ledger write lock, so two challenges on either side of a minute boundary could commit out of order. That left a wallet row ahead of the ledger counter, or a counter below the sum of its minute's wallet rows. The old limiter cleared this at the next later-minute operation. If a provider stops in that state and restarts on the new code, owner challenges stay refused until an operator restores consistent `challenge:*` rows by hand. No repair command exists. The case needs concurrent challenges across a minute boundary in the old daemon's last active minute.

Question:

1. **Should the new limiter adopt the old limiter's out-of-order shapes instead of failing closed?** Options: (a) keep failing closed, as the spec says, and document a manual repair; (b) adopt them conservatively: take the greatest retained minute and charge the larger of its ledger counter and the sum of its wallet rows, which never grants allowance the evidence does not show; (c) add an operator repair command that resets only `challenge:*` rows. Proposed default (what the code does): (a).

Nonblocking: no task waits on the answer. Option (b) changes one requirement statement and the adoption tests; option (c) adds one operator task.
