## ADDED Requirements

### Requirement: The gateway requires its own protected inbound bearer on every bind address

`chiral_rpc_gateway` SHALL refuse to start unless `--bearer-file` names its inbound bearer, whatever the bind address. Literal loopback is not an exception: the reference topology places public TLS ingress in front of the gateway's loopback port (book line 656), so a loopback bind is not evidence that only local callers reach it. No option, environment variable, development mode or configuration field SHALL disable the bearer, and no gateway command given by the deployment files or the operator guide SHALL omit it. The gateway has its own inbound bearer (book line 644).

The bearer SHALL be supplied only as a path to a protected file, never as a command-line value, an environment variable or part of a URL (book line 474). The file SHALL be opened without following a symbolic link and SHALL be a regular file of 1 to 4,096 bytes. On Unix it SHALL be owned by the gateway's effective user with no group or other permission bits; on native Windows it SHALL be in owner-protected storage (book line 474). After surrounding whitespace is removed, the bearer SHALL consist of at least 32 characters, every one visible ASCII, because an HTTP `Authorization` header carries it.

When the gateway configuration names an upstream bearer (`rpc_bearer_file`), the inbound bearer SHALL differ from it. A caller holding the upstream credential could otherwise call the upstream node directly and bypass *Only an exact eth_\* allowlist is forwarded, with fixed parameter shapes*.

The listen port SHALL be nonzero. Each refusal in this requirement SHALL happen before the gateway opens its listener or contacts the upstream RPC. The process SHALL exit with a nonzero status and report the reason on stderr without either bearer's value.

#### Scenario: A loopback gateway without a bearer file does not start

- **GIVEN** a gateway configuration whose upstream RPC answers the identity check
- **WHEN** `chiral_rpc_gateway` is started with `--bind 127.0.0.1:8547` and no `--bearer-file`
- **THEN** it exits with a nonzero status and names `--bearer-file` on stderr
- **AND** it never listens and sends no request to the upstream RPC

#### Scenario: A weak or malformed bearer does not start the gateway

- **WHEN** the bearer file holds 31 characters, 32 non-ASCII characters, nothing, or two tokens separated by a space, or does not exist
- **THEN** the gateway exits with a nonzero status before listening, and the upstream RPC receives no request

#### Scenario: An exposed or linked bearer file does not start the gateway

- **WHEN** on Unix the bearer file is readable by group or other (mode `0644`), or is a symbolic link to an otherwise valid protected file
- **THEN** the gateway exits with a nonzero status before listening, and the upstream RPC receives no request

#### Scenario: The inbound bearer must differ from the upstream bearer

- **GIVEN** a gateway configuration whose `rpc_bearer_file` holds token `T`
- **WHEN** the gateway is started with an inbound bearer file that also holds `T`, with or without a trailing newline
- **THEN** it exits with a nonzero status stating that the inbound bearer must differ from the upstream bearer, without printing `T`
- **AND** the upstream RPC receives no request

#### Scenario: Exactly 32 visible ASCII characters is enough

- **GIVEN** an owner-only bearer file holding 32 visible ASCII characters and a newline, and a different upstream bearer
- **WHEN** the gateway is started on loopback
- **THEN** it passes the identity check and serves

### Requirement: Every RPC request is authenticated before it is read or forwarded

Every request to `POST /api/chain/rpc` SHALL carry an `Authorization` header with the `Bearer` scheme followed by exactly the inbound bearer, compared in constant time (only a difference in length may be observable). The gateway SHALL refuse a request without that header, with another value, with a prefix or extension of the bearer, with the bearer but no scheme, or with the upstream bearer. A refusal SHALL be HTTP 401 with a JSON-RPC 2.0 error object whose `id` is null. It SHALL be decided from the headers alone, before the body is read or parsed, and SHALL consume no admission or rate budget of *Transport and admission are bounded*. A refused request SHALL NOT reach the upstream RPC.

The same rule SHALL apply whether a request arrives directly on loopback or through TLS ingress. Forwarding headers such as `X-Forwarded-For` SHALL NOT relax it.

The gateway SHALL NOT forward the inbound bearer to the upstream RPC. An upstream request SHALL carry only the configured upstream bearer, if any.

Every response of `POST /api/chain/rpc`, including refusals, SHALL carry `Cache-Control: no-store`.

#### Scenario: An unauthenticated request is refused without reaching the chain

- **GIVEN** a running gateway
- **WHEN** a client posts a valid `eth_blockNumber` request without an `Authorization` header, or with only `X-Forwarded-For: 127.0.0.1`
- **THEN** the gateway answers HTTP 401 with a JSON-RPC error object and `Cache-Control: no-store`
- **AND** the upstream RPC receives no request

#### Scenario: A wrong, partial or upstream bearer is refused

- **WHEN** a request carries a different bearer of equal length, the first 16 characters of the bearer, the bearer followed by one more character, the bearer without the `Bearer` scheme, or the upstream bearer
- **THEN** each is refused with HTTP 401 and none reaches the upstream RPC

#### Scenario: Refusals do not exhaust the admission budget

- **WHEN** 200 requests without a bearer are refused in quick succession
- **THEN** a following request with the bearer is admitted and answered

#### Scenario: A valid bearer is forwarded and not cached, and stays local

- **GIVEN** a gateway configured with an upstream bearer `U`
- **WHEN** a client posts `{"jsonrpc":"2.0","id":7,"method":"eth_blockNumber","params":[]}` with the inbound bearer
- **THEN** the gateway answers HTTP 200 with the upstream result for id 7 and `Cache-Control: no-store`
- **AND** every request the upstream RPC received carried `Authorization: Bearer U` and never the inbound bearer

### Requirement: Browser-origin requests are refused

A request to `POST /api/chain/rpc` that carries an `Origin` header SHALL be refused exactly as an unauthenticated request is, with HTTP 401 and `Cache-Control: no-store`, even when it carries the valid bearer and whatever the header's value, including `null`. A web page SHALL NOT be able to use the gateway. Native clients send no `Origin` header, and the desktop never gives bearer files to its webview (book line 808).

#### Scenario: A valid bearer from a browser origin is refused

- **WHEN** a request carries the valid bearer and `Origin: https://example.org`, or the valid bearer and `Origin: null`
- **THEN** the gateway answers HTTP 401 and the request does not reach the upstream RPC

### Requirement: Only an exact eth_* allowlist is forwarded, with fixed parameter shapes

The gateway SHALL forward exactly these ten methods, matched by exact, case-sensitive name, with only these `params` (book lines 512 and 667):

| Method | Accepted `params` |
|---|---|
| `eth_chainId`, `eth_blockNumber`, `eth_gasPrice`, `eth_syncing` | `[]` |
| `eth_getBalance` | `[address, "latest"]` |
| `eth_getTransactionCount` | `[address, "latest"]` or `[address, "pending"]` |
| `eth_getBlockByNumber` | `[tag, false]`, where `tag` is `latest`, `safe`, `finalized` or a canonical hexadecimal height |
| `eth_getTransactionReceipt` | `[hash]`, a 32-byte `0x`-prefixed hexadecimal hash |
| `eth_sendRawTransaction` | `[raw]`, `0x`-prefixed even-length hexadecimal of 1 byte to 16 KiB |
| `eth_estimateGas` | `[{"from", "to", "value", "data", "gasPrice", "gas"}]` with exactly these members: two addresses, canonical hexadecimal quantities, `gas` from 21,000 to 1,000,000, and `data` of at most 4,096 bytes |

An address is 20 bytes of `0x`-prefixed hexadecimal. Every other method SHALL be refused, including `net_version` and every other `net_*` or `web3_*` method, `eth_call`, `eth_sendTransaction`, `eth_getTransactionByHash`, and any signing, mining, administration, debugging, account or transaction-pool method (book line 667: "not arbitrary node administration"). No prefix, namespace, wildcard or case variant of a listed name SHALL match it.

Each request SHALL be a JSON-RPC 2.0 object with only the members `jsonrpc` (`"2.0"`), `id`, `method` and `params`. `id` SHALL be an integer or a string of at most 128 bytes; a notification, which has no `id`, SHALL be refused. A request that fails any rule in this requirement SHALL be refused with HTTP 400 and a JSON-RPC error object, and nothing SHALL be forwarded. A batch with one refused member SHALL be refused whole before any member is forwarded.

An `eth_sendRawTransaction` that passes these rules SHALL be forwarded only if it decodes as a supported signed transaction for the configured chain ID. Otherwise its reply SHALL be a rejection, and nothing SHALL be broadcast. The gateway SHALL report an upstream failure only as one of three fixed categories, rejected, pending or unavailable, never with the upstream's message text.

#### Scenario: net_version is not exposed

- **WHEN** an authenticated client posts `{"jsonrpc":"2.0","id":8,"method":"net_version","params":[]}`
- **THEN** the gateway answers HTTP 400 with the error message `RPC method is not exposed`, and the upstream RPC receives no request

#### Scenario: Names outside the exact list are refused

- **WHEN** a request names `eth_chainid`, `ETH_CHAINID`, `eth_chainId ` with a trailing space, `eth_`, `eth_*`, `web3_clientVersion`, `eth_call`, `eth_sendTransaction` or `debug_traceTransaction`
- **THEN** each is refused with HTTP 400 and none is forwarded

#### Scenario: An unsupported shape of a listed method is refused

- **WHEN** a request is `eth_getBalance` with tag `pending`, `eth_getBlockByNumber` with full transactions (`true`) or the height `0x00`, `eth_estimateGas` with an extra member or `gas` above 1,000,000, or `eth_chainId` without an `id`
- **THEN** it is refused with HTTP 400 and not forwarded

#### Scenario: One refused member refuses the whole batch

- **WHEN** a batch holds a valid `eth_blockNumber` and a `net_version` request
- **THEN** the gateway answers HTTP 400 and forwards neither

### Requirement: Transport and admission are bounded

The request, response and error-size bounds of `inference-funding-settlement` *Chain RPC transport is bounded* SHALL apply to the gateway. A request body above that requirement's 64 KiB bound SHALL be refused with HTTP 413 before it is parsed. A batch SHALL NOT widen any bound: the combined reply to one HTTP request SHALL fit the 64 KiB response bound, or the 8 MiB bound when any member is a hash-only `eth_getBlockByNumber`. A combined reply that would exceed its bound SHALL be refused with HTTP 502. Members may already have been forwarded by then, so that refusal SHALL NOT be presented as proof that a submission in the request was not admitted.

A batch SHALL hold at least one member and at most a fixed bound of members; an empty or larger batch SHALL be refused with HTTP 400 before anything is forwarded.

Admission SHALL NOT queue. Reads and `eth_sendRawTransaction` submissions SHALL have separate concurrency and rate budgets. When either budget cannot admit every member of a request, the whole request SHALL be refused with HTTP 429 before any member is forwarded.

#### Scenario: An oversized body is refused

- **WHEN** a client posts a body of 65,537 bytes with the bearer
- **THEN** the gateway answers HTTP 413 and forwards nothing

#### Scenario: Eight block reads share one block bound

- **GIVEN** a hash-only block whose reply fits 8 MiB but not one eighth of it
- **WHEN** one request for it is posted, and then a batch of eight identical requests
- **THEN** the single request is answered, and the batch is refused with HTTP 502 because the combined reply exceeds 8 MiB

#### Scenario: A saturated submission budget refuses the whole batch

- **GIVEN** a submission budget that cannot admit another `eth_sendRawTransaction`
- **WHEN** a batch holds one read and one submission
- **THEN** the gateway answers HTTP 429 and forwards neither member

### Requirement: The gateway verifies network identity and keeps its own persistent journal

`--config` SHALL name a network profile with the complete network identity (chain ID, genesis hash and deployment ID), validated as every network profile is: the pinned Conflux eSpace testnet identity, a credential-free upstream `rpc_url` that is HTTPS or literal-loopback HTTP, and an upstream bearer only through `rpc_bearer_file` (book line 459: "never put a token in the URL"). Before it listens, the gateway SHALL confirm the upstream chain ID and genesis hash and SHALL refuse to start when either differs or cannot be read. Before each forwarded call it SHALL confirm the upstream chain ID again; a call whose check fails SHALL NOT be forwarded.

`--journal-dir` SHALL be required. The gateway SHALL keep its network-scoped journal under `<journal-dir>/inference/<network-hash>/wallet-journal/`, create it with owner-only permissions without following symbolic links, and reuse it across restarts (book line 644: "persistent journal"). The reference deployment gives the gateway its own journal directory, separate from provider and relay data. The gateway SHALL hold no wallet signing key and SHALL sign nothing; it relays bytes that clients signed.

#### Scenario: An upstream on another chain prevents start

- **WHEN** the upstream RPC reports a chain ID other than 71, or a genesis hash other than the pinned one
- **THEN** the gateway exits with a nonzero status before listening

#### Scenario: The journal survives restart

- **GIVEN** a gateway that has started once with `--journal-dir /srv/chiral/data/rpc`
- **WHEN** it is stopped and started again with the same directory
- **THEN** it reuses the same journal file, and the journal directory and file grant no group or other access

### Requirement: Liveness and readiness are unauthenticated local probes

`GET /healthz` SHALL answer HTTP 200 with `{"status":"live"}` while the gateway serves. `GET /readyz` SHALL answer HTTP 200 with `{"ready":true}` when the identity check of *The gateway verifies network identity and keeps its own persistent journal* succeeds, and HTTP 503 with `{"ready":false}` otherwise. Neither probe SHALL require the bearer, and neither SHALL reveal a credential or upstream response content.

The reference TLS ingress SHALL forward only `/api/chain/rpc` to the gateway and answer every other path on the RPC domain with 404, so the probes stay local (book line 656).

#### Scenario: The probes answer without a bearer

- **GIVEN** a running gateway whose upstream answers the identity check
- **WHEN** `/healthz` and `/readyz` are requested without a bearer
- **THEN** they answer HTTP 200 with `{"status":"live"}` and `{"ready":true}`

#### Scenario: Readiness follows the upstream

- **GIVEN** a running gateway
- **WHEN** its upstream RPC stops answering
- **THEN** `/readyz` answers HTTP 503 with `{"ready":false}` while `/healthz` still answers HTTP 200

#### Scenario: Ingress exposes only the RPC route

- **WHEN** the generated public Caddyfile is read
- **THEN** the RPC domain forwards only `/api/chain/rpc` to `127.0.0.1:8547` and answers every other path with 404

### Requirement: Provisioning and preflight keep the gateway authenticated

Provisioning SHALL require the operator's inbound RPC bearer file, in calibration mode as well, SHALL refuse a bearer of fewer than 32 characters, and SHALL refuse an inbound bearer equal to the upstream bearer. It SHALL write the inbound bearer to its own owner-only secret file (`secrets/rpc.token`) and the upstream bearer to a separate one (`secrets/upstream-rpc.token`). It SHALL configure the provider and the relay to call `http://127.0.0.1:8547/api/chain/rpc` with that inbound bearer file. Every gateway command that provisioning, the Compose file and the operator guide give SHALL pass `--bearer-file` with a path, never the bearer itself (book line 474).

The provider preflight run against a running deployment SHALL require the gateway's `/healthz` and `/readyz` to answer HTTP 200. It SHALL also post a JSON-RPC request to `http://127.0.0.1:8547/api/chain/rpc` without a bearer, through a client that disables proxies and redirects, and SHALL fail unless the answer is HTTP 401.

#### Scenario: Preflight fails when the gateway answers without a bearer

- **WHEN** the gateway answers an unauthenticated `POST /api/chain/rpc` with HTTP 200, 400, 403, 404, 429, 500 or 502
- **THEN** preflight fails and names the status
- **AND** when it answers HTTP 401, this check passes

#### Scenario: Preflight does not follow a redirect

- **WHEN** the unauthenticated probe is answered with an HTTP redirect
- **THEN** preflight fails without contacting the redirect target

#### Scenario: Identical inbound and upstream bearers are refused at provisioning

- **WHEN** provisioning input names an `rpc_token_file` and an `upstream_rpc_token_file` holding the same token
- **THEN** provisioning refuses before creating the deployment directory
