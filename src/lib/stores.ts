import { writable, derived } from "svelte/store";
import { normalizeRegion, GEO_REGIONS, UNKNOWN_REGION_ID } from "$lib/geo";

export interface FileItem {
  id: string;
  name: string;
  hash: string;
  size: number;
  status:
    | "downloading"
    | "paused"
    | "completed"
    | "failed"
    | "uploaded"
    | "queued"
    | "seeding"
    | "canceled";
  progress?: number;
  uploadDate?: Date;
  owner?: string;
  description?: string;
  seeders?: number;
  seederAddresses?: string[];
  leechers?: number;
  encrypted?: boolean;
  priority?: "low" | "normal" | "high";
  downloadSpeed?: number;
  uploadSpeed?: number;
  timeRemaining?: number;
  visualOrder?: number; // For maintaining user's intended visual order
  downloadPath?: string; // Path where the file was downloaded
  speed?: string; // Download/upload speed display
  eta?: string; // Estimated time remaining display
  isEncrypted?: boolean;
  manifest?: any;
  path?: string;
  cids?: string[];
  downloadedChunks?: number[];
  totalChunks?: number;
  downloadStartTime?: number;
  price?: number; // Price in Chiral for this file
}

export interface ProxyNode {
  id: string;
  address: string;
  status: "online" | "offline" | "connecting";
  bandwidth: number;
  latency: number;
  region: string;
  reputation?: number;
  uptime?: number;
  price?: number;
  totalProxied?: number;
}

export interface WalletInfo {
  address: string;
  balance: number;
  pendingTransactions: number;
  stakedAmount?: number;
  miningRewards?: number;
  reputation?: number;
  totalEarned?: number;
  totalSpent?: number;
  totalReceived?: number;
}

export interface ETCAccount {
  address: string;
  private_key: string;
}

export interface PeerInfo {
  id: string;
  address: string;
  nickname?: string;
  status: "online" | "offline" | "away";
  reputation: number;
  sharedFiles: number;
  totalSize: number;
  joinDate: Date;
  lastSeen: Date;
  location?: string;
}

export interface PeerGeoRegionStat {
  regionId: string;
  label: string;
  count: number;
  percentage: number;
  color: string;
  peers: PeerInfo[];
}

export interface PeerGeoDistribution {
  totalPeers: number;
  regions: PeerGeoRegionStat[];
  dominantRegionId: string | null;
  generatedAt: number;
}

export const suspiciousActivity = writable<
  {
    type: string;
    description: string;
    date: string;
    severity: "low" | "medium" | "high";
  }[]
>([]);

export interface NetworkStats {
  totalPeers: number;
  onlinePeers: number;
  totalFiles: number;
  networkSize: number;
  avgDownloadSpeed: number;
  avgUploadSpeed: number;
  totalTransactions: number;
}

export interface Transaction {
  id: number;
  type: "sent" | "received" | "mining";
  amount: number;
  to?: string;
  from?: string;
  txHash?: string;
  hash?: string; // Transaction hash (primary identifier)
  date: Date;
  description: string;
  status: "submitted" | "pending" | "success" | "failed"; // Match API statuses
  transaction_hash?: string;
  gas_used?: number;
  gas_price?: number; // in Wei
  confirmations?: number;
  block_number?: number;
  nonce?: number;
  fee?: number; // Total fee in Wei
  timestamp?: number;
  error_message?: string;
}

export interface TransactionPaginationState {
  accountAddress: string | null; // The account this pagination state belongs to
  oldestBlockScanned: number | null; // The oldest block we've scanned so far
  isLoading: boolean; // Whether we're currently loading more transactions
  hasMore: boolean; // Whether there are more transactions to load
  batchSize: number; // Number of blocks to scan per batch (default: 5000)
}

export interface MiningPaginationState {
  accountAddress: string | null; // The account this pagination state belongs to
  oldestBlockScanned: number | null; // The oldest block we've scanned for mining rewards
  isLoading: boolean; // Whether we're currently loading more mining rewards
  hasMore: boolean; // Whether there are more mining rewards to load
  batchSize: number; // Number of blocks to scan per batch (default: 5000)
}

export interface BlacklistEntry {
  chiral_address: string;
  reason: string;
  timestamp: Date;
}

// Sample dummy data
const dummyFiles: FileItem[] = [
  {
    id: "0",
    name: "Video.mp4",
    hash: "QmZ4tDuvesekqMF",
    size: 50331648,
    status: "paused",
    progress: 30,
    visualOrder: 1,
  },
  {
    id: "1",
    name: "Document.pdf",
    hash: "QmZ4tDuvesekqMD",
    size: 2048576,
    status: "completed",
    progress: 100,
    visualOrder: 2,
  },
];

const dummyWallet: WalletInfo = {
  address: "",
  balance: 0,
  pendingTransactions: 0,
};

// Additional dummy data
const dummyPeers: PeerInfo[] = [];

const blacklistedPeers: BlacklistEntry[] = [
  {
    chiral_address: "0x702f05Cc6634C599f1293b844Bc9e759ef049891",
    reason: "Unfufilled requests",
    timestamp: new Date("2024-05-01T10:00:00Z"),
  },
];

const dummyNetworkStats: NetworkStats = {
  totalPeers: 1247,
  onlinePeers: 892,
  totalFiles: 45678,
  networkSize: 125899906842624, // ~125TB
  avgDownloadSpeed: 12.5, // MB/s
  avgUploadSpeed: 8.3, // MB/s
  totalTransactions: 98765,
};

const dummyTransactions: Transaction[] = [
  {
    id: 1,
    type: "received",
    amount: 50.5,
    from: "0x8765...4321",
    date: new Date("2024-03-15"),
    description: "Storage reward",
    status: "success",
  },
  {
    id: 2,
    type: "sent",
    amount: 10.25,
    to: "0x1234...5678",
    date: new Date("2024-03-14"),
    description: "Proxy service",
    status: "success",
  },
  {
    id: 3,
    type: "received",
    amount: 100,
    from: "0xabcd...ef12",
    date: new Date("2024-03-13"),
    description: "Upload reward",
    status: "success",
  },
  {
    id: 4,
    type: "sent",
    amount: 5.5,
    to: "0x9876...5432",
    date: new Date("2024-03-12"),
    description: "File download",
    status: "success",
  },
];

// Stores
export const files = writable<FileItem[]>(dummyFiles);
export const wallet = writable<WalletInfo>(dummyWallet);
export const activeDownloads = writable<number>(1);
export const transactions = writable<Transaction[]>(dummyTransactions);

// Load pagination state from localStorage
const storedPagination = typeof window !== 'undefined'
  ? localStorage.getItem('transactionPagination')
  : null;

const initialPaginationState: TransactionPaginationState = storedPagination
  ? JSON.parse(storedPagination)
  : {
      accountAddress: null,
      oldestBlockScanned: null,
      isLoading: false,
      hasMore: true,
      batchSize: 5000,
    };

export const transactionPagination = writable<TransactionPaginationState>(initialPaginationState);

// Persist pagination state to localStorage
if (typeof window !== 'undefined') {
  transactionPagination.subscribe((state) => {
    localStorage.setItem('transactionPagination', JSON.stringify({
      accountAddress: state.accountAddress,
      oldestBlockScanned: state.oldestBlockScanned,
      hasMore: state.hasMore,
      batchSize: state.batchSize,
      // Don't persist isLoading state
    }));
  });
}

// Load mining pagination state from localStorage
const storedMiningPagination = typeof window !== 'undefined'
  ? localStorage.getItem('miningPagination')
  : null;

const initialMiningPaginationState: MiningPaginationState = storedMiningPagination
  ? JSON.parse(storedMiningPagination)
  : {
      accountAddress: null,
      oldestBlockScanned: null,
      isLoading: false,
      hasMore: true,
      batchSize: 5000,
    };

export const miningPagination = writable<MiningPaginationState>(initialMiningPaginationState);

// Persist mining pagination state to localStorage
if (typeof window !== 'undefined') {
  miningPagination.subscribe((state) => {
    localStorage.setItem('miningPagination', JSON.stringify({
      accountAddress: state.accountAddress,
      oldestBlockScanned: state.oldestBlockScanned,
      hasMore: state.hasMore,
      batchSize: state.batchSize,
      // Don't persist isLoading state
    }));
  });
}

// Import real network status
import { networkStatus } from "./services/networkService";
export { networkStatus };

export const peers = writable<PeerInfo[]>(dummyPeers);

export const peerGeoDistribution = derived(
  peers,
  ($peers): PeerGeoDistribution => {
    const totals = new Map<string, PeerGeoRegionStat>();

    for (const region of GEO_REGIONS) {
      totals.set(region.id, {
        regionId: region.id,
        label: region.label,
        count: 0,
        percentage: 0,
        color: region.color,
        peers: [],
      });
    }

    for (const peer of $peers) {
      const region = normalizeRegion(peer.location);
      const bucket = totals.get(region.id);
      if (!bucket) {
        continue;
      }

      bucket.count += 1;
      bucket.peers.push(peer);
    }

    const totalPeers = $peers.length;
    for (const bucket of totals.values()) {
      bucket.percentage =
        totalPeers === 0
          ? 0
          : Math.round((bucket.count / totalPeers) * 1000) / 10;
    }

    const buckets = Array.from(totals.values());
    buckets.sort((a, b) => {
      if (a.regionId === UNKNOWN_REGION_ID && b.regionId !== UNKNOWN_REGION_ID)
        return 1;
      if (b.regionId === UNKNOWN_REGION_ID && a.regionId !== UNKNOWN_REGION_ID)
        return -1;
      if (b.count === a.count) {
        return a.label.localeCompare(b.label);
      }
      return b.count - a.count;
    });

    const dominantRegion = buckets.find(
      (bucket) => bucket.regionId !== UNKNOWN_REGION_ID && bucket.count > 0
    );

    return {
      totalPeers,
      regions: buckets,
      dominantRegionId: dominantRegion ? dominantRegion.regionId : null,
      generatedAt: Date.now(),
    };
  }
);

export const networkStats = writable<NetworkStats>(dummyNetworkStats);
export const downloadQueue = writable<FileItem[]>([]);
export const userLocation = writable<string>("US-East");
export const etcAccount = writable<ETCAccount | null>(null);
export const blacklist = writable<BlacklistEntry[]>(blacklistedPeers);

interface RecentBlock {
  id: string;
  hash: string;
  reward: number;
  timestamp: Date;
  difficulty: number;
  nonce: number;
}

export interface MiningHistoryPoint {
  timestamp: number;
  hashRate: number;
  power: number;
}

// Mining state
export interface MiningState {
  isMining: boolean;
  hashRate: string;
  totalRewards: number;
  blocksFound: number;
  activeThreads: number;
  minerIntensity: number;
  selectedPool: string;
  sessionStartTime?: number; // Track mining session start time for persistence
  recentBlocks?: RecentBlock[]; // Store recent blocks found
  miningHistory?: MiningHistoryPoint[]; // Store hash rate history for charts
}

export const miningState = writable<MiningState>({
  isMining: false,
  hashRate: "0 H/s",
  totalRewards: 0,
  blocksFound: 0,
  activeThreads: 1,
  minerIntensity: 50,
  selectedPool: "solo",
  sessionStartTime: undefined,
  recentBlocks: [],
  miningHistory: [],
});

export const miningProgress = writable({ cumulative: 0, lastBlock: 0 });

// Accurate totals from full blockchain scan
export interface AccurateTotals {
  blocksMined: number;
  totalReceived: number;
  totalSent: number;
}

export interface AccurateTotalsProgress {
  currentBlock: number;
  totalBlocks: number;
  percentage: number;
}

export const accurateTotals = writable<AccurateTotals | null>(null);
export const isCalculatingAccurateTotals = writable<boolean>(false);
export const accurateTotalsProgress = writable<AccurateTotalsProgress | null>(null);

// Calculate total mined from loaded mining reward transactions (partial - based on loaded data)
export const totalEarned = derived(transactions, ($txs) =>
  $txs
    .filter((tx) => tx.type === "mining")
    .reduce((sum, tx) => sum + tx.amount, 0)
);

export const totalSpent = derived(transactions, ($txs) =>
  $txs
    .filter((tx) => tx.type === "sent")
    .reduce((sum, tx) => sum + tx.amount, 0)
);

export const totalReceived = derived(transactions, ($txs) =>
  $txs
    .filter((tx) => tx.type === "received")
    .reduce((sum, tx) => sum + tx.amount, 0)
);

// Store for active P2P transfers and WebRTC sessions
export interface ActiveTransfer {
  fileId: string;
  transferId: string;
  type: "p2p" | "webrtc";
}

export const activeTransfers = writable<Map<string, ActiveTransfer>>(new Map());

// Interface for Bandwidth Schedule Entry
export interface BandwidthScheduleEntry {
  id: string;
  name: string;
  startTime: string; // Format: "HH:MM" (24-hour)
  endTime: string; // Format: "HH:MM" (24-hour)
  daysOfWeek: number[]; // 0-6, where 0 = Sunday
  uploadLimit: number; // KB/s, 0 = unlimited
  downloadLimit: number; // KB/s, 0 = unlimited
  enabled: boolean;
}

export interface ActiveBandwidthLimits {
  uploadLimitKbps: number;
  downloadLimitKbps: number;
  source: "default" | "schedule";
  scheduleId?: string;
  scheduleName?: string;
  nextChangeAt?: number;
}

const defaultActiveBandwidthLimits: ActiveBandwidthLimits = {
  uploadLimitKbps: 0,
  downloadLimitKbps: 0,
  source: "default",
  nextChangeAt: undefined,
  scheduleId: undefined,
  scheduleName: undefined,
};

// Interface for Application Settings
export interface AppSettings {
  storagePath: string;
  maxStorageSize: number; // GB
  autoCleanup: boolean;
  cleanupThreshold: number; // %
  maxConnections: number;
  uploadBandwidth: number; // 0 = unlimited
  downloadBandwidth: number; // 0 = unlimited
  port: number;
  enableUPnP: boolean;
  enableNAT: boolean;
  userLocation: string;
  enableProxy: boolean; // For SOCKS5 feature
  proxyAddress: string; // For SOCKS5 feature
  ipPrivacyMode: "off" | "prefer" | "strict";
  trustedProxyRelays: string[];
  disableDirectNatTraversal: boolean;
  enableAutonat: boolean; // AutoNAT reachability detection
  autonatProbeInterval: number; // Seconds between AutoNAT probes
  autonatServers: string[]; // Custom AutoNAT server multiaddrs
  enableAutorelay: boolean; // Circuit Relay v2 with AutoRelay (renamed from enableAutoRelay)
  preferredRelays: string[]; // Preferred relay node multiaddrs
  enableRelayServer: boolean; // Act as a relay server for other peers
  relayServerAlias: string; // Public alias/name for your relay server (appears in logs and bootstrapping)
  anonymousMode: boolean;
  shareAnalytics: boolean;
  enableWalletAutoLock: boolean;
  enableNotifications: boolean;
  notifyOnComplete: boolean;
  notifyOnError: boolean;
  notifyOnBandwidthCap: boolean;
  notifyOnBandwidthCapDesktop: boolean;
  soundAlerts: boolean;
  enableIPFS: boolean;
  chunkSize: number; // KB
  cacheSize: number; // MB
  logLevel: string;
  autoUpdate: boolean;
  enableBandwidthScheduling: boolean;
  bandwidthSchedules: BandwidthScheduleEntry[];
  monthlyUploadCapGb: number; // 0 = no cap
  monthlyDownloadCapGb: number; // 0 = no cap
  capWarningThresholds: number[]; // Percentages, e.g. [75, 90]
  enableFileLogging: boolean; // Enable file-based logging
  maxLogSizeMB: number; // Maximum size of a single log file in MB
  pricePerMb: number; // Price per MB in Chiral (e.g., 0.001)
  customBootstrapNodes: string[]; // Custom bootstrap nodes for DHT (leave empty to use defaults)
  autoStartDHT: boolean; // Whether to automatically start DHT on app launch
  selectedProtocol: "WebRTC" | "Bitswap" | "BitTorrent" | null; // Protocol selected for file uploads
}

// Export the settings store
// We initialize with a safe default structure. Settings.svelte will load/persist the actual state.
export const settings = writable<AppSettings>({
  storagePath: "~/Chiral-Network-Storage",
  maxStorageSize: 100,
  autoCleanup: true,
  cleanupThreshold: 90,
  maxConnections: 50,
  uploadBandwidth: 0,
  downloadBandwidth: 0,
  port: 30303,
  enableUPnP: true,
  enableNAT: true,
  userLocation: "US-East",
  enableProxy: true, // Defaulting to enabled for SOCKS5 feature
  proxyAddress: "127.0.0.1:9050", // Default Tor SOCKS address
  ipPrivacyMode: "off",
  trustedProxyRelays: [],
  disableDirectNatTraversal: false,
  enableAutonat: false, // Disabled by default - enable if you need NAT detection
  autonatProbeInterval: 30, // 30 seconds default
  autonatServers: [], // Use bootstrap nodes by default
  enableAutorelay: false, // Disabled by default - enable if you need relay connections
  preferredRelays: [], // Use bootstrap nodes as relays by default
  enableRelayServer: false, // Disabled by default - enable to help relay traffic for others
  relayServerAlias: "", // Empty by default - user can set a friendly name
  anonymousMode: false,
  shareAnalytics: true,
  enableWalletAutoLock: false,
  enableNotifications: true,
  notifyOnComplete: true,
  notifyOnError: true,
  notifyOnBandwidthCap: true,
  notifyOnBandwidthCapDesktop: false,
  soundAlerts: false,
  enableIPFS: false,
  chunkSize: 256,
  cacheSize: 1024,
  logLevel: "info",
  autoUpdate: true,
  enableBandwidthScheduling: false,
  bandwidthSchedules: [],
  monthlyUploadCapGb: 0,
  monthlyDownloadCapGb: 0,
  capWarningThresholds: [75, 90],
  enableFileLogging: false, // Disabled by default
  maxLogSizeMB: 10, // 10 MB per log file by default
  pricePerMb: 0.001, // Default price: 0.001, until ability to set pricePerMb is there, then change to 0.001 Chiral per MB
  customBootstrapNodes: [], // Empty by default - use hardcoded bootstrap nodes
  autoStartDHT: false, // Don't auto-start DHT by default
  selectedProtocol: "Bitswap" as "WebRTC" | "Bitswap" | "BitTorrent", // Default to Bitswap
});

export const activeBandwidthLimits = writable<ActiveBandwidthLimits>(
  defaultActiveBandwidthLimits
);

// Transaction polling functionality
import {
  pollTransactionStatus,
  type TransactionStatus as ApiTransactionStatus,
} from "./services/transactionService";

// Active polling tracker
const activePollingTasks = new Map<string, boolean>();

/**
 * Add a transaction and start polling for status updates
 */
export async function addTransactionWithPolling(
  transaction: Transaction
): Promise<void> {
  if (!transaction.transaction_hash) {
    throw new Error("Transaction must have a hash for polling");
  }

  const txHash = transaction.transaction_hash;

  // Prevent duplicate polling
  if (activePollingTasks.has(txHash)) {
    console.warn(`Already polling transaction ${txHash}`);
    return;
  }

  // Add to store immediately with 'submitted' status
  transactions.update((txs) => [transaction, ...txs]);

  // Mark as actively polling
  activePollingTasks.set(txHash, true);

  try {
    // Start polling with status updates
    await pollTransactionStatus(
      txHash,
      (status: ApiTransactionStatus) => {
        // Update transaction in store on each status change
        transactions.update((txs) =>
          txs.map((tx) => {
            if (tx.transaction_hash === txHash) {
              return {
                ...tx,
                status:
                  status.status === "success"
                    ? "success"
                    : status.status === "failed"
                      ? "failed"
                      : status.status === "pending"
                        ? "pending"
                        : "submitted",
                confirmations: status.confirmations || 0,
                block_number: status.block_number || undefined,
                gas_used: status.gas_used || undefined,
                error_message: status.error_message || undefined,
              };
            }
            return tx;
          })
        );
      },
      120, // 2 minutes max polling
      2000 // 2 second intervals
    );
  } catch (error) {
    console.error(`Failed to poll transaction ${txHash}:`, error);

    // Mark as failed on error
    transactions.update((txs) =>
      txs.map((tx) => {
        if (tx.transaction_hash === txHash) {
          return {
            ...tx,
            status: "failed",
            error_message:
              error instanceof Error ? error.message : "Polling failed",
          };
        }
        return tx;
      })
    );
  } finally {
    activePollingTasks.delete(txHash);
  }
}

/**
 * Helper to update transaction status manually
 */
export function updateTransactionStatus(
  txHash: string,
  updates: Partial<Transaction>
): void {
  transactions.update((txs) =>
    txs.map((tx) =>
      tx.transaction_hash === txHash ? { ...tx, ...updates } : tx
    )
  );
}
