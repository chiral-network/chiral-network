// Tests for Upload and Download page logic
// Tests file processing, drag-and-drop handling, seeding persistence,
// download queue management, and file hash operations.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

// Mock Tauri APIs
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({
    onDragDropEvent: vi.fn(() => Promise.resolve(() => {})),
  })),
}));

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] || null,
    setItem: (key: string, value: string) => { store[key] = value; },
    removeItem: (key: string) => { delete store[key]; },
    clear: () => { store = {}; },
  };
})();
global.localStorage = localStorageMock as any;

const mockInvoke = vi.mocked(invoke);

// =========================================================================
// Types matching Upload.svelte
// =========================================================================

type Protocol = "WebRTC" | "BitTorrent";

interface SharedFile {
  id: string;
  name: string;
  size: number;
  hash: string;
  protocol: Protocol;
  fileType: string;
  seeders: number;
  uploadDate: Date;
  filePath: string;
  priceChr: string;
}

// =========================================================================
// Helper functions mirroring Upload.svelte logic
// =========================================================================

function getFileType(fileName: string): string {
  const ext = fileName.split(".").pop()?.toLowerCase() || "";
  if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico"].includes(ext)) return "Image";
  if (["mp4", "avi", "mkv", "mov", "wmv", "webm", "flv", "m4v"].includes(ext)) return "Video";
  if (["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma"].includes(ext)) return "Audio";
  if (["zip", "rar", "7z", "tar", "gz", "bz2", "xz"].includes(ext)) return "Archive";
  if (["js", "ts", "html", "css", "py", "java", "cpp", "c", "php", "rb", "go", "rs"].includes(ext)) return "Code";
  if (["txt", "md", "pdf", "doc", "docx", "rtf"].includes(ext)) return "Document";
  if (["xls", "xlsx", "csv", "ods"].includes(ext)) return "Spreadsheet";
  return "File";
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

function generateMagnetLink(file: SharedFile): string {
  const encodedName = encodeURIComponent(file.name);
  return `magnet:?xt=urn:btih:${file.hash}&dn=${encodedName}&xl=${file.size}`;
}

const UPLOAD_HISTORY_KEY = "chiral_upload_history";

function saveUploadHistory(files: SharedFile[]) {
  localStorage.setItem(UPLOAD_HISTORY_KEY, JSON.stringify(files));
}

function loadUploadHistory(): SharedFile[] {
  try {
    const stored = localStorage.getItem(UPLOAD_HISTORY_KEY);
    if (stored) {
      const parsed = JSON.parse(stored);
      return parsed.map((f: any) => ({
        ...f,
        uploadDate: new Date(f.uploadDate),
      }));
    }
  } catch {
    // ignore
  }
  return [];
}

describe("Upload Page Logic", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  // =========================================================================
  // File type detection
  // =========================================================================

  describe("File Type Detection", () => {
    it("should detect image files", () => {
      expect(getFileType("photo.jpg")).toBe("Image");
      expect(getFileType("icon.png")).toBe("Image");
      expect(getFileType("animation.gif")).toBe("Image");
      expect(getFileType("vector.svg")).toBe("Image");
      expect(getFileType("photo.webp")).toBe("Image");
    });

    it("should detect video files", () => {
      expect(getFileType("movie.mp4")).toBe("Video");
      expect(getFileType("clip.avi")).toBe("Video");
      expect(getFileType("film.mkv")).toBe("Video");
      expect(getFileType("recording.mov")).toBe("Video");
      expect(getFileType("stream.webm")).toBe("Video");
    });

    it("should detect audio files", () => {
      expect(getFileType("song.mp3")).toBe("Audio");
      expect(getFileType("music.flac")).toBe("Audio");
      expect(getFileType("track.wav")).toBe("Audio");
      expect(getFileType("podcast.ogg")).toBe("Audio");
    });

    it("should detect archive files", () => {
      expect(getFileType("backup.zip")).toBe("Archive");
      expect(getFileType("data.tar")).toBe("Archive");
      expect(getFileType("compressed.7z")).toBe("Archive");
      expect(getFileType("archive.gz")).toBe("Archive");
    });

    it("should detect code files", () => {
      expect(getFileType("app.js")).toBe("Code");
      expect(getFileType("main.rs")).toBe("Code");
      expect(getFileType("script.py")).toBe("Code");
      expect(getFileType("styles.css")).toBe("Code");
      expect(getFileType("index.html")).toBe("Code");
    });

    it("should detect document files", () => {
      expect(getFileType("readme.txt")).toBe("Document");
      expect(getFileType("notes.md")).toBe("Document");
      expect(getFileType("report.pdf")).toBe("Document");
      expect(getFileType("letter.docx")).toBe("Document");
    });

    it("should detect spreadsheet files", () => {
      expect(getFileType("data.csv")).toBe("Spreadsheet");
      expect(getFileType("budget.xlsx")).toBe("Spreadsheet");
      expect(getFileType("report.xls")).toBe("Spreadsheet");
    });

    it("should return File for unknown extensions", () => {
      expect(getFileType("data.bin")).toBe("File");
      expect(getFileType("unknown.xyz")).toBe("File");
      expect(getFileType("noext")).toBe("File");
    });

    it("should be case insensitive", () => {
      expect(getFileType("PHOTO.JPG")).toBe("Image");
      expect(getFileType("Movie.MP4")).toBe("Video");
      expect(getFileType("SONG.MP3")).toBe("Audio");
    });
  });

  // =========================================================================
  // File size formatting
  // =========================================================================

  describe("File Size Formatting", () => {
    it("should format zero bytes", () => {
      expect(formatFileSize(0)).toBe("0 B");
    });

    it("should format bytes", () => {
      expect(formatFileSize(500)).toBe("500 B");
    });

    it("should format kilobytes", () => {
      expect(formatFileSize(1024)).toBe("1 KB");
      expect(formatFileSize(1536)).toBe("1.5 KB");
    });

    it("should format megabytes", () => {
      expect(formatFileSize(1048576)).toBe("1 MB");
      expect(formatFileSize(5 * 1024 * 1024)).toBe("5 MB");
    });

    it("should format gigabytes", () => {
      expect(formatFileSize(1073741824)).toBe("1 GB");
    });

    it("should format terabytes", () => {
      expect(formatFileSize(1099511627776)).toBe("1 TB");
    });
  });

  // =========================================================================
  // Magnet link generation
  // =========================================================================

  describe("Magnet Link Generation", () => {
    it("should generate valid magnet link", () => {
      const file: SharedFile = {
        id: "f-1",
        name: "test.bin",
        size: 1048576,
        hash: "abc123def456",
        protocol: "WebRTC",
        fileType: "File",
        seeders: 1,
        uploadDate: new Date(),
        filePath: "/path/to/test.bin",
        priceChr: "0",
      };

      const magnet = generateMagnetLink(file);
      expect(magnet).toBe(
        "magnet:?xt=urn:btih:abc123def456&dn=test.bin&xl=1048576"
      );
    });

    it("should encode special characters in file name", () => {
      const file: SharedFile = {
        id: "f-1",
        name: "my file (2024).mp4",
        size: 5000000,
        hash: "hash123",
        protocol: "BitTorrent",
        fileType: "Video",
        seeders: 3,
        uploadDate: new Date(),
        filePath: "/path/to/file.mp4",
        priceChr: "0",
      };

      const magnet = generateMagnetLink(file);
      expect(magnet).toContain("dn=my%20file%20(2024).mp4");
      expect(magnet).toContain("xt=urn:btih:hash123");
    });
  });

  // =========================================================================
  // Upload history persistence
  // =========================================================================

  describe("Upload History Persistence", () => {
    it("should save and load upload history", () => {
      const files: SharedFile[] = [
        {
          id: "f-1",
          name: "doc.pdf",
          size: 1024,
          hash: "hash1",
          protocol: "WebRTC",
          fileType: "Document",
          seeders: 1,
          uploadDate: new Date("2024-01-01"),
          filePath: "/docs/doc.pdf",
          priceChr: "0",
        },
      ];

      saveUploadHistory(files);
      const loaded = loadUploadHistory();

      expect(loaded).toHaveLength(1);
      expect(loaded[0].name).toBe("doc.pdf");
      expect(loaded[0].hash).toBe("hash1");
      expect(loaded[0].uploadDate).toBeInstanceOf(Date);
    });

    it("should preserve all file properties across save/load", () => {
      const file: SharedFile = {
        id: "f-test",
        name: "video.mkv",
        size: 1073741824,
        hash: "merkle_root_abc",
        protocol: "BitTorrent",
        fileType: "Video",
        seeders: 5,
        uploadDate: new Date("2024-06-15T12:00:00Z"),
        filePath: "/media/video.mkv",
        priceChr: "2.5",
      };

      saveUploadHistory([file]);
      const [loaded] = loadUploadHistory();

      expect(loaded.id).toBe("f-test");
      expect(loaded.name).toBe("video.mkv");
      expect(loaded.size).toBe(1073741824);
      expect(loaded.hash).toBe("merkle_root_abc");
      expect(loaded.protocol).toBe("BitTorrent");
      expect(loaded.fileType).toBe("Video");
      expect(loaded.seeders).toBe(5);
      expect(loaded.priceChr).toBe("2.5");
    });

    it("should handle empty upload history", () => {
      saveUploadHistory([]);
      const loaded = loadUploadHistory();
      expect(loaded).toEqual([]);
    });

    it("should handle corrupted localStorage gracefully", () => {
      localStorage.setItem(UPLOAD_HISTORY_KEY, "not valid json!");
      const loaded = loadUploadHistory();
      expect(loaded).toEqual([]);
    });

    it("should support multiple files in history", () => {
      const files: SharedFile[] = Array.from({ length: 20 }, (_, i) => ({
        id: `f-${i}`,
        name: `file-${i}.dat`,
        size: (i + 1) * 1024,
        hash: `hash-${i}`,
        protocol: (i % 2 === 0 ? "WebRTC" : "BitTorrent") as Protocol,
        fileType: "File",
        seeders: 1,
        uploadDate: new Date(),
        filePath: `/path/file-${i}.dat`,
        priceChr: "0",
      }));

      saveUploadHistory(files);
      const loaded = loadUploadHistory();
      expect(loaded).toHaveLength(20);
    });
  });

  // =========================================================================
  // File publishing via Tauri
  // =========================================================================

  describe("File Publishing", () => {
    it("should call publish_file with correct parameters", async () => {
      mockInvoke.mockResolvedValueOnce({ merkleRoot: "merkle_root_123" });

      const result = await invoke<{ merkleRoot: string }>("publish_file", {
        filePath: "/home/user/document.pdf",
        fileName: "document.pdf",
        protocol: "WebRTC",
        priceChr: null,
        walletAddress: null,
      });

      expect(result.merkleRoot).toBe("merkle_root_123");
      expect(mockInvoke).toHaveBeenCalledWith("publish_file", {
        filePath: "/home/user/document.pdf",
        fileName: "document.pdf",
        protocol: "WebRTC",
        priceChr: null,
        walletAddress: null,
      });
    });

    it("should publish file with price and wallet", async () => {
      mockInvoke.mockResolvedValueOnce({ merkleRoot: "root_456" });

      await invoke("publish_file", {
        filePath: "/home/user/premium.zip",
        fileName: "premium.zip",
        protocol: "BitTorrent",
        priceChr: "5.0",
        walletAddress: "0x1234567890abcdef1234567890abcdef12345678",
      });

      expect(mockInvoke).toHaveBeenCalledWith("publish_file", {
        filePath: "/home/user/premium.zip",
        fileName: "premium.zip",
        protocol: "BitTorrent",
        priceChr: "5.0",
        walletAddress: "0x1234567890abcdef1234567890abcdef12345678",
      });
    });

    it("should handle publish_file error", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("File not found"));

      await expect(
        invoke("publish_file", {
          filePath: "/nonexistent/file.txt",
          fileName: "file.txt",
          protocol: "WebRTC",
          priceChr: null,
          walletAddress: null,
        })
      ).rejects.toThrow("File not found");
    });
  });

  // =========================================================================
  // Drag and drop handling
  // =========================================================================

  describe("Drag and Drop", () => {
    it("should extract file paths from multiple dropped files", () => {
      const paths = ["/home/user/file1.pdf", "/home/user/file2.mp4", "/home/user/file3.zip"];

      // Simulate path extraction
      const extractedPaths: string[] = [];
      for (const path of paths) {
        extractedPaths.push(path);
      }

      expect(extractedPaths).toHaveLength(3);
      expect(extractedPaths[0]).toBe("/home/user/file1.pdf");
    });

    it("should extract file name from path (Unix)", () => {
      const path = "/home/user/documents/report.pdf";
      const fileName = path.split(/[/\\]/).pop() || "Unknown";
      expect(fileName).toBe("report.pdf");
    });

    it("should extract file name from path (Windows)", () => {
      const path = "C:\\Users\\user\\Documents\\report.pdf";
      const fileName = path.split(/[/\\]/).pop() || "Unknown";
      expect(fileName).toBe("report.pdf");
    });

    it("should handle path with no separators", () => {
      const path = "file.txt";
      const fileName = path.split(/[/\\]/).pop() || "Unknown";
      expect(fileName).toBe("file.txt");
    });
  });

  // =========================================================================
  // Re-registration of shared files
  // =========================================================================

  describe("Shared File Re-registration", () => {
    it("should call register_shared_file for each shared file", async () => {
      mockInvoke.mockResolvedValue(null);

      const files: SharedFile[] = [
        {
          id: "f-1", name: "a.pdf", size: 1024, hash: "hash1",
          protocol: "WebRTC", fileType: "Document", seeders: 1,
          uploadDate: new Date(), filePath: "/a.pdf", priceChr: "0",
        },
        {
          id: "f-2", name: "b.mp4", size: 5000000, hash: "hash2",
          protocol: "BitTorrent", fileType: "Video", seeders: 2,
          uploadDate: new Date(), filePath: "/b.mp4", priceChr: "1.5",
        },
      ];

      for (const file of files) {
        await invoke("register_shared_file", {
          fileHash: file.hash,
          filePath: file.filePath,
          fileName: file.name,
          fileSize: file.size,
          priceChr: file.priceChr !== "0" ? file.priceChr : null,
          walletAddress: file.priceChr !== "0" ? "0xwallet" : null,
        });
      }

      expect(mockInvoke).toHaveBeenCalledTimes(2);
    });
  });
});

describe("Download Page Logic", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  // =========================================================================
  // Download queue management
  // =========================================================================

  describe("Download Queue", () => {
    interface DownloadItem {
      id: string;
      hash: string;
      name: string;
      size: number;
      status: "queued" | "downloading" | "completed" | "failed" | "paused";
      progress: number;
      speed: number;
      peersConnected: number;
      sourcePeers: string[];
      addedAt: number;
      completedAt?: number;
    }

    it("should queue downloads in order", () => {
      const queue: DownloadItem[] = [];

      for (let i = 0; i < 5; i++) {
        queue.push({
          id: `dl-${i}`,
          hash: `hash-${i}`,
          name: `file-${i}.dat`,
          size: 1024 * (i + 1),
          status: "queued",
          progress: 0,
          speed: 0,
          peersConnected: 0,
          sourcePeers: [],
          addedAt: Date.now() + i,
        });
      }

      expect(queue).toHaveLength(5);
      expect(queue[0].name).toBe("file-0.dat");
      expect(queue[4].name).toBe("file-4.dat");
    });

    it("should track download progress per item", () => {
      const item: DownloadItem = {
        id: "dl-1",
        hash: "hash-1",
        name: "large.bin",
        size: 1048576,
        status: "downloading",
        progress: 0,
        speed: 0,
        peersConnected: 3,
        sourcePeers: ["peer-1", "peer-2", "peer-3"],
        addedAt: Date.now(),
      };

      // Simulate progress updates
      item.progress = 25;
      item.speed = 512000;
      expect(item.progress).toBe(25);

      item.progress = 100;
      item.status = "completed";
      item.completedAt = Date.now();
      expect(item.status).toBe("completed");
      expect(item.completedAt).toBeDefined();
    });

    it("should track multiple peers per download", () => {
      const item: DownloadItem = {
        id: "dl-1",
        hash: "hash-1",
        name: "popular.bin",
        size: 10485760,
        status: "downloading",
        progress: 0,
        speed: 0,
        peersConnected: 0,
        sourcePeers: [],
        addedAt: Date.now(),
      };

      // Discover seeders
      item.sourcePeers.push("seeder-1");
      item.sourcePeers.push("seeder-2");
      item.sourcePeers.push("seeder-3");
      item.sourcePeers.push("seeder-4");
      item.sourcePeers.push("seeder-5");
      item.peersConnected = item.sourcePeers.length;

      expect(item.sourcePeers).toHaveLength(5);
      expect(item.peersConnected).toBe(5);
    });

    it("should handle download failure and retry", () => {
      const item: DownloadItem = {
        id: "dl-1",
        hash: "hash-1",
        name: "retry.bin",
        size: 1024,
        status: "downloading",
        progress: 50,
        speed: 1024,
        peersConnected: 1,
        sourcePeers: ["peer-1"],
        addedAt: Date.now(),
      };

      // Simulate failure
      item.status = "failed";
      item.speed = 0;
      expect(item.status).toBe("failed");

      // Retry
      item.status = "queued";
      item.progress = 50; // resume from where we left off
      expect(item.status).toBe("queued");
    });

    it("should support pausing and resuming", () => {
      const item: DownloadItem = {
        id: "dl-1",
        hash: "hash-1",
        name: "pausable.bin",
        size: 10485760,
        status: "downloading",
        progress: 30,
        speed: 256000,
        peersConnected: 2,
        sourcePeers: ["peer-1", "peer-2"],
        addedAt: Date.now(),
      };

      // Pause
      item.status = "paused";
      item.speed = 0;
      expect(item.status).toBe("paused");

      // Resume
      item.status = "downloading";
      item.speed = 256000;
      expect(item.status).toBe("downloading");
      expect(item.progress).toBe(30); // preserved
    });
  });

  // =========================================================================
  // Download search
  // =========================================================================

  describe("Download Search", () => {
    it("should search files by merkle hash via DHT", async () => {
      const mockMetadata = {
        name: "found-file.bin",
        size: 2048576,
        chunkCount: 8,
        merkleRoot: "search_hash_abc",
        seeders: 3,
      };

      mockInvoke.mockResolvedValueOnce(mockMetadata);

      const result = await invoke("search_file_by_hash", {
        merkleRoot: "search_hash_abc",
      });

      expect(result).toEqual(mockMetadata);
      expect(mockInvoke).toHaveBeenCalledWith("search_file_by_hash", {
        merkleRoot: "search_hash_abc",
      });
    });

    it("should handle file not found", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("No providers found"));

      await expect(
        invoke("search_file_by_hash", { merkleRoot: "nonexistent_hash" })
      ).rejects.toThrow("No providers found");
    });
  });

  // =========================================================================
  // Seeder discovery
  // =========================================================================

  describe("Seeder Discovery", () => {
    it("should discover multiple seeders for a file", async () => {
      const seeders = [
        { peerId: "peer-1", latency: 50, bandwidth: 10000000 },
        { peerId: "peer-2", latency: 100, bandwidth: 5000000 },
        { peerId: "peer-3", latency: 200, bandwidth: 2000000 },
      ];

      mockInvoke.mockResolvedValueOnce(seeders);

      const result = await invoke("get_seeders_for_file", {
        merkleRoot: "file_hash_123",
      });

      expect(result).toHaveLength(3);
    });

    it("should allow choosing specific seeders", () => {
      const allSeeders = [
        { peerId: "fast", latency: 20, bandwidth: 50000000, reputation: 95 },
        { peerId: "medium", latency: 100, bandwidth: 10000000, reputation: 80 },
        { peerId: "slow", latency: 500, bandwidth: 1000000, reputation: 60 },
      ];

      // User selects specific seeders
      const selectedSeeders = allSeeders.filter(
        (s) => s.reputation >= 80
      );

      expect(selectedSeeders).toHaveLength(2);
      expect(selectedSeeders[0].peerId).toBe("fast");
      expect(selectedSeeders[1].peerId).toBe("medium");
    });

    it("should sort seeders by reputation then latency", () => {
      const seeders = [
        { peerId: "c", reputation: 80, latency: 50 },
        { peerId: "a", reputation: 95, latency: 100 },
        { peerId: "b", reputation: 95, latency: 30 },
        { peerId: "d", reputation: 70, latency: 20 },
      ];

      const sorted = [...seeders].sort((a, b) => {
        if (b.reputation !== a.reputation) return b.reputation - a.reputation;
        return a.latency - b.latency;
      });

      expect(sorted[0].peerId).toBe("b"); // 95 rep, 30ms
      expect(sorted[1].peerId).toBe("a"); // 95 rep, 100ms
      expect(sorted[2].peerId).toBe("c"); // 80 rep, 50ms
      expect(sorted[3].peerId).toBe("d"); // 70 rep, 20ms
    });
  });
});
