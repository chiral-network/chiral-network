## ADDED Requirements

### Requirement: Release builds compile a non-placeholder policy authority

A release build of any Chiral binary SHALL compile in the policy authority's public key from the `CHIRAL_POLICY_PUBLIC_KEY` build environment variable (book line 648). The value SHALL be 64 hexadecimal characters, with an optional `0x` prefix, that encode a valid Ed25519 public key other than the all-zero placeholder. A release build SHALL fail, without producing a binary, when the variable is absent, has the wrong length, contains non-hexadecimal characters, does not encode a valid Ed25519 public key, or is all zero.

A debug build MAY compile the all-zero placeholder when the variable is absent or invalid. That placeholder is private-development behavior only, as *The placeholder authority is private-development behavior only* specifies. The authority's private key SHALL NOT be an input to any build.

#### Scenario: A release build without a key fails

- **GIVEN** a build environment with no `CHIRAL_POLICY_PUBLIC_KEY`
- **WHEN** a release build of the binaries is started
- **THEN** the build fails and no release binary is produced

#### Scenario: A placeholder or malformed key fails a release build

- **WHEN** a release build is started with `CHIRAL_POLICY_PUBLIC_KEY` set to 64 zeros, to 64 hexadecimal characters that are not a valid Ed25519 point (for example `42` repeated 32 times), or to 63 hexadecimal characters
- **THEN** the build fails and no release binary is produced

#### Scenario: A valid key becomes the compiled authority

- **GIVEN** a valid nonzero Ed25519 public key K
- **WHEN** a release build is made with `CHIRAL_POLICY_PUBLIC_KEY` set to K and the binary later runs with no runtime override
- **THEN** the binary accepts policies signed with K's private key and rejects policies signed with any other key

### Requirement: The runtime authority override is resolved once per process

On its first use in a process, the runtime SHALL read the `CHIRAL_POLICY_PUBLIC_KEY` environment variable (book line 648). When the variable holds a valid, nonzero Ed25519 public key in the build format, that key SHALL be the effective policy authority for the rest of the process. When the variable is absent, malformed, not a valid Ed25519 public key, or all zero, the compiled key SHALL be the effective authority. The result SHALL be cached for the life of the process, so a later change of the variable SHALL NOT change the authority of a running process.

The override SHALL NOT remove the release-build requirement. Changing the effective authority changes which signatures are accepted and which watermark directory is used (*The rollback watermark is durable, protected and scoped to authority and network*); it is not a display setting (book line 648).

#### Scenario: A valid override replaces the compiled key

- **GIVEN** a binary with compiled key C and a process started with `CHIRAL_POLICY_PUBLIC_KEY` set to a different valid nonzero key K
- **WHEN** the process verifies its first policy
- **THEN** policies signed with K's private key are accepted and policies signed only with C's private key are rejected
- **AND** the process reads and writes its rollback watermark in K's directory, not C's

#### Scenario: A malformed or zero override falls back to the compiled key

- **GIVEN** a binary with compiled key C
- **WHEN** a process starts with `CHIRAL_POLICY_PUBLIC_KEY` set to 64 zeros, to `not-a-key`, or to 64 hexadecimal characters that are not a valid Ed25519 point
- **THEN** C is the effective authority of that process

#### Scenario: The authority is fixed for the life of the process

- **GIVEN** a process whose first use resolved the override K
- **WHEN** the variable is changed to another valid key K2 inside that process
- **THEN** K stays the effective authority until the process restarts

### Requirement: Only an authentic, well-formed, current policy is accepted

A version policy states `min_required`, `recommended`, `download_url`, an optional `message`, `issued_at` and `valid_until` (Unix seconds, where a `valid_until` of 0 means no expiry), and a `signature` (book lines 63, 646 and 846). The signature SHALL be an Ed25519 signature by the policy authority over an unambiguous encoding of every other field, so that changing any field invalidates it (book line 648).

A native client, relay or provider SHALL make a received policy effective only when all of the following hold (book lines 648 and 650; `CLAUDE.md`: "Version-policy acceptance is signed"):

- the signature verifies against the effective authority (an unsigned policy is governed only by *The placeholder authority is private-development behavior only*);
- `min_required` and `recommended` are numeric versions;
- `issued_at` is not later than the current time plus a small clock-skew allowance;
- `valid_until` is 0, or later than both the current time and `issued_at`;
- the rollback watermark allows it (*The rollback watermark is durable, protected and scoped to authority and network*).

A rejected policy SHALL NOT change the effective policy or the watermark.

A client's version status under the effective policy SHALL be `required` when its version is below `min_required`; otherwise `recommended` when it is below `recommended`; otherwise `ok`. A policy whose `min_required` is above its `recommended` still yields `required` below `min_required`. A malformed client or policy version SHALL make the check fail with an error; it SHALL NOT be read as zero or reported as `ok`.

#### Scenario: A tampered policy is rejected

- **GIVEN** a policy signed by the authority with `min_required` `2.0.0`
- **WHEN** its `min_required` is changed to `0.0.0` without a new signature and the policy is offered to a client
- **THEN** the client rejects it and its effective policy and watermark are unchanged

#### Scenario: A future-dated or expired policy is rejected on arrival

- **WHEN** a client at time 200 is offered an authentic policy issued at a time far in the future, or one issued at 100 with `valid_until` 200, or one whose `valid_until` is not later than its `issued_at`
- **THEN** each is rejected

#### Scenario: The three status values

- **GIVEN** an effective policy with `min_required` `1.0.0` and `recommended` `2.0.0`
- **THEN** version `0.9.0` is `required`, versions `1.0.0` and `1.5.0` are `recommended`, and versions `2.0.0` and `3.0.0` are `ok`

#### Scenario: A malformed version fails the check

- **GIVEN** an effective policy whose `min_required` is `1.x.0`, or a client version `not-a-version`
- **WHEN** the client checks its status
- **THEN** the check fails with an error and no status is reported

### Requirement: The rollback watermark is durable, protected and scoped to authority and network

The last accepted authenticated policy SHALL be the rollback watermark (book line 650). A process SHALL write it durably to protected storage before making it effective. The storage SHALL be scoped to the complete network identity (chain ID, genesis hash and deployment ID) and to the effective authority (book line 650; `CLAUDE.md`). Today this storage is `<data_dir>/inference/<network namespace>/version-policy/<authority public key hex>/` (design decision 12). Every file and directory in it SHALL be owner-private (book lines 474 and 650).

Before it makes any policy effective, and when it starts, a process SHALL read the watermark from that storage, not only from memory. A fresh process SHALL NOT accept an older policy because its memory was empty (book line 650). A policy SHALL pass the watermark only when its `issued_at` is later than the watermark's, or equal to it with byte-identical signed fields. An equal-time policy with different fields is equivocation and SHALL be rejected. Processes that share one `data_dir` SHALL serialize acceptance with an exclusive lock, and no interleaving of their acceptances SHALL lower the watermark (`CLAUDE.md`: "durable, and monotonic across processes").

Different networks, and different authorities on one network, SHALL keep independent watermarks. A policy file copied from one authority's directory into another's SHALL fail verification there.

#### Scenario: A fresh process refuses an older policy

- **GIVEN** a process that accepted an authentic policy P issued at time T and then exited
- **WHEN** a new process on the same `data_dir`, network and authority is offered an authentic, unexpired policy issued at T − 10
- **THEN** it rejects that policy and P is its effective policy

#### Scenario: An equal-time policy with different fields is equivocation

- **GIVEN** a watermark P issued at T with `min_required` `2.0.0`
- **WHEN** an authentic policy issued at T with `min_required` `0.0.0` is offered
- **THEN** it is rejected
- **AND** offering P itself again is accepted and changes nothing

#### Scenario: Concurrent processes keep the highest watermark

- **GIVEN** eight processes on one `data_dir`, each offered a different authentic policy issued at T − 1 through T − 8
- **WHEN** they all accept or reject concurrently and a ninth fresh process then starts
- **THEN** the ninth process's effective policy is the one issued at T − 1

#### Scenario: Networks and authorities keep separate watermarks

- **GIVEN** a watermark issued at T for authority K on network A
- **WHEN** a profile for network B, whose identity differs from A's only in its deployment ID, starts with authority K
- **THEN** B has no watermark and can accept an authentic policy issued before T
- **AND** when authority K2 is effective on network A, K's watermark is not used, and K's policy file copied into K2's directory fails verification

### Requirement: An expired policy keeps its watermark and blocks new work until a newer policy is accepted

Expiry SHALL NOT erase or lower the watermark (book line 650). A stored policy whose `valid_until` has passed SHALL still be restored as the watermark when a process starts. While the effective policy is expired, the version check SHALL fail. A native client then refuses the operations listed in *A required update refuses new funding and native inference but keeps recovery reachable*, and a provider stops new admission as *A provider whose own version policy is unhealthy admits no new work* requires. Refreshing an expired policy SHALL accept only a policy that passes the watermark of the expired one.

#### Scenario: A restart after expiry keeps the watermark

- **GIVEN** a stored authentic policy P issued at 200 with `valid_until` 300
- **WHEN** the process restarts at time 400
- **THEN** P is restored as the watermark and the version check fails as expired
- **AND** an authentic policy issued at 100 is rejected, an authentic policy issued at 200 with different fields is rejected, and an authentic policy issued at 350 with `valid_until` 500 is accepted

#### Scenario: An expired policy refuses native funding

- **GIVEN** a consumer whose stored policy has expired, with no `policy_url` or with a `policy_url` that offers no newer policy
- **WHEN** it is asked to fund, top up or run inference
- **THEN** the request is refused with a version-policy error (the expiry, or the fetched policy's rejection or unavailability) before any transaction is signed or any provider is contacted

### Requirement: A corrupt or insecure policy cache fails closed

A process SHALL fail its policy step when the stored policy cannot be read, exceeds the size limit, cannot be parsed, does not verify against the effective authority, has an `issued_at` in the future, has malformed versions, or has a nonzero `valid_until` that is not later than its `issued_at` (book line 650; `CLAUDE.md`: "Corrupt/insecure policy persistence fails closed"). It SHALL also fail when the policy file, its lock file or its directories are symbolic links, are not owned by the current user, or (on Unix) are accessible to the group or to others.

In each case the process SHALL NOT fall back to the bundled policy, SHALL NOT treat the cache as empty, and SHALL NOT overwrite or delete the stored file. A consumer whose cache fails SHALL NOT fund, top up or run native inference. A provider daemon or relay whose cache fails SHALL NOT start. Whether a consumer profile whose cache fails still loads, so that the recovery operations of *A required update refuses new funding and native inference but keeps recovery reachable* stay reachable, is not decided by this requirement (design open question 1).

#### Scenario: A corrupt cache does not fall back

- **GIVEN** a stored policy file that holds `{`, or an authentic policy with one field changed, or an unsigned policy
- **WHEN** a process restores the watermark or is offered the original authentic policy
- **THEN** both fail with an error, the stored file is unchanged byte for byte, and no effective policy is set

#### Scenario: Insecure permissions fail closed

- **GIVEN** an authentic stored policy whose `policy.json` or `policy.lock` has mode `0644`, or whose `policy.json` is a symbolic link to a saved copy of the policy
- **WHEN** a process restores the watermark
- **THEN** the restore fails with an error

#### Scenario: A corrupt cache stops new work

- **GIVEN** a corrupt policy cache for network A
- **WHEN** the desktop or the CLI loads a consumer profile for network A and is asked to fund, top up or run inference
- **THEN** the load or the request fails with the policy error, no transaction is signed, no provider is contacted, and the cache file is unchanged

### Requirement: The placeholder authority is private-development behavior only

The all-zero placeholder authority SHALL verify no signature (book line 650). Only while the effective authority is the placeholder MAY a process make an unsigned policy effective, in memory only, and only when the unsigned policy's `issued_at` is not earlier than the current effective policy's, the current effective policy is unsigned too, and the unsigned policy's `min_required` is not above the bundled floor. An unsigned policy SHALL NOT be stored, SHALL NOT become a watermark, SHALL NOT replace a signed effective policy, and SHALL NOT raise `min_required` above the bundled floor. While the effective authority is not the placeholder, unsigned policies SHALL be rejected.

A provider daemon or relay that is not in explicit development mode SHALL refuse to start unless it is given a signed policy that verifies under its effective authority and passes its watermark (book lines 650, 739 and 769). A process whose effective authority is the placeholder can therefore never serve outside development mode.

#### Scenario: The placeholder verifies no signature

- **GIVEN** the placeholder as the effective authority
- **WHEN** a policy carrying any 64-byte signature is offered
- **THEN** its signature does not verify and the policy is not accepted as signed

#### Scenario: An unsigned policy cannot tighten or persist

- **GIVEN** the placeholder authority and the bundled policy with `min_required` `0.0.0`
- **WHEN** an unsigned policy with `min_required` `9.9.9` is offered
- **THEN** it is rejected
- **AND** an unsigned policy with `min_required` `0.0.0` and `recommended` `9.9.9` is accepted in memory only: no policy file is written, and a new process starts without it

#### Scenario: An unsigned policy cannot displace or block a signed one

- **GIVEN** a signed effective policy
- **WHEN** a newer unsigned policy is offered
- **THEN** it is rejected
- **AND** an unsigned in-memory policy with the largest possible `issued_at` does not stop an authentic signed policy issued at 100 from being accepted under a real authority

#### Scenario: A public provider or relay needs an authentic policy

- **GIVEN** a provider configuration or relay configuration with development mode off
- **WHEN** it is started with no policy file, or with a policy that does not verify under the effective authority (including any policy under the placeholder authority)
- **THEN** it exits with an error before binding any listener

### Requirement: The authority's private signing key stays offline with the operator

Only the operator's offline signer SHALL hold the policy authority's private key (book line 648; `CLAUDE.md`: "signing keys stay with the operator/offline signer"). No client, relay, provider or build SHALL need it.

The signer SHALL read the private key only from an owner-private file, and SHALL refuse a file that is a symbolic link or (on Unix) accessible to the group or to others. It SHALL NOT accept the private key as a command-line argument or an environment variable, and SHALL NOT write it to standard output, standard error or a log (book line 474: "Do not place private keys or bearer tokens on command lines"; `CLAUDE.md`: operator secrets "use protected files, not command-line arguments ... logs"). Key generation SHALL write the new private key to a new owner-private file, SHALL refuse to overwrite an existing file, and SHALL print only the public key and the file's path. The signer SHALL sign exactly the encoding that clients verify. It SHALL be able to verify a policy against an explicitly given public key.

#### Scenario: Key generation writes a protected file

- **WHEN** the operator generates a key pair with an output path that does not exist
- **THEN** the private key is written to that path with owner-only permissions
- **AND** standard output shows only the public key and the path, never the private key
- **AND** running the command again with the same path fails and leaves the file unchanged

#### Scenario: The private key never appears on a command line

- **WHEN** the operator signs a policy
- **THEN** the signer takes the path of the private-key file, and offers no argument or environment variable that carries the private key itself
- **AND** a key file with mode `0644` is refused before anything is signed

#### Scenario: A signature round-trips to clients

- **GIVEN** a policy signed by the signer with the private key of authority K
- **WHEN** a client whose effective authority is K checks it
- **THEN** the policy is accepted
- **AND** a client whose effective authority is another key rejects it

### Requirement: Policies come only from configured sources

The policy authority SHALL come only from the compiled key or the runtime override, never from peers, offers, catalogs or provider responses (book line 63). Policy documents SHALL reach each component only as follows:

- A relay SHALL serve its effective policy at `GET /api/version-policy` (book line 43). Outside development mode it SHALL load a signed policy from its configured `version_policy_file` at start, after restoring its watermark, and SHALL refuse to start if the file is missing, exceeds its size limit (design decision 12), does not verify, or does not pass the watermark.
- A provider daemon outside development mode SHALL be started with `--version-policy` naming an offline-signed policy (book line 739), checked the same way. It MAY also serve its effective policy on its public listener, as it does today at `GET /api/version-policy`; the book does not require this.
- A native consumer SHALL fetch policies only from its profile's `network.policy_url` when that field is set (book line 460). The URL SHALL follow the profile's endpoint rules: HTTPS for a remote host, literal-loopback HTTP only for explicit development, and no credentials, query or fragment. A fetched policy SHALL pass *Only an authentic, well-formed, current policy is accepted*. When it does not, or when it cannot be fetched, the version check SHALL fail; the failure SHALL NOT be ignored. When `policy_url` is null, the consumer uses its stored watermark, or the bundled policy when none is stored.

The bundled policy (`min_required` `0.0.0`, `recommended` equal to the build's own version, unsigned) SHALL never require an update.

#### Scenario: A public relay refuses to start without a policy

- **GIVEN** a relay configuration with development mode off and no `version_policy_file`
- **WHEN** the relay starts
- **THEN** it exits with an error before binding its listener

#### Scenario: A relay serves the policy it accepted

- **GIVEN** a relay started with an authentic policy P
- **WHEN** a client requests `GET /api/version-policy`
- **THEN** the response is P with all of its fields, including its signature

#### Scenario: A rejected or unavailable fetched policy fails the check

- **GIVEN** a consumer whose `policy_url` returns a policy with an invalid signature, a policy older than the watermark, an expired policy, or no response within the fetch timeout
- **WHEN** the consumer checks its version status
- **THEN** the check fails with an error and the watermark is unchanged

#### Scenario: An unsafe policy URL is refused

- **WHEN** a consumer profile sets `policy_url` to a URL with credentials, a query or a fragment, or to plain HTTP on a non-loopback host
- **THEN** the profile is refused and no request is sent to that URL

### Requirement: Native clients state their version and providers refuse a stated version below the minimum

The native consumer SHALL send `X-Chiral-Client-Version` with its own version on every quote proposal, owner challenge, owner-authorized control request and native inference request (book lines 343, 581 and 648: "native control/onboarding supplies its version").

A provider's public listener SHALL check the header when it is present, before the request reaches its route (book line 343). A version below the provider's effective `min_required` SHALL be answered with HTTP 426 and error code `upgrade_required`. A malformed version SHALL be answered with HTTP 400 and error code `invalid_version`. A request without the header SHALL NOT be refused for lacking it, so stock OpenAI-compatible clients using exported credentials need no Chiral header (book lines 343 and 581). The native consumer SHALL report a 426 answer as an upgrade requirement.

#### Scenario: An outdated native client is refused

- **GIVEN** a provider whose effective policy has `min_required` `2.0.0`
- **WHEN** a request with `X-Chiral-Client-Version: 1.9.0` is sent to `POST /v1/contracts/propose`
- **THEN** the provider answers HTTP 426 with code `upgrade_required` and stores no quote

#### Scenario: A malformed stated version is refused

- **WHEN** a request with `X-Chiral-Client-Version: banana` reaches the provider's public listener
- **THEN** the provider answers HTTP 400 with code `invalid_version`

#### Scenario: A stock client without the header is not refused for version

- **GIVEN** a provider whose effective policy has `min_required` `2.0.0`
- **WHEN** an inference request with a valid exported credential and no `X-Chiral-Client-Version` header arrives
- **THEN** it is not refused for its version; authentication and admission apply as usual

#### Scenario: Native control and inference requests carry the version

- **WHEN** the native consumer sends a quote proposal, an owner challenge, an owner-authorized request and an inference request
- **THEN** each carries `X-Chiral-Client-Version` equal to the build's own version

### Requirement: A required update refuses new funding and native inference but keeps recovery reachable

While the client's version status is `required`, and while the version check fails (an expired policy, a rejected or unavailable fetched policy, or a malformed version), the native consumer (the desktop's native commands and the CLI) SHALL refuse these operations before it signs or broadcasts a transaction, contacts a provider, or writes account state (book lines 646–652 and 846):

- opening funding (`fund`);
- top-ups (`topup`);
- native inference (`infer` and streamed inference).

The client's own version check SHALL NOT refuse operations that inspect, reconcile or cancel existing state (`CLAUDE.md`: "historical funding recovery must remain reachable without abandoning accepted balances" and "Request status/receipts must support ambiguous-outcome recovery"; book line 814). These are: the pending-funding view, funding recovery (`funding_recovery`), explicit funding cancellation (`cancel_funding`), request status and request cancellation, credential revocation, local account, agreement and request views, wallet balance, history and export, logout, and quit. An operation that reaches a provider remains subject to that provider's own check (*Native clients state their version and providers refuse a stated version below the minimum*).

A required update or a failed check SHALL NOT delete, rewrite, rotate or cancel any account, journal entry, acknowledgement, credential or request record (book line 652). Whether `resume`, `discover`, `quote`, credential issuance and rotation, and `feedback` are refused while an update is required, and whether a profile whose policy cache is corrupt or insecure loads at all, are not decided by this requirement (design decision 5 and open question 1).

#### Scenario: A required update refuses new money and new inference

- **GIVEN** a consumer whose authentic effective policy has `min_required` above the build's version
- **WHEN** it is asked to fund a new agreement, top up an account, or run streamed or non-streamed inference
- **THEN** each request fails with an error that says a client upgrade is required
- **AND** no transaction is signed or journaled, no provider is contacted, and no account file is created or changed

#### Scenario: Recovery stays reachable

- **GIVEN** the same consumer with a pending journaled top-up, an open account with a credential, and an earlier request
- **WHEN** it runs funding recovery, cancels the pending funding with the gas and race acceptance, reads the request's status, and revokes the credential
- **THEN** none of these is refused by the client's own version check

#### Scenario: A policy that cannot be checked refuses the same new work

- **GIVEN** a consumer whose `policy_url` does not answer, or answers with an expired policy
- **WHEN** it is asked to fund, top up or run inference
- **THEN** each request fails with the policy error before any transaction is signed or any provider is contacted
- **AND** funding recovery, funding cancellation and request status are not refused by the client's own version check

#### Scenario: A required update erases nothing

- **GIVEN** a consumer with accounts, issued credentials and journaled funding when its policy starts to require an update
- **WHEN** it restarts while the update is still required
- **THEN** every account file, credential and journal entry is unchanged

### Requirement: A provider whose own version policy is unhealthy admits no new work

A provider SHALL treat its version policy as a serving gate (book line 630: existing credit stays usable "when the serving, ledger and policy gates remain healthy"). While its own effective policy is expired, its own version is below `min_required`, or its version check fails, the provider SHALL NOT accept new admission, SHALL NOT report funding readiness, and SHALL NOT publish offers.

This gate SHALL NOT delete or change agreements, balances, reservations, fee obligations or journals (book lines 630, 634 and 652). Requests already admitted settle under their own rules (`inference-request-accounting`, *Quote pressure and new-admission gates cannot strand settlement*).

#### Scenario: An expired provider policy stops new admission

- **GIVEN** a running provider whose effective policy has `valid_until` T
- **WHEN** its maintenance runs after T
- **THEN** it stops accepting new admission, reports `funding_ready` false, and publishes no new offer revision
- **AND** its accounts, balances and fee obligations are unchanged

#### Scenario: An outdated provider stops new admission

- **GIVEN** a provider started with an authentic policy whose `min_required` is above the provider's own version
- **WHEN** its maintenance runs
- **THEN** it does not accept new admission and publishes no offer

### Requirement: The desktop blocks the shell for a required update and shows a session-dismissible banner for a recommended one

The desktop SHALL take its version status only from the native `get_version_status` command (book line 804; `desktop-gui-global-rules`, *The desktop GUI is a thin view over the native command boundary*), and SHALL show it as follows (book line 846):

- **Required:** a full-screen blocking modal. It SHALL state the current version and the minimum version, and SHALL show the policy's message, as plain text, when there is one (`desktop-gui-global-rules`, *Untrusted content renders as plain text*). While it is shown, no page, navigation control or shell action behind it SHALL be reachable by pointer, keyboard or assistive technology. The native Quit confirmation that the tray opens stays available (`desktop-window-lifecycle`, *The system tray offers Show and Quit*).
- **Copy download URL:** the modal SHALL offer "Copy download URL" only when the policy's `download_url` is an HTTPS URL without credentials. The action SHALL copy exactly that URL. For any other URL, the desktop SHALL NOT offer a copy or open action and SHALL NOT present the URL as a download link.
- **Recommended:** a dismissible banner at the top. Dismissal SHALL last for the app session: the banner stays hidden through later refreshes, page changes and reloads until the app restarts. Its wording, including whether it names the versions, is presentation.
- **Ok:** nothing is shown.

The status SHALL be refreshed after every successful profile load (book line 960). Today it is also refreshed at startup and every hour (design decision 12); that cadence is not normative. After logout, or after a profile load, the desktop SHALL show no status derived from the previous network's policy (book line 815).

#### Scenario: The required modal blocks the shell

- **GIVEN** a native status of `required` with current version `0.4.0`, minimum `0.5.0` and message "Fixes payment verification"
- **THEN** the modal shows `0.4.0`, `0.5.0` and the message as plain text
- **AND** pressing Tab never moves focus to a control behind the modal, and pointer input behind it has no effect

#### Scenario: Copy is offered only for an HTTPS URL

- **GIVEN** a required status whose `download_url` is `https://example.org/releases`
- **WHEN** the user chooses "Copy download URL"
- **THEN** the clipboard holds `https://example.org/releases`
- **AND** with `download_url` `http://example.org/releases` or `https://user@example.org/releases`, no copy action is offered and the URL is not presented as a download link

#### Scenario: A dismissed banner stays hidden for the app session

- **GIVEN** a `recommended` status and a dismissed banner
- **WHEN** the status is refreshed, the user changes pages, and the webview reloads
- **THEN** the banner stays hidden
- **AND** after the app restarts with the status still `recommended`, the banner is shown again

#### Scenario: Logout drops the previous network's status

- **GIVEN** a loaded profile for network A whose policy makes the status `recommended`, with the banner shown
- **WHEN** the user logs out
- **THEN** the banner for network A's policy is no longer shown, and later refreshes do not show it while no profile is loaded

#### Scenario: Loading a profile refreshes the status

- **GIVEN** a loaded profile whose status is `ok`
- **WHEN** the user loads a profile for a network whose authentic policy requires an update
- **THEN** the required modal is shown once the load succeeds

### Requirement: Upgrades preserve accepted state and the policy watermark

Installing a new client or service version SHALL preserve accepted ledgers, acknowledgements, identity, shared nonce journals, credentials, evidence and the policy watermark (book lines 652, 673 and 675). An upgrade SHALL NOT delete or reset that state, and SHALL NOT convert accepted agreements or credit (book line 675). The policy watermark SHALL be kept under the configured `data_dir`, so that the protected backup taken before an upgrade includes it (book line 675).

#### Scenario: An upgrade keeps accounts and the watermark

- **GIVEN** a client at version `0.4.0` with accounts, credentials, journaled funding and a watermark P under its `data_dir`
- **WHEN** it is replaced by version `0.5.0` that uses the same `data_dir` and authority
- **THEN** every account, credential and journal entry is present and unchanged, P is still the watermark, and existing accounts can be used without new funding or credential rotation

#### Scenario: The backup includes the watermark

- **GIVEN** an operator taking the protected pre-upgrade backup of `data_dir`
- **THEN** the backup contains the `version-policy` directory with the stored policy of each network and authority
