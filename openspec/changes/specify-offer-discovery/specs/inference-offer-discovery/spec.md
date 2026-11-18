## ADDED Requirements

### Requirement: Discovery yields verified candidate offers, never a census or an authority

Discovery SHALL return only signed offers that the receiver itself verified for the selected complete network identity (`inference-wire-foundation`, *Complete and explicit network identity*), under the offer acceptance rules of `inference-commercial-terms` (*Complete signed offer declaration* and *Offer freshness and advertised origin*) and the revision rules of this capability (book line 29). A DHT peer, a manifest, a relay catalog and a long-running registry SHALL each be treated only as a source of candidate offers. Neither the DHT nor a relay catalog SHALL be treated as an exhaustive directory of providers, an attestation of model execution or a custodian (book line 59; `CLAUDE.md`: "Relay catalogs are caches, not exhaustive or authoritative provider directories").

A relay can omit an offer. It SHALL NOT be able to change an offer's signed price, payee or any other signed field without that offer failing verification at the receiver (book line 29). Discovery results SHALL NOT be described as complete: a provider missing from the results is not evidence that it does not exist, and a first or only reply is not evidence of the best price or quality (book line 226). The relay catalog SHALL NOT carry prompts or model output and SHALL NOT proxy inference (book lines 30 and 59).

#### Scenario: A rewritten catalog entry is not a candidate

- **GIVEN** a catalog page with three signed offers, one of which had its `input_wei_per_1k` changed after signing
- **WHEN** the native consumer reads the page
- **THEN** the changed entry fails verification and is skipped
- **AND** the other two offers are returned, and no result carries the changed price

#### Scenario: Another network's peer supplies no candidates

- **GIVEN** a provider on network deployment `test-a` that publishes a valid offer
- **WHEN** a consumer on network deployment `test-b`, with the same chain and genesis, uses that provider as its only bootstrap peer
- **THEN** discovery returns no offers

#### Scenario: The relay does not serve inference

- **WHEN** a client sends `POST /v1/chat/completions` to a relay
- **THEN** the relay does not route it to any provider or engine and returns no model output

### Requirement: DHT indexes point to candidate peers within one network, under a stable peer identity

Kademlia SHALL advertise candidate peers only. A DHT node SHALL store no ordinary value records, so that no offer or other blob is accepted from the DHT itself; each peer SHALL serve its own offers over the manifest protocol (book line 215). Every DHT protocol name and index key SHALL be bound to the complete network identity (book lines 219 and 220; `CLAUDE.md`: "network-scoped namespaces"):

- `scope` is `hash_hex(canonical_json(network))`;
- the Kademlia protocol is `/chiral/inference/kad/1/{scope}` and the Identify protocol is `/chiral/inference/id/1/{scope}`;
- an index key is Keccak-256 of `canonical_json([network, model_selector])`, where `model_selector` is `null` for the all-models index or the exact model ID string.

A node SHALL disconnect a peer whose Identify protocol names another scope. Whichever peer answers a manifest request, and whether or not its Identify information has arrived, an offer for another network SHALL NOT be accepted (*Discovery yields verified candidate offers, never a census or an authority*). Provider records SHALL have a 600-second TTL and SHALL be republished every 120 seconds; these SHALL NOT be described as the signed offer lifetime (book line 224). A node SHALL accept at most 32 bootstrap multiaddresses, each ending in `/p2p/<peer-id>`, and SHALL refuse to start otherwise. It SHALL retry bootstrap every 30 seconds (book line 224).

Each node SHALL persist its libp2p identity and SHALL reuse it after restart (book line 236; `CLAUDE.md`: "stable peer identities"). The identity is a private key, so its file SHALL be a protected secret file like the node's other secrets (book line 474; `CLAUDE.md`: "Wallet/private engine/operator secrets use protected files"): on Unix a regular file owned by the running user with no group or other permissions, and on Windows a file with a current-user-only access list that does not inherit permissions. A node SHALL create a missing identity file with that protection, SHALL NOT follow a symbolic link or reparse point to it, and SHALL refuse to start with an existing identity file that is not protected this way, without replacing it. A provider's offers SHALL name that persisted peer ID both as `peer_id` and as `deployment_id` (book lines 127, 128 and 148), and its ledger, lock and offer-revision state SHALL live under `<data_dir>/<network-hash>/<signing-wallet>/<peer-id>/` (book line 472). A new peer ID is a different deployment. Where a consumer profile's `dht.identity_file` is relative, its location is governed by `profile-path-resolution` (pending `specify-profile-path-resolution`).

#### Scenario: Index keys separate networks and models

- **GIVEN** two network identities that differ only in their deployment ID
- **THEN** their all-models index keys differ
- **AND** for one network, the all-models key, the key for model `m` and the key for model `n` all differ

#### Scenario: The peer identity survives restart

- **WHEN** a DHT service stops and starts again with the same identity file
- **THEN** it has the same peer ID and serves no offers until it publishes again

#### Scenario: An exposed identity file is refused

- **GIVEN** on Unix, an existing identity file with mode `0644`
- **WHEN** a DHT service starts with it
- **THEN** it refuses to start because the identity must be owner-only
- **AND** the file is unchanged

#### Scenario: A symbolic link to an identity file is refused

- **GIVEN** an identity path that is a symbolic link to an owner-only key file
- **WHEN** a DHT service starts with it
- **THEN** it refuses to start and neither the link nor the key file is changed

#### Scenario: A new identity file on Windows is owner-protected

- **WHEN** a DHT service on Windows starts with an identity path that does not exist
- **THEN** it creates the file with a current-user-only access list that does not inherit permissions
- **AND** a later start refuses an identity file whose access list grants another account

#### Scenario: Bootstrap addresses are bounded and name their peer

- **WHEN** a DHT configuration lists 33 bootstrap multiaddresses, or one that does not end in `/p2p/<peer-id>`
- **THEN** the service refuses to start

### Requirement: DHT connections and inbound manifest requests are admission-bounded

A DHT node SHALL bound its connections before any protocol handler is allocated for them (`CLAUDE.md`: "connection admission limits"). It SHALL bound pending inbound handshakes and pending outbound handshakes separately. It SHALL bound established connections in total, and SHALL let inbound connections use only part of that total, so that an inbound flood cannot take the capacity that outbound bootstrap and discovery dials need. It SHALL bound established connections per peer identity.

A node SHALL also bound inbound manifest requests: the number of concurrent manifest streams, the number of peers whose request rate it tracks, and the requests from one peer within a time window. A request beyond its peer's allowance SHALL be dropped unanswered and SHALL NOT reduce another peer's allowance. While the tracked-peer bound is reached, a request from a peer that is not yet tracked SHALL be dropped unanswered. The bound values are implementation values recorded in the design; changing them needs no change to this requirement.

#### Scenario: One identity cannot exceed its connection bound

- **GIVEN** a node whose per-identity bound is 2 established connections
- **WHEN** one hostile peer identity opens 6 TCP/Noise/yamux connections to it at once
- **THEN** exactly 2 are admitted and 4 are denied by the connection limiter

#### Scenario: Inbound handshakes do not consume the outbound budget

- **GIVEN** a node whose pending inbound handshakes are at their bound
- **WHEN** the node dials bootstrap and discovery peers
- **THEN** outbound handshakes are admitted up to their own bound

#### Scenario: Inbound connections leave room for outbound dials

- **GIVEN** a node whose established inbound connections are at their bound
- **WHEN** another peer connects inbound
- **THEN** it is denied
- **AND** outbound connections are still admitted until the total bound is reached

#### Scenario: A peer that floods manifest requests is not answered

- **GIVEN** a peer that has sent its full allowance of manifest requests within the current window (currently 32 requests in 30 seconds)
- **WHEN** it sends another request in that window
- **THEN** that request gets no response
- **AND** a request from another peer is answered

### Requirement: Manifests use the bounded, paged `/chiral/inference/manifest/1` protocol

A peer SHALL serve its own offers on `/chiral/inference/manifest/1` over a connection on which libp2p authenticated the responding peer (book lines 215 and 221). Each frame SHALL be a four-byte big-endian byte length followed by that many bytes of JSON. A request SHALL be exactly `{network, model, offset}` and a response exactly `{offers, next}`, and unknown members SHALL be rejected. A request frame SHALL be at most 2,048 bytes and a response frame at most 262,144 bytes; a frame whose declared length is larger SHALL be refused before its body is read (book line 221). This is the current manifest protocol. The pending target-design change `specify-p2p-provider-transport` replaces it with `/chiral/inference/manifest/2` under the same framing, bounds, paging, `next` rule, per-offer bound and responding-peer binding (*Upgraded nodes serve and query only the v2 manifest protocol*); until that change is implemented, this requirement governs.

A peer SHALL hold at most 64 locally published offers and SHALL refuse to publish an offer for a 65th namespace, while still accepting a new revision for a namespace it already holds (book line 222). It SHALL answer only a request whose `network` equals its own complete network identity and whose `offset` is a multiple of 8 below 64. It SHALL answer with at most 8 unexpired offers in the requested model scope, and with `next` equal to `offset + 8` when more such offers remain and `null` otherwise (book line 222). It SHALL order the offers of its manifest by namespace (`provider_wallet/deployment_id/model.id`), so that republishing an offer under a new revision does not move it to another page.

A receiver SHALL request offset 0 first and SHALL follow `next` only when it equals the previous offset plus 8 and is below 64 (book line 222). It SHALL ignore a page with more than 8 offers. It SHALL bound each signed offer to 16 KiB and validate each offer independently before inserting it into its registry (book line 222). Each offer SHALL name the authenticated responding peer as its `peer_id` and SHALL be in the requested model scope; an offer that does not SHALL be rejected, and the peer's reply SHALL count as invalid (book lines 147 and 226).

#### Scenario: Nine offers are served in two pages

- **GIVEN** a peer with 9 unexpired offers for model `m`
- **WHEN** a receiver requests offset 0 for model `m`
- **THEN** the response holds 8 offers and `next` 8
- **AND** the request for offset 8 returns 1 offer and `next` `null`

#### Scenario: Misaligned or out-of-range offsets are not answered

- **WHEN** a peer receives a manifest request with offset 4, or with offset 64
- **THEN** it sends no response page

#### Scenario: An oversized frame is refused before allocation

- **WHEN** a response frame declares a length of 262,145 bytes
- **THEN** the receiver refuses it without reading or allocating the body

#### Scenario: An offer naming another peer is rejected

- **GIVEN** a page from authenticated peer `A` that contains a validly signed offer whose `peer_id` is `B`
- **THEN** that offer is not inserted and `A`'s reply counts as invalid

#### Scenario: The local manifest holds at most 64 namespaces

- **GIVEN** a peer with 64 local offers in 64 namespaces
- **WHEN** it publishes an offer for a 65th namespace
- **THEN** publication is refused because the local manifest is full
- **AND** publishing a higher revision for one of the 64 namespaces still succeeds

#### Scenario: A republish keeps each offer on its page

- **GIVEN** a peer with 9 offers at revision 5
- **WHEN** it republishes all 9 at revision 6
- **THEN** each namespace is on the same page at revision 6 as it was at revision 5

### Requirement: Searches are bounded and merge verified replies until their deadline

A search SHALL collect replies for 8 seconds, SHALL query at most 64 distinct candidate peers, and SHALL use a 4-second timeout for each manifest request. A DHT service SHALL run at most four searches at once and SHALL refuse a fifth instead of queuing it (book line 223). Candidates SHALL be the peers found under the search's index key and the peers already connected, including bootstrap peers; neither kind SHALL be treated as an authority (book line 223). The caller SHALL wait at most 10 seconds for a result; that wait SHALL NOT be described as a guarantee that connection, bootstrap and the whole operation complete within the 8-second window (book line 223). A model selector SHALL be 1–256 bytes.

A search SHALL merge verified offers from every peer that answers before the deadline, rather than taking the first answer. A bad signature, a malformed offer, a mismatched network, model or peer identity, or a failed reply from one peer SHALL NOT stop another peer's offers from being used (book line 226). The result SHALL be the unexpired offers in the model scope that the service's verified registry holds when the deadline passes. For a long-running service, such as a relay, this includes offers verified by earlier searches (book line 226).

#### Scenario: A bad peer does not block a good peer

- **GIVEN** two candidate peers, one of which serves an offer with an invalid signature and the other a valid offer
- **WHEN** a search runs
- **THEN** the valid offer is in the result and the invalid one is not

#### Scenario: A fifth concurrent search is refused

- **GIVEN** a DHT service running four searches
- **WHEN** a fifth search is requested
- **THEN** it is refused at once as busy and is not queued

#### Scenario: A long-running service returns earlier verified offers

- **GIVEN** a relay whose registry holds an unexpired offer verified by an earlier search from a peer that does not answer this search
- **WHEN** a search for all models ends
- **THEN** the result still includes that offer

#### Scenario: An invalid model selector is refused

- **WHEN** a search is requested with an empty model selector or one of 257 bytes
- **THEN** it is refused before any request is sent

### Requirement: The verified registry holds at most 8,192 namespace entries

A verified offer registry SHALL key its entries by the namespace `provider_wallet/deployment_id/model.id` within one network (book line 232) and SHALL hold at most 8,192 namespace entries (book line 226). An entry SHALL count toward that bound while it holds an offer or a withdrawn revision watermark, until its retention ends under *Withdrawal keeps the revision watermark until its retention ends*. When the registry is full, an offer for a new namespace SHALL be refused without evicting another entry, and an offer for a namespace it already holds SHALL still be processed under the revision rules. Apart from discarding entries whose retention has ended, a refused or invalid offer SHALL NOT create or change an entry.

#### Scenario: A full registry refuses a new namespace but still updates an existing one

- **GIVEN** a registry holding 8,192 namespace entries whose retention has not ended
- **WHEN** a valid offer for a new namespace arrives, and then a higher revision for a namespace it holds
- **THEN** the first is refused and no entry is evicted
- **AND** the second replaces its predecessor

#### Scenario: Ended retention frees room

- **GIVEN** a full registry in which one entry's retention has ended
- **WHEN** a valid offer for a new namespace arrives
- **THEN** the ended entry is removed and the new offer is accepted

#### Scenario: An invalid first offer does not occupy a namespace

- **WHEN** an offer whose signature does not match its payload arrives for a new namespace, followed by a valid offer for that namespace
- **THEN** the invalid offer is refused and the valid offer is accepted

### Requirement: Revisions are monotonic per namespace, and rollback and equivocation are rejected

For each namespace, a registry SHALL accept an offer whose revision is higher than its entry's and SHALL replace the previous offer with it. It SHALL reject an offer with a lower revision. It SHALL reject an offer with the same revision and different content, that is, a different offer ID under `inference-wire-foundation` (*Distinct offer identities and payment commitments*), as equivocation. It SHALL accept a replay of the identical offer while that offer is still present as idempotent: it SHALL return the same offer ID and SHALL NOT change the entry or extend its retention (book lines 156 and 232). A rejected offer SHALL leave the entry unchanged. Revision comparison is local registry state, not cross-session or network-wide consensus (book line 232).

#### Scenario: A higher revision replaces and a lower one is rejected

- **GIVEN** a registry holding revision 1 of namespace `W/gpu-a/model`
- **WHEN** revision 2 of that namespace arrives, followed by revision 1 again
- **THEN** revision 2 replaces revision 1, and the replayed revision 1 is rejected

#### Scenario: Equal revision with different content is equivocation

- **GIVEN** a registry holding revision 2 of a namespace with input price `"1"`
- **WHEN** a validly signed revision 2 of the same namespace with input price `"3"` arrives
- **THEN** it is rejected as equivocation
- **AND** the registry still returns the offer with input price `"1"`

#### Scenario: An identical replay is idempotent and does not extend retention

- **GIVEN** an offer first accepted at time `T`
- **WHEN** the identical offer arrives again at `T + 1,000`
- **THEN** the registry returns the same offer ID and the entry is unchanged
- **AND** the entry's retention still ends at `T + 3,630`

#### Scenario: Deployments are separate namespaces

- **GIVEN** a registry holding revision 2 of `W/gpu-a/model`
- **WHEN** revision 1 of `W/gpu-b/model` arrives
- **THEN** it is accepted, and both offers are listed

### Requirement: Withdrawal keeps the revision watermark until its retention ends

Withdrawing an offer SHALL remove it from lookup by offer ID and from results, and SHALL keep the namespace's revision watermark (book line 232). After a withdrawal, a replay of the withdrawn revision and any lower revision SHALL be rejected, and a higher revision SHALL be accepted. A registry SHALL keep each entry, its watermark included, until 3,630 seconds after its accepted revision was first observed, which covers the 3,600-second maximum offer lifetime plus 30 seconds of issuance skew. It SHALL discard an entry whose retention has ended no later than its next insertion (book line 232). This state SHALL be in-memory state of one registry. It SHALL NOT be described as a persistent or global consensus log (book line 232).

#### Scenario: A withdrawn revision cannot be replayed

- **GIVEN** revision 1 of namespace `W/gpu-b/model` was accepted and then withdrawn
- **WHEN** the identical revision 1 arrives again
- **THEN** it is rejected and the namespace stays absent from results
- **AND** a later revision 2 of that namespace is accepted

#### Scenario: The watermark lasts 3,630 seconds

- **GIVEN** revision 1 of a namespace first accepted at time `T` and then withdrawn
- **WHEN** a validly signed revision 1 of that namespace with different content and a valid lifetime arrives at `T + 3,629`
- **THEN** it is rejected
- **AND** the same offer arriving at `T + 3,630` or later is accepted, and the registry then holds one entry for the namespace

### Requirement: Only a complete, valid, single-snapshot manifest withdraws that peer's cached offers

A receiver SHALL withdraw a cached offer because of a manifest only when all of the following hold (book line 234):

- the manifest came from the peer named by the cached offer's `peer_id`, authenticated by libp2p as that peer;
- the cached offer is in the model scope that the receiver requested;
- the receiver fetched the pages in order from offset 0, following `next` until a page whose `next` is `null`;
- every offer on every page was valid and was accepted or idempotently replayed by the registry. A rejected offer, a page with more than 8 offers, a page that does not decode, a failed or timed-out request, or a `next` that is not the previous offset plus 8 makes the manifest partial;
- the pages come from one snapshot of the peer's manifest. A one-page manifest is one snapshot. Pages of a longer manifest SHALL count as one snapshot only when every page after the first contains at least one offer and every offer on every page carries the same revision;
- the cached offer is not among the offers received in that manifest.

A partial or invalid manifest SHALL NOT withdraw any offer (book line 234). A manifest SHALL NOT withdraw another peer's offers or offers outside the requested model scope. Valid offers received in a partial manifest MAY still be inserted under the registry rules. The pending target-design change `specify-p2p-provider-transport` applies this withdrawal rule to `/chiral/inference/manifest/2` (*Upgraded nodes serve and query only the v2 manifest protocol*) and treats an exchange that a relay refused or cut as partial (*Discovery of relay-only providers fits relay budgets*).

#### Scenario: A complete manifest withdraws an omitted offer

- **GIVEN** a long-running DHT service that cached peer `P`'s offers for deployments `gpu-a` and `gpu-b`
- **WHEN** `P` withdraws `gpu-a` and a later search receives `P`'s complete one-page manifest containing only `gpu-b`
- **THEN** the cached `gpu-a` offer is withdrawn and only `gpu-b` is returned

#### Scenario: A manifest that straddles a republish withdraws nothing

- **GIVEN** a relay that caches peer `P`'s 9 offers at revision 5
- **WHEN** the relay fetches page 0 at revision 5, `P` republishes all 9 offers at revision 6, and the relay then fetches page 8 at revision 6
- **THEN** no cached offer of `P` is withdrawn
- **AND** every one of `P`'s 9 namespaces is still in the relay's results, at revision 5 or 6

#### Scenario: A manifest that shrank between pages withdraws nothing

- **GIVEN** a relay that caches peer `P`'s 9 offers, and page 0 returned 8 offers with `next` 8
- **WHEN** `P` stops publishing one model and page 8 then returns no offers with `next` `null`
- **THEN** no cached offer of `P` is withdrawn

#### Scenario: An invalid offer makes the manifest partial

- **GIVEN** a relay that caches peer `P`'s offers `X` and `Y`
- **WHEN** `P`'s only page holds `X` and an offer with an invalid signature, with `next` `null`
- **THEN** `Y` is not withdrawn

#### Scenario: A manifest withdraws only its own peer and model scope

- **GIVEN** a long-running DHT service that caches `P`'s offers for models `m` and `n`, and `Q`'s offer for model `m`
- **WHEN** a search for model `m` receives `P`'s complete manifest for `m` that holds no offers
- **THEN** `P`'s offer for `m` is withdrawn
- **AND** `P`'s offer for `n` and `Q`'s offer for `m` remain

### Requirement: Expiry and provider withdrawal bound how long an offer stays visible

Every discovery result, lookup by offer ID and served manifest page SHALL exclude an offer whose `expires_at` is not later than the current time (book line 234). A provider that withdraws its own offer SHALL stop serving it in its manifest and SHALL withdraw it from its own registry at once. It SHALL stop advertising a model's index key when no local offer of that model remains, and the all-models index key when no local offer remains (book line 234). A provider's own offers SHALL leave its manifest when they expire.

Withdrawal SHALL NOT be described as an instant global revocation. Offers that other peers already cached disappear from them only through a later complete manifest from the provider, under *Only a complete, valid, single-snapshot manifest withdraws that peer's cached offers*, or through their signed expiry (book lines 234 and 638). Consumers SHALL still check expiry and negotiate current availability (book line 638).

#### Scenario: A cached offer leaves results when it expires

- **GIVEN** a registry holding an offer whose `expires_at` is `E`
- **WHEN** results are listed at time `E`
- **THEN** the offer is not listed and lookup by its offer ID finds nothing

#### Scenario: A provider's last withdrawal stops its index advertisements

- **GIVEN** a provider whose only local offer is for model `m`
- **WHEN** it withdraws that offer
- **THEN** its manifest serves no offer
- **AND** it stops providing both the all-models index key and the index key for `m`

### Requirement: The provider publishes fresh offers only while it can accept new funding

A provider SHALL keep one durable offer-revision counter for each signing wallet and DHT peer identity, under `<data_dir>/<network-hash>/<signing-wallet>/<peer-id>/` (book lines 236 and 472). It SHALL write and sync each new counter value before it signs an offer with that value, and SHALL never reuse or lower a revision, including across restarts. It SHALL refresh its offers nominally every 30 seconds (book lines 236 and 638). A refresh that finds the provider new-funding ready SHALL take a new revision from the counter and sign each offer it publishes with that revision, an `issued_at` equal to the refresh time and an `expires_at` 120 seconds later. It SHALL publish offers only for models currently acceptable for publication under `inference-commercial-terms` and the provider's qualification gates (book line 236). In explicit development mode, an offer for a model without a serving profile SHALL be kept in the provider's own registry and SHALL NOT be served in its manifest or announced through the DHT (book line 236).

The provider SHALL publish offers only from a refresh that found it new-funding ready (book lines 252 and 630). It SHALL stop serving every offer it published when a refresh finds it not new-funding ready, when a refresh fails, when it drains, and when a managed engine fails (book lines 630–632). When new-funding readiness closes between refreshes for another reason, such as a funding verification that finds the chain RPC unavailable, the offers already published MAY stay served until the next refresh, nominally 30 seconds later, withdraws them; meanwhile new quote proposals are refused (`inference-commercial-terms`, *Bounded public quote proposal is not owner authorization*). A refresh failure SHALL also withdraw the offers published earlier in the same failed refresh. The provider's administrative offer list (`GET /admin/offers`) SHALL list every offer that its manifest serves and no offer that it has withdrawn. Withdrawing or not publishing an offer SHALL NOT cancel an accepted funding acknowledgement or change an existing account (book line 638; `inference-account-topup-request`, *Top-up recovery preserves originals without new admission*).

#### Scenario: The revision continues after restart

- **GIVEN** a provider whose persisted revision counter is 41
- **WHEN** it restarts and its first refresh publishes offers
- **THEN** every offer of that refresh carries revision 42, and the persisted counter is 42 before any of them is signed

#### Scenario: One refresh publishes 120-second offers named by the peer identity

- **WHEN** a new-funding-ready provider with peer ID `P` refreshes at time `t`
- **THEN** each offer it publishes has `peer_id` `P`, `deployment_id` `P`, `issued_at` `t`, `expires_at` `t + 120`, and the same new revision

#### Scenario: A provider that cannot accept new funding serves no offers

- **GIVEN** a provider serving offers
- **WHEN** a refresh finds its chain RPC unable to report a `safe` block, so new-funding readiness is closed
- **THEN** its manifest serves no offer and its administrative offer list is empty
- **AND** existing accounts are unchanged

#### Scenario: A failed refresh withdraws the offers it published

- **GIVEN** a new-funding-ready provider with models `A` and `B`, where the offer for `B` cannot be published (for example, because its signed offer exceeds 16 KiB)
- **WHEN** a refresh publishes `A`'s offer and then fails on `B`
- **THEN** the provider's manifest serves neither `A`'s new offer nor any earlier offer
- **AND** `funding_ready` is false, the refresh error is reported, and the administrative offer list is empty

#### Scenario: A development model without a serving profile stays local

- **GIVEN** a development provider whose only model has no serving profile
- **WHEN** it refreshes
- **THEN** the offer is in the provider's own registry and its manifest serves no offer

### Requirement: Consumer discovery uses a fresh registry and exactly one configured source

Each native consumer discovery call SHALL verify offers in a new, empty registry, so that no revision comparison carries over from one call to another (book line 232). When the consumer profile configures `dht`, discovery SHALL use only the DHT. It SHALL NOT merge catalog offers into the result, and SHALL NOT fall back to `catalog_url` when DHT discovery fails or returns no offers; a DHT failure SHALL be returned as an error (book lines 242 and 464). Discovery SHALL use `catalog_url` only when `dht` is absent (book lines 242 and 462). When neither is configured, discovery SHALL fail with an error that names both options (book line 242).

A configured `catalog_url` SHALL keep its separate role whatever source discovery uses: native provider inspection reads reputation from it, and feedback is submitted to it. The DHT SHALL NOT carry reputation events or snapshots (book line 246). Without a `catalog_url`, inspection SHALL report reputation as unavailable and feedback submission SHALL fail with an error, while DHT discovery still works (book line 246). How reputation is admitted and computed is outside this capability.

#### Scenario: A DHT failure does not fall back to the catalog

- **GIVEN** a consumer profile that configures both `dht` and `catalog_url`
- **WHEN** the DHT service cannot start or its search fails
- **THEN** discovery returns that error
- **AND** the catalog's `/api/offers` is not requested

#### Scenario: An empty DHT result does not fall back to the catalog

- **GIVEN** a consumer profile that configures both `dht` and `catalog_url`, and a catalog that lists a valid offer
- **WHEN** the DHT search finds no offers
- **THEN** discovery returns an empty list and the catalog's `/api/offers` is not requested

#### Scenario: Neither source is configured

- **WHEN** a consumer profile configures neither `dht` nor `catalog_url` and discovery is requested
- **THEN** discovery fails with an error that names both options

#### Scenario: Each call starts with an empty registry

- **GIVEN** a discovery call that returned revision 7 of a namespace
- **WHEN** a later call receives only an unexpired revision 6 of that namespace
- **THEN** the later call returns revision 6

#### Scenario: Without a catalog, reputation is unavailable

- **GIVEN** a consumer profile with `dht` and without `catalog_url`
- **WHEN** the consumer inspects a discovered offer and then submits feedback for an account
- **THEN** inspection reports reputation as unavailable with its reason
- **AND** feedback submission fails because no reputation catalog is configured

### Requirement: The relay catalog serves bounded pages of its verified registry

A relay SHALL serve `GET /api/offers` with the optional query members `offset`, `limit` and `model` (book line 244). `limit` SHALL default to 50 and SHALL be 1–100. `offset` SHALL default to 0 and SHALL be at most 8,192. `model`, when present, SHALL be 1–256 bytes and SHALL match model IDs exactly. A request outside these bounds SHALL be refused with HTTP 400. The response SHALL be `{"offers": [<signed offer>, ...], "next_offset": <number or null>}`. It SHALL list the unexpired offers of the relay's verified registry in the model scope, in namespace order, exactly as signed. `next_offset` SHALL be `offset + limit` when more offers remain and `null` otherwise (book line 244).

The relay SHALL refresh its registry through a DHT search over all models every 15 seconds (book line 244). Its pages SHALL be described as a view of a registry that can change between requests, not as a snapshot-isolated or exhaustive crawl (book line 244).

#### Scenario: Pages and the next offset

- **GIVEN** a relay registry with 120 unexpired offers
- **WHEN** a client requests `/api/offers`, and then `/api/offers?offset=100&limit=50`
- **THEN** the first response holds 50 offers and `next_offset` 50
- **AND** the second holds 20 offers and `next_offset` `null`

#### Scenario: Out-of-range pages are refused

- **WHEN** a client requests `limit=0`, `limit=101`, `offset=8193` or an empty `model`
- **THEN** the relay answers HTTP 400 and lists no offers

#### Scenario: The model filter is exact

- **GIVEN** a relay registry with offers for models `m` and `m2`
- **WHEN** a client requests `/api/offers?model=m`
- **THEN** only the offers for `m` are listed

### Requirement: The native catalog client verifies each entry and bounds pagination

The native consumer SHALL read the catalog in pages of 50 from offset 0, with the requested model filter (book line 244). It SHALL decode and verify each entry independently. An entry that does not decode, fails verification or is outside the model filter SHALL be skipped without rejecting the rest of its page. The consumer SHALL fail discovery when a page holds more than 50 offers, when a response exceeds its response-size bound, when `next_offset` is not greater than the current offset, and when an offset repeats (book line 244). It SHALL read at most 100 pages. When the catalog still offers another page after 100 pages, discovery SHALL fail with an error that asks the user to narrow the model filter, and SHALL NOT return the offers read so far as a complete result (book line 244). The catalog origin is governed by the outbound-origin rules, which are outside this capability.

#### Scenario: An invalid entry is skipped

- **GIVEN** a catalog page with one entry that does not decode as a signed offer and two valid entries
- **WHEN** the native consumer reads it
- **THEN** the two valid offers are returned

#### Scenario: Pagination that does not advance fails

- **WHEN** a catalog page at offset 50 returns `next_offset` 50, or 0
- **THEN** discovery fails with a pagination error and returns no offers

#### Scenario: An oversized page fails

- **WHEN** a catalog page holds 51 offers
- **THEN** discovery fails and returns no offers

#### Scenario: More than 100 pages asks for a narrower filter

- **GIVEN** a catalog that answers every page with 50 valid offers and `next_offset` equal to the offset plus 50
- **WHEN** the native consumer discovers
- **THEN** it reads 100 pages and fails with an error that asks the user to narrow the model filter

### Requirement: Provider liveness and readiness are point-in-time observations, separate from new-funding readiness

The provider public router SHALL answer `GET /healthz` with process liveness, `{"status":"live"}`. It SHALL answer `GET /readyz` with a JSON body containing `ready` and `funding_ready`, with HTTP 200 when `ready` is `true` and HTTP 503 otherwise (book lines 252 and 636). `ready` SHALL report serving readiness and `funding_ready` SHALL report new-funding readiness, separately (book line 252). HTTP 200 SHALL NOT be read as permission to accept fresh money (book line 636). The pending target-design change `specify-provider-launch-reputation` adds identity members to this body (*Provider readiness names the serving identity*); this requirement does not forbid them.

`ready` SHALL be `false` while the provider drains, after the configured network's chain evidence was rejected, while a managed engine has failed, while no configured model passes its readiness probes, and while the provider ledger cannot be read (book lines 252, 631 and 632). A temporary chain or RPC outage SHALL make `funding_ready` `false` and SHALL NOT by itself make `ready` `false` (book lines 252 and 630).

Readiness SHALL be provider-wide. It SHALL NOT be described as proof that a given model has a free slot; model qualification and concurrency SHALL be checked again when a request is admitted (book line 252). Readiness responses are unsigned. They SHALL NOT change signed prices, account balances or agreement terms (book line 254). An offer is a signed proposal made at its issue time, not a health certificate (book line 252).

#### Scenario: A chain outage closes only new-funding readiness

- **GIVEN** a ready provider with existing funded accounts
- **WHEN** its chain RPC becomes unreachable and the next refresh runs
- **THEN** `GET /readyz` answers HTTP 200 with `ready` `true` and `funding_ready` `false`
- **AND** `GET /healthz` answers `{"status":"live"}`

#### Scenario: A draining provider is not ready

- **WHEN** the operator drains the provider
- **THEN** `GET /readyz` answers HTTP 503 with `ready` `false` and `funding_ready` `false`

#### Scenario: A failed engine makes the provider not ready

- **GIVEN** a provider whose only model runs on a managed engine
- **WHEN** that engine process exits
- **THEN** `GET /readyz` answers HTTP 503 with `ready` `false`

### Requirement: Native provider inspection reports observations separately and changes no terms

Native provider inspection (`provider_info`) SHALL first verify the selected offer for the selected network under the registry's offer rules, and SHALL refuse an invalid or expired offer without contacting its endpoint (book line 254). It SHALL then request the offer endpoint's `/healthz` and `/readyz` under the outbound-origin rules, and SHALL return `observed_at`, a health observation, a readiness observation and a separately fetched reputation result (book line 254). The readiness observation SHALL expose `ready` and SHALL NOT present it as new-funding readiness; a client that needs `funding_ready` SHALL read the provider's `/readyz` itself (book line 254). A provider that cannot be reached SHALL be reported as unreachable in these observations, not as a failure of the whole inspection. Inspection SHALL NOT change the offer or any quote, account or agreement (book line 254).

How the desktop shows these observations is governed by `desktop-gui-pages` (pending `specify-desktop-gui-pages`, *The Marketplace discovers, inspects and funds in one gated order*). The pending target-design change `specify-provider-launch-reputation` adds a `launch_reputation` member (*Native provider inspection reports the launch reputation separately*).

#### Scenario: An expired offer is refused before contact

- **WHEN** inspection is requested for an offer whose `expires_at` has passed
- **THEN** inspection fails and no request is sent to the offer's endpoint

#### Scenario: A ready provider whose new funding is closed

- **GIVEN** a provider whose `/readyz` answers HTTP 200 with `ready` `true` and `funding_ready` `false`
- **WHEN** a consumer inspects one of its offers
- **THEN** the result has `observed_at`, health live, readiness `ready` `true`, and a separate reputation result
- **AND** nothing in the readiness observation states that new funding is open

#### Scenario: An unreachable provider

- **GIVEN** an offer whose endpoint does not accept connections
- **WHEN** a consumer inspects it
- **THEN** inspection succeeds with health and readiness reported as not reachable
