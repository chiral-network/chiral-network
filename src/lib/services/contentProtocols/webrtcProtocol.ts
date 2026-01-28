import { invoke } from "@tauri-apps/api/core";
import {
  Protocol,
  type DownloadResult,
  type FileIdentification,
  type IContentProtocol,
  type ProgressUpdate,
  type UploadOptions,
  type UploadResult,
} from "./types";
import { resolveOutputPath } from "./utils";
import { uploadFile } from "$lib/services/uploadService";

export class WebRTCProtocol implements IContentProtocol {
  getName(): Protocol {
    return Protocol.WebRTC;
  }

  async getPeersServing(_identification: FileIdentification) {
    return [];
  }

  async getContentFrom(
    peerId: string,
    identification: FileIdentification,
    progressUpdate: ProgressUpdate,
    outputPath?: string,
  ): Promise<DownloadResult> {
    progressUpdate({ status: "starting" });
    const resolvedPath = await resolveOutputPath(
      identification.fileName,
      outputPath,
    );

    const transferId = await invoke<string>("download_file_from_network", {
      peerId: peerId,
      fileHash: identification.fileHash,
      fileName: identification.fileName,
      fileSize: identification.fileSize,
      outputPath: resolvedPath,
    });

    progressUpdate({ status: "downloading" });
    return { outputPath: resolvedPath, completed: false, transferId };
  }

  async uploadFile(
    options: UploadOptions,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    progressUpdate({ status: "hashing" });
    const result = await uploadFile({
      protocol: Protocol.WebRTC,
      filePath: options.filePath,
      pricePerMb: options.pricePerMb,
      onHashingProgress: options.onHashingProgress,
    });
    return { ...result, protocol: Protocol.WebRTC };
  }

  async startSeeding(
    filePathOrData: string | Uint8Array,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    if (typeof filePathOrData !== "string") {
      return {
        success: false,
        error: "WebRTC upload requires a file path",
        protocol: Protocol.WebRTC,
      };
    }

    return await this.uploadFile(
      {
        protocol: Protocol.WebRTC,
        filePath: filePathOrData,
        pricePerMb: 0,
      },
      progressUpdate,
    );
  }

  async stopSeeding(_identification: FileIdentification): Promise<boolean> {
    return false;
  }

  async pauseDownload(_identification: FileIdentification): Promise<boolean> {
    return false;
  }

  async resumeDownload(_identification: FileIdentification): Promise<boolean> {
    return false;
  }

  async cancelDownload(_identification: FileIdentification): Promise<boolean> {
    return false;
  }
}
