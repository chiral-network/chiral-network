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

// Export Geth running status store (similar to networkStatus)
export const gethStatus = writable<"running" | "stopped">("stopped");

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

// Start periodic monitoring
export function startGethMonitoring(): () => void {
  // Check immediately
  updateGethStatus();

  // Check every 5 seconds
  const interval = setInterval(updateGethStatus, 5000);

  // Return cleanup function
  return () => {
    clearInterval(interval);
  };
}
