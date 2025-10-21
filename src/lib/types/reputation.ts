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
