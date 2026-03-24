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
      expect(mockInvoke).toHaveBeenCalledWith('send_wallet_backup_email', {
        email: 'test@example.com',
        recoveryPhrase: payload.recoveryPhrase,
        walletAddress: payload.walletAddress,
        privateKey: payload.privateKey,
      });
      expect(result).toBe(true);
    });

    it('should return true on success', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockInvoke.mockResolvedValueOnce(undefined);

      const result = await walletBackupService.sendBackupEmail({
        email: 'test@example.com',
        recoveryPhrase: 'a b c d e f g h i j k l',
        walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
        privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
      });

      expect(result).toBe(true);
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
