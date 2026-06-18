import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe('walletBackupService', () => {
  beforeEach(() => {
    vi.resetModules();
    mockInvoke.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('sendBackupEmail', () => {
    it('should invoke send_wallet_backup_email with correct payload', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockInvoke.mockResolvedValueOnce(undefined);

      const payload = {
        email: 'test@example.com',
        recoveryPhrase: 'alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu',
        walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
        privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
      };

      const result = await walletBackupService.sendBackupEmail(payload);

      expect(mockInvoke).toHaveBeenCalledOnce();
      const [, invokePayload] = mockInvoke.mock.calls[0] as [
        string,
        {
          email: string;
          walletAddress: string;
          encryptedBackup: {
            version: string;
            algorithm: string;
            kdf: string;
            iterations: number;
            salt: string;
            iv: string;
            ciphertext: string;
          };
          recoveryPhrase?: string;
          privateKey?: string;
        },
      ];

      expect(mockInvoke).toHaveBeenCalledWith('send_wallet_backup_email', {
        email: 'test@example.com',
        walletAddress: payload.walletAddress,
        encryptedBackup: invokePayload.encryptedBackup,
      });
      expect(invokePayload).not.toHaveProperty('recoveryPhrase');
      expect(invokePayload).not.toHaveProperty('privateKey');
      expect(invokePayload.encryptedBackup).toMatchObject({
        version: 'chiral-wallet-backup-v1',
        algorithm: 'AES-256-GCM',
        kdf: 'PBKDF2-SHA256',
        iterations: 210_000,
      });
      expect(invokePayload.encryptedBackup.salt).toEqual(expect.any(String));
      expect(invokePayload.encryptedBackup.iv).toEqual(expect.any(String));
      expect(invokePayload.encryptedBackup.ciphertext).toEqual(expect.any(String));
      expect(invokePayload.encryptedBackup.ciphertext).not.toContain(payload.recoveryPhrase);
      expect(invokePayload.encryptedBackup.ciphertext).not.toContain(payload.privateKey);
      expect(result.backupKey).toEqual(expect.any(String));
    });

    it('should return backup key on success', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockInvoke.mockResolvedValueOnce(undefined);

      const result = await walletBackupService.sendBackupEmail({
        email: 'test@example.com',
        recoveryPhrase: 'a b c d e f g h i j k l',
        walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
        privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
      });

      expect(result).toEqual({ backupKey: expect.any(String) });
    });

    it('should throw when invoke rejects with error message', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockInvoke.mockRejectedValueOnce('Invalid email address');

      await expect(
        walletBackupService.sendBackupEmail({
          email: 'bad',
          recoveryPhrase: 'a b c d e f g h i j k l',
          walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
          privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
        })
      ).rejects.toBe('Invalid email address');
    });

    it('should throw when invoke rejects with Error object', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockInvoke.mockRejectedValueOnce(new Error('Failed to reach email server'));

      await expect(
        walletBackupService.sendBackupEmail({
          email: 'test@example.com',
          recoveryPhrase: 'a b c d e f g h i j k l',
          walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
          privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
        })
      ).rejects.toThrow('Failed to reach email server');
    });
  });

  describe('formatError', () => {
    it('should format Error objects', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');
      const result = walletBackupService.formatError(new Error('Something failed'));
      expect(result).toBe('Something failed');
    });

    it('should format string errors', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');
      expect(walletBackupService.formatError('Email server error')).toBe('Email server error');
    });

    it('should return default message for non-Error values', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');
      expect(walletBackupService.formatError(null)).toBe('Failed to send wallet backup email');
      expect(walletBackupService.formatError(undefined)).toBe('Failed to send wallet backup email');
      expect(walletBackupService.formatError(42)).toBe('Failed to send wallet backup email');
    });

    it('should return default message for Error with empty message', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');
      expect(walletBackupService.formatError(new Error('   '))).toBe('Failed to send wallet backup email');
    });
  });
});
