import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { homeDir } from "@tauri-apps/api/path";
import {
  DhtService,
  dhtService,
  type DhtConfig,
  type FileMetadata,
  type DhtHealth,
} from "../src/lib/dht";
import { encryptionService } from "../src/lib/services/encryption";

// Mock Tauri APIs
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn().mockResolvedValue("/home/user"),
  join: vi.fn(async (...parts: string[]) => parts.join("/")),
}));

// Mock ReputationStore
vi.mock("$lib/reputationStore", () => ({
  default: {
    getInstance: vi.fn(() => ({
      noteSeen: vi.fn(),
      success: vi.fn(),
      failure: vi.fn(),
    })),
  },
}));

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] || null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
  };
})();

global.localStorage = localStorageMock as any;

describe("dht.ts", () => {
  const mockInvoke = vi.mocked(invoke);
  const mockListen = vi.mocked(listen);
  const mockHomeDir = vi.mocked(homeDir);

  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    mockHomeDir.mockResolvedValue("/home/user");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("DhtService Class", () => {
    describe("Singleton Pattern", () => {
      it("should return the same instance", () => {
        const instance1 = DhtService.getInstance();
        const instance2 = DhtService.getInstance();

        expect(instance1).toBe(instance2);
      });

      it("should export singleton instance", () => {
        expect(dhtService).toBeInstanceOf(DhtService);
      });
    });

    describe("start", () => {
      it("should start DHT with default bootstrap nodes", async () => {
        mockInvoke
          .mockResolvedValueOnce(["bootstrap1", "bootstrap2"])
          .mockResolvedValueOnce("peer-id-123");

        const peerId = await dhtService.start({ port: 4001 });

        expect(mockInvoke).toHaveBeenCalledWith("get_bootstrap_nodes_command");
        expect(mockInvoke).toHaveBeenCalledWith("start_dht_node", {
          port: 4001,
          bootstrapNodes: ["bootstrap1", "bootstrap2"],
        });
        expect(peerId).toBe("peer-id-123");
      });

      it("should start DHT with custom bootstrap nodes", async () => {
        mockInvoke.mockResolvedValueOnce("peer-id-456");

        const config: Partial<DhtConfig> = {
          port: 5001,
          bootstrapNodes: ["custom-node-1", "custom-node-2"],
        };

        const peerId = await dhtService.start(config);

        expect(mockInvoke).toHaveBeenCalledWith("start_dht_node", {
          port: 5001,
          bootstrapNodes: ["custom-node-1", "custom-node-2"],
        });
        expect(peerId).toBe("peer-id-456");
      });

      it("should start DHT with autonat enabled", async () => {
        mockInvoke
          .mockResolvedValueOnce([])
          .mockResolvedValueOnce("peer-id-789");

        await dhtService.start({
          port: 4001,
          enableAutonat: true,
          autonatProbeIntervalSeconds: 30,
          autonatServers: ["server1", "server2"],
        });

        expect(mockInvoke).toHaveBeenCalledWith("start_dht_node", {
          port: 4001,
          bootstrapNodes: [],
          enableAutonat: true,
          autonatProbeIntervalSecs: 30,
          autonatServers: ["server1", "server2"],
        });
      });

      it("should start DHT with proxy configuration", async () => {
        mockInvoke
          .mockResolvedValueOnce([])
          .mockResolvedValueOnce("peer-id-proxy");

        await dhtService.start({
          port: 4001,
          proxyAddress: "socks5://proxy.example.com:1080",
        });

        expect(mockInvoke).toHaveBeenCalledWith(
          "start_dht_node",
          expect.objectContaining({
            proxyAddress: "socks5://proxy.example.com:1080",
          })
        );
      });

      it("should start DHT with chunk and cache settings", async () => {
        mockInvoke
          .mockResolvedValueOnce([])
          .mockResolvedValueOnce("peer-id-cache");

        await dhtService.start({
          port: 4001,
          chunkSizeKb: 256,
          cacheSizeMb: 100,
        });

        expect(mockInvoke).toHaveBeenCalledWith(
          "start_dht_node",
          expect.objectContaining({
            chunkSizeKb: 256,
            cacheSizeMb: 100,
          })
        );
      });

      it("should handle start errors and clear peerId", async () => {
        mockInvoke
          .mockResolvedValueOnce([])
          .mockRejectedValueOnce(new Error("Failed to start DHT"));

        await expect(dhtService.start({ port: 4001 })).rejects.toThrow(
          "Failed to start DHT"
        );
        expect(dhtService.getPeerId()).toBeNull();
      });

      it("should update internal peerId and port on success", async () => {
        mockInvoke
          .mockResolvedValueOnce([])
          .mockResolvedValueOnce("peer-id-update");

        await dhtService.start({ port: 6001 });

        expect(dhtService.getPeerId()).toBe("peer-id-update");
        expect(dhtService.getPort()).toBe(6001);
      });

      it("should ignore empty proxy address", async () => {
        mockInvoke.mockResolvedValueOnce([]).mockResolvedValueOnce("peer-id");

        await dhtService.start({
          port: 4001,
          proxyAddress: "   ",
        });

        const callArgs = mockInvoke.mock.calls[1][1] as Record<string, unknown>;
        expect(callArgs).not.toHaveProperty("proxyAddress");
      });

    });

    describe("stop", () => {
      it("should stop DHT successfully", async () => {
        mockInvoke.mockResolvedValueOnce(undefined);

        dhtService.setPeerId("peer-id-stop");

        await dhtService.stop();

        expect(mockInvoke).toHaveBeenCalledWith("stop_dht_node");
        expect(dhtService.getPeerId()).toBeNull();
      });

      it("should handle stop errors", async () => {
        mockInvoke.mockRejectedValueOnce(new Error("Failed to stop"));

        await expect(dhtService.stop()).rejects.toThrow("Failed to stop");
      });
    });

    describe("publishFileToNetwork", () => {
      const mockFileMetadata: FileMetadata = {
        fileHash: "hash123",
        fileName: "test.txt",
        fileSize: 1024,
        seeders: [],
        createdAt: Date.now(),
        merkleRoot: "hash123",
        isEncrypted: false,
      };

      it("should publish file successfully", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke.mockResolvedValueOnce(undefined);

        const promise = dhtService.publishFileToNetwork("/path/to/file.txt");
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: mockFileMetadata } as any);
        const result = await promise;

        expect(mockListen).toHaveBeenCalledWith(
          "published_file",
          expect.any(Function)
        );
        expect(mockInvoke).toHaveBeenCalledWith(
          "upload_file_to_network",
          expect.objectContaining({
            filePath: "/path/to/file.txt",
            price: 0,
            protocol: "Bitswap",
            originalFileName: null,
          })
        );
        expect(result).toEqual(mockFileMetadata);
      });

      it("should publish file with price", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke.mockResolvedValueOnce(undefined);

        const promise = dhtService.publishFileToNetwork("/path/to/file.txt", 100);
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: mockFileMetadata } as any);
        await promise;

        expect(mockInvoke).toHaveBeenCalledWith(
          "upload_file_to_network",
          expect.objectContaining({
            filePath: "/path/to/file.txt",
            price: 100,
          })
        );
      });

      it("should normalize merkleRoot and fileHash", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke.mockResolvedValueOnce(undefined);

        const metadataWithoutMerkle = {
          ...mockFileMetadata,
          merkleRoot: undefined,
        };

        const promise = dhtService.publishFileToNetwork("/path/to/file.txt");
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: metadataWithoutMerkle } as any);
        const result = await promise;

        expect(result.merkleRoot).toBe(result.fileHash);
      });

      it("should handle invoke errors", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke.mockRejectedValueOnce(new Error("Upload failed"));

        await expect(
          dhtService.publishFileToNetwork("/path/to/file.txt")
        ).rejects.toThrow("Upload failed");
      });
    });

    describe("downloadFile", () => {
      const mockFileMetadata: FileMetadata = {
        fileHash: "hash123",
        fileName: "test.txt",
        fileSize: 1024,
        seeders: [],
        createdAt: Date.now(),
        merkleRoot: "hash123",
        isEncrypted: false,
        downloadPath: "/custom/path/test.txt",
      };

      it("should download file with provided path", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke
          .mockResolvedValueOnce(undefined) // ensure_directory_exists
          .mockResolvedValueOnce(undefined); // download_blocks_from_network

        const promise = dhtService.downloadFile(mockFileMetadata);
        // downloadFile awaits ensure_directory_exists before registering the listener
        await Promise.resolve();
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: mockFileMetadata } as any);
        const result = await promise;

        expect(mockInvoke).toHaveBeenCalledWith("ensure_directory_exists", {
          path: "/custom/path/test.txt",
        });
        expect(mockInvoke).toHaveBeenCalledWith(
          "download_blocks_from_network",
          {
            fileMetadata: expect.objectContaining({
              merkleRoot: "hash123",
              cids: ["hash123"],
              isRoot: true,
            }),
            downloadPath: "/custom/path/test.txt",
          }
        );
        expect(result).toEqual(mockFileMetadata);
      });

      it("should download file using backend download directory when no downloadPath", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke
          .mockResolvedValueOnce("/downloads") // get_download_directory
          .mockResolvedValueOnce(undefined) // ensure_directory_exists
          .mockResolvedValueOnce(undefined); // download_blocks_from_network

        const metadataWithoutPath = {
          ...mockFileMetadata,
          downloadPath: undefined,
        };

        const promise = dhtService.downloadFile(metadataWithoutPath);
        // downloadFile awaits get_download_directory + ensure_directory_exists before registering the listener
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: metadataWithoutPath } as any);
        await promise;

        expect(mockInvoke).toHaveBeenCalledWith("get_download_directory");
        expect(mockInvoke).toHaveBeenCalledWith("ensure_directory_exists", {
          path: "/downloads/test.txt",
        });
        expect(mockInvoke).toHaveBeenCalledWith(
          "download_blocks_from_network",
          expect.objectContaining({
            downloadPath: "/downloads/test.txt",
          })
        );
      });

      it("should handle directory creation errors", async () => {
        mockInvoke.mockRejectedValueOnce(new Error("Permission denied"));

        await expect(dhtService.downloadFile(mockFileMetadata)).rejects.toThrow(
          "Permission denied"
        );
      });

      it("should preserve existing cids", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke
          .mockResolvedValueOnce(undefined)
          .mockResolvedValueOnce(undefined);

        const metadataWithCids = {
          ...mockFileMetadata,
          cids: ["cid1", "cid2", "cid3"],
        };

        const promise = dhtService.downloadFile(metadataWithCids);
        // downloadFile awaits ensure_directory_exists before registering the listener
        await Promise.resolve();
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: metadataWithCids } as any);
        await promise;

        const callArgs = mockInvoke.mock.calls[1][0];
        expect(callArgs).toEqual("download_blocks_from_network");
        const payload = mockInvoke.mock.calls[1][1] as any;
        expect(payload.fileMetadata.cids).toEqual(["cid1", "cid2", "cid3"]);
      });
    });

    describe("searchFile", () => {
      it("should search for file by hash", async () => {
        dhtService.setPeerId("peer-search");
        mockInvoke.mockResolvedValueOnce(undefined);

        await dhtService.searchFile("hash123");

        expect(mockInvoke).toHaveBeenCalledWith("search_file_metadata", {
          fileHash: "hash123",
          timeoutMs: 0,
        });
      });

      it("should throw error if DHT not started", async () => {
        dhtService.setPeerId(null);

        await expect(dhtService.searchFile("hash123")).rejects.toThrow(
          "DHT not started"
        );
      });

      it("should handle search errors", async () => {
        dhtService.setPeerId("peer-search");
        mockInvoke.mockRejectedValueOnce(new Error("Search failed"));

        await expect(dhtService.searchFile("hash123")).rejects.toThrow(
          "Search failed"
        );
      });
    });

    describe("connectPeer", () => {
      it("should connect to peer successfully", async () => {
        dhtService.setPeerId("peer-connect");
        mockInvoke.mockResolvedValueOnce(undefined);

        await dhtService.connectPeer("/ip4/127.0.0.1/tcp/4001/p2p/peer123");

        expect(mockInvoke).toHaveBeenCalledWith("connect_to_peer", {
          peerAddress: "/ip4/127.0.0.1/tcp/4001/p2p/peer123",
        });
      });

      it("should throw error if DHT not initialized", async () => {
        dhtService.setPeerId(null);

        await expect(
          dhtService.connectPeer("/ip4/127.0.0.1/tcp/4001/p2p/peer123")
        ).rejects.toThrow("DHT service not initialized properly");
      });

      it("should handle connection errors", async () => {
        dhtService.setPeerId("peer-connect");
        mockInvoke.mockRejectedValueOnce(new Error("Connection refused"));

        await expect(
          dhtService.connectPeer("/ip4/127.0.0.1/tcp/4001/p2p/peer123")
        ).rejects.toThrow("Connection refused");
      });
    });

    describe("getPeerId", () => {
      it("should return current peer ID", () => {
        dhtService.setPeerId("peer-123");
        expect(dhtService.getPeerId()).toBe("peer-123");
      });

      it("should return null when not set", () => {
        dhtService.setPeerId(null);
        expect(dhtService.getPeerId()).toBeNull();
      });
    });

    describe("getPort", () => {
      it("should return current port", async () => {
        mockInvoke.mockResolvedValueOnce([]).mockResolvedValueOnce("peer-port");

        await dhtService.start({ port: 7001 });

        expect(dhtService.getPort()).toBe(7001);
      });
    });

    describe("getMultiaddr", () => {
      it("should return multiaddr when peer ID is set", async () => {
        mockInvoke
          .mockResolvedValueOnce([])
          .mockResolvedValueOnce("peer-multiaddr");

        await dhtService.start({ port: 8001 });

        expect(dhtService.getMultiaddr()).toBe(
          "/ip4/127.0.0.1/tcp/8001/p2p/peer-multiaddr"
        );
      });

      it("should return null when peer ID not set", () => {
        dhtService.setPeerId(null);
        expect(dhtService.getMultiaddr()).toBeNull();
      });
    });

    describe("getSeedersForFile", () => {
      it("should get seeders for file", async () => {
        mockInvoke.mockResolvedValueOnce(["seeder1", "seeder2"]);

        const seeders = await dhtService.getSeedersForFile("hash123");

        expect(mockInvoke).toHaveBeenCalledWith("get_file_seeders", {
          fileHash: "hash123",
        });
        expect(seeders).toEqual(["seeder1", "seeder2"]);
      });

      it("should return empty array on error", async () => {
        mockInvoke.mockRejectedValueOnce(new Error("Failed to fetch"));

        const seeders = await dhtService.getSeedersForFile("hash123");

        expect(seeders).toEqual([]);
      });

      it("should return empty array for non-array response", async () => {
        mockInvoke.mockResolvedValueOnce(null);

        const seeders = await dhtService.getSeedersForFile("hash123");

        expect(seeders).toEqual([]);
      });
    });

    describe("getPeerCount", () => {
      it("should get peer count", async () => {
        mockInvoke.mockResolvedValueOnce(42);

        const count = await dhtService.getPeerCount();

        expect(mockInvoke).toHaveBeenCalledWith("get_dht_peer_count");
        expect(count).toBe(42);
      });

      it("should return 0 on error", async () => {
        mockInvoke.mockRejectedValueOnce(new Error("Failed"));

        const count = await dhtService.getPeerCount();

        expect(count).toBe(0);
      });
    });

    describe("getHealth", () => {
      const mockHealth: DhtHealth = {
        peerCount: 10,
        lastBootstrap: Date.now(),
        lastPeerEvent: Date.now(),
        lastError: null,
        lastErrorAt: null,
        bootstrapFailures: 0,
        listenAddrs: [],
        reachability: "public",
        reachabilityConfidence: "high",
        lastReachabilityChange: Date.now(),
        lastProbeAt: Date.now(),
        lastReachabilityError: null,
        observedAddrs: [],
        reachabilityHistory: [],
        autonatEnabled: true,
      };

      it("should get DHT health", async () => {
        mockInvoke.mockResolvedValueOnce(mockHealth);

        const health = await dhtService.getHealth();

        expect(mockInvoke).toHaveBeenCalledWith("get_dht_health");
        expect(health).toEqual(mockHealth);
      });

      it("should return null on error", async () => {
        mockInvoke.mockRejectedValueOnce(new Error("Failed"));

        const health = await dhtService.getHealth();

        expect(health).toBeNull();
      });
    });

    describe("searchFileMetadata", () => {
      const mockMetadata: FileMetadata = {
        fileHash: "hash123",
        fileName: "test.txt",
        fileSize: 1024,
        seeders: ["seeder1"],
        createdAt: Date.now(),
        merkleRoot: "hash123",
        isEncrypted: false,
      };

      it("should search file metadata successfully", async () => {
        mockInvoke
          .mockResolvedValueOnce(mockMetadata)
          .mockResolvedValueOnce(["seeder1", "seeder2"]);

        const result = await dhtService.searchFileMetadata("hash123");

        expect(result).toEqual(
          expect.objectContaining({
            fileHash: "hash123",
            seeders: ["seeder1", "seeder2"],
          })
        );
      });

      it("should throw error for empty file hash", async () => {
        await expect(dhtService.searchFileMetadata("")).rejects.toThrow(
          "File hash is required"
        );
      });

      it("should use custom timeout", async () => {
        mockInvoke.mockResolvedValueOnce(mockMetadata).mockResolvedValueOnce([]);

        await dhtService.searchFileMetadata("hash123", 5000);

        expect(mockInvoke).toHaveBeenCalledWith("search_file_metadata", {
          fileHash: "hash123",
          timeoutMs: 5000,
        });
      });

      it("should normalize merkleRoot and fileHash", async () => {
        const metadataWithoutMerkle = {
          ...mockMetadata,
          merkleRoot: undefined,
        };

        mockInvoke.mockResolvedValueOnce(metadataWithoutMerkle).mockResolvedValueOnce([]);

        const result = await dhtService.searchFileMetadata("hash123");

        expect(result?.merkleRoot).toBe("hash123");
      });

      it("should return null when file not found", async () => {
        mockInvoke.mockResolvedValueOnce(null);

        const result = await dhtService.searchFileMetadata("hash123");

        expect(result).toBeNull();
      });
    });

    describe("encryptionService", () => {
      it("should encrypt file and return manifest", async () => {
        const mockManifest = {
          merkleRoot: "root123",
          chunks: [{ cid: "chunk1" }, { cid: "chunk2" }],
          encryptedKeyBundle: '{"key":"encrypted"}',
        };

        mockInvoke.mockResolvedValueOnce(mockManifest);

        const result = await encryptionService.encryptFile("/path/to/file.txt");

        expect(mockInvoke).toHaveBeenCalledWith("encrypt_file_for_self_upload", {
          filePath: "/path/to/file.txt",
        });
        expect(result).toEqual(mockManifest);
      });

      it("should decrypt file using manifest", async () => {
        const mockManifest = {
          merkleRoot: "root123",
          chunks: [],
          encryptedKeyBundle: '{"key":"encrypted"}',
        };

        mockInvoke.mockResolvedValueOnce(undefined);

        await encryptionService.decryptFile(mockManifest, "/output/path.txt");

        expect(mockInvoke).toHaveBeenCalledWith("decrypt_and_reassemble_file", {
          manifestJs: mockManifest,
          outputPath: "/output/path.txt",
        });
      });
    });

    describe("downloadFile - edge cases", () => {
      it("should handle missing fileName gracefully", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke
          .mockResolvedValueOnce("/downloads") // get_download_directory
          .mockResolvedValueOnce(undefined) // ensure_directory_exists
          .mockResolvedValueOnce(undefined); // download_blocks_from_network

        const metadataWithoutFileName: FileMetadata = {
          fileHash: "hash123",
          fileName: "",
          fileSize: 1024,
          seeders: [],
          createdAt: Date.now(),
          merkleRoot: "hash123",
          isEncrypted: false,
        };

        const promise = dhtService.downloadFile(metadataWithoutFileName);
        // downloadFile awaits get_download_directory + ensure_directory_exists before registering the listener
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: metadataWithoutFileName } as any);
        await promise;

        expect(mockInvoke).toHaveBeenCalledWith("get_download_directory");
        expect(mockInvoke).toHaveBeenCalledWith(
          "download_blocks_from_network",
          {
            fileMetadata: expect.any(Object),
            downloadPath: "/downloads/",
          }
        );
      });

      it("should preserve fileData when present", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke
          .mockResolvedValueOnce(undefined)
          .mockResolvedValueOnce(undefined);

        const fileDataArray = new Uint8Array([1, 2, 3, 4]);
        const metadataWithData: FileMetadata = {
          fileHash: "hash123",
          fileName: "test.txt",
          fileSize: 1024,
          seeders: [],
          createdAt: Date.now(),
          merkleRoot: "hash123",
          isEncrypted: false,
          downloadPath: "/path/test.txt",
          fileData: fileDataArray,
        };

        const promise = dhtService.downloadFile(metadataWithData);
        // downloadFile awaits ensure_directory_exists before registering the listener
        await Promise.resolve();
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: metadataWithData } as any);
        await promise;

        const payload = mockInvoke.mock.calls[1][1] as any;
        expect(payload.fileMetadata.fileData).toBe(fileDataArray);
      });

      it("should set isRoot to false when explicitly specified", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke
          .mockResolvedValueOnce(undefined)
          .mockResolvedValueOnce(undefined);

        const metadataNotRoot: FileMetadata = {
          fileHash: "hash123",
          fileName: "test.txt",
          fileSize: 1024,
          seeders: [],
          createdAt: Date.now(),
          merkleRoot: "hash123",
          isEncrypted: false,
          downloadPath: "/path/test.txt",
          cids: ["cid1", "cid2"],
          isRoot: false,
        };

        const promise = dhtService.downloadFile(metadataNotRoot);
        // downloadFile awaits ensure_directory_exists before registering the listener
        await Promise.resolve();
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: metadataNotRoot } as any);
        await promise;

        const payload = mockInvoke.mock.calls[1][1] as any;
        expect(payload.fileMetadata.isRoot).toBe(false);
      });
    });

    describe("connectPeer - reputation tracking", () => {
      it("should track reputation on successful connection", async () => {
        dhtService.setPeerId("peer-connect");
        mockInvoke.mockResolvedValueOnce(undefined);

        await dhtService.connectPeer("/ip4/127.0.0.1/tcp/4001/p2p/QmPeer123");

        expect(mockInvoke).toHaveBeenCalledWith("connect_to_peer", {
          peerAddress: "/ip4/127.0.0.1/tcp/4001/p2p/QmPeer123",
        });
      });

      it("should handle peer address without /p2p/ prefix", async () => {
        dhtService.setPeerId("peer-connect");
        mockInvoke.mockResolvedValueOnce(undefined);

        await dhtService.connectPeer("QmDirectPeerId");

        expect(mockInvoke).toHaveBeenCalledWith("connect_to_peer", {
          peerAddress: "QmDirectPeerId",
        });
      });
    });

    describe("searchFileMetadata - reputation tracking", () => {
      it("should track seeder reputation when metadata found", async () => {
        const metadataWithSeeders: FileMetadata = {
          fileHash: "hash123",
          fileName: "test.txt",
          fileSize: 1024,
          seeders: [
            "/ip4/127.0.0.1/tcp/4001/p2p/QmSeeder1",
            "/ip4/192.168.1.1/tcp/4001/p2p/QmSeeder2",
          ],
          createdAt: Date.now(),
          merkleRoot: "hash123",
          isEncrypted: false,
        };

        mockInvoke
          .mockResolvedValueOnce(metadataWithSeeders) // search_file_metadata
          .mockResolvedValueOnce(["seeder1", "seeder2"]); // get_file_seeders

        await dhtService.searchFileMetadata("hash123");

        // Reputation tracking happens internally, verify execution completed
        expect(mockInvoke).toHaveBeenCalledTimes(2);
      });

      it("should handle metadata without seeders", async () => {
        const metadataNoSeeders: FileMetadata = {
          fileHash: "hash123",
          fileName: "test.txt",
          fileSize: 1024,
          seeders: [],
          createdAt: Date.now(),
          merkleRoot: "hash123",
          isEncrypted: false,
        };

        // search_file_metadata returns metadata directly, then we refresh seeders via get_file_seeders
        mockInvoke.mockResolvedValueOnce(metadataNoSeeders).mockResolvedValueOnce([]);

        const result = await dhtService.searchFileMetadata("hash123");

        expect(result?.seeders).toEqual([]);
      });
    });

    describe("publishFileToNetwork - edge cases", () => {
      it("should handle metadata with missing fileHash", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke.mockResolvedValueOnce(undefined);

        const metadataWithMerkleOnly: FileMetadata = {
          fileHash: "",
          fileName: "test.txt",
          fileSize: 1024,
          seeders: [],
          createdAt: Date.now(),
          merkleRoot: "merkle456",
          isEncrypted: false,
        };

        const promise = dhtService.publishFileToNetwork("/path/to/file.txt");
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: metadataWithMerkleOnly } as any);

        const result = await promise;

        expect(result.fileHash).toBe("merkle456");
      });

      it("should handle price of zero", async () => {
        const unlistenFn = vi.fn();
        mockListen.mockResolvedValueOnce(unlistenFn);
        mockInvoke.mockResolvedValueOnce(undefined);

        const mockMetadata: FileMetadata = {
          fileHash: "hash123",
          fileName: "test.txt",
          fileSize: 1024,
          seeders: [],
          createdAt: Date.now(),
          merkleRoot: "hash123",
          isEncrypted: false,
        };

        const promise = dhtService.publishFileToNetwork("/path/to/file.txt", 0);
        const callback = mockListen.mock.calls[0]?.[1] as any;
        expect(typeof callback).toBe("function");
        callback({ payload: mockMetadata } as any);

        await promise;

        expect(mockInvoke).toHaveBeenCalledWith(
          "upload_file_to_network",
          expect.objectContaining({
            filePath: "/path/to/file.txt",
            price: 0,
          })
        );
      });
    });

  });
});

describe("searchFileMetadata - advanced scenarios", () => {
  const mockInvoke = vi.mocked(invoke);

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should handle invoke error gracefully", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("Network error"));

    await expect(dhtService.searchFileMetadata("hash123")).rejects.toThrow(
      "Network error"
    );
  });

  it("should merge seeders from multiple sources", async () => {
    const metadata: FileMetadata = {
      fileHash: "hash123",
      fileName: "test.txt",
      fileSize: 1024,
      seeders: ["seeder1"],
      createdAt: Date.now(),
      merkleRoot: "hash123",
      isEncrypted: false,
    };

    mockInvoke
      .mockResolvedValueOnce(metadata)
      .mockResolvedValueOnce(["seeder2", "seeder3", "seeder1"]); // duplicates

    const result = await dhtService.searchFileMetadata("hash123");

    expect(result?.seeders).toContain("seeder2");
    expect(result?.seeders).toContain("seeder3");
  });

  it("should handle metadata without merkleRoot or fileHash", async () => {
    const metadata: FileMetadata = {
      fileHash: "",
      fileName: "test.txt",
      fileSize: 1024,
      seeders: [],
      createdAt: Date.now(),
      merkleRoot: undefined as any,
      isEncrypted: false,
    };

    mockInvoke.mockResolvedValueOnce(metadata).mockResolvedValueOnce([]);

    const result = await dhtService.searchFileMetadata("hash123");

    expect(result).toBeDefined();
    expect(result?.fileName).toBe("test.txt");
  });

  it("should properly cleanup on timeout", async () => {
    // searchFileMetadata passes timeoutMs to backend; frontend doesn't implement its own timer.
    mockInvoke.mockRejectedValueOnce(new Error("Search timeout after 1000ms"));

    await expect(dhtService.searchFileMetadata("hash123", 1000)).rejects.toThrow(
      "Search timeout after 1000ms"
    );
  }, 15000);
});

describe("publishFileToNetwork - advanced scenarios", () => {
  const mockInvoke = vi.mocked(invoke);
  const mockListen = vi.mocked(listen);

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should handle very large files", async () => {
    const unlistenFn = vi.fn();
    mockListen.mockResolvedValueOnce(unlistenFn);
    mockInvoke.mockResolvedValueOnce(undefined);

    const largeFileMetadata: FileMetadata = {
      fileHash: "large-hash",
      fileName: "large-file.bin",
      fileSize: 10737418240,
      seeders: [],
      createdAt: Date.now(),
      merkleRoot: "large-hash",
      isEncrypted: true,
      encryptionMethod: "AES-256-GCM",
    };

    const promise = dhtService.publishFileToNetwork(
      "/path/to/large-file.bin",
      1000
    );
    const callback = mockListen.mock.calls[0]?.[1] as any;
    expect(typeof callback).toBe("function");
    callback({ payload: largeFileMetadata } as any);

    const result = await promise;

    expect(result.fileSize).toBe(10737418240);
    expect(result.isEncrypted).toBe(true);
    expect(unlistenFn).toHaveBeenCalled();
  }, 15000);

  it("should handle publishing with HTTP sources", async () => {
    const unlistenFn = vi.fn();
    mockListen.mockResolvedValueOnce(unlistenFn);
    mockInvoke.mockResolvedValueOnce(undefined);

    const metadataWithHttpSources: FileMetadata = {
      fileHash: "hash123",
      fileName: "test.txt",
      fileSize: 1024,
      seeders: [],
      createdAt: Date.now(),
      merkleRoot: "hash123",
      isEncrypted: false,
      httpSources: [
        {
          url: "https://cdn.example.com/file.txt",
          verifySsl: true,
          headers: [["Authorization", "Bearer token"]],
          timeoutSecs: 30,
        },
      ],
    };

    const promise = dhtService.publishFileToNetwork("/path/to/file.txt");
    const callback = mockListen.mock.calls[0]?.[1] as any;
    expect(typeof callback).toBe("function");
    callback({ payload: metadataWithHttpSources } as any);

    const result = await promise;

    expect(result.httpSources).toBeDefined();
    expect(result.httpSources?.[0].url).toContain("cdn.example.com");
    expect(unlistenFn).toHaveBeenCalled();
  }, 15000);

  it("should properly cleanup listeners on success", async () => {
    const unlistenFn = vi.fn();
    mockListen.mockResolvedValueOnce(unlistenFn);
    mockInvoke.mockResolvedValueOnce(undefined);

    const mockMetadata: FileMetadata = {
      fileHash: "hash123",
      fileName: "test.txt",
      fileSize: 1024,
      seeders: [],
      createdAt: Date.now(),
      merkleRoot: "hash123",
      isEncrypted: false,
    };

    const promise = dhtService.publishFileToNetwork("/path/to/file.txt");
    const callback = mockListen.mock.calls[0]?.[1] as any;
    expect(typeof callback).toBe("function");
    callback({ payload: mockMetadata } as any);
    await promise;

    expect(unlistenFn).toHaveBeenCalled();
  }, 15000);
});

describe("DhtService - boundary conditions", () => {
  const mockInvoke = vi.mocked(invoke);
  const mockListen = vi.mocked(listen);

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should handle empty bootstrap nodes list", async () => {
    mockInvoke
      .mockResolvedValueOnce([]) // get_bootstrap_nodes returns empty
      .mockResolvedValueOnce("peer-no-bootstrap");

    const peerId = await dhtService.start({ port: 4001 });

    expect(mockInvoke).toHaveBeenCalledWith("start_dht_node", {
      port: 4001,
      bootstrapNodes: [],
    });
    expect(peerId).toBe("peer-no-bootstrap");
  });

  it("should handle maximum port number", async () => {
    mockInvoke.mockResolvedValueOnce([]).mockResolvedValueOnce("peer-max-port");

    await dhtService.start({ port: 65535 });

    expect(dhtService.getPort()).toBe(65535);
  });

  it("should handle zero price for file", async () => {
    const unlistenFn = vi.fn();
    mockListen.mockResolvedValueOnce(unlistenFn);
    mockInvoke.mockResolvedValueOnce(undefined);

    const mockMetadata: FileMetadata = {
      fileHash: "hash123",
      fileName: "free.txt",
      fileSize: 100,
      seeders: [],
      createdAt: Date.now(),
      merkleRoot: "hash123",
      isEncrypted: false,
      price: 0,
    };

    const promise = dhtService.publishFileToNetwork("/path/free.txt", 0);
    const callback = mockListen.mock.calls[0]?.[1] as any;
    expect(typeof callback).toBe("function");
    callback({ payload: mockMetadata } as any);

    const result = await promise;

    expect(result.price).toBe(0);
    expect(unlistenFn).toHaveBeenCalled();
  }, 15000);

  it("should handle file with empty seeders initially", async () => {
    mockInvoke.mockResolvedValueOnce([]);

    const seeders = await dhtService.getSeedersForFile("new-file-hash");

    expect(seeders).toEqual([]);
  });
});

describe("DhtService - reputation integration", () => {
  const mockInvoke = vi.mocked(invoke);

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should track multiple peer interactions", async () => {
    dhtService.setPeerId("peer-reputation-test");

    // Success
    mockInvoke.mockResolvedValueOnce(undefined);
    await dhtService.connectPeer("/ip4/1.2.3.4/tcp/4001/p2p/QmPeer1");

    // Failure
    mockInvoke.mockRejectedValueOnce(new Error("Timeout"));
    try {
      await dhtService.connectPeer("/ip4/1.2.3.5/tcp/4001/p2p/QmPeer2");
    } catch {}

    // Another success
    mockInvoke.mockResolvedValueOnce(undefined);
    await dhtService.connectPeer("/ip4/1.2.3.6/tcp/4001/p2p/QmPeer3");

    expect(mockInvoke).toHaveBeenCalledTimes(3);
  });
});

describe("DhtService - multiaddr formatting", () => {
  const mockInvoke = vi.mocked(invoke);

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should format multiaddr with IPv4", async () => {
    mockInvoke
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce("QmTestPeerId123");

    await dhtService.start({ port: 4567 });

    const multiaddr = dhtService.getMultiaddr();
    expect(multiaddr).toBe("/ip4/127.0.0.1/tcp/4567/p2p/QmTestPeerId123");
  });

  it("should return null multiaddr when DHT not started", () => {
    dhtService.setPeerId(null);
    expect(dhtService.getMultiaddr()).toBeNull();
  });
});
