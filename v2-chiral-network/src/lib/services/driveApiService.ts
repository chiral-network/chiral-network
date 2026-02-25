const RELAY_BASE = 'http://130.245.173.73:8080';

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

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${RELAY_BASE}${path}`, {
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
    const params = parentId ? `?parent_id=${encodeURIComponent(parentId)}` : '';
    return request<DriveItem[]>(`/api/drive/items${params}`);
  },

  /** Create a new folder */
  async createFolder(name: string, parentId?: string | null): Promise<DriveItem> {
    return request<DriveItem>('/api/drive/folders', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, parent_id: parentId || null }),
    });
  },

  /** Upload a file via multipart form data */
  async uploadFile(file: File, parentId?: string | null): Promise<DriveItem> {
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
    return request<DriveItem>(`/api/drive/items/${encodeURIComponent(id)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(updates),
    });
  },

  /** Delete an item (recursive for folders) */
  async deleteItem(id: string): Promise<void> {
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
    await request<string>(`/api/drive/share/${encodeURIComponent(token)}`, {
      method: 'DELETE',
    });
  },

  /** List all share links */
  async listShareLinks(): Promise<ShareLink[]> {
    return request<ShareLink[]>('/api/drive/shares');
  },

  /** Get direct download URL for a file (includes filename for correct extension) */
  getDownloadUrl(id: string, filename: string): string {
    return `${RELAY_BASE}/api/drive/download/${encodeURIComponent(id)}/${encodeURIComponent(filename)}`;
  },

  /** Get public share URL */
  getShareUrl(token: string): string {
    return `${RELAY_BASE}/drive/${token}`;
  },
};
