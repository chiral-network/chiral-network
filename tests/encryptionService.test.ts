import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { encryptionService, type EncryptedFileBundle } from '$lib/services/encryptionService';

// Mock the invoke function
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const mockedInvoke = vi.mocked(invoke);

describe('encryptionService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('non-Tauri environment', () => {
    // By default, jsdom doesn't have __TAURI_INTERNALS__, so isTauri() returns false

    it('initializeKeypair should return empty string', async () => {
      const result = await encryptionService.initializeKeypair('abc123');
      expect(result).toBe('');
      expect(mockedInvoke).not.toHaveBeenCalled();
    });

    it('getPublicKey should return null', async () => {
      const result = await encryptionService.getPublicKey();
      expect(result).toBeNull();
      expect(mockedInvoke).not.toHaveBeenCalled();
    });

    it('encryptForRecipient should throw', async () => {
      await expect(
        encryptionService.encryptForRecipient('pubkey', new Uint8Array([1, 2, 3]))
      ).rejects.toThrow('Encryption not available in web mode');
    });

    it('decryptFile should throw', async () => {
      const bundle: EncryptedFileBundle = {
        ephemeralPublicKey: 'abc',
        ciphertext: 'def',
        nonce: 'ghi'
      };
      await expect(encryptionService.decryptFile(bundle)).rejects.toThrow(
        'Decryption not available in web mode'
      );
    });

    it('sendEncryptedFile should throw', async () => {
      await expect(
        encryptionService.sendEncryptedFile('peer', 'file.txt', new Uint8Array(), 'key', 'id')
      ).rejects.toThrow('Encrypted file transfer not available in web mode');
    });

    it('publishPublicKey should return without calling invoke', async () => {
      await encryptionService.publishPublicKey();
      expect(mockedInvoke).not.toHaveBeenCalled();
    });

    it('lookupPeerPublicKey should return null', async () => {
      const result = await encryptionService.lookupPeerPublicKey('peer123');
      expect(result).toBeNull();
      expect(mockedInvoke).not.toHaveBeenCalled();
    });
  });

  describe('Tauri environment', () => {
    beforeEach(() => {
      // Simulate Tauri environment
      (window as any).__TAURI_INTERNALS__ = {};
    });

    afterEach(() => {
      delete (window as any).__TAURI_INTERNALS__;
    });

    it('initializeKeypair should invoke backend and strip 0x prefix', async () => {
      mockedInvoke.mockResolvedValue('public_key_hex');
      const result = await encryptionService.initializeKeypair('0xabcdef123456');
      expect(mockedInvoke).toHaveBeenCalledWith('init_encryption_keypair', {
        walletPrivateKey: 'abcdef123456'
      });
      expect(result).toBe('public_key_hex');
    });

    it('initializeKeypair should pass key without 0x prefix as-is', async () => {
      mockedInvoke.mockResolvedValue('public_key_hex');
      await encryptionService.initializeKeypair('abcdef123456');
      expect(mockedInvoke).toHaveBeenCalledWith('init_encryption_keypair', {
        walletPrivateKey: 'abcdef123456'
      });
    });

    it('getPublicKey should invoke backend', async () => {
      mockedInvoke.mockResolvedValue('my_public_key');
      const result = await encryptionService.getPublicKey();
      expect(mockedInvoke).toHaveBeenCalledWith('get_encryption_public_key');
      expect(result).toBe('my_public_key');
    });

    it('encryptForRecipient should invoke backend with correct args', async () => {
      const bundle: EncryptedFileBundle = {
        ephemeralPublicKey: 'eph_key',
        ciphertext: 'encrypted_data',
        nonce: 'random_nonce'
      };
      mockedInvoke.mockResolvedValue(bundle);

      const fileData = new Uint8Array([1, 2, 3, 4, 5]);
      const result = await encryptionService.encryptForRecipient('recipient_key', fileData);

      expect(mockedInvoke).toHaveBeenCalledWith('encrypt_file_for_recipient', {
        recipientPublicKey: 'recipient_key',
        fileData: [1, 2, 3, 4, 5]
      });
      expect(result).toEqual(bundle);
    });

    it('decryptFile should invoke backend and return Uint8Array', async () => {
      mockedInvoke.mockResolvedValue([72, 101, 108, 108, 111]); // "Hello"
      const bundle: EncryptedFileBundle = {
        ephemeralPublicKey: 'eph',
        ciphertext: 'ct',
        nonce: 'nc'
      };
      const result = await encryptionService.decryptFile(bundle);
      expect(result).toBeInstanceOf(Uint8Array);
      expect(Array.from(result)).toEqual([72, 101, 108, 108, 111]);
    });

    it('sendEncryptedFile should invoke backend with all parameters', async () => {
      mockedInvoke.mockResolvedValue(undefined);
      const fileData = new Uint8Array([10, 20, 30]);

      await encryptionService.sendEncryptedFile(
        'peer123', 'secret.txt', fileData, 'pub_key', 'transfer_001'
      );

      expect(mockedInvoke).toHaveBeenCalledWith('send_encrypted_file', {
        peerId: 'peer123',
        fileName: 'secret.txt',
        fileData: [10, 20, 30],
        recipientPublicKey: 'pub_key',
        transferId: 'transfer_001'
      });
    });

    it('publishPublicKey should invoke backend', async () => {
      mockedInvoke.mockResolvedValue(undefined);
      await encryptionService.publishPublicKey();
      expect(mockedInvoke).toHaveBeenCalledWith('publish_encryption_key');
    });

    it('lookupPeerPublicKey should invoke backend', async () => {
      mockedInvoke.mockResolvedValue('peer_pub_key');
      const result = await encryptionService.lookupPeerPublicKey('peer456');
      expect(mockedInvoke).toHaveBeenCalledWith('lookup_encryption_key', {
        peerId: 'peer456'
      });
      expect(result).toBe('peer_pub_key');
    });
  });

  describe('isEncryptedFile', () => {
    it('should return true for .encrypted files', () => {
      expect(encryptionService.isEncryptedFile('document.pdf.encrypted')).toBe(true);
    });

    it('should return false for regular files', () => {
      expect(encryptionService.isEncryptedFile('document.pdf')).toBe(false);
    });

    it('should return false for empty string', () => {
      expect(encryptionService.isEncryptedFile('')).toBe(false);
    });

    it('should return true for just .encrypted', () => {
      expect(encryptionService.isEncryptedFile('.encrypted')).toBe(true);
    });
  });

  describe('getOriginalFileName', () => {
    it('should strip .encrypted suffix', () => {
      expect(encryptionService.getOriginalFileName('document.pdf.encrypted')).toBe('document.pdf');
    });

    it('should return original name if no .encrypted suffix', () => {
      expect(encryptionService.getOriginalFileName('document.pdf')).toBe('document.pdf');
    });

    it('should handle files with multiple dots', () => {
      expect(encryptionService.getOriginalFileName('my.file.name.txt.encrypted')).toBe('my.file.name.txt');
    });

    it('should handle .encrypted as the entire name', () => {
      expect(encryptionService.getOriginalFileName('.encrypted')).toBe('');
    });
  });
});
