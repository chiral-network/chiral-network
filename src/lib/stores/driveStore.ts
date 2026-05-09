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
  isPublic: boolean;
  // Seeding metadata
  merkleRoot?: string;
  protocol?: 'WebRTC' | 'BitTorrent';
  priceChi?: string;
  seeding?: boolean;
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
    isPublic: item.isPublic ?? true,
    merkleRoot: item.merkleRoot,
    protocol: (item.protocol as 'WebRTC' | 'BitTorrent') || undefined,
    priceChi: item.priceChi,
    seeding: item.seeding ?? false,
  };
}

function compareDriveItems(a: DriveItem, b: DriveItem): number {
  // Starred items always float to the top.
  if (a.starred !== b.starred) return a.starred ? -1 : 1;
  // Within the same starred bucket, keep folders before files.
  if (a.type !== b.type) return a.type === 'folder' ? -1 : 1;
  // Finally sort by name (case-insensitive).
  return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
}

function normalizePriceChi(priceChi?: string | number | null): string | null {
  const raw = `${priceChi ?? ''}`.trim();
  if (!raw) return null;
  const parsed = Number(raw);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return raw;
}

/** Sync the current wallet address + signing key to the API service. */
function syncOwner(): string {
  const account = get(walletAccount);
  const addr = account?.address ?? '';
  const privateKey = account?.privateKey ?? '';
  setDriveOwner(addr, privateKey);
  return addr;
}

function createDriveStore() {
  const empty: DriveManifest = { version: 1, items: [], shares: [], lastModified: Date.now() };
  const { subscribe, set, update } = writable<DriveManifest>(empty);
  const folderCache = new Map<string, { items: DriveItem[]; expiresAt: number }>();
  const inflightFolderLoads = new Map<string, Promise<void>>();
  const FOLDER_CACHE_TTL_MS = 3000;
  let cacheOwner = '';

  function ensureCacheOwner(owner: string) {
    if (owner !== cacheOwner) {
      cacheOwner = owner;
      clearFolderCaches();
    }
  }

  function folderCacheKey(owner: string, parentId: string | null): string {
    return `${owner}::${parentId ?? '__root__'}`;
  }

  function clearFolderCaches() {
    folderCache.clear();
  }

  return {
    subscribe,

    /** Load all items from the server (fetches root-level, then all items) */
    async load() {
      const owner = syncOwner();
      ensureCacheOwner(owner);
      if (!owner) {
        // No wallet connected — clear items
        set({ version: 1, items: [], shares: [], lastModified: Date.now() });
        clearFolderCaches();
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
        folderCache.set(folderCacheKey(owner, null), {
          items: converted.map(item => ({ ...item })),
          expiresAt: Date.now() + FOLDER_CACHE_TTL_MS,
        });
      } catch (e) {
        console.error('Failed to load drive items from server:', e);
      }
    },

    /** Load items for a specific folder from the server */
    async loadFolder(parentId: string | null) {
      const owner = syncOwner();
      ensureCacheOwner(owner);
      if (!owner) {
        set({ version: 1, items: [], shares: [], lastModified: Date.now() });
        clearFolderCaches();
        return;
      }
      const key = folderCacheKey(owner, parentId);
      const cached = folderCache.get(key);
      if (cached && cached.expiresAt > Date.now()) {
        const m = get({ subscribe });
        const otherItems = m.items.filter(i => i.parentId !== parentId);
        set({
          ...m,
          items: [...otherItems, ...cached.items.map(item => ({ ...item }))],
          lastModified: Date.now(),
        });
        return;
      }

      const existing = inflightFolderLoads.get(key);
      if (existing) {
        await existing;
        return;
      }

      const loader = (async () => {
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
          folderCache.set(key, {
            items: newItems.map(item => ({ ...item })),
            expiresAt: Date.now() + FOLDER_CACHE_TTL_MS,
          });
          set({
            ...m,
            items: [...otherItems, ...newItems],
            lastModified: Date.now(),
          });
        } catch (e) {
          console.error('Failed to load folder:', e);
        }
      })();

      inflightFolderLoads.set(key, loader);
      try {
        await loader;
      } finally {
        inflightFolderLoads.delete(key);
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
        clearFolderCaches();
        return converted;
      } catch (e) {
        console.error('Failed to create folder:', e);
        return null;
      }
    },

    async uploadFile(fileOrPath: File | string, parentId: string | null): Promise<DriveItem | null> {
      syncOwner();
      try {
        const item = await driveApi.uploadFile(fileOrPath, parentId);
        const converted = fromApi(item);
        update(m => {
          m.items.push(converted);
          return m;
        });
        clearFolderCaches();
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
        clearFolderCaches();
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
        clearFolderCaches();
      } catch (e) {
        console.error('Failed to move item:', e);
      }
    },

    async updatePrice(id: string, priceChi: string) {
      syncOwner();
      try {
        await driveApi.updateItem(id, { price_chi: priceChi });
        update(m => {
          const item = m.items.find(i => i.id === id);
          if (item) {
            item.priceChi = priceChi || undefined;
            item.modifiedAt = Date.now();
          }
          return m;
        });
      } catch (e) {
        console.error('Failed to update price:', e);
        throw e;
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
        clearFolderCaches();
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
        const updated = await driveApi.updateItem(id, { starred: newStarred });
        const converted = fromApi(updated);
        update(m => {
          const idx = m.items.findIndex(i => i.id === id);
          if (idx >= 0) {
            converted.shared = m.items[idx].shared;
            m.items[idx] = converted;
          }
          return m;
        });
        clearFolderCaches();
      } catch (e) {
        console.error('Failed to toggle star:', e);
      }
    },

    async toggleVisibility(id: string) {
      syncOwner();
      const m = get({ subscribe });
      const item = m.items.find(i => i.id === id);
      if (!item) return;
      const newPublic = !item.isPublic;
      try {
        await driveApi.toggleVisibility(id, newPublic);
        update(m => {
          const found = m.items.find(i => i.id === id);
          if (found) {
            found.isPublic = newPublic;
            found.modifiedAt = Date.now();
          }
          return m;
        });
        clearFolderCaches();
      } catch (e) {
        console.error('Failed to toggle visibility:', e);
      }
    },

    async createShareLink(itemId: string, priceChi: string, isPublic?: boolean): Promise<ShareLink | null> {
      const owner = syncOwner();
      try {
        const share = await driveApi.createShareLink(itemId, priceChi, isPublic);
        update(m => {
          m.shares.push(share);
          const item = m.items.find(i => i.id === itemId);
          if (item) item.shared = true;
          return m;
        });
        clearFolderCaches();

        // Publish share metadata to relay so the share URL works via proxy
        try {
          const account = get(walletAccount);
          const privateKey = account?.privateKey ?? '';
          if (!owner || !privateKey) {
            throw new Error('wallet locked');
          }
          const { invoke } = await import('@tauri-apps/api/core');
          await invoke('publish_drive_share', {
            shareToken: share.id,
            relayUrl: 'http://130.245.173.73:8080',
            ownerWallet: owner,
            privateKey,
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
      const owner = syncOwner();
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
        clearFolderCaches();

        // Best-effort: remove share registration from relay
        try {
          const account = get(walletAccount);
          const privateKey = account?.privateKey ?? '';
          if (!owner || !privateKey) {
            throw new Error('wallet locked');
          }
          const { invoke } = await import('@tauri-apps/api/core');
          await invoke('unpublish_drive_share', {
            shareToken: token,
            relayUrl: 'http://130.245.173.73:8080',
            ownerWallet: owner,
            privateKey,
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
        .sort(compareDriveItems);
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
      return manifest.items
        .filter(i => i.name.toLowerCase().includes(q))
        .sort(compareDriveItems);
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

    /** Publish a Drive file to the P2P network (DHT seeding) */
    async seedFile(
      itemId: string,
      protocol: 'WebRTC' | 'BitTorrent',
      priceChi?: string | number | null,
    ): Promise<DriveItem | null> {
      const owner = syncOwner();
      if (!owner) return null;
      // The publisher signs every chiral_file_<hash> + chiral_seeder_*
      // record it writes (trust contract — readers drop unsigned ones).
      // Without a private key the backend returns
      // "wallet must be unlocked" instantly, which surfaces to the user
      // as an immediate seed failure. Pull the unlocked private key
      // from the wallet store and forward it on every publish.
      const account = get(walletAccount);
      const privateKey = account?.privateKey ?? '';
      if (!privateKey) {
        console.error('seedFile: wallet is locked (no privateKey)');
        return null;
      }
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const normalizedPrice = normalizePriceChi(priceChi);
        const raw = await invoke('publish_drive_file', {
          owner,
          itemId,
          protocol,
          priceChi: normalizedPrice,
          walletAddress: owner,
          privateKey,
        });
        const converted = fromApi(raw as any);
        update(m => {
          const idx = m.items.findIndex(i => i.id === itemId);
          if (idx >= 0) {
            converted.shared = m.items[idx].shared;
            m.items[idx] = converted;
          }
          return m;
        });
        clearFolderCaches();
        return converted;
      } catch (e) {
        console.error('Failed to seed file:', e);
        return null;
      }
    },

    /**
     * Sell a folder by recursing into its descendants and seeding every
     * file at the same `priceChi`. Returns `{ filesTotal, filesSucceeded,
     * filesFailed, failures }` so the caller can surface partial-success
     * counts. After the call, the manifest is reloaded from the backend
     * so per-file seeding state is reflected.
     *
     * If `onProgress` is provided, it's invoked as each child file
     * finishes publishing — backed by `drive-folder-publish-progress`
     * events from the Rust side. Useful for keeping the seed-folder
     * button responsive on large folders where the backend's parallel
     * SHA-256 + DHT publishes can take 10s+.
     */
    async seedFolder(
      folderId: string,
      protocol: 'WebRTC' | 'BitTorrent',
      priceChi?: string | number | null,
      onProgress?: (p: {
        stage: string;
        total: number;
        completed: number;
        succeeded?: number;
        failed?: number;
        fileName?: string;
        ok?: boolean;
        error?: string | null;
      }) => void,
    ): Promise<{
      filesTotal: number;
      filesSucceeded: number;
      filesFailed: number;
      failures: { itemId: string; name: string; error: string }[];
    } | null> {
      const owner = syncOwner();
      if (!owner) return null;
      // Folder publishes sign every child file's records too — wallet
      // must be unlocked. See seedFile above for context.
      const account = get(walletAccount);
      const privateKey = account?.privateKey ?? '';
      if (!privateKey) {
        console.error('seedFolder: wallet is locked (no privateKey)');
        return null;
      }
      let unlistenStage: (() => void) | null = null;
      try {
        const core = await import('@tauri-apps/api/core');
        const evt = await import('@tauri-apps/api/event');
        if (onProgress) {
          unlistenStage = await evt.listen<{
            folderId: string;
            stage: string;
            total: number;
            completed: number;
            succeeded?: number;
            failed?: number;
            fileName?: string;
            ok?: boolean;
            error?: string | null;
          }>('drive-folder-publish-progress', (e) => {
            if (e.payload.folderId !== folderId) return;
            onProgress(e.payload);
          });
        }
        const normalizedPrice = normalizePriceChi(priceChi);
        const raw = await core.invoke<{
          folder: any;
          filesTotal: number;
          filesSucceeded: number;
          filesFailed: number;
          failures: { itemId: string; name: string; error: string }[];
        }>('publish_drive_folder', {
          owner,
          folderId,
          protocol,
          priceChi: normalizedPrice,
          walletAddress: owner,
          privateKey,
        });
        await this.load();
        return {
          filesTotal: raw.filesTotal,
          filesSucceeded: raw.filesSucceeded,
          filesFailed: raw.filesFailed,
          failures: raw.failures,
        };
      } catch (e) {
        console.error('Failed to seed folder:', e);
        return null;
      } finally {
        if (unlistenStage) unlistenStage();
      }
    },

    /** Stop selling every file inside a folder. Mirror of seedFolder. */
    async stopSeedingFolder(
      folderId: string,
      onProgress?: (p: {
        stage: string;
        total: number;
        completed: number;
        succeeded?: number;
        failed?: number;
        fileName?: string;
        ok?: boolean;
        error?: string | null;
      }) => void,
    ): Promise<{
      filesTotal: number;
      filesSucceeded: number;
      filesFailed: number;
    } | null> {
      const owner = syncOwner();
      if (!owner) return null;
      let unlistenStage: (() => void) | null = null;
      try {
        const core = await import('@tauri-apps/api/core');
        const evt = await import('@tauri-apps/api/event');
        if (onProgress) {
          unlistenStage = await evt.listen<{
            folderId: string;
            stage: string;
            total: number;
            completed: number;
            succeeded?: number;
            failed?: number;
            fileName?: string;
            ok?: boolean;
            error?: string | null;
          }>('drive-folder-unpublish-progress', (e) => {
            if (e.payload.folderId !== folderId) return;
            onProgress(e.payload);
          });
        }
        const raw = await core.invoke<{
          filesTotal: number;
          filesSucceeded: number;
          filesFailed: number;
        }>('unpublish_drive_folder', { owner, folderId });
        await this.load();
        return {
          filesTotal: raw.filesTotal,
          filesSucceeded: raw.filesSucceeded,
          filesFailed: raw.filesFailed,
        };
      } catch (e) {
        console.error('Failed to stop seeding folder:', e);
        return null;
      } finally {
        if (unlistenStage) unlistenStage();
      }
    },

    /** Stop seeding a file on the P2P network. Optimistic — flips the row's
     *  `seeding` flag immediately so the badge disappears without waiting
     *  for the DHT unregister + manifest persist round-trip; reverts the
     *  flip if the backend call fails. */
    async stopSeeding(itemId: string): Promise<void> {
      const owner = syncOwner();
      if (!owner) return;
      let snapshot: { seeding?: boolean; seedEnabled?: boolean } | null = null;
      update(m => {
        const idx = m.items.findIndex(i => i.id === itemId);
        if (idx >= 0) {
          snapshot = {
            seeding: m.items[idx].seeding,
          };
          m.items[idx] = { ...m.items[idx], seeding: false };
        }
        return m;
      });
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const raw = await invoke('drive_stop_seeding', { owner, itemId });
        const converted = fromApi(raw as any);
        update(m => {
          const idx = m.items.findIndex(i => i.id === itemId);
          if (idx >= 0) {
            converted.shared = m.items[idx].shared;
            m.items[idx] = converted;
          }
          return m;
        });
        clearFolderCaches();
      } catch (e) {
        console.error('Failed to stop seeding:', e);
        // Revert the optimistic flip so the row doesn't lie about its
        // network state.
        if (snapshot) {
          const prev = snapshot;
          update(m => {
            const idx = m.items.findIndex(i => i.id === itemId);
            if (idx >= 0) {
              m.items[idx] = { ...m.items[idx], seeding: prev.seeding ?? true };
            }
            return m;
          });
        }
      }
    },

    /** Export a .torrent file for a seeded Drive file */
    async exportTorrent(itemId: string): Promise<string | null> {
      const owner = syncOwner();
      if (!owner) return null;
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        return await invoke<string>('drive_export_torrent', { owner, itemId });
      } catch (e) {
        console.error('Failed to export torrent:', e);
        return null;
      }
    },

    /** Get all items currently being seeded */
    getSeedingItems(manifest: DriveManifest): DriveItem[] {
      return manifest.items.filter(i => i.seeding);
    },
  };
}

export const driveStore = createDriveStore();
