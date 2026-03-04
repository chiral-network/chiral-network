# Decentralized Reputation System Design

## Context

The current reputation system stores scores locally in `localStorage` (`chiral.reputation.store`), making it trivially tamper-able. A malicious peer can edit their browser storage to fake high reputation scores. The system also has no way for peers to verify each other's claims about reputation.

The codebase already has significant infrastructure that we can build on:
- **`reputation.rs`** (2,465 lines): `ReputationEvent` (18 event types, ed25519 signatures), `TransactionVerdict` (signed payment proofs), `ReputationMerkleTree` (epoch batching), `ReputationEpoch`, `NodeKeyManager`, `ReputationVerifier`
- **DHT**: `put_dht_value`/`get_dht_value` for arbitrary key-value storage with Kademlia replication
- **Blockchain**: Custom Ethereum chain (chain ID 98765), Geth integration, `ProofOfStorage.sol`, existing `ReputationEpochContract` ABI with `submitEpoch()`/`verifyEventProof()`
- **Peer metrics**: `peer_selection.rs` + `peer_health.rs` track transfer counts, success rates, latency, bandwidth
- **Rate limiter**: `reputationRateLimiter.ts` with 30 verdicts/day, 6 per target/day (currently log-only mode)

**Goal**: Replace local-only reputation with a tamper-proof, distributed system where scores are derived from cryptographically signed evidence stored in the DHT and anchored on-chain.

---

## Architecture: Three-Tier Design

```
Tier 3: BLOCKCHAIN (Anchor Layer)
  - Epoch merkle roots (immutable, periodic)
  - ReputationRegistry contract (peer stake, identity binding)
  - Verification: verifyEventProof(hash, proof, epochId)

Tier 2: DHT (Evidence Layer)
  - Signed TransactionVerdicts (individual ratings)
  - Peer public keys (for signature verification)
  - ReputationSummaries (aggregated scores, independently verifiable)
  - Epoch metadata

Tier 1: LOCAL (Cache Layer - in-memory only, NOT persisted)
  - Verified score cache (5-min TTL for peer selection)
  - Public key cache (long TTL)
  - No localStorage for reputation data
```

**Data flow**: Transfer completes -> both peers sign a verdict -> verdict stored in DHT -> periodically batched into epochs -> epoch merkle root anchored on-chain -> any peer can fetch verdicts, verify signatures, recompute scores deterministically.

---

## Data Model

### DHT Key Schema

| Key | Value | Purpose |
|-----|-------|---------|
| `rep:verdict:{H(issuer\|\|target\|\|"tx-rep")}` | `TransactionVerdict` (already exists) | Individual signed rating |
| `rep:verdicts-for:{target_peer_id}` | `Vec<TransactionVerdict>` (new) | All verdicts about a peer (append-only list) |
| `rep:pubkey:{peer_id}` | `PeerKeyRecord` (new) | Public key for signature verification |
| `rep:epoch:{epoch_id}` | `ReputationEpoch` (already exists) | Epoch metadata + merkle root |
| `rep:summary:{target_peer_id}:{epoch_id}` | `ReputationSummary` (new) | Aggregated score, independently verifiable |

### New Rust Structures (in `reputation.rs`)

```rust
/// Published to DHT so other peers can verify verdict signatures
pub struct PeerKeyRecord {
    pub peer_id: String,
    pub ed25519_public_key: String,     // hex-encoded
    pub registered_at: u64,
    pub proof_of_work_nonce: u64,       // Sybil resistance: H(peer_id||nonce) < difficulty
    pub signature: String,              // self-signed (proves key ownership)
}

/// Aggregated score computed deterministically from verified verdicts
pub struct ReputationSummary {
    pub target_peer_id: String,
    pub epoch_id: u64,
    pub score: f64,                     // 0.0 to 1.0
    pub confidence: f64,                // 0.0 to 1.0 (based on evidence quantity)
    pub total_verdicts: u64,
    pub positive_verdicts: u64,
    pub negative_verdicts: u64,
    pub unique_raters: u64,
    pub evidence_merkle_root: String,   // root of all verdicts used
    pub computed_by: String,
    pub computed_at: u64,
    pub signature: String,
}

/// Proof of meaningful activity (Sybil resistance)
pub enum ActivityProofType {
    ProofOfWork { nonce: u64, difficulty: u32 },     // ~1 sec per identity
    TokenStake { amount: u64, block_number: u64 },   // economic cost
    TransferEvidence { transfer_count: u64, total_bytes: u64 },
}
```

### What Goes Where

| Data | Storage | Why |
|------|---------|-----|
| Individual verdicts | **DHT** | Signed, replicated, tamper-evident |
| Epoch merkle roots | **Blockchain** | Immutable anchor (only 32 bytes per epoch) |
| Peer public keys | **DHT** | Discoverable for signature verification |
| Aggregated summaries | **DHT** | Computed by multiple peers, cross-verifiable |
| Hot score cache | **Local memory** | Fast peer selection (NOT persisted to disk) |

---

## Score Computation Algorithm

### Deterministic Aggregation (any peer can reproduce the same result)

```
Input: all verified verdicts about target_peer_id
Output: ReputationSummary { score, confidence }

1. FILTER: Reject verdicts where:
   - Invalid ed25519 signature
   - issuer_id == target_id (self-rating)
   - Older than 90 days
   - Rater public key not found in DHT

2. TIME DECAY: For each verdict:
   decay = 2^(-(age_days - 0.04) / 7)   // 7-day half-life, 1-hour grace
   (Uses existing RepScoreCalc.calc_decay())

3. RATER CREDIBILITY WEIGHT:
   weight = decay * rater_credibility * evidence_factor
   Where:
   - rater_credibility = rater's own reputation (bootstrapped at 0.5)
   - evidence_factor = 1.0 if verdict has transfer proof, 0.1 if not

4. COLLUSION DISCOUNT:
   - If A rated B AND B rated A, and both have < 3 other raters:
     discount = 0.5^reciprocal_count

5. AGGREGATE (using existing tanh-bounded approach):
   positive_sum = sum(weight * impact) for Good verdicts
   negative_sum = sum(weight * |impact| * 1.5) for Bad verdicts  // negatives amplified
   raw = positive_sum - negative_sum
   score = 0.5 + tanh(raw / 100) * 0.5   // bounded [0, 1], neutral at 0.5

6. CONFIDENCE:
   confidence = min(1.0, unique_raters / 5)
```

### Rater Credibility Bootstrap (PageRank-inspired)

Circular dependency resolved iteratively:
- **Iteration 0**: All peers credibility = 0.5
- **Iterations 1-5**: Recompute scores using previous iteration's credibilities
- **Converge**: Stop when max delta < 0.01

Only done during epoch computation, not per-query.

### Cold Start (New Peers)

- New peers start with score = 0.5 (neutral), confidence = 0.0
- Frontend blends: `effective_score = confidence * verified_score + (1 - confidence) * local_success_rate`
- As a peer accumulates verified verdicts, confidence rises and verified score dominates

---

## Anti-Tampering Mechanisms

### 1. Self-Rating Prevention
- `TransactionVerdict.validate()` already rejects `issuer_id == target_id`
- Verification enforced at **read time** (when computing scores), not just write time
- Transfer-evidence binding: both parties sign a shared handshake nonce before transfer; you can't be both parties

### 2. Sybil Resistance (Multi-Layered)

| Layer | Mechanism | Cost to Attacker |
|-------|-----------|-----------------|
| PoW on identity | `H(peer_id \|\| nonce) < 2^(256-20)` | ~1 sec per identity |
| Token stake | 100 CHI minimum for full-weight verdicts | Economic cost per Sybil |
| Rater credibility | New identities start at 0.5, need real history for higher weight | Can't bootstrap weight without real transfers |
| Transfer-evidence | Verdicts without transfer proof weighted at 10% | Must actually transfer data |
| Unique rater threshold | Confidence stays low until 5+ unique raters | Many Sybils still show "Low Confidence" |

### 3. Collusion Detection
- **Reciprocal rating**: A<->B mutual positive ratings with few other raters -> discount
- **Rate limits**: Max 30 verdicts/day, 6 per target/day, burst detection (existing infrastructure)
- **Statistical outliers**: Raters consistently deviating from consensus get reduced weight

### 4. DHT Integrity
- All verdicts ed25519 signed -> malicious DHT nodes can't forge them
- Epoch merkle roots on-chain -> evidence is immutable once anchored
- Kademlia replication -> censorship requires controlling multiple k-bucket nodes

---

## Blockchain Integration

### Existing Contract (keep as-is)
```solidity
// ReputationEpochContract (already in reputation.rs ABI)
submitEpoch(uint64 epochId, bytes32 merkleRoot, uint64 timestamp, uint256 eventCount)
getEpoch(uint64 epochId) -> (merkleRoot, timestamp, eventCount, submitter)
verifyEventProof(bytes32 eventHash, bytes32[] proof, uint64 epochId) -> bool
```

### New Contract: ReputationRegistry.sol
```solidity
contract ReputationRegistry {
    uint256 public constant MIN_STAKE = 100 ether; // 100 CHI

    struct PeerRegistration {
        bytes32 ed25519PubKeyHash;
        uint256 stakedAmount;
        uint256 registeredBlock;
        bool isActive;
    }

    mapping(address => PeerRegistration) public peers;

    function registerPeer(bytes32 pubKeyHash) external payable;
    function stake() external payable;
    function unstake(uint256 amount) external;  // with cooldown
    function isQualifiedRater(address peer) external view returns (bool);
    function getStake(address peer) external view returns (uint256);
}
```

### When Things Go On-Chain

| Event | Action | Frequency |
|-------|--------|-----------|
| New peer joins | `registerPeer()` | Once per peer |
| Peer adds stake | `stake()` | Occasional |
| Epoch finalization | `submitEpoch()` | Every 1 hour or 100 events |

Gas costs minimal: only 32-byte merkle root per epoch, not individual events.

### Score Verification Flow

When peer A claims "my reputation is 0.85":
1. Peer B fetches epoch IDs from blockchain
2. Fetches raw verdicts from DHT for those epochs
3. Verifies each verdict's ed25519 signature
4. Optionally verifies merkle inclusion proofs against on-chain roots
5. Runs the deterministic scoring algorithm
6. Compares result with A's claim

---

## Integration Points

### Peer Selection (`peerSelectionService.ts`)

**Current** (local-only):
```typescript
compositeScoreFromMetrics(p) -> 0.6 * localRepScore + 0.25 * freshScore + 0.15 * perfScore
```

**New** (verified):
```typescript
// Check in-memory cache (5-min TTL) first
const verified = await invoke('get_verified_reputation', { peerId: p.peer_id });
const blendedRep = verified.confidence * verified.score + (1 - verified.confidence) * p.success_rate;
return 0.6 * blendedRep + 0.25 * freshScore + 0.15 * perfScore;
```

### New Tauri Commands

| Command | Purpose |
|---------|---------|
| `get_verified_reputation(peer_id)` | Fetch verdicts from DHT, verify, compute score |
| `register_peer_key()` | Publish ed25519 public key + PoW to DHT on startup |
| `get_epoch_status()` | Current epoch info (pending events, time to finalization) |
| `submit_reputation_verdict(target_id, outcome, evidence)` | Create signed verdict, store in DHT |

### Frontend Changes

- **Reputation page**: Replace local score computation with `get_verified_reputation` calls
- **ReputationCard**: Add "Verified" / "Unverified" badge based on confidence
- **ReputationAnalytics**: Add epoch status widget (current epoch, on-chain anchor status)
- **ReputationStore**: Convert from localStorage-backed to in-memory cache only

---

## Implementation Phases

### Phase 1: Verified Score Computation (MVP)
**Goal**: DHT-verified scoring with backward-compatible fallback

1. Add `reputation_aggregator` module in `reputation.rs`:
   - `compute_verified_score()`: fetch verdicts from DHT, verify signatures, run deterministic algorithm
   - `VerifiedReputation { score, confidence, verdict_count, last_updated }` return type
2. Publish peer ed25519 public keys to DHT on node startup (`rep:pubkey:{peer_id}`)
3. Add `get_verified_reputation` Tauri command in `main.rs`
4. Update `peerSelectionService.ts` to call verified scoring (fallback to local if DHT unavailable)
5. Add "Verified" badge to `ReputationCard.svelte`

**Files**: `reputation.rs`, `dht.rs`, `main.rs`, `peerSelectionService.ts`, `ReputationCard.svelte`

### Phase 2: Sybil Resistance + Transfer Evidence
**Goal**: Harden against fake identities and unsubstantiated ratings

1. Add PoW requirement to `PeerKeyRecord` (reject keys without valid PoW)
2. Add bidirectional transfer handshake proof to `TransactionVerdict`
3. Port rate limiter from TypeScript to Rust (enforce at DHT write level)
4. Implement rater credibility weighting in score computation

**Files**: `reputation.rs`, `file_transfer.rs`, `dht.rs`

### Phase 3: Blockchain Anchoring
**Goal**: Immutable epoch anchoring for tamper-proof verification

1. Deploy `ReputationRegistry.sol` (peer registration + token staking)
2. Automate epoch finalization in DHT event loop (every 1h or 100 events)
3. Wire `ReputationEpochContract.submitEpoch()` for on-chain merkle root submission
4. Add merkle proof generation/verification for individual verdicts
5. Add epoch status widget to Reputation page

**Files**: `reputation.rs`, `dht.rs`, `ReputationRegistry.sol`, `Reputation.svelte`

### Phase 4: Collusion Detection + Advanced Analytics
**Goal**: Detect coordinated manipulation

1. Build rater-target graph from DHT verdicts
2. Implement reciprocal rating detection + cluster analysis
3. Add statistical outlier rater penalty
4. Publish `ReputationSummary` to DHT (cross-verifiable by multiple peers)
5. Add network trust graph visualization to Reputation page

**Files**: `reputation.rs` (new `collusion_detector` module), `ReputationAnalytics.svelte`

### Phase 5: Remove Local Storage
**Goal**: Eliminate tampering surface entirely

1. Remove `localStorage` persistence from `ReputationStore`
2. Convert to pure in-memory cache of verified scores
3. All reputation paths go through verified Tauri backend
4. Migration path for users upgrading from local-only versions

**Files**: `reputationStore.ts`, `peerSelectionService.ts`

---

## Verification Plan

- **Phase 1**: Start two local DHT nodes, complete a file transfer, verify verdict appears in DHT and score is computable by the other node
- **Phase 2**: Attempt to register a peer key without PoW -> rejected; submit verdict without transfer proof -> weighted at 10%
- **Phase 3**: Submit epoch, verify merkle root appears on-chain, verify `verifyEventProof()` returns true for included verdicts
- **Phase 4**: Create a ring of 3 colluding test peers, verify their mutual ratings are discounted
- **Phase 5**: Clear localStorage, verify reputation scores still load from DHT

Rust tests: `cargo test` in `src-tauri/`
TypeScript tests: `npm test` in `v2-chiral-network/`

---

## Key Files

| File | Role |
|------|------|
| `src-tauri/src/reputation.rs` | Core types, signing, merkle trees, epochs, verifier -- primary extension point |
| `src-tauri/src/dht.rs` | DHT storage/retrieval, verdict publishing, event loop |
| `src-tauri/src/peer_selection.rs` | Backend peer metrics and selection strategies |
| `src-tauri/src/main.rs` | Tauri command definitions (add new commands here) |
| `src/lib/services/peerSelectionService.ts` | Frontend peer selection (modify to use verified scores) |
| `src/lib/reputationStore.ts` | Current localStorage-backed store (migrate to in-memory cache) |
| `src/lib/services/reputationService.ts` | Frontend verdict publishing |
| `src/lib/services/reputationRateLimiter.ts` | Rate limiting (port to Rust) |
| `src/pages/Reputation.svelte` | Reputation UI page |
| `src/lib/components/ReputationCard.svelte` | Individual peer reputation display |
| `src/lib/components/ReputationAnalytics.svelte` | Analytics dashboard |
