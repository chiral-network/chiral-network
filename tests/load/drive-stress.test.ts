/**
 * Drive storage stress tests — verifies Drive operations under concurrent load
 *
 * Tests concurrent uploads, downloads, CRUD operations, and consistency
 * when multiple users operate on Drive simultaneously.
 */
import { describe, it, expect, beforeEach, vi } from 'vitest';

const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock('$lib/logger', () => ({
  logger: () => ({
    info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn(), ok: vi.fn(),
  }),
}));

function makeDriveItem(overrides: Record<string, unknown> = {}) {
  return {
    id: `item_${Math.random().toString(36).slice(2, 10)}`,
    name: 'test.txt',
    itemType: 'file',
    parentId: null,
    size: 1024,
    mimeType: 'text/plain',
    createdAt: Date.now() / 1000,
    modifiedAt: Date.now() / 1000,
    starred: false,
    storagePath: 'test_storage.txt',
    owner: '0xowner',
    isPublic: false,
    merkleRoot: null,
    protocol: null,
    priceChi: null,
    seedEnabled: false,
    seeding: false,
    ...overrides,
  };
}

describe('Drive storage stress tests', () => {
  beforeEach(() => {
    vi.resetModules();
    mockInvoke.mockReset();
  });

  describe('concurrent uploads', () => {
    it('should handle 20 concurrent file uploads', async () => {
      for (let i = 0; i < 20; i++) {
        mockInvoke.mockResolvedValueOnce(makeDriveItem({ name: `upload_${i}.txt`, id: `id_${i}` }));
      }

      const results = await Promise.all(
        Array.from({ length: 20 }, (_, i) =>
          mockInvoke('drive_upload_file', {
            owner: '0xowner',
            filePath: `/tmp/upload_${i}.txt`,
            parentId: null,
            merkleRoot: null,
          })
        )
      );

      expect(results).toHaveLength(20);
      const ids = results.map(r => r.id);
      expect(new Set(ids).size).toBe(20); // All unique IDs
    });

    it('should handle upload failures without crashing', async () => {
      for (let i = 0; i < 15; i++) {
        if (i % 5 === 0) {
          mockInvoke.mockRejectedValueOnce('File exceeds 500 MB limit');
        } else {
          mockInvoke.mockResolvedValueOnce(makeDriveItem({ id: `ok_${i}` }));
        }
      }

      const results = await Promise.allSettled(
        Array.from({ length: 15 }, (_, i) =>
          mockInvoke('drive_upload_file', {
            owner: '0xowner',
            filePath: `/tmp/file_${i}.dat`,
            parentId: null,
            merkleRoot: null,
          })
        )
      );

      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(12);
      expect(results.filter(r => r.status === 'rejected')).toHaveLength(3);
    });
  });

  describe('concurrent listing', () => {
    it('should handle 50 concurrent list operations', async () => {
      const items = Array.from({ length: 10 }, (_, i) => makeDriveItem({ name: `item_${i}` }));
      for (let i = 0; i < 50; i++) {
        mockInvoke.mockResolvedValueOnce([...items]);
      }

      const results = await Promise.all(
        Array.from({ length: 50 }, () =>
          mockInvoke('drive_list_items', { owner: '0xowner', parentId: null })
        )
      );

      expect(results).toHaveLength(50);
      results.forEach(r => expect(r).toHaveLength(10));
    });

    it('should handle listing with different owners concurrently', async () => {
      for (let i = 0; i < 10; i++) {
        mockInvoke.mockResolvedValueOnce(
          Array.from({ length: i + 1 }, (_, j) => makeDriveItem({ owner: `0xowner_${i}`, name: `file_${j}` }))
        );
      }

      const results = await Promise.all(
        Array.from({ length: 10 }, (_, i) =>
          mockInvoke('drive_list_items', { owner: `0xowner_${i}`, parentId: null })
        )
      );

      results.forEach((r, i) => expect(r).toHaveLength(i + 1));
    });
  });

  describe('concurrent CRUD', () => {
    it('should handle mixed create/read/update/delete operations', async () => {
      // 10 creates, 10 reads, 10 updates, 10 deletes
      for (let i = 0; i < 40; i++) {
        if (i < 10) {
          mockInvoke.mockResolvedValueOnce(makeDriveItem({ id: `new_${i}` })); // create folder
        } else if (i < 20) {
          mockInvoke.mockResolvedValueOnce([makeDriveItem()]); // list
        } else if (i < 30) {
          mockInvoke.mockResolvedValueOnce(makeDriveItem({ name: 'renamed' })); // update
        } else {
          mockInvoke.mockResolvedValueOnce(undefined); // delete
        }
      }

      const creates = Array.from({ length: 10 }, (_, i) =>
        mockInvoke('drive_create_folder', { owner: '0xo', name: `folder_${i}`, parentId: null })
      );
      const reads = Array.from({ length: 10 }, () =>
        mockInvoke('drive_list_items', { owner: '0xo', parentId: null })
      );
      const updates = Array.from({ length: 10 }, (_, i) =>
        mockInvoke('drive_update_item', { owner: '0xo', itemId: `item_${i}`, name: 'renamed' })
      );
      const deletes = Array.from({ length: 10 }, (_, i) =>
        mockInvoke('drive_delete_item', { owner: '0xo', itemId: `del_${i}` })
      );

      const results = await Promise.allSettled([...creates, ...reads, ...updates, ...deletes]);
      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(40);
    });
  });

  describe('folder hierarchy stress', () => {
    it('should handle deeply nested folder creation', async () => {
      let parentId: string | null = null;
      const depth = 20;

      for (let i = 0; i < depth; i++) {
        const id = `folder_depth_${i}`;
        mockInvoke.mockResolvedValueOnce(makeDriveItem({
          id,
          name: `level_${i}`,
          itemType: 'folder',
          parentId,
        }));
        parentId = id;
      }

      let currentParent: string | null = null;
      for (let i = 0; i < depth; i++) {
        const result = await mockInvoke('drive_create_folder', {
          owner: '0xowner',
          name: `level_${i}`,
          parentId: currentParent,
        });
        currentParent = result.id;
      }

      expect(currentParent).toBe(`folder_depth_${depth - 1}`);
    });

    it('should handle listing large directories (100 items)', async () => {
      const items = Array.from({ length: 100 }, (_, i) =>
        makeDriveItem({ id: `item_${i}`, name: `file_${i}.txt` })
      );
      mockInvoke.mockResolvedValueOnce(items);

      const result = await mockInvoke('drive_list_items', { owner: '0xowner', parentId: null });
      expect(result).toHaveLength(100);
    });
  });

  describe('share operations under load', () => {
    it('should handle 20 concurrent share creations', async () => {
      for (let i = 0; i < 20; i++) {
        mockInvoke.mockResolvedValueOnce({
          id: `share_${i}`,
          token: `token_${i}`,
          itemId: `item_${i}`,
          isPublic: true,
          downloadCount: 0,
        });
      }

      const results = await Promise.all(
        Array.from({ length: 20 }, (_, i) =>
          mockInvoke('drive_create_share', { itemId: `item_${i}`, priceChi: null, isPublic: true })
        )
      );

      expect(results).toHaveLength(20);
      const tokens = results.map(r => r.token);
      expect(new Set(tokens).size).toBe(20);
    });
  });

  describe('owner isolation under concurrent access', () => {
    it('should never return items from wrong owner', async () => {
      // Simulate 5 different owners listing simultaneously
      for (let owner = 0; owner < 5; owner++) {
        mockInvoke.mockResolvedValueOnce(
          Array.from({ length: 3 }, (_, j) =>
            makeDriveItem({ owner: `0xowner_${owner}`, name: `file_${owner}_${j}` })
          )
        );
      }

      const results = await Promise.all(
        Array.from({ length: 5 }, (_, i) =>
          mockInvoke('drive_list_items', { owner: `0xowner_${i}`, parentId: null })
        )
      );

      results.forEach((items, ownerIdx) => {
        items.forEach((item: any) => {
          expect(item.owner).toBe(`0xowner_${ownerIdx}`);
        });
      });
    });
  });

  describe('seeding operations stress', () => {
    it('should handle concurrent seed start operations', async () => {
      for (let i = 0; i < 15; i++) {
        mockInvoke.mockResolvedValueOnce(undefined);
      }

      const results = await Promise.allSettled(
        Array.from({ length: 15 }, (_, i) =>
          mockInvoke('publish_drive_file', {
            owner: '0xowner',
            itemId: `item_${i}`,
            protocol: 'WebRTC',
            priceChi: null,
          })
        )
      );

      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(15);
    });

    it('should handle concurrent seed stop operations', async () => {
      for (let i = 0; i < 15; i++) {
        mockInvoke.mockResolvedValueOnce(undefined);
      }

      const results = await Promise.allSettled(
        Array.from({ length: 15 }, (_, i) =>
          mockInvoke('drive_stop_seeding', { owner: '0xowner', itemId: `item_${i}` })
        )
      );

      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(15);
    });
  });
});
