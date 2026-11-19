## ADDED Requirements

### Requirement: Remote provider and catalog origins are validated before any request

The native runtime SHALL validate every remote provider origin and every catalog origin before it makes any request to it, including before any DNS resolution or connection, because only a validated origin is resolved and connected to (book lines 264 and 268). This covers the signed offer `endpoint` that a consumer inspects, quotes, funds, controls or sends inference to; the consumer profile's `catalog_url`, used for discovery, reputation and feedback; and the provider's own `public_endpoint`, which it advertises.

Origin validation SHALL be the advertised-origin rule of `inference-commercial-terms` (*Offer freshness and advertised origin*), applied unchanged to catalog origins as well: HTTPS on port 443; no username, password, query, fragment or path other than `/`; and no loopback address, `localhost` or `.localhost` name, single-label remote hostname, or literal address that fails *Outbound HTTP uses the exact public address predicate* (book line 264). This capability does not restate the spelling rules that `inference-commercial-terms` owns; it adds that the rule governs every outbound request to a provider or catalog, not only offer admission.

A provider SHALL NOT advertise a `public_endpoint` that fails validation, and a provider configuration naming one SHALL be refused (book line 266). A consumer request whose provider or catalog origin fails validation SHALL fail without resolving or connecting to that origin (book line 268).

#### Scenario: A non-public catalog origin is refused before any connection

- **GIVEN** a consumer profile without development mode and without `dht`, whose `catalog_url` is `https://relay.example:8443`, `https://10.0.0.5`, `https://relay` or `https://relay.localhost`
- **WHEN** catalog discovery, the reputation read of provider inspection or feedback submission runs
- **THEN** it fails with the origin validation error
- **AND** no DNS query and no connection is made for that origin

#### Scenario: A provider origin that fails validation is not contacted

- **GIVEN** a consumer without development mode and a validly signed offer whose `endpoint` is `https://provider.example:8443`
- **WHEN** the consumer is asked to inspect, quote or fund that offer
- **THEN** it refuses with the origin validation error
- **AND** no DNS query and no connection is made for the provider origin

#### Scenario: A provider does not advertise a non-conforming origin

- **WHEN** a provider configuration without development mode has `public_endpoint` `https://provider.example:8443`, `https://provider.example/v1`, `http://provider.example` or `https://203.0.113.7`
- **THEN** configuration validation refuses it
- **AND** no offer advertises that origin

### Requirement: The development exception covers literal loopback and exact localhost only

Explicit development mode, which SHALL default to false (book line 463), SHALL permit HTTP and ports other than 443 only for an origin whose host is a literal loopback address or exactly `localhost` (book line 264). It SHALL NOT authorize a private LAN address, a link-local or metadata address, a `localhost` subdomain or alias, any other DNS name, or cleartext HTTP to a remote host (book line 264). Every other origin SHALL be validated in development mode exactly as in *Remote provider and catalog origins are validated before any request*.

When the exception admits a `localhost` origin, every address that the origin resolves to SHALL be a loopback address. A resolved public, private or other non-loopback address SHALL refuse the origin, because a request to it would be the remote traffic that the exception does not authorize, and the client rejects any disallowed result (book lines 264 and 268).

A development provider's public listener SHALL bind a loopback address (book line 264). A development configuration naming another public bind address SHALL be refused before any listener binds.

#### Scenario: Development admits loopback origins only

- **GIVEN** development mode
- **WHEN** the origin is `http://127.0.0.1:9419`, `http://[::1]:9419` or `http://localhost:8080`
- **THEN** it passes origin validation
- **AND** `http://10.0.0.1:9419`, `https://192.168.10.1`, `http://169.254.169.254`, `https://provider.localhost`, `http://localhost.:8080` and `http://provider.example:8080` are refused

#### Scenario: Without development, loopback origins are refused

- **WHEN** development mode is off and the origin is `http://127.0.0.1:9419`, `https://127.0.0.1` or `https://localhost`
- **THEN** origin validation refuses it

#### Scenario: A localhost origin that resolves to a remote address is refused

- **GIVEN** development mode and the origin `http://localhost:9419`
- **WHEN** resolution of `localhost` returns `127.0.0.1` together with a public address such as `93.184.216.34`
- **THEN** no client is built and no connection is made to either address
- **AND** when resolution returns only `127.0.0.1` and `::1`, the client is built and pinned to those addresses

#### Scenario: A development provider listens on loopback only

- **WHEN** a development provider configuration has `public_bind` `0.0.0.0:9419` or `192.168.1.5:9419`
- **THEN** configuration validation refuses it before any listener binds
- **AND** `public_bind` `127.0.0.1:9419` is accepted

### Requirement: Advertised endpoints and catalog URLs are origins, and the SDK base URL is derived after validation

A provider SHALL advertise an origin such as `https://provider.example`, never a URL with a path such as `https://provider.example/v1` or an explicit port such as `https://provider.example:8443` (book line 266). A consumer profile's `catalog_url` SHALL be the relay's origin, not its `/api/offers` path (book lines 266 and 462); a `catalog_url` with a path, query, fragment or credentials SHALL be refused before any catalog request is made. Native requests SHALL add their paths (`/v1/...`, `/api/offers`, `/api/ratings/...`, `/healthz`, `/readyz`) to the validated origin.

The `base_url` that credential export gives an OpenAI-compatible SDK SHALL be the account's provider origin with `/v1` appended, and SHALL be produced only after that origin passes the origin validation of *Remote provider and catalog origins are validated before any request* under the profile's current development mode (book line 266). An SDK uses its own HTTP stack, not the client of *Outbound requests to a validated origin use a per-origin DNS-pinned client without proxies or redirects*, so the export is the last point at which the native runtime can apply the origin rule. When the origin fails validation, the export SHALL return the validation error and SHALL NOT return the API secret. The saved account, its signed terms and its credentials SHALL remain stored unchanged, so that recovery stays possible (`CLAUDE.md`: historical funding recovery must remain reachable).

#### Scenario: The SDK base URL is the validated origin plus /v1

- **GIVEN** a saved account whose signed offer endpoint is `https://provider.example`
- **WHEN** its credential is exported
- **THEN** `base_url` is `https://provider.example/v1`

#### Scenario: A saved origin that no longer validates is not exported

- **GIVEN** a profile without development mode and a saved account whose signed offer endpoint is `https://intranet./` (accepted by an earlier validator) or `https://localhost:8443` (saved in development mode)
- **WHEN** its credential is exported
- **THEN** the export fails with the origin validation error and returns no `api_key`
- **AND** the saved account and its credentials are unchanged

#### Scenario: A catalog path is not a catalog origin

- **WHEN** a consumer profile names `catalog_url` `https://relay.example/api/offers` or `https://relay.example/?page=1`
- **THEN** that `catalog_url` is refused
- **AND** no catalog request is made to `relay.example`

### Requirement: Outbound requests to a validated origin use a per-origin DNS-pinned client without proxies or redirects

Before connecting to a validated remote provider or catalog origin, the native runtime SHALL build an HTTP client for that origin as follows (book line 268):

- it SHALL resolve the origin's host once, with a deadline of 5 seconds; a timeout or resolution error SHALL refuse the origin;
- it SHALL inspect at most 33 answers, and SHALL refuse the origin when there is no answer, when there are more than 32 answers, or when any answer fails *Outbound HTTP uses the exact public address predicate*, apart from the loopback answers that *The development exception covers literal loopback and exact localhost only* permits;
- it SHALL pin the accepted addresses into the client, so that the client connects only to them;
- it SHALL ignore environment proxy settings and SHALL NOT follow any redirect;
- it SHALL set a connect timeout of 10 seconds.

A client SHALL be built for one validated origin and SHALL NOT be used for requests to another host (book line 268). Building a later client SHALL resolve and validate again. DNS pinning SHALL NOT be described as certificate pinning or as a promise that the origin keeps one address. HTTPS SHALL keep its normal certificate and hostname verification against the origin's hostname (book line 268).

A redirect SHALL be treated as the answer of the origin that sent it. Its target SHALL NOT be contacted, and no bearer, signed request, credential or request body SHALL be sent to it; a provider-controlled redirect does not authorize sending secrets to its target (book line 268).

#### Scenario: An empty, oversized or partly disallowed answer set is refused

- **GIVEN** the validated origin `https://provider.example`
- **WHEN** its resolution returns no address, 33 public addresses, or 31 public addresses and the private address `10.0.0.5`
- **THEN** no client is built and no connection is made
- **AND** when it returns 32 public addresses, the client is built and connects only to those addresses

#### Scenario: Slow resolution is refused

- **WHEN** resolution of a validated origin has not answered after 5 seconds
- **THEN** the origin is refused and no connection is made

#### Scenario: A redirect is not followed

- **GIVEN** a provider that answers a request with `302` and a `Location` naming another listener
- **WHEN** the consumer sends an inference, control, quote or inspection request to that provider
- **THEN** the consumer does not follow the redirect: the request fails, or inspection reports the provider not live or not ready
- **AND** the other listener receives no connection, and no account bearer or signed request reaches it

#### Scenario: Environment proxies are ignored

- **GIVEN** `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` naming a listening proxy
- **WHEN** the consumer contacts a provider or catalog origin
- **THEN** the proxy receives no connection and the request goes directly to a pinned address

### Requirement: Outbound HTTP uses the exact public address predicate

The public address predicate SHALL be `offer_registry::public_ip` exactly as implemented, and SHALL NOT be described as a classification of every present or future IANA allocation (book line 270). It SHALL apply to a literal address in an origin and to every resolved answer of *Outbound requests to a validated origin use a per-origin DNS-pinned client without proxies or redirects*.

An IPv4 address SHALL be public unless it is private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`), loopback (`127.0.0.0/8`), link-local (`169.254.0.0/16`), multicast (`224.0.0.0/4`), broadcast or reserved (`240.0.0.0/4`), unspecified or in `0.0.0.0/8`, carrier-grade NAT (`100.64.0.0/10`), in `192.0.0.0/24`, documentation (`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`) or benchmarking (`198.18.0.0/15`) (book line 270). An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) SHALL be judged by the IPv4 predicate (book line 270). Any other IPv6 address SHALL be public exactly when it is within global unicast `2000::/3` and outside `2001::/23`, `2001:db8::/32`, `2002::/16` and `3fff::/20` (book line 270).

This predicate governs outbound HTTP requests. It SHALL NOT be presented as a statement that every libp2p address follows the HTTP-origin policy (book line 270); filtering of libp2p addresses is target-design work of the pending change `specify-p2p-provider-transport` (book line 288).

#### Scenario: IPv4 range boundaries

- **WHEN** the predicate judges `100.63.255.255`, `100.128.0.0`, `198.17.255.255`, `198.20.0.0`, `192.0.1.1` and `1.1.1.1`
- **THEN** each is public
- **AND** `100.64.0.0`, `100.127.255.255`, `198.18.0.0`, `198.19.255.255`, `192.0.0.1`, `172.31.255.254`, `169.254.169.254`, `0.1.2.3` and `240.0.0.1` are not

#### Scenario: IPv4-mapped IPv6 follows the IPv4 predicate

- **WHEN** the predicate judges `::ffff:10.0.0.1`, `::ffff:127.0.0.1` and `::ffff:169.254.169.254`
- **THEN** none is public
- **AND** `::ffff:1.1.1.1` is public

#### Scenario: IPv6 global unicast with documented exclusions

- **WHEN** the predicate judges `2606:4700:4700::1111`, `2001:200::1` and `3fff:1000::1`
- **THEN** each is public
- **AND** `::`, `::1`, `fd00::1`, `fe80::1`, `ff02::1`, `2001:1ff:ffff::1`, `2001:db8::1`, `2002:7f00:1::` and `3fff:fff::1` are not

#### Scenario: Literal and resolved addresses use one predicate

- **WHEN** an origin is the literal `https://[::ffff:127.0.0.1]`, or a hostname whose resolution includes `::ffff:127.0.0.1`
- **THEN** both are refused before any connection

### Requirement: Provider administration is a separate loopback-only listener with its own protected bearer

A provider SHALL serve administration on its own listener: the administration bind address SHALL be a loopback address with a nonzero port, and the public listener's port SHALL be nonzero and different from it (book line 274). A configuration that violates this SHALL be refused before any listener binds. `/admin/*` routes SHALL be mounted only on the administration listener; the public router SHALL NOT serve them (book line 274).

The administration token SHALL be read from its own protected file (book lines 474 and 658). Every administration request SHALL carry `Authorization: Bearer` with that token and SHALL NOT carry an `Origin` header (`CLAUDE.md`: operator routes "require their protected bearer and reject browser `Origin` requests"); any other request SHALL be refused as unauthorized before any administrative action runs. An account API bearer, a wallet owner-control challenge or signature, and every other credential SHALL NOT be accepted as the administration token (book line 274).

Administration requests are local-only (book line 658). The native administration client SHALL send the administration token only to a loopback address on the local host, SHALL NOT send it through an environment proxy, and SHALL refuse any other endpoint without connecting to it.

#### Scenario: Administration binds a nonzero loopback port distinct from the public port

- **WHEN** a provider configuration has `admin_bind` `0.0.0.0:9421`, `192.168.1.5:9421` or `127.0.0.1:0`, or has `public_bind` `0.0.0.0:9421` with `admin_bind` `127.0.0.1:9421`
- **THEN** configuration validation refuses it and no listener binds
- **AND** `admin_bind` `127.0.0.1:9421` with `public_bind` `127.0.0.1:9419` is accepted

#### Scenario: The public listener serves no administration route

- **WHEN** `GET /admin/status`, `GET /admin/offers` or `POST /admin/drain` is sent to the public listener with the valid administration bearer
- **THEN** the public listener answers that the endpoint does not exist
- **AND** no administrative action runs and the provider keeps accepting

#### Scenario: Only the administration token authorizes administration

- **WHEN** an administration request carries no `Authorization` header, a wrong token, an account API bearer issued by this provider, or the valid administration bearer together with `Origin: http://localhost`
- **THEN** it is refused as unauthorized and the provider's state is unchanged

#### Scenario: The native administration client contacts only local loopback

- **WHEN** the native administration command is given the endpoint `http://10.0.0.5:9421` or `https://provider.example`
- **THEN** it refuses, makes no connection and sends the administration token nowhere

### Requirement: The private engine origin is operator configuration on a private address

Each model's `engine_url` SHALL be private operator configuration, not the advertised endpoint (book line 275). It SHALL NOT appear in a signed offer, in a quote, or in any response or error of the public listener; the provider SHALL advertise only `public_endpoint`.

`engine_url` SHALL be an `http` or `https` origin without username, password, query, fragment or path other than `/`, whose host is a literal loopback or private IPv4 address (`127.0.0.0/8`, `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`), a literal IPv6 loopback or unique-local address (`::1`, `fc00::/7`), or exactly `localhost` (book line 275). Any other host SHALL be refused, including a DNS name other than `localhost` such as an internal hostname, a `localhost` alias, a public, link-local, carrier-grade NAT or metadata address, and an IPv4-mapped IPv6 address (book line 275). A provider configuration naming such a host SHALL fail validation, and the provider SHALL send no engine request to it. A literal private address is for a protected trusted-network deployment, and the engine SHALL never be reachable as a public paid-service bypass (book line 656).

A managed engine SHALL additionally satisfy `managed-engine-runtime` *The engine listens only on its literal-loopback endpoint, and an occupied port is refused*. Engine credentials SHALL be distinct from account, administration and wallet secrets (book lines 275 and 474), as `managed-engine-runtime` *The child receives a minimal environment and its API key only as a protected file path* requires for managed and external engines alike.

Every client that the provider uses for the private engine SHALL ignore environment proxy settings and SHALL NOT follow redirects (book line 275). A redirect from the engine SHALL be treated as an engine failure; its target SHALL NOT be contacted and SHALL NOT receive the engine key or any request content.

#### Scenario: Private literal addresses and exact localhost are accepted

- **WHEN** an external engine's `engine_url` is `http://127.0.0.1:8000`, `http://10.0.0.5:8080`, `https://[fd00::1]:8443`, `http://[::1]:8000` or `http://localhost:8000`
- **THEN** the engine origin passes validation

#### Scenario: DNS names, public addresses and non-origins are refused

- **WHEN** an `engine_url` is `http://engine.internal:8080`, `http://localhost.:8000`, `http://8.8.8.8:8000`, `http://169.254.169.254`, `http://100.64.0.1:8000`, `http://[fe80::1]:8000`, `http://[::ffff:10.0.0.1]:8000`, `http://127.0.0.1:8000/v1`, `http://user@127.0.0.1:8000` or `ftp://127.0.0.1`
- **THEN** provider configuration validation refuses it
- **AND** no engine request is sent to that origin, even on a path that builds the engine client without full configuration validation

#### Scenario: An engine redirect is not followed

- **GIVEN** an engine that answers a request with `302` and a `Location` naming another loopback listener
- **WHEN** the provider sends it a request
- **THEN** the provider treats the answer as an engine failure
- **AND** the other listener receives no connection, the engine key and no request content

#### Scenario: The engine origin is never advertised

- **GIVEN** a provider whose `engine_url` is `http://10.0.0.5:8080` and whose `public_endpoint` is `https://provider.example`
- **WHEN** it publishes offers and serves `/v1/models`, `/healthz` and `/readyz`, and an engine request fails
- **THEN** no signed offer, public response or public error contains `10.0.0.5`
- **AND** every offer's `endpoint` is the configured `public_endpoint` `https://provider.example`

### Requirement: TLS ingress fronts only the public listener, and public HTTPS reachability remains required

A TLS reverse proxy MAY front the provider's public listener (book line 276). It SHALL NOT expose the administration listener, the private engine, any credential or secret file, or the filesystem (book line 276). In the reference topology, the provider's public listener, its administration listener and its engine listen on loopback, ingress forwards the provider domain only to the public listener, and administration and the engine stay private (book line 656). Host exposure SHALL be firewalled independently of the ingress allowlist (book line 656). The reference preflight run against a running deployment SHALL fail when a private or retired route is reachable through provider ingress, or when a private port (the engine 8000, the public listener 9419, administration 9421, the chain RPC gateway 8547 or the relay 8081) is bound to a non-loopback address (`docs/provider-qualification.md` line 225, the provider qualification guide that book line 607 points to).

In the current runtime, a non-development provider SHALL be reachable at its advertised public HTTPS origin, and libp2p relay support SHALL NOT be treated as NAT traversal for the inference HTTP endpoint (book line 276): a consumer SHALL send quote, funding, control and inference requests only to the offer's validated origin, never through a libp2p relay circuit. The pending target-design change `specify-p2p-provider-transport` replaces this rule when it is implemented (book line 276).

#### Scenario: The reference ingress forwards only to the public listener

- **GIVEN** provisioning input for a deployment that is not a calibration deployment
- **WHEN** the reference Caddyfile is generated
- **THEN** the provider domain forwards only public-router paths to `127.0.0.1:9419`
- **AND** no site serves files or forwards to `127.0.0.1:9421` or `127.0.0.1:8000`

#### Scenario: Preflight refuses an exposed administration or engine route

- **GIVEN** a running deployment whose public endpoint answers `GET /admin/status`, `GET /admin/runtime` or `GET /tokenize` with HTTP 200
- **WHEN** `scripts/provider-preflight.py --running` runs
- **THEN** it fails and names the exposed route

#### Scenario: Preflight refuses a private listener on a non-loopback address

- **GIVEN** a host whose listener table shows the administration port 9421 or the engine port 8000 bound to `0.0.0.0`
- **WHEN** `scripts/provider-preflight.py --running` checks the listeners
- **THEN** it fails because a private service is bound to a non-loopback address
