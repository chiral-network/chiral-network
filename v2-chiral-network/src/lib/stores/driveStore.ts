import { writable, get } from 'svelte/store';
import { driveApi, setDriveOwner, type DriveItem as ApiDriveItem, type ShareLink } from '$lib/services/driveApiService';
import { walletAccount } from '$lib/stores';

export interface DriveItem {
  id: string;
  name: string;
  type: 'file' | 'folder';
  parentId: string | null;
  size?: number;
  mimeType?: string;
  createdAt: number;
  modifiedAt: number;
  starred: boolean;
  shared: boolean;
}

export interface DriveManifest {
  version: number;
  items: DriveItem[];
  shares: ShareLink[];
  lastModified: number;
}

/** Convert server API item (camelCase itemType) to frontend item (type: 'file'|'folder') */
function fromApi(item: ApiDriveItem): DriveItem {
  return {
    id: item.id,
    name: item.name,
    type: item.itemType === 'folder' ? 'folder' : 'file',
    parentId: item.parentId ?? null,
    size: item.size,
    mimeType: item.mimeType,
    createdAt: item.createdAt * 1000, // server sends seconds, frontend uses ms
    modifiedAt: item.modifiedAt * 1000,
    starred: item.starred,
    shared: false, // will be updated from shares
  };
}

/** Sync the current wallet address to the API service. */
function syncOwner(): string {
  const account = get(walletAccount);
  const addr = account?.address ?? '';
  setDriveOwner(addr);
  return addr;
}

function createDriveStore() {
  const empty: DriveManifest = { version: 1, items: [], shares: [], lastModified: Date.now() };
  const { subscribe, set, update } = writable<DriveManifest>(empty);

  return {
    subscribe,

    /** Load all items from the server (fetches root-level, then all items) */
    async load() {
      const owner = syncOwner();
      if (!owner) {
        // No wallet connected — clear items
        set({ version: 1, items: [], shares: [], lastModified: Date.now() });
        return;
      }
      try {
        const [items, shares] = await Promise.all([
          driveApi.listItems(null),
          driveApi.listShareLinks(),
        ]);
        const sharedIds = new Set(shares.map(s => s.itemId));
        const converted = items.map(i => {
          const item = fromApi(i);
          item.shared = sharedIds.has(item.id);
          return item;
        });
        set({
          version: 1,
          items: converted,
          shares,
          lastModified: Date.now(),
        });
      } catch (e) {
        console.error('Failed to load drive items from server:', e);
      }
    },

    /** Load items for a specific folder from the server */
    async loadFolder(parentId: string | null) {
      syncOwner();
      try {
        const items = await driveApi.listItems(parentId);
        const m = get({ subscribe });
        const sharedIds = new Set(m.shares.map(s => s.itemId));
        // Replace items for this parentId
        const otherItems = m.items.filter(i => i.parentId !== parentId);
        const newItems = items.map(i => {
          const item = fromApi(i);
          item.shared = sharedIds.has(item.id);
          return item;
        });
        set({
          ...m,
          items: [...otherItems, ...newItems],
          lastModified: Date.now(),
        });
      } catch (e) {
        console.error('Failed to load folder:', e);
      }
    },

    async createFolder(name: string, parentId: string | null): Promise<DriveItem | null> {
      syncOwner();
      try {
        const item = await driveApi.createFolder(name, parentId);
        const converted = fromApi(item);
        update(m => {
          m.items.push(converted);
          return m;
        });
        return converted;
      } catch (e) {
        console.error('Failed to create folder:', e);
        return null;
      }
    },

    async uploadFile(file: File, parentId: string | null): Promise<DriveItem | null> {
      syncOwner();
      try {
        const item = await driveApi.uploadFile(file, parentId);
        const converted = fromApi(item);
        update(m => {
          m.items.push(converted);
          return m;
        });
        return converted;
      } catch (e) {
        console.error('Failed to upload file:', e);
        return null;
      }
    },

    async renameItem(id: string, newName: string) {
      syncOwner();
      try {
        await driveApi.updateItem(id, { name: newName });
        update(m => {
          const item = m.items.find(i => i.id === id);
          if (item) {
            item.name = newName;
            item.modifiedAt = Date.now();
          }
          return m;
        });
      } catch (e) {
        console.error('Failed to rename item:', e);
      }
    },

    async moveItem(id: string, newParentId: string | null) {
      syncOwner();
      try {
        await driveApi.updateItem(id, { parent_id: newParentId ?? '' });
        update(m => {
          const item = m.items.find(i => i.id === id);
          if (item) {
            item.parentId = newParentId;
            item.modifiedAt = Date.now();
          }
          return m;
        });
      } catch (e) {
        console.error('Failed to move item:', e);
      }
    },

    async deleteItem(id: string) {
      syncOwner();
      try {
        await driveApi.deleteItem(id);
        update(m => {
          // Remove the item and all descendants locally
          const toDelete = new Set<string>();
          function collectDescendants(parentId: string) {
            toDelete.add(parentId);
            m.items.filter(i => i.parentId === parentId).forEach(i => collectDescendants(i.id));
          }
          collectDescendants(id);
          m.items = m.items.filter(i => !toDelete.has(i.id));
          m.shares = m.shares.filter(s => !toDelete.has(s.itemId));
          return m;
        });
      } catch (e) {
        console.error('Failed to delete item:', e);
      }
    },

    async toggleStar(id: string) {
      syncOwner();
      const m = get({ subscribe });
      const item = m.items.find(i => i.id === id);
      if (!item) return;
      const newStarred = !item.starred;
      try {
        await driveApi.updateItem(id, { starred: newStarred });
        update(m => {
          const found = m.items.find(i => i.id === id);
          if (found) found.starred = newStarred;
          return m;
        });
      } catch (e) {
        console.error('Failed to toggle star:', e);
      }
    },

    async createShareLink(itemId: string, password?: string, isPublic?: boolean): Promise<ShareLink | null> {
      const owner = syncOwner();
      try {
        const share = await driveApi.createShareLink(itemId, password, isPublic);
        update(m => {
          m.shares.push(share);
          const item = m.items.find(i => i.id === itemId);
          if (item) item.shared = true;
          return m;
        });

        // Publish share metadata to relay so the share URL works via proxy
        try {
          const { invoke } = await import('@tauri-apps/api/core');
          await invoke('publish_drive_share', {
            shareToken: share.id,
            relayUrl: 'http://130.245.173.73:8080',
            ownerWallet: owner,
          });
        } catch (e) {
          console.warn('Failed to publish share to relay (share works locally only):', e);
        }

        return share;
      } catch (e) {
        console.error('Failed to create share link:', e);
        return null;
      }
    },

    async revokeShareLink(token: string) {
      syncOwner();
      try {
        await driveApi.revokeShareLink(token);
        update(m => {
          const share = m.shares.find(s => s.id === token);
          m.shares = m.shares.filter(s => s.id !== token);
          // Check if item still has other shares
          if (share) {
            const remaining = m.shares.filter(s => s.itemId === share.itemId);
            if (remaining.length === 0) {
              const item = m.items.find(i => i.id === share.itemId);
              if (item) item.shared = false;
            }
          }
          return m;
        });

        // Best-effort: remove share registration from relay
        try {
          const { invoke } = await import('@tauri-apps/api/core');
          await invoke('unpublish_drive_share', {
            shareToken: token,
            relayUrl: 'http://130.245.173.73:8080',
          });
        } catch {
          // Relay cleanup failure is non-critical
        }
      } catch (e) {
        console.error('Failed to revoke share link:', e);
      }
    },

    getSharesForItem(itemId: string, manifest: DriveManifest): ShareLink[] {
      return manifest.shares.filter(s => s.itemId === itemId);
    },

    getChildren(parentId: string | null, manifest: DriveManifest): DriveItem[] {
      return manifest.items
        .filter(i => i.parentId === parentId)
        .sort((a, b) => {
          if (a.type !== b.type) return a.type === 'folder' ? -1 : 1;
          return a.name.localeCompare(b.name);
        });
    },

    getBreadcrumb(itemId: string | null, manifest: DriveManifest): DriveItem[] {
      const crumbs: DriveItem[] = [];
      let current = itemId;
      while (current) {
        const item = manifest.items.find(i => i.id === current);
        if (!item) break;
        crumbs.unshift(item);
        current = item.parentId;
      }
      return crumbs;
    },

    searchByName(query: string, manifest: DriveManifest): DriveItem[] {
      const q = query.toLowerCase();
      return manifest.items.filter(i => i.name.toLowerCase().includes(q));
    },

    getItem(id: string, manifest: DriveManifest): DriveItem | undefined {
      return manifest.items.find(i => i.id === id);
    },

    getAllFolders(manifest: DriveManifest): DriveItem[] {
      return manifest.items.filter(i => i.type === 'folder');
    },

    getDownloadUrl(id: string, filename: string): string {
      return driveApi.getDownloadUrl(id, filename);
    },

    getShareUrl(token: string): string {
      return driveApi.getShareUrl(token);
    },
  };
}

export const driveStore = createDriveStore();
