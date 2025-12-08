# Reputation System

> **Status:** MVP design for transaction-backed reputation with signed transaction message proofs.

> **📘 For detailed information about signed transaction messages and non-payment complaint workflow, see [SIGNED_TRANSACTION_MESSAGES.md](./SIGNED_TRANSACTION_MESSAGES.md)**

## Overview

Chiral Network tracks peer reputation through verifiable, transaction-centric evidence. Confirmed on-chain transactions are the authoritative record of successful payments. When a download completes and payment is broadcast and confirmed, that on-chain transfer is ground truth for successful behavior. For non-payment, there may be no on-chain footprint at all. In those cases, we rely on cryptographically signed off-chain payment promises from the downloader to the seeder as evidence, and store those promises (or hashes of them) in the DHT. To keep costs low and latency acceptable, clients publish signed **Transaction Verdicts** into the DHT as an index of recent interactions. Consumers fetch those verdicts for fast heuristics, but they always re-validate against the chain (or cached receipts) before acting; if a verdict cannot be bridged back to finalized chain history, it is ignored. This hybrid model lets us iterate inside today's infrastructure while reserving long-term accuracy to the blockchain. Later releases may reuse the same storage model to incorporate additional metrics (uptime, relay quality, etc.) once the supporting evidence flow exists.

### Core Principles

1. **Blockchain as Source of Truth for Success**: Positive reputation stems from completed on-chain payments.
2. **Signed Promises for Failures**: Negative reputation for non-payment uses cryptographically signed off-chain payment messages when no on-chain transaction exists.
3. **DHT as Performance Cache**: Quick lookups without querying the full blockchain every time
4. **Transaction-Centric**: Reputation grows with successful transaction history (seeding or downloading)
5. **Proof-Backed Penalties**: Complaints require cryptographic evidence (signed handshakes, transaction data)
6. **Hybrid Verification**: Recent activity via DHT, historical data via blockchain

### Goals

- Provide a verifiable reputation signal without changing the on-chain protocol.
- Keep the system PoW-friendly: identities correspond to existing mining/transaction keys, and no dedicated storage nodes are required.
- Allow future metrics to plug into the same DHT namespace without breaking compatibility.
- Build reputation on immutable, verifiable on-chain transaction history
- Use DHT for performance without relying on it for persistence
- Support both reliable (on-chain) and unreliable (DHT gossip) penalties

## Trust Levels

Peers are bucketed by their **transaction score**, a weighted average of verdicts retrieved from the DHT. Default weights: `good = 1.0`, `disputed = 0.5`, `bad = 0.0`. Additional decay or weighting can be applied client-side. Reputation grows with the number of confirmed transactions a peer completes; clients derive those totals directly from chain-validated verdicts and may require a minimum number of successful settlements before promoting a peer into higher trust brackets.

| Trust Level | Score Range | Description |
|-------------|-------------|-------------|
| **Trusted** | 0.8 - 1.0 | Highly reliable, consistently good performance |
| **High** | 0.6 - 0.8 | Very reliable, above-average performance |
| **Medium** | 0.4 - 0.6 | Moderately reliable, acceptable performance |
| **Low** | 0.2 - 0.4 | Less reliable, below-average performance |
| **Unknown** | 0.0 - 0.2 | New or unproven peers |

## Reputation Architecture

### Two-Tier System

#### 1. On-Chain Layer (Authoritative)

The blockchain records all completed transactions. Each transaction inherently provides reputation data:

- **Successful completion** = positive reputation signal
- **Transaction count** = measure of experience and reliability
- **Role diversity** = reputation as both seeder and downloader
- **Complaint records** = negative signals with cryptographic proof

**On-chain data includes:**
- Transaction hash and block number
- Parties involved (seeder and downloader)
- File hash or content identifier
- Payment amount
- Timestamp
- Optional: Complaint flag with evidence pointer

#### 2. DHT Layer (Volatile Cache)

The DHT stores recent reputation updates for quick access:

- **Recent transaction summaries** (last 100 per peer)
- **Pending complaints** with attached cryptographic evidence
- **Score cache** to avoid repeated blockchain queries
- **Gossip signals** about suspicious behavior

**DHT cache characteristics:**
- Data expires and gets pruned regularly
- No guarantee of persistence
- Fast lookups without full blockchain scan
- Useful for real-time peer selection
- Must be verified against on-chain data when accuracy matters

### Transaction & Payment Lifecycle

The transaction flow is designed to minimize blockchain interaction during the file transfer while still providing cryptographic proof for dispute resolution. The key innovation is the use of **signed transaction messages** that serve as off-chain payment promises.

#### Visual Overview

```
┌─────────────┐                                    ┌─────────────┐
│ Downloader  │                                    │   Seeder    │
│     (A)     │                                    │     (B)     │
└──────┬──────┘                                    └──────┬──────┘
       │                                                  │
       │  1. Check B's reputation & find via DHT         │
       │─────────────────────────────────────────────────>
       │                                                  │
       │  2. Send SIGNED TRANSACTION MESSAGE (off-chain)  │
       │     {from: A, to: B, amount: X, sig: ...}       │
       │─────────────────────────────────────────────────>
       │                                                  │
       │   3. B validates signature, then checks A's      │
       │      reputation & on-chain balance (in that order)
       │                                                  │
       │  4. File chunks                                  │
       │<═════════════════════════════════════════════════│
       │                                                  │
       │  5. Submit payment to BLOCKCHAIN                 │
       │──────────────┐                                   │
       │              │                                   │
       │              ▼                                   │
       │     ┌──────────────┐                            │
       │     │  Blockchain  │                            │
       │     │ (tx recorded)│                            │
       │     └──────┬───────┘                            │
       │            │                                     │
       │  6. Payment confirmed after N blocks             │
       │<───────────┘                                     │
       │                                                  │
       │  7. Both publish 'good' verdicts to DHT          │
       │<─────────────────────────────────────────────────>
       │              (reputation increases)              │
       └──────────────────────────────────────────────────┘
```

NON-PAYMENT CASE:
If A doesn't submit payment (step 5), B still has the signed message from step 2 as cryptographic proof to file a complaint in DHT.

#### Standard Transaction Flow (Successful Case)

The handshake ordering is intentionally strict: the downloader must produce the signed payment message first; the seeder validates the signature, then performs reputation and balance checks. This prevents the seeder from making on-chain/balance checks before cryptographic commitment exists, reduces race conditions, and ensures evidence availability in case of non-payment.

1. Discovery & Reputation Check
   ├─ Downloader (A) finds Seeder (B) via DHT
   └─ A queries B's reputation score and transaction history

2. Handshake with Signed Payment Promise
   ├─ A creates signed transaction message:
   │  - From: A's address
   │  - To: B's address
   │  - Amount: File price
   │  - File hash: Target file identifier
   │  - Nonce: Prevents replay attacks
   │  - Deadline: Maximum time for transfer completion
   │  - Signature: Cryptographic proof from A
   ├─ A sends signed message to B (OFF-CHAIN, P2P only)

3. Seeder Validation
   ├─ B validates the signed message signature (authenticity)
   ├─ B then checks A's reputation score and on-chain balance
   └─ If signature, reputation, and balance pass, B proceeds to file transfer

4. File Transfer (Pure P2P, No Blockchain)
   ├─ B begins sending file chunks
   ├─ A receives and verifies chunks
   └─ Transfer completes when all chunks validated

5. Payment Settlement (On-Chain)
   ├─ A submits the signed transaction to blockchain
   ├─ Transaction propagates through network
   ├─ After confirmation_threshold blocks (default: 12)
   └─ Payment is finalized on-chain

6. Reputation Update (Both Parties)
   ├─ A publishes 'good' verdict for B (successful seeder)
   ├─ B publishes 'good' verdict for A (successful payer)
   └─ Both verdicts reference the same tx_hash

#### Non-Payment Scenario (Downloader is Malicious)

**Problem:** Downloader receives file but never submits payment to blockchain.

**Solution:** Seeder retains the signed transaction message as cryptographic proof of payment obligation, and can publish a complaint with evidence.

1-3. [Same as above: Discovery, Handshake, Seeder Validation]

4. Payment Deadline Expires
   ├─ Deadline passes without on-chain transaction
   ├─ B waits for confirmation_threshold + grace period
   └─ Still no transaction appears on blockchain

5. Seeder Files Complaint
   ├─ B publishes 'bad' verdict to DHT with evidence:
   │  - signed_transaction_message: A's signed payment promise
   │  - delivery_proof: Chunk manifest and completion logs
   │  - tx_hash: NULL (no blockchain record)
   └─ B can optionally submit on-chain complaint (costs gas)

6. Reputation Impact
   ├─ A's reputation immediately drops (gossip penalty)
   ├─ Other seeders see the signed message proof in DHT
   ├─ A becomes blacklisted by reputation-aware peers
   └─ A cannot dispute without proving blockchain payment

**Why This Works:**
- The signed transaction message is cryptographically unforgeable
- Other peers can verify A's signature independently
- A cannot claim "I never agreed to pay" because signature proves intent
- Even without blockchain record, the signed message serves as strong evidence
- Multiple complaints with signed messages from different seeders compound the penalty

#### Malicious Seeder Scenario (Seeder Doesn't Deliver)

**Problem:** Seeder accepts handshake but doesn't send file (or sends corrupted data).

**Design Decision:** The downloader becomes the victim in this case.

**Rationale:**
- Harder to prove "non-delivery" cryptographically than "non-payment"
- Downloader can abort transfer and find another seeder (low cost)
- Seeder loses potential payment (self-punishing behavior)
- Reputation system prioritizes protecting seeders (they provide value)
- Can be mitigated through seeder reputation checks before handshake

1-2. [Same: Discovery, Handshake with signed payment]

3. File Transfer Fails
   ├─ B never sends chunks, OR
   ├─ B sends corrupted/incomplete chunks
   └─ A detects failure and aborts

4. Downloader Response
   ├─ A does NOT submit payment to blockchain
   ├─ A can file 'disputed' verdict in DHT (optional)
   ├─ A looks for different seeder with better reputation
   └─ No financial loss (payment never sent)

5. Seeder Consequence
   ├─ B loses potential payment
   ├─ If A files complaint, B's reputation may drop
   ├─ Repeated failures will lower B's trust level
   └─ Eventually B becomes untrusted and ignored by downloaders

**Why Downloader is Acceptable Victim:**
- No financial loss (payment conditional on delivery)
- Can retry with different seeder immediately
- Seeder loses more (reputation + payment) for malicious behavior
- Initial reputation check filters out most malicious seeders

#### Uptime & Reliability Bonus

```
Seeder Reputation Factors:
├─ Transaction success rate (primary)
├─ Uptime duration (secondary bonus)
│  - Longer continuous uptime = higher trust
│  - Consistently online seeders less likely to be malicious
│  - Measured by peer observations and relay connectivity
└─ Total data served (volume bonus)
```

**The longer a seeder is online and serving files successfully, the better their reputation.**

This creates an incentive for long-term, reliable seeders and makes it costly for attackers to build trusted identities.

### Reputation Calculation Flow

1. Query DHT for recent activity (last N transactions)
   ├─ If cache hit → Use cached score with timestamp
   └─ If cache miss or stale → Continue to step 2

2. Query blockchain for full transaction history
   ├─ Count successful transactions (seeding + downloading)
   ├─ Identify complaint records with proofs
   └─ Calculate base score from transaction count

3. Apply penalty adjustments
   ├─ Reliable penalties: On-chain complaint with proof
   └─ Unreliable penalties: DHT gossip (lower weight)

4. Cache result in DHT for future lookups
   └─ Store with TTL (default: 10 minutes)

5. Return final reputation score

## Reputation Metrics

### Transaction Verdict Record

All transaction reputation is derived from the `TransactionVerdict` payload. Each verdict is signed by the issuer (one of the transaction parties) and stored in the DHT using the key `H(target_id || "tx-rep")`. On-chain data remains the source of truth—verifiers can always recompute reputation by replaying confirmed transactions even if DHT entries expire.

| Field | Description |
|-------|-------------|
| `target_id` | Peer ID whose reputation is updated. |
| `tx_hash` | Canonical chain reference (block + tx index or transaction hash). Can be NULL for non-payment complaints where payment never reached blockchain. |
| `outcome` | `good`, `bad`, or `disputed`. |
| `details` | Optional plain-text metadata (kept ≤ 1 KB). |
| `metric` | Optional label; defaults to `transaction`. Reserved for future metrics. |
| `issued_at` | Unix timestamp in seconds when the verdict was produced. |
| `issuer_id` | Peer ID of the issuer. |
| `issuer_seq_no` | Monotonic counter per issuer to block duplicate verdicts. |
| `issuer_sig` | Signature over all previous fields using the issuer's transaction key. |
| `tx_receipt` | Optional pointer or embedded proof (e.g., payment-channel close receipt) that links the verdict to an on-chain transaction outcome. |
| `evidence_blobs` | Optional array of detached, signed payloads. **Critical for non-payment complaints:** includes `signed_transaction_message` (the downloader's signed payment promise), `delivery_proof` (chunk manifest), and protocol logs that prove file delivery without payment. |

Validation rules:
- Reject any verdict where `issuer_id == target_id`.
- Issuer may publish exactly one verdict per `(target_id, tx_hash)`.
- DHT peers keep verdicts **pending** until `tx_hash` is at least `confirmation_threshold` blocks deep (configurable, default `12`).

**Transaction types that count:**
- Successful file downloads (as downloader, payment confirmed)
- Successful file uploads (as seeder, payment received)
- Both roles contribute equally to reputation

**What doesn't count:**
- Incomplete transactions
- Disputed transactions (until resolved)
- Transactions from blacklisted peers
- Self-transactions (same peer both sides)

### Reliable Penalty Complaints

Reliable penalties apply when a party can anchor their claim to the chain. For example, a seeder can submit a `bad` verdict with a `tx_receipt` showing the downloader never closed the payment channel and funds were reclaimed via timeout. Clients:

1. Verify the `tx_receipt` or referenced settlement on-chain after the required confirmation depth.
2. Ensure the `issuer_seq_no` monotonically increases to prevent replay.
3. Apply the penalty weight immediately once corroborated, since the underlying evidence is immutable. Implementations that track derived success totals reverse any previously credited success for the same transfer so that reliable failures immediately reduce credit toward higher trust levels.

Because these complaints rest on permanent chain data, they are treated as authoritative and can trigger automatic responses (e.g., lower trust buckets, blacklist thresholds) without waiting for additional reports.

### Payment Handshake (Downloader → Seeder)

Before any data transfer, the downloader MUST send a signed payment message to the seeder:
payer_id = downloader’s peer/wallet ID
payee_id = seeder’s ID
amount = maximum amount the downloader is willing to pay
expiry = deadline after which the promise is invalid
chain_tx_template = transaction payload that can be broadcast to the chain as-is (or with minimal wrapping)
payer_sig = downloader’s signature over the above

The seeder only starts sending data if:
1. The signature is valid, and
2. The seeder verifies the downloader’s balance and reputation (see admission control below).

If the downloader later refuses to pay or never broadcasts payment, the seeder retains this signed message as evidence and can:
Optionally broadcast it (if the chain model allows a third party to submit signed transactions from the downloader).
At minimum, publish a bad TransactionVerdict to the DHT with the signed promise attached in evidence_blobs.

### Non-payment Complaint Lifecycle

This is the most critical reputation scenario: a downloader receives a file but never pays. The solution leverages **signed transaction messages** as cryptographic proof of payment obligation.

#### Detailed Flow

1. **Handshake Phase**
   - Downloader creates a `SignedTransactionMessage`:
     ```typescript
     {
       from: downloader_address,
       to: seeder_address,
       amount: file_price,
       file_hash: target_file_hash,
       nonce: unique_identifier,
       deadline: unix_timestamp,        // e.g., 1 hour from now
       downloader_signature: sig        // Secp256k1 signature
     }
     ```
   - Downloader sends this message to seeder **OFF-CHAIN** via P2P connection
   - Seeder validates:
     - Signature authenticity (proves message from downloader's private key)
     - Then checks downloader's on-chain balance (has sufficient funds)
     - Deadline is reasonable (not already expired)
   - Seeder stores the signed message as `evidence_blob`

2. **File Transfer Phase**
   - Seeder delivers file chunks
   - Seeder logs delivery proof:
     - Chunk manifest (all chunks sent)
     - Transfer completion timestamp
     - Connection logs
   - Downloader receives and validates chunks

3. **Expected Settlement (Honest Case)**
   - Downloader submits the signed transaction to blockchain
   - Transaction propagates and gets mined
   - After `confirmation_threshold` blocks (default: 12), payment is final
   - Seeder monitors blockchain and detects payment
   - Seeder publishes `good` verdict with `tx_hash` referencing the blockchain transaction

4. **Non-payment Scenario (Malicious Downloader)**
   - Transfer completes but downloader never submits payment
   - Seeder waits until `deadline + confirmation_threshold blocks + grace_period`
   - Still no transaction appears on blockchain
   - **Seeder now has cryptographic proof of non-payment**

5. **Filing the Complaint**
   - Seeder publishes `bad` verdict to DHT:
     ```typescript
     {
       target_id: downloader_peer_id,
       tx_hash: null,                    // No blockchain record
       outcome: "bad",
       details: "Non-payment after file delivery",
       evidence_blobs: [
         signed_transaction_message,      // Downloader's signed promise
         delivery_proof_manifest,         // Proves chunks were sent
         transfer_completion_log          // Timestamps and signatures
       ],
       issuer_id: seeder_peer_id,
       issuer_sig: seeder_signature
     }
     ```
   - This verdict immediately appears in DHT (fast gossip propagation)
   - Seeder optionally submits on-chain complaint (costs gas, more permanent)

6. **Verification by Other Peers**
   - Any peer can retrieve the verdict from DHT
   - Peers validate:
     - `signed_transaction_message` signature (proves downloader's intent)
     - `delivery_proof` completeness
     - No matching `tx_hash` exists on blockchain (confirms non-payment)
   - **Downloader cannot dispute** without providing blockchain payment proof
   - Multiple failed payments to different seeders compound the penalty

7. **Reputation Impact**
   - Immediate: DHT gossip penalty reduces downloader's score
   - Short-term: Reputation-aware seeders refuse to serve this downloader
   - Long-term: If on-chain complaint filed, permanent reputation damage
   - Recovery: Downloader must complete many successful transactions to rebuild trust

**Optional reliable penalty path:** If the payment model uses a channel that allows the seeder to reclaim funds on timeout, the seeder’s close transaction (the reclaim receipt) can be attached as `tx_receipt` for a stronger, chain-anchored `bad` verdict. This augments—not replaces—the signed transaction message evidence.

#### Why Signed Messages Solve the Problem

**The Challenge:**
- File transfer happens P2P (no blockchain involvement)
- If downloader doesn't pay, there's nothing on-chain to prove it
- Seeder needs cryptographic proof for complaints

**The Solution:**
- Signed transaction message = **unforgeable payment promise**
- Signature proves downloader agreed to pay (can't deny intent)
- Other peers can independently verify the signature
- Acts as "evidence" even without blockchain record
- Multiple signed messages from different seeders = strong pattern of non-payment

**Key Properties:**
- ✅ Can't be forged (requires downloader's private key)
- ✅ Can't be repudiated (signature proves authenticity)
- ✅ Can't be reused (nonce + file_hash make each unique)
- ✅ Can be verified by anyone (public key cryptography)
- ✅ Doesn't require blockchain (works off-chain)

#### Seeder Protection Strategy

The design accepts that **malicious seeders** are harder to prove, so we protect seeders as the value providers:

**Downloader as Acceptable Victim:**
- No financial loss (payment only sent after delivery)
- Can abort and retry with different seeder
- Initial reputation check filters bad seeders
- Seeder malicious behavior is self-punishing (loses payment)

**Seeder as Protected Party:**
- Has cryptographic proof if downloader doesn't pay
- Can publish complaint with signed transaction message
- Delivers value first, so deserves protection
- Uptime bonus rewards long-term reliable seeders

**The longer a seeder is online successfully serving files, the higher their reputation, making malicious seeder identity costly to build.**

#### False Complaint Defense (Malicious Seeder Files False Non-Payment Claim)

**Problem:** Downloader pays honestly, but malicious seeder falsely claims non-payment to damage downloader's reputation.

**Solution:** Blockchain proof overrides DHT gossip. Honest downloader can prove payment exists on-chain.

1. Scenario Setup
   ├─ Downloader (A) receives file from Seeder (B)
   ├─ A submits payment to blockchain (honest behavior)
   └─ B files false 'bad' verdict claiming non-payment (malicious)

2. Seeder Files False Complaint
   ├─ B publishes 'bad' verdict to DHT with signed message
   └─ Complaint spreads via gossip

3. Downloader Defense
   ├─ A monitors DHT for complaints about themselves
   ├─ A detects false complaint from B
   └─ A submits dispute with blockchain proof:
       - tx_hash: Points to confirmed payment transaction
       - tx_receipt: Full blockchain transaction receipt
       - block_number: Where payment was mined
       - confirmations: Proof transaction is finalized

4. Network Verification
   ├─ Any peer can query blockchain for tx_hash
   ├─ Transaction matches signed message parameters:
   │   - from: A's address ✓
   │   - to: B's address ✓
   │   - amount: File price ✓
   │   - timing: Within deadline ✓
   └─ Conclusion: Payment exists, complaint is false

5. Consequences for Malicious Seeder
   ├─ False complaint dismissed immediately
   ├─ B receives severe reputation penalty (-0.5 or more)
   ├─ "False complaint" flag added to B's permanent record
   ├─ Repeated false complaints trigger auto-blacklist
   └─ Other downloaders avoid B (trust destroyed)

6. Downloader Reputation Restored
   ├─ A's reputation returns to pre-complaint level
   ├─ A publishes counter-verdict: 'disputed_resolved'
   └─ Network sees A as honest, B as malicious

**Why Blockchain Makes False Complaints Unprofitable:**

- **Unforgeable Proof:** Blockchain transactions cannot be faked or hidden
- **Permanent Record:** Payment proof exists forever on-chain
- **High Penalty Cost:** False complaints damage seeder reputation severely (more than successful complaints help)
- **Verifiable by Anyone:** Any peer can independently check blockchain
- **Cumulative Damage:** Multiple false complaints = permanent blacklist
- **Economic Loss:** Malicious seeders lose all future business

**Trust Hierarchy:**
```
1. Blockchain (ABSOLUTE TRUTH)
   └─ Confirmed transactions override all other evidence

2. Cryptographic Signatures (STRONG EVIDENCE)
   └─ Signed messages prove intent but not completion

3. DHT Gossip (ADVISORY)
   └─ Fast propagation but requires verification

4. Peer Claims (WEAK)
   └─ Must be backed by evidence
```

**Automatic Defense Mechanism:**

```typescript
// Downloader publishes pre-emptive verdict immediately after payment
// This creates a defense before seeder can file false complaint
await reputationService.publishVerdict(seederId, {
    outcome: 'good',
    tx_hash: paymentTxHash,
    role: 'downloader',
    confirmations: 12,
    details: 'Payment confirmed on blockchain'
});

// If seeder later files false complaint, network sees:
// - Downloader's verdict: "I paid" (with blockchain proof)
// - Seeder's complaint: "They didn't pay" (contradicts blockchain)
// Result: Seeder's complaint automatically dismissed
```

**Pattern Detection for Malicious Seeders:**

The system tracks false complaint patterns:
- First false complaint: Warning + small penalty
- Second false complaint: Large penalty + trust level drop
- Third false complaint: Automatic blacklist + permanent untrusted status

This makes it extremely costly for seeders to file false complaints, protecting honest downloaders.

### Pre-Transfer Admission Control (Seeder-side)

Before a seeder commits to serving a downloader, it performs an admission check:

1. Identity: Downloader includes `peer_id` and wallet address in the handshake.
2. Reputation lookup: Seeder calls `reputationService.getPeerScore(peer_id)` and checks:
   - Score ≥ configured threshold (e.g., 0.4 or 0.6).
   - No automatic blacklist entry.
3. Balance check: Seeder queries the chain (or a wallet service) to ensure:
   - Downloader’s balance ≥ expected maximum payment.
4. Signed payment message: Seeder verifies the off-chain signed transaction described above.

If any of these steps fail, the seeder may refuse the transfer, reduce the amount of data it is willing to send, or require smaller, incremental payments.

### Design Tradeoff: Asymmetric Protection (Downloader as Potential Victim)

We explicitly prioritize protecting seeders from non-paying downloaders:

1. If a downloader receives a file but never pays, we can use the signed off-chain payment message plus the absence of an on-chain payment within the agreed window (or a seeder-side timeout receipt, if channels are used) as strong negative evidence and penalize the downloader.
2. If a seeder is malicious and never sends the file after the downloader signs a payment message, the downloader is harder to protect:
   - The downloader can choose not to broadcast the payment, so there may be no on-chain footprint.
   - Without privacy-invasive logging or complex cryptographic proofs of data transfer, it is difficult to prove seeder misbehavior beyond “they never replied.”

Therefore the design chooses the downloader as the potential victim in the tricky seeder-misbehaves scenario. The system focuses on reliable penalties for non-paying downloaders, while seeder misbehavior is handled via weaker, gossip-style complaints and manual user judgments.

### Gossip-backed Penalty Signals

Not every misbehaviour is provable on-chain in real time. A seeder may still lodge an advisory complaint by attaching cryptographically signed context—such as the downloader’s handshake promising payment. These `evidence_blobs` form gossip signals:

1. Peers validate signatures to confirm the actors but cannot independently confirm settlement on-chain yet.
2. Clients apply reduced weighting by default, optionally boosting the impact when multiple distinct issuers report the same target with matching context.
3. Gossip penalties never override reliable penalties; they provide early-warning telemetry until the chain produces final evidence.

### Default Scoring Function

Clients aggregate retrieved verdicts using the following weighted average:

score = Σ(weight(event) × value(event)) / Σ(weight(event))

value(good) = 1.0
value(disputed) = 0.5
value(bad) = 0.0

`weight(event)` defaults to `1.0`. Clients may optionally enable exponential time decay by configuring a `decay_window` half-life.

### Derived Transaction Totals

When evaluating trust, clients replay confirmed verdicts to derive how many transactions a peer has successfully completed versus failed. These totals are computed from the same chain-anchored evidence as the weighted score; implementations may cache them locally for faster ranking, but no additional on-chain state is required.

- **Successful settlements** count every `good` (and optionally `disputed`) verdict tied to finalized transactions.
- **Failed settlements** count every reliable `bad` verdict, reversing any success previously credited for that transfer.

Trust-level promotion requires both a high weighted score **and** sufficient successful settlements. Reliable penalties immediately reduce the successful total, while gossip penalties stay advisory until chain evidence arrives.

## Reputation Features

### Publishing Flow (DHT `STORE`)

1. **Issuer assembles verdict** once they deem a transaction final.
2. **Issuer signs payload** with their transaction key.
3. **Issuer publishes** via `DhtService::publish_reputation_verdict` (see API snippet below):
   - Key: `H(target_id || "tx-rep")`.
   - Payload: serialized `TransactionVerdict`.
4. **Receiving DHT peer**:
   - Validates the signature and ensures `issuer_seq_no` is greater than any stored value from that issuer.
   - Checks the chain through its bundled Geth node to confirm `tx_hash` exists and meets the configured confirmation depth.
   - Stores the verdict once confirmed; otherwise caches it pending until confirmation or timeout.
   - Indexes any `tx_receipt` or `evidence_blobs` so queriers can quickly inspect the supporting material.
5. **Replication** follows the overlay’s normal rules (e.g., Kademlia `k` closest peers).

### Retrieval & Scoring (DHT `GET`)

1. **Querier computes key** `H(target_id || "tx-rep")` and issues a DHT lookup.
2. **Querier validates each verdict**:
   - Signature check using cached verifying keys.
   - Confirmation check against local Geth (drop verdicts referencing orphaned or insufficiently confirmed transactions).
   - Deduplicate by `(issuer_id, tx_hash)`.
3. **Categorize penalties**:
   - Apply full penalty weight for complaints with confirmed `tx_receipt` evidence.
   - Apply advisory weight for gossip penalties, optionally raising severity once corroborated across independent issuers.
   - Update any locally cached derived totals (successful vs. failed settlements) if the implementation uses them.
4. **Apply scoring function** to the validated set.
5. **Cache result** locally for `cache_ttl` (default 10 minutes) to reduce repeated lookups.

### Peer Analytics

- **Score trend**: plot aggregated score vs. time.
- **Recent verdicts**: show the latest `(issuer_id, outcome, details, issued_at)`.
- **Confirmation status**: display pending verdicts awaiting sufficient confirmations.
- **Trust level distribution**: bucket peers using the default thresholds.

### Peer Selection

When downloading files, the system:

1. **Queries available seeders** from DHT
2. **Retrieves transaction scores** via the lookup flow
3. **Ranks seeders** by score, breaking ties by freshness, reliable penalty counts, or additional heuristics
4. **Presents top peers** in the selection modal
5. **Allows manual override** if the user prefers a different peer

### Reputation History

Each peer maintains a history of:
- **Aggregated score** over time windows
- **Recent verdicts** (default 100 per target), separated into reliable vs gossip penalties
- **Trust level changes**
- **Pending verdicts** still waiting on chain confirmations

## Blacklisting

Users can blacklist misbehaving peers:

### Blacklist Features

- **Manual blacklisting**: Add peer by ID from the analytics page
- **Automatic blacklisting**: System flags peers that fall below a configurable score or accumulate repeated `bad` verdicts
- **Blacklist reasons**: Document why peer was blocked
- **Timestamp tracking**: When peer was blacklisted
- **Remove from blacklist**: Unblock peers

### Blacklist Criteria

Peers may be automatically blacklisted for:
- Repeated `bad` verdicts from distinct issuers
- Publishing invalid or orphaned transactions
- Protocol violations detected elsewhere in the stack
- Excessive connection abuse (rate-limited separately)

### Blacklist Settings

A simple, user-facing settings panel lets you control how blacklisting behaves. Settings are intentionally straightforward so users can quickly tune protection without needing deep technical knowledge.

- Blacklist mode
  - `manual` — Only block peers you explicitly add.
  -
 `automatic` — Allow the system to add peers that meet configured thresholds.
  - `hybrid` — Both manual and automatic blocking enabled (default).
- Auto-blacklist toggle
  - Enable or disable automatic blacklisting without affecting any manually added entries.
- Score threshold
  - Numeric value (0.0–1.0). Peers whose aggregated score falls below this value become candidates for automatic blacklisting. Default: `0.2`.
- Bad-verdicts threshold
  - Number of distinct `bad` verdicts from different issuers required to trigger automatic blacklisting. Default: `3`.
- Retention / automatic unban
  - How long a peer stays on the automatic blacklist before being eligible for automatic removal (or re-evaluation). Default: `30 days`.
- Notification preferences
  - Enable notifications when a peer is automatically blacklisted so you can review and optionally unblock them.
- Reason & notes
  - When blocking (manual or automatic), a short reason can be stored for later review (plain-text, small size).
- Local vs. shared
  - Blacklists are local to your client by default. Sharing blacklists across peers or publishing them to the network is intentionally out of scope for privacy and abuse reasons.

These settings are exposed in the Settings page under "Reputation" and via the Analytics/Peer view where you can quickly add, review, or remove blacklisted peers.

## Privacy Considerations

### What's Tracked

- Peer IDs (not real identities)
- Transaction verdict metadata (`outcome`, optional `details`)
- Confirmation status
- Issuer identifiers for verification

**On-Chain:**
- Peer IDs (cryptographic identifiers, not real identities)
- Transaction hashes and block numbers
- Complaint records with evidence hashes
- Resolution outcomes

**In DHT:**
- Recent transaction summaries
- Gossip complaints with evidence
- Cached reputation scores
- Peer activity timestamps

### What's NOT TRACKED

- File content or names
- Real-world identities
- IP addresses (hidden via relay/proxy if configured)
- Personal information or payment details beyond the chain reference
- Private keys or wallet details

### Anonymous Mode

When anonymous mode is enabled:
- Reputation persists per peer key; rotating keys resets reputation
- You can still view others’ reputation provided you can reach the DHT
- IP address is masked via relay/proxy where applicable

## Implementation Notes

### DHT API Stubs

```rust
impl DhtService {
    pub async fn publish_reputation_verdict(
        &self,
        key: String,
        verdict: TransactionVerdict,
    ) -> Result<(), String> {
        // Validate locally, then send STORE request to responsible peers.
    }

    pub async fn fetch_reputation_verdicts(
        &self,
        key: String,
    ) -> Result<Vec<TransactionVerdict>, String> {
        // Issue GET, collect responses, dedupe, and return raw payloads.
    }
}
```

Library consumers should build higher-level helpers that:
- Compute the deterministic key for a `target_id`.
- Handle pending verdict caching and confirmation rechecks.
- Expose the weighted average score to UI and selection logic.

### Configuration Defaults

| Parameter | Description | Default |
|-----------|-------------|---------|
| `confirmation_threshold` | Blocks required beyond `tx_hash` before a verdict is accepted / Blocks required before transaction counts for reputation | 12 |
| `confirmation_timeout` | Max duration to keep a verdict pending before dropping it. | 1 hour |
| `maturity_threshold` | Transactions needed to reach max base score (1.0) | 100 |
| `decay_window` | Half-life (seconds) for optional time decay. | Disabled |
| `decay_half_life` | Half-life for optional time decay (days) | 90 |
| `retention_period` | How long to keep accepted verdicts before pruning. | 90 days |
| `max_verdict_size` | Maximum bytes allowed in `details`. | 1 KB |
| `cache_ttl` | Duration to cache aggregated scores locally. | 10 minutes |
| `blacklist_mode` | How automatic blacklisting behaves: `manual`, `automatic`, or `hybrid`. | `hybrid` |
| `blacklist_auto_enabled` | Enable automatic blacklisting (does not affect manual entries). | true |
| `blacklist_score_threshold` | Score below which a peer becomes eligible for automatic blacklisting (0.0–1.0). | 0.2 |
| `blacklist_bad_verdicts_threshold` | Distinct `bad` verdicts from different issuers required to auto-blacklist a peer. | 3 |
| `blacklist_retention` | How long automatic blacklist entries are retained before re-evaluation or auto-unban. | 30 days |
| `payment_deadline_default` | Default deadline for signed transaction messages (seconds from handshake). | 3600 (1 hour) |
| `payment_grace_period` | Additional wait time after deadline before filing non-payment complaint (seconds). | 1800 (30 min) |
| `signed_message_nonce_ttl` | How long to track used nonces to prevent replay attacks (seconds). | 86400 (24 hours) |
| `min_balance_multiplier` | Required balance as multiple of file price (e.g., 1.5 = 150% of price). | 1.2 |
| `signature_algorithm` | Cryptographic signature scheme for signed transaction messages. | `secp256k1` |

## Using Reputation Data

### For Downloads

1. **Retrieve seeder scores** through the DHT lookup workflow.
2. **Prefer Trusted peers** for critical payloads.
3. **Monitor transfers** and issue a `bad` verdict if they fail.
4. **Escalate disputes** by publishing `disputed` verdicts and including relevant metadata.

Example workflow (client-side pseudocode):

```text
import { reputationService } from '$lib/services/reputationService';

// Get available seeders
const seeders = await dhtService.findSeeders(fileHash);

// Score each seeder
const scoredSeeders = await Promise.all(
    seeders.map(async (seeder) => ({
        ...seeder,
        reputation: await reputationService.getPeerScore(seeder.id),
    }))
);

// Sort by reputation
const ranked = scoredSeeders.sort((a, b) => b.reputation - a.reputation);

// Present top candidates
showPeerSelectionModal(ranked.slice(0, 10));
```

### For Uploads

```text
import { getTransactionScore } from '$lib/services/reputation';

const score = await getTransactionScore(targetPeerId, {
  confirmationThreshold: 12,
  cacheTtl: 600_000,
});
```

1. **Complete transfers reliably** to earn positive verdicts.
2. **Publish verdicts promptly** to keep your partners’ records up to date.
3. **Monitor your own score** and investigate negative spikes.

### For Network Operations

1. **Track global score distribution** to spot suspicious clusters.
2. **Feed low-score peers** into automated blacklists or rate limiters.
3. **Tune parameters** (`confirmation_threshold`, retention) based on observed chain conditions.

### Filing Complaints

**Example: Non-payment complaint with signed transaction message**

```text
// We have the downloader's signed transaction message from handshake
const signedTransactionMessage = {
    from: downloaderId,
    to: myPeerId,
    amount: filePrice,
    file_hash: fileHash,
    nonce: transferNonce,
    deadline: transferDeadline,
    downloader_signature: downloaderSig  // Cryptographic proof
};

const evidence = {
    signed_transaction_message: signedTransactionMessage, // Unforgeable payment promise
    delivery_proof: chunkManifest,                       // Proof we sent all chunks
    transfer_completion_log: completionTimestamp,        // When transfer finished
    protocol_logs: transferLogs,                         // Connection logs
};

// File gossip complaint first (fast, no gas cost)
await reputationService.fileComplaint(
    downloaderId,
    'non-payment',
    evidence,
    false // DHT gossip - immediate warning to network
);

// Then optionally file on-chain complaint (permanent, costs gas)
// Recommended after multiple failed payments
await reputationService.fileComplaint(
    downloaderId,
    'non-payment',
    evidence,
    true // On-chain - permanent record
);
```

**Verification snippet (pseudocode):**

```text
const isValid = await crypto.verifySignature(
    signedTransactionMessage,
    downloaderPublicKey
);

const txExists = await blockchain.findTransaction(
    signedTransactionMessage.from,
    signedTransactionMessage.to,
    signedTransactionMessage.amount,
    signedTransactionMessage.file_hash
);

if (isValid && !txExists) {
    // Confirmed non-payment: apply penalty
    await reputationService.applyPenalty(downloaderId, 'non_payment_confirmed');
}
```

**Example: Pre-transfer validation flow for the seeder**

This example illustrates the expected ordering: signature validation first, then reputation and balance checks.

```text
async function validateDownloaderHandshake(
    signedMessage: SignedTransactionMessage
): Promise<boolean> {
    // 1. Verify signature (must be present before any balance/reputation checks)
    const sigValid = await crypto.verifySignature(
        signedMessage,
        signedMessage.from
    );
    if (!sigValid) return false;

    // 2. Check downloader's reputation
    const reputation = await reputationService.getPeerScore(
        signedMessage.from
    );
    if (reputation < MINIMUM_REPUTATION_THRESHOLD) {
        console.log('Downloader reputation too low, rejecting');
        return false;
    }

    // 3. Check downloader's balance on blockchain
    const balance = await blockchain.getBalance(signedMessage.from);
    if (balance < signedMessage.amount) {
        console.log('Insufficient balance, rejecting');
        return false;
    }

    // 4. Check deadline is reasonable
    if (signedMessage.deadline < Date.now() + MIN_TRANSFER_TIME) {
        console.log('Deadline too soon, rejecting');
        return false;
    }

    return true; // Safe to proceed with transfer
}
```

## Troubleshooting

### Low Reputation Score

**Causes**:
- High proportion of `bad` verdicts
- Stale positive history outweighed by fresh negatives
- Peers disputing transactions due to unresolved issues

**Solutions**:
- Improve internet connection
- Resolve disputed transactions and request updated verdicts
- Avoid publishing verdicts until transactions are safely confirmed
- Keep application online so partners can issue follow-up positive verdicts

### Peers Not Showing Reputation

**Causes**:
- New peers (no history)
- DHT not connected
- Reputation service not initialized
- Pending verdicts waiting for confirmations

**Solutions**:
- Wait for peers to interact
- Check Network page for DHT status
- Restart application
- Verify local Geth sync height and confirmation parameters

### Reputation Not Updating

**Causes**:
- No recent transfers
- Application not running
- Backend service issue
- Cached score expired but lookup failed

**Solutions**:
- Perform some transfers
- Check console for errors
- Restart application
- Drop local cache or increase `cache_ttl`

## See Also

- [Network Protocol](network-protocol.md) — Peer discovery details
- [File Sharing](file-sharing.md) — Transfer workflows
- [Wallet & Blockchain](wallet-blockchain.md) — Chain interaction details
- [Roadmap](roadmap.md) — Planned uptime/storage reputation extensions

## Related Systems

- **Bitcoin / Ethereum:** No off-chain reputation; consensus alone determines validity.
- **IPFS:** Uses bilateral Bitswap ledgers stored locally per peer pair; no global reputation namespace.
- **Filecoin:** Implements on-chain collateral and storage proofs instead of off-chain scores.

## Future Extensions

### Transaction & Payment Enhancements
- **Payment channels**: Replace single signed messages with bi-directional payment channels for frequent traders, reducing blockchain transactions
- **Escrow smart contracts**: Optional on-chain escrow that automatically releases payment upon cryptographic proof of delivery
- **Multi-signature settlements**: Support multi-party transactions for bundle deals or group purchases
- **Automatic retry with penalty escalation**: If downloader fails to pay, automatically escalate complaint severity with each subsequent non-payment to different seeders

### Reputation Metric Expansions
- **Uptime tracking**: Introduce `uptime` metric label backed by periodic peer probes and relay observations
  - Track continuous online duration (longer = more trustworthy)
  - Weight recent uptime more heavily than historical
  - Penalize frequent disconnections or unstable peers
- **Relay reputation**: Add `relay` metric for Circuit Relay v2 operators
  - Track relay reliability, bandwidth, and availability
  - Reward relay operators with reputation bonuses
  - Display relay leaderboard based on service quality
- **Storage proof metrics**: Implement verifiable storage challenges to prove seeders still have advertised files
- **Bandwidth metrics**: Track actual upload/download speeds vs. advertised capabilities

### Evidence & Proof Improvements
- **Merkle proofs for chunk delivery**: Replace simple manifests with Merkle tree proofs for efficient chunk verification
- **Zero-knowledge proofs**: Allow seeders to prove file delivery without revealing file content or transfer details
- **Multi-witness complaints**: Require multiple independent witnesses before applying severe penalties
- **Encrypted evidence**: Support encrypted or hashed `details` for privacy-sensitive metadata
- **Evidence expiration**: Automatically prune old evidence blobs after reputation impact has decayed

### Real-Time & Performance
- **Streaming reputation updates**: Provide pub/sub for near-real-time score changes
- **Reputation prediction**: Machine learning models to predict peer reliability based on behavioral patterns
- **Cached reputation snapshots**: Periodic blockchain snapshots of high-reputation peers for faster bootstrapping
- **DHT replication strategy**: Optimize DHT storage to prioritize high-activity peers

### Advanced Scoring
- **Reviewer credibility weighting**: Weight complaints by the issuer's own reputation (trusted peers' complaints carry more weight)
- **Contextual reputation**: Separate scores for different file types or transaction sizes (e.g., great with small files, unreliable with large files)
- **Time-decay refinements**: Exponential decay for old transactions with configurable half-life per metric type
- **Geographic reputation**: Track reputation separately by region to account for network conditions

### Dispute Resolution
- **Multi-stage dispute process**: Allow disputed verdicts to be challenged with counter-evidence before finalizing
- **Community arbitration**: Opt-in arbitration by high-reputation peers for complex disputes
- **Appeal mechanism**: Allow peers to appeal automatic blacklisting with evidence of system errors
- **Reputation recovery programs**: Structured paths for low-reputation peers to rebuild trust through verified good behavior
- **False complaint tracking**: Maintain permanent records of false complaints to identify repeat offenders

---

## Summary: How It All Works Together

### The Non-Payment Problem & Solution

**The Challenge:** In a P2P file sharing system, file transfers happen off-chain for speed and efficiency. But this creates a problem: if a downloader receives a file and doesn't pay, there's nothing on the blockchain to prove the transaction was supposed to happen.

**The Solution:** **Signed Transaction Messages** — cryptographic payment promises that work off-chain.

### Complete Transaction Flow

1. **Discovery & Vetting**
   - Downloader finds seeder via DHT
   - Downloader checks seeder reputation and chooses a candidate

2. **Handshake (Off-Chain)**
   - Downloader creates signed message: "I promise to pay X coins to seeder Y for file Z by deadline D"
   - Signature is cryptographic proof (requires downloader's private key)
   - Seeder validates signature first, then verifies downloader reputation and balance
   - Seeder stores the message as evidence

3. **File Transfer (Pure P2P)**
   - No blockchain involvement during transfer
   - Fast, efficient, direct peer-to-peer
   - Seeder logs delivery proof (chunk manifest, timestamps)

4. **Payment (On-Chain)**
   - **Honest case:** Downloader submits payment to blockchain, everyone wins
   - **Malicious case:** Downloader doesn't pay, but seeder has signed message as proof

5. **Reputation Update**
   - **If paid:** Both parties publish 'good' verdicts referencing blockchain tx_hash
   - **If not paid:** Seeder publishes 'bad' verdict with signed message as evidence
   - Other peers can verify the signature independently
   - Downloader can't dispute without providing blockchain payment proof

### Why This Works

**Cryptographic Properties:**
- ✅ **Unforgeable:** Only downloader's private key can create valid signature
- ✅ **Non-repudiable:** Downloader can't deny agreeing to pay
- ✅ **Verifiable:** Any peer can independently verify signature authenticity
- ✅ **Unique:** Nonce + file hash prevent reuse or replay attacks
- ✅ **Off-chain:** No blockchain delay or cost during file transfer

**Economic Incentives:**
- Downloader loses reputation if they don't pay (blocked by future seeders)
- Seeder earns reputation by serving reliably over time
- Long-term honest behavior is more valuable than one-time cheating
- Uptime bonus makes malicious seeder identities costly to build

### Handling Edge Cases

**Malicious Downloader (Non-payment):**
- Seeder has signed message as unforgeable proof
- Can publish complaint to DHT immediately (gossip penalty)
- Can file on-chain complaint for permanent record
- Multiple complaints from different seeders compound the penalty
- Downloader becomes untrusted and blacklisted

**Malicious Seeder (False Complaint):**
- Downloader proves payment with blockchain transaction
- Blockchain proof overrides seeder's DHT complaint
- Seeder receives severe reputation penalty for false complaint
- False complaint flag permanently damages seeder's trust
- Repeated false complaints trigger automatic blacklist

**Malicious Seeder (Non-delivery):**
- Downloader aborts transfer, doesn't send payment
- No financial loss (payment only sent after delivery)
- Can find different seeder with better reputation
- Seeder loses potential payment (self-punishing)
- Design accepts downloader as "acceptable victim" in this case

**Why Downloader is Protected (from false complaints):**
- Blockchain provides unforgeable proof of payment
- False complaints are automatically dismissed with blockchain evidence
- Malicious seeders face severe penalties (more than legitimate complaints)
- System makes false complaints economically irrational

### Key Design Principles

1. **Blockchain as Ground Truth:** All finalized transactions are immutable proof
2. **DHT as Fast Cache:** Quick lookups without querying blockchain every time
3. **Cryptographic Evidence:** Signed messages provide proof even without blockchain record
4. **Seeder Protection Priority:** Seeders provide value, so system protects them first
5. **Uptime Rewards:** Longer online = higher reputation = more trusted
6. **Hybrid Verification:** Recent activity via DHT, historical via blockchain

### Implementation Highlights

**For Seeders:**
- Always validate signed messages before starting transfer
- Check downloader's reputation and balance (after signature verification)
- Store signed message as evidence
- Wait until deadline + grace period before filing complaint
- Earn reputation by staying online and serving reliably

**For Downloaders:**
- Check seeder reputation before handshake
- Create properly signed transaction message
- Submit payment promptly after receiving file
- Build reputation through successful transactions
- Higher reputation = more seeders willing to serve you

**For the Network:**
- DHT propagates reputation verdicts quickly
- Anyone can verify signed message authenticity
- Blockchain provides ultimate source of truth
- Reputation aggregates over time
- System converges toward honest behavior
