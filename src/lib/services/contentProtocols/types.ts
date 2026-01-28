import type { Ed2kSourceInfo, FtpSourceInfo, HttpSourceInfo } from "$lib/dht";
import type { FileHashingProgress } from "$lib/services/uploadService";
import type { ProtocolDetails } from "$lib/types/protocols";

export enum Protocol {
  WebRTC = "WEBRTC",
  BitTorrent = "BITTORRENT",
  ED2K = "ED2K",
  FTP = "FTP",
  HTTP = "HTTP",
  UNKNOWN = "UNKNOWN",
}

export interface FileIdentification {
  protocol: Protocol;
  fileHash: string;
  fileName: string;
  fileSize: number;
  protocolDetails: ProtocolDetails;
}

export interface PeerInfo {
  id: string;
  address: string;
  reputation?: number;
}

export type ProgressUpdate = (update: {
  status?: string;
  percent?: number;
  transferredBytes?: number;
  totalBytes?: number;
}) => void;

export interface UploadOptions {
  protocol: Protocol;
  filePath: string;
  pricePerMb: number;
  ftpConfig?: {
    url: string;
    username?: string;
    password?: string;
    useFtps?: boolean;
    passiveMode?: boolean;
  };
  onHashingProgress?: (progress: FileHashingProgress) => void;
}

export interface UploadResult {
  success: boolean;
  fileHash?: string;
  protocolHash?: string;
  error?: string;
  protocol: Protocol;
}

export interface DownloadResult {
  outputPath?: string;
  completed?: boolean;
  transferId?: string;
}

export interface IContentProtocol {
  getName(): Protocol;
  getPeersServing(identification: FileIdentification): Promise<PeerInfo[]>;
  getContentFrom(
    peerId: string,
    identification: FileIdentification,
    progressUpdate: ProgressUpdate,
    outputPath?: string,
  ): Promise<DownloadResult>;
  uploadFile(
    options: UploadOptions,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult>;
  startSeeding(
    filePathOrData: string | Uint8Array,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult>;
  stopSeeding(identification: FileIdentification): Promise<boolean>;
  pauseDownload(identification: FileIdentification): Promise<boolean>;
  resumeDownload(identification: FileIdentification): Promise<boolean>;
  cancelDownload(identification: FileIdentification): Promise<boolean>;
}
