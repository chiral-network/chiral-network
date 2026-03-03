/** A host's advertisement published to DHT at key chiral_host_{peer_id} */
export interface HostAdvertisement {
  peerId: string;
  walletAddress: string;
  /** Maximum storage offered in bytes */
  maxStorageBytes: number;
  /** Used storage in bytes */
  usedStorageBytes: number;
  /** Price in wei per MB per day */
  pricePerMbPerDayWei: string;
  /** Minimum deposit in wei */
  minDepositWei: string;
  /** Host uptime percentage (0-100) */
  uptimePercent: number;
  /** Unix timestamp when published */
  publishedAt: number;
  /** Unix timestamp of last heartbeat */
  lastHeartbeatAt: number;
}

export type AgreementStatus =
  | 'proposed'
  | 'accepted'
  | 'rejected'
  | 'active'
  | 'expired'
  | 'cancelled';

/** A signed hosting agreement between client and host, stored at chiral_agreement_{id} */
export interface HostingAgreement {
  agreementId: string;
  /** Client who wants files hosted */
  clientPeerId: string;
  clientWalletAddress: string;
  /** Host who provides storage */
  hostPeerId: string;
  hostWalletAddress: string;
  /** File hashes to be hosted */
  fileHashes: string[];
  /** Total size in bytes of files to host */
  totalSizeBytes: number;
  /** Duration in seconds */
  durationSecs: number;
  /** Price per MB per day in wei */
  pricePerMbPerDayWei: string;
  /** Total cost in wei for the full duration */
  totalCostWei: string;
  /** Deposit amount in wei */
  depositWei: string;
  /** Deposit transaction hash (on-chain) */
  depositTxHash?: string;
  status: AgreementStatus;
  /** Unix timestamp when proposed */
  proposedAt: number;
  respondedAt?: number;
  activatedAt?: number;
  expiresAt?: number;
  /** Client's wallet signature over the agreement body */
  clientSignature?: string;
  /** Host's wallet signature over the agreement body */
  hostSignature?: string;
  /** Peer ID of the party that requested early cancellation (mutual consent required) */
  cancelRequestedBy?: string;
}

/** Host entry for UI display, combining DHT data with reputation */
export interface HostEntry {
  advertisement: HostAdvertisement;
  reputationScore: number;
  availableStorageBytes: number;
  isOnline: boolean;
}

/** Hosting configuration stored in AppSettings */
export interface HostingConfig {
  enabled: boolean;
  /** Max storage to offer in bytes */
  maxStorageBytes: number;
  /** Price per MB per day in wei */
  pricePerMbPerDayWei: string;
  /** Minimum deposit required in wei */
  minDepositWei: string;
}
