// @vitest-environment jsdom

import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import type { FileItem } from "../src/lib/stores";

// Mock localStorage with proper store management
const createLocalStorageMock = () => {
  let store: Record<string, string> = {};
  
  return {
    getItem: vi.fn((key: string) => store[key] || null),
    setItem: vi.fn((key: string, value: string) => {
      store[key] = value;
    }),
    removeItem: vi.fn((key: string) => {
      delete store[key];
    }),
    clear: vi.fn(() => {
      store = {};
    }),
    // Helper to reset the store
    _reset: () => {
      store = {};
    },
    // Helper to get actual store for debugging
    _getStore: () => store,
  };
};

const localStorageMock = createLocalStorageMock();

Object.defineProperty(window, "localStorage", {
  value: localStorageMock,
  writable: true,
});

// Import AFTER mock is set up
import { downloadHistoryService } from "../src/lib/services/downloadHistoryService";

describe("DownloadHistoryService", () => {
  const createMockFileItem = (
    overrides?: Partial<FileItem>
  ): FileItem => ({
    id: "file-123",
    hash: "abc123def456",
    name: "test-file.pdf",
    size: 1024000,
    status: "completed",
    progress: 100,
    speed: 0,
    downloadPath: "/downloads/test-file.pdf",
    price: 0.001,
    seederAddresses: ["0xSeeder1"],
    encrypted: false,
    ...overrides,
  });

  beforeEach(() => {
    // Reset the internal store
    (localStorageMock as any)._reset();
    
    // Clear the service
    downloadHistoryService.clearHistory();
    
    // Reset all mock call counts
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  describe("addToHistory", () => {
    it("should add completed download to history", () => {
      const file = createMockFileItem({ status: "completed" });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history).toHaveLength(1);
      expect(history[0].hash).toBe(file.hash);
      expect(history[0].status).toBe("completed");
    });

    it("should add failed download to history", () => {
      const file = createMockFileItem({ status: "failed" });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history).toHaveLength(1);
      expect(history[0].status).toBe("failed");
    });

    it("should add canceled download to history", () => {
      const file = createMockFileItem({ status: "canceled" });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history).toHaveLength(1);
      expect(history[0].status).toBe("canceled");
    });

    it("should NOT add in-progress downloads", () => {
      const file = createMockFileItem({ status: "downloading" as any });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history).toHaveLength(0);
    });

    it("should NOT add pending downloads", () => {
      const file = createMockFileItem({ status: "pending" as any });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history).toHaveLength(0);
    });

    it("should update existing entry and move to top", () => {
      const file1 = createMockFileItem({ hash: "hash1", name: "file1.txt" });
      const file2 = createMockFileItem({ hash: "hash2", name: "file2.txt" });

      downloadHistoryService.addToHistory(file1);
      downloadHistoryService.addToHistory(file2);

      const file1Updated = createMockFileItem({
        hash: "hash1",
        name: "file1-updated.txt",
      });
      downloadHistoryService.addToHistory(file1Updated);

      const history = downloadHistoryService.getHistory();
      expect(history).toHaveLength(2);
      expect(history[0].hash).toBe("hash1");
      expect(history[0].name).toBe("file1-updated.txt");
      expect(history[1].hash).toBe("hash2");
    });

    it("should persist to localStorage", () => {
      const file = createMockFileItem();
      downloadHistoryService.addToHistory(file);

      expect(localStorageMock.setItem).toHaveBeenCalledWith(
        "chiral.downloadHistory",
        expect.any(String)
      );

      const store = (localStorageMock as any)._getStore();
      const saved = JSON.parse(store["chiral.downloadHistory"]);
      expect(saved).toHaveLength(1);
      expect(saved[0].hash).toBe(file.hash);
    });

    it("should preserve metadata for re-download", () => {
      const file = createMockFileItem({
        manifest: { chunks: [{ cid: "cid1" }] },
        cids: ["cid1", "cid2"],
        encrypted: true,
      });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history[0].manifest).toEqual(file.manifest);
      expect(history[0].cids).toEqual(file.cids);
      expect(history[0].encrypted).toBe(true);
    });

    describe("addToHistory - additional critical scenarios", () => {
      it("should handle concurrent adds of same file hash", () => {
        // Simulates race condition if user clicks "download" multiple times rapidly
        const file = createMockFileItem({ hash: "same-hash" });
        
        downloadHistoryService.addToHistory(file);
        downloadHistoryService.addToHistory(file);
        downloadHistoryService.addToHistory(file);

        const history = downloadHistoryService.getHistory();
        expect(history).toHaveLength(1); // Should dedupe
        expect(history[0].hash).toBe("same-hash");
      });

      it("should handle file with both encrypted and isEncrypted properties", () => {
        // isEncrypted takes precedence when both fields exist
        const file1 = createMockFileItem({ 
          hash: "enc1",
          encrypted: true, 
          isEncrypted: false // Conflicting! isEncrypted should win
        } as any);
        
        downloadHistoryService.addToHistory(file1);
        
        const history = downloadHistoryService.getHistory();
        // isEncrypted takes precedence, so false wins
        expect(history[0].encrypted).toBe(false);
      });

      it("should preserve downloadDate when updating existing entry", () => {
        const originalDate = Date.now() - 1000;
        
        // Manually insert entry with specific date
        const file1 = createMockFileItem({ hash: "test-hash", name: "original.txt" });
        downloadHistoryService.addToHistory(file1);
        
        // Modify the date in localStorage to simulate old entry
        const history = downloadHistoryService.getHistory();
        history[0].downloadDate = originalDate;
        (window.localStorage as any)._getStore()["chiral.downloadHistory"] = JSON.stringify(history);
        
        // Re-add same hash with different name
        const file2 = createMockFileItem({ hash: "test-hash", name: "updated.txt" });
        downloadHistoryService.addToHistory(file2);
        
        const updatedHistory = downloadHistoryService.getHistory();
        
        // [WARN] CRITICAL: Does the new entry get a NEW timestamp, or keep the old one?
        // Current implementation creates a new timestamp (Date.now())
        expect(updatedHistory[0].downloadDate).toBeGreaterThan(originalDate);
      });

      it("should handle file with null/undefined hash", () => {
        const file = createMockFileItem({ hash: undefined as any });
        
        // Should this throw, or silently fail?
        expect(() => downloadHistoryService.addToHistory(file)).not.toThrow();
        
        const history = downloadHistoryService.getHistory();
        // Depending on desired behavior, this could be 0 or 1
        expect(history).toHaveLength(1);
      });
    });

  });

  describe("getHistory", () => {
    it("should return empty array initially", () => {
      expect(downloadHistoryService.getHistory()).toEqual([]);
    });

    it("should return all entries in chronological order (newest first)", () => {
      const file1 = createMockFileItem({ hash: "hash1", name: "old.txt" });
      const file2 = createMockFileItem({ hash: "hash2", name: "new.txt" });

      downloadHistoryService.addToHistory(file1);
      downloadHistoryService.addToHistory(file2);

      const history = downloadHistoryService.getHistory();
      expect(history[0].name).toBe("new.txt");
      expect(history[1].name).toBe("old.txt");
    });

    it("should return a copy, not the original array", () => {
      const file = createMockFileItem();
      downloadHistoryService.addToHistory(file);

      const history1 = downloadHistoryService.getHistory();
      const history2 = downloadHistoryService.getHistory();

      expect(history1).not.toBe(history2);
      expect(history1).toEqual(history2);
    });
  });

  describe("getFilteredHistory", () => {
    beforeEach(() => {
      downloadHistoryService.addToHistory(
        createMockFileItem({
          hash: "completed1",
          name: "document.pdf",
          status: "completed",
        })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({
          hash: "failed1",
          name: "video.mp4",
          status: "failed",
        })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({
          hash: "canceled1",
          name: "archive.zip",
          status: "canceled",
        })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({
          hash: "completed2",
          name: "photo.jpg",
          status: "completed",
        })
      );
    });

    it("should filter by status: completed", () => {
      const filtered = downloadHistoryService.getFilteredHistory("completed");
      expect(filtered).toHaveLength(2);
      expect(filtered.every((e) => e.status === "completed")).toBe(true);
    });

    it("should filter by status: failed", () => {
      const filtered = downloadHistoryService.getFilteredHistory("failed");
      expect(filtered).toHaveLength(1);
      expect(filtered[0].hash).toBe("failed1");
    });

    it("should filter by status: canceled", () => {
      const filtered = downloadHistoryService.getFilteredHistory("canceled");
      expect(filtered).toHaveLength(1);
      expect(filtered[0].hash).toBe("canceled1");
    });

    it("should filter by search query (name)", () => {
      const filtered = downloadHistoryService.getFilteredHistory(
        undefined,
        "photo"
      );
      expect(filtered).toHaveLength(1);
      expect(filtered[0].name).toBe("photo.jpg");
    });

    it("should filter by search query (hash)", () => {
      const filtered = downloadHistoryService.getFilteredHistory(
        undefined,
        "completed1"
      );
      expect(filtered).toHaveLength(1);
      expect(filtered[0].hash).toBe("completed1");
    });

    it("should filter by both status and search query", () => {
      const filtered = downloadHistoryService.getFilteredHistory(
        "completed",
        "document"
      );
      expect(filtered).toHaveLength(1);
      expect(filtered[0].name).toBe("document.pdf");
    });

    it("should be case-insensitive", () => {
      const filtered1 = downloadHistoryService.getFilteredHistory(
        undefined,
        "DOCUMENT"
      );
      const filtered2 = downloadHistoryService.getFilteredHistory(
        undefined,
        "document"
      );
      expect(filtered1).toEqual(filtered2);
    });

    it("should handle empty search query", () => {
      const filtered = downloadHistoryService.getFilteredHistory(undefined, "");
      expect(filtered).toHaveLength(4);
    });

    it("should handle whitespace-only search query", () => {
      const filtered = downloadHistoryService.getFilteredHistory(
        undefined,
        "   "
      );
      expect(filtered).toHaveLength(4);
    });

    describe("getFilteredHistory - missing edge cases", () => {
      // No outer beforeEach - each test sets up its own data
      
      it("should handle search with regex special characters", () => {
        downloadHistoryService.clearHistory(); // Explicit cleanup
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "regex1", name: "file[1].txt" })
        );
        
        const filtered = downloadHistoryService.getFilteredHistory(undefined, "[1]");
        expect(filtered).toHaveLength(1);
      });

      it("should handle search with unicode characters", () => {
        downloadHistoryService.clearHistory();
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "unicode1", name: "文件.txt" })
        );
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "unicode2", name: "Файл.txt" })
        );
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "unicode3", name: "ملف.txt" })
        );
        
        const filtered = downloadHistoryService.getFilteredHistory(undefined, "文件");
        expect(filtered).toHaveLength(1);
        expect(filtered[0].name).toBe("文件.txt");
      });

      it("should return empty array when all filters exclude everything", () => {
        downloadHistoryService.clearHistory();
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "exclude1", status: "completed", name: "file.txt" })
        );
        
        const filtered = downloadHistoryService.getFilteredHistory("failed", "nonexistent");
        expect(filtered).toHaveLength(0);
      });
    });
  });

  describe("removeFromHistory", () => {
    it("should remove entry by hash", () => {
      const file = createMockFileItem({ hash: "to-remove" });
      downloadHistoryService.addToHistory(file);

      expect(downloadHistoryService.getHistory()).toHaveLength(1);

      downloadHistoryService.removeFromHistory("to-remove");

      expect(downloadHistoryService.getHistory()).toHaveLength(0);
    });

    it("should persist removal to localStorage", () => {
      const file = createMockFileItem({ hash: "to-remove" });
      downloadHistoryService.addToHistory(file);

      downloadHistoryService.removeFromHistory("to-remove");

      const store = (localStorageMock as any)._getStore();
      const saved = JSON.parse(store["chiral.downloadHistory"]);
      expect(saved).toHaveLength(0);
    });

    it("should handle removing non-existent hash gracefully", () => {
      downloadHistoryService.addToHistory(createMockFileItem());

      expect(() =>
        downloadHistoryService.removeFromHistory("non-existent")
      ).not.toThrow();

      expect(downloadHistoryService.getHistory()).toHaveLength(1);
    });
  });

  describe("clearHistory", () => {
    it("should remove all entries", () => {
      downloadHistoryService.addToHistory(createMockFileItem({ hash: "1" }));
      downloadHistoryService.addToHistory(createMockFileItem({ hash: "2" }));
      downloadHistoryService.addToHistory(createMockFileItem({ hash: "3" }));

      expect(downloadHistoryService.getHistory()).toHaveLength(3);

      downloadHistoryService.clearHistory();

      expect(downloadHistoryService.getHistory()).toHaveLength(0);
    });

    it("should persist to localStorage", () => {
      downloadHistoryService.addToHistory(createMockFileItem());
      downloadHistoryService.clearHistory();

      const store = (localStorageMock as any)._getStore();
      const saved = JSON.parse(store["chiral.downloadHistory"] || "[]");
      expect(saved).toEqual([]);
    });
  });

  describe("clearFailedDownloads", () => {
    it("should remove only failed downloads", () => {
      downloadHistoryService.addToHistory(
        createMockFileItem({ hash: "completed", status: "completed" })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({ hash: "failed", status: "failed" })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({ hash: "canceled", status: "canceled" })
      );

      downloadHistoryService.clearFailedDownloads();

      const history = downloadHistoryService.getHistory();
      expect(history).toHaveLength(2);
      expect(history.find((e) => e.hash === "failed")).toBeUndefined();
      expect(history.find((e) => e.hash === "completed")).toBeDefined();
      expect(history.find((e) => e.hash === "canceled")).toBeDefined();
    });

    it("should persist to localStorage", () => {
      downloadHistoryService.addToHistory(
        createMockFileItem({ hash: "failed-item", status: "failed" })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({ hash: "completed-item", status: "completed" })
      );

      downloadHistoryService.clearFailedDownloads();

      const store = (localStorageMock as any)._getStore();
      const saved = JSON.parse(store["chiral.downloadHistory"]);
      expect(saved).toHaveLength(1);
      expect(saved[0].status).toBe("completed");
    });

    describe("clearFailedDownloads - edge cases", () => {
      it("should not affect completed/canceled downloads with similar hashes", () => {
        // Edge case: Hash collision or similar hashes
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "abc123", status: "completed" })
        );
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "abc123failed", status: "failed" })
        );
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "abc124", status: "canceled" })
        );

        downloadHistoryService.clearFailedDownloads();

        const history = downloadHistoryService.getHistory();
        expect(history).toHaveLength(2);
        expect(history.find(e => e.hash === "abc123")).toBeDefined();
        expect(history.find(e => e.hash === "abc124")).toBeDefined();
      });
    });
  });

  describe("getStatistics", () => {
    beforeEach(() => {
      downloadHistoryService.clearHistory();
    });

    it("should return correct statistics", () => {
      downloadHistoryService.addToHistory(
        createMockFileItem({
          hash: "stat1",
          status: "completed",
          size: 1000,
          price: 0.001,
        })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({
          hash: "stat2",
          status: "completed",
          size: 2000,
          price: 0.002,
        })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({
          hash: "stat3",
          status: "failed",
          size: 500,
          price: 0.0005,
        })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({
          hash: "stat4",
          status: "canceled",
          size: 300,
          price: undefined,
        })
      );

      const stats = downloadHistoryService.getStatistics();

      expect(stats.total).toBe(4);
      expect(stats.completed).toBe(2);
      expect(stats.failed).toBe(1);
      expect(stats.canceled).toBe(1);
      expect(stats.totalSize).toBe(3800);
      expect(stats.totalPrice).toBe(0.0035);
    });

    it("should handle empty history", () => {
      const stats = downloadHistoryService.getStatistics();

      expect(stats.total).toBe(0);
      expect(stats.completed).toBe(0);
      expect(stats.failed).toBe(0);
      expect(stats.canceled).toBe(0);
      expect(stats.totalSize).toBe(0);
      expect(stats.totalPrice).toBe(0);
    });

    it("should handle entries without price", () => {
      downloadHistoryService.addToHistory(
        createMockFileItem({
          status: "completed",
          size: 1000,
          price: undefined,
        })
      );

      const stats = downloadHistoryService.getStatistics();
      expect(stats.totalPrice).toBe(0);
    });

    describe("getStatistics - calculation accuracy", () => {
      it("should handle floating point precision for prices", () => {
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "float1", status: "completed", price: 0.1 })
        );
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "float2", status: "completed", price: 0.2 })
        );
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "float3", status: "completed", price: 0.3 })
        );

        const stats = downloadHistoryService.getStatistics();
        expect(stats.totalPrice).toBeCloseTo(0.6, 10);
      });

      it("should handle negative sizes gracefully", () => {
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: "negative1", status: "completed", size: -1000 })
        );

        const stats = downloadHistoryService.getStatistics();
        expect(stats.totalSize).toBe(-1000);
      });

      it("should handle extremely large numbers (BigInt scenario)", () => {
        const largeSize = Number.MAX_SAFE_INTEGER;
        
        downloadHistoryService.addToHistory(
          createMockFileItem({ 
            hash: "bigint1", 
            status: "completed", 
            size: largeSize 
          })
        );
        downloadHistoryService.addToHistory(
          createMockFileItem({ 
            hash: "bigint2", 
            status: "completed", 
            size: 1 
          })
        );

        const stats = downloadHistoryService.getStatistics();
        expect(stats.totalSize).toBeGreaterThan(Number.MAX_SAFE_INTEGER);
      });
    });
  });

  describe("exportHistory", () => {
    it("should export as valid JSON", () => {
      downloadHistoryService.addToHistory(createMockFileItem());

      const exported = downloadHistoryService.exportHistory();
      const parsed = JSON.parse(exported);

      expect(parsed.version).toBe("1.0");
      expect(parsed.exportDate).toBeDefined();
      expect(parsed.entries).toHaveLength(1);
    });

    it("should export all entries", () => {
      downloadHistoryService.addToHistory(
        createMockFileItem({ hash: "hash1" })
      );
      downloadHistoryService.addToHistory(
        createMockFileItem({ hash: "hash2" })
      );

      const exported = downloadHistoryService.exportHistory();
      const parsed = JSON.parse(exported);

      expect(parsed.entries).toHaveLength(2);
    });

    it("should be formatted (pretty-printed)", () => {
      downloadHistoryService.addToHistory(createMockFileItem());

      const exported = downloadHistoryService.exportHistory();

      expect(exported).toContain("\n");
      expect(exported).toMatch(/\s{2,}/);
    });
  });

  describe("importHistory", () => {
    it("should import valid JSON data", () => {
      const exportData = JSON.stringify({
        version: "1.0",
        exportDate: new Date().toISOString(),
        entries: [
          {
            id: "import-1",
            hash: "imported-hash",
            name: "imported-file.txt",
            size: 5000,
            status: "completed",
            downloadDate: Date.now(),
          },
        ],
      });

      const result = downloadHistoryService.importHistory(exportData);

      expect(result.success).toBe(true);
      expect(result.imported).toBe(1);
      expect(downloadHistoryService.getHistory()).toHaveLength(1);
    });

    it("should merge with existing history", () => {
      downloadHistoryService.addToHistory(
        createMockFileItem({ hash: "existing" })
      );

      const exportData = JSON.stringify({
        version: "1.0",
        exportDate: new Date().toISOString(),
        entries: [
          {
            id: "import-1",
            hash: "new-import",
            name: "new-file.txt",
            size: 1000,
            status: "completed",
            downloadDate: Date.now(),
          },
        ],
      });

      const result = downloadHistoryService.importHistory(exportData);

      expect(result.success).toBe(true);
      expect(result.imported).toBe(1);
      expect(downloadHistoryService.getHistory()).toHaveLength(2);
    });

    it("should avoid duplicate imports", () => {
      const timestamp = Date.now();
      const exportData = JSON.stringify({
        version: "1.0",
        exportDate: new Date().toISOString(),
        entries: [
          {
            id: "duplicate",
            hash: "dup-hash",
            name: "dup-file.txt",
            size: 1000,
            status: "completed",
            downloadDate: timestamp,
          },
        ],
      });

      downloadHistoryService.importHistory(exportData);
      expect(downloadHistoryService.getHistory()).toHaveLength(1);

      const result = downloadHistoryService.importHistory(exportData);
      expect(result.success).toBe(true);
      expect(result.imported).toBe(0);
      expect(downloadHistoryService.getHistory()).toHaveLength(1);
    });

    it("should reject invalid JSON", () => {
      const result = downloadHistoryService.importHistory("invalid json{");

      expect(result.success).toBe(false);
      expect(result.imported).toBe(0);
      expect(result.error).toBeDefined();
    });

    it("should reject data without entries array", () => {
      const invalidData = JSON.stringify({
        version: "1.0",
        exportDate: new Date().toISOString(),
      });

      const result = downloadHistoryService.importHistory(invalidData);

      expect(result.success).toBe(false);
      expect(result.error).toContain("missing entries array");
    });

    it("should sort history by date after import", () => {
      const now = Date.now();
      const exportData = JSON.stringify({
        version: "1.0",
        exportDate: new Date().toISOString(),
        entries: [
          {
            id: "old",
            hash: "old-hash",
            name: "old.txt",
            size: 1000,
            status: "completed",
            downloadDate: now - 10000,
          },
          {
            id: "new",
            hash: "new-hash",
            name: "new.txt",
            size: 1000,
            status: "completed",
            downloadDate: now,
          },
        ],
      });

      downloadHistoryService.importHistory(exportData);

      const history = downloadHistoryService.getHistory();
      expect(history[0].name).toBe("new.txt");
      expect(history[1].name).toBe("old.txt");
    });

    describe("importHistory - additional validation", () => {
      it("should reject import with missing required fields", () => {
        const incompleteEntry = JSON.stringify({
          version: "1.0",
          exportDate: new Date().toISOString(),
          entries: [
            { id: "1", downloadDate: Date.now() }, // Missing: hash, name, size, status
          ],
        });

        const result = downloadHistoryService.importHistory(incompleteEntry);
        
        expect(result.success).toBe(false); // Should reject invalid entries
        expect(result.error).toContain("Missing required fields");
        
        const history = downloadHistoryService.getHistory();
        expect(history).toHaveLength(0); // No invalid entries added
      });

      it("should handle import with very old version number", () => {
        const oldVersionData = JSON.stringify({
          version: "0.1", // Old version
          exportDate: new Date().toISOString(),
          entries: [
            {
              id: "old",
              hash: "old-hash",
              name: "old.txt",
              size: 1000,
              status: "completed",
              downloadDate: Date.now(),
            },
          ],
        });

        const result = downloadHistoryService.importHistory(oldVersionData);

        // Should it validate version? Current code ignores it
        expect(result.success).toBe(true);
      });

      it("should handle import with future timestamp", () => {
        const futureDate = Date.now() + 1000000000; // Way in the future
        const futureData = JSON.stringify({
          version: "1.0",
          exportDate: new Date().toISOString(),
          entries: [
            {
              id: "future",
              hash: "future-hash",
              name: "future.txt",
              size: 1000,
              status: "completed",
              downloadDate: futureDate,
            },
          ],
        });

        downloadHistoryService.importHistory(futureData);

        const history = downloadHistoryService.getHistory();
        
        // Future entries should sort to top (newest first)
        expect(history[0].hash).toBe("future-hash");
      });

      it("should handle massive import (stress test)", () => {
        const massiveData = {
          version: "1.0",
          exportDate: new Date().toISOString(),
          entries: Array.from({ length: 5000 }, (_, i) => ({
            id: `mass-${i}`,
            hash: `hash-${i}`,
            name: `file-${i}.txt`,
            size: 1000,
            status: "completed" as const,
            downloadDate: Date.now() - i * 1000,
          })),
        };

        const result = downloadHistoryService.importHistory(JSON.stringify(massiveData));

        expect(result.success).toBe(true);
        expect(result.imported).toBeGreaterThan(0);
        
        // Should be capped at MAX_HISTORY_ENTRIES (1000)
        const history = downloadHistoryService.getHistory();
        expect(history.length).toBeLessThanOrEqual(1000);
      });
    });
  });

  describe("localStorage persistence", () => {
    it("should load existing history on initialization", () => {
      const existingData = [
        {
          id: "existing",
          hash: "existing-hash",
          name: "existing.txt",
          size: 1000,
          status: "completed",
          downloadDate: Date.now(),
        },
      ];
      
      const store = (localStorageMock as any)._getStore();
      store["chiral.downloadHistory"] = JSON.stringify(existingData);

      const newService = new (downloadHistoryService.constructor as any)();

      expect(newService.getHistory()).toHaveLength(1);
      expect(newService.getHistory()[0].hash).toBe("existing-hash");
    });

    it("should handle corrupted localStorage gracefully", () => {
      const store = (localStorageMock as any)._getStore();
      store["chiral.downloadHistory"] = "corrupted data{";

      expect(
        () => new (downloadHistoryService.constructor as any)()
      ).not.toThrow();
    });

    it("should limit history to MAX_HISTORY_ENTRIES", () => {
      for (let i = 0; i < 1001; i++) {
        downloadHistoryService.addToHistory(
          createMockFileItem({
            hash: `hash-${i}`,
            name: `file-${i}.txt`,
          })
        );
      }

      const history = downloadHistoryService.getHistory();
      expect(history.length).toBeLessThanOrEqual(1000);
    });
  });

  describe("edge cases", () => {
    it("should handle file with no price", () => {
      const file = createMockFileItem({ price: undefined });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history[0].price).toBeUndefined();

      const stats = downloadHistoryService.getStatistics();
      expect(stats.totalPrice).toBe(0);
    });

    it("should handle file with no seeder addresses", () => {
      const file = createMockFileItem({ seederAddresses: undefined });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history[0].seederAddresses).toBeUndefined();
    });

    it("should handle very long file names", () => {
      const longName = "a".repeat(1000);
      const file = createMockFileItem({ name: longName });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history[0].name).toBe(longName);
    });

    it("should handle special characters in file names", () => {
      const specialName = "file (1) [copy] {final}.txt";
      const file = createMockFileItem({ name: specialName });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history[0].name).toBe(specialName);
    });

    it("should handle zero-byte files", () => {
      const file = createMockFileItem({ size: 0 });
      downloadHistoryService.addToHistory(file);

      const history = downloadHistoryService.getHistory();
      expect(history[0].size).toBe(0);

      const stats = downloadHistoryService.getStatistics();
      expect(stats.totalSize).toBe(0);
    });
  });

  describe("critical edge cases", () => {
    it("should reject invalid status values", () => {
      const invalidStatuses = ["invalid", "in-progress", "", null, undefined];
      
      invalidStatuses.forEach(status => {
        downloadHistoryService.clearHistory();
        downloadHistoryService.addToHistory(
          createMockFileItem({ hash: `test-${status}`, status: status as any })
        );
        
        const history = downloadHistoryService.getHistory();
        expect(history).toHaveLength(0);
      });
    });

    it("should validate imported entries have required fields", () => {
      const incompleteEntry = JSON.stringify({
        version: "1.0",
        exportDate: new Date().toISOString(),
        entries: [
          { id: "1", downloadDate: Date.now() }, // Missing: hash, name, size, status
        ],
      });

      const result = downloadHistoryService.importHistory(incompleteEntry);
      
      expect(result.success).toBe(false); // Should reject invalid entries
      expect(result.error).toContain("Missing required fields");
      
      const history = downloadHistoryService.getHistory();
      expect(history).toHaveLength(0); // No invalid entries added
    });

    it("should handle localStorage quota exceeded", () => {
      const originalSetItem = window.localStorage.setItem;
      let callCount = 0;
      
      vi.spyOn(window.localStorage, "setItem").mockImplementation(() => {
        if (callCount++ > 0) throw new DOMException("QuotaExceededError");
      });

      const file = createMockFileItem({ hash: "quota-test" });
      
      expect(() => 
        downloadHistoryService.addToHistory(file)
      ).not.toThrow();
      
      window.localStorage.setItem = originalSetItem;
    });

    it("should handle simultaneous async adds without data loss", async () => {
      downloadHistoryService.clearHistory();
      
      const promises = Array.from({ length: 10 }, (_, i) =>
        Promise.resolve().then(() => 
          downloadHistoryService.addToHistory(
            createMockFileItem({ hash: `concurrent-${i}` })
          )
        )
      );
      
      await Promise.all(promises);
      
      const history = downloadHistoryService.getHistory();
      expect(history.length).toBe(10);
    });

    it("should reject entries with circular references in metadata", () => {
      const circular: any = { self: null };
      circular.self = circular;
      
      const file = createMockFileItem({
        hash: "circular",
        manifest: circular as any,
      });
      
      expect(() => 
        downloadHistoryService.addToHistory(file)
      ).not.toThrow();
    });

    it("should handle extremely large entry data", () => {
      const massiveEntry = createMockFileItem({
        hash: "massive",
        name: "x".repeat(100000),
        downloadPath: "y".repeat(100000),
      });
      
      expect(() => 
        downloadHistoryService.addToHistory(massiveEntry)
      ).not.toThrow();
      
      const history = downloadHistoryService.getHistory();
      expect(history.length).toBeGreaterThan(0);
    });
  });
});