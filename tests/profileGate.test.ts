import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';

// Test-only stand-in for the native boundary: the runtime reports itself available, so only
// the missing profile (A.1.3) can keep these pages from calling native operations.
const native = vi.hoisted(() => ({ calls: [] as string[] }));
vi.mock('$lib/native', async () => {
  const actual = await vi.importActual<typeof import('$lib/native')>('$lib/native');
  return {
    ...actual,
    nativeAvailable: () => true,
    command: vi.fn(async (operation: string) => { native.calls.push(operation); throw new Error(`unexpected native operation ${operation}`); }),
    stream: vi.fn(async () => { native.calls.push('inference_stream'); throw new Error('unexpected stream'); }),
    cancelStream: vi.fn(async () => { native.calls.push('inference_cancel'); }),
  };
});

import Usage from '../src/pages/Usage.svelte';
import Playground from '../src/pages/Playground.svelte';
import Diagnostics from '../src/pages/Diagnostics.svelte';
import { session } from '$lib/stores';

const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
const button = (root: ParentNode, pattern: RegExp) =>
  [...root.querySelectorAll<HTMLButtonElement>('button')].find(b => pattern.test(text(b)) || pattern.test(b.getAttribute('aria-label') ?? ''));
async function settle() { for (let i = 0; i < 5; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeEach(() => {
  native.calls = [];
  session.set({ address: null, configured: false });
  target = document.createElement('div'); document.body.append(target);
});
afterEach(async () => { if (mounted) await unmount(mounted); mounted = undefined; target.remove(); });

describe('profile-dependent actions wait for an explicitly loaded profile (A.1.3)', () => {
  it('Usage reads nothing, disables its actions and points to Settings', async () => {
    mounted = mount(Usage, { target }); await settle();
    expect(native.calls).toEqual([]);
    expect(button(target, /^Refresh$/)!.disabled).toBe(true);
    expect(text(target)).not.toMatch(/No accounts in this network's local journal/);
    expect(target.querySelector('a[href="#/settings"]')).not.toBeNull();
    button(target, /Funding & recovery/)!.click(); await settle();
    const recovery = target.querySelector<HTMLInputElement>('input.font-mono')!;
    recovery.value = 'contract-under-test'; recovery.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    for (const label of [/Resume journaled funding/, /Inspect wallet recovery history/, /Submit or resume cancellation/]) {
      expect(button(target, label)!.disabled, String(label)).toBe(true);
    }
    expect(native.calls).toEqual([]);
  });

  it('Playground reads no accounts and disables Refresh accounts', async () => {
    mounted = mount(Playground, { target }); await settle();
    expect(native.calls).toEqual([]);
    expect(button(target, /Refresh accounts/)!.disabled).toBe(true);
  });

  it('Diagnostics keeps the runtime tile and disables collection', async () => {
    mounted = mount(Diagnostics, { target }); await settle();
    expect(text(target)).toMatch(/Native runtime Available/);
    const collect = button(target, /Collect native diagnostics/)!;
    expect(collect.disabled).toBe(true);
    collect.click(); await settle();
    expect(native.calls).toEqual([]);
  });

  it('a loaded profile enables the same actions', async () => {
    session.set({ address: '0x' + '2'.repeat(40), configured: true });
    mounted = mount(Diagnostics, { target }); await settle();
    expect(button(target, /Collect native diagnostics/)!.disabled).toBe(false);
    await unmount(mounted); target.replaceChildren();
    mounted = mount(Usage, { target }); await settle();
    expect(native.calls).toContain('accounts');
  });
});
