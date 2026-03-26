import { describe, it, expect, beforeEach, vi } from 'vitest';

// Mock logger
vi.mock('$lib/logger', () => ({
  logger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
    ok: vi.fn(),
  }),
}));

// Mock svelte5-router
vi.mock('@mateothegreat/svelte5-router', () => ({
  goto: vi.fn(),
}));

// Mock stores
vi.mock('$lib/stores', () => ({
  walletAccount: { subscribe: vi.fn() },
}));

// Mock toastStore
vi.mock('$lib/toastStore', () => ({
  toasts: {
    show: vi.fn(),
    detail: vi.fn(),
    notify: vi.fn(),
    notifyDetail: vi.fn(),
  },
}));

// ---------------------------------------------------------------------------
// Re-implement the helper functions from Mining.svelte so we can unit-test
// them without rendering the Svelte component.
// ---------------------------------------------------------------------------

const MIN_UTILIZATION_PERCENT = 10;
const MAX_UTILIZATION_PERCENT = 100;

function clampUtilizationPercent(value: number): number {
  if (!Number.isFinite(value)) return MAX_UTILIZATION_PERCENT;
  return Math.max(MIN_UTILIZATION_PERCENT, Math.min(MAX_UTILIZATION_PERCENT, Math.round(value)));
}

function formatHashRate(rate: number): string {
  if (rate >= 1e9) return `${(rate / 1e9).toFixed(2)} GH/s`;
  if (rate >= 1e6) return `${(rate / 1e6).toFixed(2)} MH/s`;
  if (rate >= 1e3) return `${(rate / 1e3).toFixed(2)} KH/s`;
  return `${rate} H/s`;
}

function computeMiningThreads(maxThreads: number, cpuUtilizationPercent: number): number {
  return Math.max(1, Math.min(maxThreads, Math.round((maxThreads * cpuUtilizationPercent) / 100)));
}

function computeActiveMiningBackend(
  gpuMiningStatus: { running: boolean } | null,
  miningStatus: { mining: boolean } | null
): 'gpu' | 'cpu' | 'none' {
  if (gpuMiningStatus?.running) return 'gpu';
  if (miningStatus?.mining) return 'cpu';
  return 'none';
}

function updateElapsed(miningStartTime: number | null, nowMs: number): string {
  if (miningStartTime === null) return '00:00:00';
  const diff = Math.floor((nowMs - miningStartTime) / 1000);
  const h = Math.floor(diff / 3600).toString().padStart(2, '0');
  const m = Math.floor((diff % 3600) / 60).toString().padStart(2, '0');
  const s = (diff % 60).toString().padStart(2, '0');
  return `${h}:${m}:${s}`;
}

function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('Mining page helpers', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  // -------------------------------------------------------------------------
  // formatHashRate
  // -------------------------------------------------------------------------
  describe('formatHashRate', () => {
    it('should return H/s for rates below 1000', () => {
      expect(formatHashRate(0)).toBe('0 H/s');
      expect(formatHashRate(1)).toBe('1 H/s');
      expect(formatHashRate(999)).toBe('999 H/s');
    });

    it('should return KH/s for rates from 1e3 to below 1e6', () => {
      expect(formatHashRate(1000)).toBe('1.00 KH/s');
      expect(formatHashRate(1500)).toBe('1.50 KH/s');
      expect(formatHashRate(999999)).toBe('1000.00 KH/s');
    });

    it('should return MH/s for rates from 1e6 to below 1e9', () => {
      expect(formatHashRate(1e6)).toBe('1.00 MH/s');
      expect(formatHashRate(5_500_000)).toBe('5.50 MH/s');
      expect(formatHashRate(999_999_999)).toBe('1000.00 MH/s');
    });

    it('should return GH/s for rates at or above 1e9', () => {
      expect(formatHashRate(1e9)).toBe('1.00 GH/s');
      expect(formatHashRate(2.5e9)).toBe('2.50 GH/s');
      expect(formatHashRate(1e12)).toBe('1000.00 GH/s');
    });

    it('should handle exact threshold boundaries', () => {
      expect(formatHashRate(1e3)).toBe('1.00 KH/s');
      expect(formatHashRate(1e6)).toBe('1.00 MH/s');
      expect(formatHashRate(1e9)).toBe('1.00 GH/s');
    });
  });

  // -------------------------------------------------------------------------
  // clampUtilizationPercent
  // -------------------------------------------------------------------------
  describe('clampUtilizationPercent', () => {
    it('should clamp values below MIN to MIN (10%)', () => {
      expect(clampUtilizationPercent(0)).toBe(10);
      expect(clampUtilizationPercent(5)).toBe(10);
      expect(clampUtilizationPercent(-100)).toBe(10);
      expect(clampUtilizationPercent(9)).toBe(10);
    });

    it('should clamp values above MAX to MAX (100%)', () => {
      expect(clampUtilizationPercent(101)).toBe(100);
      expect(clampUtilizationPercent(200)).toBe(100);
      expect(clampUtilizationPercent(999)).toBe(100);
    });

    it('should return MAX for NaN', () => {
      expect(clampUtilizationPercent(NaN)).toBe(100);
    });

    it('should return MAX for Infinity', () => {
      expect(clampUtilizationPercent(Infinity)).toBe(100);
      expect(clampUtilizationPercent(-Infinity)).toBe(100);
    });

    it('should round fractional values', () => {
      expect(clampUtilizationPercent(50.4)).toBe(50);
      expect(clampUtilizationPercent(50.5)).toBe(51);
      expect(clampUtilizationPercent(10.1)).toBe(10);
    });

    it('should pass through valid integer values unchanged', () => {
      expect(clampUtilizationPercent(10)).toBe(10);
      expect(clampUtilizationPercent(50)).toBe(50);
      expect(clampUtilizationPercent(75)).toBe(75);
      expect(clampUtilizationPercent(100)).toBe(100);
    });
  });

  // -------------------------------------------------------------------------
  // Mining thread calculation
  // -------------------------------------------------------------------------
  describe('computeMiningThreads', () => {
    it('should compute threads from utilization and hardware threads', () => {
      // 50% of 8 threads = 4
      expect(computeMiningThreads(8, 50)).toBe(4);
      // 100% of 8 threads = 8
      expect(computeMiningThreads(8, 100)).toBe(8);
      // 25% of 4 threads = 1
      expect(computeMiningThreads(4, 25)).toBe(1);
    });

    it('should always return at least 1 thread', () => {
      expect(computeMiningThreads(8, 0)).toBe(1);
      expect(computeMiningThreads(8, 1)).toBe(1);
      expect(computeMiningThreads(1, 10)).toBe(1);
    });

    it('should never exceed maxThreads', () => {
      expect(computeMiningThreads(4, 100)).toBe(4);
      expect(computeMiningThreads(4, 150)).toBe(4);
    });

    it('should round to nearest integer', () => {
      // 33% of 8 = 2.64 -> rounds to 3
      expect(computeMiningThreads(8, 33)).toBe(3);
      // 12% of 8 = 0.96 -> rounds to 1, clamped to 1
      expect(computeMiningThreads(8, 12)).toBe(1);
    });

    it('should handle edge case of 1 max thread', () => {
      expect(computeMiningThreads(1, 50)).toBe(1);
      expect(computeMiningThreads(1, 100)).toBe(1);
    });
  });

  // -------------------------------------------------------------------------
  // Mining mode persistence (localStorage)
  // -------------------------------------------------------------------------
  describe('mining mode persistence', () => {
    it('should read saved CPU utilization from localStorage', () => {
      localStorage.setItem('chiral-cpu-utilization-percent', '75');
      const raw = localStorage.getItem('chiral-cpu-utilization-percent');
      expect(clampUtilizationPercent(parseInt(raw!, 10))).toBe(75);
    });

    it('should read saved GPU utilization from localStorage', () => {
      localStorage.setItem('chiral-gpu-utilization-percent', '60');
      const raw = localStorage.getItem('chiral-gpu-utilization-percent');
      expect(clampUtilizationPercent(parseInt(raw!, 10))).toBe(60);
    });

    it('should read saved mining mode from localStorage', () => {
      localStorage.setItem('chiral-mining-mode', 'gpu');
      const saved = localStorage.getItem('chiral-mining-mode');
      const mode = saved === 'gpu' ? 'gpu' : 'cpu';
      expect(mode).toBe('gpu');
    });

    it('should default to cpu mode when no saved mode', () => {
      const saved = localStorage.getItem('chiral-mining-mode');
      const mode = saved === 'gpu' ? 'gpu' : 'cpu';
      expect(mode).toBe('cpu');
    });

    it('should read saved threads from localStorage', () => {
      localStorage.setItem('chiral-mining-threads', '4');
      const saved = localStorage.getItem('chiral-mining-threads');
      expect(parseInt(saved!, 10)).toBe(4);
    });

    it('should parse saved GPU devices from localStorage', () => {
      localStorage.setItem('chiral-gpu-devices', JSON.stringify(['0', '1']));
      const raw = localStorage.getItem('chiral-gpu-devices');
      const parsed = JSON.parse(raw!);
      expect(Array.isArray(parsed)).toBe(true);
      const filtered = parsed.filter((v: unknown): v is string => typeof v === 'string');
      expect(filtered).toEqual(['0', '1']);
    });

    it('should handle invalid GPU devices JSON gracefully', () => {
      localStorage.setItem('chiral-gpu-devices', 'not-json');
      const raw = localStorage.getItem('chiral-gpu-devices');
      let devices: string[] = [];
      try {
        const parsed = JSON.parse(raw!);
        if (Array.isArray(parsed)) {
          devices = parsed.filter((v: unknown): v is string => typeof v === 'string');
        }
      } catch {
        devices = [];
      }
      expect(devices).toEqual([]);
    });

    it('should handle non-array GPU devices JSON gracefully', () => {
      localStorage.setItem('chiral-gpu-devices', JSON.stringify({ id: '0' }));
      const raw = localStorage.getItem('chiral-gpu-devices');
      let devices: string[] = [];
      try {
        const parsed = JSON.parse(raw!);
        if (Array.isArray(parsed)) {
          devices = parsed.filter((v: unknown): v is string => typeof v === 'string');
        }
      } catch {
        devices = [];
      }
      expect(devices).toEqual([]);
    });

    it('should filter non-string entries from GPU devices', () => {
      localStorage.setItem('chiral-gpu-devices', JSON.stringify(['0', 42, null, '1']));
      const raw = localStorage.getItem('chiral-gpu-devices');
      const parsed = JSON.parse(raw!);
      const filtered = parsed.filter((v: unknown): v is string => typeof v === 'string');
      expect(filtered).toEqual(['0', '1']);
    });

    it('should compute initial CPU utilization from saved threads when no utilization saved', () => {
      // Mimics the component logic: if savedCpuUtilizationRaw is null but savedThreads exists
      const hardwareThreads = 8;
      const savedThreads = '4';
      const parsedThreads = Math.max(1, Math.min(parseInt(savedThreads, 10) || 1, hardwareThreads));
      const result = clampUtilizationPercent((parsedThreads / hardwareThreads) * 100);
      expect(result).toBe(50);
    });

    it('should persist utilization values to localStorage', () => {
      const cpuUtil = 65;
      const gpuUtil = 80;
      localStorage.setItem('chiral-cpu-utilization-percent', cpuUtil.toString());
      localStorage.setItem('chiral-gpu-utilization-percent', gpuUtil.toString());
      expect(localStorage.getItem('chiral-cpu-utilization-percent')).toBe('65');
      expect(localStorage.getItem('chiral-gpu-utilization-percent')).toBe('80');
    });
  });

  // -------------------------------------------------------------------------
  // Active mining backend detection
  // -------------------------------------------------------------------------
  describe('computeActiveMiningBackend', () => {
    it('should return gpu when GPU mining is running', () => {
      expect(computeActiveMiningBackend({ running: true }, { mining: true })).toBe('gpu');
      expect(computeActiveMiningBackend({ running: true }, { mining: false })).toBe('gpu');
      expect(computeActiveMiningBackend({ running: true }, null)).toBe('gpu');
    });

    it('should return cpu when CPU mining is active and GPU is not', () => {
      expect(computeActiveMiningBackend({ running: false }, { mining: true })).toBe('cpu');
      expect(computeActiveMiningBackend(null, { mining: true })).toBe('cpu');
    });

    it('should return none when nothing is mining', () => {
      expect(computeActiveMiningBackend({ running: false }, { mining: false })).toBe('none');
      expect(computeActiveMiningBackend(null, null)).toBe('none');
      expect(computeActiveMiningBackend(null, { mining: false })).toBe('none');
      expect(computeActiveMiningBackend({ running: false }, null)).toBe('none');
    });

    it('should prioritize GPU over CPU when both are active', () => {
      expect(computeActiveMiningBackend({ running: true }, { mining: true })).toBe('gpu');
    });
  });

  // -------------------------------------------------------------------------
  // Mining elapsed time formatting
  // -------------------------------------------------------------------------
  describe('updateElapsed (mining elapsed time)', () => {
    it('should return 00:00:00 when start time is null', () => {
      expect(updateElapsed(null, Date.now())).toBe('00:00:00');
    });

    it('should format seconds correctly', () => {
      const start = 1000000;
      expect(updateElapsed(start, start + 5000)).toBe('00:00:05');
      expect(updateElapsed(start, start + 59000)).toBe('00:00:59');
    });

    it('should format minutes correctly', () => {
      const start = 1000000;
      expect(updateElapsed(start, start + 60000)).toBe('00:01:00');
      expect(updateElapsed(start, start + 90000)).toBe('00:01:30');
      expect(updateElapsed(start, start + 3599000)).toBe('00:59:59');
    });

    it('should format hours correctly', () => {
      const start = 1000000;
      expect(updateElapsed(start, start + 3600000)).toBe('01:00:00');
      expect(updateElapsed(start, start + 7200000)).toBe('02:00:00');
      expect(updateElapsed(start, start + 86400000)).toBe('24:00:00');
    });

    it('should format combined hours, minutes, seconds', () => {
      const start = 1000000;
      // 1h 23m 45s = 5025s
      expect(updateElapsed(start, start + 5025000)).toBe('01:23:45');
      // 12h 34m 56s
      expect(updateElapsed(start, start + 45296000)).toBe('12:34:56');
    });

    it('should pad single digits with leading zeros', () => {
      const start = 1000000;
      expect(updateElapsed(start, start + 1000)).toBe('00:00:01');
      expect(updateElapsed(start, start + 61000)).toBe('00:01:01');
    });

    it('should handle zero elapsed time', () => {
      const start = 1000000;
      expect(updateElapsed(start, start)).toBe('00:00:00');
    });
  });

  // -------------------------------------------------------------------------
  // isTauri detection
  // -------------------------------------------------------------------------
  describe('isTauri', () => {
    it('should return false when __TAURI_INTERNALS__ is not on window', () => {
      delete (window as any).__TAURI_INTERNALS__;
      expect(isTauri()).toBe(false);
    });

    it('should return true when __TAURI_INTERNALS__ is on window', () => {
      (window as any).__TAURI_INTERNALS__ = {};
      expect(isTauri()).toBe(true);
      delete (window as any).__TAURI_INTERNALS__;
    });

    it('should return true even if __TAURI_INTERNALS__ is falsy but present', () => {
      // 'in' operator returns true even for null/undefined values assigned to key
      (window as any).__TAURI_INTERNALS__ = null;
      expect(isTauri()).toBe(true);
      delete (window as any).__TAURI_INTERNALS__;
    });
  });
});
