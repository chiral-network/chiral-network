import { describe, it, expect, beforeEach, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { calculateCost, formatCost } from '$lib/speedTiers';

const mockInvoke = vi.mocked(invoke);

// Mock logger
vi.mock('$lib/logger', () => ({
  logger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
    ok: vi.fn(),
  }),
}));

describe('uploadDownloadFlow', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('publish_file', () => {
    it('should invoke publish_file with correct params', async () => {
      mockInvoke.mockResolvedValueOnce({ hash: 'abc123', name: 'test.txt', size: 1024 });

      const result = await invoke('publish_file', {
        filePath: '/tmp/test.txt',
        priceChi: null,
        walletAddress: null,
      });

      expect(mockInvoke).toHaveBeenCalledWith('publish_file', {
        filePath: '/tmp/test.txt',
        priceChi: null,
        walletAddress: null,
      });
      expect(result).toEqual({ hash: 'abc123', name: 'test.txt', size: 1024 });
    });

    it('should invoke publish_file with price', async () => {
      mockInvoke.mockResolvedValueOnce({ hash: 'paid123', name: 'premium.zip', size: 5000 });

      await invoke('publish_file', {
        filePath: '/tmp/premium.zip',
        priceChi: '0.5',
        walletAddress: '0xwallet',
      });

      expect(mockInvoke).toHaveBeenCalledWith('publish_file', expect.objectContaining({
        priceChi: '0.5',
        walletAddress: '0xwallet',
      }));
    });

    it('should handle publish failure', async () => {
      mockInvoke.mockRejectedValueOnce('File not found');

      await expect(
        invoke('publish_file', { filePath: '/nonexistent', priceChi: null, walletAddress: null })
      ).rejects.toBe('File not found');
    });
  });

  describe('search_file', () => {
    it('should return search results', async () => {
      const mockResults = [
        { hash: 'abc123', fileName: 'test.txt', fileSize: 1024, seeders: [] },
      ];
      mockInvoke.mockResolvedValueOnce(mockResults);

      const result = await invoke('search_file', { fileHash: 'abc123' });
      expect(result).toEqual(mockResults);
    });

    it('should return empty array when no results', async () => {
      mockInvoke.mockResolvedValueOnce([]);

      const result = await invoke('search_file', { fileHash: 'nonexistent' });
      expect(result).toEqual([]);
    });

    it('should handle search failure', async () => {
      mockInvoke.mockRejectedValueOnce('DHT not running');

      await expect(
        invoke('search_file', { fileHash: 'abc' })
      ).rejects.toBe('DHT not running');
    });
  });

  describe('download cost calculation', () => {
    it('should calculate cost for 10 MB file', () => {
      const cost = calculateCost(10_000_000);
      expect(cost).toBeCloseTo(0.01, 6);
    });

    it('should calculate cost for 100 MB file', () => {
      const cost = calculateCost(100_000_000);
      expect(cost).toBeCloseTo(0.1, 6);
    });

    it('should calculate cost for 1 GB file', () => {
      const cost = calculateCost(1_000_000_000);
      expect(cost).toBeCloseTo(1.0, 6);
    });

    it('should format cost correctly', () => {
      expect(formatCost(0.01)).toBe('0.01 CHI');
      expect(formatCost(0)).toBe('Free');
    });

    it('should handle zero-byte file', () => {
      expect(calculateCost(0)).toBe(0);
    });
  });

  describe('start_download invoke', () => {
    it('should call start_download with correct params', async () => {
      mockInvoke.mockResolvedValueOnce({ requestId: 'dl-001' });

      await invoke('start_download', {
        fileHash: 'abc123',
        fileName: 'test.pdf',
        walletAddress: '0xwallet',
        privateKey: '0xkey',
      });

      expect(mockInvoke).toHaveBeenCalledWith('start_download', expect.objectContaining({
        fileHash: 'abc123',
      }));
    });

    it('should handle download failure', async () => {
      mockInvoke.mockRejectedValueOnce('No seeders available');

      await expect(
        invoke('start_download', {
          fileHash: 'unavailable',
          fileName: 'missing.zip',
        })
      ).rejects.toBe('No seeders available');
    });
  });
});
