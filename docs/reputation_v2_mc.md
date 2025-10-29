# Reputation System

> **Status:** MVP design for transaction-backed reputation. Uptime, storage, and relay metrics will ship later as extensions once out-of-band evidence is available.

## Overview

Chiral Network tracks peer reputation through verifiable, transaction-centric evidence. Confirmed on-chain transaction history is the authoritative ledger: every payment or settlement that finalizes on-chain becomes durable ground truth. To keep costs low and latency acceptable, clients publish signed **Transaction Verdicts** into the DHT as an index of recent interactions. Consumers fetch those verdicts for fast heuristics, but they always re-validate against the chain (or cached receipts) before acting; if a verdict cannot be bridged back to finalized chain history, it is ignored. This hybrid model lets us iterate inside today’s infrastructure while reserving long-term accuracy to the blockchain. Later releases may reuse the same storage model to incorporate additional metrics (uptime, relay quality, etc.) once the supporting evidence flow exists.

### Goals

- Provide a verifiable reputation signal without changing the on-chain protocol.
- Keep the system PoW-friendly: identities correspond to existing mining/transaction keys, and no dedicated storage nodes are required.
- Allow future metrics to plug into the same DHT namespace without breaking compatibility.

## Trust Levels

Peers are bucketed by their **transaction score**, a weighted average of verdicts retrieved from the DHT. Default weights: `good = 1.0`, `disputed = 0.5`, `bad = 0.0`. Additional decay or weighting can be applied client-side. Reputation grows with the number of confirmed transactions a peer completes; clients derive those totals directly from chain-validated verdicts and may require a minimum number of successful settlements before promoting a peer into higher trust brackets.

| Trust Level | Score Range | Description |
|-------------|-------------|-------------|
| **Trusted** | 0.8 - 1.0 | Highly reliable, consistently good performance |
| **High** | 0.6 - 0.8 | Very reliable, above-average performance |
| **Medium** | 0.4 - 0.6 | Moderately reliable, acceptable performance |
| **Low** | 0.2 - 0.4 | Less reliable, below-average performance |
| **Unknown** | 0.0 - 0.2 | New or unproven peers |

## Reputation Metrics

### Transaction Verdict Record

All transaction reputation is derived from the `TransactionVerdict` payload. Each verdict is signed by the issuer (one of the transaction parties) and stored in the DHT using the key `H(target_id || "tx-rep")`. On-chain data remains the source of truth—verifiers can always recompute reputation by replaying confirmed transactions even if DHT entries expire.

| Field | Description |
|-------|-------------|
| `target_id` | Peer ID whose reputation is updated. |
| `tx_hash` | Canonical chain reference (block + tx index or transaction hash). |
| `outcome` | `good`, `bad`, or `disputed`. |
| `details` | Optional plain-text metadata (kept ≤ 1 KB). |
| `metric` | Optional label; defaults to `transaction`. Reserved for future metrics. |
| `issued_at` | Unix timestamp in seconds when the verdict was produced. |
| `issuer_id` | Peer ID of the issuer. |
| `issuer_seq_no` | Monotonic counter per issuer to block duplicate verdicts. |
| `issuer_sig` | Signature over all previous fields using the issuer’s transaction key. |
| `tx_receipt` | Optional pointer or embedded proof (e.g., payment-channel close receipt) that links the verdict to an on-chain transaction outcome. |
| `evidence_blobs` | Optional array of detached, signed payloads (handshake promises, challenge transcripts) that support advisory complaints. |

Validation rules:
- Reject any verdict where `issuer_id == target_id`.
- Issuer may publish exactly one verdict per `(target_id, tx_hash)`.
- DHT peers keep verdicts **pending** until `tx_hash` is at least `confirmation_threshold` blocks deep (configurable, default `12`).

### Reliable Penalty Complaints

Reliable penalties apply when a party can anchor their claim to the chain. For example, a seeder can submit a `bad` verdict with a `tx_receipt` showing the downloader never closed the payment channel and funds were reclaimed via timeout. Clients:

1. Verify the `tx_receipt` or referenced settlement on-chain after the required confirmation depth.
2. Ensure the `issuer_seq_no` monotonically increases to prevent replay.
3. Apply the penalty weight immediately once corroborated, since the underlying evidence is immutable. Implementations that track derived success totals reverse any previously credited success for the same transfer so that reliable failures immediately reduce credit toward higher trust levels.

Because these complaints rest on permanent chain data, they are treated as authoritative and can trigger automatic responses (e.g., lower trust buckets, blacklist thresholds) without waiting for additional reports.

### Non-payment Complaint Lifecycle

1. **Handshake** – Before transfer, downloader signs a payment promise (channel ID, maximum confirmation deadline) and shares it with the seeder. The seeder keeps this as an `evidence_blob`.
2. **Transfer** – Seeder delivers data while monitoring the corresponding channel or escrow path for settlement.
3. **Settlement success** – If the downloader closes the channel and payment finalizes on-chain before the deadline, the seeder publishes a `good` verdict pointing at the settlement `tx_hash`.
4. **Settlement failure** – When the deadline passes without closure, the seeder initiates their own channel close on-chain. The resulting `tx_receipt` demonstrates that funds were reclaimed because the downloader did not settle.
5. **Reliable verdict** – Seeder publishes a `bad` verdict referencing the close receipt in `tx_receipt` and attaching the original handshake in `evidence_blobs`.
6. **Verification** – Queriers confirm the channel close on-chain and validate the handshake signatures before applying the penalty.

### Gossip-backed Penalty Signals

Not every misbehaviour is provable on-chain in real time. A seeder may still lodge an advisory complaint by attaching cryptographically signed context—such as the downloader’s handshake promising payment. These `evidence_blobs` form gossip signals:

1. Peers validate signatures to confirm the actors but cannot independently confirm settlement on-chain yet.
2. Clients apply reduced weighting by default, optionally boosting the impact when multiple distinct issuers report the same target with matching context.
3. Gossip penalties never override reliable penalties; they provide early-warning telemetry until the chain produces final evidence.

### Default Scoring Function

Clients aggregate retrieved verdicts using the following weighted average:

```text
score = Σ(weight(event) × value(event)) / Σ(weight(event))

value(good) = 1.0
value(disputed) = 0.5
value(bad) = 0.0
```

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
  - `automatic` — Allow the system to add peers that meet configured thresholds.
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

### What's NOT Tracked

- File content
- Real-world identities
- IP addresses (hidden via relay/proxy if configured)
- Personal information or payment details beyond the chain reference

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
| `confirmation_threshold` | Blocks required beyond `tx_hash` before a verdict is accepted. | 12 |
| `confirmation_timeout` | Max duration to keep a verdict pending before dropping it. | 1 hour |
| `decay_window` | Half-life (seconds) for optional time decay. | Disabled |
| `retention_period` | How long to keep accepted verdicts before pruning. | 90 days |
| `max_verdict_size` | Maximum bytes allowed in `details`. | 1 KB |
| `cache_ttl` | Duration to cache aggregated scores locally. | 10 minutes |
| `blacklist_mode` | How automatic blacklisting behaves: `manual`, `automatic`, or `hybrid`. | `hybrid` |
| `blacklist_auto_enabled` | Enable automatic blacklisting (does not affect manual entries). | true |
| `blacklist_score_threshold` | Score below which a peer becomes eligible for automatic blacklisting (0.0–1.0). | 0.2 |
| `blacklist_bad_verdicts_threshold` | Distinct `bad` verdicts from different issuers required to auto-blacklist a peer. | 3 |
| `blacklist_retention` | How long automatic blacklist entries are retained before re-evaluation or auto-unban. | 30 days |

## Using Reputation Data

### For Downloads

1. **Retrieve seeder scores** through the DHT lookup workflow.
2. **Prefer Trusted peers** for critical payloads.
3. **Monitor transfers** and issue a `bad` verdict if they fail.
4. **Escalate disputes** by publishing `disputed` verdicts and including relevant metadata.

### For Uploads

```typescript
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

- Introduce additional `metric` labels (e.g., `uptime`, `relay`) backed by third-party probes.
- Support encrypted or hashed `details` for privacy-sensitive metadata.
- Provide streaming updates (pub/sub) for near-real-time score changes.
- Experiment with reviewer credibility weighting once multiple metrics exist.
