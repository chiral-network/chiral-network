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

export class FtpProtocol implements IContentProtocol {
  getName(): Protocol {
    return Protocol.FTP;
  }

  async getContentFrom(
    _peerId: string,
    identification: FileIdentification,
    progressUpdate: ProgressUpdate,
    outputPath?: string,
  ): Promise<DownloadResult> {
    const ftpSource = identification.protocolDetails.FTP?.sources[0];
    console.log("[ftp] getContentFrom", {
      fileName: identification.fileName,
      identification,
      ftpSource,
      outputPath,
    });
    if (!ftpSource) {
      throw new Error("No FTP sources available for this file");
    }

    progressUpdate({ status: "starting" });
    const resolvedPath = await resolveOutputPath(
      identification.fileName,
      outputPath,
    );

    const response = await invoke<{ transferId: string; outputPath: string }>(
      "start_ftp_download",
      {
        url: ftpSource.url,
        outputPath: resolvedPath,
        username: ftpSource.username || null,
        password: ftpSource.encryptedPassword || null,
      },
    );

    progressUpdate({ status: "downloading" });
    console.log(response);
    return {
      outputPath: response.outputPath,
      completed: false,
      transferId: response.transferId,
    };
  }

  async uploadFile(
    options: UploadOptions,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    if (!options.ftpConfig) {
      return {
        success: false,
        error: "FTP configuration is required",
        protocol: Protocol.FTP,
      };
    }

    progressUpdate({ status: "hashing" });
    const result = await uploadFile({
      protocol: Protocol.FTP,
      filePath: options.filePath,
      pricePerMb: options.pricePerMb,
      ftpConfig: {
        url: options.ftpConfig.url,
        username: options.ftpConfig.username,
        password: options.ftpConfig.password,
        useFtps: options.ftpConfig.useFtps ?? false,
        passiveMode: options.ftpConfig.passiveMode ?? true,
      },
      onHashingProgress: options.onHashingProgress,
    });
    return { ...result, protocol: Protocol.FTP };
  }

  async startSeeding(
    filePathOrData: string | Uint8Array,
    progressUpdate: ProgressUpdate,
  ): Promise<UploadResult> {
    if (typeof filePathOrData !== "string") {
      return {
        success: false,
        error: "FTP upload requires a file path",
        protocol: Protocol.FTP,
      };
    }

    return await this.uploadFile(
      {
        protocol: Protocol.FTP,
        filePath: filePathOrData,
        pricePerMb: 0,
        ftpConfig: {
          url: "",
          useFtps: false,
          passiveMode: true,
        },
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
