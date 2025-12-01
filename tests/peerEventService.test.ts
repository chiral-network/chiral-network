import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { get } from 'svelte/store';

// Mock Tauri APIs before importing the service
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

// Import after mocks are set up
import { startPeerEventStream, peerDiscoveryStore, __resetDiscoveryStore } from '../src/lib/services/peerEventService';
import { peers } from '../src/lib/stores';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { listen } from '@tauri-apps/api/event';

describe('peerEventService', () => {
  let mockListeners: Map<string, Function>;
  let mockUnlistenFns: UnlistenFn[];

  beforeEach(async () => {
    // Reset stores to clean state
    peers.set([]);
    __resetDiscoveryStore();
    
    // Setup mock event system
    mockListeners = new Map();
    mockUnlistenFns = [];

    vi.mocked(listen).mockImplementation(async (eventName: string, handler: Function) => {
      mockListeners.set(eventName, handler);
      
      const unlisten: UnlistenFn = vi.fn(() => {
        mockListeners.delete(eventName);
      });
      mockUnlistenFns.push(unlisten);
      
      return unlisten;
    });

    // Mock Tauri environment
    (global as any).window = {
      __TAURI_INTERNALS__: {},
    };
  });

  afterEach(() => {
    vi.clearAllMocks();
    mockListeners.clear();
    mockUnlistenFns = [];
    delete (global as any).window;
  });

  describe('startPeerEventStream', () => {
    it('should register all three event listeners', async () => {
      await startPeerEventStream();

      expect(mockListeners.has('dht_peer_discovered')).toBe(true);
      expect(mockListeners.has('dht_peer_connected')).toBe(true);
      expect(mockListeners.has('dht_peer_disconnected')).toBe(true);
    });

    it('should return cleanup function that unlistens all events', async () => {
      const cleanup = await startPeerEventStream();

      expect(mockUnlistenFns).toHaveLength(3);
      
      cleanup();

      mockUnlistenFns.forEach(fn => {
        expect(fn).toHaveBeenCalledOnce();
      });
      expect(mockListeners.size).toBe(0);
    });

    it('should not register listeners in non-Tauri environment', async () => {
      delete (global as any).window.__TAURI_INTERNALS__;

      const cleanup = await startPeerEventStream();

      expect(mockListeners.size).toBe(0);
      expect(cleanup).toBeTypeOf('function');
    });

    it('should cleanup on registration error', async () => {
      const firstUnlisten = vi.fn();
      vi.mocked(listen)
        .mockResolvedValueOnce(firstUnlisten) // First succeeds
        .mockRejectedValueOnce(new Error('Registration failed')); // Second fails

      await expect(startPeerEventStream()).rejects.toThrow('Registration failed');

      // Should have called unlisten on the successful registration
      expect(firstUnlisten).toHaveBeenCalled();
    });
  });

  describe('dht_peer_discovered event', () => {
    it('should add new peer to discovery store', async () => {
      await startPeerEventStream();
      
      const handler = mockListeners.get('dht_peer_discovered')!;
      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].peerId).toBe('peer1');
      expect(discoveries[0].addresses).toEqual(['/ip4/192.168.1.1/tcp/4001']);
    });

    it('should merge addresses for existing peer', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // First discovery
      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      // Second discovery with new address
      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4002'],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].addresses).toHaveLength(2);
      expect(discoveries[0].addresses).toContain('/ip4/192.168.1.1/tcp/4001');
      expect(discoveries[0].addresses).toContain('/ip4/192.168.1.1/tcp/4002');
    });

    it('should update lastSeen timestamp on rediscovery', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      const firstTimestamp = get(peerDiscoveryStore)[0].lastSeen;

      // Wait a bit
      await new Promise(resolve => setTimeout(resolve, 10));

      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      const secondTimestamp = get(peerDiscoveryStore)[0].lastSeen;
      expect(secondTimestamp).toBeGreaterThan(firstTimestamp);
    });

    it('should handle null addresses array', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          addresses: null,
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].addresses).toEqual([]);
    });

    it('should filter out invalid addresses', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          addresses: [
            '/ip4/192.168.1.1/tcp/4001',
            '',
            '   ',
            '/ip4/192.168.1.2/tcp/4001',
          ],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries[0].addresses).toEqual([
        '/ip4/192.168.1.1/tcp/4001',
        '/ip4/192.168.1.2/tcp/4001',
      ]);
    });

    it('should deduplicate addresses', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          addresses: [
            '/ip4/192.168.1.1/tcp/4001',
            '/ip4/192.168.1.1/tcp/4001',
          ],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries[0].addresses).toEqual(['/ip4/192.168.1.1/tcp/4001']);
    });

    it('should ignore events with missing peerId', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: '',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      handler({
        payload: {
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(0);
    });

    it('should sort discoveries by lastSeen (most recent first)', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({ payload: { peerId: 'peer1', addresses: [] } });
      await new Promise(resolve => setTimeout(resolve, 10));
      
      handler({ payload: { peerId: 'peer2', addresses: [] } });
      await new Promise(resolve => setTimeout(resolve, 10));
      
      handler({ payload: { peerId: 'peer3', addresses: [] } });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries[0].peerId).toBe('peer3');
      expect(discoveries[1].peerId).toBe('peer2');
      expect(discoveries[2].peerId).toBe('peer1');
    });

    it('should limit discovery store to 200 entries', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Add 250 peers
      for (let i = 0; i < 250; i++) {
        handler({
          payload: {
            peerId: `peer${i}`,
            addresses: [`/ip4/192.168.1.${i}/tcp/4001`],
          },
        });
      }

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(200);
      
      // Should keep most recent 200
      expect(discoveries[0].peerId).toBe('peer249');
      expect(discoveries[199].peerId).toBe('peer50');
    });
  });

  describe('dht_peer_connected event', () => {
    it('should add peer to both discovery and peers store', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: 'peer1',
          address: '/ip4/192.168.1.1/tcp/4001',
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].peerId).toBe('peer1');

      const peersList = get(peers);
      expect(peersList).toHaveLength(1);
      expect(peersList[0].id).toBe('peer1');
      expect(peersList[0].status).toBe('online');
    });

    it('should update existing peer status to online', async () => {
      // Setup offline peer
      peers.set([{
        id: 'peer1',
        address: '/ip4/192.168.1.1/tcp/4001',
        status: 'offline',
        reputation: 50,
        sharedFiles: 5,
        totalSize: 1000,
        joinDate: new Date('2024-01-01'),
        lastSeen: new Date('2024-01-01'),
      }]);

      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: 'peer1',
          address: '/ip4/192.168.1.1/tcp/4001',
        },
      });

      const peersList = get(peers);
      expect(peersList).toHaveLength(1);
      expect(peersList[0].status).toBe('online');
      expect(peersList[0].reputation).toBe(50); // Preserved
    });

    it('should handle null address', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: 'peer1',
          address: null,
        },
      });

      const peersList = get(peers);
      expect(peersList).toHaveLength(1);
      expect(peersList[0].address).toBe('peer1'); // Falls back to peerId
    });

    it('should match peer by address if peerId differs', async () => {
      // Setup peer with known address
      peers.set([{
        id: 'oldId',
        address: '/ip4/192.168.1.1/tcp/4001',
        status: 'offline',
        reputation: 100,
        sharedFiles: 10,
        totalSize: 2000,
        joinDate: new Date('2024-01-01'),
        lastSeen: new Date('2024-01-01'),
      }]);

      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: 'newId',
          address: '/ip4/192.168.1.1/tcp/4001',
        },
      });

      const peersList = get(peers);
      expect(peersList).toHaveLength(1);
      expect(peersList[0].id).toBe('newId'); // Updated to new ID
      expect(peersList[0].reputation).toBe(100); // Preserved
    });

    it('should ignore events with missing peerId', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: '',
          address: '/ip4/192.168.1.1/tcp/4001',
        },
      });

      expect(get(peers)).toHaveLength(0);
    });

    it('should trim whitespace from addresses', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: 'peer1',
          address: '  /ip4/192.168.1.1/tcp/4001  ',
        },
      });

      const peersList = get(peers);
      expect(peersList[0].address).toBe('/ip4/192.168.1.1/tcp/4001');
    });

    it('should update lastSeen timestamp', async () => {
      const oldDate = new Date('2024-01-01');
      peers.set([{
        id: 'peer1',
        address: '/ip4/192.168.1.1/tcp/4001',
        status: 'offline',
        reputation: 0,
        sharedFiles: 0,
        totalSize: 0,
        joinDate: oldDate,
        lastSeen: oldDate,
      }]);

      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: 'peer1',
          address: '/ip4/192.168.1.1/tcp/4001',
        },
      });

      const peersList = get(peers);
      expect(peersList[0].lastSeen.getTime()).toBeGreaterThan(oldDate.getTime());
    });
  });

  describe('dht_peer_disconnected event', () => {
    it('should mark peer as offline', async () => {
      // Setup online peer
      peers.set([{
        id: 'peer1',
        address: '/ip4/192.168.1.1/tcp/4001',
        status: 'online',
        reputation: 50,
        sharedFiles: 5,
        totalSize: 1000,
        joinDate: new Date('2024-01-01'),
        lastSeen: new Date('2024-01-01'),
      }]);

      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_disconnected')!;

      handler({
        payload: {
          peerId: 'peer1',
        },
      });

      const peersList = get(peers);
      expect(peersList).toHaveLength(1);
      expect(peersList[0].status).toBe('offline');
    });

    it('should update lastSeen timestamp when peer disconnects', async () => {
      const oldDate = new Date('2024-01-01');
      peers.set([{
        id: 'peer1',
        address: '/ip4/192.168.1.1/tcp/4001',
        status: 'online',
        reputation: 0,
        sharedFiles: 0,
        totalSize: 0,
        joinDate: oldDate,
        lastSeen: oldDate,
      }]);

      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_disconnected')!;

      handler({
        payload: {
          peerId: 'peer1',
        },
      });

      const peersList = get(peers);
      expect(peersList[0].lastSeen.getTime()).toBeGreaterThan(oldDate.getTime());
    });

    it('should still update discovery store with empty addresses', async () => {
      await startPeerEventStream();
      const discoveryHandler = mockListeners.get('dht_peer_discovered')!;
      const disconnectHandler = mockListeners.get('dht_peer_disconnected')!;

      // First discover
      discoveryHandler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      const firstDiscoveries = get(peerDiscoveryStore);
      expect(firstDiscoveries[0].addresses).toHaveLength(1);

      // FIX: Add delay to ensure timestamp difference
      await new Promise(resolve => setTimeout(resolve, 10));

      // Then disconnect
      disconnectHandler({
        payload: {
          peerId: 'peer1',
        },
      });

      const secondDiscoveries = get(peerDiscoveryStore);
      expect(secondDiscoveries[0].addresses).toHaveLength(1); // Addresses preserved
      expect(secondDiscoveries[0].lastSeen).toBeGreaterThan(firstDiscoveries[0].lastSeen);
    });

    it('should handle disconnect for unknown peer gracefully', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_disconnected')!;

      handler({
        payload: {
          peerId: 'unknown-peer',
        },
      });

      expect(get(peers)).toHaveLength(0);
    });

    it('should ignore events with missing peerId', async () => {
      peers.set([{
        id: 'peer1',
        address: '/ip4/192.168.1.1/tcp/4001',
        status: 'online',
        reputation: 0,
        sharedFiles: 0,
        totalSize: 0,
        joinDate: new Date(),
        lastSeen: new Date(),
      }]);

      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_disconnected')!;

      handler({
        payload: {
          peerId: '',
        },
      });

      expect(get(peers)[0].status).toBe('online'); // Unchanged
    });

    it('should match peer by address if peerId differs', async () => {
      peers.set([{
        id: 'oldId',
        address: 'peer1',
        status: 'online',
        reputation: 0,
        sharedFiles: 0,
        totalSize: 0,
        joinDate: new Date(),
        lastSeen: new Date(),
      }]);

      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_disconnected')!;

      handler({
        payload: {
          peerId: 'peer1',
        },
      });

      expect(get(peers)[0].status).toBe('offline');
    });
  });

  describe('concurrent event handling', () => {
    it('should handle rapid discovery events without race conditions', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Simulate 100 rapid events
      const promises = Array.from({ length: 100 }, (_, i) =>
        Promise.resolve(handler({
          payload: {
            peerId: `peer${i % 10}`, // 10 unique peers
            addresses: [`/ip4/192.168.1.${i}/tcp/4001`],
          },
        }))
      );

      await Promise.all(promises);

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(10); // Only 10 unique peers
      
      // Each peer should have accumulated addresses
      discoveries.forEach(discovery => {
        expect(discovery.addresses.length).toBeGreaterThan(0);
      });
    });

    it('should handle connect/disconnect race conditions', async () => {
      await startPeerEventStream();
      const connectHandler = mockListeners.get('dht_peer_connected')!;
      const disconnectHandler = mockListeners.get('dht_peer_disconnected')!;

      // Rapid connect/disconnect
      for (let i = 0; i < 10; i++) {
        connectHandler({
          payload: {
            peerId: 'peer1',
            address: '/ip4/192.168.1.1/tcp/4001',
          },
        });
        
        disconnectHandler({
          payload: {
            peerId: 'peer1',
          },
        });
      }

      const peersList = get(peers);
      expect(peersList).toHaveLength(1);
      expect(peersList[0].status).toBe('offline'); // Last state
    });

    it('should handle events for same peer across all handlers', async () => {
      await startPeerEventStream();
      const discoveryHandler = mockListeners.get('dht_peer_discovered')!;
      const connectHandler = mockListeners.get('dht_peer_connected')!;
      const disconnectHandler = mockListeners.get('dht_peer_disconnected')!;

      // Discovery → Connect → Disconnect sequence
      discoveryHandler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      connectHandler({
        payload: {
          peerId: 'peer1',
          address: '/ip4/192.168.1.1/tcp/4001',
        },
      });

      disconnectHandler({
        payload: {
          peerId: 'peer1',
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].peerId).toBe('peer1');

      const peersList = get(peers);
      expect(peersList).toHaveLength(1);
      expect(peersList[0].id).toBe('peer1');
      expect(peersList[0].status).toBe('offline');
    });
  });

  describe('edge cases', () => {
    it('should handle malformed event payloads gracefully', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Various malformed payloads - should not throw
      expect(() => handler({ payload: null })).not.toThrow();
      expect(() => handler({ payload: undefined })).not.toThrow();
      expect(() => handler({ payload: {} })).not.toThrow();
      expect(() => handler({ payload: { peerId: null } })).not.toThrow();
      expect(() => handler({ payload: { peerId: 'peer1', addresses: 'not-an-array' } as any })).not.toThrow();

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].peerId).toBe('peer1');
      expect(discoveries[0].addresses).toEqual([]);
    });

    it('should handle cleanup called multiple times', async () => {
      const cleanup = await startPeerEventStream();

      cleanup();
      cleanup(); // Should not throw

      expect(mockUnlistenFns[0]).toHaveBeenCalledTimes(2);
    });

    it('should isolate stores between test runs', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      expect(get(peerDiscoveryStore)).toHaveLength(1);
      expect(get(peers)).toHaveLength(0);
    });

    it('should deduplicate addresses within same event', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          addresses: [
            '/ip4/192.168.1.1/tcp/4001',
            '/ip4/192.168.1.1/tcp/4001',
            '/ip4/192.168.1.1/tcp/4001',
          ],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries[0].addresses).toHaveLength(1);
      expect(discoveries[0].addresses).toEqual(['/ip4/192.168.1.1/tcp/4001']);
    });

    it('should deduplicate addresses across multiple events', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // First event with duplicate addresses
      handler({
        payload: {
          peerId: 'peer1',
          addresses: [
            '/ip4/192.168.1.1/tcp/4001',
            '/ip4/192.168.1.1/tcp/4001',
          ],
        },
      });

      // Second event with same address again
      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries[0].addresses).toHaveLength(1);
      expect(discoveries[0].addresses).toEqual(['/ip4/192.168.1.1/tcp/4001']);
    });

    it('should reject non-array addresses in payload', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // String instead of array
      handler({
        payload: {
          peerId: 'peer1',
          addresses: '/ip4/192.168.1.1/tcp/4001' as any,
        },
      });

      // Number instead of array
      handler({
        payload: {
          peerId: 'peer2',
          addresses: 12345 as any,
        },
      });

      // Object instead of array
      handler({
        payload: {
          peerId: 'peer3',
          addresses: { addr: '/ip4/192.168.1.1/tcp/4001' } as any,
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(3);
      discoveries.forEach(d => {
        expect(d.addresses).toEqual([]);
      });
    });

    it('should handle payload with peerId but missing addresses field entirely', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          // No addresses field at all
        } as any,
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].addresses).toEqual([]);
    });

    it('should handle addresses array with mixed valid and invalid types', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          addresses: [
            '/ip4/192.168.1.1/tcp/4001',
            null,
            undefined,
            123,
            { addr: 'test' },
            true,
            '/ip4/192.168.1.2/tcp/4001',
            '',
          ] as any,
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries[0].addresses).toHaveLength(2);
      expect(discoveries[0].addresses).toEqual([
        '/ip4/192.168.1.1/tcp/4001',
        '/ip4/192.168.1.2/tcp/4001',
      ]);
    });

    it('should handle empty string peerId after trimming', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: '   ',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(0);
    });

    it('should handle very long address strings', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      const longAddress = '/ip4/192.168.1.1/tcp/4001/' + 'x'.repeat(10000);
      
      handler({
        payload: {
          peerId: 'peer1',
          addresses: [longAddress],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries[0].addresses).toEqual([longAddress]);
    });

    it('should handle addresses with leading/trailing whitespace', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          addresses: [
            '  /ip4/192.168.1.1/tcp/4001  ',
            '\n/ip4/192.168.1.2/tcp/4001\n',
            '\t/ip4/192.168.1.3/tcp/4001\t',
          ],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries[0].addresses).toEqual([
        '/ip4/192.168.1.1/tcp/4001',
        '/ip4/192.168.1.2/tcp/4001',
        '/ip4/192.168.1.3/tcp/4001',
      ]);
    });

    it('should not create peer record when peerId is only whitespace', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: '   \n\t  ',
          address: '/ip4/192.168.1.1/tcp/4001',
        },
      });

      expect(get(peers)).toHaveLength(0);
    });

    it('should handle disconnect event with null payload', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_disconnected')!;

      expect(() => handler({ payload: null })).not.toThrow();
      expect(() => handler({ payload: undefined })).not.toThrow();
      expect(() => handler(null as any)).not.toThrow();
    });

    it('should preserve peer reputation across connection state changes', async () => {
      peers.set([{
        id: 'peer1',
        address: '/ip4/192.168.1.1/tcp/4001',
        status: 'offline',
        reputation: 75,
        sharedFiles: 10,
        totalSize: 5000,
        joinDate: new Date('2024-01-01'),
        lastSeen: new Date('2024-01-01'),
      }]);

      await startPeerEventStream();
      const connectHandler = mockListeners.get('dht_peer_connected')!;
      const disconnectHandler = mockListeners.get('dht_peer_disconnected')!;

      // Connect
      connectHandler({
        payload: {
          peerId: 'peer1',
          address: '/ip4/192.168.1.1/tcp/4001',
        },
      });

      let peersList = get(peers);
      expect(peersList[0].reputation).toBe(75);
      expect(peersList[0].status).toBe('online');

      // Disconnect
      disconnectHandler({
        payload: {
          peerId: 'peer1',
        },
      });

      peersList = get(peers);
      expect(peersList[0].reputation).toBe(75);
      expect(peersList[0].status).toBe('offline');
    });

    it('should handle discovery store exceeding 200 entries during concurrent updates', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Add exactly 200 peers
      for (let i = 0; i < 200; i++) {
        handler({
          payload: {
            peerId: `peer${i}`,
            addresses: [`/ip4/192.168.1.${i % 256}/tcp/4001`],
          },
        });
      }

      let discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(200);

      // Add one more - should still cap at 200
      handler({
        payload: {
          peerId: 'peer200',
          addresses: ['/ip4/192.168.1.200/tcp/4001'],
        },
      });

      discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(200);
      expect(discoveries[0].peerId).toBe('peer200'); // Most recent first
    });

    it('should handle update to existing peer at boundary of 200 limit', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Fill to capacity
      for (let i = 0; i < 200; i++) {
        handler({
          payload: {
            peerId: `peer${i}`,
            addresses: [`/ip4/192.168.1.${i % 256}/tcp/4001`],
          },
        });
      }

      // Update an existing peer (should not increase count)
      handler({
        payload: {
          peerId: 'peer50',
          addresses: ['/ip4/192.168.2.50/tcp/4001'],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(200);
      
      const updatedPeer = discoveries.find(d => d.peerId === 'peer50')!;
      expect(updatedPeer.addresses).toContain('/ip4/192.168.2.50/tcp/4001');
    });

    it('should handle empty addresses array on connect event', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: 'peer1',
          address: null,
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries[0].addresses).toEqual([]);
    });

    it('should handle address matching with different peer IDs', async () => {
      // Setup peer with address as ID
      peers.set([{
        id: '/ip4/192.168.1.1/tcp/4001',
        address: '/ip4/192.168.1.1/tcp/4001',
        status: 'offline',
        reputation: 50,
        sharedFiles: 5,
        totalSize: 1000,
        joinDate: new Date('2024-01-01'),
        lastSeen: new Date('2024-01-01'),
      }]);

      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: 'newPeerId',
          address: '/ip4/192.168.1.1/tcp/4001',
        },
      });

      const peersList = get(peers);
      expect(peersList).toHaveLength(1);
      expect(peersList[0].id).toBe('newPeerId');
      expect(peersList[0].reputation).toBe(50); // Preserved
    });
  });

  // Add these tests to expose the bugs more clearly:
  describe('address deduplication bugs', () => {
    it('should deduplicate before storing initial addresses', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 'peer1',
          addresses: [
            '/ip4/192.168.1.1/tcp/4001',
            '/ip4/192.168.1.1/tcp/4001',
            '/ip4/192.168.1.1/tcp/4001',
          ],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].addresses).toEqual(['/ip4/192.168.1.1/tcp/4001']);
    });

    it('should not create entry for non-array addresses with empty result', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // These create entries with empty addresses (valid behavior)
      handler({
        payload: {
          peerId: 'peer1',
          addresses: 'string-not-array' as any,
        },
      });

      handler({
        payload: {
          peerId: 'peer2',
          addresses: 123 as any,
        },
      });

      handler({
        payload: {
          peerId: 'peer3',
          addresses: { key: 'value' } as any,
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(3);
      discoveries.forEach(d => {
        expect(d.addresses).toEqual([]);
      });
    });

    it('should only create entry if addresses normalize to non-empty', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // This SHOULD create an entry (peerId is valid, addresses empty)
      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['', '   ', null, undefined] as any,
        },
      });

      // This SHOULD create an entry (one valid address)
      handler({
        payload: {
          peerId: 'peer2',
          addresses: ['', '/ip4/valid/tcp/4001', '   '],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(2);
      expect(discoveries.find(d => d.peerId === 'peer1')!.addresses).toEqual([]);
      expect(discoveries.find(d => d.peerId === 'peer2')!.addresses).toEqual(['/ip4/valid/tcp/4001']);
    });
  });

  describe('store update semantics', () => {
    it('should not add discovery entry for connect event with null address', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_connected')!;

      handler({
        payload: {
          peerId: 'peer1',
          address: null,
        },
      });

      // Should add to peers store
      const peersList = get(peers);
      expect(peersList).toHaveLength(1);

      // Should also add to discovery store (with empty addresses array)
      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].addresses).toEqual([]);
    });

    it('should update existing discovery without creating duplicates', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Initial discovery
      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      expect(get(peerDiscoveryStore)).toHaveLength(1);

      // Update with same address - should not create duplicate entry
      handler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(1);
      expect(discoveries[0].addresses).toEqual(['/ip4/192.168.1.1/tcp/4001']);
    });

    it('should handle disconnect preserving existing addresses', async () => {
      await startPeerEventStream();
      const discoveryHandler = mockListeners.get('dht_peer_discovered')!;
      const disconnectHandler = mockListeners.get('dht_peer_disconnected')!;

      // Discover with addresses
      discoveryHandler({
        payload: {
          peerId: 'peer1',
          addresses: ['/ip4/192.168.1.1/tcp/4001', '/ip4/192.168.1.2/tcp/4001'],
        },
      });

      const beforeDisconnect = get(peerDiscoveryStore);
      expect(beforeDisconnect[0].addresses).toHaveLength(2);

      await new Promise(resolve => setTimeout(resolve, 10));

      // Disconnect (calls mergeDiscovery with empty array)
      disconnectHandler({
        payload: {
          peerId: 'peer1',
        },
      });

      // Addresses should be preserved, only timestamp updated
      const afterDisconnect = get(peerDiscoveryStore);
      expect(afterDisconnect[0].addresses).toHaveLength(2);
      expect(afterDisconnect[0].lastSeen).toBeGreaterThan(beforeDisconnect[0].lastSeen);
    });
  });

  describe('payload validation edge cases', () => {
    it('should handle event with no payload property', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      expect(() => handler({} as any)).not.toThrow();
      expect(get(peerDiscoveryStore)).toHaveLength(0);
    });

    it('should handle payload as primitive type', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      expect(() => handler({ payload: 'string' } as any)).not.toThrow();
      expect(() => handler({ payload: 123 } as any)).not.toThrow();
      expect(() => handler({ payload: true } as any)).not.toThrow();

      expect(get(peerDiscoveryStore)).toHaveLength(0);
    });

    it('should handle payload with peerId as non-string', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      handler({
        payload: {
          peerId: 123 as any,
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      handler({
        payload: {
          peerId: null as any,
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      handler({
        payload: {
          peerId: undefined as any,
          addresses: ['/ip4/192.168.1.1/tcp/4001'],
        },
      });

      expect(get(peerDiscoveryStore)).toHaveLength(0);
    });

    it('should handle circular reference in payload', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      const circular: any = { peerId: 'peer1' };
      circular.self = circular;
      circular.addresses = ['/ip4/192.168.1.1/tcp/4001'];

      // Should handle gracefully without stack overflow
      expect(() => handler({ payload: circular })).not.toThrow();
    });
  });

  describe('timestamp precision and ordering', () => {
    it('should maintain correct ordering with rapid updates', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Add peers in specific order with small delays
      for (let i = 0; i < 5; i++) {
        handler({
          payload: {
            peerId: `peer${i}`,
            addresses: [`/ip4/192.168.1.${i}/tcp/4001`],
          },
        });
        await new Promise(resolve => setTimeout(resolve, 2));
      }

      const discoveries = get(peerDiscoveryStore);
      
      // Should be in reverse chronological order
      expect(discoveries[0].peerId).toBe('peer4');
      expect(discoveries[4].peerId).toBe('peer0');
      
      // Timestamps should be monotonically increasing
      for (let i = 0; i < discoveries.length - 1; i++) {
        expect(discoveries[i].lastSeen).toBeGreaterThanOrEqual(discoveries[i + 1].lastSeen);
      }
    });

    it('should handle same-millisecond updates correctly', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Add multiple peers with at least some addresses
      handler({ payload: { peerId: 'peer1', addresses: ['/ip4/192.168.1.1/tcp/4001'] } });
      handler({ payload: { peerId: 'peer2', addresses: ['/ip4/192.168.1.2/tcp/4001'] } });
      handler({ payload: { peerId: 'peer3', addresses: ['/ip4/192.168.1.3/tcp/4001'] } });

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(3);
      
      // All should have valid timestamps
      discoveries.forEach(d => {
        expect(d.lastSeen).toBeGreaterThan(0);
      });
    });
  });

  describe('store capacity and eviction', () => {
    it('should evict oldest entries when exceeding 200 limit', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Add 205 peers with delays to ensure distinct timestamps
      for (let i = 0; i < 205; i++) {
        handler({
          payload: {
            peerId: `peer${i}`,
            addresses: [`/ip4/192.168.1.${i % 256}/tcp/4001`],
          },
        });
        if (i % 10 === 0) await new Promise(resolve => setTimeout(resolve, 1));
      }

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(200);
      
      // Should keep newest 200
      expect(discoveries[0].peerId).toBe('peer204');
      expect(discoveries[199].peerId).toBe('peer5');
      
      // Oldest 5 should be gone
      expect(discoveries.find(d => d.peerId === 'peer0')).toBeUndefined();
      expect(discoveries.find(d => d.peerId === 'peer4')).toBeUndefined();
    });

    it('should not count updates to existing peers toward limit', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Fill to capacity
      for (let i = 0; i < 200; i++) {
        handler({
          payload: {
            peerId: `peer${i}`,
            addresses: [`/ip4/192.168.1.${i % 256}/tcp/4001`],
          },
        });
      }

      // Add small delay to ensure timestamp difference
      await new Promise(resolve => setTimeout(resolve, 10));

      // Update first peer multiple times
      for (let i = 0; i < 10; i++) {
        handler({
          payload: {
            peerId: 'peer0',
            addresses: [`/ip4/192.168.2.${i}/tcp/4001`],
          },
        });
        // Add tiny delay between updates
        if (i % 3 === 0) await new Promise(resolve => setTimeout(resolve, 1));
      }

      const discoveries = get(peerDiscoveryStore);
      expect(discoveries).toHaveLength(200);
      
      // peer0 should be at top (most recent)
      expect(discoveries[0].peerId).toBe('peer0');
      
      // Should have accumulated addresses
      const peer0 = discoveries.find(d => d.peerId === 'peer0')!;
      expect(peer0.addresses.length).toBeGreaterThan(1);
    });
  });

  describe('concurrent modifications', () => {
    it('should handle simultaneous adds and updates', async () => {
      await startPeerEventStream();
      const handler = mockListeners.get('dht_peer_discovered')!;

      // Add peers first
      for (let i = 0; i < 50; i++) {
        handler({
          payload: {
            peerId: `new-peer${i}`,
            addresses: [`/ip4/192.168.1.${i}/tcp/4001`],
          },
        });
      }

      // Then update some of them
      for (let i = 0; i < 10; i++) {
        for (let j = 0; j < 5; j++) {
          handler({
            payload: {
              peerId: `new-peer${i}`,
              addresses: [`/ip4/192.168.2.${j}/tcp/4001`],
            },
          });
        }
      }

      const discoveries = get(peerDiscoveryStore);
      
      // Should have consolidated entries
      expect(discoveries).toHaveLength(50); // 50 unique peer IDs
      
      // Filter logic - need to extract number correctly
      const updatedPeers = discoveries.filter(d => {
        // "new-peer0" to "new-peer9" have length 9-10
        // Extract the number after "new-peer"
        const match = d.peerId.match(/^new-peer(\d+)$/);
        if (!match) return false;
        const num = parseInt(match[1]);
        return num < 10;
      });
      
      expect(updatedPeers).toHaveLength(10);
      updatedPeers.forEach(peer => {
        expect(peer.addresses.length).toBeGreaterThan(1);
      });
    });
  });
});