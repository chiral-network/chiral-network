import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { peers } from '$lib/stores';

const mockInvoke = vi.mocked(invoke);
const getBatchRatingsMock = vi.fn();

vi.mock('$lib/services/ratingApiService', () => ({
  ratingApi: {
    getBatchRatings: getBatchRatingsMock,
  },
}));

describe('hostingService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    peers.set([]);
    delete (window as any).__TAURI_INTERNALS__;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('publish / unpublish', () => {
    it('publishes host advertisement in Tauri mode', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      const { hostingService } = await import('$lib/services/hostingService');

      await hostingService.publishHostAdvertisement(
        {
          enabled: true,
          maxStorageBytes: 10_000,
          pricePerMbPerDayWei: '1000',
          minDepositWei: '2000',
        },
        '0xWallet',
      );

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith(
        'publish_host_advertisement',
        expect.objectContaining({ advertisementJson: expect.any(String) }),
      );
      const payload = JSON.parse(mockInvoke.mock.calls[0][1]!.advertisementJson as string);
      expect(payload.walletAddress).toBe('0xWallet');
      expect(payload.maxStorageBytes).toBe(10_000);
      expect(payload.pricePerMbPerDayWei).toBe('1000');
      expect(payload.minDepositWei).toBe('2000');
    });

    it('no-ops publish outside Tauri mode', async () => {
      const { hostingService } = await import('$lib/services/hostingService');
      await hostingService.publishHostAdvertisement(
        {
          enabled: true,
          maxStorageBytes: 1,
          pricePerMbPerDayWei: '1',
          minDepositWei: '1',
        },
        '0xWallet',
      );
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it('unpublishes in Tauri mode', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      const { hostingService } = await import('$lib/services/hostingService');
      await hostingService.unpublishHostAdvertisement();
      expect(mockInvoke).toHaveBeenCalledWith('unpublish_host_advertisement');
    });
  });

  describe('discoverHosts', () => {
    it('filters self/offline/stale peers and maps ratings', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      peers.set([{ id: 'peer-a', address: 'a', lastSeen: Date.now() }, { id: 'peer-b', address: 'b', lastSeen: Date.now() }]);
      getBatchRatingsMock.mockResolvedValue({
        '0xA': { average: 4.5, count: 2 },
      });

      const now = Math.floor(Date.now() / 1000);
      mockInvoke.mockImplementation(async (cmd, args) => {
        if (cmd === 'get_host_registry') {
          return JSON.stringify([
            { peerId: 'self-peer', walletAddress: '0xSelf', updatedAt: now },
            { peerId: 'peer-a', walletAddress: '0xA', updatedAt: now },
            { peerId: 'peer-b', walletAddress: '0xB', updatedAt: now },
            { peerId: 'peer-offline', walletAddress: '0xOff', updatedAt: now },
          ]);
        }
        if (cmd === 'get_peer_id') return 'self-peer';
        if (cmd === 'get_host_advertisement' && (args as any)?.peerId === 'peer-a') {
          return JSON.stringify({
            peerId: 'peer-a',
            walletAddress: '0xA',
            maxStorageBytes: 1000,
            usedStorageBytes: 250,
            pricePerMbPerDayWei: '100',
            minDepositWei: '10',
            uptimePercent: 99,
            publishedAt: now - 120,
            lastHeartbeatAt: now - 60,
          });
        }
        if (cmd === 'get_host_advertisement' && (args as any)?.peerId === 'peer-b') {
          // stale
          return JSON.stringify({
            peerId: 'peer-b',
            walletAddress: '0xB',
            maxStorageBytes: 500,
            usedStorageBytes: 100,
            pricePerMbPerDayWei: '100',
            minDepositWei: '10',
            uptimePercent: 99,
            publishedAt: now - 10_000,
            lastHeartbeatAt: now - 10_000,
          });
        }
        return null;
      });

      const { hostingService } = await import('$lib/services/hostingService');
      const hosts = await hostingService.discoverHosts();

      expect(hosts).toHaveLength(1);
      expect(hosts[0].advertisement.peerId).toBe('peer-a');
      expect(hosts[0].availableStorageBytes).toBe(750);
      expect(hosts[0].reputationScore).toBe(0.9);
      expect(hosts[0].isOnline).toBe(true);
    });

    it('returns empty list for invalid registry JSON', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      mockInvoke.mockResolvedValueOnce('not-json');
      const { hostingService } = await import('$lib/services/hostingService');
      await expect(hostingService.discoverHosts()).resolves.toEqual([]);
    });
  });

  describe('agreements', () => {
    it('proposes agreement, stores it, sends echo, and indexes locally', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      mockInvoke.mockResolvedValue(undefined);
      const { hostingService } = await import('$lib/services/hostingService');

      const agreement = await hostingService.proposeAgreement(
        'client-peer',
        '0xClient',
        'host-peer',
        '0xHost',
        ['hash-a', 'hash-b'],
        2 * 1024 * 1024, // 2 MB
        2 * 86400, // 2 days
        '1000000000000000',
        '1000',
      );

      expect(agreement.status).toBe('proposed');
      expect(agreement.totalCostWei).toBe('4000000000000000');
      expect(mockInvoke).toHaveBeenCalledWith(
        'store_hosting_agreement',
        expect.objectContaining({
          agreementId: agreement.agreementId,
          agreementJson: expect.any(String),
        }),
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        'echo_peer',
        expect.objectContaining({
          peerId: 'host-peer',
          payload: expect.any(Array),
        }),
      );

      const indexed = JSON.parse(localStorage.getItem('chiral-my-agreement-ids') || '[]');
      expect(indexed).toContain(agreement.agreementId);
    });

    it('responds to agreement and notifies proposer', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      mockInvoke.mockResolvedValue(undefined);
      const { hostingService } = await import('$lib/services/hostingService');

      vi.spyOn(hostingService, 'getAgreement').mockResolvedValue({
        agreementId: 'a-1',
        clientPeerId: 'client-peer',
        clientWalletAddress: '0xClient',
        hostPeerId: 'host-peer',
        hostWalletAddress: '0xHost',
        fileHashes: ['hash-a'],
        totalSizeBytes: 10,
        durationSecs: 100,
        pricePerMbPerDayWei: '1',
        totalCostWei: '1',
        depositWei: '1',
        status: 'proposed',
        proposedAt: 1,
      });

      const updated = await hostingService.respondToAgreement('a-1', true);
      expect(updated.status).toBe('accepted');
      expect(updated.respondedAt).toBeTypeOf('number');
      expect(mockInvoke).toHaveBeenCalledWith(
        'store_hosting_agreement',
        expect.objectContaining({ agreementId: 'a-1' }),
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        'echo_peer',
        expect.objectContaining({ peerId: 'client-peer' }),
      );
    });

    it('handles cancellation request flow for proposed and active agreements', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      mockInvoke.mockResolvedValue(undefined);
      const { hostingService } = await import('$lib/services/hostingService');

      const proposed = {
        agreementId: 'p-1',
        clientPeerId: 'client-peer',
        clientWalletAddress: '0xClient',
        hostPeerId: 'host-peer',
        hostWalletAddress: '0xHost',
        fileHashes: [],
        totalSizeBytes: 0,
        durationSecs: 60,
        pricePerMbPerDayWei: '1',
        totalCostWei: '1',
        depositWei: '1',
        status: 'proposed' as const,
        proposedAt: 1,
      };

      vi.spyOn(hostingService, 'getAgreement').mockResolvedValueOnce({ ...proposed });
      await expect(hostingService.requestCancellation('p-1', 'client-peer')).resolves.toBe('cancelled');

      const active = {
        ...proposed,
        agreementId: 'a-2',
        status: 'active' as const,
      };
      vi.spyOn(hostingService, 'getAgreement').mockResolvedValueOnce({ ...active });
      await expect(hostingService.requestCancellation('a-2', 'client-peer')).resolves.toBe('pending');

      const storeCalls = mockInvoke.mock.calls.filter((c) => c[0] === 'store_hosting_agreement');
      expect(storeCalls.length).toBeGreaterThanOrEqual(2);
    });
  });

  describe('calculateTotalCostWei', () => {
    it('calculates costs from size, duration, and unit price', async () => {
      const { hostingService } = await import('$lib/services/hostingService');
      const cost = hostingService.calculateTotalCostWei(
        3 * 1024 * 1024, // 3MB
        3 * 86400, // 3 days
        '1000000000000000',
      );
      expect(cost).toBe('9000000000000000');
    });
  });
});
