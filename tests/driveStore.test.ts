import { describe, it, expect, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import type { DriveItem, DriveManifest } from '$lib/stores/driveStore';

const mockedInvoke = vi.mocked(invoke);

// Mock wallet store
vi.mock('$lib/stores', () => {
  const { writable } = require('svelte/store');
  return {
    walletAccount: writable({ address: '0xTestWallet123' }),
    networkConnected: writable(true),
  };
});

// Mock driveApiService to avoid fetch/Tauri dependencies
vi.mock('$lib/services/driveApiService', () => ({
  driveApi: {
    listItems: vi.fn().mockResolvedValue([]),
    listShareLinks: vi.fn().mockResolvedValue([]),
    createFolder: vi.fn(),
    uploadFile: vi.fn(),
    updateItem: vi.fn(),
    deleteItem: vi.fn(),
    createShareLink: vi.fn(),
    revokeShareLink: vi.fn(),
    toggleVisibility: vi.fn(),
    getDownloadUrl: vi.fn((id: string, name: string) => `http://localhost/dl/${id}/${name}`),
    getShareUrl: vi.fn((token: string) => `http://relay/drive/${token}`),
  },
  setDriveOwner: vi.fn(),
  setLocalDriveServer: vi.fn(),
}));

describe('driveStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('DriveItem interface', () => {
    it('should have seeding metadata fields', async () => {
      const item: DriveItem = {
        id: 'test-1',
        name: 'test.txt',
        type: 'file',
        parentId: null,
        size: 1024,
        createdAt: Date.now(),
        modifiedAt: Date.now(),
        starred: false,
        shared: false,
        isPublic: true,
        merkleRoot: 'abc123hash',
        protocol: 'WebRTC',
        priceChi: '0.5',
        seeding: true,
      };
      expect(item.merkleRoot).toBe('abc123hash');
      expect(item.protocol).toBe('WebRTC');
      expect(item.priceChi).toBe('0.5');
      expect(item.seeding).toBe(true);
    });

    it('should allow optional seeding fields', async () => {
      const item: DriveItem = {
        id: 'test-2',
        name: 'regular.txt',
        type: 'file',
        parentId: null,
        createdAt: Date.now(),
        modifiedAt: Date.now(),
        starred: false,
        shared: false,
        isPublic: true,
      };
      expect(item.merkleRoot).toBeUndefined();
      expect(item.protocol).toBeUndefined();
      expect(item.priceChi).toBeUndefined();
      expect(item.seeding).toBeUndefined();
    });
  });

  describe('getSeedingItems', () => {
    it('should filter items with seeding=true', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: '1', name: 'seeded.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true, seeding: true, merkleRoot: 'hash1', protocol: 'WebRTC' as const },
          { id: '2', name: 'not-seeded.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true, seeding: false },
          { id: '3', name: 'also-seeded.mp4', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true, seeding: true, merkleRoot: 'hash3', protocol: 'BitTorrent' as const },
          { id: '4', name: 'folder', type: 'folder' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      const seeding = driveStore.getSeedingItems(manifest);
      expect(seeding).toHaveLength(2);
      expect(seeding[0].id).toBe('1');
      expect(seeding[1].id).toBe('3');
    });

    it('should return empty array when no items are seeding', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: '1', name: 'file.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      expect(driveStore.getSeedingItems(manifest)).toHaveLength(0);
    });

    it('should return empty array for empty manifest', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = { version: 1, items: [], shares: [], lastModified: 0 };
      expect(driveStore.getSeedingItems(manifest)).toHaveLength(0);
    });
  });

  describe('getChildren', () => {
    it('should return children of a given parent', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: '1', name: 'root-file.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: '2', name: 'folder', type: 'folder' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: '3', name: 'nested.txt', type: 'file' as const, parentId: '2', createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      const rootChildren = driveStore.getChildren(null, manifest);
      expect(rootChildren).toHaveLength(2);
      // Folders should sort before files
      expect(rootChildren[0].type).toBe('folder');
      expect(rootChildren[1].type).toBe('file');

      const nestedChildren = driveStore.getChildren('2', manifest);
      expect(nestedChildren).toHaveLength(1);
      expect(nestedChildren[0].name).toBe('nested.txt');
    });

    it('should sort starred items to the front, then folders before files, then alphabetically', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: '1', name: 'z-file.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: '2', name: 'a-file.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: true, shared: false, isPublic: true },
          { id: '3', name: 'z-folder', type: 'folder' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: '4', name: 'a-folder', type: 'folder' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: true, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      const children = driveStore.getChildren(null, manifest);
      expect(children.map(c => c.name)).toEqual(['a-folder', 'a-file.txt', 'z-folder', 'z-file.txt']);
    });
  });

  describe('getBreadcrumb', () => {
    it('should build breadcrumb path from item to root', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: 'root-folder', name: 'Documents', type: 'folder' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: 'sub-folder', name: 'Work', type: 'folder' as const, parentId: 'root-folder', createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: 'deep-folder', name: 'Projects', type: 'folder' as const, parentId: 'sub-folder', createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      const crumbs = driveStore.getBreadcrumb('deep-folder', manifest);
      expect(crumbs).toHaveLength(3);
      expect(crumbs[0].name).toBe('Documents');
      expect(crumbs[1].name).toBe('Work');
      expect(crumbs[2].name).toBe('Projects');
    });

    it('should return empty array for null (root)', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = { version: 1, items: [], shares: [], lastModified: 0 };
      expect(driveStore.getBreadcrumb(null, manifest)).toEqual([]);
    });
  });

  describe('searchByName', () => {
    it('should find items matching query (case-insensitive)', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: '1', name: 'Report.pdf', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: '2', name: 'notes.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: '3', name: 'Annual Report 2024', type: 'folder' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      const results = driveStore.searchByName('report', manifest);
      expect(results).toHaveLength(2);
      expect(results.map(r => r.id).sort()).toEqual(['1', '3']);
    });

    it('should return empty for no matches', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: '1', name: 'file.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      expect(driveStore.searchByName('nonexistent', manifest)).toHaveLength(0);
    });

    it('should sort search results with starred items first', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: '1', name: 'report-z.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: '2', name: 'report-a.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: true, shared: false, isPublic: true },
          { id: '3', name: 'report-folder', type: 'folder' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      const results = driveStore.searchByName('report', manifest);
      expect(results.map(r => r.id)).toEqual(['2', '3', '1']);
    });
  });

  describe('getSharesForItem', () => {
    it('should return shares matching the item', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [],
        shares: [
          { id: 'share-1', itemId: 'item-1', url: '', isPublic: true, hasPassword: false, createdAt: 0, downloadCount: 0 },
          { id: 'share-2', itemId: 'item-2', url: '', isPublic: true, hasPassword: false, createdAt: 0, downloadCount: 0 },
          { id: 'share-3', itemId: 'item-1', url: '', isPublic: false, hasPassword: true, createdAt: 0, downloadCount: 5 },
        ],
        lastModified: 0,
      };
      const shares = driveStore.getSharesForItem('item-1', manifest);
      expect(shares).toHaveLength(2);
      expect(shares[0].id).toBe('share-1');
      expect(shares[1].id).toBe('share-3');
    });
  });

  describe('getAllFolders', () => {
    it('should return only folders', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: '1', name: 'file.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: '2', name: 'folder1', type: 'folder' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
          { id: '3', name: 'folder2', type: 'folder' as const, parentId: '2', createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      const folders = driveStore.getAllFolders(manifest);
      expect(folders).toHaveLength(2);
      expect(folders.every(f => f.type === 'folder')).toBe(true);
    });
  });

  describe('getItem', () => {
    it('should find item by id', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const manifest = {
        version: 1,
        items: [
          { id: 'abc', name: 'found.txt', type: 'file' as const, parentId: null, createdAt: 0, modifiedAt: 0, starred: false, shared: false, isPublic: true },
        ],
        shares: [],
        lastModified: 0,
      };
      expect(driveStore.getItem('abc', manifest)?.name).toBe('found.txt');
      expect(driveStore.getItem('nonexistent', manifest)).toBeUndefined();
    });
  });

  describe('seedFile', () => {
    it('should invoke publish_drive_file with correct args', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');

      // Rust struct uses #[serde(rename_all = "camelCase")], so Tauri returns camelCase
      mockedInvoke.mockResolvedValueOnce({
        id: 'item-1',
        name: 'test.txt',
        itemType: 'file',
        parentId: null,
        size: 1024,
        createdAt: 1000,
        modifiedAt: 1000,
        starred: false,
        isPublic: true,
        merkleRoot: 'abc123',
        protocol: 'WebRTC',
        priceChi: null,
        seeding: true,
      });

      const result = await driveStore.seedFile('item-1', 'WebRTC');

      expect(mockedInvoke).toHaveBeenCalledWith('publish_drive_file', {
        owner: '0xTestWallet123',
        itemId: 'item-1',
        protocol: 'WebRTC',
        priceChi: null,
        walletAddress: null,
      });
      expect(result).not.toBeNull();
      expect(result!.seeding).toBe(true);
      expect(result!.merkleRoot).toBe('abc123');
      expect(result!.protocol).toBe('WebRTC');
    });

    it('should include wallet address when priceChi is set', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');

      mockedInvoke.mockResolvedValueOnce({
        id: 'item-2',
        name: 'paid.txt',
        itemType: 'file',
        parentId: null,
        size: 2048,
        createdAt: 1000,
        modifiedAt: 1000,
        starred: false,
        isPublic: true,
        merkleRoot: 'def456',
        protocol: 'BitTorrent',
        priceChi: '0.5',
        seeding: true,
      });

      await driveStore.seedFile('item-2', 'BitTorrent', '0.5');

      expect(mockedInvoke).toHaveBeenCalledWith('publish_drive_file', {
        owner: '0xTestWallet123',
        itemId: 'item-2',
        protocol: 'BitTorrent',
        priceChi: '0.5',
        walletAddress: '0xTestWallet123',
      });
    });

    it('should not include wallet address when priceChi is "0"', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');

      mockedInvoke.mockResolvedValueOnce({
        id: 'item-3',
        name: 'free.txt',
        itemType: 'file',
        parentId: null,
        size: 512,
        createdAt: 1000,
        modifiedAt: 1000,
        starred: false,
        isPublic: true,
        merkleRoot: 'ghi789',
        protocol: 'WebRTC',
        priceChi: null,
        seeding: true,
      });

      await driveStore.seedFile('item-3', 'WebRTC', '0');

      expect(mockedInvoke).toHaveBeenCalledWith('publish_drive_file', expect.objectContaining({
        walletAddress: null,
      }));
    });

    it('should return null on failure', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      mockedInvoke.mockRejectedValueOnce(new Error('DHT not running'));
      const result = await driveStore.seedFile('item-1', 'WebRTC');
      expect(result).toBeNull();
    });
  });

  describe('stopSeeding', () => {
    it('should invoke drive_stop_seeding', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');

      mockedInvoke.mockResolvedValueOnce({
        id: 'item-1',
        name: 'test.txt',
        itemType: 'file',
        parentId: null,
        size: 1024,
        createdAt: 1000,
        modifiedAt: 1000,
        starred: false,
        isPublic: true,
        merkleRoot: null,
        protocol: null,
        priceChi: null,
        seeding: false,
      });

      await driveStore.stopSeeding('item-1');

      expect(mockedInvoke).toHaveBeenCalledWith('drive_stop_seeding', {
        owner: '0xTestWallet123',
        itemId: 'item-1',
      });
    });

    it('should handle errors gracefully', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      mockedInvoke.mockRejectedValueOnce(new Error('Item not found'));
      // Should not throw
      await expect(driveStore.stopSeeding('nonexistent')).resolves.toBeUndefined();
    });
  });

  describe('exportTorrent', () => {
    it('should invoke drive_export_torrent and return path', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      mockedInvoke.mockResolvedValueOnce('/home/user/Downloads/test.torrent');

      const result = await driveStore.exportTorrent('item-1');

      expect(mockedInvoke).toHaveBeenCalledWith('drive_export_torrent', {
        owner: '0xTestWallet123',
        itemId: 'item-1',
      });
      expect(result).toBe('/home/user/Downloads/test.torrent');
    });

    it('should return null on failure', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      mockedInvoke.mockRejectedValueOnce(new Error('File not seeded'));
      const result = await driveStore.exportTorrent('item-1');
      expect(result).toBeNull();
    });
  });

  describe('getDownloadUrl', () => {
    it('should delegate to driveApi', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const url = driveStore.getDownloadUrl('id-1', 'file.txt');
      expect(url).toBe('http://localhost/dl/id-1/file.txt');
    });
  });

  describe('getShareUrl', () => {
    it('should delegate to driveApi', async () => {
      const { driveStore } = await import('$lib/stores/driveStore');
      const url = driveStore.getShareUrl('token-123');
      expect(url).toBe('http://relay/drive/token-123');
    });
  });
});
