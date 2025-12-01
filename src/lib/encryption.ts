import { invoke } from "@tauri-apps/api/core";

export interface EncryptionInfo {
  method: string;
  keyFingerprint: string;
  nonce: number[];
  salt: number[];
}

export interface EncryptionResult {
  encryptedPath: string;
  encryptionInfo: EncryptionInfo;
}

/**
 * Frontend encryption service for file encryption
 */
export class EncryptionService {
  private static instance: EncryptionService | null = null;

  private constructor() {}

  static getInstance(): EncryptionService {
    if (!EncryptionService.instance) {
      EncryptionService.instance = new EncryptionService();
    }
    return EncryptionService.instance;
  }

  /**
   * Encrypt a file with password
   */
  async encryptFileWithPassword(
    inputPath: string,
    outputPath: string,
    password: string
  ): Promise<EncryptionInfo> {
    try {
      const info = await invoke<EncryptionInfo>("encrypt_file_with_password", {
        inputPath,
        outputPath,
        password,
      });
      console.log("File encrypted successfully:", outputPath);
      return info;
    } catch (error) {
      console.error("Failed to encrypt file:", error);
      throw error;
    }
  }

  /**
   * Decrypt a file with password
   */
  async decryptFileWithPassword(
    inputPath: string,
    outputPath: string,
    password: string,
    encryptionInfo: EncryptionInfo
  ): Promise<number> {
    try {
      const size = await invoke<number>("decrypt_file_with_password", {
        inputPath,
        outputPath,
        password,
        encryptionInfo,
      });
      console.log("File decrypted successfully:", outputPath, "Size:", size);
      return size;
    } catch (error) {
      console.error("Failed to decrypt file:", error);
      throw error;
    }
  }

  /**
   * Encrypt a file for upload (creates .enc file in same directory)
   */
  async encryptFileForUpload(
    inputPath: string,
    password?: string
  ): Promise<EncryptionResult> {
    try {
      const [encryptedPath, encryptionInfo] = await invoke<
        [string, EncryptionInfo]
      >("encrypt_file_for_upload", {
        inputPath,
        password: password || null,
      });

      console.log("File encrypted for upload:", encryptedPath);
      return { encryptedPath, encryptionInfo };
    } catch (error) {
      console.error("Failed to encrypt file for upload:", error);
      throw error;
    }
  }

  /**
   * Get file extension based on encryption
   */
  getEncryptedFileName(originalPath: string): string {
    return originalPath + ".enc";
  }

  /**
   * Get decrypted file name by removing .enc extension
   */
  getDecryptedFileName(encryptedPath: string): string {
    if (encryptedPath.endsWith(".enc")) {
      return encryptedPath.slice(0, -4);
    }
    return encryptedPath + ".decrypted";
  }

  /**
   * Validate encryption info
   */
  isValidEncryptionInfo(info: any): info is EncryptionInfo {
    return (
      info !== null &&
      typeof info === "object" &&
      typeof info.method === "string" &&
      typeof info.keyFingerprint === "string" &&
      Array.isArray(info.nonce) &&
      Array.isArray(info.salt)
    );
  }

  /**
   * Create encryption metadata for file sharing
   */
  createEncryptionMetadata(encryptionInfo: EncryptionInfo) {
    return {
      isEncrypted: true,
      encryptionMethod: encryptionInfo.method,
      keyFingerprint: encryptionInfo.keyFingerprint,
    };
  }
}

// Export singleton instance
export const encryptionService = EncryptionService.getInstance();
