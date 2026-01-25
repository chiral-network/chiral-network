import { invoke } from '@tauri-apps/api/core'
import { writable, derived, get } from 'svelte/store'

// chunk recovery state for p2p downloads
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

// stores for reactive ui updates
export const recoveryList = writable<RecoveryInfo[]>([])
export const activeRecoveries = writable<Map<string, CoordinatorProgress>>(new Map())
export const corruptionAlerts = writable<CorruptionReport[]>([])

// scan for incomplete downloads
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

// get state for specific download
export async function getState(tmpPath: string): Promise<RecoveryInfo | null> {
  try {
    return await invoke<RecoveryInfo | null>('p2p_chunk_get_state', { tmpPath })
  } catch (e) {
    console.error('get state failed:', e)
    return null
  }
}

// verify all chunks
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

// check for corruption
export async function checkCorruption(tmpPath: string): Promise<CorruptionReport> {
  try {
    const report = await invoke<CorruptionReport>('p2p_chunk_check_corruption', { tmpPath })
    if (report.corruption_detected) {
      corruptionAlerts.update(alerts => {
        const exists = alerts.find(a => a.merkle_root === report.merkle_root)
        if (!exists) return [...alerts, report]
        return alerts
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

// remove download state
export async function removeState(tmpPath: string): Promise<void> {
  try {
    await invoke<void>('p2p_chunk_remove', { tmpPath })
  } catch (e) {
    console.error('remove failed:', e)
  }
}

// compute merkle root from hashes
export async function computeMerkle(hashes: string[]): Promise<string> {
  try {
    return await invoke<string>('p2p_chunk_compute_merkle', { chunkHashes: hashes })
  } catch (e) {
    console.error('merkle compute failed:', e)
    return ''
  }
}

// verify merkle root
export async function verifyMerkle(root: string, hashes: string[]): Promise<boolean> {
  try {
    return await invoke<boolean>('p2p_chunk_verify_merkle', { merkleRoot: root, chunkHashes: hashes })
  } catch (e) {
    console.error('merkle verify failed:', e)
    return false
  }
}

// hash chunk data
export async function hashChunk(data: Uint8Array): Promise<string> {
  try {
    return await invoke<string>('p2p_chunk_hash', { data: Array.from(data) })
  } catch (e) {
    console.error('hash failed:', e)
    return ''
  }
}

// startup recovery - scan and auto-resume
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

// create coordinator for multi-peer download
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

// get pending chunks to fetch
export async function assignPending(tmpPath: string): Promise<ChunkFetchReq[]> {
  try {
    return await invoke<ChunkFetchReq[]>('p2p_chunk_assign_pending', { tmpPath })
  } catch (e) {
    console.error('assign pending failed:', e)
    return []
  }
}

// report chunk fetch result
export async function reportResult(
  tmpPath: string,
  chunkIdx: number,
  success: boolean,
  bytes: number,
  durationMs: number
): Promise<CoordinatorProgress | null> {
  try {
    const progress = await invoke<CoordinatorProgress>('p2p_chunk_report_result', {
      tmpPath,
      chunkIdx,
      success,
      bytes,
      durationMs
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

// get current progress
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

// clear corruption alert
export function clearAlert(merkleRoot: string) {
  corruptionAlerts.update(alerts => alerts.filter(a => a.merkle_root !== merkleRoot))
}

// clear all alerts
export function clearAllAlerts() {
  corruptionAlerts.set([])
}

// derived store for active recovery count
export const activeRecoveryCount = derived(activeRecoveries, $m => $m.size)

// derived store for total corruption alerts
export const alertCount = derived(corruptionAlerts, $a => $a.length)
