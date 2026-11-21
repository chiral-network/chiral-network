import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';

// application-startup-boundaries task 3.3 (requirement 3, scenarios "A legacy wallet is not
// loaded automatically" and "An explicit import converts no balance") and task 3.4
// (requirement 4, scenario "A Core payout is not an eSpace balance"), on the desktop Wallet page.
// Test-only stand-in for the native boundary: it records each operation with its arguments and
// answers an import with the key's address and a balance read with an eSpace balance of 0 on
// chain 71, which is what the native core returns (application_startup_boundaries_test.rs).
const native = vi.hoisted(() => ({ calls: [] as { operation: string; args: Record<string, unknown> }[] }));
vi.mock('$lib/native', async () => {
  const actual = await vi.importActual<typeof import('$lib/native')>('$lib/native');
  return {
    ...actual,
    nativeAvailable: () => true,
    command: vi.fn(async (operation: string, args: Record<string, unknown> = {}) => {
      native.calls.push({ operation, args });
      if (operation === 'wallet_import') return '0x' + '4'.repeat(40);
      if (operation === 'balance') return { address: '0x' + '4'.repeat(40), balance_wei: '0', network: { chain_id: 71, genesis_hash: '0x' + '3f'.repeat(32), deployment_id: 'test' } };
      throw new Error(`unexpected native operation ${operation}`);
    }),
  };
});

import Wallet from '../src/pages/Wallet.svelte';
import { session } from '$lib/stores';

const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
async function settle() { for (let i = 0; i < 6; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }
const buttons = (root: ParentNode) => [...root.querySelectorAll<HTMLButtonElement>('button')];

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeEach(() => {
  native.calls = [];
  session.set({ address: null, configured: true });
  target = document.createElement('div'); document.body.append(target);
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  session.set({ address: null, configured: false });
});

describe('a profile without a wallet in its namespace (application-startup-boundaries requirement 3)', () => {
  it('loads no wallet and offers only generating one or importing a key file by its path, with no CHI or CHR balance', async () => {
    mounted = mount(Wallet, { target }); await settle();
    expect(native.calls).toEqual([]);
    // The wallet actions: generate, or import a file named by its path. No key, seed or
    // keystore text is ever typed into the page.
    const actions = buttons(target).map(text).filter(label => /wallet|import|key/i.test(label));
    expect(actions).toEqual(['Generate native wallet', 'Import key file']);
    expect(target.querySelectorAll('textarea, input[type="password"], input[type="file"]')).toHaveLength(0);
    const inputs = [...target.querySelectorAll('input')];
    expect(inputs.map(input => text(input.closest('label')))).toEqual(['Protected private-key file path']);
    // No balance is shown, and CHI or CHR appear only in the statement that none is converted.
    expect(text(target)).not.toMatch(/Verified-network balance|\d\s*(CFX|CHI|CHR)\b/);
    const mentions = [...target.querySelectorAll('p, span, td, dd, li, div')].filter(element => element.children.length === 0 && /\b(CHI|CHR)\b/.test(text(element))).map(text);
    expect(mentions).toEqual(['Explicit key import does not convert old CHI or CHR balances into CFX.']);
  });

  it('imports only the named file and shows the eSpace balance the native core reads, not a legacy or Core balance', async () => {
    mounted = mount(Wallet, { target }); await settle();
    const path = target.querySelector<HTMLInputElement>('input')!;
    path.value = '/legacy/chiral-network/wallet.key'; path.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    path.closest('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(native.calls).toEqual([{ operation: 'wallet_import', args: { path: '/legacy/chiral-network/wallet.key' } }]);
    buttons(target).find(button => /Refresh verified-network balance/.test(text(button)) || /Refresh verified-network balance/.test(button.getAttribute('aria-label') ?? ''))!.click();
    await settle();
    expect(native.calls.map(call => call.operation)).toEqual(['wallet_import', 'balance']);
    expect(native.calls[1].args).toEqual({});
    const shown = text(target);
    expect(shown).toMatch(/Verified-network balance 0\s*CFX/);
    expect(shown).not.toMatch(/\d\s*(CHI|CHR)\b/);
  });
});
