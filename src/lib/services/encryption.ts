import { invoke } from '@tauri-apps/api/core';

// This interface defines the structure of the manifest coming from the Rust backend
export interface FileManifestForJs {
  merkleRoot: string;
  chunks: {
    index: number;
    hash: string;
    size: number;
    encryptedHash: string;
    encryptedSize: number;
  }[];
  encryptedKeyBundle: string; // This is a JSON string of the EncryptedAesKeyBundle
}

export const encryptionService = {
  /**
   * Invokes the backend to chunk and encrypt a file.
   * @param filePath The absolute path to the file.
   * @param recipientPublicKey Optional recipient's X25519 public key (hex-encoded). If not provided, encrypts for self.
   * @returns A promise that resolves to the file manifest.
   */
  async encryptFile(filePath: string, recipientPublicKey?: string): Promise<FileManifestForJs> {
    if (recipientPublicKey) {
      return await invoke('encrypt_file_for_recipient', { 
        filePath, 
        recipientPublicKey 
      });
    } else {
      return await invoke('encrypt_file_for_self_upload', { filePath });
    }
  },

  /**
   * Invokes the backend to reassemble and decrypt a file from its chunks.
   * @param manifest The file manifest containing chunk info and the encrypted key.
   * @param outputPath The absolute path where the decrypted file will be saved.
   * @returns A promise that resolves when decryption is complete.
   */
  async decryptFile(manifest: FileManifestForJs, outputPath: string): Promise<void> {
    await invoke('decrypt_and_reassemble_file', { manifestJs: manifest, outputPath });
  }
};