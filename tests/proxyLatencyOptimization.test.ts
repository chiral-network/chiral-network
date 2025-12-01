import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { ProxyLatencyOptimizationService } from '../src/lib/services/proxyLatencyOptimization';

// Mock Tauri invoke
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

describe('proxyLatencyOptimization.ts', () => {
  const mockInvoke = vi.mocked(invoke);

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('Tauri Availability', () => {
    it('should detect when Tauri is available', async () => {
      mockInvoke.mockResolvedValueOnce(true);

      const isAvailable = await ProxyLatencyOptimizationService.isTauriAvailable();

      expect(isAvailable).toBe(true);
      expect(mockInvoke).toHaveBeenCalledWith('get_proxy_optimization_status');
    });

    it('should detect when Tauri is unavailable', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Tauri not available'));

      const isAvailable = await ProxyLatencyOptimizationService.isTauriAvailable();

      expect(isAvailable).toBe(false);
    });

    it('should log warning when Tauri is unavailable', async () => {
      const consoleSpy = vi.spyOn(console, 'warn');
      mockInvoke.mockRejectedValueOnce(new Error('No Tauri API'));

      await ProxyLatencyOptimizationService.isTauriAvailable();

      expect(consoleSpy).toHaveBeenCalledWith(
        'Tauri API not available:',
        expect.any(Error)
      );
    });
  });

  describe('updateProxyLatency', () => {
    it('should update proxy latency successfully', async () => {
      mockInvoke.mockResolvedValueOnce(undefined);

      await ProxyLatencyOptimizationService.updateProxyLatency('proxy-123', 50);

      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-123',
        latencyMs: 50,
      });
    });

    it('should update proxy with undefined latency', async () => {
      mockInvoke.mockResolvedValueOnce(undefined);

      await ProxyLatencyOptimizationService.updateProxyLatency('proxy-123', undefined);

      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-123',
        latencyMs: undefined,
      });
    });

    it('should handle update errors', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Update failed'));

      await expect(
        ProxyLatencyOptimizationService.updateProxyLatency('proxy-123', 50)
      ).rejects.toThrow('Failed to update proxy latency: Error: Update failed');
    });

    it('should handle zero latency', async () => {
      mockInvoke.mockResolvedValueOnce(undefined);

      await ProxyLatencyOptimizationService.updateProxyLatency('proxy-123', 0);

      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-123',
        latencyMs: 0,
      });
    });

    it('should handle very high latency values', async () => {
      mockInvoke.mockResolvedValueOnce(undefined);

      await ProxyLatencyOptimizationService.updateProxyLatency('proxy-123', 9999);

      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-123',
        latencyMs: 9999,
      });
    });
  });

  describe('getOptimizationStatus', () => {
    it('should get optimization status when enabled', async () => {
      mockInvoke.mockResolvedValueOnce(true);

      const status = await ProxyLatencyOptimizationService.getOptimizationStatus();

      expect(status).toBe(true);
      expect(mockInvoke).toHaveBeenCalledWith('get_proxy_optimization_status');
    });

    it('should get optimization status when disabled', async () => {
      mockInvoke.mockResolvedValueOnce(false);

      const status = await ProxyLatencyOptimizationService.getOptimizationStatus();

      expect(status).toBe(false);
    });

    it('should handle status query errors', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Query failed'));

      await expect(
        ProxyLatencyOptimizationService.getOptimizationStatus()
      ).rejects.toThrow('Failed to get optimization status: Error: Query failed');
    });
  });

  describe('startLatencyMonitoring', () => {
    it('should monitor online proxies with latency', async () => {
      mockInvoke.mockResolvedValueOnce(true); // isTauriAvailable
      mockInvoke.mockResolvedValue(undefined); // updateProxyLatency calls

      const proxies = [
        { id: 'proxy-1', status: 'online', latency: 50 },
        { id: 'proxy-2', status: 'online', latency: 75 },
      ];

      await ProxyLatencyOptimizationService.startLatencyMonitoring(proxies);

      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-1',
        latencyMs: 50,
      });
      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-2',
        latencyMs: 75,
      });
    });

    it('should update offline proxies with undefined latency', async () => {
      mockInvoke.mockResolvedValueOnce(true); // isTauriAvailable
      mockInvoke.mockResolvedValue(undefined);

      const proxies = [
        { id: 'proxy-1', status: 'offline' },
        { id: 'proxy-2', status: 'error' },
      ];

      await ProxyLatencyOptimizationService.startLatencyMonitoring(proxies);

      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-1',
        latencyMs: undefined,
      });
      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-2',
        latencyMs: undefined,
      });
    });

    it('should skip monitoring when Tauri is unavailable', async () => {
      const consoleSpy = vi.spyOn(console, 'warn');
      mockInvoke.mockRejectedValueOnce(new Error('No Tauri')); // isTauriAvailable fails

      const proxies = [{ id: 'proxy-1', status: 'online', latency: 50 }];

      await ProxyLatencyOptimizationService.startLatencyMonitoring(proxies);

      expect(consoleSpy).toHaveBeenCalledWith(
        'Tauri API not available, skipping latency monitoring'
      );
    });

    it('should handle empty proxy list', async () => {
      mockInvoke.mockResolvedValueOnce(true); // isTauriAvailable

      await ProxyLatencyOptimizationService.startLatencyMonitoring([]);

      // Should only call isTauriAvailable
      expect(mockInvoke).toHaveBeenCalledTimes(1);
    });

    it('should continue monitoring even if one proxy update fails', async () => {
      const consoleSpy = vi.spyOn(console, 'warn');
      mockInvoke.mockResolvedValueOnce(true); // isTauriAvailable
      mockInvoke.mockRejectedValueOnce(new Error('Update failed')); // First proxy fails
      mockInvoke.mockResolvedValueOnce(undefined); // Second proxy succeeds

      const proxies = [
        { id: 'proxy-1', status: 'online', latency: 50 },
        { id: 'proxy-2', status: 'online', latency: 75 },
      ];

      await ProxyLatencyOptimizationService.startLatencyMonitoring(proxies);

      expect(consoleSpy).toHaveBeenCalledWith(
        'Failed to update latency for proxy proxy-1:',
        expect.any(Error)
      );
      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-2',
        latencyMs: 75,
      });
    });

    it('should handle mixed proxy statuses', async () => {
      mockInvoke.mockResolvedValueOnce(true); // isTauriAvailable
      mockInvoke.mockResolvedValue(undefined);

      const proxies = [
        { id: 'proxy-1', status: 'online', latency: 50 },
        { id: 'proxy-2', status: 'offline' },
        { id: 'proxy-3', status: 'online', latency: 100 },
        { id: 'proxy-4', status: 'connecting' },
      ];

      await ProxyLatencyOptimizationService.startLatencyMonitoring(proxies);

      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-1',
        latencyMs: 50,
      });
      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-2',
        latencyMs: undefined,
      });
      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-3',
        latencyMs: 100,
      });
      expect(mockInvoke).toHaveBeenCalledWith('update_proxy_latency', {
        proxyId: 'proxy-4',
        latencyMs: undefined,
      });
    });
  });

  describe('getOptimizationStatusMessage', () => {
    it('should return enabled message when optimization is active', async () => {
      mockInvoke.mockResolvedValueOnce(true); // isTauriAvailable
      mockInvoke.mockResolvedValueOnce(true); // getOptimizationStatus

      const message = await ProxyLatencyOptimizationService.getOptimizationStatusMessage();

      expect(message).toBe('✅ Proxy latency optimization enabled');
    });

    it('should return unavailable message when no optimal proxies', async () => {
      mockInvoke.mockResolvedValueOnce(true); // isTauriAvailable
      mockInvoke.mockResolvedValueOnce(false); // getOptimizationStatus

      const message = await ProxyLatencyOptimizationService.getOptimizationStatusMessage();

      expect(message).toBe('⚠️ No optimal proxies available');
    });

    it('should return browser mode message when Tauri unavailable', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('No Tauri'));

      const message = await ProxyLatencyOptimizationService.getOptimizationStatusMessage();

      expect(message).toBe('⚠️ Running in browser mode - Tauri API unavailable');
    });

    it('should return error message on failure', async () => {
      mockInvoke.mockResolvedValueOnce(true); // isTauriAvailable
      mockInvoke.mockRejectedValueOnce(new Error('Status query failed'));

      const message = await ProxyLatencyOptimizationService.getOptimizationStatusMessage();

      expect(message).toContain('❌ Error:');
      expect(message).toContain('Status query failed');
    });

    it('should handle non-Error objects', async () => {
      mockInvoke.mockResolvedValueOnce(true); // isTauriAvailable
      mockInvoke.mockRejectedValueOnce('String error');

      const message = await ProxyLatencyOptimizationService.getOptimizationStatusMessage();

      expect(message).toBe('❌ Error: Failed to get optimization status: String error');
    });
  });

  describe('logProxyPerformance', () => {
    it('should log latency with proxy ID', () => {
      const consoleSpy = vi.spyOn(console, 'log');

      ProxyLatencyOptimizationService.logProxyPerformance('proxy-123', 50);

      expect(consoleSpy).toHaveBeenCalledWith('🚀 Proxy proxy-123 latency: 50ms');
    });

    it('should log offline status when latency is undefined', () => {
      const consoleSpy = vi.spyOn(console, 'log');

      ProxyLatencyOptimizationService.logProxyPerformance('proxy-456', undefined);

      expect(consoleSpy).toHaveBeenCalledWith('❌ Proxy proxy-456 offline or unavailable');
    });

    it('should handle zero latency', () => {
      const consoleSpy = vi.spyOn(console, 'log');

      ProxyLatencyOptimizationService.logProxyPerformance('proxy-789', 0);

      expect(consoleSpy).toHaveBeenCalledWith('🚀 Proxy proxy-789 latency: 0ms');
    });

    it('should handle very high latency', () => {
      const consoleSpy = vi.spyOn(console, 'log');

      ProxyLatencyOptimizationService.logProxyPerformance('proxy-slow', 9999);

      expect(consoleSpy).toHaveBeenCalledWith('🚀 Proxy proxy-slow latency: 9999ms');
    });
  });
});