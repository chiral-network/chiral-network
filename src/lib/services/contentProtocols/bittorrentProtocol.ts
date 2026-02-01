import { invoke } from "@tauri-apps/api/core";
import { Protocol } from "./types";
import type {
  DownloadResult,
  FileIdentification,
  IContentProtocol,
  ProgressUpdate,
  UploadOptions,
  UploadResult,
} from "./types";
import { uploadFile } from "$lib/services/uploadService";

export class BitTorrentProtocol implements IContentProtocol {
  getName(): Protocol {
    return Protocol.BitTorrent;
  }
  async getContentFrom(
    _peerId: string,
    _identification: FileIdentification,
    _progressUpdate: ProgressUpdate,
  ): Promise<DownloadResult> {
    throw new Error("BitTorrent downloads are started via the search flow");
  }

  async uploadFile(
    options: UploadOptions,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    progressUpdate({ status: "hashing" });
    const result = await uploadFile({
      protocol: "BitTorrent",
      filePath: options.filePath,
      pricePerMb: options.pricePerMb,
      onHashingProgress: options.onHashingProgress,
    });
    return { ...result, protocol: Protocol.BitTorrent };
  }

  async startSeeding(
    filePathOrData: string | Uint8Array,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    if (typeof filePathOrData !== "string") {
      return {
        success: false,
        error: "BitTorrent upload requires a file path",
        protocol: Protocol.BitTorrent,
      };
    }

    return await this.uploadFile(
      {
        protocol: Protocol.BitTorrent,
        filePath: filePathOrData,
        pricePerMb: 0,
      },
      progressUpdate,
    );
  }

  async stopSeeding(identification: FileIdentification): Promise<boolean> {
    const infoHash =
      identification.protocolDetails?.[Protocol.BitTorrent]?.infoHash ||
      identification.fileHash;
    await invoke("remove_torrent", { infoHash, deleteFiles: false });
    return true;
  }

  async pauseDownload(identification: FileIdentification): Promise<boolean> {
    const infoHash =
      identification.protocolDetails?.[Protocol.BitTorrent]?.infoHash ||
      identification.fileHash;
    await invoke("pause_torrent", { infoHash });
    return true;
  }

  async resumeDownload(identification: FileIdentification): Promise<boolean> {
    const infoHash =
      identification.protocolDetails?.[Protocol.BitTorrent]?.infoHash ||
      identification.fileHash;
    await invoke("resume_torrent", { infoHash });
    return true;
  }

  async cancelDownload(identification: FileIdentification): Promise<boolean> {
    const infoHash =
      identification.protocolDetails?.[Protocol.BitTorrent]?.infoHash ||
      identification.fileHash;
    await invoke("remove_torrent", { infoHash, deleteFiles: false });
    return true;
  }
}
