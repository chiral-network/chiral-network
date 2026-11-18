## ADDED Requirements

### Requirement: The CLI gateway is a foreground command bound to loopback only

The `chiral` CLI SHALL provide the loopback gateway as the foreground command `chiral --config <consumer profile> gateway serve --port <port>`. The command SHALL load exactly the consumer profile named by `--config` (or by `CHIRAL_CONSUMER_CONFIG`, as for every `chiral` command) and SHALL serve only that profile's network identity and consumer wallet for the life of the process. A profile without a consumer wallet SHALL be refused with an error naming the missing wallet, and no listener SHALL be opened. `--port` SHALL be required and SHALL be an integer from 1,024 to 65,535; there SHALL be no default port. No gateway socket SHALL exist unless this command is running.

The gateway SHALL bind exactly one TCP listener, on the IPv4 literal loopback address `127.0.0.1` at that port, under the bind, peer-address and no-fallback rules of `desktop-local-api-server`, *The local API server is optional, off by default and bound to loopback only*:

- it SHALL NOT bind `0.0.0.0`, `::`, `::1`, or a LAN, VPN or other interface address, and no option, environment variable or configuration field SHALL offer a non-loopback bind;
- it SHALL close any accepted connection whose peer address is not a loopback address;
- if the port is occupied, or equals a TCP port in the profile's `dht.listen` addresses, the command SHALL NOT serve, SHALL write the reason to stderr and nothing to stdout, SHALL exit with a nonzero status, and SHALL NOT choose another port or adopt the existing listener.

The command SHALL run until it is stopped as *The CLI gateway stops on interrupt or termination and cancels its in-flight requests* specifies. It SHALL NOT fork into the background or detach from its terminal, and no `chiral` command SHALL install a system service, launch agent, scheduled task or start-at-login entry for it.

#### Scenario: The gateway listens on loopback only

- **GIVEN** a consumer profile with a wallet
- **WHEN** `chiral --config consumer.json gateway serve --port 18080` has written its `ready` event
- **THEN** the only socket the process listens on for the gateway is `127.0.0.1:18080`
- **AND** a connection attempt to port 18080 on the machine's LAN address does not reach the gateway

#### Scenario: An occupied port is an error, not a fallback

- **GIVEN** another process already listens on `127.0.0.1:18080`
- **WHEN** the user runs `gateway serve --port 18080`
- **THEN** the command exits with a nonzero status, writes the reason to stderr and writes nothing to stdout
- **AND** no listener is opened on any other port

#### Scenario: The port is explicit

- **WHEN** the user runs `gateway serve` without `--port`, or with `--port 80` or `--port 70000`
- **THEN** the command exits with a nonzero status and opens no listener

#### Scenario: The DHT listen port is refused

- **GIVEN** a profile whose `dht.listen` contains `/ip4/0.0.0.0/tcp/4001`
- **WHEN** the user runs `gateway serve --port 4001`
- **THEN** the command exits with a nonzero status before binding and opens no listener

### Requirement: The CLI gateway stops on interrupt or termination and cancels its in-flight requests

On the first SIGINT or SIGTERM on Unix, or the first Ctrl-C on Windows (the signals `chiral_daemon` handles), the gateway SHALL stop accepting connections, close its listener, and write one `stopping` event to stdout carrying the signal and the number of requests in progress. It SHALL then cancel every request in progress:

- a request already dispatched to a provider SHALL be cancelled upstream in the same way as a client disconnect (`desktop-local-api-server`, *Paid routes keep request identity and never retry automatically*), and SHALL NOT be retried or re-sent, then or after a later start;
- a client whose response has not started SHALL receive HTTP 503 with error code `gateway_stopping` and, once a request key was chosen, the request identifiers;
- a client stream SHALL end with an error event that carries `gateway_stopping` and the request identifiers, without `[DONE]` or any other success marker.

When a dispatched request is cancelled this way, the gateway SHALL state on stderr that cancelled requests can still be charged for generated tokens and that `chiral requests <contract>` lists their request keys. It SHALL write one `request` event for each cancelled request, with outcome `gateway_stopping` and delivery `possibly_charged` when any byte may have reached the provider, then one `stopped` event carrying the number of cancelled requests, and SHALL exit with status 0 within 10 seconds of the first signal. A second such signal during shutdown SHALL end the process at once with a nonzero status.

Persisted request-key records SHALL be kept. No part of the gateway SHALL keep running after the process ends, and its port SHALL be free when the process has exited.

#### Scenario: Interrupt during a paid stream

- **GIVEN** a client streaming a chat completion through a network route of a serving gateway
- **WHEN** the gateway process receives SIGINT
- **THEN** stdout carries, in order, `stopping` with one request in progress, a `request` event with outcome `gateway_stopping` and delivery `possibly_charged`, and `stopped` with one cancelled request
- **AND** the client stream ends with an error event carrying `gateway_stopping` and `x-chiral-request-key`, without `[DONE]`
- **AND** the provider received exactly one request, stderr states that cancelled requests can still be charged, and the process exits with status 0 within 10 seconds

#### Scenario: Termination with nothing in flight

- **GIVEN** a serving gateway with no request in progress
- **WHEN** the process receives SIGTERM
- **THEN** stdout carries `stopping` with zero requests in progress and `stopped` with zero cancelled requests
- **AND** the process exits with status 0 and nothing listens on the gateway's port

#### Scenario: A second signal ends shutdown at once

- **GIVEN** a gateway that has written `stopping`
- **WHEN** the process receives a second SIGINT
- **THEN** the process ends at once with a nonzero status

#### Scenario: A cancelled request is not resent after restart

- **GIVEN** a request cancelled by a gateway shutdown with request key `K`
- **WHEN** the gateway is started again and serves other requests
- **THEN** the provider receives no further request for `K`
- **AND** `chiral requests <contract>` still lists `K`

### Requirement: CLI gateway state is protected and scoped to the profile's network and consumer wallet

The gateway's persistent state SHALL be one directory, `gateway/`, inside the consumer's network-scoped state directory for the profile's consumer wallet, the directory that already holds that wallet's accounts and request keys (today `<data_dir>/inference/<network namespace>/consumer/<wallet address>/gateway/`). It SHALL hold the bearer file, the route table and their lock files, and no other secret. The directory and both files SHALL be created owner-only, with the same Unix and Windows protection as other native secret files.

The gateway and its commands SHALL NOT read or write the desktop local server's `<app data>/local-server/` state. A profile with a different network identity or a different consumer wallet resolves a different directory: its gateway commands SHALL NOT show, use or change another profile's routes or bearer.

A gateway directory, bearer file or route table that is group- or world-accessible on Unix or not owner-protected on Windows, is a symbolic link, cannot be read, or is malformed SHALL make the command that reads it fail closed: the command SHALL report the problem on stderr, exit with a nonzero status, and SHALL NOT repair, widen, replace or delete it. The one exception is a route table that becomes so while `gateway serve` is already serving: the gateway SHALL keep running and refuse requests as *CLI gateway routes map names to funded network accounts only* specifies, and SHALL still NOT repair, widen, replace or delete the file. A route table that is not strict JSON under the route-table schema, holds more than 64 routes, repeats a route ID or route name, or names one credential in two routes is malformed.

#### Scenario: Another network or wallet sees nothing

- **GIVEN** routes and a bearer created with profile `A` on network `N1` and wallet `W1`
- **WHEN** `gateway show` runs with a profile on network `N2`, or with wallet `W2` on `N1`, over the same `data_dir`
- **THEN** it lists none of `A`'s routes and gives a different bearer file path
- **AND** a gateway served with that profile answers 404 `model_not_found` for `A`'s route names

#### Scenario: An insecure route table fails closed

- **GIVEN** a route table that other users of the machine can read
- **WHEN** the user runs `gateway serve` or a route command
- **THEN** the command exits with a nonzero status and reports the insecure file, and `gateway serve` writes no `ready` event and opens no listener
- **AND** the file's permissions and contents are unchanged

#### Scenario: A symbolic-link bearer file fails closed

- **GIVEN** the gateway's bearer file is a symbolic link
- **WHEN** the user runs `gateway serve`
- **THEN** the command exits with a nonzero status, opens no listener and leaves the link and its target unchanged

### Requirement: The CLI gateway authenticates requests with a protected bearer that no command prints

The gateway SHALL authenticate every request with a bearer stored as the protected file `bearer` in its gateway directory. Generation, storage, `Authorization: Bearer` acceptance, constant-time comparison, the 401 `invalid_api_key` refusal, fail-closed file handling and distinctness from every other secret SHALL follow `desktop-local-api-server`, *Local API requests are authenticated by a protected, locally generated bearer*. That requirement's Reveal, masked display and webview clauses apply only to the desktop. `gateway serve` SHALL generate the bearer when no bearer file exists, before it writes `ready`.

No `chiral` command, stdout event, error, diagnostic or log SHALL contain the bearer value. The `ready` event and `gateway show` SHALL give only the bearer file's absolute path. The CLI SHALL NOT accept the bearer value as a command-line argument, environment variable or standard-input value, and SHALL NOT pass it in any child process's arguments. There SHALL be no CLI command that reveals it; a program obtains it by reading the protected file.

`chiral gateway regenerate-bearer` SHALL atomically replace the bearer file with a newly generated bearer written with protected permissions, and SHALL print only the file's path. While a `gateway serve` process holds this gateway directory's serve lock, it SHALL refuse with a nonzero status, without changing the file, stating that the gateway must be stopped first. After a regeneration, the next `gateway serve` SHALL reject the previous bearer.

#### Scenario: The first serve creates the bearer and prints its path

- **GIVEN** no bearer file exists
- **WHEN** the user runs `gateway serve --port 18080`
- **THEN** the bearer file exists, owner-only, with at least 256 bits of entropy
- **AND** the `ready` event carries the file's absolute path, and neither stdout nor stderr contains the value

#### Scenario: Missing or wrong bearer is refused before dispatch

- **GIVEN** a serving gateway with an enabled route
- **WHEN** a request arrives with no `Authorization` header, a wrong bearer, or the correct bearer only in a query parameter
- **THEN** the gateway responds 401 `invalid_api_key`
- **AND** no provider is contacted

#### Scenario: Regeneration waits for the gateway to stop

- **GIVEN** a serving gateway
- **WHEN** the user runs `gateway regenerate-bearer` for the same profile
- **THEN** the command exits with a nonzero status, states that the gateway must be stopped first, and leaves the bearer file unchanged

#### Scenario: Regeneration invalidates the previous bearer

- **GIVEN** no gateway is serving and a tool holds the current bearer
- **WHEN** the user runs `gateway regenerate-bearer` and then `gateway serve`
- **THEN** a request with the previous bearer is refused with 401
- **AND** a request with the new bearer read from the file is accepted

#### Scenario: The bearer value appears nowhere else

- **GIVEN** a known bearer value
- **WHEN** requests are served and every `chiral gateway` command is run
- **THEN** the value appears in no stdout output, stderr output, process command line or upstream request

### Requirement: The CLI gateway applies the local API server's request gate, endpoint subset and bounds

The gateway SHALL apply, at its own bound port, the requirements *Browser-origin and unexpected-host requests are refused*, *The local API server exposes only a bounded OpenAI-compatible endpoint subset* and *Local API requests have fixed size, concurrency and duration bounds* of `desktop-local-api-server`, with their status codes, error codes, error object and numeric bounds unchanged. That covers the connection bounds first, then the exact `Host` allowlist (`127.0.0.1:<port>` and `localhost:<port>`), refusal of any `Origin` with no CORS headers, and the bearer check, all before the body is read, followed by the six-route subset and the request and response bounds. Only these differences SHALL apply:

- every route of the CLI gateway is a network route, so `GET /v1/models` SHALL list exactly the enabled route names with `owned_by` equal to `network`, and `GET /v1/requests/:id` and `POST /v1/requests/:id/cancel` SHALL be served, resolving requests as *CLI gateway requests follow the local API server's forwarding, identity and recovery rules* specifies;
- the clauses about the desktop webview SHALL NOT apply.

#### Scenario: DNS rebinding is refused

- **GIVEN** a serving gateway on port 18080
- **WHEN** a request with the valid bearer arrives with `Host: rebind.example:18080`
- **THEN** the gateway responds 421 `host_not_allowed`
- **AND** the body is not read and no provider is contacted

#### Scenario: A web page cannot use the gateway

- **WHEN** a request arrives carrying `Origin: https://evil.example` or `Origin: null`, with or without a valid bearer
- **THEN** the gateway responds 403 `origin_not_allowed` with no CORS headers, and no provider is contacted

#### Scenario: Excess concurrency is refused, not queued

- **GIVEN** 16 requests are in progress on the gateway
- **WHEN** a 17th authenticated request arrives
- **THEN** it receives 429 `local_server_busy` immediately and is never dispatched

#### Scenario: Control and engine paths are not reachable

- **WHEN** an authenticated request targets `/tokenize`, `/metrics`, `/admin/status`, `/v1/contracts/propose` or `/v1/contracts/abc/credentials`
- **THEN** the gateway responds 404 `not_found` and no provider is contacted

#### Scenario: The model list is local and network-only

- **GIVEN** two enabled routes and one disabled route, with outbound network access blocked
- **WHEN** an authenticated client calls `GET /v1/models`
- **THEN** the response lists exactly the two enabled route names, each with `owned_by` `network`
- **AND** no outbound connection is attempted

### Requirement: CLI gateway routes map names to funded network accounts only

The gateway SHALL route requests by its route table under the network-target rules of `desktop-local-api-server`, *Each route name maps to exactly one local model or one funded network account*. The table SHALL hold at most 64 routes and no secrets. Each route SHALL contain:

- a route ID: a random 128-bit value from the operating system's secure random source, assigned at creation and never changed or given to another route;
- a route name of 1–256 bytes of UTF-8 without control characters, unique by exact bytes;
- an enabled flag;
- a network target: the complete `NetworkIdentity` and consumer wallet address of the profile that created it, one account (contract) ID from that wallet's account store, and the ID of exactly one credential issued for that route, with the scopes, spend cap, maximum concurrency and expiry it was issued with.

Only network targets SHALL exist. The CLI SHALL NOT accept a local model, an installed-model identity, a provider URL, an offer, a bearer or a credential secret as a target, and the gateway SHALL NOT launch, load or contact any model engine.

The route table SHALL change only through these commands, each of which prints one JSON document of route metadata on success:

- `chiral gateway route add <name> --contract <id> --spend-cap-wei <wei> --expires-in-secs <seconds> [--max-concurrency <n>] --accept-spending-risk`;
- `chiral gateway route renew <name> --spend-cap-wei <wei> --expires-in-secs <seconds> [--max-concurrency <n>] --accept-spending-risk [--revoke-old-credential]`;
- `chiral gateway route enable <name> --accept-spending-risk` and `chiral gateway route disable <name>`;
- `chiral gateway route remove <name> [--revoke-credential]`.

Each change SHALL be written atomically, by replacing the whole file, and SHALL be serialized across processes by a route-table lock that is held only while that change is made. Under that lock, each command SHALL recheck its preconditions (name uniqueness, the route's existence and the 64-route limit) before it commits. Route commands SHALL be allowed while a gateway serves the same gateway directory. A serving gateway SHALL apply each committed change to every request whose route it resolves after the commit, without a restart, and a request already in progress SHALL keep the route it was resolved to. If the route table cannot be read, or is insecure, a symbolic link or malformed, while the gateway serves, the gateway SHALL refuse requests with 503 `route_unavailable` without contacting any provider and SHALL report the problem on stderr, until the table is valid again.

#### Scenario: Route names are unique

- **GIVEN** a route named `assistant`
- **WHEN** the user runs `gateway route add assistant` for another account
- **THEN** the command exits with a nonzero status, issues no credential and leaves the existing route unchanged

#### Scenario: Only network targets exist

- **WHEN** the user runs `gateway route add` with a provider URL, a local model path or a credential secret as `--contract`, or with a contract ID that is not in the profile wallet's account store
- **THEN** the command refuses it before any provider call
- **AND** no route, credential or engine process is created

#### Scenario: A route added while serving is used without restart

- **GIVEN** a serving gateway with no route named `writer`
- **WHEN** the user runs `gateway route add writer …` with the required flags and then a client posts a request naming `writer`
- **THEN** the gateway forwards the request to that route's account without having been restarted

#### Scenario: Disabling a route stops new requests only

- **GIVEN** a serving gateway with a stream in progress through route `assistant`
- **WHEN** the user runs `gateway route disable assistant`
- **THEN** the stream in progress continues on its route
- **AND** a new request naming `assistant` is refused with 404 `model_not_found` without contacting any provider

#### Scenario: Concurrent route commands lose no change

- **WHEN** two route commands adding different routes run at the same time against one gateway directory
- **THEN** the route table afterwards holds both routes

#### Scenario: A route table made insecure while serving

- **GIVEN** a serving gateway with an enabled route
- **WHEN** the route table is made readable by other users and a client then posts a request naming the route
- **THEN** the client receives 503 `route_unavailable`, no provider is contacted, and stderr reports the insecure file
- **AND** the gateway keeps running and the file's permissions and contents are unchanged

### Requirement: Each CLI route spends through a dedicated, capped and expiring credential

`gateway route add` and `gateway route renew` SHALL issue the route's credential through the native consumer's existing owner-authenticated `credential_issue`, with:

- scopes exactly `infer` and `read`;
- the `--spend-cap-wei` value, which SHALL be a positive canonical decimal wei string;
- an `expires_at` of `--expires-in-secs` seconds after issuance, where `--expires-in-secs` SHALL be from 60 to 2,592,000 (the Appendix A.5.3 credential lifetime bounds);
- a maximum concurrency of `--max-concurrency`, or 1 when it is omitted.

Issuing a route credential SHALL NOT make it the account's active credential. The CLI SHALL NOT bind a credential ID that the user supplies; every route credential is issued for its route. `route renew` SHALL bind the new credential to the same route ID.

`route add`, `route renew` and `route enable` SHALL refuse, stating the reason, a route whose credential lacks either scope, has a null or zero spend cap, is expired, revoked or no longer stored locally, is the account's active credential, or is bound to another route in the table, as `desktop-local-api-server`, *Each route name maps to exactly one local model or one funded network account* refuses them.

`route add`, `route renew` and `route enable` SHALL write the disclosure of that requirement to stderr on every run, before any provider call, as `chiral fund` always writes its funding disclosure. `--accept-spending-risk` SHALL be required by these commands. Without it, the command SHALL make no change and no provider call and SHALL exit with a nonzero status. The disclosure SHALL state:

- that any program able to read the gateway's bearer file can spend up to the route credential's spend cap, shown exactly in CFX converted from canonical wei without floating point, from that account's prepaid, non-refundable balance until the credential expires;
- that the cap limits authority on the same balance and is neither a separate balance nor a refund guarantee;
- that the provider sees prompts and responses in plaintext;
- that a client retry without a reused `Idempotency-Key` is a new paid request.

`route remove` and `route renew` SHALL NOT revoke a credential silently. Without `--revoke-credential` or `--revoke-old-credential`, their output SHALL name the credential that stays valid until its expiry, together with the `chiral credentials <contract> revoke <id>` command. With the flag, they SHALL revoke it through the native consumer's existing `credential_revoke`.

If `route add` or `route renew` has issued a credential and then cannot commit its change, for example because the recheck under the route-table lock finds the name taken, it SHALL exit with a nonzero status, write nothing to stdout, leave the route table unchanged, and SHALL NOT revoke that credential silently. Its stderr SHALL name the credential, state that it stays valid until its expiry, and give the `chiral credentials <contract> revoke <id>` command.

#### Scenario: A credential issued for a route that cannot be committed is reported

- **GIVEN** `gateway route add assistant …` has issued credential `K`, and another process has meanwhile committed a route named `assistant`
- **WHEN** the first command rechecks the table under the route-table lock
- **THEN** it exits with a nonzero status, writes nothing to stdout, and the table holds only the other process's route
- **AND** `K` is not revoked, and stderr names `K` and the `chiral credentials <contract> revoke` command

#### Scenario: A route credential is dedicated and capped

- **GIVEN** a funded account with no active credential
- **WHEN** the user runs `gateway route add assistant --contract C --spend-cap-wei 250000000000000000 --expires-in-secs 86400 --accept-spending-risk`
- **THEN** the provider issues one credential with scopes `infer` and `read`, spend cap `250000000000000000` and an expiry 86,400 seconds ahead
- **AND** the account still has no active credential, and the route records that credential's ID, scopes, cap and expiry
- **AND** the spending disclosure, with the cap shown exactly in CFX, was written to stderr before the provider call

#### Scenario: The spending disclosure must be accepted

- **WHEN** the user runs `gateway route add` without `--accept-spending-risk`
- **THEN** the command exits with a nonzero status, writes the disclosure with the cap shown exactly in CFX to stderr, and makes no provider call
- **AND** no route is added

#### Scenario: A route needs an explicit positive cap and a bounded expiry

- **WHEN** the user runs `gateway route add` with no `--spend-cap-wei`, with `--spend-cap-wei 0`, or with `--expires-in-secs 2592001`
- **THEN** the command refuses before any provider call, and no credential or route is created

#### Scenario: Removing a route does not revoke silently

- **GIVEN** a route whose credential is `K`
- **WHEN** the user runs `gateway route remove` for it without `--revoke-credential`
- **THEN** the route is removed, `K` is not revoked, and the output names `K` and the `chiral credentials <contract> revoke` command

#### Scenario: Renewal keeps the route ID

- **GIVEN** a route with route ID `R` whose credential has expired
- **WHEN** the user runs `gateway route renew` for it with a new cap, a new expiry and `--accept-spending-risk`
- **THEN** the route keeps route ID `R` and names the newly issued credential
- **AND** requests recorded earlier under `R` still resolve through `GET /v1/requests/:id`

### Requirement: CLI gateway requests follow the local API server's forwarding, identity and recovery rules

The gateway SHALL forward, identify and recover every request as `desktop-local-api-server` requires of a network route in these requirements:

- *Network routes forward through the native consumer path with the credential held natively*;
- *Paid routes keep request identity and never retry automatically*;
- *Paid requests remain recoverable through request status*;
- *libp2p forwarding follows the peer-to-peer transport contract*.

This includes the native consumer's transport selection over HTTPS or libp2p, the route credential as the only credential, the response-header allowlist, the classified errors without provider text, the exact free-allowance and qualification-pause mappings with their `Retry-After` rule, the 502 `provider_error` without `Location` for a provider 3xx, the request key persisted before dispatch with the gateway's route ID, no automatic retry on any path, and the `transport_unavailable`, `provider_authentication_failed` and `provider_unreachable` responses. Only these substitutions SHALL apply:

- **Loaded profile.** The loaded consumer profile is the `--config` profile, for the life of the process. A route whose recorded `NetworkIdentity` or consumer wallet differs from that profile's SHALL be refused with 503 `route_unavailable` without contacting any provider.
- **libp2p node.** The profile's libp2p node is its configured `dht` node (`p2p-provider-transport`, *Consumer libp2p use is configured explicitly*). Once `p2p-provider-transport` is implemented and the profile configures `dht`, `gateway serve` SHALL start that node before it writes `ready`. If the node cannot start, the command SHALL fail as a failure before `ready`: it SHALL write the reason to stderr and nothing to stdout, close any listener it opened, and exit with a nonzero status. A profile without `dht` SHALL serve, and libp2p-only accounts SHALL receive 503 `transport_unavailable`.
- **Credential errors.** The `no_credential` message SHALL direct the user to `chiral gateway route renew` instead of the Local server page.
- **Request routes.** `GET /v1/requests/:id` and `POST /v1/requests/:id/cancel` SHALL resolve `:id` only to a record whose route ID is a current, enabled route of this gateway's table that still targets the record's account. Records made by the desktop local server, by `chiral infer` or by a removed or retargeted route SHALL give 404 `not_found` without contacting any provider.
- **Recovery outside the gateway.** In place of the desktop's Usage & API keys page, `chiral requests <contract>` SHALL list each gateway request's request key, route ID, creation time and provider request ID when known. `chiral request <contract> <id> [--cancel] [--credential <credential id>]` SHALL read the status of, or cancel, a request by provider request ID or request key. With `--credential`, it SHALL use exactly that stored credential, with no fallback to the active credential. Both commands SHALL work while no gateway is running and after a restart, and `--cancel` SHALL write to stderr that a cancelled request can still be charged and that cancellation is not a refund, while its stdout stays the native JSON result.

#### Scenario: An interrupted stream is not retried

- **GIVEN** a controlled provider that cuts the stream after the first event
- **WHEN** a client streams a chat completion through a gateway route
- **THEN** the provider receives exactly one request
- **AND** the client stream ends with an error event carrying `x-chiral-request-key` and the provider request ID, without `[DONE]`

#### Scenario: A provider redirect is not passed to the SDK

- **GIVEN** a controlled provider that answers with 302 and `Location: http://127.0.0.1:<other port>/v1/chat/completions`
- **WHEN** a client posts a request through a gateway route
- **THEN** the client receives 502 `provider_error` with no `Location` header
- **AND** no connection is made to the redirect target

#### Scenario: Recovery after the gateway has stopped

- **GIVEN** a request with `Idempotency-Key: job-7` through route `R`, whose connection failed after the body was written and before response headers arrived, and the gateway has since stopped
- **WHEN** the user runs `chiral requests <contract>` and then `chiral request <contract> job-7 --credential <R's credential ID>`
- **THEN** `job-7` is listed with route `R`, and its authoritative status is read with exactly that credential
- **AND** no inference request is resent

#### Scenario: Another front end's request is not resolved

- **GIVEN** a request on account `A` recorded by the desktop local server, and a CLI gateway with a route to `A`
- **WHEN** a client calls `GET /v1/requests/<that request's ID>` on the CLI gateway
- **THEN** the gateway responds 404 `not_found` and contacts no provider

#### Scenario: A DHT node that cannot start stops the gateway before ready

- **GIVEN** `p2p-provider-transport` is implemented and a profile whose `dht.listen` port is already held by another process
- **WHEN** the user runs `gateway serve --port 18080` with that profile
- **THEN** the command exits with a nonzero status, writes the reason to stderr and nothing to stdout, and nothing listens on port 18080

#### Scenario: A route recorded under another identity is refused

- **GIVEN** a route table entry whose recorded `NetworkIdentity` differs from the loaded profile's
- **WHEN** a client posts a request naming that route
- **THEN** the gateway responds 503 `route_unavailable` and no provider is contacted

### Requirement: The CLI gateway reports each request's transport path without adding disclosures

The gateway's stdout events and `gateway show` are its consumer surfaces for the path rule of `p2p-provider-transport`, *The active path is disclosed before funding and during use*. Each `request` event for a request that was dispatched SHALL carry `transport` (`https` or `libp2p`) and `path`: `HTTPS`, `Direct`, `Hole-punched` or `Relayed`, observed natively on the connection that carried that request. For `Relayed`, the event SHALL also carry the relay's peer ID and the circuit limits the relay reported, or `null` with a statement that it reported none. A request reported as not sent SHALL carry `null` for both. Until `p2p-provider-transport` is implemented, every dispatched request's path SHALL be `HTTPS`.

Route listings SHALL show, from the account's retained signed offer as the native consumer's transport selection reads it, the declared transports, the provider's peer ID, and an endpoint only when one is declared. They SHALL show no observed path. The gateway SHALL NOT present any endpoint or base URL for a libp2p-only account other than its own `http://127.0.0.1:<port>/v1`.

#### Scenario: An HTTPS account

- **WHEN** a request through a route whose account is used over `https` completes
- **THEN** its `request` event carries transport `https` and path `HTTPS`, with no relay member

#### Scenario: A relayed request

- **GIVEN** `p2p-provider-transport` is implemented and a route's provider is reachable only through relay `R1`, which reported circuit limits
- **WHEN** a request through that route completes
- **THEN** its `request` event carries transport `libp2p`, path `Relayed`, `R1`'s peer ID and the reported limits

#### Scenario: The path changes during use

- **GIVEN** `p2p-provider-transport` is implemented and a stream in progress on a Relayed connection
- **WHEN** DCUtR upgrades the connection to the same provider and a later request completes
- **THEN** the stream's `request` event carries path `Relayed` and the later request's event carries path `Hole-punched`

#### Scenario: A libp2p-only route listing

- **GIVEN** a route whose account's retained offer declares only `libp2p` with peer ID `P` and a `null` endpoint
- **WHEN** the user runs `gateway show`
- **THEN** the route lists transports `["libp2p"]` and peer ID `P`, and no endpoint or provider base URL

### Requirement: The CLI gateway writes machine-readable events to stdout and diagnostics to stderr

While `gateway serve` runs, its stdout SHALL be JSON lines: each line SHALL be exactly one JSON object `{"kind": <string>, "data": <object>}`, the shape of `chiral infer --stream` events, written and flushed when the event occurs. The kinds SHALL be exactly these:

- `ready`, once, after the listener is bound and before any connection is accepted, with `address` (`127.0.0.1`), `port`, `base_url` (`http://127.0.0.1:<port>/v1`), `bearer_file` (absolute path), `network` (the complete `NetworkIdentity`), `consumer_wallet` and `routes`, listed as `gateway show` lists them;
- `request`, once for each request that passed the host, origin and bearer checks, when its response ends, with `time` (Unix seconds), `route_id`, `route`, `endpoint`, `stream`, `status` (the HTTP status returned), `outcome` (`ok` or the classified error code returned), `delivery` (`not_sent`, `provider_refused`, `completed` or `possibly_charged`), `duration_ms`, `usage_reported` (the response's usage token counts when present, otherwise `null`), `contract_id`, `request_key`, `request_id` (or `null`), and the path members of *The CLI gateway reports each request's transport path without adding disclosures*;
- `stopping` and `stopped`, as *The CLI gateway stops on interrupt or termination and cancels its in-flight requests* specifies.

A member that does not apply to a request, such as `route_id` when no route was resolved or `request_key` before one was chosen, SHALL be `null`. `delivery` describes inference requests: it SHALL be `null` for `GET /v1/models` and `GET /v1/requests/:id`, and `possibly_charged` for `POST /v1/requests/:id/cancel`. For each such cancellation, the gateway SHALL also state on stderr that the cancelled request can still be charged and that cancellation is not a refund. Together these are the CLI gateway's request-log label for cancellation under `desktop-local-api-server`, *Paid requests remain recoverable through request status*. `usage_reported` SHALL be labeled by its name as reported by the provider and not as authoritative billing. No event SHALL contain a locally computed cost. A request that failed the host, origin or bearer check SHALL produce no event.

`gateway show` SHALL print one JSON document, built from local state only without contacting any provider, chain or catalog, with `gateway_dir`, `bearer_file`, `bearer_present` and `routes`. Each route SHALL list `route_id`, `name`, `enabled`, `contract_id`, `model` (the agreed model ID), `provider_wallet`, `credential_id`, `scopes`, `spend_cap_wei` (canonical decimal wei), `max_concurrency`, `expires_at`, the transport members of *The CLI gateway reports each request's transport path without adding disclosures*, and `speed_label`. `speed_label` SHALL be derived only from the account's agreed signed offer: `Unqualified — no speed guarantee` when that offer has no serving profile and its model advertises `chat` or `completions`, whatever its prices; `No generation-speed guarantee` when it has no serving profile and advertises only `embeddings`; and `null` when it has a serving profile.

No stdout event or stderr output of any `chiral gateway` command SHALL contain a prompt, message, system prompt, completion, embedding input or vector, tool definition or argument, request or response body, the bearer, a provider credential or any other secret. Disclosures, warnings, diagnostics and errors SHALL go to stderr only. A failure before `ready` SHALL write nothing to stdout and SHALL exit with a nonzero status. `gateway show`, `gateway regenerate-bearer` and the route commands SHALL each write exactly one JSON document to stdout on success, as other `chiral` commands do, and on failure SHALL write the error to stderr, nothing to stdout, and exit with a nonzero status.

#### Scenario: The ready event comes first and carries no secret

- **WHEN** the user runs `gateway serve --port 18080` with stdout piped to a reader
- **THEN** the reader receives, before any request is sent, one line that parses as `{"kind":"ready","data":{…}}` with port 18080, base URL `http://127.0.0.1:18080/v1` and the bearer file path
- **AND** the line does not contain the bearer value

#### Scenario: Request events are metadata only

- **GIVEN** a request whose prompt and generated output contain known canary strings
- **WHEN** it completes through a gateway route
- **THEN** stdout carries one `request` event with the route, endpoint, status, outcome, delivery, request key and request ID
- **AND** the canary strings appear in no stdout or stderr output

#### Scenario: A cancellation is logged as possibly charged and not a refund

- **GIVEN** a stream in progress through a gateway route with provider request ID `R`
- **WHEN** an authenticated client posts `/v1/requests/R/cancel` and receives 202 `cancelling`
- **THEN** stdout carries a `request` event for the cancel with endpoint `/v1/requests/:id/cancel` and delivery `possibly_charged`
- **AND** stderr states that the cancelled request can still be charged and that cancellation is not a refund

#### Scenario: Unauthenticated requests produce no events

- **WHEN** 100 requests with a wrong bearer arrive
- **THEN** each receives 401 and stdout carries no `request` event for them

#### Scenario: An unqualified offer is labeled

- **GIVEN** a route whose account's agreed offer advertises `chat`, has no serving profile, and has input price, output price and minimum funding all `"0"`
- **WHEN** the user runs `gateway show`
- **THEN** the route's `speed_label` is `Unqualified — no speed guarantee`

### Requirement: The CLI gateway coexists with the desktop and other processes on one data directory

`gateway serve` SHALL NOT acquire the consumer process lock (`consumer.lock`), and network dispatch, request status and cancellation SHALL take no consumer process lock. Route commands SHALL hold the consumer process lock only inside the native credential operations they call. Funding, resume, top-up and credential operations of `chiral` and of the desktop over the same `data_dir` SHALL therefore never be refused as busy because a gateway is serving.

At most one `gateway serve` SHALL serve one gateway directory at a time. It SHALL hold a serve lock in that directory, separate from the consumer process lock, for its whole life. A second `gateway serve` for the same network identity and consumer wallet SHALL exit with a nonzero status before binding any port, stating on stderr that a gateway is already serving that profile, and SHALL NOT affect the running gateway. Gateways for different network identities or consumer wallets MAY serve at the same time on different ports.

The desktop's local API server and a CLI gateway MAY serve at the same time over the same `data_dir`. They SHALL keep separate bearers, route tables and route credentials: neither SHALL accept the other's bearer, and no credential SHALL be bound to a route of both. A port used by one is an occupied port for the other.

Request-key records SHALL be created exclusively, so that when a gateway and another process record the same account and request key at the same time, the store keeps one record and neither process overwrites it, except to add a provider request ID the record lacks.

#### Scenario: Account operations are not blocked by a serving gateway

- **GIVEN** a serving gateway
- **WHEN** the user runs `chiral credentials <contract> issue` and `chiral resume <contract>` with the same profile
- **THEN** neither is refused because consumer state is busy

#### Scenario: A second gateway on one profile is refused

- **GIVEN** a gateway serving profile `A` on port 18080
- **WHEN** the user runs `gateway serve --port 18081` with profile `A`
- **THEN** the second command exits with a nonzero status without binding port 18081
- **AND** the first gateway keeps serving

#### Scenario: Two networks serve side by side

- **GIVEN** two profiles on different networks over one `data_dir`
- **WHEN** a gateway serves each, on ports 18080 and 18081
- **THEN** each serves its own routes with its own bearer
- **AND** each refuses the other's bearer with 401

#### Scenario: The desktop server and the CLI gateway stay separate

- **GIVEN** the desktop local API server enabled on port 18080 with a profile over the same `data_dir`
- **WHEN** the user runs `gateway serve --port 18081`, and later `gateway serve --port 18080` with another profile
- **THEN** the first gateway serves, each of the two servers refuses the other's bearer with 401, and the second command fails because port 18080 is occupied

#### Scenario: One client key from two processes keeps one record

- **GIVEN** a CLI gateway route and a desktop local-server route to the same account
- **WHEN** both forward a request with `Idempotency-Key: job-42` at the same time
- **THEN** the account's request-key store holds one record for `job-42` that neither process overwrote
- **AND** the provider executes at most one of them, and the other client receives 409 `duplicate_request`

### Requirement: The CLI gateway performs no account operation over HTTP and is never advertised

While handling any request, the gateway SHALL NOT issue, rotate, revoke or export credentials, sign an owner challenge, request a quote, fund, top up, resume or cancel funding, or sign or send a chain transaction. Route commands SHALL issue and revoke credentials only through the native consumer's existing `credential_issue` and `credential_revoke`; every other account operation SHALL remain with the existing `chiral` commands and their disclosures.

The gateway SHALL follow `desktop-local-api-server`, *The local API server is never advertised and is unrelated to provider listeners*: it SHALL NOT be announced through DHT or provider records, manifests or signed offers, identify messages, relay registrations, catalog submissions, mDNS, DNS-SD or SSDP, or UPnP, NAT-PMP or PCP port mappings. Its address SHALL NOT be used as an offer endpoint or provider listener. It SHALL be a separate listener and router from any `chiral_daemon` public, administration or libp2p listener, and its bearer SHALL NOT be accepted as, or be, any of their tokens.

#### Scenario: Serving publishes nothing

- **GIVEN** a profile with a configured `dht` node
- **WHEN** a gateway serves on port 18080
- **THEN** no DHT record, manifest, offer, identify message, local-network announcement or port mapping contains its address or port

#### Scenario: HTTP gives no account control

- **WHEN** authenticated requests are sent to `/v1/contracts/propose`, `/v1/contracts/abc/topup` and `/v1/contracts/abc/credentials`
- **THEN** each receives 404 `not_found`
- **AND** no provider is contacted, nothing is signed with the wallet key and no chain transaction is sent
