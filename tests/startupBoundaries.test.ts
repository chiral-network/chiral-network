import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';

// application-startup-boundaries task 3.2 (requirement 2): the desktop's start is inert. The
// Tauri bridge modules are test-only stand-ins that record every call crossing them and answer
// only the version status; the real `$lib/native` runs on top of them, so a page's native
// operation would be recorded as `consumer_command:<operation>`. Network APIs are recorded too.
const bridge = vi.hoisted(() => ({ calls: [] as string[], network: [] as string[] }));
vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  Channel: class { onmessage: unknown = null; constructor() { bridge.calls.push('Channel'); } },
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    bridge.calls.push(command === 'consumer_command' ? `consumer_command:${String(args?.operation)}` : command);
    if (command === 'get_version_status') {
      return { currentVersion: '2.0.0', status: 'ok', policy: { minRequired: '0.0.0', recommended: '0.0.0', downloadUrl: '', issuedAt: 0, validUntil: 0, signature: '' } };
    }
    throw new Error(`unexpected native command ${command}`);
  }),
}));
vi.mock('@tauri-apps/api/window', () => ({
  // Any method of the window is recorded by name.
  getCurrentWindow: () => new Proxy({}, {
    get: (_target, name) => name === 'then' ? undefined : (..._args: unknown[]) => { bridge.calls.push(`window.${String(name)}`); return Promise.resolve(() => {}); },
  }),
}));
vi.mock('@tauri-apps/api/event', () => {
  const record = (kind: string) => vi.fn(async (event: string) => { bridge.calls.push(`${kind}:${event}`); return () => {}; });
  return { listen: record('listen'), once: record('once'), emit: record('emit'), emitTo: record('emitTo') };
});

import App from '../src/App.svelte';
import { navItems } from '$lib/navigation';
import { session } from '$lib/stores';
import { stopVersionStore, resetVersionStatus } from '$lib/stores/versionStore';

async function settle() { for (let i = 0; i < 10; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
const originals = { fetch: globalThis.fetch, open: XMLHttpRequest.prototype.open, WebSocket: globalThis.WebSocket, EventSource: (globalThis as { EventSource?: unknown }).EventSource, scrollTo: window.scrollTo };
beforeEach(() => {
  bridge.calls = []; bridge.network = [];
  // jsdom has no scrolling; routing scrolls to the top.
  window.scrollTo = (() => {}) as typeof window.scrollTo;
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(Element.prototype, 'animate', { configurable: true, writable: true,
    value: () => ({ finished: Promise.resolve(), cancel() {}, onfinish: null, currentTime: 0 }) });
  globalThis.fetch = vi.fn(async (input: unknown) => { bridge.network.push(`fetch ${String(input)}`); throw new Error('no network'); }) as typeof fetch;
  XMLHttpRequest.prototype.open = function (method: string, url: string | URL) { bridge.network.push(`xhr ${method} ${String(url)}`); } as typeof XMLHttpRequest.prototype.open;
  globalThis.WebSocket = class { constructor(url: string) { bridge.network.push(`websocket ${url}`); throw new Error('no network'); } } as unknown as typeof WebSocket;
  (globalThis as { EventSource?: unknown }).EventSource = class { constructor(url: string) { bridge.network.push(`eventsource ${url}`); throw new Error('no network'); } };
  session.set({ address: null, configured: false });
  target = document.createElement('div'); document.body.append(target);
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  stopVersionStore(); resetVersionStatus();
  globalThis.fetch = originals.fetch; XMLHttpRequest.prototype.open = originals.open; globalThis.WebSocket = originals.WebSocket;
  (globalThis as { EventSource?: unknown }).EventSource = originals.EventSource;
  window.scrollTo = originals.scrollTo;
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
  window.location.hash = '';
});

describe('the desktop start is inert before a profile is loaded (application-startup-boundaries requirement 2)', () => {
  it('reads only the version status and subscribes only to the close and quit events, on every page', async () => {
    expect(navItems.map(item => item.key)).toHaveLength(9);
    for (const { key } of navItems) {
      bridge.calls = [];
      window.location.hash = `#/${key}`;
      mounted = mount(App, { target }); await settle();
      // The page rendered (its heading or main region), so its mount-time effects ran.
      expect(document.getElementById('main')?.textContent?.trim().length, key).toBeGreaterThan(0);
      expect([...bridge.calls].sort(), key).toEqual(['get_version_status', 'listen:chiral://quit-requested', 'window.onCloseRequested']);
      expect(bridge.network, key).toEqual([]);
      await unmount(mounted); mounted = undefined; target.replaceChildren();
      stopVersionStore();
    }
  });

  it('stays inert for hours on every page: only the version status is read again, on its hourly poll', async () => {
    // Timers are simulated, so a poll or a delayed start that the settling above would not
    // reach is run too; the repeated version status shows that the clock did advance.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
    try {
      for (const { key } of navItems) {
        bridge.calls = [];
        window.location.hash = `#/${key}`;
        mounted = mount(App, { target });
        await vi.advanceTimersByTimeAsync(3 * 60 * 60 * 1000 + 1000);
        const calls = [...bridge.calls];
        expect(calls.filter(call => call !== 'get_version_status').sort(), key).toEqual(['listen:chiral://quit-requested', 'window.onCloseRequested']);
        expect(calls.filter(call => call === 'get_version_status').length, key).toBeGreaterThanOrEqual(2);
        expect(bridge.network, key).toEqual([]);
        await unmount(mounted); mounted = undefined; target.replaceChildren();
        stopVersionStore();
      }
    } finally {
      vi.useRealTimers();
    }
  });

  it('records a native operation that a page starts, so the observation is not blind', async () => {
    // The control: a loaded profile lets the Usage page read its accounts on mount.
    session.set({ address: '0x' + '2'.repeat(40), configured: true });
    window.location.hash = '#/usage';
    mounted = mount(App, { target }); await settle();
    expect(bridge.calls).toContain('consumer_command:accounts');
  });
});
