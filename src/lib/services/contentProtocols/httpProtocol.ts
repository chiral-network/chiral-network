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
import { resolveOutputPath } from "./utils";
import { uploadFile } from "$lib/services/uploadService";

export class HttpProtocol implements IContentProtocol {
  getName(): Protocol {
    return Protocol.HTTP;
  }

  async getPeersServing(_identification: FileIdentification) {
    return [];
  }

  async getContentFrom(
    _peerId: string,
    identification: FileIdentification,
    progressUpdate: ProgressUpdate,
    outputPath?: string,
  ): Promise<DownloadResult> {
    const sources =
      identification.protocolDetails?.[Protocol.HTTP]?.sources ?? [];
    if (sources.length === 0) {
      throw new Error("No HTTP sources available for this file");
    }

    progressUpdate({ status: "starting" });
    const resolvedPath = await resolveOutputPath(
      identification.fileName,
      outputPath,
    );

    await invoke("download_file_multi_source", {
      fileHash: identification.fileHash,
      outputPath: resolvedPath,
      preferMultiSource: true,
      maxPeers: 1,
    });

    progressUpdate({ status: "completed" });
    return { outputPath: resolvedPath, completed: true };
  }

  async uploadFile(
    options: UploadOptions,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    progressUpdate({ status: "hashing" });
    const result = await uploadFile({
      protocol: "HTTP",
      filePath: options.filePath,
      pricePerMb: options.pricePerMb,
      onHashingProgress: options.onHashingProgress,
    });
    return { ...result, protocol: Protocol.HTTP };
  }

  async startSeeding(
    filePathOrData: string | Uint8Array,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    if (typeof filePathOrData !== "string") {
      return {
        success: false,
        error: "HTTP upload requires a file path",
        protocol: Protocol.HTTP,
      };
    }

    return await this.uploadFile(
      {
        protocol: Protocol.HTTP,
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
