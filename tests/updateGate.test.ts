import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';

// Test-only stand-ins: a controllable version status, the native boundary and the Tauri
// window/event modules, so the real shell can render the update gate (book A.2).
const harness = vi.hoisted(() => ({ quitListener: undefined as undefined | (() => void) }));
vi.mock('$lib/stores/versionStore', async () => {
  const { writable } = await import('svelte/store');
  const versionStatus = writable({ currentVersion: '1.0.0', status: 'ok', policy: { minRequired: '0.0.0', recommended: '0.0.0', downloadUrl: '', issuedAt: 0, validUntil: 0, signature: '' } });
  return { versionStatus, initVersionStore: () => {}, stopVersionStore: () => {}, refreshVersionStatus: async () => {} };
});
vi.mock('$lib/native', async () => {
  const actual = await vi.importActual<typeof import('$lib/native')>('$lib/native');
  return { ...actual, nativeAvailable: () => true, command: vi.fn(async (operation: string) => { throw new Error(`unexpected native operation ${operation}`); }) };
});
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: async (_event: string, callback: () => void) => { harness.quitListener = callback; return () => {}; } }));

import App from '../src/App.svelte';
import UpdateGate from '../src/lib/components/UpdateGate.svelte';
import { versionStatus } from '$lib/stores/versionStore';

type Status = 'ok' | 'recommended' | 'required';
const setStatus = (status: Status, downloadUrl: string) => (versionStatus as unknown as { set: (value: unknown) => void }).set({
  currentVersion: '1.0.0', status,
  policy: { minRequired: status === 'required' ? '2.0.0' : '0.0.0', recommended: '2.0.0', downloadUrl, message: 'Policy message', issuedAt: 1, validUntil: 0, signature: '' },
});
const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
const button = (pattern: RegExp) => [...document.querySelectorAll<HTMLButtonElement>('button')].find(b => pattern.test(text(b)));
// Svelte sets the `inert` property (browsers reflect it to the attribute; jsdom does not).
const inertAncestor = (element: Element | null | undefined) => {
  for (let node = element as HTMLElement | null | undefined; node; node = node.parentElement) if ((node as HTMLElement & { inert?: boolean }).inert) return node;
  return null;
};
async function settle() { for (let i = 0; i < 8; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
let writeText: ReturnType<typeof vi.fn>;
beforeEach(() => {
  Object.defineProperty(Element.prototype, 'animate', { configurable: true, writable: true,
    value: () => ({ finished: Promise.resolve(), cancel() {}, onfinish: null, currentTime: 0 }) });
  // jsdom has no modal dialogs; a browser puts showModal() dialogs in the top layer.
  if (!HTMLDialogElement.prototype.showModal) {
    HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) { this.setAttribute('open', ''); };
    HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) { this.removeAttribute('open'); };
  }
  writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  sessionStorage.clear();
  target = document.createElement('div'); document.body.append(target);
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  setStatus('ok', '');
});

describe('download URLs are offered only over HTTPS (book A.2)', () => {
  it('copies an HTTPS URL exactly from the modal and the banner', async () => {
    setStatus('required', 'https://example.org/releases');
    mounted = mount(UpdateGate, { target }); await settle();
    button(/Copy download URL/)!.click(); await settle();
    expect(writeText).toHaveBeenCalledWith('https://example.org/releases');
    setStatus('recommended', 'https://example.org/releases'); await settle();
    button(/^Update$/)!.click(); await settle();
    expect(writeText).toHaveBeenLastCalledWith('https://example.org/releases');
  });

  for (const url of ['http://example.org/releases', 'https://user@example.org/releases', 'not a url']) {
    it(`offers no copy or Update action for ${url}`, async () => {
      setStatus('required', url);
      mounted = mount(UpdateGate, { target }); await settle();
      expect(button(/Copy download URL/)).toBeUndefined();
      expect(text(target)).toMatch(/no HTTPS download URL/);
      expect(text(target)).not.toContain(url);
      expect(target.querySelector('a[href]')).toBeNull();
      setStatus('recommended', url); await settle();
      expect(button(/^Update$/)).toBeUndefined();
      expect(writeText).not.toHaveBeenCalled();
    });
  }
});

describe('a required update blocks the shell for keyboard and assistive technology (book A.2, A.1.9)', () => {
  it('moves focus into the modal, keeps Tab there and makes the shell inert, while Quit stays usable', async () => {
    setStatus('required', 'https://example.org/releases');
    mounted = mount(App, { target }); await settle();
    const modal = document.querySelector<HTMLElement>('[role="alertdialog"]')!;
    expect(modal).not.toBeNull();
    expect(modal.contains(document.activeElement)).toBe(true);
    const copy = button(/Copy download URL/)!;
    copy.focus();
    copy.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    expect(modal.contains(document.activeElement)).toBe(true);
    copy.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true }));
    expect(modal.contains(document.activeElement)).toBe(true);
    for (const element of [document.getElementById('main'), document.querySelector('.skip-link'), [...document.querySelectorAll('nav')].find(nav => !modal.contains(nav))]) {
      expect(element, 'shell element present').toBeTruthy();
      expect(inertAncestor(element), element?.tagName).not.toBeNull();
    }
    // The tray's Quit request still opens its confirmation above the gate.
    harness.quitListener?.();
    await settle();
    const quit = document.querySelector<HTMLDialogElement>('dialog[aria-labelledby="quit-title"]')!;
    expect(quit.open).toBe(true);
    expect(inertAncestor(quit)).toBeNull();
    expect(inertAncestor(modal)).toBeNull();
  });

  it('leaves the shell usable when no update is required', async () => {
    setStatus('recommended', 'https://example.org/releases');
    mounted = mount(App, { target }); await settle();
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
    expect(inertAncestor(document.getElementById('main'))).toBeNull();
  });
});
