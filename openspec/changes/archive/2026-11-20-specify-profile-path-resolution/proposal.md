## Why

The read-only whitepaper (`docs/chiral-book.md`) says on line 449 that "Paths in consumer/provider configurations are resolved relative to their configuration file", and on line 470 that every process spending the same signing wallet on a network "must use the **same configured `data_dir` base** and coordinated journal". Line 472 adds that "Changing paths or restoring an older layout without deliberate state preservation is not an upgrade strategy", and line 652 that upgrades "must preserve accepted ledgers, acknowledgements, identity, shared nonce journals, credentials and evidence".

The provider, relay and RPC-gateway configurations already follow line 449. The consumer did not: `Consumer::load` passed the decoded profile to `Consumer::new` unchanged, so a relative `data_dir`, `network.rpc_bearer_file` or `dht.identity_file` named a place in the process working directory. The same profile loaded from two directories used two wallets, two agreement stores, two policy watermarks and two nonce journals, and `chiral wallet create` run from another directory created state there (`docs/apas-worker/comparison-2026-11-18.md` item 2, reproduced).

No specification states the rule, so nothing prevented the difference. Fixing it also changes where an existing relative profile looks for its state, so the fix needs a rule for state that an earlier client left in a working directory.

## What Changes

- Add the `profile-path-resolution` capability with 5 requirements and 15 scenarios. Each requirement cites its whitepaper lines.
- Specify that every relative path-valued field of consumer, provider, relay and RPC-gateway configurations resolves against the configuration file's directory, independent of the working directory; absolute paths are unchanged; command-line path arguments are not configuration paths and follow the working directory.
- Specify that one profile reaches one `data_dir`, wallet, agreement store, policy watermark and nonce journal from every working directory, and that a consumer profile and a provider configuration in one folder naming the same relative `data_dir` share one journal (book line 470).
- Specify that state found only at the old working-directory location is never silently ignored or moved: the load is refused, before anything is created, with an error naming the profile, the field and both locations and offering two deliberate remedies (an absolute path, or a move by the user). When the profile's own location already holds state, it is used and the other copy is left untouched.
- Specify that the CLI reports a refusal on stderr with empty stdout and a nonzero exit, and that desktop Settings shows it without replacing a profile already loaded.
- Fix `Consumer::load` (task 2.1): resolve `data_dir`, `network.rpc_bearer_file` and `dht.identity_file` with the provider's resolver, then refuse stranded state by checking only the two candidate locations of each relative field, including a consumer wallet key left only at the working-directory location when the resolved location holds only the empty directories an earlier load from elsewhere created.
- Record a coverage table in `design.md`, one code difference (fixed), two open questions for the whitepaper author and the risks.

## Capabilities

### New Capabilities

- `profile-path-resolution`: Resolution of relative paths in consumer, provider, relay and RPC-gateway configuration files against the file's directory; one state per profile from any working directory; a shared journal for configurations in one folder; refusal, never silent loss or movement, of state left at the old working-directory location; and how the CLI and desktop report that refusal.

### Modified Capabilities

None. The design relates this capability to the pending `inference-funding-settlement` (journal sharing), `client-version-policy` (upgrades preserve state and the watermark), `desktop-gui-pages` (Settings) and `desktop-share-online` (the desktop's provider shares the consumer's `data_dir`). The spec text names none of them and changes none of their requirements.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code:
  - `src-tauri/src/consumer.rs`: `resolve_profile_paths`, `refuse_stranded_profile_state` and `Consumer::load`;
  - `src-tauri/src/provider_config.rs`: `resolve_path` becomes `pub(crate)`; provider behaviour is unchanged.
  - The CLI (`bin/chiral.rs`) and desktop (`desktop.rs` `configure`, `Settings.svelte`) already propagate a load error and need no change.
- Tests: `src-tauri/tests/consumer_profile_paths_test.rs` (six binary-level regressions with a different working directory) and the library test `consumer::tests::every_relative_profile_path_resolves_against_the_profile_directory`. Each fails without the change.
- Users who ran an older CLI from a directory other than the profile's will see a refusal naming both locations until they set an absolute `data_dir` or move the directory themselves. Nothing is moved automatically.
- Missing evidence is tracked as unchecked tasks: tests for the provider's other path fields and for the relay and gateway, and a desktop test of a refused `configure`.
- No wire format, ledger schema, deployment or whitepaper change is part of this step. It grants no live-chain, public-testnet or release qualification.
