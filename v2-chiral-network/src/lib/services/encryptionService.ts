/**
 * End-to-End Encryption Service for Chiral Network V2
 *
 * Provides secure file encryption using:
 * - X25519 for key exchange (ECDH)
 * - AES-256-GCM for symmetric encryption
 * - HKDF-SHA256 for key derivation
 */

import { invoke } from '@tauri-apps/api/core';

/** Encrypted file bundle returned from encryption */
export interface EncryptedFileBundle {
  ephemeralPublicKey: string;
  ciphertext: string;
  nonce: string;
}

/** Check if running in Tauri environment */
function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Encryption Service - handles all E2E encryption operations
 */
export const encryptionService = {
  /**
   * Initialize encryption keypair from wallet private key
   * This derives a deterministic encryption keypair from the wallet,
   * so the same wallet always produces the same encryption key.
   *
   * @param walletPrivateKey - Wallet private key (hex string without 0x prefix)
   * @returns The public key (hex string) for sharing with others
   */
  async initializeKeypair(walletPrivateKey: string): Promise<string> {
    if (!isTauri()) {
      console.warn('Encryption not available in web mode');
      return '';
    }

    // Remove 0x prefix if present
    const cleanKey = walletPrivateKey.startsWith('0x')
      ? walletPrivateKey.slice(2)
      : walletPrivateKey;

    return await invoke<string>('init_encryption_keypair', {
      walletPrivateKey: cleanKey
    });
  },

  /**
   * Get our encryption public key
   * @returns The public key hex string, or null if not initialized
   */
  async getPublicKey(): Promise<string | null> {
    if (!isTauri()) {
      return null;
    }

    return await invoke<string | null>('get_encryption_public_key');
  },

  /**
   * Encrypt file data for a specific recipient
   *
   * @param recipientPublicKey - Recipient's X25519 public key (hex string)
   * @param fileData - Raw file data as Uint8Array
   * @returns Encrypted file bundle
   */
  async encryptForRecipient(
    recipientPublicKey: string,
    fileData: Uint8Array
  ): Promise<EncryptedFileBundle> {
    if (!isTauri()) {
      throw new Error('Encryption not available in web mode');
    }

    return await invoke<EncryptedFileBundle>('encrypt_file_for_recipient', {
      recipientPublicKey,
      fileData: Array.from(fileData)
    });
  },

  /**
   * Decrypt file data using our keypair
   *
   * @param encryptedBundle - The encrypted file bundle
   * @returns Decrypted file data as Uint8Array
   */
  async decryptFile(encryptedBundle: EncryptedFileBundle): Promise<Uint8Array> {
    if (!isTauri()) {
      throw new Error('Decryption not available in web mode');
    }

    const decrypted = await invoke<number[]>('decrypt_file_data', {
      encryptedBundle
    });

    return new Uint8Array(decrypted);
  },

  /**
   * Send an encrypted file to a peer
   *
   * @param peerId - Target peer's libp2p ID
   * @param fileName - Name of the file
   * @param fileData - Raw file data
   * @param recipientPublicKey - Recipient's encryption public key
   * @param transferId - Unique transfer ID
   */
  async sendEncryptedFile(
    peerId: string,
    fileName: string,
    fileData: Uint8Array,
    recipientPublicKey: string,
    transferId: string
  ): Promise<void> {
    if (!isTauri()) {
      throw new Error('Encrypted file transfer not available in web mode');
    }

    await invoke('send_encrypted_file', {
      peerId,
      fileName,
      fileData: Array.from(fileData),
      recipientPublicKey,
      transferId
    });
  },

  /**
   * Publish our encryption public key to the DHT
   * This allows other peers to find our key and send us encrypted files
   */
  async publishPublicKey(): Promise<void> {
    if (!isTauri()) {
      return;
    }

    await invoke('publish_encryption_key');
  },

  /**
   * Lookup a peer's encryption public key from the DHT
   *
   * @param peerId - The peer's libp2p ID
   * @returns The peer's public key, or null if not found
   */
  async lookupPeerPublicKey(peerId: string): Promise<string | null> {
    if (!isTauri()) {
      return null;
    }

    return await invoke<string | null>('lookup_encryption_key', { peerId });
  },

  /**
   * Check if a file name indicates it's encrypted
   */
  isEncryptedFile(fileName: string): boolean {
    return fileName.endsWith('.encrypted');
  },

  /**
   * Get the original file name from an encrypted file name
   */
  getOriginalFileName(encryptedFileName: string): string {
    if (encryptedFileName.endsWith('.encrypted')) {
      return encryptedFileName.slice(0, -'.encrypted'.length);
    }
    return encryptedFileName;
  }
};

export default encryptionService;
