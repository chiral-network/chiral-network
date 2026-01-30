import { invoke } from '@tauri-apps/api/core'
import { writable, derived } from 'svelte/store'

export interface ChunkMeta {
  idx: number
  offset: number
  size: number
  hash: string
  state: 'Pending' | 'Downloaded' | 'Verified' | 'Failed'
}

export interface RecoveryInfo {
  merkle_root: string
  file_name: string
  file_size: number
  tmp_path: string
  dst_path: string
  total_chunks: number
  pending: number
  downloaded: number
  verified: number
  failed: number
  providers: string[]
  encrypted: boolean
}

export interface VerifyResult {
  merkle_root: string
  valid: boolean
  total_chunks: number
  verified_chunks: number
  failed_chunks: number[]
  error?: string
}

export interface CorruptionReport {
  merkle_root: string
  corrupted_chunks: number[]
  missing_chunks: number[]
  verified_chunks: number[]
  total_chunks: number
  corruption_detected: boolean
}

export interface CoordinatorProgress {
  merkle_root: string
  file_name: string
  total_chunks: number
  pending: number
  downloaded: number
  verified: number
  failed: number
  active_peers: number
  avg_speed: number
  eta_secs: number
}

export interface ChunkFetchReq {
  chunk_idx: number
  peer_id: string
  offset: number
  size: number
  hash: string
}

export const recoveryList = writable<RecoveryInfo[]>([])
export const activeRecoveries = writable<Map<string, CoordinatorProgress>>(new Map())
export const corruptionAlerts = writable<CorruptionReport[]>([])

export async function scanIncomplete(dlDir: string): Promise<RecoveryInfo[]> {
  try {
    const list = await invoke<RecoveryInfo[]>('p2p_chunk_scan', { dlDir })
    recoveryList.set(list)
    return list
  } catch (e) {
    console.error('scan failed:', e)
    return []
  }
}

export async function getState(tmpPath: string): Promise<RecoveryInfo | null> {
  try {
    return await invoke<RecoveryInfo | null>('p2p_chunk_get_state', { tmpPath })
  } catch (e) {
    console.error('get state failed:', e)
    return null
  }
}

export async function verifyChunks(tmpPath: string): Promise<VerifyResult> {
  try {
    return await invoke<VerifyResult>('p2p_chunk_verify', { tmpPath })
  } catch (e) {
    console.error('verify failed:', e)
    return {
      merkle_root: '',
      valid: false,
      total_chunks: 0,
      verified_chunks: 0,
      failed_chunks: [],
      error: String(e)
    }
  }
}

export async function checkCorruption(tmpPath: string): Promise<CorruptionReport> {
  try {
    const report = await invoke<CorruptionReport>('p2p_chunk_check_corruption', { tmpPath })
    if (report.corruption_detected) {
      corruptionAlerts.update(alerts => {
        if (alerts.find(a => a.merkle_root === report.merkle_root)) return alerts
        return [...alerts, report]
      })
    }
    return report
  } catch (e) {
    console.error('corruption check failed:', e)
    return {
      merkle_root: '',
      corrupted_chunks: [],
      missing_chunks: [],
      verified_chunks: [],
      total_chunks: 0,
      corruption_detected: false
    }
  }
}

export async function removeState(tmpPath: string): Promise<void> {
  try {
    await invoke<void>('p2p_chunk_remove', { tmpPath })
  } catch (e) {
    console.error('remove failed:', e)
  }
}

export async function computeMerkle(hashes: string[]): Promise<string> {
  try {
    return await invoke<string>('p2p_chunk_compute_merkle', { chunkHashes: hashes })
  } catch (e) {
    console.error('merkle compute failed:', e)
    return ''
  }
}

export async function verifyMerkle(root: string, hashes: string[]): Promise<boolean> {
  try {
    return await invoke<boolean>('p2p_chunk_verify_merkle', { merkleRoot: root, chunkHashes: hashes })
  } catch (e) {
    console.error('merkle verify failed:', e)
    return false
  }
}

export async function hashChunk(data: Uint8Array): Promise<string> {
  try {
    return await invoke<string>('p2p_chunk_hash', { data: Array.from(data) })
  } catch (e) {
    console.error('hash failed:', e)
    return ''
  }
}

// scans dl dir and returns incomplete downloads for recovery
export async function startupRecovery(dlDir: string): Promise<RecoveryInfo[]> {
  try {
    const list = await invoke<RecoveryInfo[]>('p2p_chunk_startup_recovery', { dlDir })
    recoveryList.set(list)
    return list
  } catch (e) {
    console.error('startup recovery failed:', e)
    return []
  }
}

export async function createCoordinator(tmpPath: string): Promise<CoordinatorProgress | null> {
  try {
    const progress = await invoke<CoordinatorProgress>('p2p_chunk_create_coordinator', { tmpPath })
    activeRecoveries.update(m => {
      m.set(progress.merkle_root, progress)
      return new Map(m)
    })
    return progress
  } catch (e) {
    console.error('create coordinator failed:', e)
    return null
  }
}

export async function assignPending(tmpPath: string): Promise<ChunkFetchReq[]> {
  try {
    return await invoke<ChunkFetchReq[]>('p2p_chunk_assign_pending', { tmpPath })
  } catch (e) {
    console.error('assign pending failed:', e)
    return []
  }
}

export async function reportResult(
  tmpPath: string,
  chunkIdx: number,
  success: boolean,
  bytes: number,
  durMs: number
): Promise<CoordinatorProgress | null> {
  try {
    const progress = await invoke<CoordinatorProgress>('p2p_chunk_report_result', {
      tmpPath,
      chunkIdx,
      success,
      bytes,
      durationMs: durMs
    })
    activeRecoveries.update(m => {
      m.set(progress.merkle_root, progress)
      return new Map(m)
    })
    return progress
  } catch (e) {
    console.error('report result failed:', e)
    return null
  }
}

export async function getProgress(tmpPath: string): Promise<CoordinatorProgress | null> {
  try {
    const progress = await invoke<CoordinatorProgress>('p2p_chunk_get_progress', { tmpPath })
    activeRecoveries.update(m => {
      m.set(progress.merkle_root, progress)
      return new Map(m)
    })
    return progress
  } catch (e) {
    console.error('get progress failed:', e)
    return null
  }
}

export function clearAlert(merkleRoot: string) {
  corruptionAlerts.update(alerts => alerts.filter(a => a.merkle_root !== merkleRoot))
}

export function clearAllAlerts() {
  corruptionAlerts.set([])
}

export const activeRecoveryCnt = derived(activeRecoveries, $m => $m.size)
export const alertCnt = derived(corruptionAlerts, $a => $a.length)
