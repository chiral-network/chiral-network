# **Proof-of-Delivery Receipts (PoDR)**

## **Purpose**
- Provide verifiable attribution of which peers delivered which bytes.
- Improve reputation accuracy with cryptographic proof instead of heuristics.
- Enhance diagnostics and dispute resolution with auditable records.
- Lay groundwork for future, decoupled settlement without changing transfer protocols.

---

## **Overview**
- Seeders sign per-chunk receipts; downloaders verify and store them.
- Works across transport types by using a lightweight receipt backchannel:
  - libp2p request/response (`/chiral/receipt/1.0.0`) for private P2P and for public protocol cases (HTTP/FTP) tied to the same peer ID.
  - Optional WebRTC datachannel messages when the peer connection is WebRTC.
- Produces a file-level bundle that summarizes verified contribution per peer.

### Receipt Workflow
1. Downloader verifies chunk integrity (hash/merkle already present).
2. Downloader sends a receipt challenge: `(fileHash, chunkIndex, chunkHash, nonce, ts)`.
3. Seeder replies with signature over `CHIRAL_PODR_V1 || chunkHash || nonce || seederPeerId || ts`.
4. Downloader verifies signature using the seeder's libp2p identity public key and stores a `ChunkReceipt`.
5. After completion, compose a `FileReceiptBundle` (Merkle root over receipts) and persist alongside download metadata.

---

## **Blockchain vs PoDR**
- Short answer: Yes—PoDR still matters even if payments are recorded on-chain.
- Attribution vs accounting: On-chain entries record payments/settlement, not which peer delivered which bytes. PoDR provides verifiable attribution per peer and per chunk.
- Cost and scale: Per-chunk or per-transfer proofs on-chain are too expensive. Keep receipts off-chain; commit a Merkle root only when needed (e.g., at settlement or in disputes).
- Real-time operations: Peer selection, reputation updates, and failover need immediate local verification; waiting for chain finality is too slow for routing decisions.
- Dispute resolution: Cryptographic receipts are portable evidence for off-chain resolution or selective on-chain commitment (bundle root).
- Privacy: Delivery metadata (timing, peers, chunk maps) may be sensitive; PoDR can stay local and shared only when required.
- Multi-source accuracy: Precise per-peer contribution in swarmed downloads is hard to infer from payments alone; PoDR preserves exact attribution.

### When PoDR might be redundant
- If the payment protocol already enforces per-chunk, in-band proofs and records verifiable artifacts on-chain (or in a rollup) that are sufficient for attribution and reputation.
- If coarse, payer-declared attribution is acceptable and detailed verifiable metrics are not needed.

### Recommended approach for Chiral
- Keep PoDR off-chain for per-chunk proofs and aggregate to a file-level Merkle root.
- Optionally commit the bundle root on-chain at settlement time (or only for disputes/riskier flows).
- Use PoDR for local reputation/analytics; keep the chain for settlement/escrow and long-term accountability.

#### Operational Model: PoDR + Chain
- Local (PoDR-backed, real-time):
  - Reputation source-of-truth for routing: score peers by verified bytes, timeliness (latency), failure rate, and recency (decay old receipts).
  - Sybil/whitewashing damping: require minimum age and minimum verified-bytes thresholds before a peer influences selection; weight identities with sustained receipts higher.
  - Transport-agnostic attribution: normalize receipts from HTTP/FTP/WebRTC/private protocols through the same PoDR verifier.
  - Anomaly detection: flag repeated invalid signatures, duplicate chunk delivery claims, or abnormal receipt rates.
  - Privacy: keep raw receipts local; expose only derived scores or aggregates to the UI.

- On-chain (optional, slow path):
  - Settlement/escrow: payments reference a file-level PoDR bundle root (Merkle) rather than per-chunk artifacts.
  - Accountability anchor: periodically publish (or upon dispute) the bundle root with minimal metadata to create an immutable audit point.
  - Disputes: exchange the receipt bundle off-chain; if unresolved, submit root + minimal inclusion proofs to the chain. Most cases remain off-chain to save gas.
  - Cost control: batch roots, prefer L2, and publish only for high-value transfers or when policy requires.

#### Why PoDR beats blockchain logs for local reputation
- Timeliness: Local routing needs sub-second updates; chain finality (seconds–minutes) is too slow to influence which peer you pick next.
- Granularity: Reputation benefits from per-chunk signals (latency, retries, corruption) that are never captured on-chain; payments are coarse.
- Cost: Recording per-chunk delivery on-chain is prohibitive; local PoDR is free and continuous.
- Coverage: Not every transfer settles immediately (or at all); local reputation must work during offline/credit periods and across chains/L2s.
- Privacy: Publishing delivery relationships on-chain harms user privacy; PoDR can remain local, sharing only aggregates if desired.
- Identity binding: PoDR binds delivery to a peer's libp2p identity (even over HTTP/FTP/WebRTC); on-chain entries often lack a reliable mapping to the actual seeder.
- Robustness vs gaming: Self-payments or synthetic on-chain activity can inflate appearances; signed receipts tied to observed delivery (plus recency decay and thresholds) are harder to game locally.

---

## **Protocol**
### DHT Capability
- Provider metadata advertises `supports_receipts: true|false`.
- Downloader prefers receipt-capable peers, but remains backward compatible.

### libp2p Request/Response
- Protocol ID: `/chiral/receipt/1.0.0` (JSON payloads).
- Request:
```json
{
  "type": "CHUNK_RECEIPT_REQ",
  "file_hash": "<sha256>",
  "chunk_index": 123,
  "chunk_hash": "<sha256>",
  "nonce": "<32-byte-hex>",
  "ts": 1730000000
}
```
- Response:
```json
{
  "type": "CHUNK_RECEIPT_RES",
  "ok": true,
  "sig": "<ed25519-signature-hex>",
  "seeder_peer_id": "12D3Koo...",
  "ts": 1730000001,
  "err": null
}
```
- Domain tag: `CHIRAL_PODR_V1` prepended for signature domain separation.

### WebRTC (Optional)
- Mirror the same `*_REQ/RES` messages over datachannel (control lane or labeled channel).

---

## **Data Model**
```typescript
type ChunkReceipt = {
  fileHash: string;
  chunkIndex: number;
  chunkHash: string;
  nonce: string;      // hex
  seederPeerId: string;
  ts: number;         // unix ms
  sig: string;        // hex (ed25519)
};

type FileReceiptBundle = {
  fileHash: string;
  totalBytes: number;
  chunkReceipts: ChunkReceipt[];
  merkleRoot: string; // over serialized receipts
  createdAt: number;
};
```

---

## **Implementation Plan**
- Phase 0 — Feature Flags & Defaults
  - Add Settings/CLI toggle: "Collect proof-of-delivery receipts" (default ON for desktop builds, OFF for web builds if needed).
  - Add Settings/CLI toggle: "Publish receipt bundle root on-chain" (default OFF).

- Phase 1 — Off-chain PoDR Core
  - Types & capability: add TS types (`src/lib/types/receipts.ts`); extend provider metadata in backend (`src-tauri/src/dht.rs`) with `supports_receipts` and surface in frontend (`src/lib/dht.ts`).
  - libp2p: add request_response behaviour `/chiral/receipt/1.0.0` in Rust; sign `(chunk_hash || nonce || seeder_peer_id || ts)` when seeding that chunk.
  - File transfer hooks: trigger receipt request after chunk verification (`src-tauri/src/download_source.rs`, `src-tauri/src/manager.rs`).
  - WebRTC path: add helpers in `src/lib/services/webrtcService.ts` to exchange receipt messages after verification for WebRTC peers.

- Phase 2 — Storage, UI, Reputation
  - Persistence: add `src/lib/services/receiptsStore.ts` to store `FileReceiptBundle` per download; expose export as JSON.
  - UI: show per-peer verified bytes and "Export Receipts (JSON)" in download details.
  - Reputation: update `src/lib/reputationStore.ts` to weight by verified bytes when available; fall back to existing metrics otherwise.

- Phase 3 — Optional On-Chain Commitment (Hybrid)
  - Add an optional settlement hook to publish the file-level bundle Merkle root and minimal metadata (fileHash, totalBytes, receiptCount) via existing chain modules (`src-tauri/src/ethereum.rs`, `src/lib/services/paymentService.ts`).
  - Keep OFF by default; gated by the new toggle and environment checks. Include gas estimation and user confirmation.

- Phase 4 — Dispute & Verification Tools
  - Add a lightweight verifier CLI/command (Tauri command) to recompute Merkle roots from receipt JSON and verify signatures; used for audits/disputes without requiring on-chain data.

- Phase 5 — Protections & Policy
  - Enforce TTL/skew checks, token-bucket rate limiting, and payload caps in the receipt handler.
  - Add retention policy (configurable days/size cap) and a "Delete receipts" control in Settings.

- Docs
  - Add brief sections in `docs/network-protocol.md` and `docs/file-sharing.md`; link to this document and describe the hybrid strategy (off-chain by default, optional on-chain root).

---

## **Security & Abuse Controls**
- Nonce + TTL: 32-byte random nonce from downloader; strict time window/skew checks.
- Rate limiting: Per-connection and per-peer token buckets for receipt requests.
- Size limits: Enforce payload caps to prevent abuse.
- Keys: Use existing libp2p Ed25519 identity for signing/verification; no new key material.

---

## **Backward Compatibility**
- If `supports_receipts` is absent/false, proceed without PoDR; UI labels verified bytes as unavailable.
- Downloads and existing flows remain unchanged when receipts are not supported.

---

## **Acceptance Criteria**
- Receipts generated and verified for PoDR-capable peers; bundle persisted with valid Merkle root.
- UI displays per-peer verified contribution for multi-source downloads.
- Reputation reflects verified bytes when present.
- Protections in place: TTL, rate limiting, payload caps.
- No regressions when peers do not support receipts.

---

## **Test Plan**
- TS unit: receipt serialize/deserialize; Merkle root computation.
- Rust unit: sign/verify; codec; TTL/replay checks.
- Integration: multi-peer download, attribution matches expected; negative cases (bad sig, expired ts, wrong nonce).
- Optional E2E: WebRTC path round-trip using the signaling server.

---

## **Open Questions**
- Include downloader identity in receipt for non-repudiation vs privacy trade-off?
- Batch multiple chunk indices per request to reduce overhead?
- Default retention and export format/versioning for receipts?

---

## **References (Code Touchpoints)**
- DHT & provider metadata: `src-tauri/src/dht.rs`, `src/lib/dht.ts`
- File transfer pipeline: `src-tauri/src/download_source.rs`, `src-tauri/src/manager.rs`
- WebRTC: `src/lib/services/webrtcService.ts`, signaling in `src/lib/services/signalingService.ts`
- Reputation: `src/lib/reputationStore.ts`
- UI: `src/pages/Download.svelte`, `src/lib/components/download/*`

