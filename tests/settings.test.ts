import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';

// Test-only stand-ins for the native boundary and the Tauri window/event modules, so the
// real application shell can be mounted (A.5.8 Settings).
const native = vi.hoisted(() => ({ calls: [] as { operation: string; args: Record<string, unknown> }[] }));
vi.mock('$lib/native', async () => {
  const actual = await vi.importActual<typeof import('$lib/native')>('$lib/native');
  return {
    ...actual,
    nativeAvailable: () => true,
    command: vi.fn(async (operation: string, args: Record<string, unknown> = {}) => {
      native.calls.push({ operation, args });
      if (operation === 'configure') return { address: '0x' + '2'.repeat(40) };
      throw new Error(`unexpected native operation ${operation}`);
    }),
  };
});
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

import App from '../src/App.svelte';
import Settings from '../src/pages/Settings.svelte';
import { session } from '$lib/stores';

const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
async function settle() { for (let i = 0; i < 8; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeEach(() => {
  native.calls = [];
  // jsdom has no Web Animations; notification transitions call element.animate.
  Object.defineProperty(Element.prototype, 'animate', { configurable: true, writable: true,
    value: () => ({ finished: Promise.resolve(), cancel() {}, onfinish: null, currentTime: 0 }) });
  session.set({ address: null, configured: false });
  target = document.createElement('div'); document.body.append(target);
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  session.set({ address: null, configured: false });
  window.location.hash = '';
});

const profileEntry = () => [...target.querySelectorAll<HTMLButtonElement>('nav[aria-label="Settings sections"] button')].find(b => text(b).startsWith('Network profile'))!;

describe('Settings profile indicator states the profile in text (A.1.9, A.5.8)', () => {
  it('names the profile state with and without a loaded profile, whichever section is open', async () => {
    mounted = mount(Settings, { target }); await settle();
    for (const section of ['Network profile', 'Appearance', 'Notifications']) {
      [...target.querySelectorAll<HTMLButtonElement>('nav[aria-label="Settings sections"] button')].find(b => text(b).startsWith(section))!.click();
      await settle();
      expect(text(profileEntry()), section).toMatch(/No profile loaded/);
    }
    session.set({ address: '0x' + '2'.repeat(40), configured: true }); await settle();
    expect(text(profileEntry())).toMatch(/Profile loaded/);
    expect(text(profileEntry())).not.toMatch(/No profile/);
  });
});

describe('a successful profile load is announced after the remount (A.5.8, book line 960)', () => {
  it('shows the confirmation in a status region once the session remounts the page', async () => {
    window.location.hash = '#/settings';
    mounted = mount(App, { target }); await settle();
    const path = [...target.querySelectorAll<HTMLInputElement>('input')].find(input => /profile|configuration|path/i.test(text(input.closest('label'))))!;
    path.value = '/tmp/consumer.json'; path.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    path.closest('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(native.calls.filter(call => call.operation === 'configure').map(call => call.args)).toEqual([{ path: '/tmp/consumer.json' }]);
    const confirmation = [...document.querySelectorAll('[role="status"]')].find(node => /Native configuration loaded/.test(text(node)));
    expect(confirmation).toBeDefined();
  });
});
