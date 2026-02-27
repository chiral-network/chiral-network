const RELAY_BASE = 'http://130.245.173.73:8080';

/** Local Drive server URL — set when Tauri app starts */
let localBase: string | null = null;

/** Set the local Drive server URL for CRUD operations */
export function setLocalDriveServer(url: string) {
  localBase = url;
}

/** Get the base URL for CRUD operations (local server if available, relay as fallback) */
function getCrudBase(): string {
  return localBase || RELAY_BASE;
}

/** Current owner wallet address — set via setOwner() */
let currentOwner = '';

/** Set the owner wallet address for all Drive API requests */
export function setDriveOwner(address: string) {
  currentOwner = address;
}

export interface DriveItem {
  id: string;
  name: string;
  itemType: string; // "file" or "folder"
  parentId?: string | null;
  size?: number;
  mimeType?: string;
  createdAt: number;
  modifiedAt: number;
  starred: boolean;
  storagePath?: string;
}

export interface ShareLink {
  id: string;
  itemId: string;
  url: string;
  isPublic: boolean;
  hasPassword: boolean;
  createdAt: number;
  downloadCount: number;
}

/** Check if running inside Tauri */
let _isTauri: boolean | null = null;
function isTauri(): boolean {
  if (_isTauri === null) {
    _isTauri = !!(window as any).__TAURI_INTERNALS__;
  }
  return _isTauri;
}

/** Lazy-loaded invoke function */
let _invoke: ((cmd: string, args?: Record<string, unknown>) => Promise<any>) | null = null;
async function getInvoke() {
  if (!_invoke) {
    const { invoke } = await import('@tauri-apps/api/core');
    _invoke = invoke;
  }
  return _invoke;
}

/** Convert Tauri command result (snake_case) to frontend format (camelCase) */
function convertItem(raw: any): DriveItem {
  return {
    id: raw.id,
    name: raw.name,
    itemType: raw.item_type ?? raw.itemType ?? 'file',
    parentId: raw.parent_id ?? raw.parentId ?? null,
    size: raw.size ?? undefined,
    mimeType: raw.mime_type ?? raw.mimeType ?? undefined,
    createdAt: raw.created_at ?? raw.createdAt ?? 0,
    modifiedAt: raw.modified_at ?? raw.modifiedAt ?? 0,
    starred: raw.starred ?? false,
    storagePath: raw.storage_path ?? raw.storagePath ?? undefined,
  };
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${getCrudBase()}${path}`, {
    ...init,
    headers: {
      ...(init?.headers || {}),
      ...(currentOwner ? { 'X-Owner': currentOwner } : {}),
    },
  });
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(text || `HTTP ${res.status}`);
  }
  const contentType = res.headers.get('content-type') || '';
  if (contentType.includes('application/json')) {
    return res.json();
  }
  return (await res.text()) as unknown as T;
}

export const driveApi = {
  /** List items in a folder (null parentId = root) */
  async listItems(parentId?: string | null): Promise<DriveItem[]> {
    if (isTauri()) {
      const invoke = await getInvoke();
      const items: any[] = await invoke('drive_list_items', {
        owner: currentOwner,
        parentId: parentId ?? null,
      });
      return items.map(convertItem);
    }
    const params = parentId ? `?parent_id=${encodeURIComponent(parentId)}` : '';
    return request<DriveItem[]>(`/api/drive/items${params}`);
  },

  /** Create a new folder */
  async createFolder(name: string, parentId?: string | null): Promise<DriveItem> {
    if (isTauri()) {
      const invoke = await getInvoke();
      const item = await invoke('drive_create_folder', {
        owner: currentOwner,
        name,
        parentId: parentId ?? null,
      });
      return convertItem(item);
    }
    return request<DriveItem>('/api/drive/folders', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, parent_id: parentId || null }),
    });
  },

  /** Upload a file — in Tauri mode, takes a file path string instead of File object */
  async uploadFile(fileOrPath: File | string, parentId?: string | null): Promise<DriveItem> {
    if (isTauri() && typeof fileOrPath === 'string') {
      const invoke = await getInvoke();
      const item = await invoke('drive_upload_file', {
        owner: currentOwner,
        filePath: fileOrPath,
        parentId: parentId ?? null,
      });
      return convertItem(item);
    }
    // Fallback: HTTP multipart upload (web mode or File object)
    const file = fileOrPath as File;
    const formData = new FormData();
    formData.append('file', file);
    if (parentId) {
      formData.append('parent_id', parentId);
    }
    return request<DriveItem>('/api/drive/upload', {
      method: 'POST',
      body: formData,
    });
  },

  /** Update item properties (rename, move, star) */
  async updateItem(
    id: string,
    updates: { name?: string; parent_id?: string | null; starred?: boolean },
  ): Promise<DriveItem> {
    if (isTauri()) {
      const invoke = await getInvoke();
      const item = await invoke('drive_update_item', {
        owner: currentOwner,
        itemId: id,
        name: updates.name ?? null,
        parentId: updates.parent_id ?? null,
        starred: updates.starred ?? null,
      });
      return convertItem(item);
    }
    return request<DriveItem>(`/api/drive/items/${encodeURIComponent(id)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(updates),
    });
  },

  /** Delete an item (recursive for folders) */
  async deleteItem(id: string): Promise<void> {
    if (isTauri()) {
      const invoke = await getInvoke();
      await invoke('drive_delete_item', {
        owner: currentOwner,
        itemId: id,
      });
      return;
    }
    await request<string>(`/api/drive/items/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
  },

  /** Create a share link for an item */
  async createShareLink(
    itemId: string,
    password?: string,
    isPublic?: boolean,
  ): Promise<ShareLink> {
    if (isTauri()) {
      const invoke = await getInvoke();
      const share = await invoke('drive_create_share', {
        owner: currentOwner,
        itemId,
        password: password ?? null,
        isPublic: isPublic ?? false,
      });
      return share as ShareLink;
    }
    return request<ShareLink>('/api/drive/share', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        item_id: itemId,
        password: password || null,
        is_public: isPublic ?? false,
      }),
    });
  },

  /** Revoke a share link */
  async revokeShareLink(token: string): Promise<void> {
    if (isTauri()) {
      const invoke = await getInvoke();
      await invoke('drive_revoke_share', { token });
      return;
    }
    await request<string>(`/api/drive/share/${encodeURIComponent(token)}`, {
      method: 'DELETE',
    });
  },

  /** List all share links */
  async listShareLinks(): Promise<ShareLink[]> {
    if (isTauri()) {
      const invoke = await getInvoke();
      return await invoke('drive_list_shares') as ShareLink[];
    }
    return request<ShareLink[]>('/api/drive/shares');
  },

  /** Get direct download URL for a file (includes filename for correct extension) */
  getDownloadUrl(id: string, filename: string): string {
    return `${getCrudBase()}/api/drive/download/${encodeURIComponent(id)}/${encodeURIComponent(filename)}`;
  },

  /** Get public share URL */
  getShareUrl(token: string): string {
    return `${RELAY_BASE}/drive/${token}`;
  },
};
