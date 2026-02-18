import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

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

// Mock toastStore
vi.mock('$lib/toastStore', () => ({
  toasts: {
    show: vi.fn(),
    remove: vi.fn(),
    subscribe: vi.fn(),
  },
}));

// Mock @tauri-apps/api/event with a controllable listen mock
const mockUnlisten = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

describe('DhtService', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.useFakeTimers();
    localStorage.clear();
    // Re-setup the listen mock after clearAllMocks
    vi.mocked(listen).mockResolvedValue(mockUnlisten);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe('start()', () => {
    it('should invoke start_dht and set networkConnected to true', async () => {
      mockInvoke.mockResolvedValue('DHT started on port 4001');

      const { dhtService } = await import('$lib/dhtService');
      const { networkConnected } = await import('$lib/stores');

      await dhtService.start();

      expect(mockInvoke).toHaveBeenCalledWith('start_dht');
      expect(get(networkConnected)).toBe(true);
    });

    it('should register event listeners for peer discovery', async () => {
      mockInvoke.mockResolvedValue('ok');

      const { dhtService } = await import('$lib/dhtService');
      await dhtService.start();

      const mockListenFn = vi.mocked(listen);
      expect(mockListenFn).toHaveBeenCalledWith('peer-discovered', expect.any(Function));
      expect(mockListenFn).toHaveBeenCalledWith('ping-sent', expect.any(Function));
      expect(mockListenFn).toHaveBeenCalledWith('ping-received', expect.any(Function));
      expect(mockListenFn).toHaveBeenCalledWith('pong-received', expect.any(Function));
    });

    it('should throw on start failure without setting networkConnected', async () => {
      mockInvoke.mockRejectedValueOnce('Failed to bind port');

      const { dhtService } = await import('$lib/dhtService');
      const { networkConnected } = await import('$lib/stores');

      await expect(dhtService.start()).rejects.toBe('Failed to bind port');
      expect(get(networkConnected)).toBe(false);
    });

    it('should call get_peer_id after starting', async () => {
      mockInvoke
        .mockResolvedValueOnce('ok') // start_dht
        .mockResolvedValueOnce([])   // get_dht_peers (from polling)
        .mockResolvedValueOnce({ connectedPeers: 0, totalPeers: 0 }) // get_network_stats (from polling)
        .mockResolvedValueOnce('12D3KooWTestPeerId'); // get_peer_id

      const { dhtService } = await import('$lib/dhtService');
      await dhtService.start();

      expect(mockInvoke).toHaveBeenCalledWith('get_peer_id');
    });
  });

  describe('stop()', () => {
    it('should invoke stop_dht and clear stores', async () => {
      mockInvoke.mockResolvedValue('ok');

      const { dhtService } = await import('$lib/dhtService');
      const { networkConnected, peers, networkStats } = await import('$lib/stores');

      // Start first
      await dhtService.start();
      expect(get(networkConnected)).toBe(true);

      // Then stop
      await dhtService.stop();

      expect(mockInvoke).toHaveBeenCalledWith('stop_dht');
      expect(get(networkConnected)).toBe(false);
      expect(get(peers)).toEqual([]);
      expect(get(networkStats)).toEqual({ connectedPeers: 0, totalPeers: 0 });
    });

    it('should call unlisten functions on stop', async () => {
      mockInvoke.mockResolvedValue('ok');

      const { dhtService } = await import('$lib/dhtService');
      await dhtService.start();
      await dhtService.stop();

      // Each of the 4 event listeners should have been unlistened
      expect(mockUnlisten).toHaveBeenCalled();
    });

    it('should throw on stop failure', async () => {
      mockInvoke
        .mockResolvedValueOnce('ok') // start_dht
        .mockResolvedValueOnce([])   // get_dht_peers (from polling)
        .mockResolvedValueOnce({ connectedPeers: 0, totalPeers: 0 }) // get_network_stats
        .mockResolvedValueOnce('peer1'); // get_peer_id

      const { dhtService } = await import('$lib/dhtService');
      await dhtService.start();

      // Set up stop to fail
      mockInvoke.mockRejectedValueOnce('DHT not running');

      await expect(dhtService.stop()).rejects.toBe('DHT not running');
    });
  });

  describe('getPeerId()', () => {
    it('should return peer ID from backend', async () => {
      mockInvoke.mockResolvedValue('12D3KooWAbcDef123456');

      const { dhtService } = await import('$lib/dhtService');
      const peerId = await dhtService.getPeerId();

      expect(peerId).toBe('12D3KooWAbcDef123456');
      expect(mockInvoke).toHaveBeenCalledWith('get_peer_id');
    });

    it('should return null on error', async () => {
      mockInvoke.mockRejectedValueOnce('DHT not running');

      const { dhtService } = await import('$lib/dhtService');
      const peerId = await dhtService.getPeerId();

      expect(peerId).toBeNull();
    });
  });

  describe('getHealth()', () => {
    it('should return DHT health info', async () => {
      const mockHealth = {
        running: true,
        peerId: '12D3KooWTest',
        listeningAddresses: ['/ip4/0.0.0.0/tcp/4001'],
        connectedPeerCount: 5,
        kademliaPeers: 3,
        bootstrapNodes: [{ address: '/ip4/130.245.173.73/tcp/4001/p2p/12D3KooWRN', reachable: true }],
        sharedFiles: 10,
        protocols: ['/chiral/file-request/2.0.0'],
      };
      mockInvoke.mockResolvedValueOnce(mockHealth);

      const { dhtService } = await import('$lib/dhtService');
      const health = await dhtService.getHealth();

      expect(health.running).toBe(true);
      expect(health.connectedPeerCount).toBe(5);
      expect(health.sharedFiles).toBe(10);
      expect(health.protocols).toContain('/chiral/file-request/2.0.0');
    });

    it('should throw on health check failure', async () => {
      mockInvoke.mockRejectedValueOnce('DHT not running');

      const { dhtService } = await import('$lib/dhtService');
      await expect(dhtService.getHealth()).rejects.toBe('DHT not running');
    });
  });

  describe('pingPeer()', () => {
    it('should send ping to peer', async () => {
      mockInvoke.mockResolvedValueOnce('Pong from peer in 42ms');

      const { dhtService } = await import('$lib/dhtService');
      const result = await dhtService.pingPeer('12D3KooWTarget');

      expect(mockInvoke).toHaveBeenCalledWith('ping_peer', { peerId: '12D3KooWTarget' });
      expect(result).toBe('Pong from peer in 42ms');
    });

    it('should throw on ping failure', async () => {
      mockInvoke.mockRejectedValueOnce('Peer not found');

      const { dhtService } = await import('$lib/dhtService');
      await expect(dhtService.pingPeer('12D3KooWUnknown')).rejects.toBe('Peer not found');
    });
  });
});
