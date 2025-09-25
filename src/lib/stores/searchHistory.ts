import { writable } from "svelte/store";
import type { FileMetadata } from "$lib/dht";

export type SearchStatus = "pending" | "found" | "not_found" | "error";

export interface SearchHistoryEntry {
  id: string;
  hash: string;
  timestamp: number;
  status: SearchStatus;
  metadata?: FileMetadata;
  errorMessage?: string;
  elapsedMs?: number;
}

const HISTORY_STORAGE_KEY = "chiral.search.history.v1";
const MAX_HISTORY_ENTRIES = 25;

function loadFromStorage(): SearchHistoryEntry[] {
  if (typeof window === "undefined") {
    return [];
  }

  try {
    const raw = window.localStorage.getItem(HISTORY_STORAGE_KEY);
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw) as SearchHistoryEntry[];
    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed
      .filter((entry) => typeof entry === "object" && entry !== null)
      .map((entry) => ({
        ...entry,
        status: entry.status ?? "pending",
        timestamp:
          typeof entry.timestamp === "number" ? entry.timestamp : Date.now(),
      }))
      .sort((a, b) => b.timestamp - a.timestamp)
      .slice(0, MAX_HISTORY_ENTRIES);
  } catch (error) {
    console.warn("Failed to load search history from storage:", error);
    return [];
  }
}

function persistToStorage(entries: SearchHistoryEntry[]) {
  if (typeof window === "undefined") {
    return;
  }

  try {
    window.localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(entries));
  } catch (error) {
    console.warn("Failed to persist search history:", error);
  }
}

function createSearchHistoryStore() {
  const initial = loadFromStorage();
  const { subscribe, update, set } = writable<SearchHistoryEntry[]>(initial);

  subscribe((value) => {
    persistToStorage(value);
  });

  function addPending(hash: string): SearchHistoryEntry {
    const id =
      typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.random().toString(16).slice(2)}`;

    const entry: SearchHistoryEntry = {
      id,
      hash: hash.trim(),
      timestamp: Date.now(),
      status: "pending",
    };

    update((entries) => {
      const next = [
        entry,
        ...entries.filter((item) => item.hash !== entry.hash),
      ];
      return next.slice(0, MAX_HISTORY_ENTRIES);
    });

    return entry;
  }

  function updateEntry(
    id: string,
    patch: Partial<Omit<SearchHistoryEntry, "id" | "hash" | "timestamp">> & {
      status: Exclude<SearchStatus, "pending">;
    },
  ) {
    update((entries) =>
      entries.map((entry) => {
        if (entry.id !== id) {
          return entry;
        }

        return {
          ...entry,
          ...patch,
          elapsedMs: patch.elapsedMs ?? Date.now() - entry.timestamp,
        };
      }),
    );
  }

  function clear() {
    set([]);
  }

  return {
    subscribe,
    addPending,
    updateEntry,
    clear,
  };
}

export const dhtSearchHistory = createSearchHistoryStore();
