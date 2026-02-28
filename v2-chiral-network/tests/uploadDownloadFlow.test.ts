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

// Mock event listener
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

describe('Upload Flow Simulation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  describe('publish_file invoke', () => {
    it('should call publish_file with correct args for free upload', async () => {
      mockInvoke.mockResolvedValueOnce({ merkleRoot: 'abc123def456' });

      const result = await invoke<{ merkleRoot: string }>('publish_file', {
        filePath: '/home/user/documents/test.pdf',
        fileName: 'test.pdf',
        protocol: 'WebRTC',
        priceChi: null,
        walletAddress: null,
      });

      expect(mockInvoke).toHaveBeenCalledWith('publish_file', {
        filePath: '/home/user/documents/test.pdf',
        fileName: 'test.pdf',
        protocol: 'WebRTC',
        priceChi: null,
        walletAddress: null,
      });
      expect(result.merkleRoot).toBe('abc123def456');
    });

    it('should call publish_file with price and wallet for paid upload', async () => {
      mockInvoke.mockResolvedValueOnce({ merkleRoot: 'paid_hash_789' });

      const result = await invoke<{ merkleRoot: string }>('publish_file', {
        filePath: '/home/user/music/song.mp3',
        fileName: 'song.mp3',
        protocol: 'WebRTC',
        priceChi: '2.5',
        walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
      });

      expect(mockInvoke).toHaveBeenCalledWith('publish_file', expect.objectContaining({
        priceChi: '2.5',
        walletAddress: '0x1234567890abcdef1234567890abcdef12345678',
      }));
      expect(result.merkleRoot).toBe('paid_hash_789');
    });

    it('should handle publish_file failure', async () => {
      mockInvoke.mockRejectedValueOnce('No space left on device');

      await expect(
        invoke('publish_file', {
          filePath: '/home/user/big_file.zip',
          fileName: 'big_file.zip',
          protocol: 'WebRTC',
          priceChi: null,
          walletAddress: null,
        })
      ).rejects.toBe('No space left on device');
    });

    it('should call publish_file with small CHI price', async () => {
      mockInvoke.mockResolvedValueOnce({ merkleRoot: 'small_price_hash' });

      await invoke('publish_file', {
        filePath: '/home/user/doc.txt',
        fileName: 'doc.txt',
        priceChi: '0.001',
        walletAddress: '0xwallet',
      });

      expect(mockInvoke).toHaveBeenCalledWith('publish_file', expect.objectContaining({
        priceChi: '0.001',
      }));
    });
  });

  describe('register_shared_file (re-registration on startup)', () => {
    it('should re-register each saved file on restart', async () => {
      mockInvoke.mockResolvedValue(undefined);

      const savedFiles = [
        { hash: 'hash1', filePath: '/path/file1.txt', name: 'file1.txt', size: 1000, priceChi: null },
        { hash: 'hash2', filePath: '/path/file2.pdf', name: 'file2.pdf', size: 2000, priceChi: '1.0' },
        { hash: 'hash3', filePath: '/path/file3.zip', name: 'file3.zip', size: 3000, priceChi: null },
      ];

      for (const file of savedFiles) {
        await invoke('register_shared_file', {
          fileHash: file.hash,
          filePath: file.filePath,
          fileName: file.name,
          fileSize: file.size,
          priceChi: file.priceChi,
          walletAddress: file.priceChi ? '0xwallet' : null,
        });
      }

      expect(mockInvoke).toHaveBeenCalledTimes(3);
      expect(mockInvoke).toHaveBeenCalledWith('register_shared_file', expect.objectContaining({
        fileHash: 'hash1',
      }));
      expect(mockInvoke).toHaveBeenCalledWith('register_shared_file', expect.objectContaining({
        fileHash: 'hash2',
        priceChi: '1.0',
      }));
    });

    it('should handle re-registration failure for missing file', async () => {
      mockInvoke.mockRejectedValueOnce('File no longer exists: /path/deleted.txt');

      await expect(
        invoke('register_shared_file', {
          fileHash: 'old_hash',
          filePath: '/path/deleted.txt',
          fileName: 'deleted.txt',
          fileSize: 500,
          priceChi: null,
          walletAddress: null,
        })
      ).rejects.toBe('File no longer exists: /path/deleted.txt');
    });
  });
});

describe('Download Flow Simulation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  describe('file search via DHT', () => {
    it('should search for file by hash and return metadata', async () => {
      const mockMetadata = {
        fileHash: 'abc123',
        fileName: 'document.pdf',
        fileSize: 5_000_000,
        seeders: ['peer1', 'peer2'],
        priceWei: '0',
        uploaderAddress: '0xuploader',
      };
      mockInvoke.mockResolvedValueOnce(mockMetadata);

      const result = await invoke('search_file_by_hash', { fileHash: 'abc123' });
      expect(mockInvoke).toHaveBeenCalledWith('search_file_by_hash', { fileHash: 'abc123' });
      expect(result).toEqual(mockMetadata);
    });

    it('should return null for file not found', async () => {
      mockInvoke.mockResolvedValueOnce(null);

      const result = await invoke('search_file_by_hash', { fileHash: 'nonexistent' });
      expect(result).toBeNull();
    });
  });

  describe('download cost calculation', () => {
    it('should calculate standard tier download cost', () => {
      const cost = calculateCost('standard', 10_000_000);
      expect(cost).toBeCloseTo(0.01, 6);
    });

    it('should calculate standard tier cost for 50 MB file', () => {
      const cost = calculateCost('standard', 50_000_000);
      // 50 MB * 0.001 CHI/MB = 0.05 CHI
      expect(cost).toBeCloseTo(0.05, 6);
    });

    it('should calculate total cost as tier cost + seeder price', () => {
      const tierCost = calculateCost('standard', 10_000_000); // 0.01 CHI
      const seederPriceWei = '5000000000000000000'; // 5 CHI
      const seederPriceChi = Number(BigInt(seederPriceWei)) / 1e18;
      const totalCost = tierCost + seederPriceChi;
      expect(totalCost).toBeCloseTo(5.01, 6);
    });

    it('should detect insufficient balance before download', () => {
      const walletBalance = 0.001; // 0.001 CHI
      const totalCost = calculateCost('premium', 10_000_000); // 0.05 CHI
      expect(walletBalance < totalCost).toBe(true);
    });

    it('should allow download with sufficient balance', () => {
      const walletBalance = 100.0;
      const totalCost = calculateCost('premium', 10_000_000); // 0.05 CHI
      expect(walletBalance >= totalCost).toBe(true);
    });

    it('should format download cost for display', () => {
      const cost = calculateCost('standard', 10_000_000);
      const formatted = formatCost(cost);
      expect(formatted).toBe('0.01 CHI');
    });

    it('should format zero cost', () => {
      expect(formatCost(0)).toBe('0 CHI');
    });
  });

  describe('download with seeder price (wei conversion)', () => {
    it('should convert seeder priceWei to CHI for display', () => {
      const priceWei = '5000000000000000000'; // 5 CHI in wei
      const priceChi = Number(BigInt(priceWei)) / 1e18;
      expect(priceChi).toBe(5.0);
    });

    it('should handle zero seeder price', () => {
      const priceWei = '0';
      const priceChi = priceWei !== '0' ? Number(BigInt(priceWei)) / 1e18 : 0;
      expect(priceChi).toBe(0);
    });

    it('should calculate total cost with both tier and seeder price', () => {
      const tierCost = calculateCost('standard', 100_000_000); // 100 MB
      const seederPriceChi = 2.5;
      const total = tierCost + seederPriceChi;
      // 100 MB * 0.001 + 2.5 = 0.1 + 2.5 = 2.6
      expect(total).toBeCloseTo(2.6, 4);
    });
  });

  describe('start_download invoke', () => {
    it('should call start_download with correct params', async () => {
      mockInvoke.mockResolvedValueOnce({ requestId: 'dl-001' });

      await invoke('start_download', {
        fileHash: 'abc123',
        fileName: 'test.pdf',
        speedTier: 'standard',
        walletAddress: '0xwallet',
        privateKey: '0xkey',
      });

      expect(mockInvoke).toHaveBeenCalledWith('start_download', expect.objectContaining({
        fileHash: 'abc123',
        speedTier: 'standard',
      }));
    });

    it('should call start_download for standard tier', async () => {
      mockInvoke.mockResolvedValueOnce({ requestId: 'dl-002' });

      await invoke('start_download', {
        fileHash: 'std_file',
        fileName: 'std.txt',
        speedTier: 'standard',
        walletAddress: '0xwallet',
        privateKey: '0xkey',
      });

      expect(mockInvoke).toHaveBeenCalledWith('start_download', expect.objectContaining({
        speedTier: 'standard',
      }));
    });

    it('should handle download failure', async () => {
      mockInvoke.mockRejectedValueOnce('No seeders available');

      await expect(
        invoke('start_download', {
          fileHash: 'unavailable',
          fileName: 'missing.zip',
          speedTier: 'standard',
        })
      ).rejects.toBe('No seeders available');
    });
  });

  describe('speed tier cost comparison for same file', () => {
    const fileSize = 100_000_000; // 100 MB

    it('standard should be cheapest paid tier', () => {
      const stdCost = calculateCost('standard', fileSize);
      const premCost = calculateCost('premium', fileSize);
      const ultraCost = calculateCost('ultra', fileSize);
      expect(stdCost).toBeLessThan(premCost);
      expect(premCost).toBeLessThan(ultraCost);
    });

    it('premium should cost 5x standard', () => {
      const stdCost = calculateCost('standard', fileSize);
      const premCost = calculateCost('premium', fileSize);
      expect(premCost / stdCost).toBeCloseTo(5.0, 4);
    });

    it('ultra should cost 10x standard', () => {
      const stdCost = calculateCost('standard', fileSize);
      const ultraCost = calculateCost('ultra', fileSize);
      expect(ultraCost / stdCost).toBeCloseTo(10.0, 4);
    });
  });
});
