## ADDED Requirements

### Requirement: Launch reputation is observed only by a catalog operator's service

Launch-reputation evidence, meaning availability samples and service probes, SHALL be produced only by the catalog operator's `relay_server` process (the *observer*). The observer uses that catalog's DHT service for address knowledge, its chain client, its outbound HTTPS client, a dedicated observer libp2p node for availability samples, and short-lived probe identities for probes. Native consumers, the desktop, the CLI, SDK clients, providers and provider daemons SHALL NOT take, submit or relay samples or probe results. No catalog route, libp2p protocol or configuration file SHALL accept a sample, a probe result, an uptime or quality report, a provider's self-assessment, a request to sample or probe a provider, or a request to change or delete a record.

The observer SHALL run only when the relay configuration contains an explicit `launch_reputation` object that rejects unknown fields and contains exactly:

- `reference_peers`: 1–8 libp2p multiaddresses, each ending in `/p2p/<peer-id>`, of DHT peers the operator chooses for observer health checks;
- `reference_origins`: 1–4 origins the operator chooses for observer health checks, each named by a DNS hostname rather than an IP literal, and each validated with the outbound-origin rules used for provider origins (public HTTPS on port 443, or exact `localhost` under the relay's explicit `development` flag);
- `tokenizer_runtime`: the path, resolved relative to the configuration file, of an installed llama.cpp runtime directory whose `chiral-runtime.json` inventory verifies as the project's pinned runtime identity for the host's platform and architecture. It is used only for billing checks.

When the object is absent, the observer SHALL NOT run, no provider SHALL be sampled or probed, and the launch-reputation routes SHALL report the record unavailable. When the object is present but invalid, including a tokenizer runtime that fails inventory verification or is not the pinned identity, the relay SHALL refuse to start. The sampling and probing schedules, deadlines, resource bounds, probe corpus, account limits, window, retention and storage bounds SHALL be fixed by this capability and SHALL NOT be configurable, so that the figures of independently operated catalogs are comparable.

The observer's libp2p node SHALL use its own key pair, generated from the operating system's cryptographically secure random source, and never the catalog's DHT identity. It SHALL replace that key pair with a fresh one at least every 24 hours, and SHALL NOT persist it. Its peer ID SHALL NOT be published in any launch-reputation response or catalog route. The node SHALL NOT run Kademlia, a relay server or a manifest server. Probe identities are separate from this node and are specified in *Probe accounts use fresh, unlinkable identities and stay within the free allowance*. A record SHALL identify its catalog by the catalog's network identity, and a reader identifies the catalog by the origin it queried.

#### Scenario: A catalog without the object takes no samples or probes

- **GIVEN** a relay configuration with no `launch_reputation` object
- **WHEN** the relay runs and discovers providers, including free ones
- **THEN** no sample or probe is taken and no provider is contacted for one
- **AND** `GET /api/launch-reputation/:wallet/:peer_id` returns HTTP 503 `launch_reputation_unavailable`

#### Scenario: Sampling and probing parameters cannot be configured

- **WHEN** the `launch_reputation` object contains `interval_seconds`, `probe_interval_seconds`, `concurrency` or any other member besides `reference_peers`, `reference_origins` and `tokenizer_runtime`
- **THEN** the relay refuses to start and names the unknown member

#### Scenario: A reference origin must be a DNS name

- **WHEN** a `reference_origins` entry is `https://203.0.113.7`
- **THEN** the relay refuses to start

#### Scenario: A tokenizer runtime that is not the pinned identity

- **WHEN** `tokenizer_runtime` names a llama.cpp directory whose inventory verifies but whose runtime identity differs from the project's pinned identity
- **THEN** the relay refuses to start

#### Scenario: Nobody can submit a sample or probe result

- **WHEN** a provider or any other client sends a request to a catalog route asking it to record that a provider was available or served a request well, or to sample or probe a provider now
- **THEN** no route accepts it, nothing is recorded and no outbound request is made because of it

#### Scenario: The observer is not the catalog's DHT peer

- **GIVEN** a catalog running the observer
- **WHEN** a provider records the peer ID of every connection that fetches its manifest for an availability sample over 48 hours
- **THEN** none of those peer IDs is the catalog's DHT peer ID, and at least two distinct observer peer IDs appear
- **AND** no launch-reputation response or catalog route names an observer or probe peer ID

### Requirement: Tracked identities come only from verified discovery and are bounded

A tracked identity SHALL be the tuple of the catalog's complete network identity (chain ID, genesis hash and deployment ID), a `provider_wallet` and a `peer_id`. The observer SHALL learn an identity only from a signed offer that passes the validation and revision rules of the catalog's verified offer registry for its own network, received in the manifest response of the peer named by that offer's `peer_id` and authenticated as that peer by libp2p, either during the catalog's DHT discovery or during a sample or probe. Whether the registry had room to insert the offer SHALL NOT matter. An offer obtained from another catalog, a consumer, an API request, an unauthenticated source, or another peer's manifest SHALL NOT create or refresh an identity.

*Observer-healthy time* SHALL be time measured on the monotonic clock and accumulated only between consecutive health checks that both found the observer healthy. It SHALL be persisted, so that it continues across restarts and does not advance while the observer is stopped or degraded. Every duration in this requirement SHALL be measured in observer-healthy time.

The observer SHALL sample at most 512 tracked identities at a time, and at most 8 per `provider_wallet`. When a newly learned identity would exceed either bound, the observer SHALL admit it only by evicting one tracked identity within the same bound that has been tracked for at least 24 hours and has had no available sample in the last 24 hours. Within the 512 bound any identity is a candidate; within the per-wallet bound only that wallet's identities are. The observer SHALL choose an identity that has never had an available sample before others, then the one with the oldest most recent available sample, then the oldest admission. An evicted identity SHALL stop being sampled and probed, SHALL keep its retained data and SHALL be reported with `untracked_reason` `evicted`. When no identity is eligible for eviction, or the store cannot create a row for the identity, the new identity SHALL NOT be admitted.

A refused identity SHALL be recorded in an in-memory refusal table of at most 1,024 entries. Each entry SHALL be kept until 24 hours after the identity's most recent refusal, and when the table is full the entry with the oldest most recent refusal SHALL be removed first. While its entry exists, the identity SHALL be reported as not tracked with reason `capacity` or `wallet_limit`. It SHALL be considered again the next time the catalog verifies one of its offers. After a relay restart the table is empty, and a refused identity SHALL be reported `not_discovered` until one of its offers is verified again.

A tracked identity that has had no available sample and no verified offer for 336 hours (14 days) SHALL stop being tracked, with reason `inactive`. An identity admitted again after eviction or inactivity SHALL continue its retained history rather than start a new one.

#### Scenario: Only the offer's own peer creates an identity

- **GIVEN** peer `P2` serves, in its manifest, a validly signed offer naming `peer_id` `P1`
- **WHEN** the catalog receives that manifest page
- **THEN** no identity for `P1` is created or refreshed from it

#### Scenario: Capacity with no dormant identity

- **GIVEN** 512 tracked identities, each of which had an available sample within the last 24 hours
- **WHEN** the catalog verifies an offer from a new identity
- **THEN** the new identity is not tracked, and its record reports `tracked: false` with `untracked_reason` `capacity` for 24 hours after that refusal
- **AND** no existing identity is evicted

#### Scenario: Capacity with a dormant identity

- **GIVEN** 512 tracked identities, one of which was admitted three days ago and has had no available sample for 30 hours of observer-healthy time
- **WHEN** the catalog verifies an offer from a new identity
- **THEN** the dormant identity stops being sampled and probed, keeps its retained data and reports `untracked_reason` `evicted`
- **AND** the new identity is tracked

#### Scenario: One wallet cannot occupy many slots

- **GIVEN** a wallet with 8 tracked identities that were all available within the last 24 hours
- **WHEN** the catalog verifies an offer from a ninth peer of that wallet
- **THEN** that identity is not tracked and reports `untracked_reason` `wallet_limit`

#### Scenario: An observer outage does not evict or inactivate

- **GIVEN** tracked identities whose most recent available samples were just before the relay stopped
- **WHEN** the relay stays stopped for 30 days and then restarts
- **THEN** no identity is evicted as dormant or untracked as `inactive` because of the outage

### Requirement: Availability samples follow a memoryless schedule with a deadline

The observer SHALL draw, for each tracked identity, a sequence of availability sample times from the operating system's cryptographically secure random source. The first drawn time SHALL be uniformly random between 0 and 300 seconds after the identity's admission, or after the observer starts for an identity that was already tracked. Each later drawn time SHALL follow the previous drawn time by an exponentially distributed interval with a mean of 300 seconds. There SHALL be no minimum spacing between samples. No request, provider action or response SHALL trigger, advance or postpone a drawn time.

An identity SHALL have at most 24 sample starts per UTC hour. A drawn time that would be the 25th start of its identity in its UTC hour SHALL be discarded and not replaced; that decision depends only on the count. A drawn time reached while the same identity's previous sample is still in progress SHALL wait until that sample ends. Each sample's start SHALL be committed to the store, as a start in its UTC hour, before the sample's first outbound attempt. Several starts MAY be committed in one transaction, provided each is committed before that sample's first attempt.

After a restart, nothing SHALL be recorded for the time the observer was stopped, and missed draws SHALL NOT be caught up. The persisted start counts SHALL keep the hourly cap across the restart.

Drawn times, waits and deadlines SHALL use the monotonic clock. The wall clock SHALL be used only for UTC-hour labels and for the recorded times that results report (`sharing_since`, `identity_since`, `untracked_since`, `observed_at`, `computed_at` and the probe, free-period and flag times).

Each sample SHALL complete within 30 seconds of its actual start, including every connection attempt, every manifest page and the readiness request. A sample still incomplete at that deadline SHALL be ended and classified as if its pending step had failed. Within a sample, these deadlines SHALL apply: the existing ten-second connect deadline per connection attempt, the four-second manifest RPC timeout per page, the five-second DNS deadline and a ten-second readiness response deadline.

#### Scenario: Sample times are unpredictable

- **GIVEN** an identity whose sample has just started at time `t`
- **WHEN** its next sample is drawn
- **THEN** the interval from the previous drawn time is exponentially distributed with a mean of 300 seconds, so the next start may come at any time after `t` that the hourly cap permits
- **AND** a request from the provider, a consumer or a catalog reader does not change it

#### Scenario: The hourly cap survives a restart

- **GIVEN** an identity with 24 committed sample starts in the current UTC hour
- **WHEN** the relay restarts within that hour
- **THEN** that identity's next sample starts no earlier than the next UTC hour

#### Scenario: A restart causes no burst

- **GIVEN** 512 tracked identities and a relay that was stopped for one hour
- **WHEN** the relay restarts
- **THEN** each identity's first sample is drawn uniformly within 300 seconds of the start
- **AND** no sample is recorded for the hour the relay was stopped

#### Scenario: A hung provider cannot hold a sample slot

- **WHEN** a provider accepts the connection and never answers its manifest request or readiness request
- **THEN** the sample ends no later than 30 seconds after it started
- **AND** its slot is released for other samples

### Requirement: Observer resource bounds never decide a sample's class from provider behaviour

The observer's libp2p node SHALL have its own connection limits: at most 64 pending outgoing and 128 established connections. The observer, including its probe identities, SHALL NOT open, hold or count against the catalog DHT swarm's pending, established, incoming or per-peer connections, its relay reservations and circuits, or its discovery dials. The observer node SHALL accept no inbound connection, except the hole-punch connection of a sample in progress once `p2p-provider-transport` is implemented. Every connection and circuit that the observer opens for a sample SHALL be closed when the sample ends.

At most 64 samples SHALL be in progress. A sample whose drawn time arrives while 64 are in progress SHALL wait, in order of drawn time, and SHALL be taken when a slot frees. Its deadline SHALL start when it starts, and waiting SHALL NOT make it unknown, skipped or unavailable.

When a sample starts, before contacting anything, the observer SHALL fix the sample's attempt plan. The plan SHALL NOT grow afterwards, and SHALL contain:

- at most three direct libp2p addresses of the identity: its verified addresses first, most recently verified first, then other allowed addresses known for it;
- at most one relayed path, through one relay, with the hole-punch upgrade of `p2p-provider-transport` through it. The observer SHALL prefer the catalog's own relay when the provider holds a reservation on it;
- for HTTPS readiness, at most two of the accepted addresses of the origin named by the identity's most recent qualifying offer. When the qualifying offer found by the sample names a different origin, at most two addresses of that origin SHALL be planned when the offer is found.

A *verified address* SHALL be one of these:

- for libp2p, a direct address that the observer, or the catalog's DHT, dialed and whose connection authenticated as the identity's `peer_id`;
- for HTTPS, an IPv4 address or IPv6 /64 at which the observer received a certificate-verified readiness response naming the identity.

Verified addresses SHALL be kept in memory only, at most three per identity per transport, with the oldest replaced by a newer one. They SHALL NOT be removed because an attempt fails or authenticates another peer, and SHALL be lost when the relay restarts. At most 16 identities SHALL hold a verified address within one IPv4 address or IPv6 /64. For any further identity, its addresses there SHALL remain unverified.

Each *destination* (IPv4 address or IPv6 /64) SHALL have a budget for attempts to addresses that are unverified for the attempting identity. Those attempts are libp2p connection attempts and HTTPS readiness connections. Each such attempt SHALL be admitted only if the destination has had fewer than 12 unverified attempts in the preceding 60 seconds. The admission SHALL be decided, and the attempt counted, when the address is planned; the count is never returned. An unverified address that is not admitted SHALL be left out of the plan. An identity SHALL plan unverified addresses in at most six distinct destinations per UTC hour. Attempts to an identity's verified addresses SHALL NOT draw on a destination budget. Probes SHALL plan verified addresses only, as *Probes follow a memoryless schedule with bounded rate and concurrency* states, and so never draw on a destination budget.

Each relay other than the catalog's own relay SHALL have an observer budget of four tokens, refilled at one token per 120 seconds and kept across observer key rotations. Each planned relayed path through that relay, whether for a sample or a probe, SHALL take one token when planned, which is never returned, so that at most four observer circuits through it are open at a time and new ones are opened no faster than libp2p's default per-source rate. A relayed path without a token SHALL be left out of the plan. The catalog's own relay SHALL exempt the current observer peer and current probe identities from its per-source reservation and circuit limits. It SHALL apply its destination admission, its per-peer quotas and its circuit classification to their circuits exactly as to any other source.

A sample SHALL be recorded as **skipped**, without contacting anything, when its connection plan is empty because of these budgets although an allowed address or relayed path is known for the identity. A sample SHALL also be recorded as skipped when no readiness address could be planned because of these budgets for its qualifying offer's origin, and the offer declares no libp2p readiness. Skipped samples SHALL be excluded from availability and reported in their own count.

No bound in this requirement SHALL make a sample unknown. When the observer's own node, or a probe identity's node, refuses one of its dials because of a local limit or an exhausted local resource, the observer SHALL be degraded under *Observer degradation makes samples and probes unknown, never unavailable or failed*, which applies to every identity alike.

#### Scenario: Sybils on a provider's own address cannot hide its downtime

- **GIVEN** a provider identity with a verified address, and 40 identities of other wallets that its operator runs at the same IPv4 address, each of which hangs every sample until the deadline
- **WHEN** the provider goes offline while the observer is healthy
- **THEN** each of its samples attempts its verified address without drawing on the destination budget
- **AND** those samples are unavailable, not unknown or skipped

#### Scenario: A backlog is waited out and counted

- **GIVEN** 64 samples in progress
- **WHEN** another identity's drawn time arrives
- **THEN** that sample waits, is taken when a slot frees, and is classified by its own outcome

#### Scenario: A budget-limited plan is skipped before contact

- **GIVEN** an identity with no verified address, whose only known address is in a destination that has had 12 unverified attempts in the preceding 60 seconds
- **WHEN** its sample starts
- **THEN** no connection is attempted
- **AND** the sample is recorded as skipped

#### Scenario: Observer sampling leaves the catalog's connection budget intact

- **GIVEN** 512 tracked identities that are all unreachable
- **WHEN** they are sampled and, where free, probed continuously
- **THEN** the catalog DHT swarm holds no connection opened by the observer or a probe identity
- **AND** the relay keeps accepting its configured inbound reservations and its discovery dials

#### Scenario: Connections close when a sample ends

- **WHEN** a sample ends
- **THEN** every connection and circuit it opened is closed

### Requirement: A sample is available only when the provider's own manifest and readiness verify

A sample SHALL be **available** only when, within its deadline, all of the following hold:

1. The observer has a libp2p connection, opened from the sample's plan, whose Noise-authenticated remote peer ID equals the identity's `peer_id`.
2. Over that connection, the observer reads manifest pages from offset 0 following `next`, at most 8 pages, until a page contains a **qualifying offer**. When the identity's pricing state, defined in *Probes run only while the provider's current verified offer is free*, is not `free`, the observer SHALL instead continue until a page contains a qualifying free offer or the manifest ends, still at most 8 pages, so that a return to free is detected. The current manifest protocol SHALL be used: `/chiral/inference/manifest/1` until `p2p-provider-transport` is implemented and `/chiral/inference/manifest/2` after it, never both on one observer. A qualifying offer is a signed offer that:
   - passes the registry's validation rules for the catalog's network (size, network, signature, model, endpoint or transports, money, free allowance, peer ID, deployment ID, capacity, fee and lifetime);
   - passes a revision comparison against the registry's entry for its namespace when one exists;
   - is unexpired when verified;
   - names the identity's `provider_wallet`;
   - names the identity's `peer_id`, which equals the responding peer.
3. A readiness request for that qualifying offer returns HTTP 200 with a JSON body whose `ready` is `true` and whose `network`, `provider_wallet`, `deployment_id` and `peer_id` equal the qualifying offer's. The request SHALL use a transport the offer declares: its HTTPS endpoint first when it declares `https`, then the libp2p inference stream when it declares `libp2p` and no HTTPS attempt has confirmed readiness.

The class SHALL be decided only by these verifications and the provider's responses. Whether the registry had room to insert the qualifying offer SHALL NOT affect it. `funding_ready` SHALL NOT affect the classification, because serving readiness is deliberately separate from new-funding readiness. An offer's prices, minimum funding, free allowance and serving profile SHALL NOT affect the classification: a free offer with no serving profile qualifies exactly like a priced, qualified offer. An available sample SHALL NOT be described as evidence of qualification, speed, correct execution or honest metering.

A sample SHALL be **unavailable** when any of these steps fails for a reason on the provider's side of the path, unless it is skipped under *Observer resource bounds never decide a sample's class from provider behaviour* or unknown under *Observer degradation makes samples and probes unknown, never unavailable or failed*. Provider-side failures include:

- no allowed address or relayed path known for the peer;
- no planned connection authenticating as the peer before the deadline, whether each attempt failed, timed out, was refused or authenticated another peer;
- a relay refusing or cutting the observer's circuit;
- a failed protocol negotiation;
- a malformed, oversized or invalid manifest, or a manifest with no qualifying offer;
- an origin whose resolution fails, yields no answer or yields a disallowed address. The observer's resolver health is covered by its health check;
- a TLS failure, a redirect or a non-200 status;
- `ready` not `true`, a malformed or oversized readiness body, or missing or mismatched identity members;
- the sample deadline.

A sample MAY insert the qualifying offers it verifies into the catalog's verified registry under the registry's existing rules, as a side effect that never changes the sample's class. A sample SHALL NOT withdraw any registry entry.

#### Scenario: A ready free provider is available

- **GIVEN** a provider whose manifest contains an unexpired, valid free offer with `serving` `null` and a free allowance, naming the tracked wallet and peer ID
- **WHEN** its readiness over the offer's transport returns HTTP 200 with `ready: true`, `funding_ready: false` and identity fields equal to the offer's
- **THEN** the sample is available

#### Scenario: A full registry does not change the class

- **GIVEN** a verified registry holding 8,192 entries, with no entry and no room for the provider's namespace
- **WHEN** a ready provider with a qualifying offer is sampled
- **THEN** the sample is available
- **AND** the registry is unchanged

#### Scenario: A reachable provider with no current offer

- **GIVEN** a provider that drained, so its manifest contains only expired offers
- **WHEN** it is sampled while the observer is healthy
- **THEN** the sample is unavailable even though its readiness endpoint answers

#### Scenario: A provider that is not ready

- **WHEN** a provider's manifest contains a qualifying offer and its readiness returns HTTP 503 with `ready: false`
- **THEN** the sample is unavailable

#### Scenario: A sample withdraws nothing

- **WHEN** a sample stops reading a provider's manifest after the first page because that page contains a qualifying offer
- **THEN** no offer of that provider is withdrawn from the catalog's verified registry because of the sample

#### Scenario: A charging provider's return to free is detected by a sample

- **GIVEN** an identity whose pricing state is `charging`, and whose manifest's first page holds a priced qualifying offer and second page a qualifying free offer
- **WHEN** it is sampled
- **THEN** the sample reads the second page, is available, and sets the identity's pricing state to `free`

### Requirement: Observer degradation makes samples and probes unknown, never unavailable or failed

Every 30 seconds the observer SHALL run a health check. It SHALL be **healthy** only when all of the following succeed within ten seconds:

- the catalog's DHT service is running, and a Kademlia closest-peers query for one configured reference peer's ID, run through the catalog's DHT, completes with a response from at least one remote peer;
- at least one configured reference peer answers a libp2p ping from the observer's own node;
- the catalog's chain identity check succeeds; this is the same check its `/api/ready` route performs;
- at least one configured reference origin's hostname resolves within five seconds, through the resolver path the observer uses for provider origins, and that origin returns an HTTP response of any status to `GET /` through the outbound HTTPS client;
- since the previous check, the wall clock has advanced by the monotonic time elapsed, give or take 60 seconds;
- since the previous check, neither the observer's own node nor any probe identity's node has refused one of its dials because of a local limit or an exhausted local resource.

The observer SHALL be degraded from its start until a health check passes.

A sample SHALL be finalized at the first health check that completes after the sample ends. It SHALL be recorded as **unknown**, whatever its outcome, when the observer was not healthy at the last health check completed before the sample started, or at the finalizing check. A probe SHALL be finalized in the same way, and SHALL be recorded as unknown, whatever its outcome, when any health check from the last one completed before the probe started through its finalizing check found the observer not healthy. Degradation therefore removes good and bad outcomes alike and does not bias either figure. These samples and probes SHALL be discarded rather than recorded:

- one that started before, and was finalized after, a check that detected a clock discrepancy;
- one interrupted by observer shutdown;
- one not yet finalized at a restart.

Observer health SHALL be determined only by the conditions above. It SHALL NOT be inferred from tracked providers' responses, from the number or fraction of failing samples or probes, or from any value a provider controls. Unknown samples and probes SHALL be excluded from availability and probe success and SHALL be reported in their own counts. The observer's current state SHALL be reported in every launch-reputation response as `observer_status`.

#### Scenario: The observer loses its network

- **GIVEN** the observer's network connection fails, so no reference peer or reference origin answers
- **WHEN** samples and probes are taken during that period
- **THEN** they are recorded as unknown, not unavailable or failed
- **AND** responses report `observer_status` `degraded`

#### Scenario: Chain outage on the observer's side

- **WHEN** the catalog's chain identity check fails while a sample is taken, and the sampled provider's manifest has no current offer because it paused publication
- **THEN** the sample is recorded as unknown

#### Scenario: Resolver failure on the observer's side

- **WHEN** the observer's resolver stops answering, so no reference origin's hostname resolves, and a sample in that period fails to resolve a provider's origin
- **THEN** the sample is recorded as unknown

#### Scenario: Degradation does not inflate availability or probe success

- **GIVEN** the observer was degraded at the finalizing health check
- **WHEN** a sample in that period found the provider available and a probe in that period succeeded
- **THEN** both are recorded as unknown

#### Scenario: Nothing counts before the first passing check

- **WHEN** the relay has just started and a sample ends before any health check has passed
- **THEN** that sample is recorded as unknown

#### Scenario: A wall-clock jump is not recorded

- **WHEN** the wall clock jumps two hours forward between two health checks
- **THEN** the observer is degraded at the second check
- **AND** the samples and probes that span the jump are discarded

#### Scenario: Mass provider failure does not mark the observer degraded

- **GIVEN** the DHT check, a reference peer, a reference origin and the chain check all succeed
- **WHEN** most tracked providers fail their samples and probes in the same minute
- **THEN** those samples are unavailable and those probes are classified by their own outcomes
- **AND** the observer remains healthy

### Requirement: HTTPS observation follows the outbound-origin rules

For an offer that declares `https`, which is every offer before `p2p-provider-transport` is implemented, the readiness request SHALL be exactly `GET <origin>/readyz`. It SHALL carry no query, request body, cookie, `Authorization` header, account bearer, owner proof or other credential.

The observer SHALL send the request only after the offer's endpoint passes the outbound-origin validation that native consumers apply. It SHALL send it only through a client constructed for that origin, which SHALL:

- resolve the origin once under a five-second deadline;
- inspect at most 33 answers, and reject an empty answer set, more than 32 answers or any address that fails the public-address predicate;
- pin the accepted addresses, and connect only to the at most two of them planned for the sample, verified addresses first;
- disable environment proxies and redirect following;
- apply a ten-second connect deadline and normal certificate and hostname verification;
- read at most 16 KiB of the response.

A 3xx response SHALL be treated as not ready, and the observer SHALL NOT contact its `Location` target.

The observer SHALL keep at most one readiness request in flight per origin, and SHALL send at most one readiness request per origin per 60 seconds. It SHALL reuse an outcome, response or failure, obtained less than 60 seconds earlier for any identity whose qualifying offer names the same origin, including for a probe's readiness check. Because a response names one identity, a reused response confirms readiness only for the identity it names.

A probe's requests to an HTTPS provider SHALL go only to the offer's origin, through a client with the same resolution, pinning, proxy, redirect, connect-deadline and certificate rules, connecting only to the at most two verified addresses of that origin in the probe's plan. A probe SHALL read at most 8 MiB of an inference response, the native consumer's response bound; at most 96 KiB of a challenge, proposal, opening, credential, cancel or request-status response; and at most 16 KiB of an error response.

#### Scenario: An origin that resolves to a private address

- **WHEN** a qualifying offer's HTTPS origin resolves to a public address and an RFC 1918 address
- **THEN** the observer opens no connection to either address
- **AND** the sample is unavailable

#### Scenario: A redirecting readiness endpoint

- **WHEN** `GET <origin>/readyz` returns HTTP 302 with a `Location` naming another host
- **THEN** the observer does not connect to that host
- **AND** the sample is unavailable

#### Scenario: An origin with many addresses

- **WHEN** a qualifying offer's origin resolves to 32 public addresses
- **THEN** the readiness request makes at most two connection attempts

#### Scenario: Many identities name one origin

- **GIVEN** 50 tracked identities whose qualifying offers all name `https://provider.example`
- **WHEN** they are sampled within one minute
- **THEN** the observer sends at most one readiness request to that origin in that minute
- **AND** only an identity whose fields the response names can be available from it

### Requirement: libp2p observation follows the peer-to-peer transport rules

Manifest retrieval for every identity, readiness for an offer that declares `libp2p`, and every probe request to such an offer SHALL use only connections whose Noise-authenticated remote peer ID equals the identity's `peer_id`. The observer SHALL verify that before writing any manifest or stream request byte. A connection that authenticates a different peer ID SHALL be closed with nothing written, and the sample or probe SHALL continue with its remaining planned attempts. A relay, a hole-punch coordinator or any other peer reached at an address learned for that peer ID SHALL NOT be accepted as the provider.

The observer SHALL take candidate addresses only from its verified addresses and from what the catalog's DHT already knows (identify, Kademlia routing and provider records). Before planning, it SHALL filter them:

- every IP component SHALL pass the public-address predicate used for outbound HTTP origins, or be literal loopback under the explicit `development` flag;
- a learned address whose host is a DNS name SHALL NOT be planned;
- configured reference peers and the catalog's own relay are exempt.

It SHALL NOT dial or connect to any address, host or URL named in a manifest, readiness response, contract or inference response, or other provider response.

Every observer and probe dial SHALL be an explicit dial of planned addresses that lets no behaviour add addresses. Requests SHALL be written only on an established connection authenticated as the identity, never through a request that dials on its own. Until `p2p-provider-transport` is implemented, a sample or probe SHALL NOT start a Kademlia lookup. This filtering SHALL hold whether or not the swarm-wide filter of `p2p-provider-transport` is implemented yet.

A relayed path, once `p2p-provider-transport` is implemented, SHALL be a circuit through the planned relay. Through the catalog's own relay, the observer SHALL request a circuit as any consumer would, and that relay's admission for the destination SHALL decide it. The observer SHALL apply that capability's circuit classification:

- it MAY fetch a manifest over a coordination-sized circuit;
- it SHALL NOT open a `/chiral/inference/http/1` stream on a coordination-sized circuit, and a probe SHALL NOT send an inference request on a circuit whose remaining duration or byte budget fails that capability's check for the request;
- it SHALL attempt a hole-punch upgrade as that capability's ladder does.

When no connection that can carry the readiness stream exists before the deadline, libp2p readiness SHALL fail. A relay that refuses or cuts the observer's circuit SHALL make the relayed attempt fail, because consumers depend on the same path.

Readiness over libp2p SHALL be `GET /readyz` on `/chiral/inference/http/1` under the stream-client rules of `p2p-provider-transport`, with no redirect following, the ten-second connect deadline and a 16 KiB response bound. Probe requests over libp2p SHALL use the same stream-client rules with the 8 MiB response bound.

Until `p2p-provider-transport` is implemented, every valid offer declares an HTTPS endpoint. Until then the observer SHALL take readiness and send probe requests over HTTPS only, and providers reachable only over libp2p cannot be tracked or probed. Readiness and probes over libp2p, the connectivity ladder and relayed sampling and probing SHALL become available only with that implementation.

#### Scenario: Learned private addresses are not dialed

- **WHEN** the observer knows a tracked peer only by the addresses `192.168.1.5` and `127.0.0.1` learned through identify, without the `development` flag
- **THEN** it dials neither address
- **AND** the sample is unavailable because no allowed address is known

#### Scenario: Peer-ID mismatch

- **WHEN** a connection opened for a tracked peer authenticates a different peer ID
- **THEN** the observer closes it without writing a manifest, readiness or probe request
- **AND** the sample or probe continues with its remaining planned attempts

#### Scenario: Poisoned addresses do not displace a verified address

- **GIVEN** a tracked peer with a verified address, and eight learned addresses injected by other peers that answer as different peer IDs
- **WHEN** the peer is up and is sampled
- **THEN** its verified address is attempted first
- **AND** the sample is available

#### Scenario: Provider with a reservation on the catalog's relay

- **GIVEN** `p2p-provider-transport` is implemented and a libp2p-only provider holds a reservation on the catalog's own relay
- **WHEN** it is sampled while the relay's per-peer circuit quota for that provider is exhausted by consumers' circuits
- **THEN** the relay refuses the observer's circuit as it would refuse a consumer's
- **AND** the sample is unavailable

#### Scenario: Coordination-sized circuit

- **GIVEN** `p2p-provider-transport` is implemented, and the only authenticated path to a libp2p-only provider is a coordination-sized circuit
- **WHEN** the hole-punch upgrade fails before the deadline
- **THEN** the observer opens no `/chiral/inference/http/1` stream on that circuit and sends no probe request on it
- **AND** the sample is unavailable

#### Scenario: Before the peer-to-peer transport exists

- **GIVEN** `p2p-provider-transport` is not yet implemented
- **WHEN** the observer samples or probes a tracked identity
- **THEN** it fetches the manifest on `/chiral/inference/manifest/1`, and readiness and probe requests from the offer's HTTPS endpoint only
- **AND** a capture of its egress shows no connection attempt to a non-public address and no Kademlia lookup started by the sample or probe

### Requirement: Provider readiness names the serving identity

The provider public router's `GET /readyz` response, and therefore the same response on `/chiral/inference/http/1`, SHALL contain, in addition to `ready` and `funding_ready`, the members `network` (the provider's complete network identity), `provider_wallet`, `deployment_id` and `peer_id`, taken from the running deployment's configuration and its persisted DHT identity. Its status rule SHALL be unchanged: HTTP 200 when ready and 503 otherwise. It SHALL contain no secret, address, account or request data. `GET /healthz` SHALL be unchanged.

These members SHALL be treated as an identity binding, not as authentication or a signature: HTTPS certificate verification authenticates the origin, Noise authenticates the peer, and the provider-wallet signature on the offer binds the wallet to its endpoint and peer ID. A readiness response that lacks any of the members, or whose members differ from the qualifying offer, SHALL be treated by the observer as not ready.

#### Scenario: Readiness names its deployment

- **GIVEN** a provider deployment for wallet `W`, deployment ID `D` and peer ID `P` on network `N`
- **WHEN** a client requests `GET /readyz` over HTTPS or over the libp2p inference stream
- **THEN** the body contains `ready`, `funding_ready`, `network` `N`, `provider_wallet` `W`, `deployment_id` `D` and `peer_id` `P`
- **AND** the status is 200 exactly when `ready` is `true`

#### Scenario: Borrowed endpoint

- **GIVEN** wallet `A` signs an offer whose endpoint is the HTTPS origin of a different provider whose wallet is `B`
- **WHEN** `A`'s identity is sampled and that origin answers ready for wallet `B`
- **THEN** the sample is unavailable for `A`, and `A` cannot be probed at that origin

### Requirement: Probes run only while the provider's current verified offer is free

A *free offer* SHALL be a qualifying offer whose input and output token rates and minimum funding are all zero and that carries a signed free allowance (`free_allowance`, with its `tokens` and `lifetime_secs`, as `inference-commercial-terms` defines it after `specify-p2p-provider-transport`). Only free offers advertising `chat`, `completions` or `embeddings` SHALL be probed.

Each tracked identity SHALL have a *pricing state*, recorded with the wall-clock time of its last change:

- `unknown` when admitted, until one of the transitions below;
- `free` after any verification of the identity's own manifest, during discovery, a sample or a probe, finds a free offer;
- `charging` after a *complete* manifest read, meaning every page from offset 0 until the manifest's end as discovery and probes read it, finds at least one qualifying offer and no free offer.

Any other outcome, including an unavailable sample, an unreached probe, or a read that finds no qualifying offer, SHALL leave the pricing state unchanged, so going offline, draining or pausing publication never ends a free period.

A wallet SHALL be *free now* when at least one of its tracked identities is `free`. A wallet's *free period* SHALL begin when the wallet becomes free now, and SHALL end at the first pricing-state change after which no tracked identity of the wallet is `free` and at least one is `charging`. Eviction or inactivity of an identity SHALL NOT by itself end a free period.

The observer SHALL schedule probes only for identities whose pricing state is `free`. Each probe SHALL read the identity's complete manifest over the probe's own authenticated connection and SHALL select uniformly at random one of the free offers it finds there. When that read finds qualifying offers but no free offer, the identity becomes `charging`, the probe SHALL end without sending any contract or inference request, and it SHALL NOT be recorded in any class. Probes therefore stop while a provider charges, and resume when it returns to free. The results of earlier free periods SHALL remain reported, as *Probe results, the service score and the launch criteria are computed exactly* specifies.

#### Scenario: A new free share is probed

- **GIVEN** a provider whose first verified offer is a free offer with the allowance `{"lifetime_secs":86400,"tokens":20000}`
- **WHEN** the catalog verifies it during discovery
- **THEN** the identity's pricing state becomes `free`, its wallet's free period begins, and probes are scheduled for it

#### Scenario: Charging stops probes

- **GIVEN** a free identity whose provider publishes a new revision with nonzero rates and withdraws its free offer
- **WHEN** a probe's complete manifest read finds only that priced offer
- **THEN** the probe sends no contract or inference request and is not recorded
- **AND** the identity becomes `charging`, the wallet's free period ends, and no further probe is scheduled for it

#### Scenario: Going offline does not end a free period

- **GIVEN** a free identity
- **WHEN** it is unreachable for six hours, so its samples are unavailable and its probes unreached
- **THEN** its pricing state stays `free` and its wallet's free period continues

#### Scenario: A step down to free restarts probes

- **GIVEN** a `charging` identity whose provider's automatic pricing steps its price down to free
- **WHEN** a sample or discovery verifies the new free offer
- **THEN** the identity becomes `free`, a new wallet free period begins, and probes are scheduled again

### Requirement: Probe accounts use fresh, unlinkable identities and stay within the free allowance

A probe SHALL use a *probe account*: a free account opened from the selected free offer by a *probe identity*. A probe identity SHALL consist of a secp256k1 wallet key and a libp2p key pair, both generated from the operating system's cryptographically secure random source for that one account. Probe identity keys SHALL be held in memory only, SHALL NOT be persisted, logged, exported or returned by any route, SHALL be erased when the account is discarded, and SHALL NOT be reused for another account, another offer, another identity or another provider, or for availability samples. A probe identity's wallet holds no CFX and SHALL sign nothing except owner proofs for its own account's control operations, each only after the observer has reconstructed and verified the exact challenge payload for the intended request. It SHALL NOT sign or broadcast a transaction and SHALL NOT submit feedback.

To open a probe account, the observer SHALL request a proposal for the selected offer with `funding_wei` `"0"` and the probe identity's wallet as consumer; SHALL verify the returned quote's provider signature and that its terms equal the offer's, including zero rates, zero funding and an identical free allowance; and SHALL open the account with the probe identity's owner proof and no payment, keeping the returned account credential in memory only. Every probe request SHALL carry only that account's credential and the request headers that the shipped native consumer sends. It SHALL carry no header, cookie or other value that identifies the observer, the catalog or another probe account. Each probe identity's libp2p key SHALL be used for every libp2p connection of its account's probes and for no other connection.

A probe account SHALL be used for at most 12 probes and for at most `min(7200, floor(lifetime_secs / 2))` seconds after it was opened. The observer SHALL send a request on it only when the account's *recorded usage* plus the request's *worst case* is at most the allowance's `tokens`. Recorded usage is the sum of the `total_tokens` that the account's earlier responses reported, plus the worst case of each earlier request whose usage was not reported. A request's worst case is its maximum output tokens plus the expected prompt-token count from the billing check when one is available, or otherwise the prompt's UTF-8 byte length plus 256 (plus 1,024 for the limit kind, whose maximum output leaves no room for an underestimate). Because the provider reserves the exact prepared prompt count, an underestimated prompt can draw a `free_allowance_exhausted` refusal, which is classified as *A probe succeeds only when the provider serves it correctly* states. A probe account SHALL be discarded when any of these limits is reached, when its recorded usage reaches `floor(tokens / 2)`, when the offer it was opened from is no longer the identity's current free offer for that model, or when the observer restarts. A streaming, non-streaming or cancellation request has a worst case of at most 512 tokens (a prompt of at most 128 bytes plus 256, plus at most 128 output tokens), and every free allowance holds at least 1,000 tokens, so such a request always fits a fresh account.

#### Scenario: Accounts are not linked across providers

- **GIVEN** two free providers probed over 24 hours
- **WHEN** each records the consumer wallets and peer IDs of every account opened by the observer
- **THEN** no wallet or peer ID appears at both providers, and no wallet or peer ID is used for more than one account

#### Scenario: A quote with different terms

- **WHEN** a probe's proposal returns a quote whose free allowance differs from the selected offer's
- **THEN** the observer does not open the account and the probe is failed

#### Scenario: The allowance is never exceeded

- **GIVEN** a probe account opened from a free offer whose allowance is 1,000 tokens, with 600 tokens of recorded usage
- **WHEN** the next probe is due
- **THEN** the account is discarded because its recorded usage has reached half the allowance, and the probe opens a fresh account

#### Scenario: Keys do not survive a restart

- **WHEN** the relay restarts
- **THEN** every probe account is discarded and no probe identity key can be recovered from the store, the logs or any route

### Requirement: Probes follow a memoryless schedule with bounded rate and concurrency

The observer SHALL draw, for each identity whose pricing state is `free`, a sequence of probe times from the operating system's cryptographically secure random source. The first drawn time SHALL be uniformly random between 0 and 600 seconds after the identity became `free`, or after the observer starts for an identity that was already `free`. Each later drawn time SHALL follow the previous drawn time by an exponentially distributed interval with a mean of 600 seconds, with no minimum spacing. No request, provider action or response SHALL trigger, advance or postpone a drawn time.

An identity SHALL have at most 12 probe starts per UTC hour; a drawn time that would be the 13th SHALL be discarded and not replaced, decided on the count alone. An identity SHALL have at most one probe in progress and at most one drawn time waiting. A drawn time reached while its previous probe is in progress SHALL wait for it; a drawn time reached while another drawn time of the same identity is already waiting SHALL be discarded and not replaced, decided on that state alone, so the waiting state is bounded by the number of identities. Each probe's start SHALL be committed to the store before its first outbound attempt. After a restart, missed draws SHALL NOT be caught up, and the persisted start counts SHALL keep the hourly cap.

At most 64 probes SHALL be in progress. A probe whose drawn time arrives while 64 are in progress SHALL wait in order of drawn time; waiting SHALL NOT change its class. Each probe SHALL complete within 60 seconds of its actual start, covering every connection attempt, the manifest read, the readiness check, account opening and every request of its kind, with the ten-second connect deadline, the four-second manifest page timeout, the ten-second readiness deadline and a 30-second deadline for each response's status line and headers. A probe still incomplete at its deadline SHALL be ended and classified as if its pending step had failed.

When a probe starts, before contacting anything, the observer SHALL fix its plan, which SHALL NOT grow afterwards and SHALL contain only:

- at most three verified direct libp2p addresses of the identity;
- at most one relayed path, under the relay rules and per-relay tokens of *Observer resource bounds never decide a sample's class from provider behaviour*;
- for an offer that declares `https`, at most two verified addresses of its origin.

A probe SHALL NOT plan an unverified address. When the plan is empty although the identity is `free`, the probe SHALL be recorded as **skipped** without contacting anything. Each probe identity's libp2p node SHALL hold at most two established connections and SHALL accept no inbound connection except the hole-punch connection of its own probe in progress; every connection and circuit a probe opens SHALL be closed when the probe ends.

#### Scenario: Probe times are unpredictable

- **GIVEN** a free identity whose probe has just started
- **WHEN** its next probe is drawn
- **THEN** the interval from the previous drawn time is exponentially distributed with a mean of 600 seconds
- **AND** nothing the provider, a consumer or a reader does changes it

#### Scenario: The probe cap survives a restart

- **GIVEN** a free identity with 12 committed probe starts in the current UTC hour
- **WHEN** the relay restarts within that hour
- **THEN** its next probe starts no earlier than the next UTC hour

#### Scenario: Probes never reach unverified addresses

- **GIVEN** a free identity that has never been available, so it has no verified address
- **WHEN** its probe starts
- **THEN** no connection is attempted and the probe is recorded as skipped

#### Scenario: A probe backlog is waited out

- **GIVEN** 64 probes in progress
- **WHEN** another free identity's probe time arrives
- **THEN** that probe waits, is taken when a slot frees, and is classified by its own outcome
- **AND** a further drawn time of that identity reached while it waits is discarded and not replaced

### Requirement: Probe requests use a fixed public corpus and fixed kinds

Probe prompts SHALL come from one versioned probe corpus file committed to the repository: exactly 64 prompts, each 16 to 128 bytes of UTF-8, with no control characters other than line feed, and with no text resembling a special or control token of common chat templates (for example `<|`, `|>`, `<s>`, `</s>`, `[INST]`, `<start_of_turn>` or `<bos>`). The corpus SHALL include non-ASCII text. Its SHA-256 SHALL be reported in every record as `probe_corpus_sha256`. Each probe SHALL draw one prompt uniformly at random from the corpus with the cryptographically secure random source, except that a probe sending a deferred probe request, as *A probe succeeds only when the provider serves it correctly* defines, SHALL reuse that request's prompt.

The endpoint SHALL be `completions` when the selected offer advertises it; otherwise `chat`, with exactly one `user` message whose content is the prompt; otherwise, for an offer advertising only `embeddings`, `embeddings` with the prompt as its single input.

For `completions` and `chat`, the observer SHALL choose one probe kind with the cryptographically secure random source, unless it is sending a deferred probe request:

- **streaming**, with probability 1/2: `stream: true`, `stream_options: {"include_usage": true}` and a maximum output of `min(16, max_output_tokens)`;
- **non-streaming**, with probability 1/4: `stream: false` and a maximum output of `min(16, max_output_tokens)`;
- **cancellation**, with probability 1/8: streaming with a maximum output of `min(128, max_output_tokens)`, chosen only when `max_output_tokens` is at least 64 and replaced by the streaming kind otherwise;
- **limit**, with probability 1/8: `stream: false` and a maximum output of `max_output_tokens + 1`.

A kind whose worst case does not fit the probe account under *Probe accounts use fresh, unlinkable identities and stay within the free allowance* SHALL be replaced by the streaming kind; when that does not fit either, the account SHALL be discarded and a fresh one opened. Every generation request SHALL set `temperature` to 0, SHALL carry a fresh random `Idempotency-Key`, and SHALL contain no tools, response format or other parameter. An embeddings probe SHALL send exactly the prompt as input with no other parameter. A probe SHALL send exactly one inference request, plus, for the cancellation kind, one cancel request and at most four request-status reads.

#### Scenario: A completions-capable offer is probed on completions

- **WHEN** a probe selects a free offer advertising `chat` and `completions`
- **THEN** its inference request goes to `/v1/completions` with the drawn corpus prompt

#### Scenario: A chat-only offer is probed on chat

- **WHEN** a probe selects a free offer advertising only `chat`
- **THEN** its inference request goes to `/v1/chat/completions` with one `user` message holding the drawn prompt

#### Scenario: An embeddings-only offer is probed on embeddings

- **GIVEN** a free offer advertising only `embeddings`
- **WHEN** a probe selects it
- **THEN** its inference request goes to `/v1/embeddings` with the drawn corpus prompt as its single input and no other parameter
- **AND** no generation kind is chosen

#### Scenario: Corpus identity is reported

- **WHEN** any record is returned
- **THEN** it carries the SHA-256 of the corpus file the observer used

### Requirement: A probe succeeds only when the provider serves it correctly

A recorded probe SHALL have exactly one class:

- **skipped**: its plan was empty, as *Probes follow a memoryless schedule with bounded rate and concurrency* states;
- **unreached**: no planned connection authenticated as the identity before the deadline, the complete manifest read failed or found no qualifying offer, or the readiness check of *A sample is available only when the provider's own manifest and readiness verify* did not report the identity ready. Availability already records these failures, so unreached probes SHALL be excluded from probe success and reported in their own count;
- **busy**: a proposal, account opening or the inference request was refused before dispatch with a retryable busy response: HTTP 429 with error code `provider_saturated`, `model_saturated` or `rate_limit`, or the qualification-pause response that `desktop-share-online` defines for a provider's automatic qualification run (HTTP 503 with error code `qualification_in_progress`); or the inference request was refused with HTTP 402 `free_allowance_exhausted` on a probe account that had already sent an earlier inference request, in which case the account SHALL be discarded. A busy probe SHALL NOT be retried. Busy probes SHALL be excluded from probe success and reported in their own count;
- **unknown**: as *Observer degradation makes samples and probes unknown, never unavailable or failed* states;
- **failed**: any other failure after readiness was confirmed, including a failed or malformed proposal, quote, opening or credential; quote terms that differ from the offer; an error status other than busy for a request the kind expects to succeed, including HTTP 402 `free_allowance_expired` (the observer never uses an account beyond half its lifetime) and HTTP 402 `free_allowance_exhausted` on an account's first inference request; a body that does not meet the kind's criteria; and the probe deadline;
- **succeeded**: the kind's criteria below are met.

A provider sees a probe's kind and prompt only in the inference request, and could otherwise refuse the kinds it would fail as busy. Therefore, when an inference request is refused as busy, the observer SHALL record that probe's endpoint, kind and corpus prompt index as the identity's *deferred probe request*, persisted with the identity row, and the identity's next probe that reaches its inference request SHALL send that deferred request, on its own or a fresh account under the account limits, instead of drawing a new kind and prompt. The deferred request SHALL be cleared only when a probe that sent it is recorded succeeded or failed, or when the identity's selected free offer no longer admits it (its endpoint is no longer advertised, the cancellation kind's `max_output_tokens` condition no longer holds, or its worst case does not fit a fresh account under the offer's allowance), in which case the next probe draws normally. Refusing a kind as busy therefore only postpones that kind and stops the identity's counted probes until it is served, and a busy refusal at a proposal or opening, before the kind is visible, leaves any deferred request unchanged.

Every succeeded generation probe, other than the limit kind, SHALL have returned usage with non-negative integer `prompt_tokens`, `completion_tokens` and `total_tokens`, where `total_tokens` equals the sum of the other two and `completion_tokens` is at most the requested maximum output; and SHALL have returned an `x-request-id` header. The kind criteria SHALL be:

- **streaming**: HTTP 200 with content type `text/event-stream`; every event a well-formed chunk object of the endpoint, followed by exactly one final chunk carrying `usage`, then `data: [DONE]`, with the stream ending before the deadline;
- **non-streaming**: HTTP 200 with a well-formed response object of the endpoint holding exactly one choice whose `finish_reason` is `stop` or `length`, and `usage`;
- **cancellation**: after the first content event, the observer sends `POST /v1/requests/:id/cancel` with the account credential. The probe succeeds when the cancel request returns HTTP 200 or 202, the stream ends within 10 seconds after that response, `GET /v1/requests/:id` reports a terminal state within 10 seconds after that response, and any usage reported shows `completion_tokens` below the requested maximum. When the stream ends before the cancel request is sent, the probe SHALL be classified by the streaming criteria;
- **limit**: HTTP 400 with an error object and no generated content;
- **embeddings**: HTTP 200 with a well-formed response holding exactly one non-empty embedding of finite numbers, and `usage` whose `prompt_tokens` equals `total_tokens`.

For a succeeded streaming or cancellation probe, the observer SHALL record the time from sending the inference request to its first content event in one of nine buckets with upper bounds of 250, 500, 1,000, 2,000, 4,000, 8,000, 16,000 and 32,000 milliseconds and a final open bucket. Latency SHALL be recorded and shown and SHALL NOT affect any class, score or criterion, because the observer's network path is not the consumer's.

Generated content SHALL be treated as untrusted data. It SHALL be checked only for the structure above, and SHALL NOT be compared with reference outputs, stored, logged or displayed. A succeeded probe SHALL NOT be described as evidence of which model ran, of qualification or speed, or of correct execution.

#### Scenario: A well-formed streaming probe succeeds

- **WHEN** a streaming probe receives chunks, a final chunk with consistent usage and `[DONE]` within the deadline
- **THEN** the probe succeeds and its time to first content is recorded in its bucket

#### Scenario: Missing usage fails a probe

- **WHEN** a non-streaming probe receives a well-formed completion without `usage`
- **THEN** the probe is failed

#### Scenario: Busy during automatic qualification

- **GIVEN** a free provider running its automatic qualification, which refuses free admission with HTTP 503 `qualification_in_progress`
- **WHEN** a probe's inference request is refused with that response
- **THEN** the probe is busy, is not retried, and does not enter probe success
- **AND** the identity's next probe that reaches its inference request sends the same endpoint, kind and prompt

#### Scenario: Refusing hard kinds as busy does not raise probe success

- **GIVEN** a free provider that answers every cancellation and limit probe with HTTP 429 `rate_limit` and serves every other kind correctly
- **WHEN** a cancellation probe is refused
- **THEN** that probe is busy and the cancellation request becomes the identity's deferred probe request
- **AND** every later probe of the identity that reaches its inference request sends that cancellation request, so no further probe of the identity is counted until the provider serves it

#### Scenario: Allowance refusals

- **WHEN** a probe account's first inference request is refused with HTTP 402 `free_allowance_exhausted`, or any inference request with HTTP 402 `free_allowance_expired`
- **THEN** the probe is failed
- **AND** a `free_allowance_exhausted` refusal of a later request on an account is busy, the account is discarded and the request is deferred to a fresh account

#### Scenario: Cancellation ignored

- **WHEN** a cancellation probe's cancel request returns 202 but the stream continues for 20 seconds
- **THEN** the probe is failed

#### Scenario: A limit violation

- **WHEN** a limit probe's request with a maximum output of `max_output_tokens + 1` is accepted and generates content
- **THEN** the probe is failed

#### Scenario: An unreachable provider is not double-counted

- **WHEN** a probe cannot authenticate any planned connection to a free identity while the observer is healthy
- **THEN** the probe is unreached and does not enter probe success, while that period's samples count against availability

### Requirement: Billing honesty is checked against the declared tokenizer

For every succeeded probe whose response reported `prompt_tokens`, the catalog SHALL compute the *expected* prompt-token count of the exact request it sent and record one billing outcome: `matched` when the reported count equals it, `mismatched` when it differs, or `unchecked` when the catalog could not compute it.

The expected count SHALL be the number of tokens that the project's pinned llama.cpp runtime, the identity that `tokenizer_runtime` verifies, produces for that request under the offer's declared model:

- for `completions`, the prompt tokenized as the pinned runtime tokenizes a `/v1/completions` prompt;
- for `chat`, the messages rendered with the chat template, from the model's GGUF metadata, whose normalized source has the SHA-256 `chat_template_sha256` (line endings normalized to line feed and one trailing line feed removed, as the artifact contract normalizes it), with the generation prompt appended, and tokenized as the pinned runtime tokenizes a rendered `/v1/chat/completions` prompt;
- for `embeddings`, the input tokenized as the pinned runtime tokenizes a `/v1/embeddings` input.

The catalog SHALL obtain the declared tokenizer and templates as follows:

- the offer's `model.id` SHALL have the form `<namespace>/<repository>/<path>`, with `<namespace>/<repository>` a valid Hugging Face repository ID and `<path>` a relative path ending in `.gguf`, as the artifact contract produces it, and `model.tokenizer_revision` SHALL be a 40-character lowercase hexadecimal commit; otherwise the outcome SHALL be `unchecked` with reason `tokenizer_unavailable`;
- the catalog SHALL request `https://huggingface.co/<namespace>/<repository>/resolve/<tokenizer_revision>/<path>` with a range covering the first 33,554,432 bytes (32 MiB), carrying no credential or token, and SHALL read at most 32 MiB of the response whether or not the range is honored;
- the request SHALL follow the native Hugging Face client rules of `desktop-model-acquisition`: HTTPS only; each host resolved once under a five-second deadline with every answer public and pinned; no environment proxies; and redirects followed hop by hop, at most five, only to `https` URLs on port 443 at `huggingface.co` or a compiled allowlisted Hugging Face download host;
- the bytes SHALL be parsed with a bounded GGUF reader that caps declared lengths and counts before allocating. The header, metadata section and tensor-information section SHALL end within the bytes read; otherwise the outcome SHALL be `unchecked` with reason `tokenizer_too_large`;
- when no embedded template has the declared SHA-256, or rendering fails, a `chat` outcome SHALL be `unchecked` with reason `template_unavailable`, never `mismatched`;
- a parsed prefix SHALL be cached by repository, revision and path, with at most 64 entries and 1 GiB in total, least recently used first evicted; a failed fetch SHALL be remembered for six hours and not retried sooner; at most one fetch SHALL be in flight and at most 12 SHALL start per hour; a check that cannot fetch under these bounds SHALL be `unchecked` with reason `tokenizer_unavailable`;
- a fetch SHALL be started only by a billing check, never by a catalog read.

The catalog does not hold the whole artifact, so it SHALL rely on Hugging Face's TLS origin and the immutable commit for the prefix it reads, and SHALL state that trust assumption in operator documentation.

Tokenization and template rendering SHALL run in a separate process of the pinned runtime, launched only after its inventory verifies, with no network access, a minimal environment, no access to the relay's keys, configuration secrets or data directory beyond its own input, a ten-second deadline per check, and at most two processes at a time. A process that fails or exceeds its deadline SHALL make the outcome `unchecked` with reason `tokenizer_failed`. At observer start and after any tokenizer process failure, the catalog SHALL run a self-test against a fixed set of golden prompt-token counts that the pinned runtime produced for fixed public GGUF artifacts at pinned commits. While the self-test is failing, every outcome SHALL be `unchecked` with reason `self_test_failed`, and `/api/health` SHALL report it.

A `mismatched` outcome SHALL raise a **billing mismatch flag** on the provider wallet: `flagged_at` is the probe's finalization time, and the flag is active until `flagged_at` plus 1,209,600 seconds (14 days). A later mismatch SHALL replace the flag's evidence and extend its end. A flag SHALL be cleared only by expiry, never by later matched outcomes, a new peer ID or a price change. The flag SHALL carry its evidence: the model ID, revision, `tokenizer_revision` and `chat_template_sha256`, the endpoint, the corpus prompt's index, `probe_corpus_sha256`, the expected and reported prompt-token counts, and the tokenizer runtime identity. With it, anyone can reproduce the count.

An `unchecked` outcome SHALL NOT be treated or displayed as a mismatch. A billing check SHALL NOT be described as evidence of which model ran, or of the correctness of completion-token counts, which it does not check.

#### Scenario: Matching counts

- **WHEN** a succeeded completions probe reports `prompt_tokens` equal to the count the pinned runtime produces for that prompt under the declared revision
- **THEN** its billing outcome is `matched`

#### Scenario: Overstated prompt tokens raise the flag

- **WHEN** a succeeded chat probe reports `prompt_tokens` 61 where the declared template and tokenizer give 41
- **THEN** its billing outcome is `mismatched`
- **AND** the provider wallet carries a billing mismatch flag for 14 days, with the prompt index, the expected count 41 and the reported count 61

#### Scenario: A template the catalog cannot find

- **WHEN** a chat-only offer's `chat_template_sha256` matches none of the templates embedded in the declared GGUF's metadata
- **THEN** the billing outcome is `unchecked` with reason `template_unavailable`, and no flag is raised

#### Scenario: A model ID that names no Hugging Face artifact

- **WHEN** a free offer's `model.id` is `my-model`
- **THEN** its probes' billing outcomes are `unchecked` with reason `tokenizer_unavailable`, and no download is attempted

#### Scenario: A redirect off the allowlist

- **WHEN** the tokenizer request redirects to a host that is not `huggingface.co` or on the allowlist
- **THEN** the catalog does not connect to that host, the outcome is `unchecked`, and the failure is remembered for six hours

#### Scenario: A broken tokenizer pipeline raises no flag

- **GIVEN** the self-test fails after a tokenizer runtime upgrade
- **WHEN** probes report any prompt-token counts
- **THEN** every billing outcome is `unchecked` with reason `self_test_failed`, and no flag is raised

### Requirement: Availability results are computed exactly from retained samples

The catalog SHALL count each identity's availability samples by class (`available`, `unavailable`, `unknown`, `skipped`) in the UTC hour of each sample's start. The window SHALL be the current UTC hour and the 335 UTC hours before it (336 hours in total). The catalog SHALL compute results using integer arithmetic only, with `computed_at` being the catalog's wall-clock time when the record is computed.

For one identity, the **peer section** SHALL contain:

- `tracked`; `untracked_reason`; and `untracked_since`, the time the identity stopped being tracked or was first refused, or `null`;
- `identity_since`: the catalog's own wall-clock time at which it first admitted the identity, or `null`;
- `samples`: the identity's four class counts in the window;
- `availability_14d`: `floor(10000 × available / (available + unavailable))`, in basis points, or `null` when `available + unavailable` is zero;
- `counted_since`: the start of the oldest UTC hour in the window that holds an available or unavailable sample, or `null` when there is none;
- `expected_samples`: `floor(S / 300)`. `S` is the number of seconds from `counted_since` to `computed_at`, or to `untracked_since` while the identity is untracked with reason `inactive`. It is zero when `counted_since` is `null`;
- `coverage_bps`: `min(10000, floor(10000 × (available + unavailable) / expected_samples))`, or `null` when `expected_samples` is zero;
- `evicted_seconds`: the seconds between `counted_since` and `computed_at` during which the identity was evicted;
- `observed_at`: the finalization time of the identity's most recent finalized sample of any class, or `null`.

For the identity's wallet, the **wallet section** SHALL aggregate every identity row of that `provider_wallet` in the network, whether tracked or retained:

- `identities`: the number of those rows;
- `samples`: the sums of their counts;
- `availability_14d` and `coverage_bps`: computed from those sums by the same formulas;
- `counted_since`: the oldest of their `counted_since`;
- `expected_samples`: the sum of their `expected_samples`;
- `observed_at`: the latest of their `observed_at`.

The record SHALL also contain `sharing_since`: the catalog's own wall-clock time, in UNIX seconds, at which it first verified an offer from that wallet in its network under the identity rules, never an offer's `issued_at` or any other provider-supplied time. It SHALL be `null` when the catalog holds no such time.

The wallet section SHALL be the headline. A provider SHALL NOT leave its wallet's figure by changing its peer ID. The catalog SHALL NOT substitute zero, one hundred percent, a neutral value or any estimate for a missing figure. Availability SHALL NOT be weighted, smoothed, combined with payment-gated reputation or adjusted by the offer's prices.

#### Scenario: No counted samples is no figure

- **GIVEN** an identity whose samples in the window are all unknown or skipped
- **WHEN** its record is computed
- **THEN** `availability_14d`, `counted_since` and `coverage_bps` are `null`, `expected_samples` is zero, and the counts show only unknown and skipped samples

#### Scenario: Exact basis points

- **GIVEN** 2,999 available, 1 unavailable, 40 unknown and 7 skipped samples in the window
- **WHEN** the record is computed
- **THEN** `availability_14d` is 9,996
- **AND** the unknown and skipped samples do not enter the figure

#### Scenario: Coverage

- **GIVEN** an identity with 300 available and unavailable samples whose `counted_since` is 48 hours before `computed_at`
- **WHEN** the record is computed
- **THEN** `expected_samples` is 576 and `coverage_bps` is 5,208

#### Scenario: A new peer of an established wallet

- **GIVEN** a wallet whose first verified offer the catalog saw 90 days ago from peer `P1`, and whose identity `P1` has 100 available and 300 unavailable samples in the window
- **WHEN** the same wallet starts sharing from a new peer `P2`, which then has 50 available samples
- **THEN** the record for `P2` reports the 90-day-old `sharing_since`
- **AND** its wallet section counts 150 available and 300 unavailable samples, with `availability_14d` 3,333
- **AND** its peer section reports `P2`'s own `identity_since` and samples only

### Requirement: Probe results, the service score and the launch criteria are computed exactly

The catalog SHALL count each identity's recorded probes by class (`succeeded`, `failed`, `busy`, `unreached`, `unknown`, `skipped`), its billing outcomes (`matched`, `mismatched`, `unchecked`) and its time-to-first-content buckets, in the UTC hour of each probe's start, over the same 336-hour window, using integer arithmetic only. *Counted probes* are succeeded plus failed probes.

The **peer section** SHALL add `pricing` (the identity's pricing state) and `pricing_since`; `probes` (its class counts in the window); `probe_success_14d`, which is `floor(10000 × succeeded / (succeeded + failed))` or `null` when there are no counted probes; and `latest_probe_at`, the finalization time of the identity's most recent counted probe, or `null`.

The **wallet section** SHALL add:

- `probes`, `billing_checks` and `first_token_ms_buckets` (nine counts): the sums over the wallet's identity rows in the window;
- `probe_success_14d` from those sums by the same formula, and `latest_probe_at`, the latest of the identities' values, kept beyond the window for as long as the wallet row exists;
- `free_now` and `free_since`, the start of the current free period, or `null`;
- `free_periods`: the wallet's completed free periods, newest first, at most 16, each with `started_at`, `ended_at`, its probe class counts and `launch_criteria_met_at_end`. That last member is the value of `launch_criteria.met` computed at `ended_at` from the wallet's state immediately before the period ended, that is, with `probe_basis` `window` and the window's counted probes;
- `probe_basis`, `probe_count` and `probe_success_bps`:
  - when `free_now` is `true`: `probe_basis` is `window`, with the window's counted probes and `probe_success_14d`;
  - when `free_now` is `false` and `free_periods` is not empty: `probe_basis` is `free_periods`. Walking `free_periods` from the newest, the catalog sums succeeded and failed probes until the sum of counted probes is at least 100 or the list ends; `probe_count` is that sum, and `probe_success_bps` is `floor(10000 × succeeded / probe_count)`, or `null` when `probe_count` is zero;
  - otherwise all three are `null`;
- `service_score_bps`: `floor(availability_14d × probe_success_bps / 10000)` using the wallet's `availability_14d`, or `null` when either is `null`. The *service score* shown to people is this value divided by 100;
- `billing_mismatch`: the active flag with its `flagged_at`, `until` and evidence, or `null` when no flag is active at `computed_at`;
- `launch_criteria`: `{met, unmet}`, where `unmet` lists, in this order, each failed criterion and `met` is `true` exactly when `unmet` is empty:
  - `observed_under_24h`: the wallet's `counted_since` is `null` or less than 86,400 seconds before `computed_at`;
  - `probes_under_100`: `probe_count` is `null` or below 100;
  - `availability_below_95`: the wallet's `availability_14d` is `null` or below 9,500;
  - `probe_success_below_98`: `probe_success_bps` is `null` or below 9,800;
  - `billing_mismatch`: a billing mismatch flag is active.

When `probe_basis` is `free_periods`, the walk from the newest free period until at least 100 counted probes SHALL be applied as the book's rule, so that a short free period cannot by itself replace a longer history.

Billing outcomes that are `unchecked` SHALL NOT fail any launch criterion. The launch criteria SHALL NOT require any minimum number or share of `matched` outcomes or of checked probes; only an active billing mismatch flag fails the billing criterion.

The book's *ready to charge* SHALL be read as `launch_criteria.met` together with a valid qualification. For a wallet whose offers advertise only `embeddings`, the same `launch_criteria` SHALL apply, computed over its embeddings probes and their input-token billing outcomes, and the readiness that the provider's automatic pricing reads SHALL be `launch_criteria.met` alone, without a qualification term, because embeddings cannot qualify a generation serving profile. The record SHALL NOT add a separate member for this case. The catalog SHALL NOT assess qualification: the provider's own automatic pricing checks its qualification evidence, and consumers check the signed offer's serving profile. No record member SHALL claim that a provider is qualified or ready to charge.

The catalog SHALL NOT substitute a value for a missing figure, SHALL NOT weight or smooth probe success, and SHALL NOT combine any of these values with payment-gated reputation. Unknown, unreached, busy and skipped probes SHALL appear in their counts and SHALL NOT enter probe success.

#### Scenario: Service score while free

- **GIVEN** a free wallet with `availability_14d` 9,800 and 196 succeeded and 4 failed probes in the window
- **WHEN** its record is computed
- **THEN** `probe_basis` is `window`, `probe_success_bps` is 9,800 and `service_score_bps` is 9,604

#### Scenario: Launch criteria met

- **GIVEN** a free wallet whose `counted_since` is 30 hours before `computed_at`, with `availability_14d` 9,700, 140 counted probes with `probe_success_bps` 9,857 and no active billing mismatch flag
- **WHEN** its record is computed
- **THEN** `launch_criteria` is `{"met": true, "unmet": []}`

#### Scenario: Launch criteria unmet

- **GIVEN** a free wallet observed for 20 hours with 80 counted probes, all succeeded, and `availability_14d` 10,000
- **WHEN** its record is computed
- **THEN** `launch_criteria.unmet` is `["observed_under_24h", "probes_under_100"]`

#### Scenario: Charging uses the latest free periods

- **GIVEN** a charging wallet whose newest completed free period has 3 counted probes, all succeeded, and whose previous one has 150 counted probes with 135 succeeded
- **WHEN** its record is computed
- **THEN** `probe_basis` is `free_periods`, `probe_count` is 153 and `probe_success_bps` is 9,019
- **AND** a short free period cannot by itself replace the longer history

#### Scenario: A billing mismatch blocks the launch criteria

- **GIVEN** a wallet that otherwise meets every criterion and has a billing mismatch flag flagged three days ago
- **WHEN** its record is computed
- **THEN** `launch_criteria.met` is `false` and `unmet` is `["billing_mismatch"]`

#### Scenario: Unchecked billing does not block the launch criteria

- **GIVEN** a free wallet that meets every other criterion and whose 140 succeeded probes all have billing outcome `unchecked`, with none `matched` or `mismatched`
- **WHEN** its record is computed
- **THEN** `launch_criteria` is `{"met": true, "unmet": []}`

#### Scenario: An embeddings share's launch criteria

- **GIVEN** a free wallet whose offers advertise only `embeddings`, observed for 30 hours, with `availability_14d` 9,700 and 120 counted embeddings probes of which 119 succeeded, whose billing outcomes are input-token checks with none `mismatched`
- **WHEN** its record is computed
- **THEN** `probe_success_bps` is 9,916 and `launch_criteria` is `{"met": true, "unmet": []}`
- **AND** that value alone is the readiness the provider's automatic pricing reads, with no qualification term

#### Scenario: No probes is no score

- **GIVEN** a wallet that has always charged and has availability samples but no recorded probe
- **WHEN** its record is computed
- **THEN** `probe_basis`, `probe_success_bps` and `service_score_bps` are `null`

### Requirement: Launch-reputation storage is durable, network-scoped, bounded and retention-limited

The catalog SHALL keep launch reputation in a durable SQLite store under its network-scoped data directory. The store SHALL be bound to the catalog's complete network identity and SHALL refuse to open for a different network. When the `launch_reputation` object is present and the store cannot be opened, is corrupt or belongs to another network, the relay SHALL refuse to start, and SHALL NOT silently delete, reset or replace the store.

The store SHALL hold at most:

- 1,024 identity rows, tracked and retained, each with its tracking status, pricing state and at most one deferred probe request (endpoint, kind and corpus prompt index);
- hourly rows only for hours inside the window, each holding the identity's sample start and class counts, probe start and class counts, billing outcome counts and time-to-first-content bucket counts;
- 16 eviction intervals per identity, only for intervals that overlap the window;
- 16,384 wallet rows, each holding `sharing_since`, the time of the wallet's most recent verified offer, whether any identity of the wallet has ever had an available sample, the current free period's start, `latest_probe_at`, and the billing mismatch flag with its evidence;
- 16 completed free-period summaries per wallet;
- the observer-healthy-time counter.

Any offer from a wallet's own peer that is verified under the identity rules, whether during discovery, a sample or a probe, SHALL create or refresh that wallet's row, whether or not an identity is tracked for it.

Retention SHALL be:

- hourly rows older than the window SHALL be deleted at least once an hour, and eviction intervals that end before the window SHALL be deleted with them;
- an identity row SHALL be deleted when it is not tracked and has no hourly row in the window;
- when a new identity row would exceed 1,024, the catalog SHALL delete an untracked identity row that has no available, unavailable, succeeded or failed count in the window, the one with the oldest most recent sample first. When there is none, the new row SHALL NOT be created and the identity SHALL be refused with reason `capacity`. A row with such counts in the window SHALL NOT be deleted to make room;
- a wallet row, with its free-period summaries and flag, SHALL be kept for 180 days after the wallet's most recent verified offer and then deleted; when a 17th free-period summary is added, the oldest SHALL be deleted;
- when a new wallet row would exceed 16,384, the catalog SHALL delete, among wallets with no tracked identity that have never had an available sample and have no active billing mismatch flag, the row with the oldest most recent verified offer. When there is none, the new wallet row SHALL NOT be created, and that wallet's `sharing_since` SHALL stay `null` until a row can be created.

Each sample's and probe's start SHALL be committed before its first outbound attempt. Finalized classes, billing outcomes, flags, pricing-state changes, free-period changes and the observer-healthy-time counter SHALL be written in one transaction per health check.

The store SHALL contain only these data: network identities, wallets and peer IDs; tracking status, pricing state, deferred probe requests, reasons and their times, and eviction intervals; the hourly counts above; finalization, last-available and last-probe times, and observer-healthy-time values; `sharing_since`, most-recent-offer times and ever-available flags; free-period summaries; and billing mismatch flags with their evidence. It SHALL NOT contain IP addresses, multiaddresses, endpoints, offer bodies, readiness bodies, observer or probe identity keys, probe account credentials, quote or account identifiers, generated content, consumer identities, request data beyond the flag's evidence, or records of who read a record.

The tokenizer prefix cache SHALL be kept outside the store under the catalog's data directory, within the bounds of *Billing honesty is checked against the declared tokenizer*, and MAY be deleted at any time without affecting any record.

#### Scenario: A store of another network at the scoped path

- **GIVEN** a launch-reputation store file at the relay's network-scoped path whose recorded network identity differs from the relay's
- **WHEN** the relay starts with the `launch_reputation` object
- **THEN** the relay refuses to start
- **AND** the store file is unchanged

#### Scenario: Data older than the window is removed

- **WHEN** an hourly row falls outside the 336-hour window
- **THEN** it is deleted within one hour and no longer contributes to any windowed figure

#### Scenario: History is not erased to make room

- **GIVEN** 1,024 identity rows, each with available or unavailable samples in the window
- **WHEN** a newly verified identity would need a row
- **THEN** no row is deleted
- **AND** the new identity is refused with reason `capacity`

#### Scenario: Sybil wallets do not evict an established wallet

- **GIVEN** a wallet with no tracked identity, one of whose identities once had an available sample
- **WHEN** 16,384 new wallets that have never had an available sample publish verified offers
- **THEN** that wallet's row, its `sharing_since` and its free-period summaries remain

#### Scenario: Free-period results outlive the window

- **GIVEN** a wallet whose only free period ended 30 days ago with 150 counted probes, and which has charged since
- **WHEN** its record is computed
- **THEN** its `free_periods` still reports that period, and `probe_basis` is `free_periods`

#### Scenario: `sharing_since` resets after 180 days of absence

- **GIVEN** a wallet whose most recent verified offer was 181 days ago
- **WHEN** it publishes a qualifying offer again
- **THEN** its `sharing_since` is the time of this new verification, and its earlier free periods are no longer reported

#### Scenario: No secrets or content in the store

- **WHEN** the store is inspected after probing free providers over HTTPS and libp2p
- **THEN** it contains no IP address, endpoint, probe key, credential, account identifier or generated text

### Requirement: The catalog serves launch reputation through a bounded read-only API

The catalog SHALL serve:

- `GET /api/launch-reputation/:wallet/:peer_id`, returning one record;
- `POST /api/launch-reputation/query` with a JSON body, at most 16 KiB and rejecting unknown fields, of the form `{"identities": [{"provider_wallet", "peer_id"}, ...]}` holding 1–64 distinct identities, returning `{"records": [...]}` in request order.

A wallet SHALL be canonical, `0x` followed by 40 lowercase hexadecimal digits. A peer ID SHALL equal the string obtained by parsing it and encoding it again. Anything else SHALL be rejected with HTTP 400 `invalid_wallet` or `invalid_peer_id`, never answered as `not_discovered`.

A record SHALL be exactly:

```text
{
  network, provider_wallet, peer_id,
  experimental_testnet: true,
  observer_status, computed_at,
  window_hours: 336,
  mean_sample_interval_seconds: 300,
  mean_probe_interval_seconds: 600,
  probe_corpus_sha256, tokenizer_runtime,
  sharing_since,
  wallet: {
    identities,
    samples: {available, unavailable, unknown, skipped},
    expected_samples, coverage_bps,
    availability_14d, counted_since, observed_at,
    free_now, free_since,
    probes: {succeeded, failed, busy, unreached, unknown, skipped},
    probe_success_14d, latest_probe_at,
    billing_checks: {matched, mismatched, unchecked},
    first_token_ms_buckets: [nine counts],
    free_periods: [
      {started_at, ended_at,
       probes: {succeeded, failed, busy, unreached, unknown, skipped},
       launch_criteria_met_at_end}
    ],
    probe_basis, probe_count, probe_success_bps,
    service_score_bps,
    billing_mismatch: null | {
      flagged_at, until, model_id, model_revision, tokenizer_revision,
      chat_template_sha256, endpoint, prompt_index, probe_corpus_sha256,
      expected_prompt_tokens, reported_prompt_tokens, tokenizer_runtime
    },
    launch_criteria: {met, unmet}
  },
  peer: {
    tracked, untracked_reason, untracked_since, identity_since,
    samples: {available, unavailable, unknown, skipped},
    expected_samples, coverage_bps, evicted_seconds,
    availability_14d, counted_since, observed_at,
    pricing, pricing_since,
    probes: {succeeded, failed, busy, unreached, unknown, skipped},
    probe_success_14d, latest_probe_at
  }
}
```

`untracked_reason` SHALL be `null` when `tracked` is `true`, and otherwise one of `not_discovered`, `capacity`, `wallet_limit`, `evicted` or `inactive`. `observer_status` SHALL be `healthy` or `degraded`. `pricing` SHALL be `unknown`, `free` or `charging`. `probe_basis` SHALL be `window`, `free_periods` or `null`. `tokenizer_runtime` SHALL name the pinned runtime identity used for billing checks. A well-formed request for an identity the catalog holds no data about SHALL return HTTP 200 with `tracked: false`, `untracked_reason` `not_discovered`, zero counts and `null` figures in the peer section. Its wallet section and `sharing_since` SHALL still report what the catalog holds for that wallet. It SHALL never be an error that a client could display as zero availability or a zero score.

Errors SHALL be:

- HTTP 400 `invalid_wallet`, `invalid_peer_id` or `invalid_query`;
- HTTP 413 for an oversized body;
- HTTP 429 `launch_reputation_busy` when the route's 16 concurrent read slots are in use; these are separate from the four feedback verification slots;
- HTTP 503 `launch_reputation_unavailable` when the observer is not configured or the store cannot be read.

Reads SHALL use stored data and the in-memory refusal table only, and SHALL NOT trigger a sample, a probe, a tokenizer fetch, an outbound connection or a DNS lookup. `GET /api/health` SHALL add a `launch_reputation` member reporting whether the observer is enabled, its `observer_status`, the number of tracked identities, the number of identities being probed and whether the billing self-test passes. The relay's TLS ingress generated by operator provisioning SHALL expose these routes.

#### Scenario: A tracked free identity

- **WHEN** a client requests the record of a tracked free identity with counted samples and probes
- **THEN** the catalog returns HTTP 200 with the record, including both sections, the availability figures, the probe counts, `service_score_bps` and `launch_criteria`

#### Scenario: An identity and wallet the catalog never saw

- **WHEN** a client requests a well-formed wallet and peer ID that the catalog never verified
- **THEN** the catalog returns HTTP 200 with `tracked: false`, `untracked_reason` `not_discovered`, zero counts in both sections, `null` `sharing_since`, `null` figures and an empty `free_periods`

#### Scenario: A new peer of a known wallet

- **WHEN** a client requests a peer ID the catalog never verified, for a wallet that has a `sharing_since` and samples and probes from another peer
- **THEN** the peer section reports `not_discovered` with zero counts
- **AND** the record reports that wallet's `sharing_since` and wallet section

#### Scenario: Non-canonical identifiers

- **WHEN** a client requests a wallet written with uppercase hexadecimal digits
- **THEN** the catalog returns HTTP 400 `invalid_wallet`, not `not_discovered`

#### Scenario: Reads never contact providers or Hugging Face

- **WHEN** a client queries 64 identities, including ones the catalog never saw
- **THEN** the catalog makes no outbound connection, DNS lookup, sample, probe or tokenizer fetch because of the query

#### Scenario: Observer not configured

- **WHEN** a client requests a record from a catalog whose relay configuration has no `launch_reputation` object
- **THEN** the catalog returns HTTP 503 `launch_reputation_unavailable`

### Requirement: Launch reputation never enters payment-gated reputation, and free service still cannot vote

Payment-gated feedback admission and the Elo calculation SHALL be unchanged by this capability. An account **cannot vote** when its quote's funding is zero, or when both of its quoted token rates are zero; this is the catalog's existing admission rule. Feedback admission SHALL continue to reject such submissions, so free sessions, including probe accounts, SHALL NOT vote. The reputation snapshot (`GET /api/ratings/:wallet`) SHALL keep its existing fields and calculation, and SHALL gain only the `votes_14d` member of *The reputation snapshot reports recent paid votes*. Its calculation SHALL NOT read launch-reputation data. No catalog route and no native or desktop surface SHALL combine launch reputation and the payment-gated score into one score, rank, badge or ordering, or use availability, probes or the service score to weight, admit or revalidate a vote.

The native consumer SHALL refuse feedback for an account that cannot vote before contacting the catalog, including an account with an opening payment whose quote has both token rates zero. For such an account, the desktop's feedback surface SHALL NOT offer a submission. It SHALL state that free service earns no ratings, because a free vote costs nothing to fake, and that the catalog's launch reputation is shown instead.

#### Scenario: A free agreement cannot vote

- **WHEN** a submission whose quote has zero funding and zero token rates reaches `POST /api/ratings/feedback`
- **THEN** the catalog rejects it as before and stores no vote

#### Scenario: Positive funding with zero rates

- **GIVEN** a retained legacy account opened with a positive payment against a quote whose input and output rates are both zero
- **WHEN** the user requests feedback for it
- **THEN** the native consumer refuses before contacting the catalog
- **AND** the Usage feedback view offers no submission and shows that free service earns no ratings

#### Scenario: A high service score does not move the Elo score

- **GIVEN** a provider with no eligible votes, 100% availability and a service score of 100
- **WHEN** its reputation snapshot is read
- **THEN** it reports the neutral baseline with zero votes and `votes_14d` 0, exactly as without launch reputation

#### Scenario: Free account feedback surface

- **GIVEN** a free account in Usage & API keys
- **WHEN** the user opens its feedback view
- **THEN** no submission control is offered and the statement that free service earns no ratings is shown

### Requirement: The reputation snapshot reports recent paid votes

`GET /api/ratings/:wallet` SHALL add one member, `votes_14d`: the number of the snapshot's eligible rows, selected and revalidated exactly as for `vote_count`, whose stored feedback `issued_at` is no more than 1,209,600 seconds (14 days) before the snapshot's computation time. A revision moves its row's `issued_at`, so a revised vote counts as recent. `votes_14d` SHALL be at most `vote_count`. It SHALL NOT change `elo`, `base_elo`, `vote_count`, feedback admission or revalidation, and SHALL be subject to the snapshot's existing unavailability errors. It exists so that automatic pricing can apply the book's rule of a payment-gated score below 45 with at least 5 votes in 14 days.

#### Scenario: Recent and old votes

- **GIVEN** a provider with seven eligible votes, three of whose latest feedback was issued within the last 14 days
- **WHEN** its snapshot is read
- **THEN** `vote_count` is 7 and `votes_14d` is 3, and `elo` is computed exactly as before

#### Scenario: Unavailable evidence

- **WHEN** required RPC evidence is unavailable while a snapshot is computed
- **THEN** the snapshot fails with HTTP 503 `chain_unavailable` as before, and no `votes_14d` is served

### Requirement: Native provider inspection reports the launch reputation separately

The native `provider_info` result SHALL contain a `launch_reputation` member beside `health`, `readiness` and `reputation`. It SHALL be read from the configured `catalog_url` whether discovery used the DHT or the catalog, with a ten-second deadline and a 32 KiB response bound. It SHALL be either `{"available": true, "record": <record>}` or `{"available": false, "error": <reason>}`. The reason SHALL distinguish:

- no configured catalog;
- a catalog that does not publish launch reputation (HTTP 404 or 405);
- a catalog that reports the record unavailable (HTTP 503);
- a busy catalog (HTTP 429);
- an unreachable catalog;
- an invalid response.

An unavailable record SHALL NOT be replaced by a zero, a neutral value, the reputation result or an inference from readiness or from the offer's prices.

The native consumer SHALL accept a record only when all of the following hold:

- its `network` equals the loaded profile's identity, and its `provider_wallet` and `peer_id` equal the inspected offer's;
- `experimental_testnet` is `true`, `window_hours` is 336, `mean_sample_interval_seconds` is 300, `mean_probe_interval_seconds` is 600, and `probe_corpus_sha256` is 64 lowercase hexadecimal characters;
- `untracked_reason` is consistent with `tracked`, and every enumerated member holds one of its allowed values;
- every count is a non-negative integer; the peer section's sample counts sum to at most 8,064 and its probe counts to at most 4,032; `identities` is at most 1,024; the wallet section's sample counts sum to at most `identities × 8,064` and its probe counts to at most `identities × 4,032`; `billing_checks` and `first_token_ms_buckets` each sum to at most the wallet's succeeded probes; and `free_periods` has at most 16 entries, newest first, each with `ended_at` not before `started_at`;
- in each section, `availability_14d` and `counted_since` are `null` exactly when `available + unavailable` is zero; `availability_14d`, `coverage_bps` and `probe_success_14d` equal the values recomputed from that section's counts and `expected_samples`; and the peer section's `expected_samples` equals the value recomputed from its `counted_since`, `computed_at`, `untracked_reason` and `untracked_since`;
- `probe_basis`, `probe_count`, `probe_success_bps`, `service_score_bps` and `launch_criteria` equal the values recomputed from the record's own members;
- `billing_mismatch` is `null` or has `until` equal to `flagged_at` plus 1,209,600 and later than `computed_at`;
- no timestamp is more than 30 seconds in the future.

Otherwise it SHALL report an invalid response.

A native `launch_reputations` operation SHALL accept 1–64 distinct identities, rejecting non-canonical ones. It SHALL query `POST /api/launch-reputation/query` once with a ten-second deadline and a 512 KiB response bound, validate each record as above, and return one result per identity. The readiness summary of `provider_info` SHALL report whether the readiness identity members match the offer: `match`, `mismatch` or `unreported`. A mismatch SHALL be shown as not ready.

#### Scenario: DHT discovery with a configured catalog

- **GIVEN** a consumer profile with both `dht` and `catalog_url`
- **WHEN** the user inspects an offer found through the DHT
- **THEN** `provider_info` reads the launch reputation from the configured catalog

#### Scenario: No catalog configured

- **WHEN** a consumer profile without `catalog_url` inspects an offer
- **THEN** `launch_reputation` is `{"available": false}` with the reason that no catalog is configured, and no figure is shown

#### Scenario: Inconsistent catalog response

- **WHEN** a catalog returns a record whose `service_score_bps` differs from the value recomputed from its availability and probe members
- **THEN** the native consumer reports an invalid response and shows no figure from it

#### Scenario: Impossible probe count

- **WHEN** a catalog returns a peer section whose probe counts sum to 4,033
- **THEN** the native consumer reports an invalid response

#### Scenario: Readiness identity mismatch

- **WHEN** an inspected offer's readiness returns `ready: true` with a `provider_wallet` different from the offer's
- **THEN** the inspection reports readiness as not ready with an identity mismatch

### Requirement: The Marketplace shows the launch reputation beside, never blended into, the Elo score

Each Marketplace offer card SHALL show the provider's launch reputation from the native `launch_reputations` operation, labeled as observed by the configured catalog and naming the catalog by its host. Percentages and scores SHALL be truncated to one decimal place, never rounded up, and dates and ages SHALL come from the record's catalog times. The lines SHALL use the wallet section:

- "Launching — free" when the offer's rates and minimum funding are zero, `free_now` is `true` and `launch_criteria.met` is `false`;
- "Service score *S* (*catalog*-observed)", where *S* is `service_score_bps` divided by 100, or "No service score yet" when it is `null` and `sharing_since` is not;
- "*N*% available over 14 days (*catalog*-observed)", where *N* is `availability_14d` divided by 100. When `counted_since` is later than the start of the window, it SHALL instead read "*N*% available since *date of counted_since* (*catalog*-observed)". With `sharing_since` but no figure it SHALL read "No availability samples yet";
- "*P* probes · latest *age* ago" when `probe_basis` is `window`, or "*P* probes while free · latest *age* ago" when it is `free_periods`, where *P* is `probe_count` and *age* comes from `latest_probe_at`;
- "Billing mismatch until *date*" while `billing_mismatch` is not `null`;
- "Sharing since *date*";
- "Low coverage" when the wallet's `coverage_bps` is below 5,000;
- "No track record yet", alone, when the record has neither `sharing_since` nor any counted sample or probe;
- "Launch reputation unavailable" with the native reason, on that card only, when the record is unavailable.

When the offer's own peer is not tracked, the card SHALL add one line from the peer section:

- `not_discovered`: "This peer is not yet tracked by *catalog*";
- `capacity`: "This peer is not tracked by *catalog* (capacity)";
- `wallet_limit`: "This peer is not tracked by *catalog* (per-wallet limit)";
- `evicted`: "This peer has not been sampled by *catalog* since *date* (capacity)";
- `inactive`: "This peer has been inactive since *date*".

Beside the launch reputation, every card SHALL show "Reputation: see inspection", so that no card shows the launch reputation without pointing to the payment-gated reputation.

The Marketplace SHALL deduplicate the identities of the offers shown. It SHALL request them in chunks of at most 64, with at most two requests in flight. A failed chunk SHALL mark only its own cards as unavailable. An HTTP 429 SHALL be retried at most once, after a delay of two to five seconds. Results for a superseded discovery SHALL be discarded.

The inspection panel SHALL show a Launch reputation tile beside, and separate from, the Health, Readiness and Reputation tiles. The tile SHALL show:

- `sharing_since`, `free_now` with `free_since`, and the peer's pricing state;
- for the wallet section and for the peer section, the availability figure, the available, unavailable, unknown and skipped sample counts, the expected samples and the coverage; unknown SHALL be labeled as excluded because the observer was degraded, and skipped as excluded because of the observer's own budgets;
- the probe counts by class, with busy, unreached, unknown and skipped labeled as excluded and why; `probe_success_14d`; the probe basis with `probe_count` and `probe_success_bps`; and `latest_probe_at`;
- the service score, and `launch_criteria` with each unmet criterion in words;
- the time-to-first-content buckets, labeled as measured from the catalog's network location and not scored;
- the billing outcome counts, "Billing unchecked" when no outcome is `matched` or `mismatched`, labeled as not a mismatch and not blocking the launch criteria, and an active flag with its evidence;
- each completed free period with its dates, counts and whether the launch criteria were met at its end;
- the peer's `identity_since`, tracking status and evicted time;
- `observed_at`, `observer_status`, `tokenizer_runtime` and the catalog host.

The panel SHALL state that:

- availability and probes are one catalog's observations from its own network location, not speed, quality, honesty or consensus, and independent catalogs can disagree;
- probes run only while the provider is free, use the catalog's own free accounts, and do not prove which model ran;
- the billing check compares reported prompt tokens with the declared tokenizer only;
- a provider can recognize the observer by its network address and treat it differently, and quality during a free period may not persist after the provider charges;
- the launch criteria do not include qualification, which the offer's serving profile shows;
- low coverage means the observer took fewer samples than its schedule intends.

For an offer whose input and output rates are both zero, the panel SHALL also state that free service earns no ratings and that reputation reflects paying users' feedback only.

The Marketplace SHALL NOT combine the launch reputation with the Elo score. It SHALL NOT reorder, hide or filter offers by either by default, and SHALL treat free and paid offers' availability identically. Status SHALL be conveyed by text, not color alone. In a browser preview, the Marketplace SHALL show no launch reputation and SHALL NOT fabricate one.

#### Scenario: A launching free provider

- **GIVEN** a free offer whose record, from catalog `catalog.example`, has `free_now` `true`, `launch_criteria.met` `false`, `availability_14d` 9,876 with `counted_since` 18 hours ago, 72 counted window probes of which 71 succeeded with the latest 9 minutes ago, so `probe_success_bps` 9,861 and `service_score_bps` 9,738, and `sharing_since` 2026-11-15
- **WHEN** its card is shown
- **THEN** it reads "Launching — free", "Service score 97.3 (catalog.example-observed)", "98.7% available since 2026-11-15 (catalog.example-observed)", "72 probes · latest 9 min ago" and "Sharing since 2026-11-15"
- **AND** it shows "Reputation: see inspection"

#### Scenario: A charging provider

- **GIVEN** a priced offer whose record has `probe_basis` `free_periods`, `probe_count` 153 with `probe_success_bps` 9,019, a full window of availability at 9,950 and `service_score_bps` 8,973
- **WHEN** its card is shown
- **THEN** it reads "Service score 89.7", "99.5% available over 14 days" and "153 probes while free · latest *age* ago", with no "Launching — free" line

#### Scenario: Billing mismatch

- **WHEN** a record has an active billing mismatch flag until 2026-11-30
- **THEN** the card reads "Billing mismatch until 2026-11-30", and the inspection tile shows the expected and reported counts and the prompt index

#### Scenario: Billing unchecked

- **GIVEN** a record whose `billing_checks` are `{"matched": 0, "mismatched": 0, "unchecked": 40}` and no active flag
- **WHEN** its inspection tile is shown
- **THEN** it reads "Billing unchecked", says this is not a mismatch and does not block the launch criteria, and the card shows no billing mismatch line

#### Scenario: A provider the catalog has not seen

- **WHEN** a record has no `sharing_since` and no counted sample or probe
- **THEN** the card reads "No track record yet" and "This peer is not yet tracked by catalog.example"

#### Scenario: Many offers

- **GIVEN** discovery returns 300 offers from 150 distinct identities
- **WHEN** their cards load
- **THEN** the Marketplace sends three batch requests, with at most two in flight
- **AND** a failed request marks only its own cards as "Launch reputation unavailable"

#### Scenario: Never blended

- **WHEN** the inspection panel shows an Elo of 50.0 with 0 votes and a service score of 99.0
- **THEN** they appear in separate tiles with their own labels
- **AND** no combined score, rank or badge is shown

#### Scenario: Browser preview

- **WHEN** the Marketplace runs outside the native runtime
- **THEN** no launch reputation is shown and native actions fail with the preview explanation

### Requirement: Observation learns no more than a consumer and handles no other consumer's data

The observer SHALL learn about a provider only what any native consumer of its free offers would learn: the addresses it dials or that the DHT reports, its signed offers, its readiness response, and the quotes, credentials, request identifiers, usage and responses of the observer's own probe accounts. Availability samples SHALL NOT request a quote, open an account or send a prompt, and SHALL call no provider route other than the manifest protocol and `GET /readyz`. Probes SHALL call only the manifest protocol, `GET /readyz`, the challenge, proposal, opening and credential routes for their own accounts, the inference route of their kind, and for the cancellation kind the cancel and request-status routes of their own request. The observer SHALL NOT send funding, sign a transaction, submit feedback, or send any prompt other than a probe corpus prompt. It SHALL NOT persist provider addresses, endpoints or response bodies; verified addresses SHALL stay in memory, and generated content SHALL be discarded after classification. Its logs SHALL carry identities, classifications and reason codes. They SHALL NOT carry addresses unless the operator explicitly enables diagnostic logging that is documented as recording them, and SHALL NEVER carry probe keys, credentials or generated content.

Launch-reputation reads SHALL NOT require consumer identification, and the catalog SHALL NOT record in its store which client read which record. A record SHALL contain no data of any consumer other than the observer, and the only request data it contains is a billing mismatch flag's evidence about a corpus prompt.

#### Scenario: Sample traffic is limited to manifest and readiness

- **WHEN** a priced provider's public router and manifest handler record every request from the observer's addresses during a day
- **THEN** the only requests are manifest pages and `GET /readyz`
- **AND** none carries a credential, prompt, quote or payment

#### Scenario: Probe traffic is limited to the observer's own free accounts

- **WHEN** a free provider records every request from the observer's addresses during a day
- **THEN** besides manifest pages and readiness, the requests are challenges, free proposals and openings for probe wallets, credential operations of those accounts, inference requests with corpus prompts, and cancel and status requests for the probes' own requests
- **AND** none carries a payment, a transaction or feedback

### Requirement: A provider can improve its launch reputation only by actually serving

The only way a provider's samples SHALL become available is by its being reachable, serving a qualifying offer from its own authenticated manifest and reporting ready with its own identity, at sample times it cannot choose or predict. The only way its probes SHALL succeed is by its correctly serving real free requests from accounts it cannot link to the observer through their wallets or peer IDs, at probe times it cannot choose or predict. No signed statement, provider timestamp, readiness value for another identity, catalog request or consumer action SHALL add an available sample or succeeded probe, or remove an unavailable sample, failed probe or billing mismatch flag.

A relay that the provider uses refusing or cutting the observer's circuit SHALL count against the provider, because consumers depend on the same path. The observer's own budgets SHALL decide skipping only from the observer's state when a plan is fixed, and SHALL never apply to an identity's verified addresses. The effects of observer budgets and degradation SHALL appear only as unknown or skipped counts and lower coverage, never as available samples or succeeded probes. A busy response SHALL NOT be retried, SHALL appear in its own count, and SHALL defer its request as *A probe succeeds only when the provider serves it correctly* requires, so that refusing selected kinds cannot raise probe success. Ending a free period with a short run of good probes SHALL NOT replace an earlier free period's results while fewer than 100 counted probes separate them.

A provider SHALL be able to abandon its history only by changing its wallet, which starts a new `sharing_since`, new figures and no billing mismatch flag. A new peer ID SHALL start a new peer section but SHALL leave the wallet's `sharing_since`, figures, free periods, billing mismatch flag and payment-gated reputation unchanged. Both kinds of change SHALL remain visible in the record and SHALL NOT be hidden or merged by the catalog.

#### Scenario: Reporting ready from a borrowed origin does not help

- **WHEN** a provider's offers name another provider's origin and its own libp2p manifest is always reachable
- **THEN** its samples are unavailable and its probes unreached, because the readiness response names another identity

#### Scenario: A new peer ID keeps the wallet's history

- **GIVEN** a wallet whose peer `P1` has poor availability and a billing mismatch flag in the window
- **WHEN** its operator stops `P1` and starts sharing from a new peer `P2` under the same wallet
- **THEN** the wallet section of `P2`'s record still includes `P1`'s samples and probes in the window and the flag

#### Scenario: A new wallet shows a new history

- **GIVEN** a wallet with poor availability that stops sharing
- **WHEN** its operator starts sharing from a new wallet
- **THEN** the new wallet's record shows a new `sharing_since` and no samples or probes before its own

#### Scenario: A self-operated relay cannot hide downtime

- **GIVEN** a provider whose only relay is one it operates
- **WHEN** the provider is offline and that relay refuses the observer's circuit with a resource-limit status
- **THEN** the sample is unavailable

### Requirement: The observer cannot be used to reach private hosts or amplify traffic

Every outbound connection that the observer makes SHALL be to one of these:

- an address that passes the public-address predicate, or literal loopback under the explicit `development` flag;
- an operator-configured reference peer or reference origin;
- the catalog's own relay;
- for tokenizer fetches only, `huggingface.co` and the allowlisted Hugging Face download hosts, at public addresses.

The observer SHALL send only fixed requests: manifest requests within the existing frame bounds; `GET /readyz` with no body; the probe requests that *Probe requests use a fixed public corpus and fixed kinds* defines, with the account-control requests that precede them; and tokenizer range requests. It SHALL NOT follow redirects, except a tokenizer fetch's allowlisted redirects, and SHALL NOT dial addresses named in responses. No read or write route of the catalog SHALL cause an outbound request.

These bounds SHALL hold regardless of how many tracked identities name a destination:

- each sample makes at most three direct libp2p connection attempts, one relayed path with its hole-punch upgrade, and two connection attempts per readiness request;
- each probe makes at most three direct libp2p connection attempts and one relayed path, and at most two HTTPS connection attempts, all to verified addresses only;
- each destination (IPv4 address or IPv6 /64) receives at most 12 attempts in any 60 seconds to addresses unverified for the attempting identity;
- at most 16 identities are verified within one destination;
- each identity reaches unverified addresses in at most six distinct destinations per UTC hour;
- each origin receives at most one readiness request per 60 seconds;
- each identity receives at most 24 samples and 12 probes per UTC hour;
- each third-party relay carries at most four observer circuits at a time, opened no faster than libp2p's default per-source rate;
- tokenizer fetches are at most one in flight and 12 per hour, each at most 32 MiB;
- responses are read only up to their bounds.

#### Scenario: Sybil offers aimed at one host

- **GIVEN** 500 tracked identities whose offers name origins and addresses that all resolve to one public IPv4 address, none of them verified there
- **WHEN** they are sampled and, being free, scheduled for probes
- **THEN** the observer makes at most 12 connection attempts to that address in any 60 seconds, and no probe connects to it

#### Scenario: Eight advertised victim addresses

- **GIVEN** an identity that advertises eight public addresses of other hosts and never authenticates at any of them
- **WHEN** it is sampled repeatedly
- **THEN** each sample makes at most three connection attempts
- **AND** the identity reaches at most six distinct destinations in any UTC hour

#### Scenario: Metadata endpoint

- **WHEN** a qualifying offer's origin resolves to `169.254.169.254`
- **THEN** the observer makes no connection to it

#### Scenario: Free offers aimed at a third-party Hugging Face repository

- **GIVEN** 1,000 free identities whose offers name 1,000 different public Hugging Face repositories
- **WHEN** their probes need billing checks
- **THEN** the catalog starts at most 12 tokenizer fetches per hour, each reading at most 32 MiB, and the remaining checks are `unchecked`

#### Scenario: Readers cannot steer the observer

- **WHEN** a client queries the record of an identity whose peer ID it invented, naming no real peer
- **THEN** the catalog returns `not_discovered` and contacts nothing
