import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

import { saveSeedList, loadSeedList, clearSeedList, type SeedRecord } from '../src/lib/services/seedPersistence';

// Simple in-memory localStorage mock
function createLocalStorageMock() {
  const store: Record<string, string> = {};
  return {
    getItem(key: string) {
      return store[key] ?? null;
    },
    setItem(key: string, value: string) {
      store[key] = value;
    },
    removeItem(key: string) {
      delete store[key];
    },
    clear() {
      for (const k of Object.keys(store)) delete store[k];
    },
  };
}

describe('seedPersistence', () => {
  const sampleSeeds: SeedRecord[] = [
    { id: '1', path: '/tmp/a', hash: 'h1', name: 'a', size: 123 },
  ];

  beforeEach(() => {
    // Reset any global invoke
    delete (globalThis as any).__tauri_invoke;
    delete (globalThis as any).invoke;
    (globalThis as any).localStorage = createLocalStorageMock();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    delete (globalThis as any).localStorage;
  });

  it('falls back to localStorage when tauri invoke not available and save/load work', async () => {
    await saveSeedList(sampleSeeds);
    const loaded = await loadSeedList();
    expect(loaded).toEqual(sampleSeeds);
  });

  it('uses tauri invoke when available for saveSeedList', async () => {
    const mockInvoke = vi.fn(async (cmd: string, args?: any) => {
      // emulate successful write
      return true;
    });
    (globalThis as any).__tauri_invoke = mockInvoke;

    await saveSeedList(sampleSeeds);
    // the first argument to the invoke call should be the command name
    expect(mockInvoke).toHaveBeenCalled();
    expect(mockInvoke.mock.calls[0][0]).toBe('write_seed_list');
  });

  it('loadSeedList reads from tauri invoke when it returns string payload', async () => {
    const payload = JSON.stringify({ version: 2, seeds: sampleSeeds });
    const mockInvoke = vi.fn(async (cmd: string) => payload);
    (globalThis as any).invoke = mockInvoke;

    const loaded = await loadSeedList();
    expect(mockInvoke).toHaveBeenCalled();
    expect(mockInvoke.mock.calls[0][0]).toBe('read_seed_list');
    expect(loaded).toEqual(sampleSeeds);
  });

  it('clearSeedList clears localStorage and calls tauri clear when available', async () => {
    const mockInvoke = vi.fn(async (cmd: string) => true);
    (globalThis as any).invoke = mockInvoke;
    (globalThis as any).localStorage.setItem('chiral.seeds.v1', 'x');

    await clearSeedList();
    expect(mockInvoke).toHaveBeenCalled();
    expect(mockInvoke.mock.calls[0][0]).toBe('clear_seed_list');
    expect((globalThis as any).localStorage.getItem('chiral.seeds.v1')).toBeNull();
  });
});
