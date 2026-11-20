## ADDED Requirements

### Requirement: Credential issuance and rotation accept exactly the four-member options object

Issuance (`POST /v1/contracts/:id/credentials`) and rotation (`POST /v1/contracts/:id/credentials/:credential_id/rotate`) SHALL accept as their body a JSON object whose only members are `expires_at`, `spend_cap_wei`, `max_concurrency` and `scopes` (book line 345; rotation issues a replacement credential and takes the same options, book lines 336 and 930). A body that is not a JSON object SHALL be refused, including a JSON array that lists the same four values by position, and so SHALL an object with an unknown member or a repeated member. The provider SHALL refuse members that break these rules (book line 345):

- `scopes` SHALL be a non-empty list whose every element is `infer` or `read`;
- `spend_cap_wei` SHALL be `null`, meaning no credential cap, or a canonical decimal wei string as `inference-wire-foundation` (*Exact monetary representation and CFX conversion*) defines;
- `max_concurrency` SHALL NOT exceed the concurrency limit of the account the credential is issued for;
- `expires_at` SHALL be the time from which the credential no longer authenticates.

Who may issue and rotate, and the one-use consumption of the owner proof even when the body is then refused, are specified by `inference-owner-control` (*Account control authorization follows ownership and read scope* and *Consuming owner authority does not imply business success*; book line 336). A refused issuance or rotation SHALL create no credential and SHALL change no existing credential.

#### Scenario: Valid options issue a credential with exactly those limits

- **GIVEN** an account whose concurrency limit is 2
- **WHEN** its owner issues a credential with `{"expires_at": <one hour ahead>, "spend_cap_wei": "900", "max_concurrency": 2, "scopes": ["infer", "read"]}`
- **THEN** the provider returns the new credential
- **AND** the account's credential list shows it with that expiry, cap, concurrency limit and both scopes

#### Scenario: A positional array is not the options object

- **WHEN** the owner sends `[<one hour ahead>, null, 1, ["read"]]` as the body of an issuance or of a rotation, with a valid owner proof for exactly that body
- **THEN** the request is refused as malformed
- **AND** no credential is created, and the credential named by the rotation still authenticates

#### Scenario: Options outside the contract are refused

- **WHEN** the owner issues or rotates with `scopes` `[]` or `["admin"]`, with `max_concurrency` greater than the account's limit, with `spend_cap_wei` `"01"`, `"1.5"`, `"-1"`, `""` or the JSON number `1`, or with an extra member such as `"account": "other"`
- **THEN** each request is refused
- **AND** the account's credentials are unchanged

### Requirement: The provider keeps only a hash of each secret, separate from the credential's public identifier

The response that creates a credential (issuance, rotation, or the first opening of an account) SHALL return its secret together with the credential's public identifier, and that response is the only one that carries the secret. The provider SHALL persist only a hash of the secret and SHALL NOT write the plaintext secret to its ledger, its logs or any other state it keeps (book lines 345 and 671). The public identifier SHALL be generated independently of the secret: identifiers appear in listings, request records and URLs, and SHALL NOT disclose the secret (book line 345).

Every other provider response, including the credential list, account, receipt and request views, an opening replay and every error, SHALL carry credential metadata only, never a secret (book line 345). Credential metadata is information about a credential other than its secret, such as its identifier, account, expiry, scopes, limits and revocation state. Because the provider cannot return a secret it does not hold, a lost secret SHALL be replaced only by explicit owner-authorized issuance or rotation (book line 495). Opening replay returns no credential, as `inference-account-opening-request` (*Opening replay preserves accepted account state*) requires.

#### Scenario: The ledger holds no plaintext secret

- **GIVEN** a provider that has created credentials through a first opening, an issuance and a rotation
- **WHEN** its ledger database files are searched for each returned secret
- **THEN** no secret is found
- **AND** each credential is stored with the hash of its secret

#### Scenario: The credential list carries metadata only

- **WHEN** the account owner, or a bearer of that account with `read` scope, lists the account's credentials
- **THEN** each entry carries the credential's identifier, expiry, scopes, limits and revocation state
- **AND** no entry carries a secret

#### Scenario: Identifier and secret are distinct

- **WHEN** a credential is issued
- **THEN** its public identifier differs from its secret, and neither contains the other

### Requirement: An account bearer authorizes the inference routes only within its credential's account, scope and lifetime

The provider's public inference routes SHALL authenticate an account bearer only when it is the secret of an existing credential that has not been revoked and whose `expires_at` is later than the provider's current time (book lines 337–339, 345 and 495). The request SHALL act only on that credential's account (book line 337). The credential SHALL carry the scope that the route requires (book lines 337–339):

- `infer` for `GET /v1/models`, `POST /v1/chat/completions`, `POST /v1/completions`, `POST /v1/embeddings` and `POST /v1/requests/:id/cancel`;
- `read` for `GET /v1/requests/:id`.

A bearer that does not authenticate, or that lacks the required scope, SHALL be refused before any reservation, engine request or change to request state (book lines 337–339 and 581). Other specifications own the rest of this path. `inference-request-accounting` owns the recheck of inference authority, caps and concurrency inside the reservation (*Reservation atomically binds authority limits and credit*) and the account isolation of request reads (*Request and receipt history supports attributable recovery*). `inference-owner-control` owns bearer access to the control reads (*Account control authorization follows ownership and read scope*).

#### Scenario: A read-only bearer cannot infer or cancel

- **GIVEN** a valid credential whose only scope is `read`
- **WHEN** it calls `GET /v1/models`, `POST /v1/chat/completions`, `POST /v1/completions`, `POST /v1/embeddings`, or `POST /v1/requests/:id/cancel` for a request of its own account
- **THEN** each call is refused for missing scope
- **AND** no request is reserved and no cancellation is signalled

#### Scenario: An inference-only bearer cannot read request status

- **GIVEN** a valid credential whose only scope is `infer` and a request of its account
- **WHEN** it calls `GET /v1/requests/:id` for that request
- **THEN** the call is refused for missing scope

#### Scenario: A revoked or expired bearer is refused on every route

- **GIVEN** a credential that has been revoked, or whose `expires_at` is equal to or earlier than the provider's current time
- **WHEN** it calls any route listed above
- **THEN** the call is refused as unauthenticated
- **AND** no account, credential or request state changes

### Requirement: Revocation and rotation act on one named credential of the signed account

Revocation (`DELETE /v1/contracts/:id/credentials/:credential_id`) SHALL make the named credential of the signed account stop authenticating for every later request (book lines 336 and 677). It SHALL leave the account's other credentials, its agreement and its amounts unchanged.

Rotation SHALL revoke the named credential and issue its replacement, with the options of *Credential issuance and rotation accept exactly the four-member options object*, as one atomic change: if the replacement cannot be issued, the named credential SHALL remain exactly as it was (book line 336). Rotation SHALL be available for a credential that has expired or has been revoked, because explicit rotation is a remedy for an unusable default credential (book line 495).

A credential identifier that does not belong to the account named in the signed request SHALL be refused without changing the credentials of either account. What revocation and rotation do to the hold of a request already admitted under the named credential is specified by `inference-request-accounting` (*Reservation atomically binds authority limits and credit*), not by this capability.

#### Scenario: Revocation stops one credential and no other

- **GIVEN** an account with valid credentials A and B
- **WHEN** the owner revokes A
- **THEN** A no longer authenticates
- **AND** B still authenticates, and the account's agreement and amounts are unchanged

#### Scenario: A refused rotation keeps its predecessor

- **GIVEN** a valid credential A
- **WHEN** the owner rotates A with `max_concurrency` greater than the account's limit
- **THEN** the rotation is refused
- **AND** A still authenticates, and no replacement credential exists

#### Scenario: An expired or revoked credential can be rotated

- **GIVEN** a credential A that has expired, or that has been revoked
- **WHEN** the owner rotates A with valid options that include `infer`
- **THEN** the replacement authenticates for the account with the requested scopes
- **AND** A still does not authenticate

#### Scenario: An identifier from another account is refused

- **GIVEN** two accounts X and Y of the same owner, and a credential C of account Y
- **WHEN** the owner sends a revocation or rotation for account X naming C, with a valid owner proof for exactly that request
- **THEN** the request is refused
- **AND** the credentials of X and of Y are unchanged, and C still authenticates

### Requirement: Credential limits restrict authority over the same prepaid balance

A credential's spend cap and concurrency limit SHALL only restrict what that credential can authorize against its account's single prepaid balance (book line 345). Issuing, rotating, revoking or the expiry of a credential SHALL NOT change the account's funded, available, reserved or spent amounts, its fee obligations or its signed agreement (book lines 345 and 357). A spend cap larger than the available balance SHALL NOT authorize a request that the balance cannot cover. The unused part of a cap of a revoked or expired credential SHALL NOT be credited, returned or refunded to anyone. No native surface SHALL present a credential cap as a balance or as a refund guarantee (book line 345).

`inference-request-accounting` (*Reservation atomically binds authority limits and credit*) specifies how account and credential limits are enforced together against the same ledger (book line 355).

#### Scenario: A large cap grants no credit

- **GIVEN** an account with 1,000 wei available
- **WHEN** the owner issues a credential with spend cap `1000000000000000000` and that credential requests inference whose maximum cost is 2,000 wei
- **THEN** issuance leaves the account's amounts unchanged
- **AND** the request is refused because the balance cannot cover it, not because of the cap

#### Scenario: Revoking unused cap refunds nothing

- **GIVEN** a credential with spend cap 900 wei that has spent 100 wei
- **WHEN** the owner revokes it
- **THEN** the account's funded, available, reserved and spent amounts are unchanged
- **AND** no funding, refund or fee record is created

### Requirement: Credential management stays reachable without new admission

Owner-authorized issuance, rotation, revocation and listing of credentials SHALL NOT depend on any of the following (book lines 357, 495 and 677; `CLAUDE.md`: "Explicit revocation/issuance/rotation and historical funding recovery must remain reachable without abandoning accepted balances"):

- the provider accepting new accounts or new funding;
- new-funding settlement readiness;
- a currently qualified or available engine;
- a live or current offer;
- quote or opening capacity.

These operations SHALL NOT renegotiate the account's original model and pricing contract (book line 357).

The native client's own version check on these operations belongs to `client-version-policy` (*A required update refuses new funding and native inference but keeps recovery reachable*). That requirement keeps revocation reachable and leaves issuance and rotation to its own open question.

#### Scenario: A draining provider still manages credentials

- **GIVEN** a provider that is draining, whose new-funding readiness is lost, whose engine has no qualified model and whose quote budgets are exhausted
- **WHEN** an account owner issues, rotates, revokes and lists credentials
- **THEN** each operation succeeds
- **AND** the account's agreement and amounts are unchanged

### Requirement: The native consumer keeps credential secrets only in its protected account store

The native consumer SHALL keep each credential secret it receives, from a first opening, an issuance or a rotation, only in its protected account store, which is scoped to the profile's network identity (book lines 345 and 472). That store's directory and files SHALL be accessible only to the owning user: on Unix, with no group or other permission; on native Windows, owner-protected (book line 474).

The consumer SHALL open these files without following a symbolic link. It SHALL refuse to use a store file or directory that is a symbolic link or is not protected, and SHALL NOT widen, repair or replace it (book line 474; `CLAUDE.md`: secrets use protected files, and unsafe symlinks are refused). The consumer SHALL NOT write the secret to any other file, including the files that record request keys. It SHALL NOT place the secret on a command line, in a log, in a diagnostic or in a prompt (book lines 345, 474 and 671). An operator's backup of the consumer state is not such a file; it needs equally protected access (book lines 474 and 673). Where a profile's state lives is specified by `profile-path-resolution` (*One profile reaches one state from every working directory*).

#### Scenario: Stored secrets are owner-only and nowhere else

- **GIVEN** a native consumer on Unix that has opened an account and issued a credential
- **THEN** the account store's directory and the file holding the secrets are owned by the user and have no group or other permission, and that file is a regular file
- **AND** no other file under the profile's `data_dir` contains either secret

#### Scenario: An unprotected store fails closed

- **GIVEN** an account file that is readable by the user's group, or that is a symbolic link to another file
- **WHEN** the consumer is asked to export, issue, rotate, revoke or run inference for that account
- **THEN** the operation is refused
- **AND** the file, its permissions and the link target are unchanged

### Requirement: Native credential commands output metadata, and only an explicit export reveals a secret

Native credential management SHALL output credential metadata only and SHALL NOT output a credential secret (book line 345). This covers the CLI commands `chiral credentials <contract> list`, `issue`, `rotate` and `revoke`, and the desktop operations `credentials`, `credential_issue`, `credential_rotate` and `credential_revoke`. Only an explicit export SHALL return a secret: the CLI command `chiral credentials <contract> export [--id <credential_id>]` and the desktop operation `credential_export` (book lines 345, 474, 603 and 930). The export SHALL return the secret together with the SDK base URL and the agreement's model (book lines 603 and 930).

Other specifications own the rest of the export. `outbound-http-safeguards` owns the SDK base URL (*Advertised endpoints and catalog URLs are origins, and the SDK base URL is derived after validation*). `desktop-gui-global-rules` (*Secrets stay native except for an explicitly revealed credential*) and `desktop-gui-pages` (*API credentials are managed as metadata and revealed only on request*) own how the webview handles a revealed key and the GUI's own SDK snippets.

No CLI argument SHALL accept a credential secret (book lines 345 and 474). Error messages and diagnostic output SHALL NOT contain a secret (book line 345). The repository's SDK examples (`examples/openai_client.py` and `examples/openai-client.mjs`, book line 603) SHALL obtain the secret through the explicit export, keep it inside their own process and SHALL NOT print it (book line 474).

#### Scenario: Management outputs carry no secret

- **GIVEN** an account whose credential secrets are known
- **WHEN** the CLI commands `credentials list`, `issue`, `rotate` and `revoke`, and the corresponding desktop operations, are run for it, including runs that fail
- **THEN** no secret appears on standard output, on standard error or in any returned result

#### Scenario: An explicit export reveals one secret

- **WHEN** the user runs `chiral credentials <contract> export --id <credential_id>`
- **THEN** standard output is one JSON document with that credential's secret, the model and the SDK base URL
- **AND** standard error contains no secret

#### Scenario: The SDK examples never print the key

- **WHEN** either SDK example runs against an exported credential, including a run whose request fails or is interrupted
- **THEN** the secret appears on neither standard output nor standard error

### Requirement: Rotating the native default keeps a usable default

When the owner explicitly rotates the credential that the native consumer holds as the account's default, the consumer SHALL make the replacement its new default if the replacement has the `infer` scope (book line 495). Rotating a default that has expired or been revoked SHALL therefore restore a usable default for native inference. A replacement without the `infer` scope SHALL NOT become the default for native inference (book line 495). Until an explicit issuance or rotation restores a usable default, the consumer SHALL NOT silently select another stored credential in place of the missing or unusable one (book line 495).

`inference-funding-settlement` (*Fund and resume recover the saved agreement without rotating credentials*) owns two related rules: `fund` and `resume` never rotate, and explicit issuance restores a missing default without replacing a valid one.

#### Scenario: Rotating an expired default restores it

- **GIVEN** a native account whose default credential has expired, or has been revoked at the provider
- **WHEN** the owner rotates that credential with the scopes `infer` and `read`
- **THEN** native inference and an export without a credential identifier use the replacement

#### Scenario: A read-only replacement is not a default

- **GIVEN** a native account with a valid default credential
- **WHEN** the owner rotates it with only the `read` scope
- **THEN** an export without a credential identifier fails instead of returning the replacement or any other stored credential
- **AND** an export naming the replacement's identifier succeeds
