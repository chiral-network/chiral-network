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
- [Settlement and Balances](#settlement-and-balances)
- [Reputation System](#reputation-system)
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

1. Consumer searches the DHT for offers of a class (e.g. `storage`), receiving signed offers.
2. Consumer verifies each offer's signature, drops invalid ones, and ranks survivors by price and provider Elo.
3. Consumer funds a prepaid balance: an on-chain CHI payment to the chosen provider's wallet.
4. Provider verifies the payment against the chain (mined, correct recipient, amount, chain ID) and credits the consumer's balance, splitting off the platform fee via `split_payment`.
5. Consumer uses the resource over the provider's HTTP API (S3 / submission / OpenAI-compatible).
6. Provider meters usage and draws the balance down at published rates; service pauses when the balance is exhausted until the consumer tops up.
7. Consumer submits a payment-gated rating; the provider's reputation updates.

---

## Resource Offers (Discovery)

> **Status: the offer record generalizes today's signed host-advertisement machinery (`hosting.rs`, `hosting/publish-ad`, `hosting/registry`) from a single "hosting" type to a typed, multi-class offer.** The signing, publish/refresh, and read-verify paths are reused.

A resource offer is a signed DHT record. The signed payload carries:

| Field | Meaning |
|-------|---------|
| `provider_wallet` | secp256k1 address; the payee and the reputation subject |
| `resource_class` | `storage` \| `container` \| `inference` |
| `capacity` | class-specific descriptor (GB available; CPU/mem/GPU; model IDs) |
| `price_schedule` | CHI per metered unit (GB-month + egress; container-hour / GPU-hour; per-1K input/output tokens) |
| `endpoint` | public base URL of the provider's HTTP API (scheme + host [+ port]) |
| `region` | optional locality hint |
| `valid_until` | Unix-seconds; readers ignore expired offers |
| `signature` | ECDSA over the length-prefixed, domain-tagged payload above |

- **Key namespace.** `chiral_offer_<class>_<wallet>` in the DHT; the provider also registers as a Kademlia provider for the class so consumers can enumerate sellers. Reserved namespaces reject raw `dht_put` (403) — offers are writable only through the signed publication command.
- **Publish / refresh / expire.** A provider republishes on an interval; readers drop records past `valid_until`, so a provider that stops refreshing falls out of the catalog.
- **Read path.** `search_offers(class)` collects records, verifies signatures, drops unsigned/invalid, and returns survivors with the provider Elo attached (batch reputation lookup). As in the content design, the first signature-valid replica may be acted on without waiting for quorum convergence.

---

## Resource Interfaces

Each class is a standard HTTP API the provider serves at its offer's `endpoint`. Consumers use existing tooling; the backend supplies helpers and a provider-side server.

### Storage — S3-compatible

> **Status: new provider-side server; replaces the retired chunked file-transfer protocol.**

- Object operations: `PUT`/`GET`/`DELETE` object, list bucket, and presigned URLs for time-boxed anonymous access.
- Addressing: an object may be keyed by the SHA-256 of its content; the object's ETag is that hash, so a consumer verifies integrity against the name it requested. Content-addressed public-read objects are the successor to "sharing a file by its hash."
- Metering: GB-month stored (sampled) + GB egress.
- Auth: writes and management use the owner-proof scheme ([Security](#security-implementation)); public-read objects are unauthenticated by design.

### Containerized compute

> **Status: specified; v1 reference provider runs a single-node container runtime.**

- Submit: `POST` a container spec (image ref, CPU/mem/GPU request, ports, env); receive a handle + endpoint.
- Lifecycle: `GET` status/logs, `DELETE` to tear down (stops metering).
- Metering: wall-clock runtime at the per-hour (or per-GPU-hour) rate.

### LLM serving — OpenAI-compatible

> **Status: specified; provider fronts a served model with a token meter.**

- Endpoints mirror the OpenAI HTTP API (e.g. `POST /v1/chat/completions`, `GET /v1/models`).
- Metering: input + output tokens at the per-1K rates in the offer; usage is returned in each response so the consumer can reconcile against its balance drain.

---

## Settlement and Balances

> **Status: balance accounting is new; it is built from the existing single-shot payment-verification primitives (`wallet::verify_tx_details`, the spent-tx ledger, `speed_tiers::split_payment`).**

Settlement reuses the on-chain payment path and adds per-`(consumer, provider)` balance accounting on the provider side.

1. **Fund.** The consumer sends CHI to the provider's wallet and presents the tx hash. The provider verifies it exactly as a content seeder verified a download payment: mined (`wait_for_tx_mined`), recipient is the provider, amount is credited in full, and `tx.chainId == geth::chain_id()` (cross-chain replay rejected). The tx is recorded in a spent-tx ledger keyed on `(tx_hash, provider)` so a funding payment credits exactly one balance.
2. **Credit + fee.** `split_payment(amount)` divides the payment into the provider's credit and the platform fee (default 0.5%, 0.1% floor) with exact `u128` integer arithmetic; `credit + fee == amount` exactly. The fee is forwarded to the platform wallet.
3. **Draw down.** As the consumer uses the resource, the provider meters usage and decrements the balance at the offer's rates. Metering is provider-side (the honest-provider assumption of Part I §9); the consumer reconciles against observable usage (object listings, returned token counts, container uptime).
4. **Top up / exhaust.** When the balance runs low the consumer funds again (a new payment, steps 1–2). When it reaches zero, service pauses. **Balances are non-refundable** — there is no withdrawal path in v1.

---

## Reputation System

> **Status: the Elo engine, on-chain verification of the rating event, and the batch lookup are reused as-is (`rating_api.rs`, `rating_storage.rs`, the relay `/api/ratings/*` routes). The change is admitting a subjective score in the event, gated by verified payment.**

The design rationale and formula are in [Part I](#part-i-white-paper), §7. A rating event carries the provider wallet, the rater wallet, the normalized subjective score, the CHI amount, the funding `tx_hash`, and a timestamp. Before an event is admitted, the relay verifies the `tx_hash` on-chain (sender = rater, recipient = provider, amount) — the backend does not trust client-submitted event data — so **only a consumer who actually paid a provider can move that provider's score**, and the move is weighted by amount and recency.

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
