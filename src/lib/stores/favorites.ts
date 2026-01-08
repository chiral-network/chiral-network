import { writable } from 'svelte/store';

export interface FavoriteFile {
  hash: string;
  name: string;
  size: number;
  addedAt: Date;
  protocol?: string;
  seeders?: number;
  leechers?: number;
}

const STORAGE_KEY = 'chiral-favorites';

function createFavoritesStore() {
  // Load from localStorage
  const stored = typeof window !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null;
  const initial: FavoriteFile[] = stored ? JSON.parse(stored) : [];
  
  // Convert date strings back to Date objects
  const favorites = initial.map(fav => ({
    ...fav,
    addedAt: new Date(fav.addedAt)
  }));

  const { subscribe, set, update } = writable<FavoriteFile[]>(favorites);

  return {
    subscribe,
    add: (file: Omit<FavoriteFile, 'addedAt'>) => {
      update(favs => {
        // Check if already exists
        if (favs.some(f => f.hash === file.hash)) {
          return favs;
        }
        const newFav: FavoriteFile = {
          ...file,
          addedAt: new Date()
        };
        const updated = [...favs, newFav];
        localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
        return updated;
      });
    },
    remove: (hash: string) => {
      update(favs => {
        const updated = favs.filter(f => f.hash !== hash);
        localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
        return updated;
      });
    },
    isFavorite: (hash: string, favs: FavoriteFile[]) => {
      return favs.some(f => f.hash === hash);
    },
    clear: () => {
      set([]);
      localStorage.removeItem(STORAGE_KEY);
    }
  };
}

export const favorites = createFavoritesStore();
