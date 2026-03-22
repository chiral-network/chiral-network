import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

describe('walletBackupService', () => {
  beforeEach(() => {
    vi.resetModules();
    mockFetch.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('sendBackupEmail', () => {
    it('should POST to /api/wallet/backup-email with correct payload', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true }),
      });

      const payload = {
        email: 'test@example.com',
        recoveryPhrase: 'alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu',
        walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
        privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
      };

      const result = await walletBackupService.sendBackupEmail(payload);

      expect(mockFetch).toHaveBeenCalledOnce();
      const [url, init] = mockFetch.mock.calls[0];
      expect(url).toContain('/api/wallet/backup-email');
      expect(init.method).toBe('POST');
      expect(init.headers['Content-Type']).toBe('application/json');

      const body = JSON.parse(init.body);
      expect(body.email).toBe('test@example.com');
      expect(body.recoveryPhrase).toBe(payload.recoveryPhrase);
      expect(body.walletAddress).toBe(payload.walletAddress);
      expect(body.privateKey).toBe(payload.privateKey);
      expect(result).toBe(true);
    });

    it('should return true when response is ok', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true }),
      });

      const result = await walletBackupService.sendBackupEmail({
        email: 'test@example.com',
        recoveryPhrase: 'a b c d e f g h i j k l',
        walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
        privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
      });

      expect(result).toBe(true);
    });

    it('should throw with error text on HTTP failure', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
        text: async () => 'Invalid email address',
      });

      await expect(
        walletBackupService.sendBackupEmail({
          email: 'bad',
          recoveryPhrase: 'a b c d e f g h i j k l',
          walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
          privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
        })
      ).rejects.toThrow('Invalid email address');
    });

    it('should throw with HTTP status when error text is empty', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 503,
        text: async () => '',
      });

      await expect(
        walletBackupService.sendBackupEmail({
          email: 'test@example.com',
          recoveryPhrase: 'a b c d e f g h i j k l',
          walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
          privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
        })
      ).rejects.toThrow('HTTP 503');
    });

    it('should handle JSON parse failure gracefully', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => { throw new Error('not json'); },
      });

      const result = await walletBackupService.sendBackupEmail({
        email: 'test@example.com',
        recoveryPhrase: 'a b c d e f g h i j k l',
        walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
        privateKey: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
      });

      expect(result).toBe(false);
    });
  });

  describe('formatError', () => {
    it('should format Error objects', async () => {
      const { walletBackupService } = await import('$lib/services/walletBackupService');
      const result = walletBackupService.formatError(new Error('Something failed'));
      expect(result).toBe('Something failed');
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
