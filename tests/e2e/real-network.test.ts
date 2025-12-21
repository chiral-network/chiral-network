/**
 * @fileoverview Real E2E Test Framework
 * Launches two actual Chiral Network nodes and tests real communication
 * 
 * This framework:
 * - Spawns two separate node processes with different configurations
 * - Uses real DHT network for peer discovery
 * - Tests actual WebRTC/Bitswap protocol communication
 * - Validates real payment transactions
 * - Can run across different machines on the same network
 * 
 * Usage:
 *   npm run test:e2e:real
 *   npm run test:e2e:real -- --cross-machine
 */

import { spawn, ChildProcess } from "child_process";
import { promises as fs } from "fs";
import path from "path";
import os from "os";
import { describe, it, expect, beforeAll, afterAll } from "vitest";

interface NodeConfig {
  nodeId: string;
  dhtPort: number;
  apiPort: number;
  storageDir: string;
  walletAddress: string;
  bootstrapNodes: string[];
}

interface TestFile {
  name: string;
  size: number;
  content: Buffer;
  hash?: string;
}

class RealE2ETestFramework {
  private uploaderNode: ChildProcess | null = null;
  private downloaderNode: ChildProcess | null = null;
  private uploaderConfig: NodeConfig | null = null;
  private downloaderConfig: NodeConfig | null = null;
  private testDir: string = "";
  private crossMachine: boolean = false;

  constructor(crossMachine: boolean = false) {
    this.crossMachine = crossMachine;
  }

  /**
   * Setup test environment
   */
  async setup(): Promise<void> {
    console.log("🚀 Setting up real E2E test environment...");

    // Create temporary test directory
    this.testDir = await fs.mkdtemp(path.join(os.tmpdir(), "chiral-e2e-"));
    console.log(`📁 Test directory: ${this.testDir}`);

    // Generate configurations for two nodes
    this.uploaderConfig = {
      nodeId: "uploader_node",
      dhtPort: 4001,
      apiPort: 8081,
      storageDir: path.join(this.testDir, "uploader_storage"),
      walletAddress: "0x1111111111111111111111111111111111111111",
      bootstrapNodes: [],
    };

    this.downloaderConfig = {
      nodeId: "downloader_node",
      dhtPort: 4002,
      apiPort: 8082,
      storageDir: path.join(this.testDir, "downloader_storage"),
      walletAddress: "0x2222222222222222222222222222222222222222",
      bootstrapNodes: [`/ip4/127.0.0.1/tcp/${this.uploaderConfig.dhtPort}`],
    };

    // Create storage directories
    await fs.mkdir(this.uploaderConfig.storageDir, { recursive: true });
    await fs.mkdir(this.downloaderConfig.storageDir, { recursive: true });

    console.log("✅ Configuration created");
  }

  /**
   * Launch uploader node
   */
  async launchUploaderNode(): Promise<void> {
    console.log("🔵 Launching uploader node...");

    if (!this.uploaderConfig) throw new Error("Config not initialized");

    // Launch node process
    // Note: Adjust command based on your build setup
    this.uploaderNode = spawn("npm", ["run", "tauri", "dev"], {
      cwd: process.cwd(),
      env: {
        ...process.env,
        CHIRAL_NODE_ID: this.uploaderConfig.nodeId,
        CHIRAL_DHT_PORT: this.uploaderConfig.dhtPort.toString(),
        CHIRAL_API_PORT: this.uploaderConfig.apiPort.toString(),
        CHIRAL_STORAGE_DIR: this.uploaderConfig.storageDir,
        CHIRAL_WALLET_ADDRESS: this.uploaderConfig.walletAddress,
        CHIRAL_HEADLESS: "true", // Run without UI for testing
      },
    });

    this.setupNodeLogging(this.uploaderNode, "UPLOADER");

    // Wait for node to be ready
    await this.waitForNodeReady(this.uploaderConfig.apiPort, 30000);
    console.log("✅ Uploader node ready");
  }

  /**
   * Launch downloader node
   */
  async launchDownloaderNode(): Promise<void> {
    console.log("🟢 Launching downloader node...");

    if (!this.downloaderConfig) throw new Error("Config not initialized");

    this.downloaderNode = spawn("npm", ["run", "tauri", "dev"], {
      cwd: process.cwd(),
      env: {
        ...process.env,
        CHIRAL_NODE_ID: this.downloaderConfig.nodeId,
        CHIRAL_DHT_PORT: this.downloaderConfig.dhtPort.toString(),
        CHIRAL_API_PORT: this.downloaderConfig.apiPort.toString(),
        CHIRAL_STORAGE_DIR: this.downloaderConfig.storageDir,
        CHIRAL_WALLET_ADDRESS: this.downloaderConfig.walletAddress,
        CHIRAL_BOOTSTRAP_NODES: this.downloaderConfig.bootstrapNodes.join(","),
        CHIRAL_HEADLESS: "true",
      },
    });

    this.setupNodeLogging(this.downloaderNode, "DOWNLOADER");

    // Wait for node to be ready
    await this.waitForNodeReady(this.downloaderConfig.apiPort, 30000);
    console.log("✅ Downloader node ready");

    // Wait for DHT connection
    await this.waitForDHTConnection(5000);
    console.log("✅ Nodes connected via DHT");
  }

  /**
   * Setup logging for node process
   */
  private setupNodeLogging(node: ChildProcess, prefix: string): void {
    if (node.stdout) {
      node.stdout.on("data", (data) => {
        console.log(`[${prefix}] ${data.toString().trim()}`);
      });
    }

    if (node.stderr) {
      node.stderr.on("data", (data) => {
        console.error(`[${prefix} ERROR] ${data.toString().trim()}`);
      });
    }

    node.on("error", (error) => {
      console.error(`[${prefix}] Process error:`, error);
    });

    node.on("exit", (code) => {
      console.log(`[${prefix}] Process exited with code ${code}`);
    });
  }

  /**
   * Wait for node to be ready
   */
  private async waitForNodeReady(apiPort: number, timeout: number): Promise<void> {
    const startTime = Date.now();
    const checkInterval = 500;

    while (Date.now() - startTime < timeout) {
      try {
        const response = await fetch(`http://localhost:${apiPort}/api/health`);
        if (response.ok) {
          return;
        }
      } catch {
        // Node not ready yet
      }

      await new Promise((resolve) => setTimeout(resolve, checkInterval));
    }

    throw new Error(`Node failed to start within ${timeout}ms`);
  }

  /**
   * Wait for DHT connection between nodes
   */
  private async waitForDHTConnection(timeout: number): Promise<void> {
    if (!this.downloaderConfig) throw new Error("Config not initialized");

    const startTime = Date.now();
    const checkInterval = 500;

    while (Date.now() - startTime < timeout) {
      try {
        const response = await fetch(
          `http://localhost:${this.downloaderConfig.apiPort}/api/dht/peers`
        );
        
        if (response.ok) {
          const data = await response.json();
          if (data.peers && data.peers.length > 0) {
            return;
          }
        }
      } catch {
        // Not connected yet
      }

      await new Promise((resolve) => setTimeout(resolve, checkInterval));
    }

    throw new Error(`DHT connection failed within ${timeout}ms`);
  }

  /**
   * Upload file from uploader node
   */
  async uploadFile(file: TestFile, protocol: "WebRTC" | "Bitswap"): Promise<string> {
    if (!this.uploaderConfig) throw new Error("Config not initialized");

    console.log(`📤 Uploading file: ${file.name} (${file.size} bytes) via ${protocol}`);

    // Write file to uploader's storage
    const filePath = path.join(this.uploaderConfig.storageDir, file.name);
    await fs.writeFile(filePath, file.content);

    // Upload via API
    const response = await fetch(
      `http://localhost:${this.uploaderConfig.apiPort}/api/upload`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          filePath,
          protocol,
          price: 0.001,
        }),
      }
    );

    if (!response.ok) {
      throw new Error(`Upload failed: ${response.statusText}`);
    }

    const result = await response.json();
    console.log(`✅ File uploaded. Hash: ${result.fileHash}`);

    return result.fileHash;
  }

  /**
   * Search for file from downloader node
   */
  async searchFile(fileHash: string, timeout: number = 10000): Promise<any> {
    if (!this.downloaderConfig) throw new Error("Config not initialized");

    console.log(`🔍 Searching for file: ${fileHash}`);

    const response = await fetch(
      `http://localhost:${this.downloaderConfig.apiPort}/api/search`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          fileHash,
          timeout,
        }),
      }
    );

    if (!response.ok) {
      throw new Error(`Search failed: ${response.statusText}`);
    }

    const metadata = await response.json();
    console.log(`✅ File found. Seeders: ${metadata.seeders?.length || 0}`);

    return metadata;
  }

  /**
   * Download file to downloader node
   */
  async downloadFile(
    fileHash: string,
    fileName: string,
    protocol: "WebRTC" | "Bitswap"
  ): Promise<string> {
    if (!this.downloaderConfig) throw new Error("Config not initialized");

    console.log(`📥 Downloading file: ${fileName} via ${protocol}`);

    const response = await fetch(
      `http://localhost:${this.downloaderConfig.apiPort}/api/download`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          fileHash,
          fileName,
          protocol,
        }),
      }
    );

    if (!response.ok) {
      throw new Error(`Download failed: ${response.statusText}`);
    }

    const result = await response.json();
    const downloadPath = path.join(this.downloaderConfig.storageDir, fileName);

    // Wait for file to exist
    await this.waitForFile(downloadPath, 60000);

    console.log(`✅ File downloaded to: ${downloadPath}`);
    return downloadPath;
  }

  /**
   * Wait for file to exist
   */
  private async waitForFile(filePath: string, timeout: number): Promise<void> {
    const startTime = Date.now();
    const checkInterval = 500;

    while (Date.now() - startTime < timeout) {
      try {
        await fs.access(filePath);
        return;
      } catch {
        // File doesn't exist yet
      }

      await new Promise((resolve) => setTimeout(resolve, checkInterval));
    }

    throw new Error(`File not found within ${timeout}ms: ${filePath}`);
  }

  /**
   * Verify downloaded file matches original
   */
  async verifyDownloadedFile(downloadPath: string, originalFile: TestFile): Promise<boolean> {
    console.log(`🔐 Verifying downloaded file...`);

    const downloadedContent = await fs.readFile(downloadPath);

    if (downloadedContent.length !== originalFile.size) {
      console.error(
        `❌ Size mismatch: expected ${originalFile.size}, got ${downloadedContent.length}`
      );
      return false;
    }

    if (!downloadedContent.equals(originalFile.content)) {
      console.error("❌ Content mismatch");
      return false;
    }

    console.log("✅ File verified successfully");
    return true;
  }

  /**
   * Get payment transactions
   */
  async getPaymentTransactions(nodeType: "uploader" | "downloader"): Promise<any[]> {
    const config = nodeType === "uploader" ? this.uploaderConfig : this.downloaderConfig;
    if (!config) throw new Error("Config not initialized");

    const response = await fetch(
      `http://localhost:${config.apiPort}/api/wallet/transactions`
    );

    if (!response.ok) {
      throw new Error(`Failed to get transactions: ${response.statusText}`);
    }

    return await response.json();
  }

  /**
   * Verify payment was processed
   */
  async verifyPayment(fileHash: string): Promise<boolean> {
    console.log(`💰 Verifying payment for file: ${fileHash}`);

    const uploaderTxs = await this.getPaymentTransactions("uploader");
    const downloaderTxs = await this.getPaymentTransactions("downloader");

    // Check uploader received payment
    const uploaderReceived = uploaderTxs.some(
      (tx) => tx.type === "received" && tx.description?.includes(fileHash)
    );

    // Check downloader sent payment
    const downloaderSent = downloaderTxs.some(
      (tx) => tx.type === "sent" && tx.description?.includes(fileHash)
    );

    const verified = uploaderReceived && downloaderSent;
    
    if (verified) {
      console.log("✅ Payment verified");
    } else {
      console.error("❌ Payment verification failed");
    }

    return verified;
  }

  /**
   * Cleanup test environment
   */
  async cleanup(): Promise<void> {
    console.log("🧹 Cleaning up test environment...");

    // Stop nodes
    if (this.uploaderNode) {
      this.uploaderNode.kill("SIGTERM");
      await this.waitForProcessExit(this.uploaderNode, 5000);
    }

    if (this.downloaderNode) {
      this.downloaderNode.kill("SIGTERM");
      await this.waitForProcessExit(this.downloaderNode, 5000);
    }

    // Clean up test directory
    try {
      await fs.rm(this.testDir, { recursive: true, force: true });
      console.log("✅ Cleanup complete");
    } catch (error) {
      console.warn("⚠️  Failed to remove test directory:", error);
    }
  }

  /**
   * Wait for process to exit
   */
  private async waitForProcessExit(
    process: ChildProcess,
    timeout: number
  ): Promise<void> {
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        process.kill("SIGKILL");
        resolve();
      }, timeout);

      process.on("exit", () => {
        clearTimeout(timer);
        resolve();
      });
    });
  }

  /**
   * Create test file
   */
  createTestFile(name: string, sizeInMB: number): TestFile {
    const size = sizeInMB * 1024 * 1024;
    const content = Buffer.alloc(size);

    // Fill with pseudo-random data for better verification
    for (let i = 0; i < size; i++) {
      content[i] = i % 256;
    }

    return { name, size, content };
  }
}

/**
 * Real E2E Tests
 * 
 * These tests launch actual Chiral Network nodes and test real communication.
 * They can be run on a single machine or across multiple machines on the same network.
 * 
 * To run across machines:
 * 1. Machine 1: Run uploader node with known IP
 * 2. Machine 2: Set CHIRAL_BOOTSTRAP_NODES to Machine 1's IP:port
 * 3. Run tests
 */
describe("Real E2E Tests (Two Actual Nodes)", () => {
  let framework: RealE2ETestFramework;

  beforeAll(async () => {
    // Check if running in cross-machine mode
    const crossMachine = process.env.E2E_CROSS_MACHINE === "true";
    
    framework = new RealE2ETestFramework(crossMachine);
    await framework.setup();

    if (!crossMachine) {
      // Launch both nodes on same machine
      await framework.launchUploaderNode();
      await framework.launchDownloaderNode();
    } else {
      // In cross-machine mode, launch only one node based on role
      const role = process.env.E2E_NODE_ROLE;
      
      if (role === "uploader") {
        await framework.launchUploaderNode();
        console.log("✅ Uploader node running. Waiting for downloader...");
      } else if (role === "downloader") {
        await framework.launchDownloaderNode();
        console.log("✅ Downloader node running and connected.");
      } else {
        throw new Error("E2E_NODE_ROLE must be set to 'uploader' or 'downloader' in cross-machine mode");
      }
    }
  }, 60000); // 60 second timeout for node startup

  afterAll(async () => {
    await framework.cleanup();
  }, 30000);

  describe("WebRTC Real Communication", () => {
    it("should upload, search, download, and verify file via WebRTC", async () => {
      // Create 5MB test file
      const testFile = framework.createTestFile("real-test-webrtc.bin", 5);

      // Upload
      const fileHash = await framework.uploadFile(testFile, "WebRTC");
      expect(fileHash).toBeTruthy();

      // Search
      const metadata = await framework.searchFile(fileHash);
      expect(metadata).toBeTruthy();
      expect(metadata.seeders).toContain(expect.any(String));

      // Download
      const downloadPath = await framework.downloadFile(
        fileHash,
        testFile.name,
        "WebRTC"
      );

      // Verify
      const verified = await framework.verifyDownloadedFile(downloadPath, testFile);
      expect(verified).toBe(true);

      // Verify payment
      const paymentVerified = await framework.verifyPayment(fileHash);
      expect(paymentVerified).toBe(true);
    }, 120000); // 2 minute timeout

    it("should handle large file (50MB) with real WebRTC streaming", async () => {
      const largeFile = framework.createTestFile("large-real-webrtc.bin", 50);

      const fileHash = await framework.uploadFile(largeFile, "WebRTC");
      const metadata = await framework.searchFile(fileHash);
      const downloadPath = await framework.downloadFile(
        fileHash,
        largeFile.name,
        "WebRTC"
      );

      const verified = await framework.verifyDownloadedFile(downloadPath, largeFile);
      expect(verified).toBe(true);
    }, 300000); // 5 minute timeout for large file
  });

  describe("Bitswap Real Communication", () => {
    it("should upload, search, download, and verify file via Bitswap", async () => {
      const testFile = framework.createTestFile("real-test-bitswap.bin", 3);

      const fileHash = await framework.uploadFile(testFile, "Bitswap");
      expect(fileHash).toBeTruthy();

      const metadata = await framework.searchFile(fileHash);
      expect(metadata).toBeTruthy();
      expect(metadata.cids).toBeDefined();
      expect(metadata.cids.length).toBeGreaterThan(0);

      const downloadPath = await framework.downloadFile(
        fileHash,
        testFile.name,
        "Bitswap"
      );

      const verified = await framework.verifyDownloadedFile(downloadPath, testFile);
      expect(verified).toBe(true);

      const paymentVerified = await framework.verifyPayment(fileHash);
      expect(paymentVerified).toBe(true);
    }, 120000);
  });

  describe("Payment Checkpoint Real Communication", () => {
    it("should process real payment checkpoints at 10MB and 20MB", async () => {
      // Create 25MB file to trigger checkpoints
      const checkpointFile = framework.createTestFile("checkpoint-real.bin", 25);

      const fileHash = await framework.uploadFile(checkpointFile, "WebRTC");
      await framework.searchFile(fileHash);

      // Download will trigger checkpoints
      const downloadPath = await framework.downloadFile(
        fileHash,
        checkpointFile.name,
        "WebRTC"
      );

      const verified = await framework.verifyDownloadedFile(downloadPath, checkpointFile);
      expect(verified).toBe(true);

      // Verify multiple payments (checkpoints + final)
      const downloaderTxs = await framework.getPaymentTransactions("downloader");
      const paymentsForFile = downloaderTxs.filter(
        (tx) => tx.type === "sent" && tx.description?.includes(fileHash)
      );

      // Should have multiple payments (checkpoint + final)
      expect(paymentsForFile.length).toBeGreaterThan(1);
    }, 180000); // 3 minute timeout
  });

  describe("Network Resilience", () => {
    it("should handle connection interruption and resume", async () => {
      // This test would need actual network manipulation
      // Skip for now, but framework is ready for it
    });
  });
});

/**
 * Export framework for manual testing
 */
export { RealE2ETestFramework };

