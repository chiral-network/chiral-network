# Chiral Network

Chiral Network is a decentralized marketplace for cloud resources. Independent providers sell **storage**, **containerized compute**, and **LLM inference**; consumers discover providers without a central operator, use them over standard HTTP APIs, and pay in a native proof-of-work currency. It runs as a desktop application (Tauri 2 + Svelte 5 + Rust) and as a headless daemon for running providers and for automated testing.

This book is in two parts:

- **[Part I: White Paper](#part-i-white-paper)** — the conceptual design: what the system guarantees and why, at the level of mechanisms rather than code, with no implementation detail.
- **[Part II: Design and Implementation](#part-ii-design-and-implementation)** — the concrete realization: architecture, the offer record, the resource interfaces, settlement, reputation, APIs, deployment, and operations.

---

# Part I: White Paper

*Chiral Network: A Peer-to-Peer Market for Cloud Resources*

**Abstract.** Cloud computing is delivered almost entirely by central operators who set prices, hold the data, and constitute a single point of failure and censorship. Decentralized alternatives exist for narrow slices — a network for storage here, a market for GPU time there — but each is a silo with its own token and its own trust assumptions. Chiral Network is a single market in which heterogeneous resources — bytes at rest, running containers, model inference — are advertised, discovered, and paid for under one identity and one currency. Providers advertise offers signed by their wallet keys, so a listing's capacity, price, and payment address cannot be forged or redirected by whichever node relays it. Consumers reach providers directly over standard interfaces — an S3-compatible API for storage, an OpenAI-compatible API for inference, a submission API for containers — and pay by funding a prepaid balance that the provider draws down as it meters usage. The network verifies cryptographically that money moved and to whom, but deliberately does not try to prove that a meter was fair; instead it makes dishonesty expensive, because only a consumer with an on-chain-verified payment to a provider may rate it, and that rating is weighted by what was spent. The result is a market where pricing is left to providers, honesty is disciplined by reputation rather than by a referee, and no intermediary can forge, reprice, or redirect what providers publish.

## 1. Introduction

The economics of the cloud are the economics of a landlord. A handful of operators own the storage, the compute, and the accelerators; they set the prices, meter the usage, keep the logs, and can revoke access at will. Tenants accept this because the alternative — assembling reliable infrastructure from strangers — has historically been impossible to make safe. Decentralized projects have chipped at individual pieces (distributed storage, volunteer compute, GPU spot markets), but each solves one resource in isolation, under its own currency and its own assumptions, and none offers the thing a tenant actually wants: one place to rent whatever it needs, from whoever will sell it cheapest, without trusting a central operator to be fair.

What makes a single, operator-free market hard is not discovery or payment — public-key identity and a mined currency solve those — but *metering*. An atomic file download can be escrowed: pay the exact price, receive the exact bytes, verify the hash. A month of storage, an hour of a running container, or a million tokens of inference cannot be escrowed per unit without a machinery of proofs that does not yet exist in practice. Chiral Network makes this trade explicit rather than pretending otherwise. It keeps everything that public-key cryptography can guarantee — that a listing is authentic, that a payment reached the right wallet, that a record cannot be hijacked — and for the part that cryptography cannot cheaply guarantee, fair metering and honest service, it substitutes an incentive: bind every payment to an identity, let only paying customers rate a provider, and weight their verdicts by what they spent. A provider that over-meters or under-delivers then loses future revenue faster than it can defraud its present customers.

The system combines four mechanisms:

1. a **content-addressed discovery layer** in which every resource offer is signed by its provider's key;
2. **direct provider interfaces** over standard HTTP APIs, so a consumer's existing tools work unchanged;
3. a **native proof-of-work currency** with prepaid, provider-drawn settlement; and
4. a **payment-gated reputation function** that admits subjective judgment without admitting forgery.

## 2. System Overview

The network is organized as three planes that share one identity system.

**Discovery plane.** A Kademlia distributed hash table stores small signed records — *resource offers*: a provider announcing that it sells a given class of resource, at a given price, reachable at a given endpoint. Any peer may store and serve these records; none is trusted to have authored them.

**Interaction plane.** Once a consumer has chosen a provider, it talks to that provider's public endpoint **directly**, over a standard HTTP API appropriate to the resource. There is no protocol-level intermediary between consumer and provider on the data path. In v1 a provider must be publicly reachable — a real IP address, ideally a domain name with TLS — so this plane requires no relayed reachability or NAT traversal.

**Settlement plane.** A proof-of-work blockchain (Ethash, account-based) carries the native currency, CHI. A consumer funds a prepaid balance by paying a provider on-chain; the provider verifies the payment against the chain and draws the balance down as it meters usage. Miners issue currency and order transactions.

**Identity.** A participant's identity is a single ECDSA keypair — the same key that controls its currency balance signs its published offers. The entity that *gets paid* for a resource is cryptographically the entity that *advertised* it, and reputation attaches to the address that actually receives money. A peer's network-transport identity (its DHT node ID) is distinct, but every offer binds the two by signature.

Participants play four roles, in any combination: **providers** advertise resources and set prices; **consumers** discover, use, and pay for them; **miners** secure settlement and earn block rewards; and **bootstrap/relay operators** provide the DHT entry points and host shared services such as the reputation registry.

## 3. Signed Offers as the Trust Primitive

The foundational rule is unchanged from a content network and applies just as well to a resource market: *data is trusted because of who signed it, never because of where it came from.* A DHT is an adversarial place — any node can claim to hold any key and answer with anything — so Chiral Network treats it purely as an untrusted bulletin board.

Every long-lived record carries an ECDSA signature by the wallet that owns it. The central record is the **resource offer** — "wallet *W* sells resource class *C*, with capacity/spec *S*, at price schedule *P*, reachable at endpoint *E*" — signed by *W*. Because the endpoint and the price live *inside* the signed payload, a relaying node cannot substitute its own endpoint to intercept traffic, nor its own price or wallet to divert payment.

Two symmetric rules enforce the contract. *Writers refuse to publish unsigned:* a client that cannot sign (its key locked or absent) declines to write rather than emit an unverifiable record. *Readers drop invalid:* a record whose signature is missing or wrong is treated as nonexistent, however plausible its contents.

Signatures are computed over a canonical, length-prefixed, domain-tagged encoding of the record's fields. Length prefixing makes the serialization injective, and the domain tag prevents a signature produced for one record type from being replayed as another. Records under a given key are owned by first claim: only the wallet that first wrote a key can overwrite it. And because authenticity lives in the record, a reader may act on the *first* replica whose signature verifies rather than waiting for a replication quorum — collapsing lookup latency without weakening integrity.

## 4. Resources and Discovery

A resource offer names, at minimum: the provider's wallet, the resource **class** (storage, container, or inference), a capacity or capability descriptor, a price schedule in CHI per metered unit, a public endpoint, and a validity window. It is signed by the provider's wallet.

To publish, a provider writes its signed offer under a key derived from the resource class and its own identity, and refreshes it periodically. To discover, a consumer queries by class, collects the offers, discards any whose signatures fail, and ranks the survivors by price and by the provider's reputation (Section 7). Offers expire if not refreshed, so the catalog self-cleans: a provider that goes offline simply stops refreshing and drops out, rather than lingering as a dead listing.

Discovery is deliberately thin. It tells a consumer *who* is selling *what*, at *what price*, *where* — and nothing that must be trusted, because everything a consumer acts on is either signed by the provider or verified directly against the provider's endpoint once contact is made.

## 5. The Three Resource Classes

The planes above — identity, discovery, settlement, reputation — are shared. The classes differ only in the interface a consumer speaks and the unit a provider meters.

**Storage.** A provider exposes an **S3-compatible object interface**: buckets and objects, `PUT`/`GET`/`DELETE`, listing, and presigned URLs. Usage is metered as capacity stored over time (GB-month) plus egress (GB transferred). An object may be addressed by the hash of its content, which makes storage self-certifying — a consumer checks what it retrieves against the name it asked for — and subsumes the older use case of sharing a file by its hash: a shared file is simply a public-read object. (The bespoke peer-to-peer file transports of earlier designs — a custom chunked protocol, BitTorrent, magnet links — are retired in favor of the standard S3 interface.)

**Containerized compute.** A provider advertises CPU, memory, and optional GPU capacity at a per-hour price. A consumer submits a container specification — an image reference, a resource request, ports, and environment — and receives an endpoint at which the running container is reachable. Usage is metered as runtime; the consumer tears the container down to stop billing.

**LLM serving.** A provider advertises the models it hosts and a price per thousand input and output tokens, and exposes an **OpenAI-compatible HTTP API**. A consumer sends inference requests with existing tooling; the provider meters tokens consumed.

Each class admits its own honesty checks (Section 9): storage can in principle be challenged for retrievability, compute for liveness, inference for latency — but the quality of what is served (was the model good? was the container stable?) is a subjective judgment, which the reputation system is designed to carry.

## 6. Settlement

Payment precedes service, as before — but for a metered resource "service" is continuous rather than a single delivery, so payment takes the form of a **prepaid balance** rather than a per-transfer transaction.

A consumer funds a balance by paying the provider's advertised address on-chain and presenting the transaction. The provider verifies the payment directly against the chain — that it is mined, that the recipient is its own address, that the amount is what it credits, and that the transaction was made on this network's chain (the chain-identifier check rejects cross-chain replays) — and credits the balance. As the consumer uses the resource, the provider meters usage and draws the balance down at its published rates. When the balance is exhausted, service pauses until the consumer tops it up.

Balances are **non-refundable**: a consumer funds what it intends to spend and tops up incrementally, rather than parking a large deposit it must later reclaim. This keeps settlement to a single primitive — an on-chain payment the provider verifies — with no escrow, no refund path, and no withdrawal protocol. From each funded payment a small platform fee (0.5% by default, adjustable down to a 0.1% floor) is split off with exact integer arithmetic, so the provider's credit and the fee sum precisely to the amount paid; there is no floating-point rounding anywhere in the settlement path.

This is the deliberate trade of Section 1 made concrete. The network proves that money moved and to whom; it does not prove that the meter was fair. What disciplines the meter is that the consumer watches its balance drain against *observable* usage — objects it can list, tokens returned in responses, container uptime it can measure — and that any discrepancy feeds a reputation score the provider cannot afford to lose. Resources offered at a rate of zero skip settlement entirely.

## 7. Reputation

Consumers choosing among providers need a signal of reliability, and for a resource market that signal is unavoidably subjective — was the storage durable, the model useful, the container stable? An earlier, content-only version of this network computed reputation *only* from objective transfer outcomes and deliberately **rejected** subjective ratings, on the grounds that ratings are free to fabricate. A resource market cannot do without subjective judgment, so Chiral Network admits user feedback but **gates** it: only a wallet with an on-chain-verified payment to a provider may rate that provider, and each rating is weighted by the amount paid and by how recently.

This preserves exactly the property the old rejection was protecting. Fabricating a favorable history still costs real, fee-bearing payments — a provider that wants to buy itself a reputation must pay itself through wallets whose payments are enumerable on-chain, losing the platform fee on every wash and moving the score only in proportion to money actually spent. The single change is that the admitted signal is now a *judgment* (a score) rather than a *completion bit*; its resistance to forgery is identical, because it rests on the same verified-payment gate.

Each wallet carries a score on a 0–100 scale, initialized to 50, updated per event in the style of the Elo rating system. For an event with score-outcome $S \in [0,1]$ (the normalized user rating) against a provider whose current score is $r$:

$$E = \frac{1}{1 + 10^{(50 - r)/12}}, \qquad r \leftarrow \mathrm{clamp}\big(r + K\,(S - E),\ 0,\ 100\big), \qquad K = 4\,w_t\,w_a$$

The expected-outcome term $E$ gives the update curvature: a high-scored provider gains little from another good review but loses sharply from a bad one. The time weight $w_t$ decays linearly to zero over a 180-day lookback, so reputation reflects recent conduct. The amount weight $w_a = 1 + \min(1, \ln(1+a)/\ln 51)$, where $a$ is the CHI paid, lets larger verified payments move the score up to twice as much as trivial ones, growing only logarithmically so reputation cannot be bought in one large transaction. Scores are displayed alongside offers in discovery, so reliability directly affects a provider's ability to win business.

## 8. Incentives

Every behavior the network needs is paid for; none relies on altruism.

**Providing.** Providers earn their published metered rates on every unit of resource sold. Pricing is theirs to set; the market and the reputation score, not a central schedule, discipline it.

**Mining.** Proof-of-work miners earn a fixed block reward of 5 CHI, which is also the currency's issuance mechanism, giving new participants a path to acquiring CHI by contributing computation rather than only by selling resources. Transactions carry no gas price — settlement is free to users, and the chain's security budget is the block reward alone.

The loop closes: consumers obtain CHI by mining or by themselves providing; they spend it renting resources; providers earn it for supplying them; miners earn it for ordering everyone's settlement.

## 9. Trust Model

It is as important to state what the network does *not* protect as what it does.

**Out of scope: anonymity.** Chiral Network is not an anonymity network. Wallet addresses, provider endpoints, IP addresses, and which resources a peer offers or consumes are observable, as in any unencrypted system. The guarantees below are of *integrity and authenticity*, not of unlinkability.

**In scope (cryptographic).** The signed-record discipline of Section 3, applied end to end, yields:

- **No forged listings.** A record not signed by its owner is dropped by every reader. Discovery can lie about *availability* but not about a provider's price, endpoint, or payee.
- **No payment redirection.** The payee lives inside the signed offer; diverting payment requires the provider's private key.
- **No replay.** Cross-chain replay of a funding transaction is rejected by the chain-identifier check; double-crediting of the same transaction is prevented by a spent-transaction ledger; signature malleability is eliminated by accepting only canonical (low-*s*) signatures.
- **No name hijacking.** First-claim-wins ownership means an existing offer is replaceable only by its original signer.

**Rests on reputation (the honest-provider assumption).** The network does *not* cryptographically guarantee that a provider meters fairly, that stored bytes remain retrievable, that a container keeps running, or that inference is what was advertised. These rest on the incentive of Section 7: a provider that takes a balance and cheats can do so once per victim, the loss bounded by what that victim pre-funded, and the conduct is recorded against the wallet that got paid, pricing its future business accordingly.

**v1 constraints.** Providers must be publicly reachable — there is no NAT traversal for providers in v1, so a would-be storage or compute seller needs its own public IP (ideally a domain with TLS). Prepaid balances are non-refundable.

## 10. Limitations and Future Work

- **Trustless metering.** The honest-provider assumption is the largest gap. Future work: proofs of retrievability for storage, verifiable or attested compute, signed token-count receipts for inference, and escrow contracts that release funds only against consumer-signed usage receipts.
- **Refunds and escrow.** Non-refundable prepaid is the v1 simplification; a withdrawal/escrow protocol would let consumers reclaim unused balance and reduce the trust placed in providers.
- **NAT'd providers.** Requiring public reachability excludes home providers; a relay/circuit path for providers is deferred.
- **Dispute resolution.** Beyond the reputation penalty, there is no arbitration of a contested charge.
- **Currency and registry.** Proof-of-work settlement is only as strong as the honest hash-power share, modest on a young chain; and the reputation registry is currently a trusted aggregator that tallies verified events (Part II).

## 11. Conclusion

Chiral Network treats cloud resources the way a content network treated files: as goods in a market, named and priced by their owners, discovered without an operator, and paid for in a mined currency. Where cryptography can enforce a guarantee — authenticity of a listing, destination of a payment, ownership of a name — it does. Where it cannot cheaply do so — the fairness of a meter, the quality of a service — the network does not pretend, and instead makes dishonesty a losing trade by binding reputation to verified payment. What remains is a market in which anyone with spare storage, compute, or a served model can sell it, anyone can buy it, and neither side has to trust an intermediary that could forge, reprice, or redirect what the other publishes.

---

# Part II: Design and Implementation

This part covers the concrete realization of Chiral Network — architecture, the offer record, the resource interfaces, settlement, reputation, APIs, deployment, and operations. For the conceptual design (what the system guarantees and why), read [Part I](#part-i-white-paper) first.

> **Current scope (v1).** Three resource classes — **S3-compatible storage**, **container compute**, **LLM serving**. Discovery is via signed DHT **resource offers**; the blockchain is the **settlement** layer only. Payment is a **prepaid, non-refundable balance** the provider draws down as it meters. Providers must be **publicly reachable** (own IP, ideally a domain + TLS); there is **no NAT traversal** for providers in v1. Reputation is **payment-gated user feedback** running on the existing Elo engine.
>
> **Superseded.** The earlier file-sharing transports — the bespoke chunked libp2p protocol, BitTorrent, magnet/`.torrent` — are retired; storage now speaks S3 over HTTP. Several primitives are **reused and generalized** rather than rebuilt: the signed-record discovery discipline, the wallet and on-chain payment verification, the platform-fee split, the Elo reputation engine, version enforcement, and the headless daemon / CLI / relay. This section flags forward-looking (not-yet-built) mechanics with a **Status** note.

---

## Table of Contents

- [Architecture](#architecture)
- [Resource Offers (Discovery)](#resource-offers-discovery)
- [Resource Interfaces](#resource-interfaces)
- [Service Contracts and the Handshake](#service-contracts-and-the-handshake)
- [Wire Protocol & API Reference](#wire-protocol--api-reference-contract-spine)
- [Data-Plane API: Storage (S3)](#data-plane-api-storage-s3)
- [Data-Plane API: Compute (Containers)](#data-plane-api-compute-containers)
- [Data-Plane API: Inference (LLM)](#data-plane-api-inference-llm)
- [Reputation System](#reputation-system)
- [Design Decisions](#design-decisions)
- [Provider Implementation](#provider-implementation)
- [Implementation Plan (Milestones)](#implementation-plan-milestones)
- [Identity and Wallet](#identity-and-wallet)
- [Blockchain and Mining](#blockchain-and-mining)
- [Version Enforcement](#version-enforcement)
- [Application Surface](#application-surface)
- [Backend Modules](#backend-modules)
- [Headless Mode and CLI](#headless-mode-and-cli)
- [Security Implementation](#security-implementation)
- [Deployment](#deployment)
- [Getting Started](#getting-started)
- [Testing](#testing)
- [Configuration](#configuration)

---

## Architecture

The application consists of three layers:

1. **Frontend** — Svelte 5 with TypeScript, rendered in a Tauri webview or browser: the marketplace UI, a provider dashboard, wallet, and mining controls.
2. **Backend** — Rust, handling P2P discovery (libp2p Kademlia), blockchain interaction (Geth), the provider-side resource interfaces, settlement accounting, and local storage.
3. **Blockchain** — a private Ethash proof-of-work chain (chain ID 98765) where users mine CHI and fund provider balances.

### Tech Stack

| Layer | Technology | Version |
|-------|-----------|---------|
| Desktop shell | Tauri | 2.x |
| Frontend framework | Svelte | 5.38 |
| Frontend language | TypeScript | 5.7 |
| Build tool | Vite | 7.1 |
| Styling | TailwindCSS | 3.4 |
| Backend language | Rust | 2021 edition |
| P2P discovery | libp2p | 0.53 |
| HTTP server | Axum | 0.7 |
| Blockchain client | Core-Geth | 1.12.20 |
| Crypto | ethers.js (frontend), secp256k1 + ed25519-dalek (backend) |

### Planes

```
+-------------------------------------------------------------+
|  DISCOVERY  — libp2p Kademlia DHT (untrusted bulletin board)|
|  signed resource offers: chiral_offer_<class>_<wallet>      |
+-------------------------------------------------------------+
          |  (verify signature, rank by price + reputation)
          v
+-------------------------------------------------------------+
|  INTERACTION — direct consumer -> provider HTTP(S)          |
|   storage: S3-compatible   compute: submit/lifecycle        |
|   inference: OpenAI-compatible                              |
|   (providers are publicly reachable; no NAT traversal)      |
+-------------------------------------------------------------+
          |  (meter usage, draw down balance)
          v
+-------------------------------------------------------------+
|  SETTLEMENT — Ethash PoW chain (CHI), RPC localhost:8545    |
|   prepaid balance funded on-chain; provider verifies + credits |
+-------------------------------------------------------------+
          |
          v
+-------------------------------------------------------------+
|  RELAY / BOOTSTRAP  130.245.173.73                          |
|   :4001 libp2p (DHT bootstrap)   :8080 HTTP (reputation,    |
|   version policy)   — no provider NAT relay in v1           |
+-------------------------------------------------------------+
```

### Data Flow: Renting a Resource

1. **Discover.** Consumer searches the DHT for offers of a class (e.g. `storage`), verifies each offer's signature, drops invalid/expired ones, and ranks survivors by price, provider Elo, and region.
2. **Handshake.** Consumer contacts the chosen provider's endpoint and proposes terms referencing the offer; the provider confirms the exact terms and returns a fresh anti-replay `nonce`.
3. **Contract on-chain.** Consumer builds a **contract transaction** — recipient = provider wallet, `value` = the amount to prepay, `data` = the agreed contract commitment (offer reference, terms hash, nonce) — signs it, and broadcasts it. The signed tx is an immutable, timestamped record of the deal.
4. **Verify + open.** Consumer hands the `tx_hash` to the provider; the provider verifies it on-chain (recipient, value, decoded terms, `from` = consumer, chain ID), records `(tx_hash, provider)` in its contract ledger, credits the balance (`value − platform fee` via `split_payment`), and opens the contract.
5. **Use.** Consumer uses the resource over the provider's HTTP API (S3 / container / OpenAI-compatible), authenticated to the contract.
6. **Meter + draw down.** Provider meters usage and decrements the balance at the contract's rates, reporting usage back to the consumer. When the balance runs low the consumer tops up (a further payment referencing the contract); when it hits zero, service pauses.
7. **Rate.** The consumer submits a payment-gated rating keyed to the contract tx; the provider's Elo updates.

---

## Resource Offers (Discovery)

> **Status: the offer record generalizes today's signed host-advertisement machinery (`hosting.rs`, `hosting/publish-ad`, `hosting/registry`) from a single "hosting" type to a typed, multi-class offer.** The signing, publish/refresh, and read-verify paths are reused. (`main` has moved substantially since this was drafted — reconcile exact symbol names at implementation time.)

A resource offer is a signed DHT record. The signed payload is a canonical, length-prefixed, domain-tagged (`chiral-offer-v1`) encoding of:

| Field | Meaning |
|-------|---------|
| `provider_wallet` | secp256k1 address; the payee, reputation subject, and offer signer |
| `resource_class` | `storage` \| `container` \| `inference` |
| `capacity` | class-specific descriptor (below) |
| `price_schedule` | class-specific CHI-per-unit rates (below) |
| `endpoint` | public base URL of the provider's HTTPS API (host + optional port) |
| `region` | optional locality hint (e.g. `us-east`) |
| `min_funding` | smallest contract the provider will open (CHI) |
| `offer_nonce` | monotonic; lets the provider supersede a prior offer under first-claim ownership |
| `valid_until` | Unix-seconds; readers ignore expired offers |
| `signature` | secp256k1 over the canonical payload above |

**Class-specific `capacity` / `price_schedule`:**

| Class | `capacity` | `price_schedule` |
|-------|-----------|------------------|
| `storage` | `gb_available`, `max_object_bytes` | `per_gb_month`, `per_gb_egress` |
| `container` | `cpu_cores`, `mem_gb`, `gpu_model?`, `gpu_count?` | `per_vcpu_hour`, `per_gb_mem_hour`, `per_gpu_hour?` |
| `inference` | `models: [{id, context_len, quantization?}]` | per-model `per_1k_input_tokens`, `per_1k_output_tokens` |

- **DHT keys.** Each offer is stored at `chiral_offer_<class>_<wallet>`; the provider also registers as a Kademlia provider under a per-class index (`chiral_offers_<class>`) so consumers can enumerate sellers of a class. Reserved namespaces reject raw `dht_put` (403) — offers are writable only through the signed publish command.
- **Publish / refresh / expire.** A provider republishes on a short interval (≈ every 2–3 min) with `valid_until` a few minutes out; one that stops refreshing falls out of the catalog. First-claim ownership means only the provider's wallet can overwrite its own offer key.
- **Discovery + ranking.** `search_offers(class)` resolves the class index, fetches each offer, verifies signatures, drops unsigned/invalid/expired, attaches provider Elo via batch reputation lookup, and ranks by price × reputation (with an optional region filter). The first signature-valid replica may be used without waiting for quorum convergence.
- **Relay aggregation (convenience, advisory).** The relay may cache a verified per-class offer list (mirroring today's host registry) so light clients get a fast catalog; clients re-verify every signature, so the cache is never trusted — it only accelerates discovery.

---

## Resource Interfaces

Each class is a standard HTTP API the provider serves over HTTPS at its offer's `endpoint`. A consumer authenticates to the API **against its open contract** (see [Service Contracts](#service-contracts-and-the-handshake) and the auth decision in [Design Decisions](#design-decisions)); the desktop app ships a client for each class, and existing third-party tooling works via the **contract-scoped session credential** the provider issues when the contract opens (an S3 key / OpenAI bearer / container token bound to `contract_id`); a local wallet-signing proxy is available for users who want no shared secret.

### Storage — S3-compatible

> **Status: new provider-side server (may wrap an existing S3 implementation, e.g. MinIO, behind the contract/auth layer).**

- **API.** S3 object semantics: `PUT`/`GET`/`HEAD`/`DELETE` object, multipart upload for large objects, `GET` bucket (list), and presigned URLs for time-boxed anonymous access. Path-style and virtual-host-style addressing.
- **Namespacing.** One bucket namespace per contract (seeded by the contract tx hash); object keys are arbitrary UTF-8, up to the offer's `max_object_bytes`.
- **Integrity & content addressing.** The object ETag is its SHA-256, so a consumer verifies bytes against the name it requested. A public-read object addressed by its hash is the successor to "sharing a file by its hash"; such objects are served unauthenticated (presigned or public ACL).
- **Metering.** Capacity is sampled (GB-hours accumulated into GB-month); egress is metered on `GET` bytes; the balance is drawn down each interval at `per_gb_month` / `per_gb_egress`.
- **Exhaustion & durability.** At zero balance the provider stops accepting writes and (after a grace window — see [Design Decisions](#design-decisions)) may delete stored objects. **v1 is single-provider: durability is the consumer's responsibility** — replicate across providers client-side if you need it. The book states this risk plainly rather than hiding it.

### Containerized compute — hardened OCI

> **Status: specified; v1 reference provider runs OCI containers under a hardened Docker/Podman runtime.**

- **Submit.** `POST /containers` with `{ image, cmd?, env?, ports, resources{ vcpu, mem_gb, gpu? } }`; the provider pulls the image (from allowed registries), schedules it within the contract's resource cap, and returns a handle + a public endpoint (an assigned subdomain/port reverse-proxied to the container).
- **Isolation (both directions).** Containers run **non-privileged**, with user-namespace remapping, a seccomp/AppArmor profile, dropped capabilities, a read-only rootfs by default, no host bind-mounts, cgroup CPU/memory/PID caps, and an egress network policy. This protects the *provider's* host from hostile workloads; the *consumer* in turn trusts the provider not to introspect its container — a mutual-trust boundary that reputation, not cryptography, polices. MicroVM isolation (Firecracker/Kata) is the noted hardening path beyond v1.
- **Lifecycle.** `GET /containers/:id` (status, stats, logs), `DELETE /containers/:id` (stop — halts metering). The container is force-stopped when the balance is exhausted.
- **Persistence.** v1 rootfs is ephemeral; a container may mount a Storage-class bucket for durable state (composition across classes). Local persistent volumes are future work.
- **Metering.** Wall-clock runtime × the contract's `per_vcpu_hour` / `per_gb_mem_hour` / `per_gpu_hour`, billed per second and drawn down periodically.

### LLM serving — OpenAI-compatible

> **Status: specified; provider fronts one or more served models with a token meter.**

- **API.** Mirrors the OpenAI HTTP API: `GET /v1/models`, `POST /v1/chat/completions` (including streaming SSE), `POST /v1/completions`, `POST /v1/embeddings`. Standard OpenAI SDKs point at the provider's endpoint with the contract session as the bearer key.
- **Model selection.** The consumer sets `model` to an ID the offer advertises. A provider cannot cheaply *prove* it runs the claimed model rather than a smaller/quantized one — a reputation-policed quality property (see [Design Decisions](#design-decisions) for optional model attestation).
- **Metering.** Input + output tokens (the response `usage` block) at the model's `per_1k_input_tokens` / `per_1k_output_tokens`; streaming responses meter tokens as they emit. The consumer can approximately re-count with a local tokenizer to reconcile against balance drain.
- **Limits.** Per-contract concurrency and rate limits; requests are refused (not queued indefinitely) when the balance cannot cover the maximum possible completion.

---

## Service Contracts and the Handshake

> **Status: new. Built from the existing on-chain payment-verification primitives (wallet tx verification: mined + recipient + amount + chain-id, and the `(tx, …)` spent-tx ledger) plus the `split_payment` fee cut. No smart contract is introduced — the chain carries only signed, value-bearing transactions.**

A **service contract** is the agreement between a consumer and a provider, committed as a single signed on-chain transaction. The transaction *is* the contract: it moves the prepaid CHI to the provider and carries the agreed terms in its `data` field, so the deal is immutable, timestamped, and attributable to the consumer's wallet.

### The handshake

1. **Propose.** Consumer → `POST {endpoint}/contracts/propose` with the accepted `offer` reference and desired terms (`funding_amount`, class-specific parameters such as bucket name / resource shape / model). Over HTTPS to the provider's public endpoint.
2. **Quote.** Provider → returns the exact terms it will honor, a fresh `contract_nonce` (anti-replay), and the `terms_hash` to commit. It rejects proposals below the offer's `min_funding`.
3. **Commit on-chain.** Consumer builds the **contract transaction**:
   - `to` = `provider_wallet`, `value` = `funding_amount`
   - `data` = domain-tagged (`chiral-contract-v1`) canonical encoding of `{ offer_ref, terms_hash, contract_nonce }`
   - signs it with the consumer wallet and broadcasts it to the chain.
4. **Open.** Consumer → `POST {endpoint}/contracts/open { tx_hash }`. The provider verifies on-chain: mined for a large contract, or accepted optimistically from the mempool for a small one (see [Design Decisions](#design-decisions)), `to` = self, `value` = the quoted amount, `data` decodes to the quoted `terms_hash` + unused `contract_nonce`, `from` = the consumer, `chainId == geth::chain_id()`. It records `(tx_hash, provider)` in the contract ledger (one tx ⇒ one contract, no replay), credits the balance = `value − fee` (via `split_payment`; the fee — default 0.5%, 0.1% floor — is forwarded to the platform wallet), and returns the **contract handle** (`contract_id = tx_hash`) plus a **contract-scoped session credential** — an S3 access-key/secret, an OpenAI-style bearer, or a container token bound to `contract_id` — which is the handle standard tooling authenticates with (proof of contract control, not an independent grant of authority).

### Balance, metering, and top-ups

- **Drawdown.** The provider meters usage per class ([Resource Interfaces](#resource-interfaces)) and decrements the contract balance at the agreed rates. Metering is provider-side — the honest-provider assumption of [Part I §9](#9-trust-model) — and the provider reports running usage to the consumer (response headers / a `GET {endpoint}/contracts/:id` endpoint) so the consumer can reconcile against observable output.
- **Top-up.** When the balance runs low, the consumer sends a further payment transaction whose `data` references `contract_id`; the provider verifies it and adds to the same balance. No new handshake is needed.
- **Signed receipts.** On demand (`GET {endpoint}/contracts/:id/receipt`) and periodically, the provider returns a **usage receipt signed by its wallet** — `{contract_id, funded_wei, spent_wei, balance_wei, usage, as_of}` — so the consumer holds portable, attributable evidence of what it was charged. Reputation ratings and disputes cite these ([wire format](#wire-protocol--api-reference-contract-spine)).
- **Exhaustion.** At zero balance the provider pauses service (class-specific: storage may enter a read-only grace window; containers are stopped; inference is refused).
- **Non-refundable.** There is no withdrawal path. A consumer funds what it intends to spend and tops up incrementally.

### What binds — cryptographically vs by trust

- **Cryptographic:** the payee and amount (the tx), the consumer's identity (`from`), the agreed terms (`terms_hash` in `data`, matched against the provider's signed offer), and no replay/double-open (contract ledger + chain-id + low-`s`).
- **By reputation:** that the provider then *delivers* the metered resource and meters it honestly. A provider that takes a contract and cheats can do so once per victim, bounded by that victim's prepaid balance, and the conduct is recorded against the paid wallet.

---

## Wire Protocol & API Reference (Contract Spine)

> **Status: normative spec for v1 of the shared spine** — the signed encodings, the handshake, the contract transaction, and the session credential. The three class data-plane APIs (S3, container, LLM) get their own reference in a later pass. Reconcile exact symbol names with current `main` at implementation time.

### Conventions

- **Transport.** HTTPS to the provider's offer `endpoint`; control-plane paths live under `/v1/`. JSON bodies are `Content-Type: application/json; charset=utf-8`.
- **Hashing & signatures.** `keccak256` for all digests. Signatures are secp256k1 **recoverable**, low-`s` (EIP-2), 65 bytes `r ‖ s ‖ v` as `0x`-hex; the signer is recovered with `ecrecover` and must equal the claimed wallet (same primitive as `wallet::recover_signer`).
- **Hex & numbers.** Binary fields (addresses, hashes, nonces, signatures) are `0x`-prefixed lower-case hex. Wei amounts are decimal **strings** (they exceed JS safe-integer range).
- **Canonical byte encoding** for signed records:
  ```
  lp(b)                 = uint32_be(len(b)) ‖ b        # length-prefixed bytes
  jcs(obj)              = RFC-8785 canonical JSON (sorted keys, no space), UTF-8
  canonical(tag, f₁…fₙ) = lp(ascii(tag)) ‖ lp(f₁) ‖ … ‖ lp(fₙ)
  digest                = keccak256(canonical(…))
  ```
  Addresses are 20 raw bytes; `u128`/`u64` integers are fixed big-endian (16 / 8 bytes); structured sub-objects (`capacity`, `price_schedule`, `params`) are `jcs(...)` bytes. The domain tag makes a signature for one record type unusable as another.
- **Versioning.** Every request carries `X-Chiral-Client-Version: <semver>`; a provider returns `426 Upgrade Required` (policy JSON in the body) below `minRequired` — the same gate as the rest of the network.
- **Error envelope** (all non-S3 endpoints; S3 data-plane uses S3-native XML for tool compatibility):
  ```json
  { "error": { "code": "below_min_funding", "message": "human text",
               "retryable": false, "detail": {} } }
  ```

### Signed encodings

**Resource offer** — domain tag `chiral-offer-v1`, fields in order:
```
provider_wallet : addr(20)
resource_class  : u8          # 1=storage 2=container 3=inference
capacity        : jcs(...)    # class-specific (see Resource Offers)
price_schedule  : jcs(...)
endpoint        : utf8        # "https://host[:port]"
region          : utf8
min_funding_wei : u128(16)
offer_nonce     : u64(8)
valid_until     : u64(8)      # unix seconds
```
`offer_signature = sign(keccak256(canonical("chiral-offer-v1", …)))`.
`offer_ref = keccak256(canonical("chiral-offer-v1", …))` — 32 bytes; names the exact signed offer the consumer accepted (any change to price / endpoint / etc. changes `offer_ref`).

**Contract terms** — the full negotiated deal, hashed into the on-chain commitment. A JSON object hashed with JCS:
```json
{ "offer_ref":"0x…", "provider_wallet":"0x…", "consumer_wallet":"0x…",
  "resource_class":"storage", "funding_amount_wei":"…", "rates":{…},
  "params":{…}, "contract_nonce":"0x…16", "expiry":1712345678 }
```
`terms_hash = keccak256(jcs(terms))`. Both sides compute it identically; the consumer commits it on-chain, the provider recomputes and checks equality at `open`.

**Contract transaction `data`** — compact fixed layout, 84 bytes:
```
MAGIC(4)=0x43485231 "CHR1" ‖ offer_ref(32) ‖ terms_hash(32) ‖ contract_nonce(16)
```
The tx itself: `to = provider_wallet`, `value = funding_amount_wei`, signed by the consumer wallet. **Top-up** txs use `MAGIC=0x43485232 "CHR2" ‖ contract_id(32)` (36 bytes) so the provider credits the right open contract.

### Handshake endpoints

#### `POST /v1/contracts/propose`
Request:
```json
{ "offer_ref":"0x…32", "consumer_wallet":"0x…20",
  "funding_amount_wei":"1000000000000000000", "params":{ /* class-specific */ } }
```
`200 OK`:
```json
{ "provider_wallet":"0x…", "resource_class":"storage",
  "contract_nonce":"0x…16", "terms":{ /* full terms object */ },
  "terms_hash":"0x…32", "quote_expires_at":1712345678 }
```
| Status | `error.code` | Meaning |
|---|---|---|
| 400 | `invalid_request` | malformed body / params fail the class schema |
| 404 | `offer_not_found` | `offer_ref` unknown or expired |
| 409 | `below_min_funding` | `funding_amount_wei < min_funding` |
| 409 | `capacity_unavailable` | provider cannot currently honor the shape |
| 426 | `upgrade_required` | client below `minRequired` |

The `contract_nonce` is single-use and ties this quote to the eventual on-chain commitment; `quote_expires_at` bounds how long the consumer has to commit.

#### `POST /v1/contracts/open`
Request: `{ "tx_hash":"0x…32" }`
`201 Created`:
```json
{ "contract_id":"0x…32", "status":"open",
  "funded_wei":"1000000000000000000", "fee_wei":"5000000000000000",
  "balance_wei":"995000000000000000",
  "credential":{ /* see Session credential */ },
  "opened_at":1712345678, "expires_at":1712432078 }
```
`202 Accepted` (optimistic; tx seen but not yet confirmed): `{ "contract_id":"0x…", "status":"pending", "retry_after_s":15 }`.
| Status | `error.code` | `retryable` | Meaning |
|---|---|---|---|
| 402 | `payment_invalid` | false | wrong recipient / amount / chain, or `terms_hash`/`offer_ref` mismatch |
| 404 | `tx_not_found` | true | not yet visible on-chain — retry after `retry_after_s` |
| 409 | `already_open` | — | idempotent: returns the existing contract body |
| 410 | `quote_expired` | false | `contract_nonce` / quote no longer valid |

Verification at `open`: `to == provider_wallet`, `value == terms.funding_amount_wei`, `chainId == geth::chain_id()`, `data` parses to the quoted `offer_ref` + `terms_hash` + an unused `contract_nonce`, and `from == terms.consumer_wallet`; then `(tx_hash, provider)` is recorded in the contract ledger (replay-proof), `split_payment` cuts the fee, and the balance is credited. The 402-vs-404 split mirrors the seeder `PaymentProof` handler (permanent vs retryable).

#### `GET /v1/contracts/:contract_id`
Auth: `Authorization: Bearer <session bearer>`.
`200 OK`:
```json
{ "contract_id":"0x…", "status":"open|paused|closed",
  "funded_wei":"…", "spent_wei":"…", "balance_wei":"…",
  "rates":{…}, "usage":{ /* class-specific meters */ }, "updated_at":1712345678 }
```
`401 unauthorized` on a bad/absent credential; `404` if unknown.

#### `POST /v1/contracts/:contract_id/topup`
Request `{ "tx_hash":"0x…" }` (a `CHR2` tx referencing this contract). `200 OK` returns the updated balance; same 402 / 404 semantics as `open`.

#### `GET /v1/contracts/:contract_id/receipt`
Auth: `Authorization: Bearer <session bearer>`. Returns a **provider-wallet-signed** usage statement — portable evidence a consumer can cite in a rating or dispute:
```json
{ "contract_id":"0x…", "funded_wei":"…", "spent_wei":"…", "balance_wei":"…",
  "usage":{ /* class-specific meters */ }, "as_of":1712349278,
  "provider_wallet":"0x…", "signature":"0x…65" }
```
`signature = sign(keccak256(canonical("chiral-receipt-v1", contract_id, funded_wei, spent_wei, balance_wei, jcs(usage), as_of)))`; the consumer checks `ecrecover == provider_wallet`.

### Session credential

Issued in the `open` response, bound to `contract_id`, scoped to that contract's resources only:
```json
{ "bearer":"chi_sess_<≥43 base64url chars>",
  "s3":{ "endpoint":"https://host", "region":"chiral",
         "access_key_id":"AKIA…", "secret_access_key":"…",
         "bucket":"c-<contract_id[:16]>" },
  "expires_at":1712432078 }
```
- **Bearer** — an opaque, high-entropy token (≥256 bits) the provider maps server-side to the contract. Presented as `Authorization: Bearer <bearer>` on the control-plane, container, and LLM APIs. Treat it as a password: whoever holds it can spend the contract's balance-worth of service until `expires_at`.
- **S3** — an access-key/secret pair for standard SigV4; the provider maps `access_key_id → contract`, so unmodified S3 SDKs/CLIs work.
- **Lifetime & rotation** — valid while the contract is `open` and before `expires_at`; rotate with `POST /v1/contracts/:id/credential:rotate` (invalidates the prior credential). The credential grants no authority beyond the contract — it is a transport convenience over the on-chain proof of contract control, not an independent grant.
- *Optional variant:* a provider-signed capability token — `base64url(jcs{contract_id,scope,exp}) . base64url(sig)` — for stateless verification without server-side session state. Opaque bearer is the v1 default.

### Metering headers (data-plane)

Every storage / container / inference response carries drawdown telemetry so any client can watch billing in real time:
```
X-Chiral-Contract-Id: 0x…
X-Chiral-Cost-Wei:     12500000000000      # cost attributed to this request
X-Chiral-Balance-Wei:  982500000000000     # remaining after this request
```

---

## Data-Plane API: Storage (S3)

> **Status: normative v1 spec for the storage data plane.** It wraps standard S3 semantics behind the contract/credential layer of the [Wire Protocol spine](#wire-protocol--api-reference-contract-spine), so unmodified S3 SDKs/CLIs work against it. Container and inference data planes follow in later passes.

### Auth, endpoint, addressing

- **Auth.** AWS **SigV4** with the contract's issued `access_key_id` / `secret_access_key` (the [session credential](#session-credential)); `region = chiral`, `service = s3`. Presigned URLs use SigV4 query signing for anonymous, time-boxed access.
- **Bucket.** Exactly one bucket per contract, named `c-<contract_id[:16]>` and returned in the credential. There is no `CreateBucket` / `ListBuckets` — the bucket lives for the life of the contract.
- **Addressing.** Path-style (`https://host/c-<id>/<key>`) and virtual-host-style (`https://c-<id>.host/<key>`) both accepted. Keys are arbitrary UTF-8 up to 1024 bytes; single-`PUT` bodies up to the offer's `max_object_bytes`, larger via multipart.

### Object operations

| Operation | Request | Success | Notes |
|---|---|---|---|
| Put object | `PUT /{bucket}/{key}` body=bytes | `200` + `ETag` | `Content-Length` required; optional `Content-Type`, `x-amz-meta-*`, `x-amz-checksum-sha256` |
| Get object | `GET /{bucket}/{key}` | `200` / `206` (`Range`) | streams bytes; egress metered |
| Head object | `HEAD /{bucket}/{key}` | `200` (headers only) | size, ETag, content-type, checksum |
| Delete object | `DELETE /{bucket}/{key}` | `204` | idempotent |
| List objects | `GET /{bucket}?list-type=2&prefix=&delimiter=&max-keys=&continuation-token=` | `200` XML `ListBucketResult` | paginated |
| Initiate multipart | `POST /{bucket}/{key}?uploads` | `200` XML (`UploadId`) | for large objects |
| Upload part | `PUT /{bucket}/{key}?partNumber=N&uploadId=U` body=bytes | `200` + part `ETag` | parts ≥ 5 MiB except the last |
| Complete multipart | `POST /{bucket}/{key}?uploadId=U` body=`<CompleteMultipartUpload>…</…>` | `200` XML result | assembles parts |
| Abort multipart | `DELETE /{bucket}/{key}?uploadId=U` | `204` | frees parts |

Data-plane responses carry the standard `ETag` / `Content-Length` / `Content-Type` / `Last-Modified` **plus** the metering trio `X-Chiral-Contract-Id` / `X-Chiral-Cost-Wei` / `X-Chiral-Balance-Wei`.

### Integrity & content addressing

- **ETag** is the object's **MD5** (quoted hex) for single-part puts — the AWS convention, so integrity-checking clients keep working; multipart ETags use the AWS `"<md5-of-part-md5s>-<n>"` form.
- **Content SHA-256** rides the native `x-amz-checksum-sha256` header (base64), settable on `PUT` and returned on `GET`/`HEAD` (with `x-amz-checksum-algorithm: SHA256`). This is the content-addressing hook: a **content-addressed object** uses the hex SHA-256 as its key and the checksum header to verify — the standards-aligned successor to "sharing a file by its hash."

### Metering, balance & exhaustion

- **Billed:** stored capacity (sampled into GB-month at `per_gb_month`) and **egress** on `GET`/part downloads (at `per_gb_egress`). Ingress is not billed beyond the capacity it creates.
- Every response reports live drawdown via the `X-Chiral-*` headers.
- **Insufficient balance:** an operation that cannot be covered returns `402` with S3-XML `<Code>InsufficientBalance</Code>`. On full exhaustion the provider blocks writes immediately and serves reads for a grace window (default — see [Design Decisions](#design-decisions)), after which objects may be deleted. **Single-provider durability is the consumer's responsibility.**

### Errors (S3-native XML)

```xml
<Error><Code>NoSuchKey</Code><Message>…</Message>
       <Resource>/c-…/key</Resource><RequestId>…</RequestId></Error>
```

| HTTP | `Code` | Meaning |
|---|---|---|
| 404 | `NoSuchKey` / `NoSuchBucket` | object / bucket absent |
| 403 | `AccessDenied` / `SignatureDoesNotMatch` | bad SigV4 or wrong contract |
| 400 | `EntityTooLarge` | body exceeds `max_object_bytes` |
| 400 | `InvalidArgument` / `MalformedXML` | bad request |
| 402 | `InsufficientBalance` | balance cannot cover the op *(Chiral extension)* |
| 403 | `ContractPaused` | exhausted, in read-only grace *(Chiral extension)* |
| 410 | `ContractClosed` | contract ended *(Chiral extension)* |

### Public-read objects (distribution)

A consumer exposes an object anonymously two ways: a **presigned GET** (SigV4 query URL bounded by `X-Amz-Expires`), or a **public-read ACL** (`x-amz-acl: public-read` on `PUT`) that serves the object at its bucket URL without auth. Egress on anonymous reads still draws down the owning contract's balance, so a popular public object drains the balance and pauses when exhausted — the intended "pay for the availability you provide" behavior, and the direct successor to the old file-sharing use case.

### Deviations from AWS S3 (v1)

| Area | Chiral v1 |
|---|---|
| Buckets | one per contract, auto-provisioned; no `CreateBucket` / `ListBuckets` |
| Region | fixed `chiral` |
| Credentials | contract-scoped, ephemeral (expire with the contract), rotatable |
| Billing | per-op against the contract balance; `InsufficientBalance` (402) |
| Integrity | ETag = MD5; content hash via `x-amz-checksum-sha256` |
| Durability | single-provider, non-refundable; no cross-region replication |
| Not in v1 | versioning, lifecycle rules, bucket policies / CORS config, object tagging, SSE-KMS |

---

## Data-Plane API: Compute (Containers)

> **Status: normative v1 spec for the container data plane** — a Chiral-native control API over a hardened OCI runtime (isolation model in [Resource Interfaces](#resource-interfaces)). Bearer-authenticated against the contract; MicroVM isolation is the noted post-v1 path.

### Auth & envelope

- **Auth.** `Authorization: Bearer <session bearer>` (the [session credential](#session-credential)). Paths under `/v1/`, JSON bodies, the spine's [error envelope](#conventions) and `X-Chiral-*` metering headers on every response.
- **Resource envelope.** The contract `terms.params` fix what the contract may consume — `max_vcpu`, `max_mem_gb`, `max_gpu` (model + count), and the allowed image registries. A contract may run **one or more** containers concurrently as long as their summed requests fit the envelope; metering sums all running containers.

### Endpoints

| Operation | Request | Success |
|---|---|---|
| Run container | `POST /v1/containers` | `201` container object |
| List | `GET /v1/containers` | `200` `{ "containers": [...] }` |
| Inspect | `GET /v1/containers/:id` | `200` container object |
| Logs | `GET /v1/containers/:id/logs?since=&tail=&follow=` | `200` text (or SSE if `follow=true`) |
| Stats | `GET /v1/containers/:id/stats` | `200` utilization snapshot |
| Stop | `POST /v1/containers/:id/stop` | `200` (halts metering; allocation released) |
| Delete | `DELETE /v1/containers/:id` | `204` (stop + remove) |

#### `POST /v1/containers`
Request:
```json
{ "image": "docker.io/library/nginx:1.27",
  "cmd": ["nginx","-g","daemon off;"],
  "env": { "KEY": "value" },
  "ports": [ { "container_port": 80, "protocol": "tcp" } ],
  "resources": { "vcpu": 2, "mem_gb": 4, "gpu": { "model": "a100", "count": 1 } },
  "pull_secret": { "registry": "…", "auth": "…" } }
```
`201 Created`:
```json
{ "container_id": "ctr_<base32>", "status": "provisioning",
  "image_digest": "sha256:…",
  "resources": { "vcpu": 2, "mem_gb": 4, "gpu": { "model": "a100", "count": 1 } },
  "endpoints": [ { "container_port": 80, "url": "https://ctr-<id>-80.<provider-host>", "proto": "https" } ],
  "created_at": 1712345678 }
```
The provider pulls `image` (allowed registries only), enforces the [isolation profile](#resource-interfaces) (non-privileged, read-only rootfs, dropped caps, seccomp, cgroup caps, egress policy), and reverse-proxies each exposed `container_port` at an assigned HTTPS subdomain (TLS terminated at the provider).

#### `GET /v1/containers/:id`
```json
{ "container_id": "ctr_…", "status": "running",
  "resources": {…}, "endpoints": [...],
  "uptime_s": 3600,
  "usage": { "vcpu_seconds": 7200, "gpu_seconds": 3600, "cost_wei": "…" },
  "health": "healthy", "restart_count": 0, "updated_at": 1712349278 }
```

#### `GET /v1/containers/:id/logs`
`follow=false` (default) returns the buffered tail as `text/plain`; `follow=true` streams new lines as `text/event-stream` (SSE) until the client disconnects or the container exits.

#### `GET /v1/containers/:id/stats`
```json
{ "cpu_pct": 41.2, "mem_used_mb": 812, "mem_limit_mb": 4096,
  "gpu_pct": 88.0, "net_rx_bytes": 0, "net_tx_bytes": 0, "sampled_at": 1712349278 }
```

### Ingress & networking

- Each exposed `container_port` gets an assigned **HTTPS subdomain** (`https://ctr-<id>-<port>.<provider-host>`), TLS-terminated at the provider and reverse-proxied to the container. Raw TCP port mapping is a post-v1 option.
- Outbound egress is permitted under the provider's abuse policy; egress bandwidth is billed at the offer's `per_gb_egress` when set, otherwise folded into the runtime rate.

### Isolation constraints (wire-visible)

A request for `privileged: true`, host networking, host bind-mounts, or capabilities outside the allowed set is rejected with `400 invalid_spec`. The rootfs is read-only unless the spec declares an ephemeral, size-capped writable scratch. Durable state is achieved by mounting a Storage-class bucket, not a local volume (v1).

### Metering, states & exhaustion

- **Billed:** per-second wall-clock × the contract's `per_vcpu_hour` / `per_gb_mem_hour` / `per_gpu_hour`, summed over running containers, plus egress if priced. GPU time is billed while allocated.
- **States:** `provisioning → running → (stopped | exited | failed) → removed`. `stopped` (via `/stop` or balance exhaustion) releases the allocation and halts metering; the ephemeral rootfs does **not** survive a stop.
- **Exhaustion:** at zero balance running containers are stopped and the contract enters `ContractPaused`; a top-up re-enables new runs (previous ephemeral state is gone).

### Errors

| HTTP | `error.code` | Meaning |
|---|---|---|
| 400 | `invalid_spec` | bad image ref / resources / a rejected privileged or host option |
| 402 | `insufficient_balance` | balance cannot cover the requested shape |
| 403 | `contract_paused` | balance exhausted |
| 404 | `container_not_found` | unknown id |
| 409 | `envelope_exceeded` | request + running containers exceed the contract cap |
| 409 | `capacity_unavailable` | provider cannot currently place the workload |
| 422 | `image_not_allowed` | registry not in the offer's allowlist |
| 422 | `image_pull_failed` | pull or auth error |

### v1 limits

Ephemeral rootfs only (durable state via a mounted Storage bucket); HTTP(S) ingress only (no raw TCP); hardened-OCI isolation (no microVM); one spec = one container (no autoscaling/orchestration); bring a prebuilt image (no in-provider build).

---

## Data-Plane API: Inference (LLM)

> **Status: normative v1 spec for the inference data plane** — an OpenAI-compatible API, so standard OpenAI SDKs work by pointing `base_url` at the provider's `endpoint` and using the contract session as the API key. Bearer-authenticated against the contract.

### Auth & base

- **Auth.** `Authorization: Bearer <session bearer>` — the OpenAI convention; set the SDK's `api_key` to the [session credential](#session-credential)'s bearer and `base_url` to `<endpoint>/v1`.
- **Error shape.** Errors use the **OpenAI** envelope (not the spine envelope) so SDKs parse them: `{ "error": { "message", "type", "param", "code" } }`. The `X-Chiral-Client-Version` gate and `X-Chiral-*` metering headers still apply.

### Endpoints

| Operation | Request | Notes |
|---|---|---|
| List models | `GET /v1/models` | the models the offer advertises |
| Chat completion | `POST /v1/chat/completions` | primary; `stream:true` → SSE |
| Text completion | `POST /v1/completions` | legacy compatibility |
| Embeddings | `POST /v1/embeddings` | billed on input tokens only |

#### `POST /v1/chat/completions`
Request (OpenAI-shaped):
```json
{ "model": "llama-3.1-70b-instruct",
  "messages": [ { "role": "user", "content": "…" } ],
  "max_tokens": 512, "temperature": 0.7,
  "stream": true, "stream_options": { "include_usage": true } }
```
Non-streaming `200 OK` is the OpenAI `chat.completion` object, including `usage`:
```json
{ "id":"chatcmpl-…", "object":"chat.completion", "model":"llama-3.1-70b-instruct",
  "choices":[ { "index":0, "message":{"role":"assistant","content":"…"}, "finish_reason":"stop" } ],
  "usage":{ "prompt_tokens":42, "completion_tokens":128, "total_tokens":170 } }
```
Non-streaming responses carry the actual `X-Chiral-Cost-Wei` / `X-Chiral-Balance-Wei` headers.

**Streaming** (`stream:true`) is Server-Sent Events — `data: {chat.completion.chunk}` lines then `data: [DONE]`. Because HTTP headers precede the body, cost/balance cannot ride in headers; with `stream_options.include_usage:true` the **final chunk** carries `usage` plus Chiral fields:
```
data: {"id":"chatcmpl-…","object":"chat.completion.chunk",
       "choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
       "usage":{"prompt_tokens":42,"completion_tokens":128,"total_tokens":170},
       "x_chiral":{"cost_wei":"…","balance_wei":"…"}}

data: [DONE]
```

#### `POST /v1/embeddings`
```json
{ "model":"bge-large-en", "input":["text a","text b"] }
```
`200 OK`: an OpenAI `list` of `{ object:"embedding", index, embedding:[…] }` + `usage:{ prompt_tokens, total_tokens }`. Billed on input tokens only.

#### `GET /v1/models`
`200 OK` reflects the offer's `capacity.models` and `price_schedule`:
```json
{ "object":"list", "data":[ { "id":"llama-3.1-70b-instruct", "object":"model",
    "context_length":131072,
    "chiral":{ "per_1k_input_wei":"…", "per_1k_output_wei":"…" } } ] }
```

### Model selection & identity

The consumer sets `model` to an advertised id; an unknown id returns `404 model_not_found`. A provider cannot cheaply prove it runs the claimed model rather than a smaller/quantized one — a **reputation-policed** quality property; model attestation is deferred to future work (see [Design Decisions](#design-decisions)).

### Metering & pre-authorization

- **Billed:** `prompt_tokens × per_1k_input + completion_tokens × per_1k_output` (embeddings: input only), drawn down on completion.
- **Pre-authorization:** before generating, the provider checks the balance covers the *worst case* — `prompt_tokens + max_tokens` — and returns `402` if not, so a request never exhausts mid-stream. The actual (usually lower) cost is billed on completion.
- **Reporting:** non-streaming → the `usage` block + `X-Chiral-*` headers; streaming → the final chunk's `usage` + `x_chiral` fields (headers are already sent). A local tokenizer lets the consumer reconcile.

### Errors (OpenAI-shaped)

```json
{ "error": { "message":"…", "type":"insufficient_quota", "param":null, "code":"insufficient_balance" } }
```

| HTTP | `type` / `code` | Meaning |
|---|---|---|
| 401 | `invalid_request_error` / `invalid_api_key` | bad / expired session bearer |
| 404 | `invalid_request_error` / `model_not_found` | model not served |
| 400 | `invalid_request_error` / `context_length_exceeded` | prompt + `max_tokens` > context |
| 402 | `insufficient_quota` / `insufficient_balance` | balance can't cover worst-case cost *(Chiral uses 402, not OpenAI's 429)* |
| 403 | `invalid_request_error` / `contract_paused` | balance exhausted |
| 426 | `invalid_request_error` / `upgrade_required` | client below `minRequired` |
| 429 | `rate_limit_exceeded` | per-contract concurrency / rate cap |

### Deviations from OpenAI (v1)

| Area | Chiral v1 |
|---|---|
| Auth | bearer = contract session credential (ephemeral), not a long-lived API key |
| Billing | per-token against the contract balance; **402 `insufficient_quota`** on exhaustion (not 429) |
| Models | only what the provider serves; identity is reputation-policed, no attestation |
| Streaming usage | via `stream_options.include_usage`; final chunk carries `usage` + `x_chiral` cost/balance |
| Endpoints | `chat/completions`, `completions`, `embeddings`, `models` |
| Not in v1 | assistants / threads, files, fine-tuning, batch, images, audio, moderations; function/tool calling passes through to the model but is not separately guaranteed |

---

## Reputation System

> **Status: the Elo engine, on-chain verification of the rating event, and the batch lookup are reused as-is (`rating_api.rs`, `rating_storage.rs`, the relay `/api/ratings/*` routes). The change is admitting a subjective score in the event, gated by verified payment.**

The design rationale and formula are in [Part I](#part-i-white-paper), §7. A rating event is keyed to a **service contract**: it carries the provider wallet, the rater wallet, a normalized subjective score, the contract's CHI `funding_amount`, the contract `tx_hash`, and a timestamp. Before admission the relay verifies the `tx_hash` on-chain (it is a contract-funding tx: `from` = rater, `to` = provider, `value` = amount) — the backend never trusts client-submitted event data — so **only the consumer who actually opened a contract with a provider can move that provider's score**, weighted by the contract amount and recency. A star/thumbs rating normalizes to `S ∈ [0,1]`; the latest rating for a given contract supersedes earlier ones (a consumer may revise after longer use), so a single contract yields a single live vote whose weight is its funding amount.

### Parameters

| Parameter | Value |
|-----------|-------|
| Score range | 0 to 100 (clamped) |
| Base score for new wallets | 50 |
| Lookback window | 180 days |
| Time weight `w_time` | Linear decay from 1.0 (today) to 0.0 (180 days ago) |
| Amount weight `w_amount` | `1.0 + clamp(ln(1 + chi) / ln(51), 0, 1)` — 1.0 (small) to 2.0 (50+ CHI) |
| Outcome `S` | normalized user rating in `[0, 1]` |
| Expected score | `1 / (1 + 10^((50 - elo) / 12))` |
| K factor | `4 * w_time * w_amount` |
| Update | `elo = clamp(elo + K * (S - expected), 0, 100)` |

### API Endpoints (Relay Server)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/ratings/:wallet` | GET | Elo score and event history for a provider |
| `/api/ratings/batch` | POST | Batch lookup for ranking offers in discovery |
| `/api/ratings/feedback` | POST | Record a payment-gated rating (verified on-chain before admission) |

---

## Design Decisions

The decisions that shaped this design, now settled — each records the choice; the mechanics live in the linked sections.

1. **Post-handshake auth** — the provider issues a **contract-scoped session credential** (S3 access-key/secret, OpenAI-style bearer, or container token bound to `contract_id`) at `open`, so off-the-shelf S3/OpenAI tooling works directly; a local wallet-signing proxy is the no-shared-secret alternative. The credential is proof of contract control, not an independent grant of authority.
2. **Optimistic service start** — a small contract starts on *mempool-seen* with a short confirmation deadline; a large one waits for the tx to be *mined*. Bounds start latency without exposing the provider to large unconfirmed value. ([Service Contracts](#service-contracts-and-the-handshake))
3. **On-chain contract payload** — the contract tx commits only `{offer_ref, terms_hash, contract_nonce}` (compact, private); full terms are exchanged in the handshake and stored on both sides, bound by `terms_hash`. ([Wire Protocol](#wire-protocol--api-reference-contract-spine))
4. **Storage retention at exhaustion** — at zero balance, writes stop immediately and reads continue for a bounded grace window, after which objects may be deleted; single-provider durability is the consumer's responsibility. ([Storage](#data-plane-api-storage-s3))
5. **Provider usage receipts** — the provider issues **wallet-signed usage receipts** (`GET /v1/contracts/:id/receipt`) so a consumer holds portable, attributable evidence of what it was charged, for disputes and reputation. ([Service Contracts](#service-contracts-and-the-handshake))
6. **Discovery aggregation** — both: the DHT is the source of truth; the relay caches a verified per-class offer list for speed; clients always re-verify signatures. ([Resource Offers](#resource-offers-discovery))
7. **LLM model attestation** — v1 relies on reputation for model identity; a provider-published model fingerprint / attested runtime is future work. ([Inference](#data-plane-api-inference-llm))
8. **Container persistence** — v1 rootfs is ephemeral; durable state is a mounted Storage-class bucket; local persistent volumes are future work. ([Compute](#data-plane-api-compute-containers))

---

## Provider Implementation

> **Status: implemented and runnable** (branch `feat/resource-exchange-v1`). The settlement engine (`resource_offer`, `codec`, `service_contract`, `contract_ledger`, `session_credential`, `usage_receipt`), the provider cores + HTTP routers (handshake `contract_api`; storage `storage_api`; inference `llm_api`; compute `container_api`), the real on-chain `RpcChainVerifier` (`eth_getTransactionByHash`), the shared-state composition, the `provider_gateway` assembly, and the real container runtime (`docker_runtime::DockerCliRuntime`, the hardened `docker` CLI), and the `chiral_provider` binary (`provider_daemon`, configured via `CHIRAL_PROVIDER_*`) are all implemented. Coverage is unit + HTTP-handler + **end-to-end integration** (`tests/provider_integration.rs`: all three providers driven through propose → open → data-plane → metered drawdown over one shared `ProviderState`). Providers also **publish their signed offer to the DHT on startup** (`DhtService::register_offer`, wired through `provider_daemon::run` via an embedded headless DHT node), so they are discoverable. **Remaining (not provider-side):** the consumer discovery/search UI (query `chiral_offers_<class>`, verify offers, rank by Elo) and tool-compat polish (full S3 SigV4, LLM SSE streaming). These are marketplace-client work / soak testing, not provider functions.

A **provider** is a headless process — the `chiral_daemon` in *provider mode*, or a dedicated `chiral_provider` binary — that (1) publishes signed offers to the DHT, (2) runs an HTTPS server exposing the contract handshake plus one or more data-plane APIs, and (3) meters usage against an in-memory contract ledger. All three classes share the same node skeleton and settlement engine; they differ only in the **data-plane server** and its **meter**.

### Shared provider node

```
                +-------------------- Provider process --------------------+
 DHT  <-------- | Offer publisher   (resource_offer: sign + republish)     |
                |                                                           |
 HTTPS (own TLS)| Axum gateway                                             |
   consumer --> |  /v1/contracts/*   Contract service (handshake)          |
                |     propose / open / get / topup / receipt               |
                |  /v1/...           Data-plane router (class-specific)     |
                |        |                     |                            |
                |        v                     v                            |
                |  Auth middleware       Meter -> ContractLedger::draw_down |
                |  (SessionStore)                      |                    |
                |  ContractLedger <---- on-chain verify (wallet+rpc_client) |
                +-----------------------------------------------------------+
                          | persistence (ledger, spent-tx, offers) -> disk
```

- **Offer publisher.** Builds a signed `ResourceOffer`, publishes it under `chiral_offer_<class>_<wallet>` + the class index, and refreshes on an interval (≈ every 2–3 min). Reuses the DHT put / provider machinery.
- **Contract service** (`/v1/contracts/*`). **propose:** validate `offer_ref` + class params, quote `ContractTerms`, mint a single-use `contract_nonce`, return `terms_hash`. **open:** verify the funding tx on-chain (reuse `wallet` verification — mined, `to`=self, `value`, `chainId`, via `rpc_client`), parse the tx `data` (`service_contract::parse` → `CHR1`), check the committed `terms_hash`/`nonce`, credit with `ContractLedger::open`, mint a `SessionCredential`, register it in the `SessionStore`. **topup:** verify a `CHR2` tx → `ContractLedger::topup`. **get/receipt:** read the ledger; sign a `UsageReceipt`.
- **Auth middleware.** Resolves the presented credential — bearer or S3 access-key — to a `contract_id` via `SessionStore` (honoring expiry); 401 on miss.
- **Meter → drawdown.** Each data-plane operation computes a `cost_wei` and calls `ContractLedger::draw_down(contract_id, cost_wei)`; an `Err(insufficient)` becomes the class's exhaustion response.
- **State & persistence.** `Arc<Mutex<ContractLedger>>` + `Arc<Mutex<SessionStore>>` in app state. The ledger, the spent-tx set, and published offers persist to disk (JSON under `<data_dir>/provider/`) so a restart neither loses balances nor re-accepts a funding tx — extending the existing spent-tx-ledger persistence pattern.
- **Reachability.** The provider terminates HTTPS itself (own cert/domain per [Deployment](#deployment)); no relay/NAT in v1.

### Storage provider (S3)

- **Server.** Axum handlers implementing the S3 subset ([Data-Plane: Storage](#data-plane-api-storage-s3)). v1 recommendation: **custom Axum handlers over a local object store** (full control of auth + metering); fronting a standalone S3 impl (MinIO) behind a Chiral auth/meter proxy is the fallback if S3 coverage gaps bite.
- **Object store.** Bytes on disk at `<data_dir>/provider/storage/<bucket>/<key-digest>`; a metadata index (`key → {size, content_type, etag=md5, sha256, created}`) persisted (sled or JSON); `x-amz-checksum-sha256` computed on `PUT`.
- **Auth.** SigV4 verified against the contract's `secret_access_key` from `SessionStore::secret_for_access_key`; presigned URLs check the same secret; public-read objects bypass auth.
- **Metering.** Egress metered on `GET`/part bytes → `draw_down(bytes · per_gb_egress / GiB)`; capacity sampled by a periodic task summing per-contract stored bytes and charging `bytes · per_gb_month · Δt / month`. A pre-check refuses an op the balance can't cover (`402 InsufficientBalance`).
- **Exhaustion.** Zero balance → block writes; serve reads through a grace timer; then GC the bucket (Design Decision #4).

### Container provider (hardened OCI)

- **Runtime driver.** Drive Docker/Podman via the Docker Engine API (Rust `bollard`). On submit: allowed-registry check + pull; create/start with the hardened profile — non-root, `cap-drop=ALL` (+ minimal adds), seccomp, read-only rootfs, `--memory`/`--cpus`/`--pids-limit` from the contract envelope, no host mounts, an egress policy; publish the exposed port.
- **Ingress.** A reverse proxy on the provider's HTTPS front routes the assigned subdomain (`ctr-<id>-<port>.<host>`) to the container's mapped port; TLS terminates at the proxy.
- **Meter.** A periodic task sums running-container resource-seconds → `draw_down(vcpu·rate + mem·rate + gpu·rate)`; insufficient → stop the container and pause the contract.
- **Lifecycle.** `GET`/`DELETE`/`logs`(stream)/`stats` map to Docker API calls. Ephemeral rootfs; durable state via a mounted Storage bucket is future work. GPU via the runtime's device requests, billed while allocated.

### LLM provider (OpenAI-compatible)

- **Model backend.** The provider runs a local inference server (llama.cpp `server`, vLLM, or Ollama); the Chiral LLM server is a **thin proxy** in front that adds contract auth + metering + the `x_chiral` fields and normalizes to the exact OpenAI shape.
- **Auth.** Bearer → `SessionStore::resolve_bearer` → contract.
- **Pre-authorization.** Before forwarding, estimate worst-case cost (`prompt_tokens + max_tokens`) with a local tokenizer; `> balance` → `402 insufficient_quota`.
- **Meter.** On completion, read `usage` from the backend, `draw_down(in·per_1k_in + out·per_1k_out)`; for streaming, buffer usage and emit it plus `x_chiral` in the final chunk.
- **Models.** `/v1/models` reflects the offer's advertised models; the proxy maps a requested `model` to a backend model and 404s unknown ones.

### Engine → provider integration

| Engine module | Storage | Container | LLM |
|---|---|---|---|
| `resource_offer` (advertise) | ✓ | ✓ | ✓ |
| `service_contract` (handshake `data` / terms) | ✓ | ✓ | ✓ |
| `contract_ledger` (balance / drawdown) | ✓ | ✓ | ✓ |
| `session_credential` (auth) | bearer + **SigV4 secret** | bearer | bearer |
| `usage_receipt` (evidence) | ✓ | ✓ | ✓ |

---

## Implementation Plan (Milestones)

Phased carry-out. **Phase 0 is done** (settlement engine); the rest builds on it. Each phase names new/touched files, a verification, and the main risk.

**Phase 0 — Settlement engine (done).** `resource_offer`, `codec`, `service_contract`, `contract_ledger`, `usage_receipt`, `session_credential` — pure, unit-tested (34 tests). *Not wired.*

**Phase 1 — Shared provider node (wiring).** Offer publisher (DHT publish/search) + the `/v1/contracts/*` handshake handlers + on-chain funding verification + ledger/session state + persistence, mounted into `chiral_daemon` provider mode. New: `provider_node.rs`, `contract_api.rs`; touches `dht.rs`, `chiral_daemon.rs`, `lib.rs`. *Verify:* a client runs propose→open→get→topup→receipt against a live daemon (integration test). *Risk:* high — `dht.rs`/daemon are hot files.

**Phase 2 — Storage provider (first end-to-end).** S3 Axum handlers + disk object store + SigV4 + capacity/egress metering + exhaustion. New: `storage_provider.rs`. *Verify:* `aws s3 cp` with the contract's issued keys stores/reads an object and draws the balance down. *Risk:* medium.

**Phase 3 — Reputation (payment-gated).** `POST /api/ratings/feedback` keyed to a contract tx, verified on-chain; rating UI; ranking wired into discovery. Touches `rating_api.rs` / `reputation.rs`. *Verify:* a rating lands only with a valid contract payment; score moves by amount.

**Phase 4 — LLM provider.** OpenAI-compatible proxy + token meter over the spine (reuses Phase 1). New: `llm_provider.rs` + a model-backend adapter. *Verify:* the `openai` SDK completes a chat against the endpoint and bills tokens.

**Phase 5 — Container provider.** `bollard` driver (hardened profile) + ingress proxy + runtime meter. New: `container_provider.rs`. *Verify:* submit an image, reach it at its subdomain, watch runtime bill; teardown halts metering. *Risk:* isolation correctness.

**Phase 6 — Client & UX.** Marketplace browse (offers by class + Elo), provider dashboard (offers/earnings/contracts), a consumer contract client + local proxy for tool auth; provider-mode daemon flags/config. Touches frontend + `chiral.rs` / `chiral_daemon.rs`.

**Cross-cutting:** persistence, config/env (provider wallet key, endpoint/domain, model-backend URL), deployment (public IP + TLS), and an integration test per phase. Reused throughout: wallet + on-chain verification, the DHT signed-record discipline, the Elo engine, version enforcement, and the headless daemon / CLI / relay.

---

## Identity and Wallet

Reused unchanged. A single secp256k1 keypair is a participant's identity, offer signer, and payee.

- Generate from a 12-word BIP39 mnemonic; import via private key or recovery phrase; optional one-time email backup.
- Send/receive CHI; transaction history classified (send, receive, balance funding, provider earnings).
- All wallet logic lives in `wallet.rs`; `lib.rs` holds thin command wrappers. RPC reads walk an ordered fallback list (`rpc_client::call_with_fallbacks`): direct canonical Geth → the relay's `/api/chain/rpc` proxy, so either path can be down without taking the wallet offline. Write paths pin a single endpoint to avoid double-broadcast.

---

## Blockchain and Mining

A private Ethereum-compatible chain using Ethash proof-of-work.

### Chain Parameters

| Parameter | Value |
|-----------|-------|
| Chain ID / Network ID | 98765 |
| Consensus | Ethash |
| Block reward | 5 CHI |
| Genesis difficulty | 0x400000 (4,194,304) |
| Gas price | 0 (free transactions) |
| Sync mode | `full` (configurable via `CHIRAL_GETH_SYNCMODE`) |
| GC mode | `archive` (keeps all state; prevents height regression on restart) |
| Client | Core-Geth v1.12.20 |
| Bootstrap enode | `130.245.173.73:30303` |
| RPC | local `127.0.0.1:8545`; remote fallback `130.245.173.73:8545` |

Embedded Geth binds its HTTP RPC to loopback only, exposes only the `eth,net,web3,miner` namespaces (no `admin`, so `admin_stopRPC` cannot be reached remotely), and does not enable wildcard browser CORS; public read-only RPC access goes through the relay's `/api/chain/rpc` proxy allowlist. Mining auto-starts Geth with the wallet as coinbase; CPU threads are configurable; status is polled every 10 s via batch RPC.

---

## Version Enforcement

Unchanged from the content design — a defence-in-depth scheme keeping vulnerable builds off the network, layered so one bypass does not disable it. `VersionPolicy` (`version.rs`) carries `minRequired`, `recommended`, `downloadUrl`, `message`, `issuedAt`, `validUntil`, and an Ed25519 `signature`. Layers: the `UpdateGate.svelte` UI (soft banner / hard modal), the `ensure_version_supported` Tauri gate, an `X-Chiral-Client-Version` HTTP middleware (426 below `minRequired`), and a libp2p Identify check (`agent_version = chiral/<v>`, disconnect on mismatch). The relay serves `/api/version-policy`; desktops promote it if `is_acceptable_remote_policy` accepts. Operators activate signed policies at deploy time via `CHIRAL_POLICY_PUBLIC_KEY`; `chiral-policy-sign` handles keygen / sign / verify.

---

## Application Surface

| Route | Page | Description |
|-------|------|-------------|
| `/marketplace` | Marketplace | Browse offers by resource class; view price and provider Elo; fund a balance and connect. |
| `/provider` | My Resources | Provider dashboard: publish/refresh offers, view balances and earnings, manage running resources. |
| `/wallet` | Wallet | Create, import, or restore a wallet; optional email backup. |
| `/account` | Account | Wallet address, CHI balance, transaction history, reputation. Send CHI. |
| `/network` | Network | P2P connections, local Geth control, peer list, bootstrap and DHT health. |
| `/mining` | Mining | CPU/GPU mining controls; hash rate, block height, total mined CHI. |
| `/settings` | Settings | Appearance, notifications, provider defaults. |
| `/diagnostics` | Diagnostics | Event log, DHT health, bootstrap and Geth status, log viewer. |

---

## Backend Modules

Rust backend under `src-tauri/src/` (selected; adapted for the resource exchange):

| Module | File | Responsibility |
|--------|------|---------------|
| Command Layer | `lib.rs` | Thin Tauri command wrappers, AppState |
| Wallet | `wallet.rs` | Balance, tx signing (EIP-155), history, on-chain payment verification |
| RPC Client | `rpc_client.rs` | Connection-pooled HTTP, batch JSON-RPC, response cache |
| DHT Service | `dht.rs` | libp2p Kademlia, peer management, signed offer publish/search |
| Offers / Marketplace | `hosting.rs`, `hosting_server.rs` | Offer record types, signing, publish/registry (generalized from host ads) |
| Settlement | `speed_tiers.rs`, `wallet.rs` | `split_payment` (fee cut, default 0.5% / 0.1% floor) + balance funding verification |
| Storage provider | *(new)* | S3-compatible object server + metering |
| Compute / inference providers | *(new)* | container submission + OpenAI-compatible token meter |
| Rating API | `rating_api.rs`, `rating_storage.rs` | Elo computation + payment-gated feedback endpoints |
| Geth Process | `geth.rs`, `geth_bootstrap.rs` | Core-Geth lifecycle, mining, bootstrap health |
| Chain RPC | `chain_rpc_api.rs` | Blockchain RPC proxy |
| Version Policy | `version.rs` | `VersionPolicy`, Ed25519 sign/verify, effective-policy slot |

Binary targets: `chiral-network` (desktop), `chiral` (CLI), `chiral_daemon` (headless provider/node), `relay_server` (bootstrap + reputation), `chiral-policy-sign` (operator CLI).

---

## Headless Mode and CLI

The daemon runs the backend without a GUI — used to **run a provider** on a public host, and for automated testing. It exposes an HTTP API (default port 9419).

```bash
chiral_daemon --port 9419 --auto-start-dht --auto-mine --miner-address 0xABC
```

| Category | Endpoints (prefix `/api/headless/` unless noted) |
|----------|-----------|
| Health | `GET /api/health`, `GET /api/ready` |
| Version policy | `GET /api/version-policy` (mounted on the gateway router) |
| Wallet | `GET wallet`; `POST wallet/create`, `wallet/import`, `wallet/balance`, `wallet/send`, `wallet/history` |
| DHT | `POST dht/start`, `dht/stop`, `dht/get`, `dht/ping`; `GET dht/health`, `dht/peers`, `dht/peer-id` |
| Offers | `POST offers/publish` (signed), `offers/unpublish`; `GET offers/search?class=` |
| Settlement | `POST settlement/fund` (verify + credit), `settlement/meter`; `GET settlement/balance` |
| Geth | `POST geth/start`, `geth/stop`; `GET geth/status`, `geth/logs` |
| Mining | `POST mining/start`, `mining/stop`; `GET mining/status`, `mining/blocks` |
| Reputation | `POST ratings/feedback`; `GET ratings/:wallet` |

The CLI talks to a running daemon over HTTP: `chiral daemon status`, `chiral wallet create`, `chiral offers search --class storage`, `chiral mining start --threads 4`.

---

## Security Implementation

The trust model is in [Part I](#part-i-white-paper) (§3, §6, §9). Implementation inventory:

**Signed records (writers refuse to publish unsigned; readers drop unsigned/invalid):**
- Resource offers (`chiral_offer_<class>_<wallet>`) — signed by the provider wallet over a length-prefixed canonical payload including price, endpoint, and payee, so a relaying node cannot substitute any of them.
- Reserved namespaces reject raw `dht_put` (403); offers are writable only through the signed publish command.

**Settlement verification:**
- Funding tx checked on-chain before credit: mined, recipient = provider, amount, and `chainId == geth::chain_id()` (cross-chain replay rejected).
- Spent-tx ledger keyed on `(tx_hash, provider)` so one funding payment credits exactly one balance.
- Exact `u128` split (`split_payment`) — no `f64`, no rounding tolerance; `credit + fee == amount`.
- ECDSA signatures enforce low-`s` (EIP-2), so signature hex is unique per (key, message).

**HTTP authentication (owner-proof):**
- Authenticated provider-management routes require `X-Owner: 0x<wallet>` + `X-Owner-Sig: <unix_ts>:<hex_sig>` over a canonical `(wallet, ts, method, path)` payload; the server recovers the signer and rejects on mismatch/expiry (±5-minute window). Private keys never leave the desktop process (`compute_owner_proof`).

**Rests on reputation (not cryptographically enforced):** fair metering, storage retrievability, container liveness, and inference quality — disciplined by payment-gated reputation (§7), not by proofs. This is the deliberate v1 trade.

---

## Deployment

**Running a provider (v1).** A provider must be **publicly reachable**: its offer's `endpoint` has to resolve to a public IP that consumers can connect to directly. A domain name with TLS is strongly recommended — consumers connect over HTTPS, and a stable hostname survives IP changes. There is **no NAT traversal or circuit relay for providers** in v1; a provider behind NAT is not reachable and its offers are dead listings. Consumers, which only make outbound calls, may be behind NAT.

**Relay / bootstrap.** The canonical box `130.245.173.73` runs the DHT bootstrap (libp2p :4001) and the HTTP gateway (:8080) serving the reputation registry (`/api/ratings/*`), the version policy (`/api/version-policy`), and the chain-RPC proxy (`/api/chain/rpc`). Its former role relaying data for NAT'd peers is **not** part of the v1 provider path.

**Blockchain.** Geth is spawned locally (RPC 8545, P2P 30303) or reached via the canonical fallback; the CDN/always-on infrastructure of earlier designs is retired.

---

## Getting Started

```bash
npm install                                   # frontend deps
npm run tauri:dev                             # desktop dev
npm run build                                 # frontend build
npm test                                      # frontend tests
cargo test  --manifest-path src-tauri/Cargo.toml   # rust tests
cargo check --manifest-path src-tauri/Cargo.toml   # rust type-check
```

Prerequisites: Node.js 20+, the Rust toolchain (rustup), npm.

---

## Testing

- **Frontend (vitest):** `npm test` — store/service unit tests, load/stress, and network tests (skipped in CI).
- **Rust:** `cargo test --manifest-path src-tauri/Cargo.toml` — wallet CHI/Wei conversion, payment verification, offer signing/verification, the split-payment invariant (`credit + fee == amount`), reputation storage, and version-policy sign/verify.
- **Scaled:** Docker/k3s multi-node harness for DHT discovery and offer propagation across many nodes.

---

## Configuration

### Environment Variables (selected)

| Variable | Default | Description |
|----------|---------|-------------|
| `CHIRAL_RPC_ENDPOINT` | `http://130.245.173.73:8545` | Remote blockchain RPC fallback |
| `CHIRAL_GETH_SYNCMODE` | `full` | Geth sync mode (`full` or `snap`) |
| `CHIRAL_DAEMON_PORT` | `9419` | Daemon HTTP port |
| `CHIRAL_AUTO_START_DHT` | `false` | Auto-start DHT on daemon boot |
| `CHIRAL_AUTO_MINE` | `false` | Auto-start mining (implies DHT + Geth) |
| `CHIRAL_MINER_ADDRESS` | none | Wallet address for mining rewards |
| `CHIRAL_MINING_THREADS` | `1` | CPU mining thread count |
| `CHIRAL_POLICY_PUBLIC_KEY` | placeholder zeros | 32-byte hex Ed25519 key that activates signed `VersionPolicy` updates |

### Data Directories

| Platform | Path |
|----------|------|
| Linux | `~/.local/share/chiral-network/` |
| macOS | `~/Library/Application Support/chiral-network/` |
| Windows | `%APPDATA%/chiral-network/` |

Subdirectories include `geth/` (blockchain data, archive mode), provider stores for served resources, and `tx_metadata.json` (persisted transaction metadata).
