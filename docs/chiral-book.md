# Chiral Network

Chiral Network is a decentralized file sharing application built on peer-to-peer networking with a native blockchain for payments and reputation tracking. It runs as a desktop application (Tauri 2 + Svelte 5 + Rust) and as a headless daemon for server deployments and automated testing.

This book is in two parts:

- **[Part I: White Paper](#part-i-white-paper)** — the conceptual design: what the system guarantees and why, at the level of mechanisms rather than code, with no implementation detail.
- **[Part II: Design and Implementation](#part-ii-design-and-implementation)** — the concrete realization: architecture, modules, wire protocols, parameters, APIs, deployment, and operations.

---

# Part I: White Paper

*Chiral Network: A Peer-to-Peer Market for File Storage and Retrieval*

**Abstract.** A purely peer-to-peer file sharing network allows content to be published, discovered, and retrieved without a central operator. Existing peer-to-peer systems solve discovery and transfer but not persistence: a file remains available only as long as volunteers choose to serve it, and no mechanism compensates them for doing so. Chiral Network treats storage and bandwidth as goods in a market. Files are named by their content hash and discovered through a distributed hash table; transfers are settled in a native proof-of-work currency; and every record a peer acts on — file metadata, seeder advertisements, folder manifests, price quotes — is signed by the publisher's wallet key, so authenticity is verified cryptographically rather than assumed from whichever node served the record. A reputation function derived solely from verified transfer outcomes, rather than subjective ratings, gives buyers a forgery-resistant signal of seller reliability. The result is a network where availability is purchased rather than donated, and where no intermediary can forge, reprice, or redirect what peers publish.

## 1. Introduction

Distribution of digital content today relies almost entirely on central services. The model works well until it doesn't: the operator is a single point of failure and censorship, extracts rent from both sides of every exchange, and accumulates a complete record of who stores and retrieves what. Peer-to-peer file sharing networks were built to remove the central operator, and at the level of mechanics they succeeded — distributed hash tables solve discovery, and swarming protocols solve transfer.

What they did not solve is the economics. In volunteer networks, serving a file costs bandwidth and storage and earns nothing, so availability decays with interest: popular content is over-replicated while everything else quietly disappears when its last altruistic seeder departs. Reciprocity schemes (tit-for-tat) only reward peers during a shared download and create no incentive to keep older content alive.

What is needed is a way to pay for retrieval and persistence directly, peer to peer, with payment and delivery bound tightly enough that neither side must trust the other, and without reintroducing an intermediary who can forge listings, substitute payees, or manufacture reputation. This paper describes such a system. Chiral Network combines four mechanisms:

1. a **content-addressed discovery layer** in which every published record is signed by its owner's key;
2. a **chunked transfer protocol** with per-chunk integrity verification;
3. a **native currency** on a proof-of-work blockchain, with payment verified on-chain before service is rendered; and
4. an **outcome-based reputation function** computed from verified transfers rather than from ratings.

## 2. System Overview

The network is organized as three planes that share one identity system.

**Discovery plane.** A Kademlia distributed hash table stores small signed records: file metadata, seeder advertisements, and folder manifests. Any peer may store and serve these records; none is trusted to have authored them.

**Transfer plane.** File content moves directly between peers over a chunked request–response protocol. Chunks are verified individually as they arrive and the assembled file is verified against its content hash.

**Settlement plane.** A proof-of-work blockchain (Ethash, account-based) carries the native currency, CHI. Miners issue currency and order transactions; sellers verify payment on-chain before serving content.

**Identity.** A participant's identity is a single ECDSA keypair — the same key that controls their currency balance signs their published records. This unification is deliberate: it means the entity that *gets paid* for a file is cryptographically the entity that *published* it, and it lets reputation attach to the address that actually receives money. A peer's network-transport identity (its DHT node ID) is distinct, but every record binds the two by signature, so a transport identity cannot impersonate a wallet.

Participants play five roles, in any combination: **publishers** announce files and set prices; **consumers** pay for and retrieve them; **miners** secure settlement and earn block rewards; **relay operators** provide reachability to peers behind NAT; and **persistent hosts** sell always-on storage so that availability survives the publisher going offline.

## 3. Signed Records as the Trust Primitive

The foundational rule of the network is that *data is trusted because of who signed it, never because of where it came from*. A DHT is an adversarial place: any node can claim to hold any key and answer with anything. Chiral Network therefore treats the DHT purely as an untrusted bulletin board.

Every long-lived record carries an ECDSA signature by the wallet that owns it:

- **File metadata** — name, size, price, and payment address for a content hash, signed by the publisher.
- **Seeder advertisements** — "peer *P* serves file *F* for wallet *W*", signed by *W*, binding the transport identity to the paid identity.
- **Folder manifests** — a file list with a bundle price and payee, signed by the folder's owner (Section 7).
- **Transfer envelopes** — the price and payee quoted at the moment of transfer, signed by the seeder (Section 5).

Two symmetric rules enforce the contract. *Writers refuse to publish unsigned:* a client that cannot sign (because its key is locked or absent) declines to write rather than emit an unverifiable record. *Readers drop invalid:* a record whose signature is missing or wrong is treated as nonexistent, however plausible its contents.

Signatures are computed over a canonical, length-prefixed, domain-tagged encoding of the record's fields. Length prefixing makes the serialization injective — no two distinct field tuples produce the same signed bytes — and the domain tag prevents a signature produced for one record type or protocol from being replayed as another.

Records under a given key are owned by first claim: once a key has been written by a wallet, only that wallet's signature can overwrite it. An adversary can neither hijack an existing name nor re-publish an existing record with altered contents, because alteration breaks the signature and replacement requires the original signer's key.

A useful consequence of pushing authenticity into the records themselves is that *replication quorums become unnecessary for reads*. A conventional DHT read waits for several replicas to converge before trusting a value; here, the first replica whose signature verifies is as good as any majority, so a reader may act on the first arrival. This collapses lookup latency without weakening integrity — a property quorum systems cannot offer, since they derive trust from agreement among storage nodes rather than from the data.

## 4. Content Addressing and Discovery

A file's identifier is the SHA-256 hash of its content. Content addressing makes the name self-certifying: whatever a consumer receives can be checked against the name it asked for, so a correct file cannot be substituted regardless of who served it.

To publish, a peer writes the signed metadata record under the file's hash, writes a signed seeder advertisement binding its own transport identity to that hash, and registers as a provider in the DHT's provider index. To search, a consumer looks up the hash and assembles the metadata, the set of seeder advertisements, and the provider list.

Verification is per-record, so a search degrades gracefully under partial forgery: if the metadata record fails verification but seeder advertisements verify, the consumer discards only the untrusted fields (the display name, the claimed payment address) and still surfaces the honest seeders. A single forged or corrupted record therefore cannot make a well-seeded file unfindable — an attack that would succeed in systems where one metadata object vouches for everything beneath it.

Seeder advertisements are refreshed periodically and expire otherwise, so the seeder set self-cleans as peers depart; a peer that stops sharing a file affirmatively withdraws its advertisement rather than lingering as a ghost.

## 5. File Transfer

Content is transferred directly between consumer and seeder in fixed-size chunks over a request–response protocol.

The exchange begins with a metadata envelope in which the seeder states the file's size, chunk count, price, and payment address, *signed by the seeder's wallet key*. This signature closes the most direct theft in a payment-bearing transfer protocol: an unauthenticated quote would let any peer that can answer a request — or any intermediary — substitute its own payment address and collect the price of a file it does not own. A consumer that receives an invalid envelope simply fails over to the next seeder.

Chunks are then requested in sequence. Each chunk carries its own hash and is verified on receipt; out-of-sequence chunks are rejected before they consume bandwidth; failed chunks are retried a bounded number of times and then the transfer fails over to another seeder. When all chunks have arrived, the assembled file is hashed and compared with the content address — the per-chunk checks localize errors cheaply, and the full-file check is the backstop that makes the transfer end-to-end self-certifying.

## 6. Payments

Payment precedes service. For a priced file the consumer first pays the seeder's advertised address on-chain, then presents the transaction hash with its chunk requests; the seeder serves no content until it has verified the payment itself.

Verification is done against the chain directly, never against the consumer's claims. The seeder checks that the transaction is mined, that the recipient is its own address, that the amount covers the price, and that the transaction was made on this network's chain — the chain identifier check rejects replays of signed transactions captured from other chains. The two failure modes are deliberately distinguished: *not yet mined* is a transient verdict inviting retry, while *wrong recipient or amount* is permanent, so honest consumers are not punished for settlement latency and dishonest ones cannot retry their way past arithmetic.

A payment authorizes exactly one delivery. Each seeder keeps a ledger of spent transactions keyed by the *(transaction, file)* pair: the same transaction presented twice for the same file is refused, and a payment for one file cannot be redeemed for a different one — necessary because one wallet may sell many files at many prices. The ledger records a transaction only on successful verification, so a transiently-failed presentation can be retried safely.

A file's price is set by its seller; the consumer pays exactly the price quoted in the seeder's signed envelope. (What a seller *should* ask is anchored by the network-computed reference fee of Section 8, which informs defaults and estimates but never enters verification.) From each payment a small platform fee is split off, computed with exact integer arithmetic such that the seller's share and the fee sum precisely to the total — there is no floating-point rounding anywhere in the payment path, because rounding tolerances in payment verification are exploitable margins. Free files (price zero) skip payment entirely.

## 7. Folder Bundles

A folder can be sold as a single product at a single price, rather than as the sum of its files.

A folder's identifier is content-derived, like a file's: the hash of the owner's address together with the sorted list of *(relative path, file hash)* pairs it contains. The construction is deterministic and order-independent — the same owner publishing the same file set always produces the same folder identity — and owner-bound, so two sellers offering identical contents have distinct folder identities and neither can claim the other's.

The owner publishes a signed manifest under this identifier carrying the file list, the bundle price, and the payment address. Price and payee live *inside* the signed payload; a hostile peer cannot take a popular folder's manifest and republish it with its own wallet substituted, because the substitution breaks the owner's signature. Buyers pay once to the manifest's address and then retrieve each member file at price zero. Search reports the *common-seeder intersection* — the peers that hold every file in the bundle — so a buyer can judge whether the whole bundle, not merely its pieces, is retrievable.

In the current design, member files published at price zero are individually retrievable by anyone holding their bare content hashes; binding member retrieval to proof of bundle purchase is future work (Section 13).

## 8. Incentives

Every behavior the network needs is paid for; none relies on altruism.

**Seeding.** Sellers earn their asking price on every download. Storage of other people's content is compensated through the hosting marketplace, where peers advertise offers and form agreements with publishers; agreed files are then seeded like the host's own.

**Mining.** Proof-of-work miners earn a fixed block reward of 5 CHI, which is also the currency's issuance mechanism. New participants thus have a path to acquiring currency by contributing computation, rather than only by selling content. Transactions carry no gas price — settlement is free to users, and the chain's security budget is the block reward alone.

**Persistent hosting.** Always-on hosts sell durability: a publisher pays for a hosting duration, the host keeps the file discoverable and served while the lease runs, and the lease expires automatically — paid persistence rather than indefinite donation. So that infrastructure neither undercuts the peer market nor gouges where peers are scarce, a host's asking price is indexed to it: the price is the greater of a floor and 1.2× the median of current peer asks for comparable service. Hosts thereby track the market upward and downward but always sit slightly above it — they are the convenience option, not a subsidized monopolist.

**Price discovery.** Sellers set prices freely, but a young market gives them little to set prices against. The network therefore computes a **reference download fee** — a rate in CHI per megabyte used as the default ask for new listings, the cost estimate shown to buyers, and the floor in the persistent-host pricing rule above. The reference fee is advisory by construction: it never enters payment verification, so peers need not agree on it bit-for-bit and no fee oracle has to be trusted. Each node derives it from data it already holds:

$$f \;=\; \mathrm{clamp}\!\Big(M \cdot \big(H_0/H\big)^{1/2},\; f_{\min},\; f_{\max}\Big)$$

The market term $M$ is the median per-megabyte price of the fee-bearing transfers settled in the trailing two weeks. The sample is verifiable: fee-bearing transfers are enumerable from the chain itself, since each pays the platform split to a known address, and the corresponding file sizes are bound by seller-signed metadata. Using the median means the term moves only when more than half of paid volume moves, and the platform fee prices every transaction an attacker would have to spend steering it. The supply term $(H_0/H)^{1/2}$ tracks the network's hash power $H$, estimated from recent block headers as accumulated difficulty over elapsed time ($H_0$ is a fixed calibration reference). Hash power measures the real resource cost of producing a unit of CHI: as it rises, each CHI embodies more expended work, so the CHI-denominated fee falls — holding the *real* price of a megabyte roughly steady as the network grows — and the square root damps the response to mining volatility. The two terms are complementary estimators of the same quantity: $M$ reads the price level off realized demand, the hash-power term off the currency's production cost, and the latter carries the index while the former is thin. Finally, the clamp bounds the index to a fixed band around a bootstrap anchor $f_0$, the published value moves by at most a fixed fraction per day, and when too few transfers exist in the window $M$ falls back to $f_0$ — which is also the network's launch value. Because the index is advisory, sellers deviate from it freely, which keeps the index-to-market feedback loop loose; the clamp, the daily rate limit, and the median's robustness bound what remains.

The loop closes: consumers obtain CHI by mining or by selling; they spend it on retrieval; sellers and hosts earn it for availability; miners earn it for ordering everyone's settlement.

## 9. Reputation

Buyers choosing among seeders need a signal of reliability. Subjective star ratings are the obvious mechanism and were deliberately rejected: ratings are free to fabricate, so any identity can manufacture a history of praise. Chiral Network computes reputation only from *transfer outcomes* — completions and failures of actual transfers, with paid transfers verified against the chain (sender, recipient, amount) before the event is admitted. Fabricating a positive history therefore costs real payments, and the platform fee on each makes wash-trading reputation a strictly losing proposition rather than a free one.

Each wallet carries a score on a 0–100 scale, initialized to 50, updated per event in the style of the Elo rating system. For an event with outcome $S$ (1 for a completed transfer, 0 for a failed one) against a wallet whose current score is $r$:

$$E = \frac{1}{1 + 10^{(50 - r)/12}}, \qquad r \leftarrow \mathrm{clamp}\big(r + K\,(S - E),\ 0,\ 100\big), \qquad K = 4\,w_t\,w_a$$

The expected-outcome term $E$ gives the update its useful curvature: a high-scored wallet gains little from yet another success but loses sharply on a failure, while a low-scored wallet can climb quickly by performing well. The weight $w_t$ decays linearly from 1 to 0 over a 180-day lookback, so reputation reflects recent conduct and both old sins and old glories expire. The weight $w_a = 1 + \min(1, \ln(1+a)/\ln 51)$, where $a$ is the CHI amount, lets larger verified payments move the score up to twice as much as trivial ones — weight grows logarithmically, so reputation cannot simply be bought in one large transaction.

Scores are displayed alongside search results, so reliability directly affects a seller's ability to win business.

## 10. Trust Model

It is as important to state what the network does *not* protect as what it does.

**Out of scope: anonymity.** Chiral Network is not an anonymity network. Wallet addresses, transport identities, IP addresses, and which files a peer publishes or requests are observable by network participants, as in any unencrypted peer-to-peer system. The guarantees below are guarantees of *integrity and authenticity*, not of unlinkability.

**In scope.** The signed-record discipline of Section 3, applied end to end, yields the following properties:

- **No forged listings.** A record not signed by its owner is dropped by every reader. The discovery layer can lie about availability but not about content, price, or payee.
- **No payment redirection.** Every quote a consumer pays against — metadata, manifest, transfer envelope — has the payee inside a signed payload. Redirecting payment requires the seller's private key.
- **No replay, in any direction.** Cross-chain replay of signed transactions is rejected by the chain-identifier check; cross-file and double-spend replay by the *(transaction, file)* ledger; signature malleability is eliminated by accepting only canonical (low-*s*) signatures, so a signature cannot be mutated into a "different" one for the same message. Requests to authenticated services are signed over the method, path, and a timestamp, valid for a few minutes — a captured proof cannot be replayed against a different endpoint or later in time.
- **No name hijacking.** First-claim-wins ownership (Section 3) means an existing record is replaceable only by its original signer. Reserved record namespaces refuse raw writes entirely; they are writable only through the signed publication paths.
- **No infrastructure-relayed reach into private networks.** Services that register publisher-supplied URLs validate them against a public-address policy, rejecting private, link-local, and cloud-metadata address ranges in all their encodings — a registration cannot be used to aim infrastructure at targets inside someone's network perimeter.

**Residual trust.** Proof-of-work settlement is as strong as the honest share of hash power, which on a young network is modest in absolute terms (Section 13). The reputation registry is currently an aggregation service that clients trust to tally verified outcomes honestly (Section 13). And a seller who takes payment and refuses service can still do so once per victim — what the system guarantees is that such conduct is recorded against the wallet that got paid, and prices its future business accordingly.

## 11. Network Infrastructure

Two classes of infrastructure improve availability. Neither is a trust root: every record they serve is verified by its signature regardless of origin, so compromising them degrades reachability, not authenticity.

**Relays.** Most consumer peers sit behind NAT and cannot accept inbound connections. Relay nodes provide circuit relay — forwarding traffic between peers that cannot connect directly — plus a stable entry point for bootstrapping into the DHT. Relays practice routing hygiene: only publicly reachable addresses enter the routing tables they propagate, so the tables are not polluted with dial targets that can never succeed. Relays also host registry services — the reputation tally, the version-policy endpoint (Section 12), and a registry mapping published shares and sites to their origins, where registrations are signed by their owners and governed by the same first-claim-wins rule as DHT records.

**Persistent hosts.** The always-on hosts of Section 8 are economically distinguished peers, not protocol-privileged ones. They speak the same publication and transfer protocols, sign records with their own wallets, verify payments like any seller, and their leases expire by the same clock they charge by.

## 12. Protocol Governance

A peer-to-peer network has no operator who can force an upgrade, yet a client build with a known payment or signature flaw endangers its counterparties, not just itself. Chiral Network handles this with a signed **version policy**: a small document stating the minimum tolerated client version, the currently recommended one, and the time it was issued.

The policy is distributed by the network itself — any node serves its current view — and authenticated independently of its carrier: policies are signed by an offline project key, and a fetched policy replaces the current one only if it verifies and is no older than what the client already holds. The freshness rule prevents rollback: a hostile or stale node cannot revive an obsolete policy to re-admit vulnerable builds. (During initial deployment, before the signing key is in service, unsigned policies are accepted only if they do not raise the minimum above what the client's own build shipped with — a hostile relay can nudge peers to upgrade but can never lock honest clients out.)

Enforcement is deliberately layered, so no single bypass disables it: the user interface warns below the recommended version and blocks below the minimum; the client refuses to join the network or begin downloads when unsupported; services reject requests from outdated clients; and peers check each other's advertised versions on connection and disconnect those below the minimum. An old client is thus squeezed out of the network from four directions at once.

## 13. Limitations and Future Work

Stated plainly, in roughly decreasing order of consequence:

- **Settlement security is proportional to honest hash power.** A private proof-of-work chain with a small mining population is cheaply attackable by a determined adversary with rented computation. The economic design (Section 8) is sound at any scale, but the finality of payments is only as strong as the mining base; growing it — or anchoring settlement to a stronger chain — is the most important open item.
- **The reputation registry is centralized.** Outcome events are verified against the chain, but a single service tallies them; it could censor or misreport. Because events are on-chain-verifiable, the natural evolution is federation or client-side recomputation from attested events, removing the trusted tally.
- **Folder purchases do not yet gate member files.** Bundle members are published at price zero and are individually retrievable by anyone with their bare hashes. Closing the gap requires carrying proof-of-bundle-purchase in the transfer protocol's payment presentation.
- **One on-chain transaction per purchase.** Per-download settlement is acceptable at file scale but wrong for micro-purchases; payment channels or batched settlement are the standard remedies.
- **No anonymity** (Section 10). Confidential retrieval — private information retrieval, onion routing of transfers — is an explicit non-goal at present.
- **Relays are an availability concentration.** Discovery and transfer are fully decentralized, but bootstrap, NAT traversal, and the registries lean on few nodes today. The protocols themselves are relay-agnostic; diversifying operators is deployment work rather than design work.

## 14. Conclusion

We have described a peer-to-peer file sharing network organized as a market. Content addressing makes names self-certifying; a DHT provides discovery without privileged servers; and a native proof-of-work currency lets retrieval and persistence be bought and sold rather than donated. The system's distinguishing commitment is that *every record a peer acts on is signed by the wallet that profits from it* — discovery infrastructure is thereby reduced to an untrusted bulletin board, payment redirection and record forgery are excluded by construction rather than by policy, and reputation can be computed from verified economic events instead of fabricable ratings. Availability becomes a priced good: files persist not while someone remembers to be generous, but while someone finds it worth paying for.

---

*This paper describes the design at the level of mechanisms and guarantees. For the concrete realization — module layout, wire protocols, parameters, deployment, and operations — see [Part II: Design and Implementation](#part-ii-design-and-implementation).*

---

# Part II: Design and Implementation

This part covers the concrete realization of Chiral Network — architecture, modules, wire protocols, parameters, APIs, deployment, and operations. For the conceptual design (what the system guarantees and why), read [Part I](#part-i-white-paper) first.


---

## Table of Contents

- [Architecture](#architecture)
- [Feature Reference](#feature-reference)
- [Security Implementation](#security-implementation)
- [Getting Started](#getting-started)
- [Application Pages](#application-pages)
- [Backend Modules](#backend-modules)
- [Blockchain and Mining](#blockchain-and-mining)
- [Reputation System](#reputation-system)
- [Dynamic Fee Index](#dynamic-fee-index)
- [Version Enforcement](#version-enforcement)
- [File Transfer Protocol](#file-transfer-protocol)
- [Headless Mode and CLI](#headless-mode-and-cli)
- [Docker and Scaled Testing](#docker-and-scaled-testing)
- [Testing](#testing)
- [Project Structure](#project-structure)
- [Configuration](#configuration)

---

## Architecture

The application consists of three layers:

1. **Frontend** -- Svelte 5 with TypeScript, rendered in a Tauri webview or browser.
2. **Backend** -- Rust, handling P2P networking (libp2p), blockchain interaction (Geth), file transfer, and local storage.
3. **Blockchain** -- A private Ethash proof-of-work chain (chain ID 98765) where users mine CHI tokens and pay for file downloads.

### Tech Stack

| Layer | Technology | Version |
|-------|-----------|---------|
| Desktop shell | Tauri | 2.x |
| Frontend framework | Svelte | 5.38 |
| Frontend language | TypeScript | 5.7 |
| Build tool | Vite | 7.1 |
| Styling | TailwindCSS | 3.4 |
| Backend language | Rust | 2021 edition |
| P2P networking | libp2p | 0.53 |
| HTTP server | Axum | 0.7 |
| Blockchain client | Core-Geth | 1.12.20 |
| Crypto | ethers.js (frontend), secp256k1 + ed25519-dalek (backend) |

### Component Diagram

```
+-----------------------------------------------------------+
|                     Desktop Application                    |
|  +-------------------+    +----------------------------+  |
|  |   Svelte 5 UI     |    |     Tauri IPC Bridge       |  |
|  |  (Pages, Stores,  |--->|  invoke() / listen()       |  |
|  |   Services)        |    |                            |  |
|  +-------------------+    +----------------------------+  |
+-----------------------------------------------------------+
            |                           |
            v                           v
+-----------------------------------------------------------+
|                     Rust Backend                           |
|  +----------+  +---------+  +--------+  +-------------+  |
|  | DhtService|  | Geth    |  | Drive  |  | File        |  |
|  | (libp2p)  |  | Process |  | API    |  | Transfer    |  |
|  +----------+  +---------+  +--------+  +-------------+  |
|  +----------+  +---------+  +--------+  +-------------+  |
|  | Wallet   |  | RPC     |  | Hosting|  | Encryption  |  |
|  | (wallet. |  | Client  |  | Server |  | Keypair     |  |
|  |  rs)     |  | (pooled)|  |        |  |             |  |
|  +----------+  +---------+  +--------+  +-------------+  |
+-----------------------------------------------------------+
            |                           |
            v                           v
+-------------------------+    +------------------------+
|   P2P Network (libp2p)  |    |   Blockchain (Geth)    |
|   Kademlia DHT          |    |   Ethash PoW chain     |
|   TCP + Noise + Yamux   |    |   Chain ID: 98765      |
|   File chunk protocol   |    |   RPC: localhost:8545   |
+-------------------------+    +------------------------+
            |
            v
+-------------------------+
|   Relay Server           |
|   130.245.173.73         |
|   :4001 libp2p relay     |
|   :8080 HTTP API         |
|   - Circuit relay v2     |
|   - Kademlia routing     |
|   - Reputation API       |
|   - Drive share proxy    |
|   - WebSocket tunnels    |
|   - Email backup relay   |
+-------------------------+
```

### Data Flow: File Download

1. Publisher registers a file on the DHT with its hash, name, size, price, and peer ID.
2. Consumer searches the DHT by file hash or magnet link.
3. Consumer sees the file info, seeder list, and Elo scores.
4. Consumer confirms the download. If the file has a price, CHI is sent to the seeder's wallet — at the price quoted in the seeder's signed `FileInfo` envelope, split between seller and platform wallet via `split_payment` (default 0.5% platform fee).
5. Consumer's node sends chunk requests to the seeder over the libp2p file transfer protocol.
6. Each 256 KB chunk is SHA-256 verified on receipt.
7. After all chunks arrive, the full file hash is verified.
8. The file is saved to the download directory and optionally added to Drive.

---

## Feature Reference

### File Sharing
- Publish files to the DHT so other peers can discover and download them.
- Chunked file transfer protocol (256 KB chunks) with SHA-256 verification per chunk and full-file hash verification on completion.
- Set a CHI price per file. Payments are processed on-chain before the download begins; the buyer pays the seeder's signed price (the earlier burn-address per-MB download fee was removed to avoid double-charging).
- Platform fee on all transactions — default 0.5%, adjustable down to a 0.1% floor (split between seller and platform wallet; the fee is a cut of the listed price, not a surcharge added on top).
- The client-side cost *estimate* (`calculate_download_cost` Tauri command, `chiral download cost` CLI) is not charged by the payment path. It currently uses the static anchor 0.01 CHI/MB; the [Dynamic Fee Index](#dynamic-fee-index) specifies its network-derived replacement (hashpower + 14-day median of paid-transfer prices).

### Folder Bundles
- Sell an entire folder as a single product at a folder-level price — buyers pay once for the bundle, not the sum of per-file prices.
- The folder hash is content-addressed: SHA-256 of the owner address plus the sorted `(rel_path, file_hash)` list of every file in the folder. Same files + same owner always produce the same hash, so re-publishes are stable.
- The seller publishes a signed `chiral_folder_<hash>` manifest to the DHT and registers as a Kademlia provider for that hash. The manifest carries `priceWei` and `walletAddress` (both inside the signed payload, so a hostile peer can't substitute a different price/recipient for an existing folder hash). Child files are published at price 0 — payment is collected once at the folder level.
- Buyers paste the folder hash into Search, see the file list, the bundle price, and the set of seeders that hold every file in the bundle (the "common seeders" intersection).
- "Buy Folder for X CHI" sends one transaction to the folder's payment wallet and then dispatches each child file's chunked transfer at price 0 — no per-file payment loop.
- Known V1 gap: child files remain individually downloadable at price 0 when a buyer has the bare file hash. Closing this requires extending the chunked-transfer PaymentProof with folder context (tracked as a follow-up).

### ChiralDrop
- Direct peer-to-peer file transfer between two users, similar to AirDrop.
- Discover nearby peers on the network.
- Accept or decline incoming transfer requests.
- Optional pricing for paid file drops.

### Drive
- Local file management system with folders, uploads, renaming, starring, and deletion.
- Files are stored locally at `~/.local/share/chiral-network/chiral-drive/`.
- Seed files to the P2P network directly from Drive.
- HTTP preview pages for downloaded files (images, video, audio, PDF, text).

### Mining
- CPU mining with configurable thread count and utilization percentage.
- GPU mining support via ethminer (limited to older NVIDIA GPUs, Compute Capability 7.5 and below).
- Mining rewards are 5 CHI per block.
- Real-time hash rate display via eth_hashrate RPC.

### Wallet
- Generate a new wallet from a 12-word BIP39 mnemonic.
- Import an existing wallet using a private key or recovery phrase.
- Send and receive CHI tokens.
- Optional one-time email backup of wallet credentials.
- Transaction history with type classification (send, receive, download payment, file sale).

### Reputation (Elo)
- Each wallet has an Elo reputation score (0-100) derived from file transfer outcomes.
- Completed transfers increase the score; failed transfers decrease it.
- Time-weighted: recent events within a 180-day lookback period carry more weight.
- Amount-weighted: larger transfers have a proportionally larger effect (logarithmic scaling).
- Batch lookup available for displaying seller reputations on the download page.

### Hosting Marketplace
- Publish a host advertisement to offer storage to the network.
- Browse available hosts, propose hosting agreements, and track active agreements.
- Hosted files are automatically seeded to the DHT.
- CDN Servers tab: always-on infrastructure servers separated from peer hosts.

### CDN Service
- Always-on file hosting servers that keep files available when the uploader goes offline.
- Market-based dynamic pricing: `max(floor_price, median_peer_price × 1.2)`.
- Payment required before upload — verified on-chain with exact integer math.
- Uploader sets a download price that other users pay to download from the CDN.
- Files auto-expire and are cleaned up when the paid hosting duration elapses.
- CDN re-seeds all active files to DHT on startup (15s after bootstrap).
- Expiration cleanup runs every 60 seconds — files past their paid duration are removed from disk and from the DHT seeder list.
- CDN can also host static sites (HTML/JS/CSS bundles), separate from per-file uploads.
- Download page queries CDN servers directly as fallback when DHT search is slow.
- Deployed at `130.245.173.73:9420` with 227 GB capacity.
- Desktop app: Hosts → CDN Servers tab → Upload from Drive with payment confirmation.

---

## Security Implementation

The trust model and its rationale are in [Part I](#part-i-white-paper) (Sections 3, 6, 10). This section is the implementation-level inventory. Note the scope: these are integrity / authenticity protections — Chiral Network is not an anonymity network, and wallet addresses, peer IDs, IPs, and publish/request patterns are observable by participants.

**Signed records (writers refuse to publish unsigned; readers drop unsigned/invalid):**

- File metadata (`chiral_file_<hash>`) — signed by publisher wallet over a length-prefixed canonical payload. `search_file` rejects unsigned/invalid metadata as not-found.
- Seeder entries (`chiral_seeder_<hash>_<peer>`) — signed by the seeder's wallet, binding peer ID + file hash + wallet address. `fetch_seeders` drops empty-signature non-stub entries.
- Folder manifests (`chiral_folder_<hash>`) — signed by `owner_wallet` over a payload that includes the folder's `priceWei` and `walletAddress`, so a hostile peer can't republish the same hash with a swapped price/recipient. The Tauri `search_folder` and the headless `POST /api/headless/folder/search` both verify and drop unsigned/invalid bundles. Manifests published before folder-level pricing existed (v1) are still accepted via a fallback gated on the pricing fields being empty.
- Chunked-transfer `FileInfo` envelopes — signed by the seeder's wallet. The downloader verifies before consuming the seeder's claimed `wallet_address` / `price_wei` and fails over to other seeders on bad signatures (closes the payment-redirection vector where a hostile seeder could substitute its own wallet).

**HTTP authentication (replaces the previously-trusted bare `X-Owner` header):**

- Authenticated routes require both `X-Owner: 0x<wallet>` and `X-Owner-Sig: <unix_ts>:<hex_signature>` headers.
- Signed payload is length-prefixed canonical bytes binding wallet ↔ HTTP method ↔ path-with-query ↔ timestamp; a captured proof can't be replayed against a different endpoint within its ±5-minute window.
- Server-side `auth::owner_proof_middleware` recovers the secp256k1 signer and rejects with 401 on mismatch / expiry.
- Applied to: `/api/drive/*`, `POST /api/ratings/transfer`, and the unregister DELETEs on relay register routes.
- Tauri command `compute_owner_proof` produces the header in-process; wallet private keys never leave the desktop app.

**Relay registration (FM-A04/A05):**

- `register_share` / `register_site` POST bodies carry an ECDSA signature by `owner_wallet` over `(operation, id, owner_wallet, origin_url)`. Captured proofs can't be reused with a substituted origin URL.
- First-claim-wins is enforced: an existing record can only be overwritten by the wallet that originally signed it.
- Origin-URL validation rejects link-local (incl. AWS / GCP cloud metadata at `169.254.169.254`), multicast, broadcast, unspecified addresses, and anything outside `http(s)://`. Loopback stays accepted because `fix_origin_url` substitutes the registrant's public IP at request time. Private RFC1918, CGNAT, and unique-local IPv6 origins are accepted only when the relay operator explicitly includes the target IP/CIDR in `CHIRAL_RELAY_SHARE_PRIVATE_ORIGIN_ALLOWLIST`.

**Payment verification:**

- On-chain tx receipt checked before serving file chunks. Chain ID is verified so cross-chain replays of signed txs are rejected.
- Spent-tx ledger keys on `(tx_hash, file_hash)` so one payment ↔ one file delivery (no replay across different priced files seeded by the same wallet).
- Drive shares additionally bind each redeemed `tx_hash` to the first share token it unlocks, so a publicly-shared `?access=<tx>` URL can't unlock any of the wallet's other shares.
- `wait_for_tx_mined` is checked separately from `verify_tx_details` so the seeder can return a retryable "not yet confirmed" answer when chain propagation is slow.
- CDN payment uses exact `u128` ceil-rounded math — no `f64` truncation, no percentage tolerance.

**Operational hardening:**

- Local-daemon CORS allowlists only Tauri webview origins (blocks CSRF from arbitrary websites visited by the user); relay-mode keeps `Any`.
- `dht_put` headless route refuses raw writes to reserved-namespace keys (returns 403); each namespace has its own dedicated signed-publication command.
- Drive multipart upload caps body at 500 MiB before allocation; `is_item_under_shared_root` short-circuits parent cycles.
- ECDSA signatures enforce low-`s` (EIP-2), so signature hex is unique per (key, message).
- Relay filters private IPs from Kademlia routing table.
- Stop seeding removes peer from DHT seeder list (prevents ghost seeders).
- Platform fee on all transactions — default 0.5%, adjustable down to a 0.1% floor (remainder to the seller); `split_payment` is the single source of truth and `seller + fee == total` exactly, with exact integer arithmetic (no rounding tolerance).
- Wallet RPC reads use an ordered fallback list (`rpc_client::call_with_fallbacks`): direct canonical Geth → relay's `/api/chain/rpc` proxy. Either path can be down without taking the wallet UI offline.
- RPC failures surface as a yellow "canonical RPC unreachable" banner in the wallet UI rather than a misleading `0.00`. Mining page renders an inline divergence warning when local-Geth balance disagrees with canonical-RPC balance for the miner address (private-fork diagnostic).
- Embedded Geth binds its HTTP RPC to loopback only, exposes only `eth,net,web3,miner`, and does not enable wildcard browser CORS. Public read-only RPC access goes through the `/api/chain/rpc` proxy allowlist.

---

## Getting Started

### Prerequisites

- Node.js 20+
- Rust toolchain (rustup)
- npm

### Development

```bash
# Install frontend dependencies
npm install

# Start the desktop app in development mode
npm run tauri:dev

# Build the frontend only
npm run build

# Run frontend tests
npm test

# Run Rust tests
cargo test --manifest-path src-tauri/Cargo.toml

# Type check the Rust backend
cargo check --manifest-path src-tauri/Cargo.toml
```

### Headless Mode

Run the application without a GUI for server deployments or automated testing:

```bash
# Start the daemon
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral_daemon -- --port 9419

# Start with auto-mining
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral_daemon -- \
  --port 9419 \
  --auto-mine \
  --miner-address 0xYOUR_WALLET \
  --mining-threads 4

# Use the CLI
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- wallet create
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- dht start --port 9419
```

---

## Application Pages

| Route | Page | Description |
|-------|------|-------------|
| `/wallet` | Wallet | Create, import, or restore a wallet. Optional email backup of recovery phrase. |
| `/account` | Account | View wallet address, CHI balance, transaction history, and reputation score. Send CHI to other addresses. |
| `/network` | Network | Manage P2P connections. Start/stop the local Geth node. View peer list, bootstrap health, and DHT status. |
| `/download` | Download | Search for files by hash or magnet link. View seeder list with Elo scores. Pay and download files. |
| `/drive` | Drive | Local file manager with folders. Upload, rename, star, delete files. Seed files to the P2P network. |
| `/chiraldrop` | ChiralDrop | Direct peer-to-peer file transfers. Discover nearby peers and send/receive files. |
| `/hosts` | Hosts | Hosting marketplace. Publish storage offers, browse hosts, manage agreements. |
| `/mining` | Mining | CPU and GPU mining controls. View hash rate, block height, and total mined CHI. |
| `/settings` | Settings | Appearance (dark mode, color theme, nav style), notification preferences, download directory. |
| `/diagnostics` | Diagnostics | System event log, DHT health, bootstrap status, Geth status, mining diagnostics, Geth log viewer. |

---

## Backend Modules

The Rust backend is organized into the following modules under `src-tauri/src/`:

| Module | File | Responsibility |
|--------|------|---------------|
| Command Layer | `lib.rs` | Thin Tauri command wrappers, AppState management (103 commands) |
| Wallet | `wallet.rs` | Balance queries, transaction signing (EIP-155), history, metadata persistence, CHI/Wei conversion |
| RPC Client | `rpc_client.rs` | Connection-pooled HTTP client, batch JSON-RPC, response cache with TTL |
| DHT Service | `dht.rs` | libp2p Kademlia DHT, peer management, file publishing/searching, chunk transfer protocol |
| File Transfer | `file_transfer.rs` | Chunked file sending/receiving, SHA-256 verification, retry logic |
| Geth Process | `geth.rs` | Manages Core-Geth lifecycle, mining, batch RPC status queries |
| Drive API | `drive_api.rs` | HTTP routes for file CRUD, share links, preview pages |
| Drive Storage | `drive_storage.rs` | On-disk manifest and file storage management |
| Hosting Server | `hosting_server.rs` | Axum gateway server combining Drive, Rating, and Hosting routes |
| Hosting Types | `hosting.rs` | Site metadata, MIME detection, persistence |
| Rating API | `rating_api.rs` | Elo reputation calculation and HTTP endpoints |
| Rating Storage | `rating_storage.rs` | Persistent storage for reputation events |
| Relay Share Proxy | `relay_share_proxy.rs` | Reverse proxy + WebSocket tunnel for NAT traversal |
| Wallet Backup | `wallet_backup_api.rs` | SMTP email sending for wallet credential backup |
| Encryption | `encryption.rs` | X25519 key exchange and AES-GCM file encryption |
| Chain RPC | `chain_rpc_api.rs` | Blockchain RPC proxy |
| Speed Tiers | `speed_tiers.rs` | `split_payment` (fee split, single source of truth — default 0.5%, 0.1% floor) + download cost estimation (0.01 CHI/MB) |
| Event Sink | `event_sink.rs` | Frontend event emission abstraction |
| Geth Bootstrap | `geth_bootstrap.rs` | Bootstrap node health checking and selection |
| Version Policy | `version.rs` | `VersionPolicy` types, Ed25519 sign/verify, `is_acceptable_remote_policy`, global effective-policy slot |

Total: 24 Rust source files, 5 binary targets.

### Binary Targets

| Binary | Source | Purpose |
|--------|--------|---------|
| `chiral-network` | `src-tauri/src/main.rs` | Desktop application (Tauri) |
| `chiral` | `src-tauri/src/bin/chiral.rs` | Command-line interface |
| `chiral_daemon` | `src-tauri/src/bin/chiral_daemon.rs` | Headless daemon server |
| `relay_server` | `src-tauri/src/bin/relay_server.rs` | Relay and reputation server |
| `chiral-policy-sign` | `src-tauri/src/bin/chiral_policy_sign.rs` | Operator CLI: keygen / sign / verify a `VersionPolicy` with the project's offline Ed25519 key |

---

## Blockchain and Mining

Chiral Network runs a private Ethereum-compatible blockchain using the Ethash proof-of-work consensus algorithm.

### Chain Parameters

| Parameter | Value |
|-----------|-------|
| Chain ID | 98765 |
| Network ID | 98765 |
| Consensus | Ethash |
| Block reward | 5 CHI |
| Genesis difficulty | 0x400000 (4,194,304) |
| Gas limit | 0x47b760 (4,700,000) |
| Gas price | 0 (free transactions) |
| Client | Core-Geth v1.12.20 |

### Geth Configuration

| Setting | Value | Notes |
|---------|-------|-------|
| Sync mode | `full` | Replays all blocks from genesis; preserves full history on restart |
| GC mode | `archive` | Keeps all state; prevents block height regression on restart |
| Cache | 1024 MB | RAM cache for blockchain state |
| Max peers | 50 | Maximum Geth P2P connections |

### How Mining Works

1. The application auto-starts Geth with the wallet address as the coinbase (miner.etherbase).
2. On the Mining page, users can start CPU mining with a configurable number of threads.
3. Geth communicates via JSON-RPC on `localhost:8545`.
4. Mining status is polled every 10 seconds using batch RPC (eth_mining + eth_hashrate + eth_coinbase + eth_blockNumber in one request).
5. Balance and total mined both query the local Geth node via `eth_getBalance` through the shared `rpc_client.rs` connection pool.
6. All wallet queries route through `effective_rpc_endpoint()`: local Geth if running, otherwise remote fallback at `130.245.173.73:8545`.

### Bootstrap Node

A bootstrap node runs at `130.245.173.73` and serves as the initial peer for new nodes joining the network. It runs both Geth (port 8545 for RPC, port 30303 for P2P) and the relay server (port 8080 for HTTP, port 4001 for libp2p).

---

## Reputation System

The design rationale and the Elo update formula are in [Part I](#part-i-white-paper), Section 9. The system replaces an earlier 1-to-5-star user-rating model; historical rating data was reset to start the new system fresh.

### Parameters

| Parameter | Value |
|-----------|-------|
| Score range | 0 to 100 (clamped) |
| Base score for new wallets | 50 |
| Lookback window | 180 days |
| Time weight `w_time` | Linear decay from 1.0 (today) to 0.0 (180 days ago) |
| Amount weight `w_amount` | `1.0 + clamp(ln(1 + chi) / ln(51), 0, 1)` — 1.0 (free) to 2.0 (50+ CHI) |
| Outcome | 1.0 for completed, 0.0 for failed |
| Expected score | `1 / (1 + 10^((50 - elo) / 12))` |
| K factor | `4 * w_time * w_amount` |
| Update | `elo = clamp(elo + K * (outcome - expected), 0, 100)` |

### API Endpoints (Relay Server)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/ratings/:wallet` | GET | Get Elo score and event history for a wallet |
| `/api/ratings/batch` | POST | Batch lookup of Elo scores for multiple wallets |
| `/api/ratings/transfer` | POST | Record a transfer outcome (completed/failed) |

Wallet addresses are normalized to lowercase for consistent lookup.

### Integration

- The Download page displays seeder Elo scores next to each search result.
- After a file transfer completes or fails, the outcome is automatically reported to the relay.
- Paid-transfer events are verified against the on-chain tx (sender, recipient, amount) before being recorded — the backend does not trust frontend-submitted event data. Future hardening can add per-event cryptographic attestations.

---

## Dynamic Fee Index

> **Status: specified, not yet implemented.** Current builds use the static bootstrap anchor `f₀ = 0.01 CHI/MB` everywhere the index is consumed. This section is the normative spec for the replacement.

The reference download fee (design rationale: Part I, Section 8) replaces the fixed 0.01 CHI/MB constant with a network-derived rate:

```
f = clamp( M × (H₀ / H)^0.5 ,  f_min, f_max )
```

The index is **advisory**: it drives defaults and estimates but is never checked in payment verification, so nodes do not need bit-for-bit agreement on its value and no trusted fee oracle is introduced.

### Parameters

| Parameter | Value | Meaning |
|-----------|-------|---------|
| `f₀` | 0.01 CHI/MB | Bootstrap anchor: launch value and thin-market fallback (today's `COST_PER_MB_WEI`) |
| `f_min`, `f_max` | 0.001, 0.1 CHI/MB | Hard band: `f₀/10` to `f₀×10` |
| Market window | 14 days | Trailing window for the median `M` |
| `N_min` | 30 transfers | Fewer qualifying transfers in the window ⇒ `M := f₀` |
| Hashpower window | 4,096 blocks (~15 h at 13 s blocks) | `H = Σ difficulty / (t_last − t_first)` over the window |
| `H₀` | fixed at activation | Chosen so `f = f₀` under the hashpower observed at rollout (continuity) |
| Damping exponent | 0.5 | Square-root response to hashpower swings |
| Rate limit | ±10% per day | Maximum movement of the published index toward its target |

### Data sources

- **`H` (network hashpower)** — derived from block headers (`difficulty`, `timestamp`) over the trailing window via `eth_getBlockByNumber` through the shared `rpc_client.rs` (local Geth when running, canonical-RPC fallback otherwise). This is consensus data: every synced node computes the same value.
- **`M` (market median)** — fee-bearing transfers are enumerable on-chain: each pays the platform fee to `speed_tiers::PLATFORM_WALLET`, so a fee transaction of amount `x` marks a transfer totalling `x ÷ fee_rate` (`200·x` at the default 0.5%). Per-MB normalization joins the transfer to its file size from seller-signed metadata (`chiral_file_*` records / signed `FileInfo` envelopes). Practically, the relay can serve a precomputed 14-day median (the rating system already records on-chain-verified transfer events); clients may recompute or spot-check it, and small divergence is harmless because the index is advisory.

### Integration points (where the static constant lives today)

- `src-tauri/src/speed_tiers.rs` — `COST_PER_MB_WEI`, the backend estimate constant
- `calculate_download_cost` Tauri command (`lib.rs`) — buyer-facing cost estimate
- `src/lib/speedTiers.ts` — frontend mirror of the estimate
- `chiral download cost` CLI subcommand (`src-tauri/src/bin/chiral.rs`)
- CDN pricing floor — `floor_price` in `max(floor, median_peer_price × 1.2)` becomes index-derived
- Suggested default per-file ask when seeding from Drive

---

## Version Enforcement

Chiral Network ships a defence-in-depth scheme for keeping vulnerable client builds off the network. It is layered so a single bypass does not disable enforcement. (Design rationale: Part I, Section 12.)

### `VersionPolicy`

The on-the-wire policy (`src-tauri/src/version.rs`) carries:

| Field | Meaning |
|-------|---------|
| `minRequired` | Versions strictly below this are blocked. |
| `recommended` | Versions below this trigger a soft "update available" nudge. |
| `downloadUrl` | Where the UI sends users to upgrade. |
| `message` | Optional human-readable reason (e.g. "fixes payment bug"). |
| `issuedAt` | Unix-seconds the policy was issued (used for rollback protection). |
| `validUntil` | Unix-seconds after which clients should re-fetch. `0` = no expiry. |
| `signature` | Hex Ed25519 signature over a length-prefixed canonical payload. |

Comparing the running build's `CARGO_PKG_VERSION` against the effective policy returns one of three states:

- `ok` — version ≥ `recommended`, no UI.
- `recommended` — `recommended > version ≥ minRequired`, soft banner the user can dismiss for the session.
- `required` — `version < minRequired`, full-screen blocking modal (`UpdateGate.svelte`).

### Enforcement layers

1. **UI gate (`UpdateGate.svelte`)** — driven by `versionStore` over the Tauri `get_version_status` command; renders the soft banner / hard modal.
2. **Tauri command gate** — `ensure_version_supported` is called from `start_dht_internal` and `start_download` so a stale build cannot join the DHT or initiate a download.
3. **HTTP middleware** — every `/api/*` route on the gateway server (relay, daemon, desktop hosting) reads `X-Chiral-Client-Version` and returns `426 Upgrade Required` (with the policy JSON in the body) when the client is below `minRequired`. Health and `/api/version-policy` are exempted.
4. **libp2p Identify** — `agent_version` is set to `chiral/<version>` and the Identify handler disconnects peers whose advertised version is below `minRequired`, blacklisting them so they aren't re-dialled.

### Distribution

Every binary embeds a `bundled_policy()` snapshot at compile time. On startup, the desktop app probes `http://130.245.173.73:8080/api/version-policy` (the relay's gateway) and promotes the result via `update_effective_policy()` if it passes acceptance. The same `/api/version-policy` route is mounted by the relay server, the headless daemon, and the desktop's hosting server, so any peer can read the network's current view of the policy.

A global `EFFECTIVE_POLICY` slot (`OnceCell<RwLock<VersionPolicy>>`) holds the live policy. It's a sync `parking_lot::RwLock` because the libp2p event loop reads it from non-async contexts.

### Acceptance rules (`is_acceptable_remote_policy`)

A fetched policy replaces the current effective policy only if:

1. **Rollback protection** — `remote.issuedAt` is not older than `current.issuedAt`.
2. **Signature** — if `remote.signature` is non-empty, it must verify against `POLICY_PUBLIC_KEY`.
3. **Unsigned transitional path** — if `current.signature` is empty *and* `remote.signature` is empty *and* `remote.minRequired` does not raise the floor above the binary's bundled `minRequired`, the policy is accepted. Once a signed policy has been adopted, unsigned remotes are no longer accepted.

The transitional path lets relays advertise the recommended-version nudge before the project's offline signing key is wired in, while still preventing a hostile relay from raising `minRequired` to lock honest peers out.

### Operator CLI: `chiral-policy-sign`

The `chiral-policy-sign` binary signs and verifies policies with the project's offline Ed25519 key:

```bash
# Generate a new project keypair (paste the public hex into POLICY_PUBLIC_KEY).
chiral-policy-sign keygen

# Sign a policy JSON.
chiral-policy-sign sign --key <secret-hex> --in policy.json --out policy.signed.json

# Verify (defaults to the binary's compiled-in public key; --pub overrides).
chiral-policy-sign verify --in policy.signed.json
```

The compile-time `POLICY_PUBLIC_KEY` constant is a 32-byte zero placeholder. Operators activate signed policies at deploy time without recompiling by setting the `CHIRAL_POLICY_PUBLIC_KEY` environment variable to the 32-byte public key (hex, with or without `0x` prefix); `version::policy_public_key()` resolves the env var on first access and caches it. All three binaries (desktop, daemon, relay) print a `[VERSION]` line on startup confirming whether signed policies are enabled or warning that the placeholder is still in use. Until a real key is wired in, only the unsigned-transitional path can promote a remote policy.

---

## File Transfer Protocol

Files are transferred using a custom request-response protocol built on libp2p.

### Protocol Details

| Property | Value |
|----------|-------|
| Protocol ID | `/chiral/file-request/2.0.0` |
| Chunk size | 256 KB |
| Encoding | CBOR (custom codec) |
| Request limit | 1 MB |
| Response limit | 32 MB |
| Verification | SHA-256 per chunk + full file hash |
| Retry | Up to 3 attempts per chunk |

### Message Types

**Request:** `ChunkRequest` enum with `FileInfo` (metadata request) and `Chunk` (data request with offset) variants.

**Response:** `ChunkResponse` enum with `FileInfo` (file metadata: name, size, hash, chunk count) and `Chunk` (data bytes + SHA-256 hash) variants.

---

## Headless Mode and CLI

### Daemon

The headless daemon runs the full backend without a GUI. It exposes an HTTP API on the configured port (default 9419).

```bash
chiral_daemon --port 9419 --auto-start-dht --auto-mine --miner-address 0xABC
```

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--port` | `CHIRAL_DAEMON_PORT` | 9419 | HTTP API port |
| `--auto-start-dht` | `CHIRAL_AUTO_START_DHT` | false | Start DHT on boot |
| `--auto-start-geth` | `CHIRAL_AUTO_START_GETH` | false | Start Geth on boot |
| `--auto-mine` | `CHIRAL_AUTO_MINE` | false | Start mining (implies DHT + Geth) |
| `--miner-address` | `CHIRAL_MINER_ADDRESS` | none | Wallet for mining rewards |
| `--mining-threads` | `CHIRAL_MINING_THREADS` | 1 | CPU mining threads |

### Daemon API Endpoints

All headless paths are prefixed with `/api/headless/` except health, ready, drive, and the publicly-mounted `/api/version-policy`.

| Category | Endpoints |
|----------|-----------|
| Health | `GET /api/health`, `GET /api/ready`, `GET runtime` |
| Version policy | `GET /api/version-policy` — returns the currently-effective `VersionPolicy` (mounted on the gateway router; available on relay, daemon, and desktop hosting server alike) |
| Wallet | `GET wallet`, `POST wallet/create`, `wallet/import`, `wallet/balance`, `wallet/send`, `wallet/receipt`, `wallet/history`, `wallet/faucet`; `GET wallet/chain-id` |
| DHT | `POST dht/start`, `dht/stop`, `dht/put`, `dht/get`, `dht/ping`, `dht/echo`; `GET dht/health`, `dht/peers`, `dht/peer-id`, `dht/listening-addresses` |
| Files | `POST file/search`, `dht/register-shared-file`, `dht/unregister-shared-file`, `dht/request-file`, `dht/send-file` |
| ChiralDrop | `GET drop/inbox`, `drop/outgoing`; `POST drop/accept`, `drop/decline` |
| Geth | `POST geth/install`, `geth/start`, `geth/stop`; `GET geth/status`, `geth/logs` |
| Mining | `POST mining/start`, `mining/stop`, `mining/miner-address`; `GET mining/status`, `mining/blocks` |
| Hosting | `POST hosting/publish-ad`; `GET hosting/registry` |
| Folder bundles | Tauri-only: `publish_drive_folder`, `unpublish_drive_folder`, `search_folder` (one content-addressed hash per folder) |
| CDN | `POST cdn/upload`; `GET cdn/files`, `cdn/pricing`, `cdn/status`; `DELETE cdn/files/:hash`; `PUT cdn/files/:hash` |
| Drive | Full CRUD via `/api/drive/*` (requires both `X-Owner` and `X-Owner-Sig: <unix_ts>:<hex_signature>` headers; see [Security Implementation](#security-implementation)) |
| Diagnostics | `GET bootstrap-health` |

### CLI

The CLI tool communicates with a running daemon over HTTP.

```bash
chiral daemon status --port 9419
chiral wallet create
chiral wallet show
chiral account balance
chiral account send --to 0xADDRESS --amount 1.5
chiral dht start --port 9419
chiral dht peers --port 9419
chiral download search --hash FILEHASH --port 9419
chiral drive ls
chiral mining start --threads 4 --port 9419
chiral mining status --port 9419
```

---

## Docker and Scaled Testing

### Docker Images

The project includes a multi-stage Dockerfile that produces four image targets:

| Target | Binary | Ports | Purpose |
|--------|--------|-------|---------|
| `daemon` | `chiral_daemon` | 9419, 30303 | Headless P2P node |
| `relay` | `relay_server` | 4001, 8080 | Bootstrap relay server |
| `cli` | `chiral` | -- | Command-line tool |
| `test-node` | `chiral_daemon` + `chiral` | 9419, 30303 | Testing with healthcheck |

### Docker Compose Files

| File | Purpose |
|------|---------|
| `docker-compose.yml` | General test network (relay + scalable nodes) |
| `docker-compose.local-test.yml` | Local isolated testing with relay |
| `docker-compose.production-net.yml` | 30 nodes on host networking, connected to production relay |
| `docker-compose.scaled-test.yml` | Scaled integration test overlay |

```bash
# 30 production-connected nodes (host networking)
docker compose -f docker-compose.production-net.yml up -d

# Local isolated testing
docker compose -f docker-compose.local-test.yml up -d --scale node=10

# Tear down
docker compose -f docker-compose.production-net.yml down
```

### Kubernetes Deployment (k3s/Rancher)

Test nodes can also be deployed to the k3s cluster at `130.245.173.231`:

```bash
export KUBECONFIG=~/.kube/config-k3s
kubectl apply -f k8s/chiral-30-pods.yaml
kubectl get pods -n chiral-test
```

### Stress Testing

A 12-phase, 35-test stress suite exercises every feature across 30 nodes:

```bash
bash scripts/stress-test-30-nodes.sh
```

#### Stress Test Phases (stress-test-30-nodes.sh)

| Phase | Name | What It Tests |
|-------|------|--------------|
| 1 | Health & Connectivity | All 30 health/readiness endpoints |
| 2 | DHT Network | Unique peer IDs, peer counts, relay circuits, cross-node ping |
| 3 | DHT Storage | Cross-node put/get, 10 concurrent writes |
| 4 | Wallet | Create (10 nodes), import, balance query, chain ID |
| 5 | File Registration | Publish file, search from publisher + 5 remote nodes |
| 6 | Echo Protocol | Direct echo + fan-out to 10 nodes |
| 7 | Hosting Ads | Publish advertisement, query registry from remote node |
| 8 | Concurrent Stress | 30 simultaneous DHT puts, peer queries, health checks |
| 9 | Ping Mesh | 10 random node pairs |
| 10 | Drive Operations | List items, create folder |
| 11 | Bootstrap Health | Diagnostics report |
| 12 | DHT Reconnect | Stop DHT, restart, verify peer recovery |

---

## Testing

### Unit and Integration Tests (vitest)

```bash
npm test                    # Run all frontend tests
npm test -- tests/load/     # Run load tests only
```

The test suite contains 585+ tests across 35 files:

| Category | Files | Tests | Coverage |
|----------|-------|-------|----------|
| Store/service unit tests | 24 | 497 | Stores, services, utilities, wallet, DHT, Drive |
| Load/stress tests | 9 | 89 | Concurrent operations, throughput, caching |
| Network tests (skipped in CI) | 2 | 43 | Relay server, gateway endpoints |

### Rust Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

270+ Rust tests across 14 modules covering: wallet CHI/Wei conversion, genesis validation, syncing logic, mining status, serialization, GPU error detection, Kademlia peer filtering, encryption, hosting server, rating storage, relay share proxy, drive API, owner-proof signing (path/method/wallet binding + replay protection), folder-bundle hashing (order independence, owner case-folding, file-set sensitivity), folder-manifest v1/v2 signature roundtrip, and CDN payment math (`required_upload_wei` ceil rounding + saturation).

### Scaled Integration Tests

12-phase stress test running against 30 Docker/k8s containers. See the [Docker and Scaled Testing](#docker-and-scaled-testing) section.

---

## Project Structure

```
chiral-network/
  src/                          # Frontend (Svelte 5 + TypeScript)
    App.svelte                  # Main app shell, routing, DHT auto-start on login
    pages/                      # 11 page components
      Account.svelte            # Wallet balance, transactions, reputation
      Download.svelte           # File search, download with CHI payments
      Drive.svelte              # Local file manager, seeding, sharing
      Mining.svelte             # CPU/GPU mining controls
      Network.svelte            # Peer list, DHT health, bootstrap status
      Hosts.svelte              # Hosting marketplace, agreements
      ChiralDrop.svelte         # Direct P2P file transfers
      Wallet.svelte             # Wallet creation, import, backup
      Settings.svelte           # Appearance, notifications, download dir
      Diagnostics.svelte        # Event log, system info
    lib/
      stores.ts                 # Svelte stores (wallet, settings, peers)
      dhtService.ts             # Frontend DHT service (event listeners before start)
      services/                 # 8 service modules
        walletService.ts        # Balance caching (10s TTL), chain ID
        gethService.ts          # Geth/mining status polling (10s interval)
        hostingService.ts       # Host discovery, agreements, echo retry
        driveApiService.ts      # Drive CRUD operations
        ratingApiService.ts     # Reputation batch lookups
        encryptionService.ts    # File encryption helpers
        walletBackupService.ts  # Email backup
        colorThemeService.ts    # Theme management
      components/               # Reusable UI components
      chiralDropStore.ts        # Wallet-specific ChiralDrop history
      toastStore.ts             # Toast notification system
      logout.ts                 # Logout with 5s DHT timeout + loading state

  src-tauri/                    # Backend (Rust, 23 source files)
    src/
      lib.rs                    # Thin Tauri command wrappers (103 commands)
      wallet.rs                 # All wallet logic (balance, tx, history, signing)
      rpc_client.rs             # Connection-pooled HTTP, batch RPC, cache
      dht.rs                    # libp2p Kademlia DHT, peer discovery, file transfer
      geth.rs                   # Geth lifecycle, mining, batch status queries
      geth_bootstrap.rs         # Bootstrap node health checking
      file_transfer.rs          # Chunked protocol (256KB, SHA-256)
      drive_api.rs              # Drive HTTP routes, preview pages
      drive_storage.rs          # Drive manifest and file storage
      hosting.rs                # Hosting types, MIME detection, persistence
      hosting_server.rs         # Axum gateway server
      relay_share_proxy.rs      # Reverse proxy + WebSocket tunnel
      rating_api.rs             # Reputation HTTP endpoints
      rating_storage.rs         # Elo computation
      encryption.rs             # AES-GCM + X25519 encryption
      wallet_backup_api.rs      # Email backup endpoint
      chain_rpc_api.rs          # Blockchain RPC proxy
      speed_tiers.rs            # Payment split (fee cut, default 0.5%) + cost estimate
      event_sink.rs             # Frontend event emission
      bin/
        chiral.rs               # CLI client
        chiral_daemon.rs        # Headless daemon (44 API routes)
        relay_server.rs         # Production relay server

  tests/                        # Frontend tests (vitest, 585+ tests)
  scripts/
    stress-test-30-nodes.sh     # 12-phase, 35-test stress suite
    local-test-cluster.sh       # Local process-based test cluster
    full-feature-test.sh        # Feature validation suite
    extended-feature-test.sh    # Extended feature tests
    scaled-test.sh              # Scaled test orchestrator
    docker-test.sh              # Basic Docker test

  Dockerfile                    # Multi-stage build (daemon, relay, cli, test-node)
  Dockerfile.local              # Pre-built binary image
  docker-compose.yml            # General test network
  docker-compose.local-test.yml # Local isolated testing
  docker-compose.production-net.yml # 30 nodes, host networking, production relay
```

---

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CHIRAL_RPC_ENDPOINT` | `http://130.245.173.73:8545` | Remote blockchain RPC fallback |
| `CHIRAL_GETH_SYNCMODE` | `full` | Geth sync mode (`full` or `snap`) |
| `CHIRAL_BOOTSTRAP_NODES` | Built-in bootstrap list | Comma-separated enode URLs |
| `CHIRAL_DAEMON_PORT` | `9419` | Daemon HTTP port |
| `CHIRAL_AUTO_START_DHT` | `false` | Auto-start DHT on daemon boot |
| `CHIRAL_AUTO_START_GETH` | `false` | Auto-start Geth on daemon boot |
| `CHIRAL_AUTO_MINE` | `false` | Auto-start mining (implies DHT + Geth) |
| `CHIRAL_MINER_ADDRESS` | none | Wallet address for mining rewards |
| `CHIRAL_MINING_THREADS` | `1` | CPU mining thread count |
| `CHIRAL_GPU_MINER_PATH` | auto-detected | Path to ethminer binary |
| `CHIRAL_WALLET_EMAIL_SMTP_HOST` | none | SMTP server for email backup |
| `CHIRAL_WALLET_EMAIL_FROM` | none | Sender address for email backup |
| `CHIRAL_POLICY_PUBLIC_KEY` | placeholder zeros | 32-byte hex (with or without `0x` prefix) of the project's Ed25519 policy-signing public key. Setting this activates signed `VersionPolicy` updates without recompiling. Generate the matching keypair with `chiral-policy-sign keygen`. |
| `CHIRAL_WALLET_KEY_FILE` | none | Path to a file containing a single hex secp256k1 private key (with or without `0x` prefix; mode 0600 expected). At startup the daemon loads the key, derives the address, and populates `state.wallet` so the CDN module can sign `chiral_seeder_*` / `chiral_file_*` records and `ChunkResponse::FileInfo` envelopes. Without it, the CDN runs with empty signatures and clients reject every record it publishes. Used in production at `/etc/chiral-cdn-wallet.key` on the canonical relay. |
| `CHIRAL_RELAY_SHARE_PRIVATE_ORIGIN_ALLOWLIST` | none | Comma-separated IP/CIDR allowlist for private relay-share origins, e.g. `10.0.0.0/8,100.64.0.0/10,fd00::/8`. Applies only to private RFC1918, CGNAT, and unique-local IPv6 origin literals; link-local, cloud metadata, unspecified, multicast, and broadcast targets remain blocked. |

### Local Storage Keys

User data is stored in localStorage with wallet-specific keys to prevent data leakage between accounts:

- `chiraldrop_history_<address>` -- ChiralDrop transfer history
- `chiraldrop_history_encrypted_<address>` -- Encrypted history cache
- `chiral_download_history_<address>` -- Download history
- `chiral_active_downloads_<address>` -- Active downloads
- `chiral_saved_recipients_<address>` -- Saved recipient addresses

### Data Directories

| Platform | Path |
|----------|------|
| Linux | `~/.local/share/chiral-network/` |
| macOS | `~/Library/Application Support/chiral-network/` |
| Windows | `%APPDATA%/chiral-network/` |

Subdirectories:
- `chiral-drive/` -- Drive file storage
- `geth/` -- Blockchain data and logs (archive mode)
- `agreements/` -- Hosting agreement JSON files
- `sites/` -- Hosted site files
- `headless/` -- Daemon PID file
- `tx_metadata.json` -- Persisted transaction metadata
- `hosted_sites.json` -- Hosted site registry
