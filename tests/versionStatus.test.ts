import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { mount, tick, unmount } from 'svelte';

// Test-only stand-ins for the Tauri core bridge (get_version_status), the native
// consumer boundary and the window/event modules (client-version-policy requirement 13).
const bridge = vi.hoisted(() => ({ status: null as unknown, fail: false }));
vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  Channel: class {},
  invoke: vi.fn(async (command: string) => {
    if (command !== 'get_version_status') throw new Error(`unexpected invoke ${command}`);
    if (bridge.fail) throw new Error('version status unavailable');
    return bridge.status;
  }),
}));
vi.mock('$lib/native', async () => {
  const actual = await vi.importActual<typeof import('$lib/native')>('$lib/native');
  return {
    ...actual,
    nativeAvailable: () => true,
    command: vi.fn(async (operation: string) => {
      if (operation === 'configure') return { address: '0x' + '2'.repeat(40) };
      if (operation === 'logout') return null;
      throw new Error(`unexpected native operation ${operation}`);
    }),
  };
});
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

import App from '../src/App.svelte';
import { refreshVersionStatus, resetVersionStatus, stopVersionStore, versionStatus } from '$lib/stores/versionStore';
import { session } from '$lib/stores';

const recommended = { currentVersion: '1.0.0', status: 'recommended', policy: { minRequired: '0.0.0', recommended: '9.0.0', downloadUrl: 'https://example.org/releases', issuedAt: 1, validUntil: 0, signature: 'aa' } };
const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
async function settle() { for (let i = 0; i < 8; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeEach(async () => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(Element.prototype, 'animate', { configurable: true, writable: true,
    value: () => ({ finished: Promise.resolve(), cancel() {}, onfinish: null, currentTime: 0 }) });
  bridge.status = recommended; bridge.fail = false;
  await refreshVersionStatus();
  target = document.createElement('div'); document.body.append(target);
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  stopVersionStore();
  resetVersionStatus();
  session.set({ address: null, configured: false });
  window.location.hash = '';
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
});

describe('no status from another network stays visible (book A.1.8, A.2)', () => {
  it('a reset drops the status, and a failed refresh then leaves the neutral default', async () => {
    expect(get(versionStatus).status).toBe('recommended');
    resetVersionStatus();
    expect(get(versionStatus).status).toBe('ok');
    expect(get(versionStatus).policy.recommended).toBe('0.0.0');
    bridge.fail = true;
    await refreshVersionStatus();
    expect(get(versionStatus).status).toBe('ok');
  });

  it('logging out removes the previous network\'s update banner', async () => {
    session.set({ address: '0x' + '2'.repeat(40), configured: true });
    mounted = mount(App, { target }); await settle();
    expect([...document.querySelectorAll('[role="status"]')].some(node => /Update available/.test(text(node)))).toBe(true);
    [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => /Log out/.test(text(button)) || /Log out/.test(button.getAttribute('aria-label') ?? ''))!.click();
    await settle();
    expect(get(versionStatus).status).toBe('ok');
    expect([...document.querySelectorAll('[role="status"]')].some(node => /Update available/.test(text(node)))).toBe(false);
  });

  it('a profile load whose status refresh fails shows no previous status', async () => {
    window.location.hash = '#/settings';
    mounted = mount(App, { target }); await settle();
    expect(get(versionStatus).status).toBe('recommended');
    bridge.fail = true;
    const path = [...target.querySelectorAll<HTMLInputElement>('input')].find(input => /configuration file path/i.test(text(input.closest('label'))))!;
    path.value = '/tmp/other-network.json'; path.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    path.closest('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(get(versionStatus).status).toBe('ok');
    expect([...document.querySelectorAll('[role="status"]')].some(node => /Update available/.test(text(node)))).toBe(false);
  });
});
