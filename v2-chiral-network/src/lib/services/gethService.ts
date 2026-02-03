/**
 * Geth Service - Manages Geth blockchain node
 *
 * Handles:
 * - Geth download and installation
 * - Starting/stopping Geth
 * - Mining management
 * - Status queries
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { writable, type Writable } from 'svelte/store';

// ============================================================================
// Types
// ============================================================================

export interface DownloadProgress {
  downloaded: number;
  total: number;
  percentage: number;
  status: string;
}

export interface GethStatus {
  installed: boolean;
  running: boolean;
  syncing: boolean;
  currentBlock: number;
  highestBlock: number;
  peerCount: number;
  chainId: number;
}

export interface MiningStatus {
  mining: boolean;
  hashRate: number;
  minerAddress: string | null;
}

// ============================================================================
// Stores
// ============================================================================

export const gethStatus: Writable<GethStatus | null> = writable(null);
export const miningStatus: Writable<MiningStatus | null> = writable(null);
export const downloadProgress: Writable<DownloadProgress | null> = writable(null);
export const isDownloading: Writable<boolean> = writable(false);

// ============================================================================
// Service
// ============================================================================

class GethService {
  private statusInterval: ReturnType<typeof setInterval> | null = null;
  private unlistenDownload: (() => void) | null = null;

  /**
   * Check if Geth is installed
   */
  async isInstalled(): Promise<boolean> {
    try {
      return await invoke<boolean>('is_geth_installed');
    } catch (error) {
      console.error('Failed to check Geth installation:', error);
      return false;
    }
  }

  /**
   * Download and install Geth
   */
  async download(): Promise<void> {
    isDownloading.set(true);
    downloadProgress.set({
      downloaded: 0,
      total: 0,
      percentage: 0,
      status: 'Starting download...'
    });

    // Listen for progress events
    this.unlistenDownload = await listen<DownloadProgress>('geth-download-progress', (event) => {
      downloadProgress.set(event.payload);
    });

    try {
      await invoke('download_geth');
      downloadProgress.set({
        downloaded: 0,
        total: 0,
        percentage: 100,
        status: 'Installation complete!'
      });
    } finally {
      isDownloading.set(false);
      if (this.unlistenDownload) {
        this.unlistenDownload();
        this.unlistenDownload = null;
      }
    }
  }

  /**
   * Start Geth node
   */
  async start(minerAddress?: string): Promise<void> {
    await invoke('start_geth', { minerAddress });
    // Start status polling
    this.startStatusPolling();
  }

  /**
   * Stop Geth node
   */
  async stop(): Promise<void> {
    await invoke('stop_geth');
    this.stopStatusPolling();
    gethStatus.set(null);
    miningStatus.set(null);
  }

  /**
   * Get current Geth status
   */
  async getStatus(): Promise<GethStatus> {
    const status = await invoke<GethStatus>('get_geth_status');
    gethStatus.set(status);
    return status;
  }

  /**
   * Start mining
   */
  async startMining(threads: number = 1): Promise<void> {
    await invoke('start_mining', { threads });
    await this.getMiningStatus();
  }

  /**
   * Stop mining
   */
  async stopMining(): Promise<void> {
    await invoke('stop_mining');
    await this.getMiningStatus();
  }

  /**
   * Get mining status
   */
  async getMiningStatus(): Promise<MiningStatus> {
    const status = await invoke<MiningStatus>('get_mining_status');
    miningStatus.set(status);
    return status;
  }

  /**
   * Set miner address (coinbase)
   */
  async setMinerAddress(address: string): Promise<void> {
    await invoke('set_miner_address', { address });
  }

  /**
   * Get chain ID
   */
  async getChainId(): Promise<number> {
    return await invoke<number>('get_chain_id');
  }

  /**
   * Start polling for status updates
   */
  startStatusPolling(intervalMs: number = 5000): void {
    this.stopStatusPolling();

    // Initial fetch
    this.getStatus().catch(console.error);
    this.getMiningStatus().catch(console.error);

    // Poll every intervalMs
    this.statusInterval = setInterval(async () => {
      try {
        await this.getStatus();
        await this.getMiningStatus();
      } catch (error) {
        // Geth might not be running
        console.debug('Status poll failed:', error);
      }
    }, intervalMs);
  }

  /**
   * Stop status polling
   */
  stopStatusPolling(): void {
    if (this.statusInterval) {
      clearInterval(this.statusInterval);
      this.statusInterval = null;
    }
  }

  /**
   * Initialize the service - check status on startup
   */
  async initialize(): Promise<void> {
    try {
      const installed = await this.isInstalled();
      if (installed) {
        // Try to get status (Geth might already be running)
        const status = await this.getStatus();
        if (status.running) {
          this.startStatusPolling();
        }
      }
    } catch (error) {
      console.debug('Geth initialization check failed:', error);
    }
  }
}

// Export singleton instance
export const gethService = new GethService();
