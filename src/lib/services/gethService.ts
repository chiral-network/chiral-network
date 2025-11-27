import { invoke } from '@tauri-apps/api/core';
import { writable } from 'svelte/store';

export interface GethStatus {
  installed: boolean;
  running: boolean;
  binaryPath: string | null;
  dataDir: string;
  dataDirExists: boolean;
  logPath: string | null;
  logAvailable: boolean;
  logLines: number;
  version: string | null;
  lastLogs: string[];
  lastUpdated: number;
}

export interface SyncStatus {
  syncing: boolean;
  current_block: number;
  highest_block: number;
  starting_block: number;
  progress_percent: number;
  blocks_remaining: number;
  estimated_seconds_remaining: number | null;
}

// Export Geth running status store (similar to networkStatus)
export const gethStatus = writable<"running" | "stopped">("stopped");
export const gethSyncStatus = writable<SyncStatus | null>(null);

export async function getStatus(
  dataDir?: string,
  logLines = 40
): Promise<GethStatus> {
  return invoke<GethStatus>('get_geth_status', {
    dataDir,
    logLines,
  });
}

// Function to update Geth running status
export async function updateGethStatus(): Promise<void> {
  try {
    const isRunning = await invoke<boolean>("is_geth_running");
    gethStatus.set(isRunning ? "running" : "stopped");
  } catch (error) {
    // Silently fail - Geth may not be running
    gethStatus.set("stopped");
  }
}

// Function to update sync status
export async function updateSyncStatus(): Promise<void> {
  try {
    const syncStatus = await invoke<SyncStatus>("get_blockchain_sync_status");
    gethSyncStatus.set(syncStatus);
  } catch (error) {
    // Silently fail - Geth may not be running
    gethSyncStatus.set(null);
  }
}

// Start periodic monitoring
export function startGethMonitoring(): () => void {
  // Check immediately
  updateGethStatus();
  updateSyncStatus();

  // Check status every 5 seconds, sync status every 10 seconds
  const statusInterval = setInterval(updateGethStatus, 5000);
  const syncInterval = setInterval(updateSyncStatus, 10000);

  // Return cleanup function
  return () => {
    clearInterval(statusInterval);
    clearInterval(syncInterval);
  };
}
