/**
 * @fileoverview Common test helpers for E2E tests
 * Provides utilities for mocking DHT nodes, peers, files, and payment services
 */

import { vi } from "vitest";
import type { FileMetadata } from "../../src/lib/dht";

export interface MockPeer {
  id: string;
  address: string;
  reputation: number;
  bandwidth: number;
  connected: boolean;
  protocol: "Bitswap" | "WebRTC" | "BitTorrent" | "HTTP" | "FTP";
}

export interface MockFileData {
  hash: string;
  name: string;
  size: number;
  content: Uint8Array;
  chunks: number;
  cids?: string[];
}

export interface MockPaymentResult {
  success: boolean;
  transactionId: string;
  transactionHash: string;
  amount: number;
  error?: string;
}

/**
 * Test data factory
 */
export class TestDataFactory {
  /**
   * Create mock peers for testing
   */
  static createMockPeers(
    count: number = 3,
    protocol: MockPeer["protocol"] = "WebRTC"
  ): MockPeer[] {
    return Array.from({ length: count }, (_, i) => ({
      id: `peer_${i}_${Date.now()}`,
      address: `0x${i.toString(16).padStart(40, "0")}`,
      reputation: 50 + Math.random() * 50, // 50-100 reputation
      bandwidth: 1000 + Math.random() * 9000, // 1-10 Mbps
      connected: true,
      protocol,
    }));
  }

  /**
   * Create mock file data for testing
   */
  static createMockFile(
    name: string = "test-file.txt",
    size: number = 1024 * 1024 // 1MB default
  ): MockFileData {
    const content = new Uint8Array(size);
    // Fill with pseudo-random data
    for (let i = 0; i < size; i++) {
      content[i] = i % 256;
    }

    const chunkSize = 64 * 1024; // 64KB chunks
    const chunks = Math.ceil(size / chunkSize);

    return {
      hash: `QmTest${name.replace(/[^a-zA-Z0-9]/g, "")}${Date.now()}`,
      name,
      size,
      content,
      chunks,
      cids: Array.from({ length: chunks }, (_, i) => `QmChunk${i}`),
    };
  }

  /**
   * Create mock file metadata
   */
  static createMockMetadata(
    file: MockFileData,
    seeders: MockPeer[],
    protocol: string = "WebRTC"
  ): FileMetadata {
    return {
      merkleRoot: file.hash,
      fileHash: file.hash,
      fileName: file.name,
      fileSize: file.size,
      fileData: Array.from(file.content),
      seeders: seeders.map((p) => p.id),
      seederAddresses: seeders.map((p) => p.address),
      createdAt: Date.now(),
      mimeType: "text/plain",
      isEncrypted: false,
      cids: file.cids,
      price: 0.001,
      uploaderAddress: seeders[0]?.address,
      protocol,
    } as FileMetadata;
  }

  /**
   * Create mock payment result
   */
  static createMockPaymentResult(
    success: boolean = true,
    amount: number = 0.001
  ): MockPaymentResult {
    return {
      success,
      transactionId: `tx_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
      transactionHash: `0x${Math.random().toString(16).substr(2, 64)}`,
      amount,
      error: success ? undefined : "Payment failed",
    };
  }
}

/**
 * Mock DHT service for testing
 */
export class MockDHTService {
  private publishedFiles = new Map<string, FileMetadata>();
  private searchDelay: number;

  constructor(searchDelay: number = 100) {
    this.searchDelay = searchDelay;
  }

  /**
   * Mock publish file to DHT
   */
  async publishFile(metadata: FileMetadata): Promise<void> {
    await this.delay(50);
    this.publishedFiles.set(metadata.merkleRoot, metadata);
  }

  /**
   * Mock search for file metadata
   */
  async searchFileMetadata(
    fileHash: string
  ): Promise<FileMetadata | null> {
    await this.delay(this.searchDelay);
    return this.publishedFiles.get(fileHash) || null;
  }

  /**
   * Mock get seeders for file
   */
  async getSeedersForFile(fileHash: string): Promise<string[]> {
    await this.delay(50);
    const metadata = this.publishedFiles.get(fileHash);
    return metadata?.seeders || [];
  }

  /**
   * Get all published files
   */
  getPublishedFiles(): Map<string, FileMetadata> {
    return this.publishedFiles;
  }

  /**
   * Clear all published files
   */
  clear(): void {
    this.publishedFiles.clear();
  }

  private delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}

/**
 * Mock payment service for testing
 */
export class MockPaymentService {
  private processedPayments = new Set<string>();
  private balance: number;

  constructor(initialBalance: number = 10) {
    this.balance = initialBalance;
  }

  /**
   * Mock calculate download cost
   */
  async calculateDownloadCost(fileSize: number): Promise<number> {
    const sizeInMB = fileSize / (1024 * 1024);
    const pricePerMB = 0.001;
    const cost = Math.max(0.0001, sizeInMB * pricePerMB);
    return parseFloat(cost.toFixed(8));
  }

  /**
   * Mock process download payment
   */
  async processDownloadPayment(
    fileHash: string,
    fileName: string,
    fileSize: number,
    seederAddress: string
  ): Promise<MockPaymentResult> {
    const paymentKey = `${fileHash}_${seederAddress}`;

    if (this.processedPayments.has(paymentKey)) {
      return {
        success: false,
        transactionId: "",
        transactionHash: "",
        amount: 0,
        error: "Payment already processed",
      };
    }

    const amount = await this.calculateDownloadCost(fileSize);

    if (this.balance < amount) {
      return {
        success: false,
        transactionId: "",
        transactionHash: "",
        amount: 0,
        error: "Insufficient balance",
      };
    }

    this.balance -= amount;
    this.processedPayments.add(paymentKey);

    return TestDataFactory.createMockPaymentResult(true, amount);
  }

  /**
   * Get current balance
   */
  getBalance(): number {
    return this.balance;
  }

  /**
   * Check if payment was processed
   */
  hasProcessedPayment(fileHash: string, seederAddress: string): boolean {
    return this.processedPayments.has(`${fileHash}_${seederAddress}`);
  }

  /**
   * Reset payment service
   */
  reset(initialBalance: number = 10): void {
    this.processedPayments.clear();
    this.balance = initialBalance;
  }
}

/**
 * Event listener helper for async events
 */
export class EventHelper {
  private listeners = new Map<string, Array<(data: any) => void>>();

  /**
   * Register event listener
   */
  on(event: string, callback: (data: any) => void): void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, []);
    }
    this.listeners.get(event)!.push(callback);
  }

  /**
   * Emit event
   */
  emit(event: string, data: any): void {
    const callbacks = this.listeners.get(event);
    if (callbacks) {
      callbacks.forEach((cb) => cb(data));
    }
  }

  /**
   * Wait for specific event
   */
  waitForEvent<T = any>(
    event: string,
    timeout: number = 5000
  ): Promise<T> {
    return new Promise((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        reject(new Error(`Timeout waiting for event: ${event}`));
      }, timeout);

      this.on(event, (data) => {
        clearTimeout(timeoutId);
        resolve(data as T);
      });
    });
  }

  /**
   * Clear all listeners
   */
  clear(): void {
    this.listeners.clear();
  }
}

/**
 * Mock Tauri invoke helper
 */
export class MockTauriInvoke {
  private handlers = new Map<string, (args: any) => Promise<any>>();
  private eventHelper = new EventHelper();

  /**
   * Register mock handler for Tauri command
   */
  mockCommand(command: string, handler: (args: any) => Promise<any>): void {
    this.handlers.set(command, handler);
  }

  /**
   * Create invoke function for testing
   */
  createInvoke() {
    return vi.fn(async (command: string, args?: any) => {
      const handler = this.handlers.get(command);
      if (handler) {
        return await handler(args || {});
      }
      throw new Error(`No mock handler for command: ${command}`);
    });
  }

  /**
   * Get event helper for listening to events
   */
  getEventHelper(): EventHelper {
    return this.eventHelper;
  }

  /**
   * Clear all handlers
   */
  clear(): void {
    this.handlers.clear();
    this.eventHelper.clear();
  }
}

/**
 * Download progress simulator
 */
export class DownloadProgressSimulator {
  private totalChunks: number;
  private downloadedChunks: Set<number>;
  private onProgress?: (progress: number, downloaded: number, total: number) => void;

  constructor(totalChunks: number) {
    this.totalChunks = totalChunks;
    this.downloadedChunks = new Set();
  }

  /**
   * Set progress callback
   */
  setProgressCallback(
    callback: (progress: number, downloaded: number, total: number) => void
  ): void {
    this.onProgress = callback;
  }

  /**
   * Simulate downloading a chunk
   */
  async downloadChunk(chunkIndex: number, delayMs: number = 10): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, delayMs));
    
    if (chunkIndex >= 0 && chunkIndex < this.totalChunks) {
      this.downloadedChunks.add(chunkIndex);
      const downloaded = this.downloadedChunks.size;
      const progress = (downloaded / this.totalChunks) * 100;
      
      if (this.onProgress) {
        this.onProgress(progress, downloaded, this.totalChunks);
      }
    }
  }

  /**
   * Simulate downloading all chunks
   */
  async downloadAll(delayPerChunk: number = 10): Promise<void> {
    for (let i = 0; i < this.totalChunks; i++) {
      await this.downloadChunk(i, delayPerChunk);
    }
  }

  /**
   * Check if download is complete
   */
  isComplete(): boolean {
    return this.downloadedChunks.size === this.totalChunks;
  }

  /**
   * Get progress percentage
   */
  getProgress(): number {
    return (this.downloadedChunks.size / this.totalChunks) * 100;
  }

  /**
   * Reset simulator
   */
  reset(): void {
    this.downloadedChunks.clear();
  }
}

/**
 * WebRTC handshake simulator
 */
export class WebRTCHandshakeSimulator {
  private connections = new Map<string, { state: string; dataChannelOpen: boolean }>();

  /**
   * Simulate WebRTC handshake between two peers
   */
  async simulateHandshake(
    peerA: string,
    peerB: string,
    delayMs: number = 100
  ): Promise<boolean> {
    // Simulate offer
    await this.delay(delayMs / 3);
    this.connections.set(peerA, { state: "connecting", dataChannelOpen: false });
    
    // Simulate answer
    await this.delay(delayMs / 3);
    this.connections.set(peerB, { state: "connecting", dataChannelOpen: false });
    
    // Simulate connection established
    await this.delay(delayMs / 3);
    this.connections.set(peerA, { state: "connected", dataChannelOpen: true });
    this.connections.set(peerB, { state: "connected", dataChannelOpen: true });
    
    return true;
  }

  /**
   * Check if connection is established
   */
  isConnected(peerId: string): boolean {
    const conn = this.connections.get(peerId);
    return conn?.state === "connected" && conn?.dataChannelOpen === true;
  }

  /**
   * Close connection
   */
  closeConnection(peerId: string): void {
    this.connections.delete(peerId);
  }

  /**
   * Clear all connections
   */
  clear(): void {
    this.connections.clear();
  }

  private delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}

/**
 * Helper to clean up test resources
 */
export class TestCleanup {
  private cleanupFns: Array<() => void | Promise<void>> = [];

  /**
   * Register cleanup function
   */
  register(fn: () => void | Promise<void>): void {
    this.cleanupFns.push(fn);
  }

  /**
   * Execute all cleanup functions
   */
  async cleanup(): Promise<void> {
    for (const fn of this.cleanupFns) {
      await fn();
    }
    this.cleanupFns = [];
  }
}

