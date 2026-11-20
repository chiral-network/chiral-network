## Why

The read-only whitepaper (`docs/chiral-book.md`) says on line 31, in the Settlement plane of *Product and trust model*: "separately operated Conflux eSpace RPC verifies native CFX funding. … The application never starts a node or miner on login; Core mining payouts require an explicit Core-to-eSpace transfer before use." The surrounding lines retire the earlier product's node and mining: line 17 ("Historical storage/Drive/CDN/Core-Geth/in-app-mining guides are not current runtime instructions"), line 777 (no new mining pool) and line 793 ("bundled Core-Geth/Ethash mining" is not a compatibility contract). Line 679 keeps legacy data in place with "no automatic legacy data importer", line 644 says "Native provisioning starts no service", and line 940 asks the Wallet page to explain that Core and eSpace balances are separate. `CLAUDE.md` adds that no "app-managed mining is part of this release" and that the application must not "auto-start legacy services" or automatically import, convert, migrate or delete legacy data.

In the earlier application, "login" was the in-app wallet login, which started the bundled Geth node and, with a stored preference, mining (commit `49ed4da9`, `src/App.svelte:473`: "Auto-start Geth node once when user logs in").

The comparison pass of 2026-11-18 (`docs/apas-worker/comparison-2026-11-18.md`) lists this rule as missing specification item 16. Neighbouring rules have owners:

- `inference-wire-foundation` admits only eSpace chain 71 with no legacy-chain fallback;
- `chain-rpc-gateway` refuses mining methods;
- `desktop-gui-global-rules` keeps network actions waiting for an explicit profile;
- `desktop-gui-pages` owns the Wallet page's CHI/CHR and Core/eSpace statements and the preference fields;
- `provider-daemon-lifecycle` owns the daemon's legacy-data rule;
- the target-design `desktop-share-online` never restarts a share by itself.

Nothing states that the application runs no chain node or miner, that a login or session start launches no legacy service, that the desktop and CLI leave legacy data in place, that Core-space funds are not eSpace funds, or that native provisioning starts no service. A later change could add any of these behaviours without a failing requirement.

## What Changes

- Add the `application-startup-boundaries` capability for the current runtime: 5 requirements and 14 scenarios. Each requirement cites its whitepaper lines and comes from the whitepaper or `CLAUDE.md`, not from current code.
- Specify that the desktop, the CLI and the headless binaries bundle, install, start and manage no chain node or miner, reach the chain only through the profile's separately operated eSpace RPC, and offer no mining control.
- Specify that neither an operating-system login, nor the application's launch, nor a session start (profile or wallet load) starts a node, a miner or a legacy service; that the application registers no start-at-login entry, service or scheduled task for them; that a stored automatic-mining preference is never honoured; and that the application does not start, restart, enable or schedule a legacy service that it finds on the host.
- Specify that the desktop and CLI leave legacy data in place, never import it automatically, convert no CHI or CHR, and accept a legacy key only by explicit import.
- Specify that Core-space balances and mining payouts are never presented as eSpace funds and are never transferred without the user's explicit action.
- Specify that native provisioning starts no service, as line 644 says, and that the Compose deployment has no node or miner service. Container provisioning (`--container`) is not covered, because the book does not address it.
- In `design.md`, record what "on login" and "node" mean, the owners of the neighbouring rules, nine decisions, a coverage table, the code search (no code difference found), observations, one question for the whitepaper author and the risks.

## Capabilities

### New Capabilities

- `application-startup-boundaries`: What the application may start, and when. It covers the following: no chain node or miner at any time; nothing retired started at login, launch or session start; no start-at-login entry for them; legacy data left in place and never imported automatically; Core-space funds not treated as eSpace funds; and native provisioning that starts no service.

### Modified Capabilities

None. The spec text references the normative `inference-wire-foundation` and `desktop-window-lifecycle`, and the pending `chain-rpc-gateway`, `managed-engine-runtime`, `desktop-gui-global-rules`, `desktop-gui-pages`, `inference-funding-settlement`, `provider-daemon-lifecycle`, `profile-path-resolution` and `desktop-share-online`. It adds no obligation to them and changes none of their requirements.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/main.rs`, `src-tauri/src/lib.rs` (`run`) and `src-tauri/src/desktop.rs` (`run`, the `setup` hook, the native operations);
  - `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/build.rs`, `src-tauri/Cargo.toml` and `package.json` (no plugin, no shell permission);
  - `src-tauri/src/bin/*.rs`, `src-tauri/src/engine_runtime.rs` (the only process spawn, for the provider's engine), `src-tauri/src/chain_client.rs` and `rpc_client.rs` (only `eth_*` methods);
  - `src-tauri/src/consumer.rs`, `wallet.rs` and `network.rs` (state in the network-scoped namespace, explicit import);
  - `src/App.svelte`, `src/lib/stores.ts`, `src/pages/Wallet.svelte` and `src/pages/Network.svelte`;
  - `scripts/provision-provider.py`, `provision-runtime.py`, `provision-model.py`, `docker-compose.yml` and `Dockerfile`.
- No code difference was found; `design.md` lists the searches. Missing acceptance evidence is tracked as unchecked tasks 3.1–3.5, and installer and real-login evidence as task 4.1.
- One nonblocking question for the whitepaper author is recorded in `design.md`: whether the application may ever register itself to start at login when that starts no node, miner or legacy service.
- No wire format, ledger schema, deployment, product code, test or whitepaper change is part of this specification step. This change grants no live, installer, public-testnet or release qualification.
