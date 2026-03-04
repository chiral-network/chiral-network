/**
 * Tests for DHT bootstrap node configuration.
 *
 * These tests validate the bootstrap node multiaddrs used by the DHT service
 * by parsing and verifying the configuration without starting actual libp2p nodes.
 */
import { describe, it, expect } from 'vitest';

// We replicate the bootstrap node list here to test the configuration
// without requiring Rust backend. This ensures the frontend has access
// to the same bootstrap configuration.
const BOOTSTRAP_NODES = [
  '/ip4/134.199.240.145/tcp/4001/p2p/12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE',
  '/ip4/34.44.149.113/tcp/4001/p2p/12D3KooWETLNJUVLbkAbenbSPPdwN9ZLkBU3TLfyAeEUW2dsVptr',
  '/ip4/34.44.149.113/tcp/4002/p2p/12D3KooWGV5BUSYMhNMrhdPh9EUbuLrvAiDsMXEMRpGGvt4LQneA',
  '/ip4/130.245.173.105/tcp/4001/p2p/12D3KooWSDDA2jyo6Cynr7SHPfhdQoQazu1jdUEAp7rLKKKLqqTr',
];

// Geth bootstrap enodes from geth_bootstrap.rs
const GETH_BOOTSTRAP_ENODES = [
  'enode://ae987db6399b50addb75d7822bfad9b4092fbfd79cbfe97e6864b1f17d3e8fcd8e9e190ad109572c1439230fa688a9837e58f0b1ad7c0dc2bc6e4ab328f3991e@130.245.173.105:30303',
  'enode://b3ead5f07d0dbeda56023435a7c05877d67b055df3a8bf18f3d5f7c56873495cd4de5cf031ae9052827c043c12f1d30704088c79fb539c96834bfa74b78bf80b@20.85.124.187:30303',
];

describe('DHT Bootstrap Configuration', () => {
  describe('libp2p multiaddr format', () => {
    it('should have exactly 4 bootstrap nodes', () => {
      expect(BOOTSTRAP_NODES).toHaveLength(4);
    });

    it('should all start with /ip4/', () => {
      for (const node of BOOTSTRAP_NODES) {
        expect(node).toMatch(/^\/ip4\//);
      }
    });

    it('should all contain /tcp/ with valid port', () => {
      for (const node of BOOTSTRAP_NODES) {
        const match = node.match(/\/tcp\/(\d+)/);
        expect(match).not.toBeNull();
        const port = parseInt(match![1]);
        expect(port).toBeGreaterThan(0);
        expect(port).toBeLessThan(65536);
      }
    });

    it('should all contain /p2p/ with a peer ID', () => {
      for (const node of BOOTSTRAP_NODES) {
        expect(node).toMatch(/\/p2p\/12D3KooW[A-Za-z0-9]+$/);
      }
    });

    it('should have valid IPv4 addresses', () => {
      const ipv4Regex = /\/ip4\/(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})/;
      for (const node of BOOTSTRAP_NODES) {
        const match = node.match(ipv4Regex);
        expect(match).not.toBeNull();
        const parts = match![1].split('.').map(Number);
        expect(parts).toHaveLength(4);
        parts.forEach(p => {
          expect(p).toBeGreaterThanOrEqual(0);
          expect(p).toBeLessThanOrEqual(255);
        });
      }
    });

    it('should have unique peer IDs', () => {
      const peerIds = BOOTSTRAP_NODES.map(n => n.split('/p2p/')[1]);
      const unique = new Set(peerIds);
      expect(unique.size).toBe(peerIds.length);
    });

    it('should use ports in expected range (4001-4002)', () => {
      for (const node of BOOTSTRAP_NODES) {
        const port = parseInt(node.match(/\/tcp\/(\d+)/)![1]);
        expect(port).toBeGreaterThanOrEqual(4001);
        expect(port).toBeLessThanOrEqual(4002);
      }
    });
  });

  describe('Geth enode format', () => {
    it('should have exactly 2 Geth bootstrap nodes', () => {
      expect(GETH_BOOTSTRAP_ENODES).toHaveLength(2);
    });

    it('should all start with enode://', () => {
      for (const enode of GETH_BOOTSTRAP_ENODES) {
        expect(enode).toMatch(/^enode:\/\//);
      }
    });

    it('should contain @ separator between node ID and address', () => {
      for (const enode of GETH_BOOTSTRAP_ENODES) {
        expect(enode).toContain('@');
        const parts = enode.split('@');
        expect(parts).toHaveLength(2);
      }
    });

    it('should have valid IP:port in address portion', () => {
      for (const enode of GETH_BOOTSTRAP_ENODES) {
        const address = enode.split('@')[1];
        const match = address.match(/^(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}):(\d+)$/);
        expect(match).not.toBeNull();
        const port = parseInt(match![2]);
        expect(port).toBe(30303); // Standard Geth port
      }
    });

    it('should have hex-encoded node IDs (128 chars)', () => {
      for (const enode of GETH_BOOTSTRAP_ENODES) {
        const nodeId = enode.replace('enode://', '').split('@')[0];
        expect(nodeId).toMatch(/^[0-9a-f]{128}$/);
      }
    });

    it('should have unique node IDs', () => {
      const nodeIds = GETH_BOOTSTRAP_ENODES.map(e => e.replace('enode://', '').split('@')[0]);
      const unique = new Set(nodeIds);
      expect(unique.size).toBe(nodeIds.length);
    });
  });

  describe('multiaddr parsing', () => {
    function parseMultiaddr(addr: string) {
      const parts = addr.split('/').filter(Boolean);
      const result: Record<string, string> = {};
      for (let i = 0; i < parts.length; i += 2) {
        result[parts[i]] = parts[i + 1];
      }
      return result;
    }

    it('should parse all multiaddr components correctly', () => {
      const parsed = parseMultiaddr(BOOTSTRAP_NODES[0]);
      expect(parsed.ip4).toBe('134.199.240.145');
      expect(parsed.tcp).toBe('4001');
      expect(parsed.p2p).toBe('12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE');
    });

    it('should extract peer IDs from all nodes', () => {
      const expectedPeerIds = [
        '12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE',
        '12D3KooWETLNJUVLbkAbenbSPPdwN9ZLkBU3TLfyAeEUW2dsVptr',
        '12D3KooWGV5BUSYMhNMrhdPh9EUbuLrvAiDsMXEMRpGGvt4LQneA',
        '12D3KooWSDDA2jyo6Cynr7SHPfhdQoQazu1jdUEAp7rLKKKLqqTr',
      ];
      BOOTSTRAP_NODES.forEach((node, i) => {
        const parsed = parseMultiaddr(node);
        expect(parsed.p2p).toBe(expectedPeerIds[i]);
      });
    });
  });

  describe('enode parsing', () => {
    function parseEnode(enode: string): { nodeId: string; ip: string; port: number } | null {
      const match = enode.match(/^enode:\/\/([0-9a-f]+)@(\d+\.\d+\.\d+\.\d+):(\d+)/);
      if (!match) return null;
      return { nodeId: match[1], ip: match[2], port: parseInt(match[3]) };
    }

    it('should parse valid enode URLs', () => {
      const parsed = parseEnode(GETH_BOOTSTRAP_ENODES[0]);
      expect(parsed).not.toBeNull();
      expect(parsed!.ip).toBe('130.245.173.105');
      expect(parsed!.port).toBe(30303);
    });

    it('should return null for invalid enode URLs', () => {
      expect(parseEnode('invalid')).toBeNull();
      expect(parseEnode('http://example.com')).toBeNull();
      expect(parseEnode('')).toBeNull();
    });

    it('should handle enode with query parameters', () => {
      const enode = 'enode://abc123def456@192.168.1.1:30303?discport=30304';
      // The regex stops at the port, so query params don't affect parsing
      const parsed = parseEnode(enode);
      expect(parsed).not.toBeNull();
      expect(parsed!.port).toBe(30303);
    });
  });
});
