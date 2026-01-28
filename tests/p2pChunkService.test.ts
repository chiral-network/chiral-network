import { describe, it, expect, beforeEach, vi } from 'vitest'
import { get } from 'svelte/store'

// mock tauri invoke before importing the service
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

import { invoke } from '@tauri-apps/api/core'
import {
  recoveryList,
  activeRecoveries,
  corruptionAlerts,
  scanIncomplete,
  getState,
  verifyChunks,
  checkCorruption,
  removeState,
  computeMerkle,
  verifyMerkle,
  hashChunk,
  startupRecovery,
  createCoordinator,
  assignPending,
  reportResult,
  getProgress,
  clearAlert,
  clearAllAlerts,
  activeRecoveryCnt,
  alertCnt
} from '$lib/services/p2pChunkService'

const mockInvoke = vi.mocked(invoke)

const mockRecovery = {
  merkle_root: 'abc123',
  file_name: 'test.zip',
  file_size: 1048576,
  tmp_path: '/tmp/test.zip.part',
  dst_path: '/dl/test.zip',
  total_chunks: 4,
  pending: 1,
  downloaded: 2,
  verified: 1,
  failed: 0,
  providers: ['peer1'],
  encrypted: false
}

const mockProgress = {
  merkle_root: 'abc123',
  file_name: 'test.zip',
  total_chunks: 4,
  pending: 1,
  downloaded: 2,
  verified: 1,
  failed: 0,
  active_peers: 2,
  avg_speed: 1024000,
  eta_secs: 30
}

describe('p2pChunkService', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    recoveryList.set([])
    activeRecoveries.set(new Map())
    corruptionAlerts.set([])
  })

  describe('scanIncomplete', () => {
    it('returns list and updates store', async () => {
      mockInvoke.mockResolvedValue([mockRecovery])
      const res = await scanIncomplete('/dl')
      expect(mockInvoke).toHaveBeenCalledWith('p2p_chunk_scan', { dlDir: '/dl' })
      expect(res).toEqual([mockRecovery])
      expect(get(recoveryList)).toEqual([mockRecovery])
    })

    it('returns empty on error', async () => {
      mockInvoke.mockRejectedValue(new Error('fail'))
      const res = await scanIncomplete('/dl')
      expect(res).toEqual([])
    })
  })

  describe('getState', () => {
    it('returns recovery info', async () => {
      mockInvoke.mockResolvedValue(mockRecovery)
      const res = await getState('/tmp/test.zip.part')
      expect(res).toEqual(mockRecovery)
    })

    it('returns null on error', async () => {
      mockInvoke.mockRejectedValue(new Error('fail'))
      const res = await getState('/tmp/test.zip.part')
      expect(res).toBeNull()
    })
  })

  describe('verifyChunks', () => {
    it('returns valid result', async () => {
      const result = { merkle_root: 'abc', valid: true, total_chunks: 4, verified_chunks: 4, failed_chunks: [] }
      mockInvoke.mockResolvedValue(result)
      const res = await verifyChunks('/tmp/test')
      expect(res.valid).toBe(true)
    })

    it('returns fallback on error', async () => {
      mockInvoke.mockRejectedValue(new Error('bad'))
      const res = await verifyChunks('/tmp/test')
      expect(res.valid).toBe(false)
      expect(res.error).toContain('bad')
    })
  })

  describe('checkCorruption', () => {
    it('adds alert on corruption', async () => {
      const report = {
        merkle_root: 'abc123',
        corrupted_chunks: [1, 2],
        missing_chunks: [3],
        verified_chunks: [0],
        total_chunks: 4,
        corruption_detected: true
      }
      mockInvoke.mockResolvedValue(report)
      await checkCorruption('/tmp/test')
      expect(get(corruptionAlerts)).toHaveLength(1)
      expect(get(alertCnt)).toBe(1)
    })

    it('does not duplicate alerts', async () => {
      const report = {
        merkle_root: 'abc123',
        corrupted_chunks: [1],
        missing_chunks: [],
        verified_chunks: [0, 2, 3],
        total_chunks: 4,
        corruption_detected: true
      }
      mockInvoke.mockResolvedValue(report)
      await checkCorruption('/tmp/test')
      await checkCorruption('/tmp/test')
      expect(get(corruptionAlerts)).toHaveLength(1)
    })

    it('skips alert when no corruption', async () => {
      const report = {
        merkle_root: 'abc123',
        corrupted_chunks: [],
        missing_chunks: [],
        verified_chunks: [0, 1, 2, 3],
        total_chunks: 4,
        corruption_detected: false
      }
      mockInvoke.mockResolvedValue(report)
      await checkCorruption('/tmp/test')
      expect(get(corruptionAlerts)).toHaveLength(0)
    })
  })

  describe('createCoordinator', () => {
    it('updates active recoveries map', async () => {
      mockInvoke.mockResolvedValue(mockProgress)
      const res = await createCoordinator('/tmp/test')
      expect(res).toEqual(mockProgress)
      const map = get(activeRecoveries)
      expect(map.get('abc123')).toEqual(mockProgress)
      expect(get(activeRecoveryCnt)).toBe(1)
    })

    it('returns null on error', async () => {
      mockInvoke.mockRejectedValue(new Error('fail'))
      const res = await createCoordinator('/tmp/test')
      expect(res).toBeNull()
    })
  })

  describe('reportResult', () => {
    it('updates progress in store', async () => {
      mockInvoke.mockResolvedValue(mockProgress)
      const res = await reportResult('/tmp/test', 0, true, 262144, 100)
      expect(mockInvoke).toHaveBeenCalledWith('p2p_chunk_report_result', {
        tmpPath: '/tmp/test',
        chunkIdx: 0,
        success: true,
        bytes: 262144,
        durationMs: 100
      })
      expect(res).toEqual(mockProgress)
    })
  })

  describe('getProgress', () => {
    it('updates active recoveries', async () => {
      mockInvoke.mockResolvedValue(mockProgress)
      await getProgress('/tmp/test')
      expect(get(activeRecoveries).get('abc123')).toEqual(mockProgress)
    })
  })

  describe('assignPending', () => {
    it('returns chunk fetch requests', async () => {
      const reqs = [{ chunk_idx: 0, peer_id: 'peer1', offset: 0, size: 262144, hash: 'h0' }]
      mockInvoke.mockResolvedValue(reqs)
      const res = await assignPending('/tmp/test')
      expect(res).toEqual(reqs)
    })

    it('returns empty on error', async () => {
      mockInvoke.mockRejectedValue(new Error('fail'))
      const res = await assignPending('/tmp/test')
      expect(res).toEqual([])
    })
  })

  describe('removeState', () => {
    it('calls invoke', async () => {
      mockInvoke.mockResolvedValue(undefined)
      await removeState('/tmp/test')
      expect(mockInvoke).toHaveBeenCalledWith('p2p_chunk_remove', { tmpPath: '/tmp/test' })
    })
  })

  describe('computeMerkle', () => {
    it('returns root hash', async () => {
      mockInvoke.mockResolvedValue('deadbeef')
      const res = await computeMerkle(['h0', 'h1', 'h2'])
      expect(res).toBe('deadbeef')
    })
  })

  describe('verifyMerkle', () => {
    it('returns true for valid', async () => {
      mockInvoke.mockResolvedValue(true)
      const res = await verifyMerkle('root', ['h0', 'h1'])
      expect(res).toBe(true)
    })
  })

  describe('hashChunk', () => {
    it('returns hash string', async () => {
      mockInvoke.mockResolvedValue('chunkhashabc')
      const data = new Uint8Array([1, 2, 3])
      const res = await hashChunk(data)
      expect(res).toBe('chunkhashabc')
      expect(mockInvoke).toHaveBeenCalledWith('p2p_chunk_hash', { data: [1, 2, 3] })
    })
  })

  describe('startupRecovery', () => {
    it('scans and populates store', async () => {
      mockInvoke.mockResolvedValue([mockRecovery])
      const res = await startupRecovery('/dl')
      expect(res).toEqual([mockRecovery])
      expect(get(recoveryList)).toEqual([mockRecovery])
    })
  })

  describe('clearAlert', () => {
    it('removes specific alert', () => {
      corruptionAlerts.set([
        { merkle_root: 'a', corrupted_chunks: [], missing_chunks: [], verified_chunks: [], total_chunks: 0, corruption_detected: true },
        { merkle_root: 'b', corrupted_chunks: [], missing_chunks: [], verified_chunks: [], total_chunks: 0, corruption_detected: true }
      ])
      clearAlert('a')
      const alerts = get(corruptionAlerts)
      expect(alerts).toHaveLength(1)
      expect(alerts[0].merkle_root).toBe('b')
    })
  })

  describe('clearAllAlerts', () => {
    it('clears all', () => {
      corruptionAlerts.set([
        { merkle_root: 'a', corrupted_chunks: [], missing_chunks: [], verified_chunks: [], total_chunks: 0, corruption_detected: true },
        { merkle_root: 'b', corrupted_chunks: [], missing_chunks: [], verified_chunks: [], total_chunks: 0, corruption_detected: true }
      ])
      clearAllAlerts()
      expect(get(corruptionAlerts)).toHaveLength(0)
      expect(get(alertCnt)).toBe(0)
    })
  })
})
