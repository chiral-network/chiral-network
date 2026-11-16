## ADDED Requirements

### Requirement: The provider public router is also served over the libp2p inference stream

When libp2p serving is enabled, the provider SHALL accept inbound streams of protocol `/chiral/inference/http/1` on its persisted libp2p identity, over TCP connections secured by Noise and multiplexed by yamux, including connections upgraded by hole punching and relayed circuits. Each stream SHALL carry HTTP/1.1 requests and responses, including `text/event-stream` (SSE) responses, with the same connection semantics as a TCP connection to the public HTTP listener. The stream handler SHALL dispatch every request into the same public router instance that serves the public HTTP listener: the `/v1/*` inference, request-status and contract/control routes, `/healthz`, `/readyz`, and every other route that router mounts, with the same authentication, owner-proof binding, client-version handling, admission, rate limits, ledger, accounting, body limits, body deadlines and generation deadlines. The set of routes reachable over the stream SHALL equal the set reachable on the public HTTP listener.

The provider SHALL construct its public router once per process and SHALL serve both listeners from that one instance, or from clones of it that share its state. It SHALL NOT construct a separate router for the stream listener, because each construction creates separate gateway state. The per-account rate window, each model's slots, the qualified arrival envelope of each model whose offer has a serving profile (a model without one, such as a free share, has no arrival envelope), the body-reader and control slots, and the cancellation registry SHALL each be one state shared by both listeners. A request-status or cancellation request SHALL reach the same live operation whichever listener admitted that operation.

The provider SHALL NOT derive routing, authorization or account identity from the `Host` header or from the consumer's peer ID; account authority SHALL remain the account bearer or the one-use owner proof. Where the public router applies optional source controls, the source identity of a stream SHALL be its authenticated remote peer ID, never the address of a relay that carried it; a peer ID SHALL NOT be represented as Sybil resistance. A provider without libp2p serving enabled SHALL NOT support the protocol. Native consumers SHALL open a fresh stream for each request.

#### Scenario: One ledger behind two listeners

- **GIVEN** a provider serving both its HTTPS origin and `/chiral/inference/http/1`
- **WHEN** an authorized chat completion with `Idempotency-Key` `k1` is admitted and settled over the stream
- **THEN** the same account ledger records its reservation, usage and settlement as an HTTPS request would
- **AND** a later request with key `k1` for that account over HTTPS returns HTTP 409 `duplicate_request` with the original request identifier and status

#### Scenario: Cancel crosses listeners

- **GIVEN** a streaming chat completion admitted over `/chiral/inference/http/1` that is still generating
- **WHEN** the account's bearer sends `POST /v1/requests/:id/cancel` for that request over the public HTTP listener
- **THEN** the provider signals the live operation and returns HTTP 202 `cancelling`
- **AND** the request's known usage settles under the existing accounting contract
- **AND** `GET /v1/requests/:id` over either listener reports the same request status

#### Scenario: Shared admission state

- **WHEN** requests for one account and model arrive concurrently over both listeners
- **THEN** they are admitted against one per-account rate window, one set of model slots, one qualified arrival envelope when the model's offer has a serving profile, and one pool of body-reader slots
- **AND** a request that would exceed any of these is refused on whichever listener it arrives, exactly as if every request had arrived on one listener

#### Scenario: Identical limits and deadlines

- **WHEN** a control request body larger than 96 KiB, or a body that does not complete within the five-second body deadline, is sent over the stream
- **THEN** the provider returns the same rejection it returns on the public HTTP listener
- **AND** no control slot, challenge or quote state differs from the HTTP case

#### Scenario: Streaming response over a stream

- **WHEN** an authorized streaming chat completion is sent over the stream
- **THEN** the provider returns the same SSE event sequence, terminal event and authoritative usage as on the public HTTP listener

#### Scenario: Peer identity is not account authority

- **WHEN** a request without a bearer or owner proof arrives over the stream from a peer that previously funded an account
- **THEN** the provider rejects it exactly as an unauthenticated HTTP request
- **AND** neither the `Host` header nor the remote peer ID selects or authorizes an account

#### Scenario: Serving disabled

- **GIVEN** a provider whose configuration does not enable libp2p serving
- **WHEN** a peer opens `/chiral/inference/http/1`
- **THEN** protocol negotiation fails and no HTTP request is processed

### Requirement: Administration and engine surfaces are never reachable over the stream

The stream handler SHALL dispatch only into the public router. Administration routes, the loopback administration listener, the private engine and its tokenization, detokenization, template, metrics, model-loading and distributed-worker endpoints, and any route that the public router does not mount SHALL NOT be reachable over `/chiral/inference/http/1`. The handler SHALL NOT act as a generic proxy, tunnel or `CONNECT` target and SHALL NOT open an outbound connection on behalf of a stream. An administrator bearer presented over the stream SHALL confer no authority.

#### Scenario: Admin route over the stream

- **WHEN** a peer sends `GET /admin/status` or `POST /admin/drain` with the correct administrator bearer over the stream
- **THEN** the provider returns the same not-found response as the public HTTP listener
- **AND** no administration action or status disclosure occurs

#### Scenario: Engine and metrics paths

- **WHEN** a peer requests an engine tokenization, template, metrics or model-loading path over the stream
- **THEN** the request is not forwarded to the engine and receives the public router's not-found response

#### Scenario: Proxy attempt

- **WHEN** a peer sends a `CONNECT` request or an absolute-form request naming another host over the stream
- **THEN** the provider rejects it without opening any outbound connection

### Requirement: Inbound serving streams are bounded resources

The provider SHALL bound concurrent inbound `/chiral/inference/http/1` streams in total and per remote peer by explicit configured limits, and SHALL refuse an excess stream before reading a request from it. yamux's per-connection stream allowance SHALL NOT substitute for these limits. Because no TLS ingress fronts this listener, the stream server itself SHALL also enforce, from explicit configuration and before the router's body limits and deadlines: a request-head deadline, a maximum request-head size, an idle timeout for a stream with no request in progress, and a maximum number of requests per stream.

A stream that has not delivered a complete request head within the head deadline SHALL be reset without dispatch. A request head that exceeds the maximum size SHALL be rejected with HTTP 431 or a stream reset, without dispatch. A stream that stays idle beyond the idle timeout SHALL be closed. After a stream has carried its maximum number of requests, the server SHALL close it without reading another request. None of these rejections SHALL consume an admission, control or body-reader slot. After the request head, the router's existing body limits and body deadlines SHALL apply unchanged. Stream limits SHALL NOT raise model concurrency, account or credential concurrency, control slots or any other admission limit.

A stream that the remote peer or a relay resets or closes SHALL be handled exactly as a closed connection on the public HTTP listener. It SHALL NOT be represented as cancellation without charge; known usage SHALL settle and missing usage SHALL follow the existing unknown-usage policy.

#### Scenario: Per-peer stream limit

- **GIVEN** a peer already holds the configured maximum number of concurrent serving streams
- **WHEN** it opens another stream
- **THEN** the provider refuses that stream before reading a request
- **AND** other peers' streams and existing requests are unaffected

#### Scenario: Slow request head

- **WHEN** a stream does not deliver a complete request head within the configured head deadline
- **THEN** the provider resets it without dispatching a request or consuming an admission slot

#### Scenario: Oversized request head

- **WHEN** a stream sends a request head larger than the configured maximum request-head size
- **THEN** the provider rejects it with HTTP 431 or a stream reset
- **AND** no route handler runs and no admission, control or body-reader slot is consumed

#### Scenario: Idle stream

- **GIVEN** a stream whose previous response has completed
- **WHEN** no new request head begins within the configured idle timeout
- **THEN** the provider closes the stream

#### Scenario: Requests per stream exhausted

- **GIVEN** a stream that has carried the configured maximum number of requests
- **WHEN** the peer writes another request on it
- **THEN** the provider closes the stream without reading or dispatching that request

#### Scenario: Stream cut during generation

- **GIVEN** an admitted streaming request whose stream is reset by a relay after tokens were generated
- **WHEN** the provider observes the reset
- **THEN** the request is handled as an HTTP client disconnect and its known usage settles under the existing accounting contract

### Requirement: Native consumers authenticate the provider by its signed peer ID

Before writing any request bytes on a `/chiral/inference/http/1` stream, including a bearer credential, an owner proof, a challenge request or a request body, the native consumer SHALL verify that the connection's Noise-authenticated remote peer ID equals the selected offer's signed `peer_id`. It SHALL refuse a connection or stream whose authenticated peer differs, including a relay, a hole-punch coordinator or any peer reached at an address learned for that peer ID, and SHALL NOT fall back to an address, relay or transport that cannot prove that peer ID. On a relayed circuit the authenticated peer SHALL be the provider at the circuit's far end, never the relay.

DNS names, WebPKI certificates and HTTP-origin DNS pinning SHALL play no role in authenticating this path. The redirect rule SHALL still apply: the stream client SHALL NOT follow a redirect, and SHALL NOT dial or connect to any URL, host or multiaddress named in a provider response, including a `Location` header or a response-body field. A 3xx response SHALL be surfaced to its caller as a provider error without its `Location` header. The stream client SHALL apply the HTTPS client's ten-second connect deadline to each connection attempt, and SHALL apply the same response deadlines, stream idle timeout and 8 MiB cumulative response-size bound that the HTTPS client applies to the same operation. A response that exceeds that bound or a deadline after request bytes were written SHALL be aborted and SHALL be handled as an ambiguous outcome under *Transport failures are recovered through request status, never a blind paid retry*.

A peer-ID refusal SHALL be reported as a provider-authentication failure, not as provider unavailability, an empty result or a successful operation. An account SHALL remain bound to the peer ID of its retained signed offer; a provider that presents a different peer ID SHALL be refused for that account, with no automatic transfer of the account or its balance.

#### Scenario: Mismatched peer before any bytes

- **WHEN** a connection reached through an address learned for the offer's peer ID authenticates a different peer ID
- **THEN** the consumer closes it without writing a credential, owner proof or body
- **AND** reports a provider-authentication failure

#### Scenario: Relay impersonation

- **WHEN** a relay attempts to answer the stream itself instead of forwarding a circuit to the provider
- **THEN** the authenticated peer ID differs from the offer's `peer_id` and the consumer refuses the stream

#### Scenario: Redirect over the stream

- **WHEN** a response received over the stream has a 3xx status and a `Location` header
- **THEN** the consumer does not follow it and sends no credential or request to the redirect target
- **AND** its caller receives a provider error that does not contain the `Location` value

#### Scenario: Response names another address

- **WHEN** a provider response names a multiaddress, host or URL in a header or body field
- **THEN** the consumer does not dial or connect to it

#### Scenario: Oversized response

- **WHEN** a response received over the stream exceeds 8 MiB in total
- **THEN** the consumer aborts it
- **AND** treats the request as ambiguous and reconciles it through request status instead of re-sending it

#### Scenario: Provider identity replaced

- **GIVEN** an account whose retained offer names peer ID `P1`
- **WHEN** the provider later presents only peer ID `P2`
- **THEN** the consumer refuses to send that account's requests to `P2`
- **AND** the account, its balance and its pending recovery remain bound to `P1`

### Requirement: Transport selection follows the signed offer and never changes money or terms

For a newly selected offer, the native consumer SHALL use `libp2p` only when the offer declares it and `https` only when the offer declares it together with an endpoint. A v1 offer SHALL NOT be newly selected (`inference-commercial-terms`, *Offer freshness and advertised origin*). When both transports are available, the native consumer SHALL prefer `https` for new operations and MAY use `libp2p` for a later operation when the HTTPS origin is unreachable. An operation already dispatched SHALL NOT be re-sent on another transport.

Funding SHALL continue to pay the signed `provider_wallet` on chain. Quotes, funding-intent acknowledgements, receipts and feedback SHALL be the same signed objects with the same domains on every transport. Neither the transport nor the path SHALL change payee, price, fee, concurrency or any agreement term. An account SHALL use its retained offer's endpoint for `https` and its retained offer's `peer_id` for `libp2p`; a retained v1 offer SHALL be treated as declaring exactly `https`. It SHALL use `libp2p` only when the retained offer, or a currently valid signed v2 offer from the same provider wallet and deployment for the account's model naming the same `peer_id`, declares it. An account whose retained offer is a v1 offer, opened before the provider upgraded, MAY therefore be served over `libp2p` by its signed `peer_id`, and SHALL keep its retained v1 terms when it is. A provider-specific balance SHALL NOT transfer to another peer ID, deployment or origin.

A native consumer whose profile has no libp2p node SHALL present a libp2p-only offer as unreachable from that profile. No consumer surface SHALL invent an HTTPS origin or SDK base URL for a libp2p-only offer or account.

#### Scenario: libp2p-only offer

- **WHEN** the user selects a verified offer whose `transports` is `["libp2p"]` and whose `endpoint` is `null`
- **THEN** inspection, quote, funding control and inference all use `/chiral/inference/http/1` to the offer's peer ID
- **AND** the funding transaction pays the offer's `provider_wallet`

#### Scenario: Both transports declared

- **WHEN** an offer declares both `https` and `libp2p`
- **THEN** new operations use the validated HTTPS origin
- **AND** a request whose HTTPS dispatch became ambiguous is not re-sent over libp2p

#### Scenario: Saved v1 account after the provider drops its HTTPS origin

- **GIVEN** an account opened before the provider upgraded, whose retained offer is a v1 offer with peer ID `P1`, which therefore declares exactly `https`
- **WHEN** the same provider wallet and deployment now publish only valid v2 offers for the account's model declaring only `libp2p` for `P1`
- **THEN** the account's requests use `/chiral/inference/http/1` to `P1` under the account's retained v1 terms
- **AND** the account is never served by another peer ID or an origin its offers did not declare

#### Scenario: Profile without a libp2p node

- **GIVEN** a consumer profile with a catalog origin and no `dht` node
- **WHEN** a libp2p-only offer is listed
- **THEN** it is shown as unreachable from this profile
- **AND** no base URL or SDK snippet is presented for it

### Requirement: Learned addresses are filtered before dialing or advertising

Addresses learned from peers, including identify listen and observed addresses, Kademlia routing and provider-record addresses, DCUtR address exchanges, addresses listed in AutoNAT dial-back requests and the relay component of a relayed address, SHALL be dialed, retained or re-advertised only when every IP component satisfies the public-address predicate used for outbound HTTP origins, including its IPv4-mapped IPv6 handling. Learned private, loopback, link-local, CGNAT, multicast, broadcast, unspecified, documentation and benchmark addresses SHALL be dropped. A learned address whose host is a DNS name SHALL NOT be dialed. At most eight learned addresses SHALL be retained per peer.

Explicitly configured bootstrap and relay multiaddresses SHALL be exempt as operator configuration. Explicit development mode SHALL additionally permit learned literal loopback addresses only, never private LAN addresses. A provider SHALL advertise a direct address only after AutoNAT confirms it dialable and only when it satisfies the public predicate (or is literal loopback in development), and SHALL advertise a relayed address only through a relay on which it currently holds a reservation.

#### Scenario: Mixed identify addresses

- **WHEN** a peer's identify message lists `192.168.1.5`, `10.0.0.7`, `127.0.0.1` and a global unicast address
- **THEN** only the global unicast address is retained and dialed

#### Scenario: Relayed address through a private relay address

- **WHEN** a provider record advertises a relayed address whose relay component is in `100.64.0.0/10`
- **THEN** the consumer drops that relayed address

#### Scenario: Development exception stays narrow

- **GIVEN** explicit development mode
- **WHEN** a peer advertises a literal loopback address and an RFC 1918 address
- **THEN** the loopback address may be dialed and the RFC 1918 address is still dropped

#### Scenario: Configured relay is operator configuration

- **WHEN** the operator configures a relay multiaddress ending in `/p2p/<peer-id>`
- **THEN** the node dials it as configured, independently of the learned-address filter

### Requirement: Every node selects its Kademlia mode from AutoNAT-confirmed reachability

Every DHT node, whether a consumer, a provider with or without libp2p serving, or a relay, SHALL run AutoNAT to classify its own reachability. It SHALL operate Kademlia in server mode only while it has an AutoNAT-confirmed address that satisfies the public-address predicate (or, in explicit development mode, a confirmed literal loopback address). It SHALL operate in client mode otherwise, including from startup until such a confirmation and after a confirmed address is lost. Configuration SHALL NOT force Kademlia server mode. A node in client mode SHALL NOT advertise the Kademlia protocol to its peers.

Every node SHALL answer AutoNAT dial-back requests, subject to these restrictions: it SHALL dial back only addresses whose IP equals the requester's observed public IP and satisfies the public-address predicate (or is literal loopback in explicit development mode), SHALL NOT dial any other address the request lists, and SHALL bound its dial-backs per requesting peer and in total.

#### Scenario: Consumer behind NAT

- **GIVEN** a consumer node without an AutoNAT-confirmed public address
- **WHEN** it connects to the DHT
- **THEN** it runs Kademlia in client mode and does not advertise the Kademlia protocol
- **AND** other peers do not insert it into their routing tables

#### Scenario: Confirmed public node

- **GIVEN** a relay or provider node that started in Kademlia client mode
- **WHEN** AutoNAT confirms one of its global unicast addresses
- **THEN** the node switches to Kademlia server mode
- **AND** it returns to client mode if that confirmation is lost

#### Scenario: AutoNAT dial-back is restricted

- **WHEN** an AutoNAT request lists an RFC 1918 address, a loopback address and a public address whose IP differs from the requester's observed IP
- **THEN** the answering node dials none of them
- **AND** it dials back only addresses at the requester's observed public IP

### Requirement: Providers establish libp2p reachability explicitly

While libp2p serving is enabled, the provider SHALL hold a reservation on at least one configured relay, renew each reservation before it expires, re-reserve after loss with bounded backoff, and support DCUtR so that a consumer connected through a relay can upgrade to a direct connection. Holding a reservation SHALL NOT depend on having a published offer. The provider SHALL publish its AutoNAT-confirmed direct and current relayed multiaddresses through identify and its DHT records, and SHALL serve its offers on the manifest protocol defined by *Upgraded nodes serve and query only the v2 manifest protocol*, which MAY use the same connections.

The provider's authenticated loopback administration status SHALL report its AutoNAT classification, its Kademlia mode, each held reservation by relay peer ID with the limits that relay reported for the reservation, whether the most recent inbound circuit through each relay received serving-grade or coordination-sized limits, and its advertised addresses. This observation SHALL NOT be served on the public router or over the stream. Loss of every reservation without a confirmed direct address SHALL be reported as libp2p-unreachable and SHALL NOT change published prices, accounts or agreement terms.

#### Scenario: Provider behind NAT

- **GIVEN** AutoNAT classifies the provider as not publicly reachable
- **WHEN** libp2p serving starts with one configured relay
- **THEN** the provider holds a reservation on that relay and advertises the relayed address
- **AND** its DHT node runs in Kademlia client mode

#### Scenario: Reservation before publication

- **GIVEN** a provider started drained before its first publication, with no published offer
- **WHEN** libp2p serving starts
- **THEN** the provider holds a reservation on a configured relay
- **AND** administration status reports that reservation and that its circuits receive coordination-sized limits

#### Scenario: Publicly reachable provider

- **WHEN** AutoNAT confirms a global unicast address as dialable
- **THEN** the provider advertises that direct address and its DHT node runs in Kademlia server mode
- **AND** it still holds at least one relay reservation

#### Scenario: All reservations lost

- **GIVEN** a provider without a confirmed direct address
- **WHEN** every relay reservation is lost
- **THEN** administration status reports libp2p-unreachable and the provider retries reservations with bounded backoff
- **AND** prices, accounts and agreement terms are unchanged

### Requirement: Upgraded nodes serve and query only the v2 manifest protocol

v2 offers SHALL be served only on `/chiral/inference/manifest/2`. That protocol SHALL use the request and response framing, the 2,048-byte request and 262,144-byte response frame bounds, the 64-offer manifest limit, 8-offer pages, aligned offsets, the `next` rule, the 16 KiB per-offer bound and the responding-peer binding defined for `/chiral/inference/manifest/1`, and searches on it SHALL keep the existing search peer sample, collection window, concurrent-search limit and four-second manifest RPC timeout.

A node implementing v2 SHALL retire `/chiral/inference/manifest/1` at once, with no acceptance window: it SHALL NOT support `/1` for inbound requests, SHALL NOT send `/1` requests, and SHALL NOT publish, serve or list a v1 offer on any manifest protocol or catalog. There SHALL be no v1 acceptance cutoff, no v1 counterpart of a v2 offer, no period during which a node serves or queries both protocols, and no discovery that mixes v1 and v2 offers. Private testnet nodes upgrade together; an un-upgraded node neither receives v2 offers from upgraded nodes nor contributes offers to them, and fails protocol negotiation instead of misdecoding a page.

A receiver SHALL treat a `/2` page containing a v1 payload as an invalid manifest: it SHALL insert no offer from that page and SHALL NOT use it to withdraw any offer. A complete valid `/2` manifest SHALL withdraw that peer's cached offers omitted from the requested model scope, under the existing withdrawal rule. A candidate peer that does not support `/2` SHALL be treated as having no offers: the failed negotiation SHALL NOT be treated as an empty or invalid manifest, SHALL NOT withdraw any cached offer, and SHALL NOT by itself make the search result incomplete.

The relay catalog SHALL list only v2 offers, as individual entries in its existing `/api/offers` page format. A catalog consumer SHALL decode and validate each entry independently, and SHALL skip an entry that is a v1 offer or that it cannot decode, rather than the whole page.

Retiring `/1` SHALL govern only new discovery, quotes and accounts; top-ups, requests and recovery of an existing account SHALL continue under its saved terms. It SHALL NOT affect the parsing, verification or service of v1 offers, quotes, acknowledgements and receipts persisted in saved consumer accounts and provider ledgers, which `inference-commercial-terms` keeps for the life of each account; serving such an account requires no manifest.

#### Scenario: An upgraded node neither serves nor queries `/1`

- **GIVEN** a node implementing v2
- **WHEN** a peer opens `/chiral/inference/manifest/1` to it
- **THEN** protocol negotiation fails and no offer is served
- **AND** the node's own searches send no `/1` request, and every page it serves on `/2` contains only v2 offers

#### Scenario: Un-upgraded candidate

- **WHEN** a search candidate, such as a node that does not implement v2, does not support `/2`
- **THEN** the receiver records no offers from that candidate and sends it no `/1` request
- **AND** no cached offer is withdrawn because of the failed negotiation

#### Scenario: v1 payload on a `/2` page

- **WHEN** a `/2` page contains a v1 payload
- **THEN** the receiver inserts no offer from that page
- **AND** no cached offer of that peer is withdrawn because of it

#### Scenario: Catalog page with a v1 entry

- **WHEN** a catalog page read by a consumer implementing v2 lists a v1 entry alongside v2 entries
- **THEN** the consumer skips the v1 entry as a retired schema
- **AND** it still validates and uses the v2 entries on that page

#### Scenario: Retirement does not strand a saved v1 account

- **GIVEN** a saved account whose retained offer and quote are v1, opened before consumer and provider upgraded
- **WHEN** both nodes have retired `/1`
- **THEN** the consumer still uses, reads and recovers the account through the provider's public router, as `inference-commercial-terms` and *Transport selection follows the signed offer and never changes money or terms* state
- **AND** no `/1` manifest or v1 offer publication is needed for it

### Requirement: Discovery of relay-only providers fits relay budgets

A serving relay's serving-grade circuit byte limit SHALL leave room for discovery. Serving-relay mode SHALL fail to start when that limit is smaller than one maximum manifest exchange on `/chiral/inference/manifest/2` (a 2,048-byte request frame and a 262,144-byte response frame, each with its four-byte length prefix), plus a fixed allowance for the identify and Kademlia traffic on the same connection that the implementation defines and documents.

A consumer search SHALL keep at most four concurrent relayed circuits through any one relay, and SHALL open new circuits through any one relay no faster than libp2p's default per-source circuit rate: an allowance of 30 circuits, refilled at one circuit per 120 seconds. It SHALL reuse an existing authenticated connection to a candidate when it has one. These bounds SHALL NOT raise the search's existing peer sample, collection window, concurrent-search limit or manifest RPC timeout.

A candidate that the search did not query because of these bounds, or whose manifest exchange a relay refused or cut, SHALL make the search result incomplete. The result SHALL state that it is incomplete and how many candidates were affected. A partial exchange SHALL NOT be treated as a complete manifest and SHALL NOT withdraw any cached offer.

#### Scenario: More relay-only providers than one relay's per-source bound

- **GIVEN** ten providers reachable only through relay `R`
- **WHEN** a consumer search reaches them as candidates
- **THEN** at most four circuits through `R` are open at any time and new circuits through `R` stay within the default per-source rate
- **AND** every provider whose manifest was collected within the search window is returned
- **AND** if any candidate was not queried, the result states that it is incomplete and how many candidates were affected

#### Scenario: Relay cut during discovery

- **WHEN** a relay cuts a circuit during a manifest page exchange
- **THEN** that page is not used as a complete manifest and no cached offer of that peer is withdrawn
- **AND** the search result states that it is incomplete

#### Scenario: Serving-grade byte limit below the discovery floor

- **WHEN** a relay enables serving mode with a serving-grade circuit byte limit smaller than one maximum manifest exchange on `/chiral/inference/manifest/2` plus the documented allowance
- **THEN** the relay refuses to start in serving mode

### Requirement: Consumers dial along the direct, hole-punched and relayed ladder

To reach a libp2p provider, the native consumer SHALL resolve the offer's `peer_id` to addresses through its own DHT node and identify, apply the learned-address filter, attempt filtered direct addresses first, then connect through an advertised relayed address and allow DCUtR to upgrade to a direct connection, and keep the relayed circuit when hole punching fails. Each connection attempt SHALL be bounded by the ten-second connect deadline, and the whole ladder SHALL be bounded by a deadline. When the consumer profile restricts relays, the consumer SHALL use only relayed addresses through the listed relays.

The consumer SHALL classify the connection carrying each request as **Direct** (a non-relayed connection dialed directly), **Hole-punched** (a direct connection established by DCUtR) or **Relayed** (a circuit relay v2 connection). It SHALL dispatch each request on the best authenticated connection available at dispatch, subject to *Paid requests are dispatched only on circuits that can carry them*. An in-flight stream SHALL remain on the connection where it started; only later requests SHALL use an upgraded connection. When no authenticated path is established within the ladder deadline, the consumer SHALL report the provider unreachable over libp2p without sending a request.

#### Scenario: Directly dialable provider

- **WHEN** the provider advertises an AutoNAT-confirmed global address that the consumer can dial
- **THEN** requests travel over a Direct connection without a relay

#### Scenario: Hole punch succeeds after a relayed start

- **GIVEN** a request in flight on a Relayed connection
- **WHEN** DCUtR establishes a direct connection to the same authenticated peer
- **THEN** the in-flight request finishes on the relayed connection
- **AND** later requests use the Hole-punched connection and are labeled Hole-punched

#### Scenario: Hole punching fails

- **WHEN** both peers are behind symmetric NAT or CGNAT and DCUtR fails
- **THEN** requests use the Relayed connection and are labeled Relayed

#### Scenario: No path

- **WHEN** neither a direct address nor a usable relay yields an authenticated connection before the ladder deadline
- **THEN** the consumer reports the provider unreachable over libp2p and sends no request bytes

### Requirement: Serving relays enforce explicit operator-chosen limits

A relay SHALL carry serving traffic only in an explicitly enabled serving mode whose configuration states every one of: the serving-grade maximum circuit duration and maximum bytes per circuit, per-peer bandwidth, maximum reservations in total and per peer, maximum circuits in total and per peer, reservation lifetime, reservation rate limits per peer and per IP address, circuit rate limits per source peer and per source IP address, and the maximum number of offer-evidence records that *Serving-grade circuit limits apply only until 24 hours after the destination's last relay-verified offer expires* defines, which SHALL be at least the maximum number of reservations. Serving mode SHALL fail to start when any of these values is missing or zero, when the evidence-record maximum is below the reservation maximum, or when the serving-grade circuit byte limit is below the discovery floor of *Discovery of relay-only providers fits relay budgets*, and SHALL NOT inherit libp2p defaults for them. This capability SHALL NOT fix their values; they are operator choices, subject for project relays to the minimum below.

Coordination-sized limits SHALL be libp2p's default circuit limits: 120 seconds and 131,072 bytes per circuit. These suffice only for hole-punch coordination, reachability checks and small manifests. A node not configured in serving mode SHALL NOT grant any circuit beyond coordination-sized limits.

A relay SHALL account each circuit's limits for the whole circuit: its duration from establishment, and its bytes in both directions across every stream on the relayed connection. It SHALL report each circuit's effective limits in its connection response, and each reservation's limits in its reservation response, as circuit relay v2 provides. It SHALL enforce the limits by closing the circuit, and both peers SHALL observe that closure as a transport failure. Per-peer bandwidth SHALL be enforced across all of a peer's circuits. The per-peer circuit and bandwidth limits SHALL count every circuit for which the peer is the source or the destination, so that they also bound each provider's circuits and relayed bytes. A relay SHALL NOT be a party to an inference agreement. Relay operation SHALL add no fee, payment, agreement or term to the inference protocol: no offer, quote, funding-intent acknowledgement, receipt or feedback SHALL name a relay or carry a relay charge, and a relay SHALL NOT alter the payee, price, platform fee or terms of an inference agreement.

Shipped profiles MAY name serving relays that the project operates (project relays) in their relay lists, and a provider MAY list in its libp2p serving configuration relays that any operator runs, alongside or instead of project relays. A listed relay SHALL be explicit configuration, and every relay, whoever operates it, SHALL be subject to the same peer-ID authentication, circuit-budget check, path disclosure and lack of protocol authority. A project relay SHALL run in serving mode. Its serving-grade maximum circuit duration and maximum circuit bytes SHALL at least fit one paid request per circuit as the circuit-budget check of *Paid requests are dispatched only on circuits that can carry them* computes it: a freshly established serving-grade circuit through it, after the time and bytes that the implementation documents for connection setup, identify and Kademlia before the first dispatch, SHALL have remaining bytes that cover the head and the public router's maximum inference request body plus the 8 MiB response-size bound with their transport framing, and remaining duration that covers the stream client's inference response deadline. A project relay SHALL enforce per-provider reservation, circuit and bandwidth quotas through the per-peer limits above, including for circuits granted during the 24-hour grace period. A project relay's limit and quota values SHALL be implementation inputs, fixed in its shipped configuration within this capability's implementation and measured by its release gates for relay saturation, quotas and abuse and for relayed latency and throughput; a relay operated by anyone else SHALL keep its operator's choices. On a relay whose serving-grade circuits cannot carry a paid request, the consumer SHALL NOT dispatch it and SHALL fall back or report the request not sent as that requirement states.

#### Scenario: Incomplete serving configuration

- **WHEN** a relay enables serving mode without a per-peer bandwidth, circuit byte limit or circuit rate limit
- **THEN** the relay refuses to start in serving mode instead of applying a default

#### Scenario: Circuit limit reached

- **WHEN** the bytes a relayed circuit has carried in both directions, across all its streams, reach the circuit's byte limit, or its duration limit elapses
- **THEN** the relay closes the circuit
- **AND** both peers report a transport failure rather than a completed response

#### Scenario: Non-serving node

- **GIVEN** a node that is not configured as a serving relay
- **WHEN** it relays a circuit
- **THEN** the circuit is limited to 120 seconds and 131,072 bytes

#### Scenario: Per-peer reservation quota

- **WHEN** one peer requests more reservations than the configured per-peer maximum
- **THEN** the relay refuses the excess reservation without affecting other peers' reservations

#### Scenario: Circuit rate per source peer

- **WHEN** one source peer requests circuits faster than the configured per-source circuit rate
- **THEN** the relay refuses the excess circuits with a resource-limit status
- **AND** other source peers' circuits are unaffected

#### Scenario: Per-provider circuit quota

- **GIVEN** a provider peer that is already the destination of the configured per-peer maximum number of circuits through a serving relay
- **WHEN** another consumer requests a circuit to that provider through the relay
- **THEN** the relay refuses that circuit with a resource-limit status
- **AND** other providers' circuits are unaffected
- **AND** the refused consumer writes no request byte on that path, and a request it cannot dispatch on another path is reported as not sent

#### Scenario: No relay fee or term

- **GIVEN** a paid account used over a Relayed path, whether through a project relay or a relay another operator runs
- **WHEN** the consumer funds the account, sends requests and receives receipts
- **THEN** funding pays the signed quote's amounts to the offer's `provider_wallet` and nothing to the relay
- **AND** no offer, quote, funding-intent acknowledgement, receipt or feedback names the relay or carries a relay fee, payment or term

#### Scenario: Project relay carries one paid request

- **GIVEN** a project relay running its shipped serving configuration
- **WHEN** a consumer establishes a fresh serving-grade circuit through it to a provider with a currently valid `libp2p` offer, and is ready to dispatch a paid inference request whose body is the public router's maximum inference request body
- **THEN** the circuit-budget check passes for that request with the 8 MiB response-size bound and the stream client's inference response deadline
- **AND** a candidate project relay configuration whose serving-grade duration or byte limit fails this check is not named in a shipped profile

#### Scenario: Relay list mixes operators

- **GIVEN** a provider whose libp2p serving configuration lists a project relay named by its shipped profile and a relay that another operator runs
- **WHEN** libp2p serving starts and consumers reach the provider through each relay
- **THEN** the provider holds a reservation on each listed relay, treating both as configured relays
- **AND** consumers apply the same peer-ID authentication, circuit-budget check and path disclosure on either relay, and neither relay becomes a party to any agreement

### Requirement: Serving-grade circuit limits apply only until 24 hours after the destination's last relay-verified offer expires

A relay SHALL grant any peer, within its reservation quotas and rate limits, a reservation whose circuits receive coordination-sized limits. Holding a reservation SHALL NOT require an offer.

A qualifying offer for a destination peer is a signed offer that meets all of these conditions when the relay verifies it:

- it is a v2 offer that verifies under `inference-commercial-terms` for the relay's selected network;
- it is unexpired;
- it names the destination as `peer_id` and declares `libp2p`;
- it was obtained by authenticated manifest retrieval over `/chiral/inference/manifest/2` from that destination peer, either over the destination's own connection to the relay or held in the relay's verified registry from that destination's own manifest response.

An offer supplied by another peer, by a catalog or by any unauthenticated source SHALL NOT qualify. The offer's prices, minimum funding, free allowance and serving profile SHALL NOT affect qualification: a free offer whose `serving` is `null` qualifies like a priced offer with a serving profile, and neither SHALL be treated as proof of qualification or payment. A new revision that changes only the price, including a change between free and priced terms made by automatic pricing, SHALL be recorded like any other qualifying offer and SHALL NOT change the limits of later circuits.

The grace end for a destination is the signed `expires_at` of the most recent qualifying offer the relay verified for that destination, plus 24 hours (86,400 seconds). Until that offer's `expires_at` the destination has a current qualifying offer; from its `expires_at` until the grace end the destination is in its grace period. A serving relay SHALL apply its serving-grade circuit limits to a circuit only when, at the time the circuit is established, it holds an offer-evidence record for the destination whose grace end has not passed under the clock rule below. Every other circuit SHALL receive only coordination-sized limits. The grace period SHALL count from that signed `expires_at`, not from the time the relay verified the offer, not from a later retrieval that found no qualifying offer and not from any other event, so that it covers every way a destination stops having a qualifying offer: expiry after a drain or *Stop sharing*, withdrawal by complete manifest, a publication pause during a chain or RPC outage, and a performance-withdrawal latch. A circuit granted during the grace period SHALL receive the same serving-grade limits and SHALL count against the same per-peer reservation, circuit, bandwidth and rate limits, and therefore the same per-provider quotas, as any other serving-grade circuit. Circuits already open SHALL keep the limits they were granted when the destination's offers lapse or its grace period ends.

Clock rule: the relay SHALL compare its own wall clock, in integer Unix seconds, with the signed integer `expires_at` of the recorded offer, with no skew or other allowance: the grace end has not passed while the relay's wall clock reads less than `expires_at` plus 86,400 seconds, and has passed once it reads that value or more. Because offer acceptance limits issuance to at most 30 seconds ahead of the relay's clock and lifetime to at most 3,600 seconds, a recorded `expires_at` is at most 3,630 seconds after the relay's wall-clock time of verification. The relay SHALL also record its monotonic time of each verification and SHALL treat the grace end as passed, whatever its wall clock reads, once 90,030 seconds (3,630 plus 86,400) have elapsed on its monotonic clock since that verification, so that a backward step of its wall clock cannot extend a record beyond that bound. A forward step of its wall clock can only end a grace period early.

The relay SHALL keep at most one offer-evidence record per destination peer ID, scoped to its selected network. Whenever it verifies a qualifying offer for that peer, it SHALL create the record or replace its offer fields with that offer's content ID and signed `expires_at` and the relay's monotonic time of that verification, so that the record always describes the most recent qualifying offer the relay verified; when one manifest response yields more than one qualifying offer, the relay SHALL record the one with the latest `expires_at`. A record SHALL contain only that peer ID, the network scope, that content ID, that `expires_at` and that verification time; it SHALL contain no account, consumer, request or payment data. A record SHALL be removed once its grace end has passed under the clock rule. The number of records SHALL NOT exceed the configured evidence-record maximum; when a new record would exceed it, the relay SHALL evict the record whose grace end is earliest, and among records with the same grace end the one with the oldest verification time. Removal or eviction SHALL only ever reduce later circuits to coordination size.

The relay SHALL obtain evidence by requesting the destination's `/2` manifest over the destination's connection to the relay, or from its verified registry. It SHALL do so when a circuit is requested to a destination with no record or whose recorded offer has expired, and, while a destination with a record holds a reservation on the relay, when the recorded offer expires. It SHALL NOT request one destination's manifest more than once per 120 seconds or with more than one request in flight, and needs only the first page that contains a qualifying offer. A retrieval that fails, is refused or finds no qualifying offer SHALL leave the record unchanged, so its grace end stays 24 hours after the recorded `expires_at`.

Offer-evidence records SHALL be held in memory only. After a relay restart the relay SHALL hold no records, and a destination SHALL receive serving-grade limits through it again only after it verifies a qualifying offer for that destination; a destination without a current qualifying offer SHALL receive only coordination-sized circuits through a restarted relay, even before 24 hours have passed since its last valid offer expired, so a restart ends the grace period early on that relay. The relay's documented serving configuration SHALL state this restart behavior, and consumer disclosure SHALL state it as *The active path is disclosed before funding and during use* requires. Evidence SHALL NOT make the relay a party to, custodian of or attestor for any offer or agreement.

#### Scenario: Peer without an offer is reachable for coordination

- **GIVEN** a peer with no published signed offer and no offer-evidence record on the relay whose grace end has not passed, such as a provider started drained before its first publication
- **WHEN** it requests a reservation within the relay's quotas
- **THEN** the relay grants a reservation with coordination-sized limits
- **AND** a peer dialing it through that relay obtains a circuit limited to 120 seconds and 131,072 bytes, over which the destination's peer ID is authenticated and DCUtR can run

#### Scenario: Destination with a valid libp2p offer

- **WHEN** a circuit is requested to a destination whose authenticated `/2` manifest contains an unexpired, verified v2 offer for the relay's network that names the destination and declares `libp2p`
- **THEN** the relay grants the circuit its serving-grade limits and reports them in its connection response
- **AND** it does so whether that offer is priced or free and whether its `serving` is a serving profile or `null`

#### Scenario: A price revision does not change circuit grading

- **GIVEN** a relay holding an offer-evidence record for destination `D` from a free qualifying offer with a free allowance
- **WHEN** `D` publishes a higher revision that changes only the price to nonzero rates with a `null` allowance, and the relay verifies it
- **THEN** the relay replaces the record's offer fields with that revision's content ID and `expires_at`
- **AND** later circuits to `D` receive the same serving-grade limits as before, and circuits already open keep theirs

#### Scenario: Destination without qualifying evidence

- **GIVEN** a relay that holds no offer-evidence record for the destination, or whose record's grace end has passed
- **WHEN** the destination has no offer, only expired offers, only v1 or `https`-only offers, offers for another network, an offer naming a different peer ID, or an offer supplied only by another peer
- **THEN** the relay grants the circuit only coordination-sized limits

#### Scenario: Grace after the destination's offers lapse

- **GIVEN** a relay whose most recently verified qualifying offer for destination `D` carries the signed `expires_at` `E`
- **AND** `D` then has no qualifying offer because it drained, stopped sharing, paused publication during a chain outage or latched performance withdrawal
- **WHEN** a circuit to `D` is requested while the relay's wall clock reads less than `E` plus 86,400 seconds
- **THEN** the relay grants the circuit its serving-grade limits and reports them in its connection response
- **AND** while `D` keeps serving its existing accounts, a consumer can read request status and receipts and cancel requests for its account with `D` over that circuit

#### Scenario: Grace counts from the signed expiry, not the verification

- **GIVEN** a relay that verified, at wall-clock time `t`, a qualifying offer for destination `D` with signed `expires_at` `t` plus 3,600 seconds
- **AND** `D` withdrew its offers by complete manifest at `t` plus 60 seconds, and the relay's retrieval when the recorded offer expired found no qualifying offer
- **WHEN** a circuit to `D` is requested at `t` plus 88,000 seconds on the relay's wall clock, more than 24 hours after the verification but less than 24 hours after `t` plus 3,600
- **THEN** the relay grants the circuit its serving-grade limits
- **AND** its record for `D` still holds `expires_at` `t` plus 3,600, unchanged by the retrieval that found no qualifying offer

#### Scenario: Grace ends 24 hours after the recorded expiry

- **GIVEN** a relay whose most recently verified qualifying offer for destination `D` carries the signed `expires_at` `E`, with no later verification
- **WHEN** a circuit to `D` is requested while the relay's wall clock reads `E` plus 86,400 seconds or more
- **THEN** the relay grants the circuit only coordination-sized limits and no longer holds a record for `D`
- **AND** a serving-grade circuit to `D` opened before then continues under the limits it was granted

#### Scenario: A wall-clock step does not extend the grace

- **GIVEN** a relay that verified a qualifying offer for destination `D` at monotonic time `m`, with no later verification
- **AND** the relay's wall clock was stepped backward, so that it still reads less than the recorded `expires_at` plus 86,400 seconds
- **WHEN** a circuit to `D` is requested at monotonic time `m` plus 90,030 seconds or later
- **THEN** the relay grants the circuit only coordination-sized limits
- **AND** the relay has removed its record for `D`

#### Scenario: Quotas still apply during grace

- **GIVEN** a destination within its grace period that is already the destination of the configured per-peer maximum number of circuits through the relay
- **WHEN** another consumer requests a circuit to it
- **THEN** the relay refuses that circuit with a resource-limit status, exactly as for a destination with a current offer
- **AND** the refused consumer's request is not sent unless another path passes the circuit-budget check

#### Scenario: Relay restart ends grace

- **GIVEN** a relay whose most recently verified qualifying offer for destination `D` expired one hour ago, and `D` has had no qualifying offer since
- **WHEN** the relay restarts and a circuit to `D` is then requested
- **THEN** the relay grants only coordination-sized limits
- **AND** `D` receives serving-grade circuits through that relay again only after the relay verifies a qualifying offer for it

#### Scenario: Evidence records are bounded

- **GIVEN** a relay holding the configured maximum number of offer-evidence records
- **WHEN** it verifies a qualifying offer for a destination that has no record
- **THEN** it evicts the record whose grace end is earliest and records the new verification
- **AND** a destination whose record was evicted and that has no current qualifying offer receives only coordination-sized circuits

### Requirement: Paid requests are dispatched only on circuits that can carry them

Circuit limits SHALL be accounted for the whole circuit: one relayed connection, both directions and every stream on it, including identify, Kademlia, manifest and serving streams, measured at the transport level including Noise and yamux framing. A circuit SHALL be coordination-sized when its reported duration limit is at most 120 seconds or its reported byte limit is at most 131,072 bytes. The native consumer SHALL NOT open a `/chiral/inference/http/1` stream on a coordination-sized circuit. The provider SHALL refuse a `/chiral/inference/http/1` stream that arrives on an inbound relayed connection whose reported limits are coordination-sized.

Before writing any byte of a paid inference request on a relayed connection, the native consumer SHALL compute that circuit's remaining duration and byte budget from the limits the relay reported for it. The remaining budget is those limits less the time elapsed since the circuit was established, the bytes already carried in either direction, and the bytes still reserved by other in-flight requests on that circuit. The consumer SHALL dispatch only when the remaining bytes cover the request's head and body plus the stream client's 8 MiB response-size bound, with their transport framing, and the remaining duration covers the response deadline the stream client applies to that request. The bytes so reserved SHALL stay reserved for that request until its response ends.

Otherwise the consumer SHALL first attempt, within the ladder deadline, a DCUtR upgrade to the same authenticated peer or a fresh serving-grade circuit through a relay allowed for that provider, and SHALL dispatch on a connection that passes the check. If none passes, it SHALL report the request as not sent, with no request byte written. A circuit for which the relay reported no limit SHALL be disclosed as reporting none, SHALL NOT be treated as coordination-sized, and SHALL pass this check; a later cut of it SHALL remain a transport failure.

In this requirement a paid inference request includes an inference request on a free account. Such a request draws down the account's signed free allowance (`inference-commercial-terms`, *Free accounts are served only within their signed allowance*) even when its response is lost, and its bytes share the circuit with paid requests, so the native consumer SHALL check, reserve and report it exactly as a paid request.

#### Scenario: Insufficient remaining budget

- **GIVEN** a relayed circuit whose remaining byte budget is smaller than a paid request's head and body plus 8 MiB
- **AND** neither a DCUtR upgrade nor a fresh serving-grade circuit can be established within the ladder deadline
- **WHEN** the user sends that paid chat completion
- **THEN** no request byte is written
- **AND** the request is reported as not sent, with no reconciliation required

#### Scenario: Fresh circuit carries the request

- **GIVEN** a relayed circuit whose remaining duration is shorter than the request's response deadline
- **WHEN** a fresh serving-grade circuit to the same authenticated peer with sufficient budget is established
- **THEN** the paid request is dispatched on the fresh circuit and is labeled Relayed

#### Scenario: Coordination-sized circuit

- **GIVEN** the only authenticated path to the provider is a coordination-sized circuit
- **WHEN** the consumer would send an inspection, quote, control, status or inference request
- **THEN** it opens no serving stream on that circuit
- **AND** the provider refuses a serving stream that arrives on such a circuit

#### Scenario: Concurrent requests share one circuit budget

- **GIVEN** an in-flight paid request on a relayed circuit that holds its reserved budget
- **WHEN** a second paid request needs more than the remaining unreserved budget
- **THEN** the second request is not dispatched on that circuit

#### Scenario: A free-account request is checked like a paid one

- **GIVEN** a consumer with a free account and a paid account at the same provider, and a relayed circuit to it whose remaining unreserved byte budget is smaller than a request's head and body plus 8 MiB
- **WHEN** the user sends a chat completion on the free account and neither a DCUtR upgrade nor a fresh serving-grade circuit passes the check within the ladder deadline
- **THEN** no request byte is written and the request is reported as not sent
- **AND** a free-account request dispatched on a circuit holds its reserved budget until its response ends, so a paid request on the same circuit is checked against the remainder

### Requirement: Relays forward end-to-end encrypted streams without protocol authority

Inference traffic over a relayed circuit SHALL be protected end to end by the Noise session between the consumer and provider peers, and the relay SHALL forward only those encrypted bytes. The relay SHALL NOT receive or need any account bearer, owner proof, wallet key, administrator token or account state, and SHALL NOT terminate, parse or proxy the HTTP carried by a circuit. Modification of relayed bytes SHALL cause the session to fail rather than deliver altered content. A relay's refusal, delay, drop or cut SHALL be a transport failure. The relay catalog SHALL continue not to proxy inference.

#### Scenario: Tampered relayed bytes

- **WHEN** a relay alters bytes of a relayed session
- **THEN** the Noise session fails and the consumer reports a transport failure
- **AND** no altered prompt, output or signed object is accepted

#### Scenario: Relay sees no content or secrets

- **WHEN** an authorized paid request travels over a relayed circuit
- **THEN** the relay's logs and state contain only peer IDs, addresses, timing and byte counts, with no bearer, owner proof, prompt or output

### Requirement: Transport failures are recovered through request status, never a blind paid retry

A refused, reset, cut or timed-out stream, circuit or connection, a hole-punch failure during a request, and a peer-ID refusal SHALL be transport failures. When a transport failure occurs after any byte of an inference request was written, the native consumer SHALL treat the outcome as ambiguous, retain the request's `Idempotency-Key` and any received request ID, and SHALL NOT automatically re-dispatch it on the same or another path, transport or provider. Recovery SHALL re-establish an authenticated path, which may use a different rung of the ladder, and read the request's status by request ID, or by the retained key in the account's request list when no ID was received, before any new chargeable operation is offered for that request. A user-initiated resend SHALL reuse the original key so that a request the provider already admitted returns `duplicate_request`. A failure before any request byte was written, including a request withheld by the circuit-budget check, SHALL be reported as not sent.

Control and funding operations interrupted by a transport failure SHALL follow the existing durable funding and recovery state machine with their original evidence and SHALL NOT prompt for a second payment. Cancellation SHALL use `POST /v1/requests/:id/cancel` over a re-established path and SHALL remain labeled as not a refund. When no path can be established, the consumer SHALL report the provider unreachable and keep the request and any pending funding unresolved and resumable, not failed, cancelled or refunded.

#### Scenario: Relay cuts a long SSE stream

- **GIVEN** a streaming request whose response headers and request ID were received over a relayed circuit
- **WHEN** the relay closes the circuit mid-stream
- **THEN** the consumer marks the request ambiguous and does not re-send it
- **AND** after reconnecting it reads `GET /v1/requests/:id` before offering any new paid request for that prompt

#### Scenario: Cut before the request ID arrives

- **WHEN** the stream fails after the request body was written but before response headers arrived
- **THEN** the consumer reconciles by finding the retained `Idempotency-Key` in the account's request list
- **AND** an explicit resend with the same key returns `duplicate_request` if the provider had admitted it

#### Scenario: Failure before sending

- **WHEN** the peer-ID check or dial fails before any request byte was written
- **THEN** the consumer reports the request as not sent and no reconciliation is required

#### Scenario: Interrupted funding control call

- **WHEN** a funding-intent or opening call over libp2p fails after it was written
- **THEN** the consumer surfaces the journaled stage and offers resume with the original evidence
- **AND** no second payment is prepared

#### Scenario: Provider unreachable during recovery

- **WHEN** no authenticated path to the provider can be established
- **THEN** the request and any pending funding remain unresolved and resumable and are reported as such

### Requirement: The active path is disclosed before funding and during use

Native consumer surfaces in the desktop and CLI SHALL show the active path to a libp2p provider as Direct, Hole-punched or Relayed, and for Relayed the relay's peer ID and the circuit limits the relay reported or a statement that it reported none; for an HTTPS provider they SHALL show HTTPS. Offer presentation SHALL show the declared transports and the peer ID, and SHALL show an endpoint only when one is declared.

Before an opening or top-up on a libp2p path, the pre-funding risk disclosure that the user must actively accept SHALL additionally state:

- the active path;
- for a Relayed path, that the relay sees both peers' IP addresses and peer IDs, connection timing and traffic volume, but not prompts, outputs, credentials or signed objects;
- for a Direct or Hole-punched path, that this machine's public IP address is revealed to the provider and the provider's to this machine;
- that a relay can delay, drop, refuse or cut circuits;
- for a Relayed path, that once the provider stops publishing valid offers, a serving relay carries its service to existing accounts, including request status, receipts and cancellation, only until 24 hours after the last of the provider's offers that this relay verified expires, which can be sooner than 24 hours after the provider's last published offer expires when the relay did not verify that offer, and not at all through a relay that restarts in that time; after that the account is reachable only over a Direct or Hole-punched path or after the provider publishes a valid offer again;
- that hole-punched and relayed paths add latency and can reduce throughput; for an offer with a serving profile, that this falls outside the provider's qualified guarantee, whose waivers cover provider-side misses only, and for an offer without one, that no speed guarantee applies on any path;
- that the path can change during use, and that a change from Relayed to Hole-punched reveals each side's public IP address to the other.

During use, each request SHALL be shown with the path of the connection that carried it, and the displayed path SHALL update when it changes. Path observations SHALL be computed natively and cross the native boundary as public data only; outside the native runtime no path SHALL be fabricated.

#### Scenario: Funding over a relayed path

- **GIVEN** the provider is reachable only through a relay
- **WHEN** the user reaches the funding step
- **THEN** the disclosure names the Relayed path, the relay peer ID, its reported limits, the relay's visibility of both peers' IP addresses and peer IDs, timing and volume, the latency caveat, and that once the provider stops publishing valid offers, relayed service ends 24 hours after the last of its offers that the relay verified expires, or earlier if the relay restarts
- **AND** funding stays disabled until the user actively accepts it

#### Scenario: Funding over a direct or hole-punched path

- **GIVEN** the active path at the funding step is Direct or Hole-punched
- **WHEN** the user reaches the funding step
- **THEN** the disclosure states that this machine's public IP address is revealed to the provider and the provider's to this machine
- **AND** funding stays disabled until the user actively accepts it

#### Scenario: Path changes during use

- **WHEN** a session that started Relayed is upgraded by hole punching
- **THEN** later requests are shown as Hole-punched and the in-flight request remains shown as Relayed

#### Scenario: HTTPS provider

- **WHEN** the selected offer is used over `https`
- **THEN** the path is shown as HTTPS and no relay or peer-to-peer disclosure is added

#### Scenario: Browser preview

- **WHEN** the GUI runs outside the native runtime
- **THEN** no transport path is displayed as observed and native path actions fail with an explanatory error

### Requirement: The transport path does not change qualification or waiver terms

Qualification, serving-profile performance claims and performance waivers SHALL remain provider-side measurements under the existing performance contract, with the same measurement points on either listener: time to first token SHALL start after the complete bounded request body arrives at the provider. Latency or throughput loss attributable to the transport path SHALL NOT be recorded as a provider miss, SHALL NOT create a waiver entitlement and SHALL NOT be presented as covered by the signed speed class. Measured relayed-path latency and throughput SHALL be recorded as release evidence separate from qualification reports and SHALL NOT be signed into a serving profile. An offer whose `serving` is `null`, including a free unqualified generation offer, SHALL NOT be presented on any path as carrying a speed class, speed guarantee or waiver.

#### Scenario: Slow relay delays the request body

- **WHEN** a relay delays delivery of a request body, but the body still completes within the body deadline
- **THEN** the provider's time to first token starts when the complete body arrives
- **AND** the relay delay neither counts as a provider miss nor creates a waiver

#### Scenario: Consumer display of speed

- **WHEN** a qualified offer is used over a Relayed path
- **THEN** the consumer shows the signed speed class as a provider-side target and does not present it as end-to-end latency over that path

#### Scenario: Free offer without a serving profile over a relay

- **GIVEN** a free v2 generation offer whose prices and minimum funding are zero, whose `serving` is `null` and which declares only `libp2p`
- **WHEN** it is used over a Relayed path
- **THEN** the consumer shows the path and relay disclosure but no speed class, speed guarantee or waiver for any path

### Requirement: Provider libp2p serving is configured explicitly on the persisted deployment identity

Provider configuration SHALL enable libp2p serving only through an explicit strict JSON block that rejects unknown fields and requires the provider's `dht` configuration. The block SHALL list 1–32 relay multiaddresses, each ending in `/p2p/<peer-id>`. It SHALL state, each as a mandatory nonzero value: the total and per-peer inbound stream limits, the request-head deadline, the maximum request-head size, the stream idle timeout and the maximum number of requests per stream. The serving identity SHALL be the persisted DHT identity whose peer ID the offers sign and whose peer ID scopes the provider's ledger, lock and offer-revision directory. A different identity SHALL be a different deployment, not a migration, and existing accounts SHALL remain bound to the original peer ID.

Published `transports` SHALL derive from configuration: `https` exactly when an advertised HTTPS origin is configured and `libp2p` exactly when libp2p serving is enabled. Configuration that enables neither SHALL be rejected. Enabling libp2p serving SHALL NOT change the public HTTP listener rules, the loopback administration listener, the private engine boundary or any secret file, and the libp2p identity file SHALL remain owner-only protected.

#### Scenario: libp2p-only provider configuration

- **WHEN** a provider configuration has no advertised HTTPS origin and enables libp2p serving with one relay
- **THEN** the provider publishes offers with `transports` `["libp2p"]` and `endpoint` `null` signed with its persisted peer ID

#### Scenario: Neither transport

- **WHEN** a provider configuration has no advertised origin and does not enable libp2p serving
- **THEN** configuration validation fails

#### Scenario: Missing stream-server limit

- **WHEN** the libp2p serving block omits the maximum request-head size, the idle timeout or the maximum requests per stream, or sets any stream limit to zero
- **THEN** configuration validation fails instead of applying a default

#### Scenario: Lost identity is a new deployment

- **WHEN** the provider starts with a newly generated libp2p identity
- **THEN** it operates as a different deployment with a different deployment directory
- **AND** accounts bound to the previous peer ID are not served or migrated under the new identity

### Requirement: Consumer libp2p use is configured explicitly

A consumer profile SHALL reach libp2p providers only through its configured `dht` node. An optional strict `p2p` block, rejecting unknown fields, MAY restrict relayed paths to an allowlist of 1–32 relay peer IDs. No relay, bootstrap peer or provider SHALL be selected implicitly; provider addresses SHALL come from configuration or filtered discovery. The consumer's libp2p identity SHALL be its existing owner-only protected DHT identity file and SHALL NOT be account, credential or payment authority.

#### Scenario: Relay allowlist

- **GIVEN** a consumer `p2p` block that allows only relay `R1`
- **WHEN** a provider advertises relayed addresses through `R1` and `R2`
- **THEN** the consumer uses only `R1` for relayed paths

#### Scenario: Unknown consumer field

- **WHEN** the consumer `p2p` block contains an unrecognized member
- **THEN** profile loading fails without ignoring the member

### Requirement: Operator provisioning supports libp2p-only provider deployments

The operator provisioning input accepted by `scripts/provision-provider.py` and documented in `docs/provider-qualification.md` SHALL accept a strict libp2p serving object with the same fields and bounds as the provider configuration's libp2p serving block. It SHALL make the provider DNS name optional when, and only when, that object is present. Provisioning without a provider DNS name SHALL write a provider configuration whose advertised origin is `null` and which contains the libp2p serving block, and SHALL generate no provider TLS ingress. Provisioning with a provider DNS name SHALL keep writing `https://<provider DNS name>` as the advertised origin and MAY also write the block. Input with neither SHALL be rejected. The relay and RPC DNS names and the ACME email SHALL remain required for the relay and RPC gateway they configure, and calibration SHALL remain loopback-only.

For a deployment without an advertised origin, `scripts/provider-preflight.py` SHALL run its health, readiness and refused-route checks against the loopback public listener instead of the advertised origin, and SHALL NOT construct a URL from a null origin. Through authenticated administration status it SHALL verify that libp2p serving is enabled with the configured relays, and that the provider holds at least one relay reservation or an AutoNAT-confirmed direct address.

#### Scenario: Provisioning a libp2p-only deployment

- **WHEN** the provisioning input has no provider DNS name and contains a valid libp2p serving object
- **THEN** the generated provider configuration has a `null` advertised origin and that libp2p serving block
- **AND** no TLS ingress is generated for the provider listener

#### Scenario: Provisioning with neither transport

- **WHEN** the provisioning input has neither a provider DNS name nor a libp2p serving object
- **THEN** provisioning fails before writing a deployment

#### Scenario: Preflight of a libp2p-only deployment

- **GIVEN** a provisioned deployment whose advertised origin is `null`
- **WHEN** the operator runs preflight against the running provider
- **THEN** health, readiness and refused-route checks run against the loopback public listener
- **AND** preflight fails unless administration status shows libp2p serving enabled with the configured relays and at least one held reservation or confirmed direct address
