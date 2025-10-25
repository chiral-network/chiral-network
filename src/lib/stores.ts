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
  version?: number; // File version number for versioning system
  isNewVersion?: boolean; // Whether this is a new version of an existing file
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

export interface ChatMessage {
  id: string;
  peerId: string;
  peerNickname: string;
  content: string;
  timestamp: Date;
  type: "sent" | "received";
  read: boolean;
}

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
  type: "sent" | "received";
  amount: number;
  to?: string;
  from?: string;
  txHash?: string;
  date: Date;
  description: string;
  status: "pending" | "completed";
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
  address: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb1",
  balance: 1000.5,
  pendingTransactions: 5,
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
    status: "completed",
  },
  {
    id: 2,
    type: "sent",
    amount: 10.25,
    to: "0x1234...5678",
    date: new Date("2024-03-14"),
    description: "Proxy service",
    status: "completed",
  },
  {
    id: 3,
    type: "received",
    amount: 100,
    from: "0xabcd...ef12",
    date: new Date("2024-03-13"),
    description: "Upload reward",
    status: "completed",
  },
  {
    id: 4,
    type: "sent",
    amount: 5.5,
    to: "0x9876...5432",
    date: new Date("2024-03-12"),
    description: "File download",
    status: "completed",
  },
];

// Stores
export const files = writable<FileItem[]>(dummyFiles);
export const wallet = writable<WalletInfo>(dummyWallet);
export const activeDownloads = writable<number>(1);
export const transactions = writable<Transaction[]>(dummyTransactions);

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

export const chatMessages = writable<ChatMessage[]>([]);
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

export const totalEarned = derived(
  miningState,
  ($miningState) => $miningState.totalRewards
);

export const totalSpent = derived(transactions, ($txs) =>
  $txs
    .filter((tx) => tx.type === "sent")
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
  autoStartDht: boolean; // Automatically start DHT network on app launch
  anonymousMode: boolean;
  shareAnalytics: boolean;
  enableNotifications: boolean;
  notifyOnComplete: boolean;
  notifyOnError: boolean;
  soundAlerts: boolean;
  enableDHT: boolean;
  enableIPFS: boolean;
  chunkSize: number; // KB
  cacheSize: number; // MB
  logLevel: string;
  autoUpdate: boolean;
  enableBandwidthScheduling: boolean;
  bandwidthSchedules: BandwidthScheduleEntry[];
  pricePerMb: number; // Price per MB in Chiral (e.g., 0.001)
}

// Export the settings store
// We initialize with a safe default structure. Settings.svelte will load/persist the actual state.
export const settings = writable<AppSettings>({
  storagePath: "~/ChiralNetwork/Storage",
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
  enableAutonat: true, // Enable AutoNAT by default
  autonatProbeInterval: 30, // 30 seconds default
  autonatServers: [], // Use bootstrap nodes by default
  enableAutorelay: true, // Enable AutoRelay by default
  preferredRelays: [], // Use bootstrap nodes as relays by default
  enableRelayServer: true, // Enabled by default - helps strengthen the network
  relayServerAlias: "", // Empty by default - user can set a friendly name
  autoStartDht: false, // Disabled by default - user must opt-in
  anonymousMode: false,
  shareAnalytics: true,
  enableNotifications: true,
  notifyOnComplete: true,
  notifyOnError: true,
  soundAlerts: false,
  enableDHT: true,
  enableIPFS: false,
  chunkSize: 256,
  cacheSize: 1024,
  logLevel: "info",
  autoUpdate: true,
  enableBandwidthScheduling: false,
  bandwidthSchedules: [],
  pricePerMb: 0.001, // Default price: 0.001 Chiral per MB
});
