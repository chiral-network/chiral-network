import { writable, get } from 'svelte/store';

export interface DriveItem {
  id: string;
  name: string;
  type: 'file' | 'folder';
  parentId: string | null;
  // File-specific
  hash?: string;
  size?: number;
  mimeType?: string;
  encrypted?: boolean;
  localPath?: string;
  // Metadata
  createdAt: number;
  modifiedAt: number;
  starred: boolean;
  shared: boolean;
}

export interface DriveManifest {
  version: number;
  items: DriveItem[];
  lastModified: number;
}

const STORAGE_KEY = 'chiral.drive.v1';

function generateId(): string {
  return crypto.randomUUID?.() ?? Math.random().toString(36).substring(2, 15);
}

async function tauriInvoke(cmd: string, args?: any): Promise<any> {
  const invoke = (globalThis as any).__tauri_invoke ?? (globalThis as any).invoke;
  if (!invoke) throw new Error('tauri invoke not available');
  return invoke(cmd, args);
}

function createDriveStore() {
  const empty: DriveManifest = { version: 1, items: [], lastModified: Date.now() };
  const { subscribe, set, update } = writable<DriveManifest>(empty);

  let saveTimeout: ReturnType<typeof setTimeout> | null = null;

  function debouncedSave() {
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => persist(), 400);
  }

  async function persist() {
    const manifest = get({ subscribe });
    manifest.lastModified = Date.now();
    const json = JSON.stringify(manifest);
    try {
      if ((globalThis as any).__tauri_invoke || (globalThis as any).invoke) {
        await tauriInvoke('save_drive_manifest', { manifestJson: json });
        return;
      }
    } catch (e) {
      console.warn('tauri save_drive_manifest failed, falling back to localStorage', e);
    }
    try {
      localStorage.setItem(STORAGE_KEY, json);
    } catch (e) {
      console.error('Failed to persist drive manifest', e);
    }
  }

  return {
    subscribe,

    async load() {
      try {
        if ((globalThis as any).__tauri_invoke || (globalThis as any).invoke) {
          const res = await tauriInvoke('load_drive_manifest');
          if (res) {
            const parsed = typeof res === 'string' ? JSON.parse(res) : res;
            if (parsed && Array.isArray(parsed.items)) {
              set(parsed as DriveManifest);
              return;
            }
          }
        }
      } catch (e) {
        console.warn('tauri load_drive_manifest failed, trying localStorage', e);
      }
      try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (raw) {
          const parsed = JSON.parse(raw);
          if (parsed && Array.isArray(parsed.items)) {
            set(parsed as DriveManifest);
            return;
          }
        }
      } catch (e) {
        console.warn('Failed to load drive manifest from localStorage', e);
      }
    },

    createFolder(name: string, parentId: string | null): DriveItem {
      const item: DriveItem = {
        id: generateId(),
        name,
        type: 'folder',
        parentId,
        createdAt: Date.now(),
        modifiedAt: Date.now(),
        starred: false,
        shared: false,
      };
      update(m => {
        m.items.push(item);
        return m;
      });
      debouncedSave();
      return item;
    },

    addFile(file: { name: string; parentId: string | null; hash?: string; size?: number; localPath?: string; encrypted?: boolean }): DriveItem {
      const item: DriveItem = {
        id: generateId(),
        name: file.name,
        type: 'file',
        parentId: file.parentId,
        hash: file.hash,
        size: file.size,
        localPath: file.localPath,
        encrypted: file.encrypted,
        createdAt: Date.now(),
        modifiedAt: Date.now(),
        starred: false,
        shared: false,
      };
      update(m => {
        m.items.push(item);
        return m;
      });
      debouncedSave();
      return item;
    },

    renameItem(id: string, newName: string) {
      update(m => {
        const item = m.items.find(i => i.id === id);
        if (item) {
          item.name = newName;
          item.modifiedAt = Date.now();
        }
        return m;
      });
      debouncedSave();
    },

    moveItem(id: string, newParentId: string | null) {
      update(m => {
        const item = m.items.find(i => i.id === id);
        if (item) {
          item.parentId = newParentId;
          item.modifiedAt = Date.now();
        }
        return m;
      });
      debouncedSave();
    },

    deleteItem(id: string) {
      update(m => {
        // Collect all descendants recursively
        const toDelete = new Set<string>();
        function collectDescendants(parentId: string) {
          toDelete.add(parentId);
          m.items.filter(i => i.parentId === parentId).forEach(i => collectDescendants(i.id));
        }
        collectDescendants(id);
        m.items = m.items.filter(i => !toDelete.has(i.id));
        return m;
      });
      debouncedSave();
    },

    toggleStar(id: string) {
      update(m => {
        const item = m.items.find(i => i.id === id);
        if (item) item.starred = !item.starred;
        return m;
      });
      debouncedSave();
    },

    markShared(id: string) {
      update(m => {
        const item = m.items.find(i => i.id === id);
        if (item) item.shared = true;
        return m;
      });
      debouncedSave();
    },

    getChildren(parentId: string | null, manifest: DriveManifest): DriveItem[] {
      return manifest.items
        .filter(i => i.parentId === parentId)
        .sort((a, b) => {
          // Folders first, then by name
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
  };
}

export const driveStore = createDriveStore();
