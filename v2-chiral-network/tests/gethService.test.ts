import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

const mockInvoke = vi.mocked(invoke);

// Mock @tauri-apps/api/event
const mockUnlisten = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(mockUnlisten),
}));

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

describe('gethService', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe('stores', () => {
    it('gethStatus should initialize as null', async () => {
      const { gethStatus } = await import('$lib/services/gethService');
      expect(get(gethStatus)).toBeNull();
    });

    it('miningStatus should initialize as null', async () => {
      const { miningStatus } = await import('$lib/services/gethService');
      expect(get(miningStatus)).toBeNull();
    });

    it('downloadProgress should initialize as null', async () => {
      const { downloadProgress } = await import('$lib/services/gethService');
      expect(get(downloadProgress)).toBeNull();
    });

    it('isDownloading should initialize as false', async () => {
      const { isDownloading } = await import('$lib/services/gethService');
      expect(get(isDownloading)).toBe(false);
    });
  });

  describe('isInstalled', () => {
    it('should invoke is_geth_installed command', async () => {
      mockInvoke.mockResolvedValueOnce(true);
      const { gethService } = await import('$lib/services/gethService');
      const result = await gethService.isInstalled();
      expect(mockInvoke).toHaveBeenCalledWith('is_geth_installed');
      expect(result).toBe(true);
    });

    it('should return false on error', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('not found'));
      const { gethService } = await import('$lib/services/gethService');
      const result = await gethService.isInstalled();
      expect(result).toBe(false);
    });
  });

  describe('start', () => {
    it('should invoke start_geth with miner address', async () => {
      mockInvoke.mockResolvedValue(undefined);
      const { gethService } = await import('$lib/services/gethService');
      await gethService.start('0xABC');
      expect(mockInvoke).toHaveBeenCalledWith('start_geth', { minerAddress: '0xABC' });
    });

    it('should invoke start_geth without miner address', async () => {
      mockInvoke.mockResolvedValue(undefined);
      const { gethService } = await import('$lib/services/gethService');
      await gethService.start();
      expect(mockInvoke).toHaveBeenCalledWith('start_geth', { minerAddress: undefined });
    });
  });

  describe('stop', () => {
    it('should invoke stop_geth and clear stores', async () => {
      mockInvoke.mockResolvedValue(undefined);
      const { gethService, gethStatus, miningStatus } = await import('$lib/services/gethService');

      // Set some values first
      gethStatus.set({ installed: true, running: true, syncing: false, currentBlock: 100, highestBlock: 100, peerCount: 2, chainId: 13337 });
      miningStatus.set({ mining: true, hashRate: 1000, minerAddress: '0xABC' });

      await gethService.stop();

      expect(mockInvoke).toHaveBeenCalledWith('stop_geth');
      expect(get(gethStatus)).toBeNull();
      expect(get(miningStatus)).toBeNull();
    });
  });

  describe('getStatus', () => {
    it('should invoke get_geth_status and update store', async () => {
      const mockStatus = {
        installed: true,
        running: true,
        syncing: false,
        currentBlock: 42,
        highestBlock: 42,
        peerCount: 3,
        chainId: 13337,
      };
      mockInvoke.mockResolvedValueOnce(mockStatus);
      const { gethService, gethStatus } = await import('$lib/services/gethService');

      const result = await gethService.getStatus();

      expect(mockInvoke).toHaveBeenCalledWith('get_geth_status');
      expect(result).toEqual(mockStatus);
      expect(get(gethStatus)).toEqual(mockStatus);
    });
  });

  describe('mining', () => {
    it('startMining should invoke with thread count', async () => {
      const miningResult = { mining: true, hashRate: 500, minerAddress: '0xABC' };
      mockInvoke
        .mockResolvedValueOnce(undefined) // start_mining
        .mockResolvedValueOnce(miningResult); // get_mining_status
      const { gethService } = await import('$lib/services/gethService');

      await gethService.startMining(4);

      expect(mockInvoke).toHaveBeenCalledWith('start_mining', { threads: 4 });
      expect(mockInvoke).toHaveBeenCalledWith('get_mining_status');
    });

    it('startMining should default to 1 thread', async () => {
      mockInvoke
        .mockResolvedValueOnce(undefined)
        .mockResolvedValueOnce({ mining: true, hashRate: 100, minerAddress: null });
      const { gethService } = await import('$lib/services/gethService');

      await gethService.startMining();

      expect(mockInvoke).toHaveBeenCalledWith('start_mining', { threads: 1 });
    });

    it('stopMining should invoke and refresh mining status', async () => {
      mockInvoke
        .mockResolvedValueOnce(undefined) // stop_mining
        .mockResolvedValueOnce({ mining: false, hashRate: 0, minerAddress: null }); // get_mining_status
      const { gethService, miningStatus } = await import('$lib/services/gethService');

      await gethService.stopMining();

      expect(mockInvoke).toHaveBeenCalledWith('stop_mining');
      expect(get(miningStatus)?.mining).toBe(false);
    });

    it('getMiningStatus should update store', async () => {
      const status = { mining: true, hashRate: 2500, minerAddress: '0xDEF' };
      mockInvoke.mockResolvedValueOnce(status);
      const { gethService, miningStatus } = await import('$lib/services/gethService');

      const result = await gethService.getMiningStatus();

      expect(result).toEqual(status);
      expect(get(miningStatus)).toEqual(status);
    });
  });

  describe('setMinerAddress', () => {
    it('should invoke set_miner_address', async () => {
      mockInvoke.mockResolvedValueOnce(undefined);
      const { gethService } = await import('$lib/services/gethService');

      await gethService.setMinerAddress('0x1234');

      expect(mockInvoke).toHaveBeenCalledWith('set_miner_address', { address: '0x1234' });
    });
  });

  describe('getChainId', () => {
    it('should return chain ID from invoke', async () => {
      mockInvoke.mockResolvedValueOnce(13337);
      const { gethService } = await import('$lib/services/gethService');

      const result = await gethService.getChainId();

      expect(mockInvoke).toHaveBeenCalledWith('get_chain_id');
      expect(result).toBe(13337);
    });
  });

  describe('statusPolling', () => {
    it('should poll at specified interval', async () => {
      const mockStatus = { installed: true, running: true, syncing: false, currentBlock: 1, highestBlock: 1, peerCount: 0, chainId: 13337 };
      const mockMining = { mining: false, hashRate: 0, minerAddress: null };
      mockInvoke.mockResolvedValue(mockStatus);
      // Override for mining status calls
      mockInvoke
        .mockResolvedValueOnce(mockStatus) // initial getStatus
        .mockResolvedValueOnce(mockMining)  // initial getMiningStatus
        .mockResolvedValueOnce(mockStatus) // poll getStatus
        .mockResolvedValueOnce(mockMining); // poll getMiningStatus

      const { gethService } = await import('$lib/services/gethService');

      gethService.startStatusPolling(1000);

      // Initial fetch happens immediately
      await vi.advanceTimersByTimeAsync(0);
      expect(mockInvoke).toHaveBeenCalledWith('get_geth_status');
      expect(mockInvoke).toHaveBeenCalledWith('get_mining_status');

      // Advance to first poll
      const callsBefore = mockInvoke.mock.calls.length;
      await vi.advanceTimersByTimeAsync(1000);
      expect(mockInvoke.mock.calls.length).toBeGreaterThan(callsBefore);

      gethService.stopStatusPolling();
    });

    it('stopStatusPolling should clear interval', async () => {
      mockInvoke.mockResolvedValue({});
      const { gethService } = await import('$lib/services/gethService');

      gethService.startStatusPolling(1000);
      gethService.stopStatusPolling();

      const callCount = mockInvoke.mock.calls.length;
      await vi.advanceTimersByTimeAsync(5000);
      // After initial calls, no more calls should happen (interval is cleared)
      // Allow for the initial async calls that may still be pending
      expect(mockInvoke.mock.calls.length).toBeLessThanOrEqual(callCount + 2);
    });
  });

  describe('initialize', () => {
    it('should start polling when geth is installed and running', async () => {
      mockInvoke
        .mockResolvedValueOnce(true) // is_geth_installed
        .mockResolvedValueOnce({ installed: true, running: true, syncing: false, currentBlock: 10, highestBlock: 10, peerCount: 1, chainId: 13337 }) // get_geth_status
        .mockResolvedValue({}); // subsequent polling calls

      const { gethService } = await import('$lib/services/gethService');
      await gethService.initialize();

      expect(mockInvoke).toHaveBeenCalledWith('is_geth_installed');
      expect(mockInvoke).toHaveBeenCalledWith('get_geth_status');

      gethService.stopStatusPolling();
    });

    it('should not poll when geth is not installed', async () => {
      mockInvoke.mockResolvedValueOnce(false); // is_geth_installed
      const { gethService } = await import('$lib/services/gethService');

      await gethService.initialize();

      expect(mockInvoke).toHaveBeenCalledWith('is_geth_installed');
      expect(mockInvoke).not.toHaveBeenCalledWith('get_geth_status');
    });

    it('should not poll when geth is installed but not running', async () => {
      mockInvoke
        .mockResolvedValueOnce(true) // is_geth_installed
        .mockResolvedValueOnce({ installed: true, running: false, syncing: false, currentBlock: 0, highestBlock: 0, peerCount: 0, chainId: 13337 }); // get_geth_status

      const { gethService } = await import('$lib/services/gethService');
      await gethService.initialize();

      // Should not start polling - only isInstalled and getStatus were called
      const callCount = mockInvoke.mock.calls.length;
      await vi.advanceTimersByTimeAsync(10000);
      // No additional polling calls
      expect(mockInvoke.mock.calls.length).toBe(callCount);
    });

    it('should handle initialization errors gracefully', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('init failed'));
      const { gethService } = await import('$lib/services/gethService');

      // Should not throw
      await expect(gethService.initialize()).resolves.toBeUndefined();
    });
  });

  describe('download', () => {
    it('should set isDownloading during download', async () => {
      const { listen } = await import('@tauri-apps/api/event');
      vi.mocked(listen).mockResolvedValueOnce(mockUnlisten);
      mockInvoke.mockResolvedValueOnce(undefined); // download_geth

      const { gethService, isDownloading, downloadProgress } = await import('$lib/services/gethService');

      const downloadPromise = gethService.download();

      // isDownloading is set synchronously before awaits
      expect(get(isDownloading)).toBe(true);
      expect(get(downloadProgress)).toEqual({
        downloaded: 0,
        total: 0,
        percentage: 0,
        status: 'Starting download...',
      });

      await downloadPromise;

      expect(get(isDownloading)).toBe(false);
      expect(get(downloadProgress)?.percentage).toBe(100);
      expect(get(downloadProgress)?.status).toBe('Installation complete!');
      expect(mockUnlisten).toHaveBeenCalled();
    });

    it('should reset isDownloading on error', async () => {
      const { listen } = await import('@tauri-apps/api/event');
      vi.mocked(listen).mockResolvedValueOnce(mockUnlisten);
      mockInvoke.mockRejectedValueOnce(new Error('download failed'));

      const { gethService, isDownloading } = await import('$lib/services/gethService');

      await expect(gethService.download()).rejects.toThrow('download failed');

      expect(get(isDownloading)).toBe(false);
      expect(mockUnlisten).toHaveBeenCalled();
    });
  });
});
