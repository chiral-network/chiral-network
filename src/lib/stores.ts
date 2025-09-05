import { writable } from "svelte/store";

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
    | "seeding";
  progress?: number;
  uploadDate?: Date;
  owner?: string;
  description?: string;
  seeders?: number;
  leechers?: number;
  encrypted?: boolean;
  priority?: "low" | "normal" | "high";
  downloadSpeed?: number;
  uploadSpeed?: number;
  timeRemaining?: number;
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
  totalEarned: number;
  totalSpent: number;
  stakedAmount?: number;
  miningRewards?: number;
  reputation?: number;
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

// Sample dummy data
const dummyFiles: FileItem[] = [
  {
    id: "1",
    name: "Document.pdf",
    hash: "QmZ4tDuvesekqMD",
    size: 2048576,
    status: "completed",
    progress: 100,
  },
  {
    id: "2",
    name: "Image.jpg",
    hash: "QmZ4tDuvesekqME",
    size: 1024000,
    status: "downloading",
    progress: 65,
  },
  {
    id: "3",
    name: "Video.mp4",
    hash: "QmZ4tDuvesekqMF",
    size: 50331648,
    status: "paused",
    progress: 30,
  },
  {
    id: "4",
    name: "Archive.zip",
    hash: "QmZ4tDuvesekqMG",
    size: 10485760,
    status: "uploaded",
    progress: 100,
  },
];

const dummyProxyNodes: ProxyNode[] = [
  {
    id: "1",
    address: "192.168.1.100:8080",
    status: "online",
    bandwidth: 100,
    latency: 20,
    region: "US-East",
  },
  {
    id: "2",
    address: "10.0.0.50:8080",
    status: "online",
    bandwidth: 50,
    latency: 45,
    region: "EU-West",
  },
  {
    id: "3",
    address: "172.16.0.10:8080",
    status: "offline",
    bandwidth: 0,
    latency: 999,
    region: "Asia-Pacific",
  },
  {
    id: "4",
    address: "192.168.2.25:8080",
    status: "connecting",
    bandwidth: 75,
    latency: 30,
    region: "US-West",
  },
];

const dummyWallet: WalletInfo = {
  address: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
  balance: 1000.5,
  pendingTransactions: 2,
  totalEarned: 250.75,
  totalSpent: 45.25,
};

// Additional dummy data
const dummyPeers: PeerInfo[] = [
  {
    id: "peer1",
    address: "192.168.1.50",
    nickname: "AliceNode",
    status: "online",
    reputation: 4.8,
    sharedFiles: 150,
    totalSize: 5368709120,
    joinDate: new Date("2024-01-01"),
    lastSeen: new Date(),
    location: "US-East",
  },
  {
    id: "peer2",
    address: "10.0.0.25",
    nickname: "BobStorage",
    status: "online",
    reputation: 4.5,
    sharedFiles: 89,
    totalSize: 2147483648,
    joinDate: new Date("2024-02-15"),
    lastSeen: new Date(),
    location: "EU-West",
  },
  {
    id: "peer3",
    address: "172.16.0.100",
    nickname: "CharlieShare",
    status: "away",
    reputation: 4.2,
    sharedFiles: 45,
    totalSize: 1073741824,
    joinDate: new Date("2024-03-01"),
    lastSeen: new Date(Date.now() - 3600000),
    location: "Asia-Pacific",
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

// Stores
export const files = writable<FileItem[]>(dummyFiles);
export const proxyNodes = writable<ProxyNode[]>(dummyProxyNodes);
export const wallet = writable<WalletInfo>(dummyWallet);
export const activeDownloads = writable<number>(2);
export const networkStatus = writable<"connected" | "disconnected">(
  "connected"
);
export const peers = writable<PeerInfo[]>(dummyPeers);
export const chatMessages = writable<ChatMessage[]>([]);
export const networkStats = writable<NetworkStats>(dummyNetworkStats);
export const downloadQueue = writable<FileItem[]>([]);
export const currentTheme = writable<"light" | "dark">("light");
