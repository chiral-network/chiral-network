import { invoke } from '@tauri-apps/api/core';

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

export async function getStatus(
  dataDir?: string,
  logLines = 40
): Promise<GethStatus> {
  return invoke<GethStatus>('get_geth_status', {
    dataDir,
    logLines,
  });
}
