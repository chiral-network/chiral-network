export interface PeerReputation {
  peerId: string;
  trustLevel: TrustLevel;
  score: number;
  totalInteractions: number;
  successfulInteractions: number;
  lastSeen: Date;
  reputationHistory: ReputationEvent[];
  metrics: PeerMetrics;
}

export enum TrustLevel {
  Unknown = 'Unknown',
  Low = 'Low',
  Medium = 'Medium',
  High = 'High',
  Trusted = 'Trusted'
}

// Transaction verdict outcomes
export type VerdictOutcome = 'good' | 'bad' | 'disputed';

// Transaction verdict for reputation system
export interface TransactionVerdict {
  target_id: string;           // Peer being rated
  tx_hash: string | null;      // Blockchain tx hash (null for non-payment complaints)
  outcome: VerdictOutcome;     // good, bad, or disputed
  details?: string;            // Optional details (max 1KB)
  metric?: string;             // Metric type (default: "transaction")
  issued_at: number;           // Unix timestamp
  issuer_id: string;           // Peer who issued verdict
  issuer_seq_no: number;       // Monotonic counter per issuer
  issuer_sig: string;          // Signature over verdict
  tx_receipt?: string;         // Optional blockchain receipt pointer
  evidence_blobs?: string[];   // Evidence (signed messages, proofs, logs)
}

// Signed transaction message for handshake
export interface SignedTransactionMessage {
  from: string;                // Downloader's wallet address
  to: string;                  // Seeder's wallet address  
  amount: number;              // Payment amount in Chiral
  file_hash: string;           // Target file identifier
  nonce: string;               // Unique identifier to prevent replay
  deadline: number;            // Unix timestamp deadline
  downloader_signature: string; // Cryptographic signature
}

// Evidence types for complaints
export interface NonPaymentEvidence {
  signed_transaction_message: SignedTransactionMessage;
  delivery_proof: ChunkManifest;
  transfer_completion_log: TransferLog;
  protocol_logs?: string;
}

export interface ChunkManifest {
  chunks_sent: number;
  total_chunks: number;
  chunk_hashes: string[];
  completion_timestamp: number;
}

export interface TransferLog {
  started_at: number;
  completed_at: number;
  bytes_transferred: number;
  connection_id: string;
}

export interface ReputationEvent {
  id: string;
  type: EventType;
  peerId: string;
  raterPeerId: string; // Node that created this event
  timestamp: Date;
  data: Record<string, any>;
  impact: number; // -1 to 1, negative for bad events, positive for good events
  signature: string; // Ed25519 signature for verification
  epoch?: number; // Epoch this event belongs to
}

export enum EventType {
  FileTransferSuccess = 'FileTransferSuccess',
  FileTransferFailure = 'FileTransferFailure',
  PaymentSuccess = 'PaymentSuccess',
  PaymentFailure = 'PaymentFailure',
  ConnectionEstablished = 'ConnectionEstablished',
  ConnectionLost = 'ConnectionLost',
  DhtQueryAnswered = 'DhtQueryAnswered',
  StorageOffered = 'StorageOffered',
  MaliciousBehaviorReport = 'MaliciousBehaviorReport',
  FileShared = 'FileShared'
}

export interface PeerMetrics {
  averageLatency: number;
  bandwidth: number;
  uptime: number;
  storageOffered: number;
  filesShared: number;
  encryptionSupported: boolean;
}

export interface ReputationAnalytics {
  totalPeers: number;
  trustedPeers: number;
  averageScore: number;
  topPerformers: PeerReputation[];
  recentEvents: ReputationEvent[];
  trustLevelDistribution: Record<TrustLevel, number>;
}

// New types for Merkle tree and epoch management
export interface ReputationEpoch {
  epochId: number;
  merkleRoot: string;
  timestamp: number;
  blockNumber?: number;
  eventCount: number;
  submitter: string;
}

export interface MerkleProof {
  leafIndex: number;
  proofHashes: string[];
  totalLeaves: number;
}

export interface ReputationEventData {
  id: string;
  peerId: string;
  raterPeerId: string;
  type: EventType;
  timestamp: number;
  data: Record<string, any>;
  impact: number;
}

// Blacklist entry for reputation system
export interface BlacklistEntry {
  peerId: string;
  reason: string;
  timestamp: number;
  isAutomatic: boolean;
  expiresAt?: number;
}

// Reputation configuration
export interface ReputationConfig {
  minScoreForTransfer: number;    // Minimum reputation to allow transfers
  cacheTimeoutMs: number;         // How long to cache reputation scores
  verdictTTL: number;             // Time-to-live for verdicts in seconds
  autoBlacklistThreshold: number; // Score below which to auto-blacklist
}

// Default configuration
export const DEFAULT_REPUTATION_CONFIG: ReputationConfig = {
  minScoreForTransfer: 0.3,
  cacheTimeoutMs: 300000, // 5 minutes
  verdictTTL: 2592000,    // 30 days
  autoBlacklistThreshold: 0.1
};
