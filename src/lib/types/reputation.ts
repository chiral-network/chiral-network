// TypeScript interfaces for Blockchain Reputation System
// This file defines the data structures used in the frontend

export interface ReputationEvent {
  peerId: string;
  eventType: ReputationEventType;
  timestamp: number;
  metadata: Record<string, string>;
  signature: string;
}

export enum ReputationEventType {
  FileTransferSuccess = 'FileTransferSuccess',
  FileTransferFailure = 'FileTransferFailure',
  ConnectionEstablished = 'ConnectionEstablished',
  ConnectionLost = 'ConnectionLost',
  MaliciousBehavior = 'MaliciousBehavior',
}

export interface ReputationEpoch {
  epochId: number;
  startTime: number;
  endTime: number;
  events: ReputationEvent[];
  merkleRoot: string;
  signature: string;
  blockchainTxHash?: string;
}

export interface BlockchainReputationData {
  peerId: string;
  score: number;
  verificationStatus: VerificationStatus;
  epochCount: number;
  totalEvents: number;
  lastVerified: number;
  merkleProof?: string;
}

export enum VerificationStatus {
  Verified = 'Verified',
  Pending = 'Pending',
  Failed = 'Failed',
  NotAvailable = 'NotAvailable',
}

export interface BlockchainReputationEpoch {
  epochId: number;
  timestamp: number;
  eventCount: number;
  merkleRoot: string;
  txHash: string;
  verificationStatus: VerificationStatus;
}

export interface BlockchainReputationAnalytics {
  totalPeers: number;
  verifiedPeers: number;
  averageBlockchainScore: number;
  verificationSuccessRate: number;
  blockchainConnectivityStatus: string;
  recentEpochs: BlockchainReputationEpoch[];
  reputationDistribution: {
    excellent: number; // 0.8-1.0
    good: number;       // 0.6-0.8
    fair: number;       // 0.4-0.6
    poor: number;       // 0.2-0.4
    bad: number;        // 0.0-0.2
  };
}

// Existing interfaces (for reference)
export enum TrustLevel {
  High = 'High',
  Medium = 'Medium',
  Low = 'Low',
  Unknown = 'Unknown',
}

export interface PeerReputation {
  peerId: string;
  score: number;
  trustLevel: TrustLevel;
  interactions: number;
  lastSeen: number;
  uptime: number;
  encryptionSupported: boolean;
  blockchainReputation?: BlockchainReputationData;
}

export interface ReputationAnalytics {
  totalPeers: number;
  averageScore: number;
  trustLevelDistribution: Record<TrustLevel, number>;
  topPerformers: PeerReputation[];
  recentActivity: Array<{
    timestamp: number;
    event: string;
    peerId: string;
  }>;
}