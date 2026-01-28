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

export class Ed2kProtocol implements IContentProtocol {
  getName(): Protocol {
    return Protocol.ED2K;
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
      identification.protocolDetails?.[Protocol.ED2K]?.sources ?? [];
    if (sources.length === 0) {
      throw new Error("No ED2K sources available for this file");
    }

    progressUpdate({ status: "starting" });
    const resolvedPath = await resolveOutputPath(
      identification.fileName,
      outputPath,
    );
    console.log("not implemented");
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
    _options: UploadOptions,
    _progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    return {
      success: false,
      error: "ED2K uploads are not supported",
      protocol: Protocol.ED2K,
    };
  }

  async startSeeding(
    _filePathOrData: string | Uint8Array,
    _progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    return {
      success: false,
      error: "ED2K uploads are not supported",
      protocol: Protocol.ED2K,
    };
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
