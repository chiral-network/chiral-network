import { Protocol } from "$lib/services/contentProtocols/types";
export type ProtocolDetailsByProtocol = {
  [Protocol.HTTP]: HttpProtocolDetails;
  [Protocol.FTP]: FtpProtocolDetails;
  [Protocol.ED2K]: Ed2kProtocolDetails;
  [Protocol.BitTorrent]: BitTorrentProtocolDetails;
  [Protocol.WebRTC]: WebRtcProtocolDetails;
  [Protocol.UNKNOWN]: never;
};
export type ProtocolDetails = Partial<ProtocolDetailsByProtocol>;

// HTTP Protocol Details
export interface HttpProtocolDetails {
  sources: HttpSourceInfo[];
}

export interface HttpSourceInfo {
  url: string;
  authHeader?: string;
  verifySsl: boolean;
  headers?: [string, string][];
  timeoutSecs?: number;
}

// FTP Protocol Details
export interface FtpProtocolDetails {
  sources: FtpSourceInfo[];
}

export interface FtpSourceInfo {
  url: string;
  username?: string;
  encryptedPassword?: string;
  passiveMode: boolean;
  useFtps: boolean;
  timeoutSecs?: number;
  supportsResume: boolean;
  fileSize: number;
  lastChecked?: number;
  isAvailable: boolean;
}

// ED2K Protocol Details
export interface Ed2kProtocolDetails {
  sources: Ed2kSourceInfo[];
}

export interface Ed2kSourceInfo {
  serverUrl: string;
  fileHash: string;
  fileSize: number;
  fileName?: string;
  sources?: string[];
  timeout?: number;
  chunkHashes?: string[];
}

// BitTorrent Protocol Details
export interface BitTorrentProtocolDetails {
  infoHash: string;
  trackers: string[];
}

// BitSwap/IPFS Protocol Details
export interface BitswapProtocolDetails {
  cids: string[];
  isRoot: boolean;
}

// WebRTC Protocol Details
export interface WebRtcProtocolDetails {
  enabled: boolean;
}

// Encryption Details
export interface EncryptionDetails {
  method: string;
  keyFingerprint: string;
  encryptedKeyBundle: EncryptedAesKeyBundle;
}

export interface EncryptedAesKeyBundle {
  ciphertext: number[];
  nonce: number[];
  ephemeralPublicKey: number[];
}
