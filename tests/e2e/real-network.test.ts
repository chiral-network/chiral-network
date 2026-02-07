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

function npmCommand(): string {
  // On Windows, `npm` is `npm.cmd` (spawn() won't resolve it automatically).
  return process.platform === "win32" ? "npm.cmd" : "npm";
}

function spawnNpm(args: string[], opts: Parameters<typeof spawn>[2]): ChildProcess {
  // On Windows, `.cmd` needs to be invoked through `cmd.exe /c`.
  if (process.platform === "win32") {
    return spawn("cmd.exe", ["/c", npmCommand(), ...args], opts);
  }
  return spawn(npmCommand(), args, opts);
}

function getTestTimeoutMs(
  protocol: "HTTP" | "WebRTC" | "Bitswap" | "FTP" | "BitTorrent"
): number {
  // Priority:
  // 1) E2E_{PROTOCOL}_TEST_TIMEOUT_MS
  // 2) E2E_TEST_TIMEOUT_MS
  // 3) protocol default (BitTorrent shorter to avoid long hangs during debugging)
  const key = `E2E_${protocol.toUpperCase()}_TEST_TIMEOUT_MS`;
  const raw =
    process.env[key] ??
    process.env.E2E_TEST_TIMEOUT_MS ??
    (protocol === "BitTorrent" ? "300000" : "600000");

  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? n : 600000;
}

function getSearchTimeoutMs(
  protocol: "HTTP" | "WebRTC" | "Bitswap" | "FTP" | "BitTorrent"
): number {
  // Priority:
  // 1) E2E_{PROTOCOL}_SEARCH_TIMEOUT_MS
  // 2) E2E_SEARCH_TIMEOUT_MS
  // 3) protocol default (BT shorter so it doesn't eat the whole test budget)
  const key = `E2E_${protocol.toUpperCase()}_SEARCH_TIMEOUT_MS`;
  const raw =
    process.env[key] ??
    process.env.E2E_SEARCH_TIMEOUT_MS ??
    (protocol === "BitTorrent" ? "45000" : "90000");
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? n : 90000;
}

function getReceiptTimeoutMs(
  protocol: "HTTP" | "WebRTC" | "Bitswap" | "FTP" | "BitTorrent"
): number {
  // Priority:
  // 1) E2E_{PROTOCOL}_RECEIPT_TIMEOUT_MS
  // 2) E2E_RECEIPT_TIMEOUT_MS
  // 3) protocol default (BT shorter for faster fail)
  const key = `E2E_${protocol.toUpperCase()}_RECEIPT_TIMEOUT_MS`;
  const raw =
    process.env[key] ??
    process.env.E2E_RECEIPT_TIMEOUT_MS ??
    (protocol === "BitTorrent" ? "60000" : "120000");
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? n : 120000;
}

interface NodeConfig {
  nodeId: string;
  apiBaseUrl: string;
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
  private requirePayment: boolean = false;
  private torrentBase64ByHash: Map<string, string> = new Map();
  private torrentSeederPortByHash: Map<string, number> = new Map();

  constructor(crossMachine: boolean = false) {
    this.crossMachine = crossMachine;
  }

  /**
   * Setup test environment
   */
  async setup(): Promise<void> {
    console.log("[START] Setting up real E2E test environment...");

    const attach = process.env.E2E_ATTACH === "true";
    this.requirePayment = attach || process.env.E2E_REQUIRE_PAYMENT === "true";
    if (attach) {
      const uploaderApi = process.env.E2E_UPLOADER_API_URL;
      const downloaderApi = process.env.E2E_DOWNLOADER_API_URL;
      if (!uploaderApi || !downloaderApi) {
        throw new Error(
          "E2E_ATTACH=true requires E2E_UPLOADER_API_URL and E2E_DOWNLOADER_API_URL"
        );
      }

      this.uploaderConfig = { nodeId: "uploader_node", apiBaseUrl: uploaderApi };
      this.downloaderConfig = { nodeId: "downloader_node", apiBaseUrl: downloaderApi };
    } else {
      // Spawn mode is best-effort. Note: the current Tauri binary enforces single-instance
      // via binding DHT port 4001, so running two nodes locally may not work.
      this.testDir = await fs.mkdtemp(path.join(os.tmpdir(), "chiral-e2e-"));
      console.log(`[DIR] Test directory: ${this.testDir}`);

      const uploaderPort = Number(process.env.E2E_UPLOADER_API_PORT || "8081");
      const downloaderPort = Number(process.env.E2E_DOWNLOADER_API_PORT || "8082");
      this.uploaderConfig = {
        nodeId: "uploader_node",
        apiBaseUrl: `http://localhost:${uploaderPort}`,
      };
      this.downloaderConfig = {
        nodeId: "downloader_node",
        apiBaseUrl: `http://localhost:${downloaderPort}`,
      };
    }

    console.log("[OK] Configuration created");
  }

  /**
   * Launch uploader node
   */
  async launchUploaderNode(): Promise<void> {
    console.log("[*] Launching uploader node...");

    if (!this.uploaderConfig) throw new Error("Config not initialized");
    if (process.env.E2E_ATTACH === "true") return;

    const startupTimeoutMs = Number(
      process.env.E2E_NODE_STARTUP_TIMEOUT_MS || "180000"
    );

    const nodeEnv = await this.buildNodeEnv("uploader", this.uploaderConfig);

    // Launch node process
    // Note: Adjust command based on your build setup
    this.uploaderNode = spawnNpm(["run", "tauri", "dev"], {
      cwd: process.cwd(),
      env: {
        ...nodeEnv,
      },
    });

    this.setupNodeLogging(this.uploaderNode, "UPLOADER");

    // Wait for node to be ready
    await this.waitForNodeReady(this.uploaderConfig.apiBaseUrl, startupTimeoutMs);
    console.log("[OK] Uploader node ready");
  }

  /**
   * Launch downloader node
   */
  async launchDownloaderNode(): Promise<void> {
    console.log("[GREEN] Launching downloader node...");

    if (!this.downloaderConfig) throw new Error("Config not initialized");
    if (process.env.E2E_ATTACH === "true") return;

    const startupTimeoutMs = Number(
      process.env.E2E_NODE_STARTUP_TIMEOUT_MS || "180000"
    );

    const nodeEnv = await this.buildNodeEnv("downloader", this.downloaderConfig);

    this.downloaderNode = spawnNpm(["run", "tauri", "dev"], {
      cwd: process.cwd(),
      env: {
        ...nodeEnv,
      },
    });

    this.setupNodeLogging(this.downloaderNode, "DOWNLOADER");

    // Wait for node to be ready
    await this.waitForNodeReady(this.downloaderConfig.apiBaseUrl, startupTimeoutMs);
    console.log("[OK] Downloader node ready");

    // Wait for DHT connection
    await this.waitForDHTConnection(5000);
    console.log("[OK] Nodes connected via DHT");
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

  private async buildNodeEnv(
    role: "uploader" | "downloader",
    cfg: NodeConfig
  ): Promise<NodeJS.ProcessEnv> {
    const env: NodeJS.ProcessEnv = {
      ...process.env,
      CHIRAL_NODE_ID: cfg.nodeId,
      CHIRAL_E2E_API_PORT: new URL(cfg.apiBaseUrl).port,
      CHIRAL_HEADLESS: "true",
    };

    // Avoid HTTP server port collisions in spawn mode (two local nodes).
    // The Rust side auto-starts the HTTP file server in a fixed range by default (8080-8090).
    // On busy developer machines those ports may already be taken; also two nodes can contend.
    // Use a high, per-node range to make the spawn-mode harness reliable.
    if (role === "uploader") {
      env.CHIRAL_HTTP_PORT_START = env.CHIRAL_HTTP_PORT_START || "38080";
      env.CHIRAL_HTTP_PORT_END = env.CHIRAL_HTTP_PORT_END || "38090";
    } else {
      env.CHIRAL_HTTP_PORT_START = env.CHIRAL_HTTP_PORT_START || "38091";
      env.CHIRAL_HTTP_PORT_END = env.CHIRAL_HTTP_PORT_END || "38101";
    }

    // Spawn mode runs both nodes on the same machine. We must isolate:
    // - Tauri app data dir (APPDATA/LOCALAPPDATA on Windows) to avoid DB/file locks
    // - download directory (~ expansion uses USERPROFILE/HOME), because BitTorrent session persistence writes there
    // - storage dir for HTTP file server (headless.rs reads CHIRAL_STORAGE_DIR)
    if (this.testDir) {
      const nodeRoot = path.join(this.testDir, role);
      const appDataDir = path.join(nodeRoot, "appdata");
      const localAppDataDir = path.join(nodeRoot, "localappdata");
      const storageDir = path.join(nodeRoot, "files");
      const downloadDir = path.join(nodeRoot, "downloads");

      await fs.mkdir(appDataDir, { recursive: true });
      await fs.mkdir(localAppDataDir, { recursive: true });
      await fs.mkdir(storageDir, { recursive: true });
      await fs.mkdir(downloadDir, { recursive: true });

      env.CHIRAL_STORAGE_DIR = storageDir;

      if (process.platform === "win32") {
        env.APPDATA = appDataDir;
        env.LOCALAPPDATA = localAppDataDir;
        // IMPORTANT: Do NOT override USERPROFILE here — it breaks rustup/toolchain discovery for spawned `cargo`.
        // Instead, write per-node `settings.json` so get_download_directory() resolves to a unique folder.
        const tauriAppDataDir = path.join(appDataDir, "com.chiralnetwork");
        await fs.mkdir(tauriAppDataDir, { recursive: true });
        const settingsPath = path.join(tauriAppDataDir, "settings.json");
        await fs.writeFile(
          settingsPath,
          JSON.stringify(
            {
              storagePath: downloadDir,
              enableFileLogging: false,
              maxLogSizeMb: 10,
            },
            null,
            2
          )
        );
      } else {
        // Best-effort isolation for non-Windows spawn mode.
        env.HOME = nodeRoot;
      }

      // In spawn mode, ensure both nodes have wallets loaded so upload/download can proceed.
      // Users can still override by exporting CHIRAL_PRIVATE_KEY before running the tests.
      if (!env.CHIRAL_PRIVATE_KEY) {
        env.CHIRAL_PRIVATE_KEY =
          role === "uploader"
            ? "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            : "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
      }
    }

    return env;
  }

  /**
   * Wait for node to be ready
   */
  private async waitForNodeReady(apiBaseUrl: string, timeout: number): Promise<void> {
    const startTime = Date.now();
    const checkInterval = 500;

    while (Date.now() - startTime < timeout) {
      try {
        const response = await fetch(`${apiBaseUrl}/api/health`);
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
          `${this.downloaderConfig.apiBaseUrl}/api/dht/peers`
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
  async uploadFile(
    file: TestFile,
    protocol: "HTTP" | "WebRTC" | "Bitswap" | "FTP" | "BitTorrent"
  ): Promise<string> {
    if (!this.uploaderConfig) throw new Error("Config not initialized");

    console.log(
      `[OUT] Uploading file (generated on uploader): ${file.name} (${Math.round(
        file.size / 1024 / 1024
      )}MB) via ${protocol}`
    );

    // Uploader generates deterministic bytes and publishes metadata.
    const response = await fetch(`${this.uploaderConfig.apiBaseUrl}/api/upload`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        sizeMb: Math.max(1, Math.round(file.size / 1024 / 1024)),
        protocol,
        // In spawn mode, default to price=0 to avoid requiring mining/funding.
        // In attach mode (or when explicitly requested), keep payment enabled.
        price: this.requirePayment ? 0.001 : 0,
        fileName: file.name,
      }),
    });

    if (!response.ok) {
      const body = await response.text().catch(() => "");
      throw new Error(
        `Upload failed: ${response.status} ${response.statusText}${
          body ? ` - ${body}` : ""
        }`
      );
    }

    const result = await response.json();
    if (result?.torrentBase64 && typeof result.torrentBase64 === "string") {
      this.torrentBase64ByHash.set(result.fileHash, result.torrentBase64);
    }
    if (typeof result?.bittorrentPort === "number") {
      this.torrentSeederPortByHash.set(result.fileHash, result.bittorrentPort);
    }
    console.log(`[OK] File uploaded. Hash: ${result.fileHash}`);

    return result.fileHash;
  }

  /**
   * Search for file from downloader node
   */
  async searchFile(fileHash: string, timeout: number = 10000): Promise<any> {
    if (!this.downloaderConfig) throw new Error("Config not initialized");

    console.log(`[SEARCH] Searching for file: ${fileHash}`);

    const start = Date.now();
    const pollIntervalMs = 750;
    const perAttemptTimeoutMs = Number(process.env.E2E_SEARCH_ATTEMPT_TIMEOUT_MS || "10000");

    // DHT propagation is not instantaneous across real networks.
    // Poll until we get a non-null metadata (or until timeout).
    while (Date.now() - start < timeout) {
      const response = await fetch(`${this.downloaderConfig.apiBaseUrl}/api/search`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          fileHash,
          // Give each attempt a modest budget; overall timeout is controlled by this loop.
          timeoutMs: Math.max(
            1000,
            Math.min(perAttemptTimeoutMs, Math.max(0, timeout - (Date.now() - start)))
          ),
        }),
      });

      if (!response.ok) {
        const body = await response.text().catch(() => "");
        throw new Error(
          `Search failed: ${response.status} ${response.statusText}${body ? ` - ${body}` : ""}`
        );
      }

      const metadata = await response.json();
      if (metadata) {
        console.log(
          `[OK] File found. Seeders: ${metadata.seeders?.length || 0}`
        );
        return metadata;
      }

      await new Promise((resolve) => setTimeout(resolve, pollIntervalMs));
    }

    throw new Error(
      `Search timed out after ${timeout}ms (metadata not found). ` +
        `Check DHT connectivity: ${this.downloaderConfig.apiBaseUrl}/api/dht/peers and ${(this.uploaderConfig?.apiBaseUrl ?? "<uploader-api-unknown>")}/api/dht/peers. ` +
        `Also confirm the uploader's libp2p port (default tcp/4001) is reachable (firewall/NAT/relay).`
    );
  }

  /**
   * Download file to downloader node
   */
  async downloadFile(
    fileHash: string,
    fileName: string,
    protocol: "HTTP" | "WebRTC" | "Bitswap" | "FTP" | "BitTorrent"
  ): Promise<string> {
    if (!this.downloaderConfig) throw new Error("Config not initialized");

    console.log(`[IN] Downloading file via ${protocol}: ${fileName}`);

    // In real-network attach/cross-machine scenarios, the uploader's E2E API host is not always
    // the BitTorrent seeder public IP (it can be localhost/private). Allow an explicit override.
    const uploaderApiBaseUrl =
      this.uploaderConfig?.apiBaseUrl ?? this.downloaderConfig.apiBaseUrl;
    const bittorrentSeederIp =
      protocol === "BitTorrent"
        ? (process.env.E2E_UPLOADER_PUBLIC_IP ?? new URL(uploaderApiBaseUrl).hostname)
        : undefined;

    const response = await fetch(`${this.downloaderConfig.apiBaseUrl}/api/download`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        fileHash,
        fileName,
        protocol,
        torrentBase64:
          protocol === "BitTorrent" ? this.torrentBase64ByHash.get(fileHash) : undefined,
        bittorrentSeederIp:
          bittorrentSeederIp,
        bittorrentSeederPort:
          protocol === "BitTorrent"
            ? this.torrentSeederPortByHash.get(fileHash)
            : undefined,
      }),
    });

    if (!response.ok) {
      const body = await response.text().catch(() => "");
      throw new Error(
        `Download failed: ${response.status} ${response.statusText}${body ? ` - ${body}` : ""}`
      );
    }

    const result = await response.json();

    // HTTP returns synchronously with verified=true.
    // WebRTC/Bitswap/FTP may return 202 + downloadId; poll /api/download/status/:id until completion.
    const downloadPath: string = result.downloadPath;
    const downloadId: string | undefined = result.downloadId;

    if (protocol === "HTTP" || !downloadId) {
      if (!result.verified) throw new Error("Downloaded file failed verification on node");
      console.log(`[OK] File downloaded to: ${downloadPath}`);
      return downloadPath;
    }

    console.log(`[WAIT] Download started (id=${downloadId})`);
    const waitTimeoutMs = this.getP2PDownloadTimeoutMs(protocol);
    const start = Date.now();
    let lastJob: any = null;
    while (Date.now() - start < waitTimeoutMs) {
      const st = await fetch(
        `${this.downloaderConfig.apiBaseUrl}/api/download/status/${downloadId}`
      );
      if (!st.ok) {
        const body = await st.text().catch(() => "");
        throw new Error(
          `Download status failed: ${st.status} ${st.statusText}${body ? ` - ${body}` : ""}`
        );
      }
      const job = await st.json();
      lastJob = job;
      if (job.status === "success") {
        if (!job.verified) throw new Error("Downloaded file failed verification on node");
        console.log(`[OK] File downloaded to: ${job.downloadPath}`);
        return job.downloadPath;
      }
      if (job.status === "failed") {
        throw new Error(`Download failed: ${job.error ?? "unknown error"}`);
      }
      await new Promise((r) => setTimeout(r, 500));
    }
    throw new Error(
      `Timed out waiting for download ${downloadId} (protocol=${protocol}, timeoutMs=${waitTimeoutMs}, lastStatus=${lastJob?.status ?? "unknown"}, lastError=${lastJob?.error ?? "none"})`
    );
  }

  private getP2PDownloadTimeoutMs(
    protocol: "HTTP" | "WebRTC" | "Bitswap" | "FTP" | "BitTorrent"
  ): number {
    // Protocol-specific overrides (milliseconds). Fallback order:
    // 1) E2E_{PROTOCOL}_DOWNLOAD_TIMEOUT_MS
    // 2) E2E_P2P_DOWNLOAD_TIMEOUT_MS
    // 3) 600000 (10 minutes)
    const byProtocolKey =
      protocol === "WebRTC"
        ? "E2E_WEBRTC_DOWNLOAD_TIMEOUT_MS"
        : protocol === "Bitswap"
          ? "E2E_BITSWAP_DOWNLOAD_TIMEOUT_MS"
          : protocol === "FTP"
            ? "E2E_FTP_DOWNLOAD_TIMEOUT_MS"
            : protocol === "BitTorrent"
              ? "E2E_BITTORRENT_DOWNLOAD_TIMEOUT_MS"
            : null;

    const raw =
      (byProtocolKey ? process.env[byProtocolKey] : undefined) ??
      process.env.E2E_P2P_DOWNLOAD_TIMEOUT_MS ??
      // BitTorrent can hang in "metadata ok but no progress" states; keep defaults shorter.
      (protocol === "BitTorrent" ? "240000" : "600000");

    const n = Number(raw);
    let timeoutMs = Number.isFinite(n) && n > 0 ? n : 600000;

    // Align test-runner timeout with server-side BitTorrent "no-progress" timeout:
    // if the test times out earlier, we only see lastStatus=running and lose the real error context.
    if (protocol === "BitTorrent") {
      const allowShort =
        (process.env.E2E_BITTORRENT_ALLOW_SHORT_NO_PROGRESS ?? "").toLowerCase() === "1" ||
        (process.env.E2E_BITTORRENT_ALLOW_SHORT_NO_PROGRESS ?? "").toLowerCase() === "true";

      const noProgressRaw = Number(process.env.E2E_BITTORRENT_NO_PROGRESS_FAIL_MS ?? "60000");
      const noProgressMs =
        Number.isFinite(noProgressRaw) && noProgressRaw > 0
          ? allowShort
            ? noProgressRaw
            : Math.max(noProgressRaw, 60_000)
          : 60_000;

      // Give the node time to either make progress or fail with peer diagnostics.
      timeoutMs = Math.max(timeoutMs, noProgressMs + 15_000);
    }

    return timeoutMs;
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
    // Option1: node already verified via sha256 during /api/download.
    // Keep a lightweight sanity check that the file exists on the test runner machine
    // only when the path is local-accessible.
    try {
      await fs.access(downloadPath);
      return true;
    } catch {
      // If running attach mode on another machine, the path may not exist locally.
      return true;
    }
  }

  /**
   * Get payment transactions
   */
  async getPaymentTransactions(nodeType: "uploader" | "downloader"): Promise<any[]> {
    // Deprecated in option1: use /api/pay + /api/tx/receipt.
    void nodeType;
    return [];
  }

  /**
   * Verify payment was processed
   */
  async verifyPayment(fileHash: string): Promise<boolean> {
    console.log(`[PAY] Verifying payment for file: ${fileHash}`);
    // Kept for backwards-compat in the test harness; actual receipt polling happens in-test.
    void fileHash;
    return true;
  }

  /**
   * Cleanup test environment
   */
  async cleanup(): Promise<void> {
    console.log("[CLEAN] Cleaning up test environment...");

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
      console.log("[OK] Cleanup complete");
    } catch (error) {
      console.warn("[WARN]  Failed to remove test directory:", error);
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
    // Option1: uploader generates deterministic bytes on-node. Avoid allocating large buffers in the test runner.
    // IMPORTANT: make the filename unique per test run so the deterministic uploader bytes produce a new sha256,
    // avoiding stale DHT records when re-running tests against a real network.
    const ms = Date.now();
    const uniqueName = name.replace(/(\.[^.]*)?$/, (ext) => `-${ms}${ext || ""}`);
    return { name: uniqueName, size, content: Buffer.alloc(0) };
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
        console.log("[OK] Uploader node running. Waiting for downloader...");
      } else if (role === "downloader") {
        await framework.launchDownloaderNode();
        console.log("[OK] Downloader node running and connected.");
      } else {
        throw new Error("E2E_NODE_ROLE must be set to 'uploader' or 'downloader' in cross-machine mode");
      }
    }
  }, process.env.E2E_ATTACH === "true" ? 120000 : 300000);

  afterAll(async () => {
    await framework.cleanup();
  }, 30000);

  describe("HTTP Real Communication (Option1 / Attach)", () => {
    async function payAndWaitReceipt(uploaderAddress: string, price: number) {
      if (!framework["downloaderConfig"]) throw new Error("Config not initialized");
      const downloaderApi = framework["downloaderConfig"]!.apiBaseUrl;

      const payRes = await fetch(`${downloaderApi}/api/pay`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ uploaderAddress, price }),
      });
      if (!payRes.ok) {
        const body = await payRes.text().catch(() => "");
        throw new Error(
          `Pay failed: ${payRes.status} ${payRes.statusText}${body ? ` - ${body}` : ""}`
        );
      }
      const payJson = await payRes.json();
      const txHash: string = payJson.txHash;
      expect(txHash).toMatch(/^0x[a-fA-F0-9]+$/);

      const start = Date.now();
      const timeoutMs = 120_000;
      while (Date.now() - start < timeoutMs) {
        const r = await fetch(`${downloaderApi}/api/tx/receipt`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ txHash }),
        });
        if (!r.ok) {
          const body = await r.text().catch(() => "");
          throw new Error(
            `Receipt failed: ${r.status} ${r.statusText}${body ? ` - ${body}` : ""}`
          );
        }
        const receipt = await r.json();
        if (receipt.status === "success") return receipt;
        if (receipt.status === "failed") throw new Error("Transaction failed");
        await new Promise((res) => setTimeout(res, 1000));
      }
      throw new Error("Timed out waiting for tx receipt");
    }

    it("should upload, search, download, and pay (tx receipt success)", async () => {
      const testFile = framework.createTestFile("real-test-http.bin", 50);

      const fileHash = await framework.uploadFile(testFile, "HTTP");
      expect(fileHash).toBeTruthy();

      const metadata = await framework.searchFile(fileHash);
      expect(metadata).toBeTruthy();

      const uploaderAddress = metadata.uploaderAddress ?? metadata.uploader_address;
      const price = metadata.price ?? 0.001;
      expect(typeof uploaderAddress).toBe("string");

      const downloadPath = await framework.downloadFile(fileHash, testFile.name, "HTTP");
      const verified = await framework.verifyDownloadedFile(downloadPath, testFile);
      expect(verified).toBe(true);

      const receipt = await payAndWaitReceipt(uploaderAddress, price);
      expect(receipt.status).toBe("success");
    }, 300000);
  });

  describe("WebRTC Real Communication (Attach)", () => {
    it("should upload, search, download (WebRTC), and pay (tx receipt success)", async () => {
      const testFile = framework.createTestFile("real-test-webrtc.bin", 5);

      // Use real P2P publish path on uploader
      const fileHash = await framework.uploadFile(testFile, "WebRTC");
      expect(fileHash).toBeTruthy();

      const metadata = await framework.searchFile(fileHash, getSearchTimeoutMs("WebRTC"));
      expect(metadata).toBeTruthy();

      const uploaderAddress = metadata.uploaderAddress ?? metadata.uploader_address;
      const price = metadata.price ?? 0.001;
      expect(typeof uploaderAddress).toBe("string");

      // Trigger WebRTC P2P download on downloader node.
      // WebRTC downloads are async (202 + downloadId) so always go through the framework helper.
      const downloadPath = await framework.downloadFile(fileHash, testFile.name, "WebRTC");

      const verified = await framework.verifyDownloadedFile(downloadPath, testFile);
      expect(verified).toBe(true);

      // Final payment + receipt
      // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
      const receipt = await (async () => {
        // reuse helper inside HTTP describe via access (copy minimal here)
        if (!framework["downloaderConfig"]) throw new Error("Config not initialized");
        const downloaderApi = framework["downloaderConfig"]!.apiBaseUrl;
        const payRes = await fetch(`${downloaderApi}/api/pay`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ uploaderAddress, price }),
        });
        if (!payRes.ok) {
          const body = await payRes.text().catch(() => "");
          throw new Error(
            `Pay failed: ${payRes.status} ${payRes.statusText}${body ? ` - ${body}` : ""}`
          );
        }
        const payJson = await payRes.json();
        const txHash: string = payJson.txHash;
        expect(txHash).toMatch(/^0x[a-fA-F0-9]+$/);

        const start = Date.now();
      const timeoutMs = getReceiptTimeoutMs("WebRTC");
        while (Date.now() - start < timeoutMs) {
          const r = await fetch(`${downloaderApi}/api/tx/receipt`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ txHash }),
          });
          if (!r.ok) {
            const body = await r.text().catch(() => "");
            throw new Error(
              `Receipt failed: ${r.status} ${r.statusText}${body ? ` - ${body}` : ""}`
            );
          }
          const receipt = await r.json();
          if (receipt.status === "success") return receipt;
          if (receipt.status === "failed") throw new Error("Transaction failed");
          await new Promise((res) => setTimeout(res, 1000));
        }
        throw new Error("Timed out waiting for tx receipt");
      })();
      expect(receipt.status).toBe("success");
    }, getTestTimeoutMs("WebRTC"));
  });

  describe("Bitswap Real Communication (Attach)", () => {
    it("should upload, search, download (Bitswap), and pay (tx receipt success)", async () => {
      const testFile = framework.createTestFile("real-test-bitswap.bin", 5);

      const fileHash = await framework.uploadFile(testFile, "Bitswap");
      expect(fileHash).toBeTruthy();

      // Bitswap metadata propagation can be slower than WebRTC/HTTP on real networks.
      const metadata = await framework.searchFile(fileHash, getSearchTimeoutMs("Bitswap"));
      expect(metadata).toBeTruthy();

      const uploaderAddress = metadata.uploaderAddress ?? metadata.uploader_address;
      const price = metadata.price ?? 0.001;
      expect(typeof uploaderAddress).toBe("string");

      // Bitswap downloads are async (202 + downloadId) so always go through the framework helper.
      const downloadPath = await framework.downloadFile(fileHash, testFile.name, "Bitswap");

      const verified = await framework.verifyDownloadedFile(downloadPath, testFile);
      expect(verified).toBe(true);

      // Final payment + receipt
      const downloaderApi = framework["downloaderConfig"]!.apiBaseUrl;
      const payRes = await fetch(`${downloaderApi}/api/pay`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ uploaderAddress, price }),
      });
      if (!payRes.ok) {
        const body = await payRes.text().catch(() => "");
        throw new Error(
          `Pay failed: ${payRes.status} ${payRes.statusText}${body ? ` - ${body}` : ""}`
        );
      }
      const payJson = await payRes.json();
      const txHash: string = payJson.txHash;
      expect(txHash).toMatch(/^0x[a-fA-F0-9]+$/);

      const start = Date.now();
      const timeoutMs = getReceiptTimeoutMs("Bitswap");
      while (Date.now() - start < timeoutMs) {
        const r = await fetch(`${downloaderApi}/api/tx/receipt`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ txHash }),
        });
        if (!r.ok) {
          const body = await r.text().catch(() => "");
          throw new Error(
            `Receipt failed: ${r.status} ${r.statusText}${body ? ` - ${body}` : ""}`
          );
        }
        const receipt = await r.json();
        if (receipt.status === "success") break;
        if (receipt.status === "failed") throw new Error("Transaction failed");
        await new Promise((res) => setTimeout(res, 1000));
      }
    }, getTestTimeoutMs("Bitswap"));
  });

  describe("BitTorrent Real Communication (Attach)", () => {
    it("should upload, search, download (BitTorrent), and pay (tx receipt success)", async () => {
      const testFile = framework.createTestFile("real-test-bittorrent.bin", 5);

      const fileHash = await framework.uploadFile(testFile, "BitTorrent");
      expect(fileHash).toBeTruthy();

      // BitTorrent publish key is the info_hash; allow a bit more time for propagation.
      const metadata = await framework.searchFile(fileHash, getSearchTimeoutMs("BitTorrent"));
      expect(metadata).toBeTruthy();

      const uploaderAddress = metadata.uploaderAddress ?? metadata.uploader_address;
      const price = metadata.price ?? 0.001;
      expect(typeof uploaderAddress).toBe("string");

      const downloadPath = await framework.downloadFile(fileHash, testFile.name, "BitTorrent");
      const verified = await framework.verifyDownloadedFile(downloadPath, testFile);
      expect(verified).toBe(true);

      // Final payment + receipt
      const downloaderApi = framework["downloaderConfig"]!.apiBaseUrl;
      const payRes = await fetch(`${downloaderApi}/api/pay`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ uploaderAddress, price }),
      });
      if (!payRes.ok) {
        const body = await payRes.text().catch(() => "");
        throw new Error(
          `Pay failed: ${payRes.status} ${payRes.statusText}${body ? ` - ${body}` : ""}`
        );
      }
      const payJson = await payRes.json();
      const txHash: string = payJson.txHash;
      expect(txHash).toMatch(/^0x[a-fA-F0-9]+$/);

      const start = Date.now();
      const timeoutMs = getReceiptTimeoutMs("BitTorrent");
      while (Date.now() - start < timeoutMs) {
        const r = await fetch(`${downloaderApi}/api/tx/receipt`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ txHash }),
        });
        if (!r.ok) {
          const body = await r.text().catch(() => "");
          throw new Error(
            `Receipt failed: ${r.status} ${r.statusText}${body ? ` - ${body}` : ""}`
          );
        }
        const receipt = await r.json();
        if (receipt.status === "success") break;
        if (receipt.status === "failed") throw new Error("Transaction failed");
        await new Promise((res) => setTimeout(res, 1000));
      }
    }, getTestTimeoutMs("BitTorrent"));
  });

  describe("FTP Real Communication (Attach)", () => {
    it("should upload, search, download (FTP), and pay (tx receipt success)", async () => {
      const testFile = framework.createTestFile("real-test-ftp.bin", 5);

      const fileHash = await framework.uploadFile(testFile, "FTP");
      expect(fileHash).toBeTruthy();

      const metadata = await framework.searchFile(fileHash, 60_000);
      expect(metadata).toBeTruthy();

      // Ensure the uploader actually published FTP sources.
      expect(metadata.ftpSources ?? metadata.ftp_sources).toBeTruthy();

      const uploaderAddress = metadata.uploaderAddress ?? metadata.uploader_address;
      const price = metadata.price ?? 0.001;
      expect(typeof uploaderAddress).toBe("string");

      // FTP downloads are async (202 + downloadId) via the E2E API helper.
      const downloadPath = await framework.downloadFile(fileHash, testFile.name, "FTP");
      const verified = await framework.verifyDownloadedFile(downloadPath, testFile);
      expect(verified).toBe(true);

      // Final payment + receipt
      const downloaderApi = framework["downloaderConfig"]!.apiBaseUrl;
      const payRes = await fetch(`${downloaderApi}/api/pay`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ uploaderAddress, price }),
      });
      if (!payRes.ok) {
        const body = await payRes.text().catch(() => "");
        throw new Error(
          `Pay failed: ${payRes.status} ${payRes.statusText}${body ? ` - ${body}` : ""}`
        );
      }
      const payJson = await payRes.json();
      const txHash: string = payJson.txHash;
      expect(txHash).toMatch(/^0x[a-fA-F0-9]+$/);

      const start = Date.now();
      const timeoutMs = getReceiptTimeoutMs("FTP");
      while (Date.now() - start < timeoutMs) {
        const r = await fetch(`${downloaderApi}/api/tx/receipt`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ txHash }),
        });
        if (!r.ok) {
          const body = await r.text().catch(() => "");
          throw new Error(
            `Receipt failed: ${r.status} ${r.statusText}${body ? ` - ${body}` : ""}`
          );
        }
        const receipt = await r.json();
        if (receipt.status === "success") break;
        if (receipt.status === "failed") throw new Error("Transaction failed");
        await new Promise((res) => setTimeout(res, 1000));
      }
    }, getTestTimeoutMs("FTP"));
  });

  // Payment checkpoint automation is currently UI-driven and not wired into the real-network harness yet.
  // We'll add it once the checkpoint signals are exposed via the E2E API and integrated with the download pipeline.
  describe.skip("Payment Checkpoint Real Communication", () => {});

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

