## Why

The read-only whitepaper (`docs/chiral-book.md`) states the account-credential contract in *Consumer control, credentials, and request lifecycle* (lines 325–360). The relevant parts are:

- the authorization matrix rows for credential management and for the bearer-authorized inference routes (lines 335–339);
- the credential paragraph (line 345): issuance accepts `{expires_at, spend_cap_wei, max_concurrency, scopes}`, scopes are `infer` and/or `read`, the spend cap is nullable canonical money, credential concurrency cannot exceed its account, the provider stores token hashes, the secret is separate from its public identifier and is kept in protected native storage and exported intentionally, native management outputs metadata, and a cap is not a new balance or refund guarantee;
- the statement that account and credential limits are enforced against the same ledger (line 355), and that credential recovery is not renegotiation (line 357).

Other lines add to it. Lines 493 and 495 cover recovery without rotation and explicit issuance or rotation of an expired or revoked default. Line 472 scopes consumer credentials by network. Line 474 requires protected secret files. Line 671 keeps credentials out of logs, and line 677 tells consumers to revoke compromised credentials. `CLAUDE.md` adds: "Resuming an already-open account must not rotate an exported API credential. Explicit revocation/issuance/rotation and historical funding recovery must remain reachable", secrets in protected files, and private keys never in Svelte.

The comparison pass of 2026-11-18 (`docs/apas-worker/comparison-2026-11-18.md`) lists this contract as missing specification item 12. Existing changes cover parts of it:

- `inference-owner-control` (pending `specify-owner-control-authorization`) owns who may issue, rotate, revoke and list credentials, and the consumption of the owner proof;
- `inference-request-accounting` (pending `specify-inference-request-accounting`) owns the reservation-time checks of scope, cap and concurrency, and what revocation does to an admitted hold;
- `inference-account-opening-request` (in `openspec/specs`) owns opening replay without a new secret;
- `inference-funding-settlement` (pending `specify-funding-settlement`) owns `fund` and `resume` without rotation, and explicit issuance restoring a missing default;
- `outbound-http-safeguards` (pending `specify-outbound-http-safeguards`) owns the SDK base URL of an export;
- `desktop-gui-global-rules` and `desktop-gui-pages` (pending) own the webview's handling of credential metadata and of a revealed key;
- `client-version-policy` (pending `specify-version-policy`) owns the native version gate.

Nothing specifies the rest. That includes the issuance options object and its member rules, hash-only storage and the separation of secret and identifier, and the scope that each inference route requires. It also includes the meaning of revocation and rotation, a cap as a limit rather than a balance, and credential management while the provider is draining. Finally, it includes the consumer's protected store, the metadata-only native outputs, and how rotation keeps a usable native default. A later change could therefore relax any of these without a failing requirement.

## What Changes

- Add the `account-credentials` capability: 9 requirements and 23 scenarios for the current runtime. Each requirement cites its whitepaper lines and comes from the whitepaper or `CLAUDE.md`, not from current code. Values that the book does not state are recorded in `design.md` as current implementation values. They are the 90-day expiry ceiling, the 64-credential bound, the 100-entry list, the token format and the hash function. They also include the first-opening credential, the default options, the encoding of `expires_at`, the members of the creating response, the list and the export, and the store's layout and modes.
- Specify that issuance and rotation accept exactly the four-member options object, and the meaning of each member.
- Specify that the provider stores only a hash of each secret, returns the secret only in the response that creates it, and keeps the public identifier independent of it.
- Specify the scope that each bearer-authorized inference route requires, and that a revoked or expired credential is refused.
- Specify revocation of one named credential, atomic rotation (including of an expired or revoked credential), and refusal of another account's credential identifier.
- Specify that a credential's cap and concurrency restrict authority over the one prepaid balance and never create or refund money.
- Specify that credential management needs no new-admission readiness.
- Specify the consumer's protected, network-scoped store, the metadata-only native outputs with explicit export as the only exception, SDK examples that never print the exported key, and how rotating the native default keeps a usable default.
- In `design.md`, record a coverage table (requirement, code, existing tests, gaps), two code differences, observations outside this capability, three questions for the whitepaper author and the risks. The code differences and the missing acceptance evidence are unchecked tasks.

## Capabilities

### New Capabilities

- `account-credentials`: Provider-account credentials in the current runtime. This covers the issuance and rotation options object, hash-only storage separate from the public identifier, route scopes for bearer-authorized inference, revocation and atomic rotation, caps as limits on the one prepaid balance, and credential management without new admission. On the consumer side, it covers the protected store, metadata-only native outputs with explicit export, and native-default rotation.

### Modified Capabilities

None. The spec references the pending `inference-owner-control`, `inference-request-accounting`, `inference-funding-settlement`, `outbound-http-safeguards`, `desktop-gui-global-rules`, `desktop-gui-pages`, `client-version-policy` and `profile-path-resolution`, and the normative `inference-wire-foundation` and `inference-account-opening-request`. It adds no obligation to them and does not change their requirements. The target-design changes `specify-desktop-local-api-server` and `specify-cli-loopback-gateway` issue route credentials through this contract; this change does not modify them.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/contracts.rs`: credential routes, `authorize_owner`, `authorize_read`, `parse`, `error`;
  - `src-tauri/src/billing_store.rs`: `CredentialIssueRequest`, `CredentialView`, the `credentials` table, `authenticate`, `issue_credential`, `credentials`, `revoke_credential`, `rotate_credential`, and the first-opening credential in `open_account`;
  - `src-tauri/src/inference_gateway.rs`: `authenticate` and its route scopes, and `ledger_error`;
  - `src-tauri/src/auth.rs`: `bearer`;
  - `src-tauri/src/protocol.rs`: `Credential`, `hash_hex`, `random_id`, `deserialize_object`;
  - `src-tauri/src/consumer.rs`: `SavedAccount`, `private_read`, `private_write`, `account_directory`, `store_credential`, `credentials`, `credential_issue`, `credential_revoke`, `credential_rotate`, `credential_export`, `credential`, `credential_metadata`;
  - `src-tauri/src/wallet.rs`: `validate_secret_file`, `create_secret_file`, `open_secret_file`, `secure_directory`;
  - `src-tauri/src/desktop.rs` and `src-tauri/src/bin/chiral.rs`: the credential operations and commands.
- Two code differences were found. Tasks 2.1 and 2.2 track them:
  - the issuance and rotation bodies, and native options, also decode from a positional JSON array;
  - native explicit issuance can replace a valid default credential once that credential is older than the 100 newest credential records of its account, because it judges the default from the bounded provider list.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.9. The main gaps:
  - most refusals of issuance options;
  - a search of the ledger files for plaintext secrets;
  - scope refusals on the inference routes;
  - rotation of an expired or revoked credential;
  - revocation and listing while the provider is draining;
  - a cap larger than the balance;
  - the protections of the native store as actually written, and a native Windows run;
  - CLI output of the credential commands, and an automated run of the SDK examples;
  - rotation of an expired default.
- Three questions for the whitepaper author are recorded in `design.md`:
  - whether `spend_cap_wei` may be omitted;
  - whether rotating a credential that is not the recorded default restores a missing default;
  - whether the credential list must include every credential that can still authenticate.

  All three are nonblocking.
- No wire format, ledger schema, deployment or whitepaper change is part of this specification step. This change grants no live, public-testnet or release qualification.
