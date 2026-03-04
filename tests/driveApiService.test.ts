import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = vi.mocked(invoke);

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

describe('driveApiService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  describe('convertItem (snake_case → camelCase)', () => {
    it('should convert snake_case backend response to camelCase', async () => {
      // Set up Tauri environment so the Tauri path is used
      (window as any).__TAURI_INTERNALS__ = {};

      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xTest');

      // Mock invoke for listItems — returns snake_case from Rust backend
      mockedInvoke.mockResolvedValueOnce([
        {
          id: 'item-1',
          name: 'document.pdf',
          item_type: 'file',
          parent_id: null,
          size: 2048,
          mime_type: 'application/pdf',
          created_at: 1700000000,
          modified_at: 1700001000,
          starred: true,
          storage_path: '/data/item-1',
          is_public: false,
          merkle_root: 'hash123',
          protocol: 'WebRTC',
          price_chi: '1.5',
          seeding: true,
        },
      ]);

      const items = await driveApi.listItems(null);
      expect(items).toHaveLength(1);

      const item = items[0];
      expect(item.id).toBe('item-1');
      expect(item.name).toBe('document.pdf');
      expect(item.itemType).toBe('file');
      expect(item.parentId).toBeNull();
      expect(item.size).toBe(2048);
      expect(item.mimeType).toBe('application/pdf');
      expect(item.createdAt).toBe(1700000000);
      expect(item.modifiedAt).toBe(1700001000);
      expect(item.starred).toBe(true);
      expect(item.isPublic).toBe(false);
      // Seeding fields
      expect(item.merkleRoot).toBe('hash123');
      expect(item.protocol).toBe('WebRTC');
      expect(item.priceChi).toBe('1.5');
      expect(item.seeding).toBe(true);

      delete (window as any).__TAURI_INTERNALS__;
    });

    it('should handle already camelCase fields (fallback)', async () => {
      (window as any).__TAURI_INTERNALS__ = {};

      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xTest');

      mockedInvoke.mockResolvedValueOnce([
        {
          id: 'item-2',
          name: 'file.txt',
          itemType: 'folder',
          parentId: 'parent-1',
          createdAt: 1000,
          modifiedAt: 2000,
          starred: false,
          isPublic: true,
          merkleRoot: undefined,
          seeding: false,
        },
      ]);

      const items = await driveApi.listItems(null);
      expect(items[0].itemType).toBe('folder');
      expect(items[0].parentId).toBe('parent-1');
      expect(items[0].seeding).toBe(false);

      delete (window as any).__TAURI_INTERNALS__;
    });

    it('should default seeding to false when missing', async () => {
      (window as any).__TAURI_INTERNALS__ = {};

      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xTest');

      mockedInvoke.mockResolvedValueOnce([
        {
          id: 'item-3',
          name: 'old.txt',
          item_type: 'file',
          created_at: 0,
          modified_at: 0,
          starred: false,
          // No seeding fields at all — simulates pre-migration data
        },
      ]);

      const items = await driveApi.listItems(null);
      expect(items[0].seeding).toBe(false);
      expect(items[0].merkleRoot).toBeUndefined();
      expect(items[0].protocol).toBeUndefined();
      expect(items[0].priceChi).toBeUndefined();

      delete (window as any).__TAURI_INTERNALS__;
    });
  });

  describe('Tauri command routing', () => {
    beforeEach(() => {
      (window as any).__TAURI_INTERNALS__ = {};
    });

    afterEach(() => {
      delete (window as any).__TAURI_INTERNALS__;
    });

    it('listItems should invoke drive_list_items', async () => {
      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xOwner');

      mockedInvoke.mockResolvedValueOnce([]);

      await driveApi.listItems('folder-1');

      expect(mockedInvoke).toHaveBeenCalledWith('drive_list_items', {
        owner: '0xOwner',
        parentId: 'folder-1',
      });
    });

    it('listItems with null parentId should pass null', async () => {
      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xOwner');

      mockedInvoke.mockResolvedValueOnce([]);

      await driveApi.listItems(null);

      expect(mockedInvoke).toHaveBeenCalledWith('drive_list_items', {
        owner: '0xOwner',
        parentId: null,
      });
    });

    it('createFolder should invoke drive_create_folder', async () => {
      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xOwner');

      mockedInvoke.mockResolvedValueOnce({
        id: 'new-folder',
        name: 'My Folder',
        item_type: 'folder',
        created_at: 1000,
        modified_at: 1000,
        starred: false,
      });

      const result = await driveApi.createFolder('My Folder', null);

      expect(mockedInvoke).toHaveBeenCalledWith('drive_create_folder', {
        owner: '0xOwner',
        name: 'My Folder',
        parentId: null,
      });
      expect(result.name).toBe('My Folder');
      expect(result.itemType).toBe('folder');
    });

    it('uploadFile should invoke drive_upload_file with path', async () => {
      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xOwner');

      mockedInvoke.mockResolvedValueOnce({
        id: 'uploaded-1',
        name: 'photo.jpg',
        item_type: 'file',
        size: 4096,
        created_at: 1000,
        modified_at: 1000,
        starred: false,
      });

      const result = await driveApi.uploadFile('/home/user/photo.jpg', 'folder-1');

      expect(mockedInvoke).toHaveBeenCalledWith('drive_upload_file', {
        owner: '0xOwner',
        filePath: '/home/user/photo.jpg',
        parentId: 'folder-1',
      });
      expect(result.name).toBe('photo.jpg');
    });

    it('deleteItem should invoke drive_delete_item', async () => {
      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xOwner');

      mockedInvoke.mockResolvedValueOnce(undefined);

      await driveApi.deleteItem('item-to-delete');

      expect(mockedInvoke).toHaveBeenCalledWith('drive_delete_item', {
        owner: '0xOwner',
        itemId: 'item-to-delete',
      });
    });

    it('toggleVisibility should invoke drive_toggle_visibility', async () => {
      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xOwner');

      mockedInvoke.mockResolvedValueOnce({
        id: 'item-1',
        name: 'file.txt',
        item_type: 'file',
        created_at: 1000,
        modified_at: 1000,
        starred: false,
        is_public: true,
      });

      await driveApi.toggleVisibility('item-1', true);

      expect(mockedInvoke).toHaveBeenCalledWith('drive_toggle_visibility', {
        owner: '0xOwner',
        itemId: 'item-1',
        isPublic: true,
      });
    });
  });

  describe('URL generation', () => {
    it('getDownloadUrl should include id and filename', async () => {
      const { driveApi, setLocalDriveServer } = await import('$lib/services/driveApiService');
      setLocalDriveServer('http://localhost:9419');

      const url = driveApi.getDownloadUrl('item-abc', 'report.pdf');
      expect(url).toContain('item-abc');
      expect(url).toContain('report.pdf');
      expect(url).toContain('http://localhost:9419');
    });

    it('getShareUrl should use relay base', async () => {
      const { driveApi } = await import('$lib/services/driveApiService');
      const url = driveApi.getShareUrl('token-xyz');
      expect(url).toContain('token-xyz');
      expect(url).toContain('/drive/');
    });

    it('getDownloadUrl should URL-encode special characters', async () => {
      const { driveApi, setLocalDriveServer } = await import('$lib/services/driveApiService');
      setLocalDriveServer('http://localhost:9419');

      const url = driveApi.getDownloadUrl('id', 'file name (1).txt');
      expect(url).toContain('file%20name%20(1).txt');
    });
  });

  describe('setDriveOwner / setLocalDriveServer', () => {
    it('setDriveOwner should configure the owner for requests', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      const { driveApi, setDriveOwner } = await import('$lib/services/driveApiService');
      setDriveOwner('0xNewOwner');

      mockedInvoke.mockResolvedValueOnce([]);
      await driveApi.listItems(null);

      expect(mockedInvoke).toHaveBeenCalledWith('drive_list_items', {
        owner: '0xNewOwner',
        parentId: null,
      });

      delete (window as any).__TAURI_INTERNALS__;
    });
  });
});
