## Why

The read-only whitepaper (`docs/chiral-book.md`) defines a signed client-version policy. Lines 646–652 require a non-placeholder `CHIRAL_POLICY_PUBLIC_KEY` in release builds, a runtime override that is resolved once per process and falls back to the compiled key, rollback protection "under protected storage, scoped to authority and network" that a fresh process cannot bypass, a watermark kept across restart and expired-policy refresh, caches that fail closed when corrupt or insecure, a placeholder authority for private development only, an authority key kept offline, and upgrades that preserve accepted state. Line 846 adds the desktop gate: a full-screen blocking modal for a required update, with "Copy download URL" for an HTTPS URL only, and a banner for a recommended update whose dismissal lasts for the app session. Supporting lines put the policy on the relay (43), make the authority explicit (63), keep the version header optional for stock clients (343, 581), make `policy_url` a consumer profile field (460), forbid private keys on command lines (474), treat the policy as a provider serving gate (630), include the policy cache in backups (675), start the managed daemon with an offline-signed policy (739), and list the authority and rollback protection as release gates (769, 774). `CLAUDE.md` adds: "Version-policy acceptance is signed, network/authority-scoped, durable, and monotonic across processes."

No existing or pending specification covers any of this. `specify-desktop-gui-pages` recorded (design observation 9) that "The version gate is not specified by any change" and that the book does not say which recovery paths stay open while an update is required. Without a specification, a later change could weaken rollback protection, the fail-closed cache or the gate without a failing requirement.

## What Changes

- Add the `client-version-policy` capability with 14 requirements and 47 scenarios. Each requirement cites its whitepaper lines and comes from the whitepaper or `CLAUDE.md`, not from current code. Values that the book does not state are recorded in `design.md` as current implementation values.
- Specify the release-build authority, and the runtime override resolved once per process with fallback to the compiled key.
- Specify what makes a policy acceptable (signature over every field, numeric versions, time bounds) and the three statuses `required`, `recommended` and `ok`.
- Specify the rollback watermark: written durably before use, owner-private, scoped to the complete network identity and the authority, read from storage by every fresh process, monotonic across concurrent processes, and kept through expiry.
- Specify that corrupt or insecure caches fail closed, and that the placeholder authority verifies nothing and cannot persist, tighten or displace a policy, so that a public daemon or relay cannot run on it.
- Specify custody of the authority's private key: a protected file only, never argv, the environment or standard output.
- Specify the policy sources (relay file, daemon `--version-policy`, consumer `policy_url`) and the client-version header that providers check only when present.
- Specify what a required update, or a policy that cannot be checked, refuses on the native client (new funding, top-ups and native inference) and what the client's own check never refuses (funding recovery, funding cancellation, request status, credential revocation and local views). Leave the rest to an author question.
- Specify the provider's own policy as a serving gate, the desktop gate (blocking for keyboard and assistive technology, HTTPS-only copy, session-long dismissal, no status from a previous network), and preservation of state and the watermark across upgrades.
- Record a coverage table in `design.md` (requirement → code → existing tests, or the missing case). Record code differences and missing acceptance evidence as unchecked tasks.

## Capabilities

### New Capabilities

- `client-version-policy`: The policy authority (compiled and overridden), policy acceptance and statuses, the durable scoped rollback watermark, expiry and fail-closed caches, the placeholder authority's limits, custody of the authority key, policy sources, the client-version header, native and provider enforcement, the desktop gate, and preservation across upgrades.

### Modified Capabilities

None. This capability references the normative `desktop-window-lifecycle` capability and the pending `desktop-gui-global-rules` and `inference-request-accounting` capabilities. It adds no obligation to them and does not change their requirements. It is consistent with the pending `desktop-share-online`, `inference-funding-settlement`, `desktop-local-models` and `desktop-local-api-server` changes.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/build.rs` and `src-tauri/src/policy_key_validation.rs`: the release-build key;
  - `src-tauri/src/version.rs`: the policy, its encoding and verification, the override, acceptance, statuses and the persisted watermark;
  - `src-tauri/src/bin/chiral_policy_sign.rs`: the offline signer;
  - `src-tauri/src/bin/relay_server.rs` and `src-tauri/src/bin/chiral_daemon.rs`: loading and serving the policy;
  - `src-tauri/src/app.rs`: the header check, `/api/version-policy` and the provider's serving gate;
  - `src-tauri/src/consumer.rs`: `version_status`, `ensure_version_supported` and the version header;
  - `src-tauri/src/desktop.rs`: `ensure_supported`, `get_version_status` and `logout`;
  - `src/lib/stores/versionStore.ts`, `src/lib/components/UpdateGate.svelte`, `src/App.svelte` and `src/pages/Settings.svelte`: the desktop gate.
- Four code differences were found. Tasks 2.1–2.4 track them:
  - the signer takes the authority's private key on the command line or from the environment and prints a new key to standard output;
  - the required-update modal does not block the keyboard or assistive technology;
  - "Copy download URL" is offered for any URL, and its fallback points the user to a non-HTTPS URL;
  - the gate keeps showing the previous network's status after logout, and after a profile load whose status check fails.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.14. The existing evidence is 34 unit tests in `version.rs`; no HTTP, consumer, daemon or relay startup, CLI or GUI test covers the policy.
- Release and live evidence (the published authority, rollback protection on real desktops and services, the gate on three operating systems, and an upgrade drill) is task 4.1. It needs an authorized private deployment and is never satisfied by fixtures.
- Three questions for the whitepaper author are recorded in `design.md` and in one entry of `docs/apas-worker/report.md`: which operations stay available while an update is required, what happens when no current policy can be obtained, and whether a signed policy should name its network.
- No wire format, storage layout, deployment or whitepaper change is part of this specification step. This change grants no release, public-testnet or live-network qualification.
