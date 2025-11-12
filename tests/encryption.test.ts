import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  EncryptionService,
  encryptionService as singletonService,
  type EncryptionInfo,
  type EncryptionResult,
} from '../src/lib/encryption';
import {
  encryptionService as chunkEncryptionService,
  type FileManifestForJs,
} from '../src/lib/services/encryption';

// Mock Tauri invoke
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

describe('encryption.ts', () => {
  const mockInvoke = vi.mocked(invoke);

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('EncryptionService (Password-based)', () => {
    describe('Singleton Pattern', () => {
      it('should return the same instance', () => {
        const instance1 = EncryptionService.getInstance();
        const instance2 = EncryptionService.getInstance();
        
        expect(instance1).toBe(instance2);
      });

      it('should export singleton instance', () => {
        expect(singletonService).toBeInstanceOf(EncryptionService);
      });
    });

    describe('encryptFileWithPassword', () => {
      const mockEncryptionInfo: EncryptionInfo = {
        method: 'AES-256-GCM',
        keyFingerprint: 'abc123def456',
        nonce: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        salt: [11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26],
      };

      it('should encrypt file with password successfully', async () => {
        mockInvoke.mockResolvedValueOnce(mockEncryptionInfo);

        const result = await singletonService.encryptFileWithPassword(
          '/path/to/input.txt',
          '/path/to/output.enc',
          'password123'
        );

        expect(mockInvoke).toHaveBeenCalledWith('encrypt_file_with_password', {
          inputPath: '/path/to/input.txt',
          outputPath: '/path/to/output.enc',
          password: 'password123',
        });
        expect(result).toEqual(mockEncryptionInfo);
      });

      it('should log success message on encryption', async () => {
        const consoleSpy = vi.spyOn(console, 'log');
        mockInvoke.mockResolvedValueOnce(mockEncryptionInfo);

        await singletonService.encryptFileWithPassword(
          '/path/to/input.txt',
          '/path/to/output.enc',
          'password123'
        );

        expect(consoleSpy).toHaveBeenCalledWith(
          'File encrypted successfully:',
          '/path/to/output.enc'
        );
      });

      it('should handle encryption errors', async () => {
        const consoleErrorSpy = vi.spyOn(console, 'error');
        mockInvoke.mockRejectedValueOnce(new Error('Encryption failed'));

        await expect(
          singletonService.encryptFileWithPassword(
            '/path/to/input.txt',
            '/path/to/output.enc',
            'password123'
          )
        ).rejects.toThrow('Encryption failed');

        expect(consoleErrorSpy).toHaveBeenCalledWith(
          'Failed to encrypt file:',
          expect.any(Error)
        );
      });

      it('should handle empty password', async () => {
        mockInvoke.mockResolvedValueOnce(mockEncryptionInfo);

        await singletonService.encryptFileWithPassword(
          '/path/to/input.txt',
          '/path/to/output.enc',
          ''
        );

        expect(mockInvoke).toHaveBeenCalledWith('encrypt_file_with_password', {
          inputPath: '/path/to/input.txt',
          outputPath: '/path/to/output.enc',
          password: '',
        });
      });
    });

    describe('decryptFileWithPassword', () => {
      const mockEncryptionInfo: EncryptionInfo = {
        method: 'AES-256-GCM',
        keyFingerprint: 'abc123def456',
        nonce: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        salt: [11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26],
      };

      it('should decrypt file with password successfully', async () => {
        mockInvoke.mockResolvedValueOnce(1024);

        const size = await singletonService.decryptFileWithPassword(
          '/path/to/input.enc',
          '/path/to/output.txt',
          'password123',
          mockEncryptionInfo
        );

        expect(mockInvoke).toHaveBeenCalledWith('decrypt_file_with_password', {
          inputPath: '/path/to/input.enc',
          outputPath: '/path/to/output.txt',
          password: 'password123',
          encryptionInfo: mockEncryptionInfo,
        });
        expect(size).toBe(1024);
      });

      it('should log success message on decryption', async () => {
        const consoleSpy = vi.spyOn(console, 'log');
        mockInvoke.mockResolvedValueOnce(2048);

        await singletonService.decryptFileWithPassword(
          '/path/to/input.enc',
          '/path/to/output.txt',
          'password123',
          mockEncryptionInfo
        );

        expect(consoleSpy).toHaveBeenCalledWith(
          'File decrypted successfully:',
          '/path/to/output.txt',
          'Size:',
          2048
        );
      });

      it('should handle decryption errors', async () => {
        const consoleErrorSpy = vi.spyOn(console, 'error');
        mockInvoke.mockRejectedValueOnce(new Error('Wrong password'));

        await expect(
          singletonService.decryptFileWithPassword(
            '/path/to/input.enc',
            '/path/to/output.txt',
            'wrongpassword',
            mockEncryptionInfo
          )
        ).rejects.toThrow('Wrong password');

        expect(consoleErrorSpy).toHaveBeenCalledWith(
          'Failed to decrypt file:',
          expect.any(Error)
        );
      });

      it('should return file size in bytes', async () => {
        mockInvoke.mockResolvedValueOnce(1048576); // 1MB

        const size = await singletonService.decryptFileWithPassword(
          '/path/to/input.enc',
          '/path/to/output.txt',
          'password123',
          mockEncryptionInfo
        );

        expect(size).toBe(1048576);
      });
    });

    describe('encryptFileForUpload', () => {
      const mockEncryptionInfo: EncryptionInfo = {
        method: 'AES-256-GCM',
        keyFingerprint: 'xyz789',
        nonce: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        salt: [11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26],
      };

      it('should encrypt file for upload with password', async () => {
        const mockResult: [string, EncryptionInfo] = [
          '/path/to/file.txt.enc',
          mockEncryptionInfo,
        ];
        mockInvoke.mockResolvedValueOnce(mockResult);

        const result = await singletonService.encryptFileForUpload(
          '/path/to/file.txt',
          'password123'
        );

        expect(mockInvoke).toHaveBeenCalledWith('encrypt_file_for_upload', {
          inputPath: '/path/to/file.txt',
          password: 'password123',
        });
        expect(result).toEqual({
          encryptedPath: '/path/to/file.txt.enc',
          encryptionInfo: mockEncryptionInfo,
        });
      });

      it('should encrypt file for upload without password', async () => {
        const mockResult: [string, EncryptionInfo] = [
          '/path/to/file.txt.enc',
          mockEncryptionInfo,
        ];
        mockInvoke.mockResolvedValueOnce(mockResult);

        const result = await singletonService.encryptFileForUpload(
          '/path/to/file.txt'
        );

        expect(mockInvoke).toHaveBeenCalledWith('encrypt_file_for_upload', {
          inputPath: '/path/to/file.txt',
          password: null,
        });
        expect(result.encryptedPath).toBe('/path/to/file.txt.enc');
      });

      it('should log success message', async () => {
        const consoleSpy = vi.spyOn(console, 'log');
        const mockResult: [string, EncryptionInfo] = [
          '/path/to/file.txt.enc',
          mockEncryptionInfo,
        ];
        mockInvoke.mockResolvedValueOnce(mockResult);

        await singletonService.encryptFileForUpload('/path/to/file.txt');

        expect(consoleSpy).toHaveBeenCalledWith(
          'File encrypted for upload:',
          '/path/to/file.txt.enc'
        );
      });

      it('should handle encryption errors', async () => {
        const consoleErrorSpy = vi.spyOn(console, 'error');
        mockInvoke.mockRejectedValueOnce(new Error('Upload encryption failed'));

        await expect(
          singletonService.encryptFileForUpload('/path/to/file.txt')
        ).rejects.toThrow('Upload encryption failed');

        expect(consoleErrorSpy).toHaveBeenCalledWith(
          'Failed to encrypt file for upload:',
          expect.any(Error)
        );
      });
    });

    describe('getEncryptedFileName', () => {
      it('should append .enc extension', () => {
        const result = singletonService.getEncryptedFileName('/path/to/file.txt');
        expect(result).toBe('/path/to/file.txt.enc');
      });

      it('should handle files with multiple extensions', () => {
        const result = singletonService.getEncryptedFileName('/path/to/file.tar.gz');
        expect(result).toBe('/path/to/file.tar.gz.enc');
      });

      it('should handle files without extension', () => {
        const result = singletonService.getEncryptedFileName('/path/to/README');
        expect(result).toBe('/path/to/README.enc');
      });
    });

    describe('getDecryptedFileName', () => {
      it('should remove .enc extension', () => {
        const result = singletonService.getDecryptedFileName('/path/to/file.txt.enc');
        expect(result).toBe('/path/to/file.txt');
      });

      it('should append .decrypted if no .enc extension', () => {
        const result = singletonService.getDecryptedFileName('/path/to/file.txt');
        expect(result).toBe('/path/to/file.txt.decrypted');
      });

      it('should handle multiple .enc extensions correctly', () => {
        const result = singletonService.getDecryptedFileName('/path/to/file.enc.enc');
        expect(result).toBe('/path/to/file.enc');
      });
    });

    describe('isValidEncryptionInfo', () => {
      it('should validate correct encryption info', () => {
        const validInfo: EncryptionInfo = {
          method: 'AES-256-GCM',
          keyFingerprint: 'abc123',
          nonce: [1, 2, 3],
          salt: [4, 5, 6],
        };

        expect(singletonService.isValidEncryptionInfo(validInfo)).toBe(true);
      });

      it('should reject info with missing method', () => {
        const invalidInfo = {
          keyFingerprint: 'abc123',
          nonce: [1, 2, 3],
          salt: [4, 5, 6],
        };

        expect(singletonService.isValidEncryptionInfo(invalidInfo)).toBe(false);
      });

      it('should reject info with missing keyFingerprint', () => {
        const invalidInfo = {
          method: 'AES-256-GCM',
          nonce: [1, 2, 3],
          salt: [4, 5, 6],
        };

        expect(singletonService.isValidEncryptionInfo(invalidInfo)).toBe(false);
      });

      it('should reject info with non-array nonce', () => {
        const invalidInfo = {
          method: 'AES-256-GCM',
          keyFingerprint: 'abc123',
          nonce: 'not-an-array',
          salt: [4, 5, 6],
        };

        expect(singletonService.isValidEncryptionInfo(invalidInfo)).toBe(false);
      });

      it('should reject info with non-array salt', () => {
        const invalidInfo = {
          method: 'AES-256-GCM',
          keyFingerprint: 'abc123',
          nonce: [1, 2, 3],
          salt: 'not-an-array',
        };

        expect(singletonService.isValidEncryptionInfo(invalidInfo)).toBe(false);
      });

      it('should reject null', () => {
        expect(singletonService.isValidEncryptionInfo(null)).toBe(false);
      });

      it('should reject undefined', () => {
        expect(singletonService.isValidEncryptionInfo(undefined)).toBe(false);
      });

      it('should reject non-object values', () => {
        expect(singletonService.isValidEncryptionInfo('string')).toBe(false);
        expect(singletonService.isValidEncryptionInfo(123)).toBe(false);
        expect(singletonService.isValidEncryptionInfo(true)).toBe(false);
      });
    });

    describe('createEncryptionMetadata', () => {
      it('should create metadata from encryption info', () => {
        const encryptionInfo: EncryptionInfo = {
          method: 'AES-256-GCM',
          keyFingerprint: 'abc123def456',
          nonce: [1, 2, 3],
          salt: [4, 5, 6],
        };

        const metadata = singletonService.createEncryptionMetadata(encryptionInfo);

        expect(metadata).toEqual({
          isEncrypted: true,
          encryptionMethod: 'AES-256-GCM',
          keyFingerprint: 'abc123def456',
        });
      });

      it('should always set isEncrypted to true', () => {
        const encryptionInfo: EncryptionInfo = {
          method: 'ChaCha20-Poly1305',
          keyFingerprint: 'xyz789',
          nonce: [1, 2, 3],
          salt: [4, 5, 6],
        };

        const metadata = singletonService.createEncryptionMetadata(encryptionInfo);

        expect(metadata.isEncrypted).toBe(true);
      });

      it('should not include nonce or salt in metadata', () => {
        const encryptionInfo: EncryptionInfo = {
          method: 'AES-256-GCM',
          keyFingerprint: 'abc123',
          nonce: [1, 2, 3],
          salt: [4, 5, 6],
        };

        const metadata = singletonService.createEncryptionMetadata(encryptionInfo);

        expect(metadata).not.toHaveProperty('nonce');
        expect(metadata).not.toHaveProperty('salt');
      });
    });
  });

  describe('Chunk-based Encryption Service', () => {
    describe('encryptFile', () => {
      const mockManifest: FileManifestForJs = {
        merkleRoot: 'root_hash_123',
        chunks: [
          {
            index: 0,
            hash: 'chunk_hash_0',
            size: 1024,
            encryptedHash: 'enc_hash_0',
            encryptedSize: 1040,
          },
          {
            index: 1,
            hash: 'chunk_hash_1',
            size: 1024,
            encryptedHash: 'enc_hash_1',
            encryptedSize: 1040,
          },
        ],
        encryptedKeyBundle: '{"key":"encrypted_aes_key"}',
      };

      it('should encrypt file for recipient with public key', async () => {
        mockInvoke.mockResolvedValueOnce(mockManifest);

        const result = await chunkEncryptionService.encryptFile(
          '/path/to/file.txt',
          'recipient_public_key_hex'
        );

        expect(mockInvoke).toHaveBeenCalledWith('encrypt_file_for_recipient', {
          filePath: '/path/to/file.txt',
          recipientPublicKey: 'recipient_public_key_hex',
        });
        expect(result).toEqual(mockManifest);
      });

      it('should encrypt file for self upload without recipient key', async () => {
        mockInvoke.mockResolvedValueOnce(mockManifest);

        const result = await chunkEncryptionService.encryptFile('/path/to/file.txt');

        expect(mockInvoke).toHaveBeenCalledWith('encrypt_file_for_self_upload', {
          filePath: '/path/to/file.txt',
        });
        expect(result).toEqual(mockManifest);
      });

      it('should return manifest with chunks', async () => {
        mockInvoke.mockResolvedValueOnce(mockManifest);

        const result = await chunkEncryptionService.encryptFile('/path/to/file.txt');

        expect(result.chunks).toHaveLength(2);
        expect(result.chunks[0]).toHaveProperty('index');
        expect(result.chunks[0]).toHaveProperty('hash');
        expect(result.chunks[0]).toHaveProperty('encryptedHash');
      });

      it('should include merkle root in manifest', async () => {
        mockInvoke.mockResolvedValueOnce(mockManifest);

        const result = await chunkEncryptionService.encryptFile('/path/to/file.txt');

        expect(result.merkleRoot).toBe('root_hash_123');
      });

      it('should include encrypted key bundle', async () => {
        mockInvoke.mockResolvedValueOnce(mockManifest);

        const result = await chunkEncryptionService.encryptFile('/path/to/file.txt');

        expect(result.encryptedKeyBundle).toBe('{"key":"encrypted_aes_key"}');
      });

      it('should handle encryption errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('File not found'));

        await expect(
          chunkEncryptionService.encryptFile('/invalid/path.txt')
        ).rejects.toThrow('File not found');
      });
    });

    describe('decryptFile', () => {
      const mockManifest: FileManifestForJs = {
        merkleRoot: 'root_hash_123',
        chunks: [
          {
            index: 0,
            hash: 'chunk_hash_0',
            size: 1024,
            encryptedHash: 'enc_hash_0',
            encryptedSize: 1040,
          },
        ],
        encryptedKeyBundle: '{"key":"encrypted_aes_key"}',
      };

      it('should decrypt file with manifest', async () => {
        mockInvoke.mockResolvedValueOnce(undefined);

        await chunkEncryptionService.decryptFile(
          mockManifest,
          '/path/to/output.txt'
        );

        expect(mockInvoke).toHaveBeenCalledWith('decrypt_and_reassemble_file', {
          manifestJs: mockManifest,
          outputPath: '/path/to/output.txt',
        });
      });

      it('should handle decryption errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('Invalid manifest'));

        await expect(
          chunkEncryptionService.decryptFile(mockManifest, '/path/to/output.txt')
        ).rejects.toThrow('Invalid manifest');
      });

      it('should handle missing chunks error', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('Chunks not found'));

        await expect(
          chunkEncryptionService.decryptFile(mockManifest, '/path/to/output.txt')
        ).rejects.toThrow('Chunks not found');
      });

      it('should complete without returning value', async () => {
        mockInvoke.mockResolvedValueOnce(undefined);

        const result = await chunkEncryptionService.decryptFile(
          mockManifest,
          '/path/to/output.txt'
        );

        expect(result).toBeUndefined();
      });
    });
  });
});