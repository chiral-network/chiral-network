## Why

On 2026-11-16 the author designed how a new provider earns the standing to charge (book `docs/chiral-book.md`, commit `fe473508` plus the uncommitted 2026-11-16 edit, working-copy SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`, *Author decisions* items 1–11, whose items 8–11 replace the earlier items 8 and 9):

- A new desktop share starts with a **launch window**: at least 24 hours of free service until the provider is ready to charge, with qualification run automatically in the window. Free offers sign a per-account **free allowance** (default 20,000 tokens and 24 hours).
- After the window, an **Auto-price** checkbox, on by default, moves the price every 6 hours in steps of 25% of a target, between a floor (default free) and the target, driven by reputation.
- Free sessions still cannot vote, because a free vote costs nothing to fake. Instead, a catalog operator's observer publishes **launch reputation**: availability for every provider, and, only while a provider is free, **probes** — small real requests from rotating, unlinkable free accounts — that check correct service and **billing honesty** (reported prompt tokens against the declared tokenizer). The **service score** is 100 × availability × probe success over 14 days, and **ready to charge** needs at least 24 hours observed and 100 probes, availability ≥ 95%, probe success ≥ 98%, no billing mismatch and a valid qualification.
- It is a **signal, not a gate**: the provider's Auto-price and consumers read the same figures. *Status and milestones* item (5) makes this milestone 5, which "must exist before launch windows or Auto-price are presented as available".

The earlier draft of this change (as `specify-provider-track-record`) specified only the availability observer. Nothing observes or probes providers today. `relay_server` refreshes a verified offer registry and serves payment-gated feedback, native inspection takes one point-in-time readiness probe, and `/readyz` does not say which wallet or peer it serves.

An observer that dials provider-supplied addresses, opens accounts and downloads tokenizer metadata named by providers adds risks — SSRF and traffic amplification, identity resets, gaming of the figures, false billing accusations and untrusted GGUF parsing — so the contract has to be written before implementation.

## What Changes

- **One observer, two kinds of evidence.** The catalog operator's `relay_server` runs the observer only with an explicit strict `launch_reputation` object (formerly `track_record`): 1–8 reference peers, 1–4 DNS-named reference origins for its health checks, and a `tokenizer_runtime` directory holding the pinned llama.cpp runtime for billing checks. Every schedule, bound, corpus and threshold is a fixed constant so catalogs are comparable. Consumers and providers never take or submit evidence.
- **Availability (kept from the earlier draft, reframed).** The availability observer is unchanged in substance: a dedicated observer node with a rotating, unpublished key; identities learned only from verified discovery (512 at a time, 8 per wallet, dormant-only eviction, a refusal table, 14-day inactivity in observer-healthy time); memoryless samples (mean 300 s, at most 24 per hour, 30 s deadline); resource bounds that never let a provider cause an exclusion (64 in progress, fixed per-sample plans, verified-address exemption, 12 unverified attempts per minute per destination, third-party relay tokens); available, unavailable, unknown and skipped classes; HTTPS and libp2p rules; `/readyz` identity members; wallet and peer sections with coverage. One change: a sample keeps reading pages of a charging identity's manifest until it finds a free offer or the manifest ends, so a return to free is detected.
- **Pricing state and free periods.** Each identity is `unknown`, `free` (a verified free offer was seen) or `charging` (a complete manifest read found only priced offers). Going offline never ends a free period. A wallet's free periods are recorded.
- **Probes, only while free.** For each free identity: memoryless probe times (mean 600 s, at most 12 per hour, at most one draw waiting per identity), 64 in progress, a 60-second deadline, verified addresses only. Each probe account has a fresh secp256k1 wallet key and libp2p key from the OS CSPRNG, held in memory only, never reused across accounts or providers, holding no CFX; free accounts need no payment. An account serves at most 12 probes, for at most `min(2 h, lifetime/2)`, never beyond its signed allowance and retired at half of it. Prompts come from a fixed public corpus of 64; the endpoint is `completions` when advertised, otherwise `chat`, otherwise `embeddings`; kinds are streaming (1/2), non-streaming (1/4), cancellation (1/8) and limit (1/8).
- **Probe classes.** Succeeded, failed, busy (retryable admission refusals, including the automatic-qualification pause), unreached (already counted by availability), unknown and skipped. Only succeeded and failed enter probe success. Latency to first content is recorded in buckets and shown, never scored.
- **Billing honesty.** The catalog fetches at most a 32 MiB metadata prefix of the declared GGUF at its pinned revision from Hugging Face, under the desktop's Hugging Face client rules, with a bounded cache and fetch rate. A confined subprocess of the pinned runtime tokenizes the exact request, protected by a golden-count self-test. A mismatch raises a *Billing mismatch* flag for 14 days, with reproducible evidence. An unresolvable model, missing template or failed tokenizer is `unchecked`, shown as "Billing unchecked", never a mismatch, and does not block the launch criteria. Probes make no model-identity claim.
- **Results.** The record adds probe counts and success, `latest_probe_at`, billing outcomes, first-content buckets, `free_now`, up to 16 completed free periods, the probe basis (the window while free; while charging, the book's rule: the most recent free periods, newest first, until at least 100 counted probes), `service_score_bps`, the active flag, and `launch_criteria` `{met, unmet}`, which is the book's *ready to charge* without qualification. For an embeddings-only share, `launch_criteria.met` is the whole readiness Auto-price reads, over embeddings probes and input-token billing checks (an implementation decision the author may revisit). The catalog does not assess qualification; Auto-price and consumers check it from the provider's evidence and the signed offer.
- **Storage.** The same fail-closed, network-bound SQLite store, extended with probe, billing, pricing and free-period data. It holds no keys, credentials, account IDs, addresses or generated content.
- **API (renamed).** `GET /api/launch-reputation/:wallet/:peer_id` and `POST /api/launch-reputation/query` (1–64 identities), with errors `launch_reputation_busy` and `launch_reputation_unavailable`, and a `/api/health` summary. `GET /api/ratings/:wallet` gains one additive member, `votes_14d`, for Auto-price's payment-gated rule. Elo and feedback admission are unchanged.
- **Native and Marketplace.** `provider_info` gains `launch_reputation`, and a batch `launch_reputations` operation serves cards, both with full recomputation checks. Cards show, per A.4, *Launching — free*, the service score, 14-day availability, the probe count with its age, *Billing mismatch*, "Sharing since" or "No track record yet", beside "Reputation: see inspection". Inspection gets a Launch reputation tile beside, never blended into, Elo.
- **Reputation boundary.** Free sessions, including probe accounts, still cannot vote; the native cannot-vote refusal and the Usage statement remain.

## Capabilities

### New Capabilities

- `provider-launch-reputation` (renamed from `provider-track-record`): the catalog-operated observer and everything it publishes. It covers:
  - observer placement, identity and configuration;
  - tracked identities and their bounds;
  - availability sampling, resource bounds, classification, health, and the HTTPS and libp2p rules;
  - the provider readiness identity binding;
  - pricing state and free periods;
  - probe eligibility, identities, accounts, schedule, corpus, kinds and classification;
  - billing honesty;
  - availability results, probe results, the service score and the launch criteria;
  - storage, the read API, and `votes_14d`;
  - separation from payment-gated reputation;
  - native inspection and Marketplace display;
  - privacy, gaming resistance, and SSRF and amplification resistance.

  One capability was chosen over a separate `provider-service-probes`. Probes share the observer's health check, identities, verified addresses, relay budgets, store, record, API and display, and the service score multiplies the two figures. A split would make every one of those requirements cross-reference between capabilities without separating any owner.

### Modified Capabilities

None.

- Payment-gated reputation has no OpenSpec capability yet. This change keeps its book contract, and specifies its one additive snapshot member in its own requirement rather than creating a reputation capability.
- `inference-commercial-terms` supplies offer validation, the free allowance (`free_allowance` `{tokens, lifetime_secs}`, pending in `specify-p2p-provider-transport`) and quote terms, and is relied on, not changed.
- `p2p-provider-transport` (pending in `specify-p2p-provider-transport`) supplies peer-ID authentication, the ladder, circuit classification, the circuit-budget check and relay limits. It is relied on conditionally, not changed.
- `desktop-model-acquisition` (pending in `specify-desktop-local-models`) supplies the Hugging Face client rules that tokenizer fetches follow.
- `desktop-share-online` (pending in `specify-desktop-share-online`) supplies the retryable busy response of an automatic qualification run, and consumes this capability for Auto-price.

## Impact

- **Native (planned):**
  - `src-tauri/src/bin/relay_server.rs`: the `launch_reputation` object, observer startup and fail-closed store, the routes, the `/api/health` summary, and the tokenizer runtime check.
  - New modules for the observer node, scheduler and resource bounds, health monitor and classifier, store, probe identities and accounts, probe runner and classifier, and the billing checker (GGUF prefix fetch, bounded reader and confined tokenizer process).
  - `src-tauri/src/dht.rs`: filtered address reads, discovery verification results with pricing state, targeted manifest fetches on explicit dials, the DHT health query, and after M3 the catalog relay's observer and probe-identity exemption.
  - `src-tauri/src/app.rs`: identity members in `/readyz`.
  - `src-tauri/src/rating_api.rs` and `rating_storage.rs`: the additive `votes_14d`. `reputation.rs` and the Elo calculation are unchanged.
  - `src-tauri/src/consumer.rs`, `desktop.rs` and `src/lib/native.ts`: `launch_reputation`, `launch_reputations`, validation, the readiness identity match and the cannot-vote check.
- **Frontend (planned):** `src/pages/Marketplace.svelte` (card lines, batching, Launch reputation tile) and `src/pages/Usage.svelte` (the cannot-vote statement). No new webview permission.
- **Repository data (planned):** the versioned probe corpus file and the golden tokenizer counts.
- **Operator tooling (planned):** `scripts/provision-provider.py` adds `/api/launch-reputation` and `/api/launch-reputation/*` to the relay ingress allowlist and accepts the `launch_reputation` object. A relay host that runs the observer needs the pinned runtime installed with `scripts/provision-runtime.py`, outbound HTTPS to Hugging Face, and OS confinement for the tokenizer process.
- **Dependencies:**
  - **M3 (`specify-p2p-provider-transport`).** It supplies the free allowance schema (v2 offers and quotes on `/chiral/inference/manifest/2`), the contract and inference routes on `/chiral/inference/http/1`, and relayed paths. Before M3 is implemented, there is no free allowance and every offer has an HTTPS endpoint, so availability works over HTTPS and `/1`. Probes need M3's free allowance, and libp2p-only providers, including every desktop share, can be sampled and probed only after M3.
  - **M4 (`specify-desktop-share-online`).** It consumes the API for Auto-price: `launch_criteria`, `free_periods[0].launch_criteria_met_at_end`, `service_score_bps`, the wallet's `availability_14d`, and `elo` with `votes_14d` from `/api/ratings/:wallet`, with qualification checked by M4 itself. M4 records this change's implementation and qualification as a release dependency for presenting launch windows or Auto-price as available (book milestone 5).
  - **M1 (`specify-desktop-local-models`).** Its Hugging Face client rules and its artifact contract's model ID form are reused.
  - **Providers first.** Providers should be upgraded with `/readyz` identity members before a catalog enables the observer; otherwise their samples are unavailable and their probes unreached.
  - **Archive order.** This change modifies no pending capability and can be archived independently of M1–M4.
  - **Implementation order:**
    1. `/readyz` identity on providers;
    2. the HTTPS availability observer;
    3. M3;
    4. libp2p observation, probes, billing checks and the coverage qualification;
    5. M4's launch window and Auto-price presented as available.
- **Documentation:** `docs/chiral-book.md` and `CLAUDE.md` are read-only for this change. Task 2.11 proposes wording to the author after implementation. That includes the `CLAUDE.md` relay boundary: the relay would send its own corpus prompts as a free consumer, while still never proxying or carrying anyone else's prompts or inference.
- **Deployment:** enabling the observer on any live catalog, including the historical `130.245.173.73` relay, is a deployment that needs its own authorization, cutover and monitoring. Nothing here authorizes it.
- **Out of scope:**
  - per-model or slot availability, and speed or throughput measurement;
  - proof of which model ran, and checking completion-token counts;
  - cross-catalog replication or consensus;
  - ranking, routing, slashing or any gate from these figures;
  - consumer-reported uptime;
  - ratings for free service;
  - the Auto-price controller and the share dashboard, which belong to M4;
  - display of a free offer's allowance on the offer card, which belongs to the offer's owner (M3 and M4).
