import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { mount, tick, unmount } from 'svelte';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

// application-startup-boundaries task 3.2 (requirement 2, scenario "A stored automatic mining
// preference is ignored"). Before any module of the application loads, the webview storage
// holds the earlier application's `chiral-settings` entry and preferences of this application
// with legacy runtime fields; every access to webview storage is then recorded (area, method,
// key and written value). The harness's own seeding and inspection pause the recording.
const storage = vi.hoisted(() => {
  const legacy = JSON.stringify({ autoStartMining: true });
  localStorage.setItem('chiral-settings', legacy);
  localStorage.setItem('chiral-inference-settings', JSON.stringify({ autoStartMining: true, downloadDirectory: '/tmp', theme: 'dark' }));
  const state = { legacy, paused: false, calls: [] as { area: string; method: string; key: string | null; value?: string }[] };
  const prototype = Storage.prototype as unknown as Record<string, (...args: unknown[]) => unknown>;
  for (const method of ['getItem', 'setItem', 'removeItem', 'key', 'clear']) {
    const original = prototype[method];
    prototype[method] = function (this: Storage, ...args: unknown[]) {
      if (!state.paused) state.calls.push({ area: this === window.localStorage ? 'local' : 'session', method, key: args.length ? String(args[0]) : null, value: method === 'setItem' ? String(args[1]) : undefined });
      return original.apply(this, args);
    };
  }
  const length = Object.getOwnPropertyDescriptor(Storage.prototype, 'length')!;
  Object.defineProperty(Storage.prototype, 'length', { configurable: true, get(this: Storage) {
    if (!state.paused) state.calls.push({ area: this === window.localStorage ? 'local' : 'session', method: 'length', key: null });
    return length.get!.call(this);
  } });
  return state;
});
const native = vi.hoisted(() => ({ operations: [] as string[] }));
vi.mock('$lib/native', async () => {
  const actual = await vi.importActual<typeof import('$lib/native')>('$lib/native');
  return {
    ...actual,
    nativeAvailable: () => true,
    command: vi.fn(async (operation: string) => {
      native.operations.push(operation);
      if (operation === 'configure') return { address: '0x' + '2'.repeat(40) };
      if (operation === 'logout') return null;
      throw new Error(`unexpected native operation ${operation}`);
    }),
  };
});
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

import App from '../src/App.svelte';
import { settings, session } from '$lib/stores';

const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
async function settle() { for (let i = 0; i < 8; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }
function paused<T>(read: () => T): T { storage.paused = true; try { return read(); } finally { storage.paused = false; } }

// What the modules did with webview storage while they loaded, before the harness's first
// `clear()` (tests/setup.ts) ran for the first test.
const atLoad = storage.calls.slice();

beforeEach(() => {
  // tests/setup.ts cleared storage; the earlier application's entry is put back for each test.
  paused(() => localStorage.setItem('chiral-settings', storage.legacy));
  storage.calls.length = 0;
  native.operations = [];
});

describe('a stored automatic mining preference is ignored (application-startup-boundaries requirement 2)', () => {
  it('keeps the theme and drops autoStartMining and downloadDirectory, without touching chiral-settings', () => {
    const local = atLoad.filter(call => call.area === 'local');
    expect(local.map(({ method, key }) => `${method} ${key}`)).toEqual(['getItem chiral-inference-settings', 'setItem chiral-inference-settings']);
    const inUse = get(settings);
    expect(inUse.theme).toBe('dark');
    expect(inUse).not.toHaveProperty('autoStartMining');
    expect(inUse).not.toHaveProperty('downloadDirectory');
    const persisted = JSON.parse(local[1].value!);
    expect(persisted).toEqual(inUse);
    expect(Object.keys(persisted).sort()).toEqual(['colorTheme', 'navStyle', 'notifications', 'reducedMotion', 'theme']);
  });

  it('neither reads, rewrites nor removes chiral-settings through start, profile load and logout, and starts no other native operation', async () => {
    Object.defineProperty(Element.prototype, 'animate', { configurable: true, writable: true,
      value: () => ({ finished: Promise.resolve(), cancel() {}, onfinish: null, currentTime: 0 }) });
    // jsdom has no scrolling; routing scrolls to the top.
    const scrollTo = window.scrollTo;
    window.scrollTo = (() => {}) as typeof window.scrollTo;
    session.set({ address: null, configured: false });
    window.location.hash = '#/settings';
    const target = document.createElement('div'); document.body.append(target);
    const mounted = mount(App, { target }); await settle();
    const path = [...target.querySelectorAll<HTMLInputElement>('input')].find(input => /profile|configuration|path/i.test(text(input.closest('label'))))!;
    path.value = '/tmp/consumer.json'; path.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    path.closest('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(get(session).configured).toBe(true);
    const logout = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.title === 'Close the native session')!;
    logout.click(); await settle();
    expect(get(session).configured).toBe(false);
    await unmount(mounted); target.remove(); window.location.hash = ''; window.scrollTo = scrollTo;

    expect(native.operations).toEqual(['configure', 'logout']);
    const touched = storage.calls.filter(call => call.area === 'local');
    expect(touched.filter(call => call.key === 'chiral-settings' || ['clear', 'key', 'length'].includes(call.method))).toEqual([]);
    expect(touched.filter(call => !['chiral-inference-settings', 'chiral-sidebar-collapsed'].includes(call.key ?? ''))).toEqual([]);
    expect(paused(() => localStorage.getItem('chiral-settings'))).toBe(storage.legacy);
    // The control: such a read is recorded.
    localStorage.getItem('chiral-settings');
    expect(storage.calls.at(-1)).toMatchObject({ area: 'local', method: 'getItem', key: 'chiral-settings' });
    expect(get(settings)).not.toHaveProperty('autoStartMining');
  });

  it('names no other webview storage key in the application source', () => {
    // The recording above covers the Storage methods; a property read such as
    // `localStorage['chiral-settings']` would bypass it, so the source is read too.
    const files: string[] = [];
    const walk = (directory: string) => { for (const name of readdirSync(directory)) { const path = join(directory, name); if (statSync(path).isDirectory()) walk(path); else files.push(path); } };
    walk('src');
    expect(files.length).toBeGreaterThan(30);
    const uses: string[] = [];
    for (const file of files) {
      const source = readFileSync(file, 'utf8');
      expect(source.includes('chiral-settings'), file).toBe(false);
      for (const match of source.matchAll(/\b(localStorage|sessionStorage)\b\s*(\.\s*\w+|\[)?/g)) uses.push(`${file}: ${match[1]}${(match[2] ?? '').replace(/\s/g, '')}`);
    }
    expect(uses.sort()).toEqual([
      'src/lib/components/Sidebar.svelte: localStorage.getItem',
      'src/lib/components/Sidebar.svelte: localStorage.setItem',
      // Existence checks: `window.sessionStorage` before each use.
      'src/lib/components/UpdateGate.svelte: sessionStorage',
      'src/lib/components/UpdateGate.svelte: sessionStorage',
      'src/lib/components/UpdateGate.svelte: sessionStorage.getItem',
      'src/lib/components/UpdateGate.svelte: sessionStorage.setItem',
      'src/lib/stores.ts: localStorage.getItem',
      'src/lib/stores.ts: localStorage.setItem',
    ]);
  });
});
