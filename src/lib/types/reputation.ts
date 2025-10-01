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
  timestamp: Date;
  data: Record<string, any>;
  impact: number; // -1 to 1, negative for bad events, positive for good events
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
