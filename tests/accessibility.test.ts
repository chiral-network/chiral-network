import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import type { Account } from '$lib/native';

// Test-only stand-in for the native boundary (A.1.9 accessibility checks).
const native = vi.hoisted(() => ({ handlers: {} as Record<string, (args: Record<string, unknown>) => unknown> }));
vi.mock('$lib/native', async () => {
  const actual = await vi.importActual<typeof import('$lib/native')>('$lib/native');
  return {
    ...actual,
    nativeAvailable: () => true,
    command: vi.fn(async (operation: string, args: Record<string, unknown> = {}) => {
      const handler = native.handlers[operation];
      if (!handler) throw new Error(`unexpected native operation ${operation}`);
      return handler(args);
    }),
  };
});

import Toast from '../src/lib/components/Toast.svelte';
import Usage from '../src/pages/Usage.svelte';
import Marketplace from '../src/pages/Marketplace.svelte';
import { session, settings } from '$lib/stores';
import { motionDuration, prefersReducedMotion, scrollBehavior } from '$lib/motion';

const provider = '0x' + '1'.repeat(40);
const saved: Account = {
  contract_id: 'contract-under-test', provider_wallet: provider, consumer_wallet: '0x' + '2'.repeat(40),
  network: { chain_id: 71, genesis_hash: '0x' + '3'.repeat(64), deployment_id: 'a11y-test' },
  model: { id: 'test/model', revision: 'a'.repeat(40), tokenizer_revision: 'b'.repeat(40), chat_template_sha256: 'c'.repeat(64), quantization: null, context_length: 4096, max_output_tokens: 256, capabilities: ['chat'], serving: null },
  price: { input_wei_per_1k: '1000', output_wei_per_1k: '2000' }, status: 'active', funded_wei: '1000', fee_wei: '5', spent_wei: '0',
  waived_wei: '0', reserved_wei: '0', available_wei: '995', reversed_wei: '0', provider_loss_wei: '0', usage: { prompt_tokens: 0, completion_tokens: 0 },
};
const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
async function settle() { for (let i = 0; i < 6; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeEach(() => {
  // jsdom has no Web Animations; Svelte transitions call element.animate for nonzero durations.
  Object.defineProperty(Element.prototype, 'animate', { configurable: true, writable: true,
    value: () => ({ finished: Promise.resolve(), cancel() {}, onfinish: null, currentTime: 0 }) });
  native.handlers = {};
  session.set({ address: saved.consumer_wallet, configured: true });
  settings.update(value => ({ ...value, reducedMotion: false }));
  target = document.createElement('div'); document.body.append(target);
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  session.set({ address: null, configured: false });
  settings.update(value => ({ ...value, reducedMotion: false }));
  vi.restoreAllMocks();
});

function openUsage(pending: unknown[] = []) {
  native.handlers = {
    accounts: () => [saved], pending_accounts: () => ({ pending }), account: () => saved,
    usage: () => ({}), credentials: () => ({}), funding: () => ({}), local_requests: () => ({}),
    agreement: () => ({ contract_id: saved.contract_id, provider_wallet: provider, endpoint: 'https://provider.test', peer_id: 'peer', model: 'test/model', fee_bps: 50 }),
  };
  mounted = mount(Usage, { target });
  return settle();
}

describe('live-region roles (A.1.9)', () => {
  it('announces failures and warnings as alerts, success and information as status', async () => {
    for (const [type, role] of [['error', 'alert'], ['warning', 'alert'], ['success', 'status'], ['info', 'status']] as const) {
      const toast = mount(Toast, { target, props: { message: `${type} message`, type, onClose: () => {} } });
      await settle();
      const element = [...target.querySelectorAll('[role]')].find(node => text(node).includes(`${type} message`));
      expect(element?.getAttribute('role'), type).toBe(role);
      await unmount(toast); target.replaceChildren();
    }
  });

  it('marks the Usage funding notices as status messages', async () => {
    await openUsage([{ contract_id: 'contract-under-test', topup_wei: '500000000000000000', funding_stage: 'topup_settlement_pending', tx_hash: null }]);
    const header = [...target.querySelectorAll('[role="status"]')].find(node => /1 journaled funding operation pending/.test(text(node)));
    expect(header).toBeDefined();
    const unresolved = [...target.querySelectorAll('[role="status"]')].find(node => /^Funding unresolved\./.test(text(node)));
    expect(unresolved).toBeDefined();
  });
});

describe('every Usage field has a visible associated label (A.1.9)', () => {
  it('labels every input, select and text area on every tab', async () => {
    await openUsage();
    const unlabelled: string[] = [];
    for (const tab of ['Overview', 'API keys', 'Requests', 'Funding & recovery', 'Feedback']) {
      [...target.querySelectorAll<HTMLButtonElement>('button')].find(button => text(button).startsWith(tab))!.click();
      await settle();
      for (const field of target.querySelectorAll<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>('input, select, textarea')) {
        if (field.getAttribute('aria-hidden') === 'true') continue;
        const named = [...(field.labels ?? [])].some(label => text(label).length > 0) || !!field.getAttribute('aria-label')?.trim();
        if (!named) unlabelled.push(`${tab}: <${field.tagName.toLowerCase()} type=${field.getAttribute('type')}>`);
      }
    }
    expect(unlabelled).toEqual([]);
  });
});

describe('reduced motion covers script-driven motion (A.1.9)', () => {
  it('combines the system and in-app preferences', () => {
    expect(prefersReducedMotion()).toBe(false);
    expect(motionDuration(250)).toBe(250);
    expect(scrollBehavior()).toBe('smooth');
    settings.update(value => ({ ...value, reducedMotion: true }));
    expect(motionDuration(250)).toBe(0);
    expect(scrollBehavior()).toBe('auto');
    settings.update(value => ({ ...value, reducedMotion: false }));
    vi.spyOn(window, 'matchMedia').mockImplementation(query => ({
      matches: query === '(prefers-reduced-motion: reduce)', media: query, onchange: null,
      addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
    }));
    expect(motionDuration(250)).toBe(0);
  });

  it('skips the notification animation when reduced motion is on', async () => {
    const animate = vi.fn(() => ({ finished: Promise.resolve(), cancel() {}, currentTime: 0, onfinish: null }));
    Object.defineProperty(Element.prototype, 'animate', { configurable: true, writable: true, value: animate });
    settings.update(value => ({ ...value, reducedMotion: true }));
    let toast = mount(Toast, { target, intro: true, props: { message: 'reduced', type: 'info', onClose: () => {} } });
    await settle();
    expect(animate).not.toHaveBeenCalled();
    await unmount(toast); target.replaceChildren();
    settings.update(value => ({ ...value, reducedMotion: false }));
    toast = mount(Toast, { target, intro: true, props: { message: 'animated', type: 'info', onClose: () => {} } });
    await settle();
    expect(animate).toHaveBeenCalled();
    // The animated path starts (Svelte calls animate only for a nonzero duration or delay).
    await unmount(toast);
  });

  it('scrolls to the funding panel instantly when reduced motion is on', async () => {
    const scroll = vi.fn();
    Object.defineProperty(Element.prototype, 'scrollIntoView', { configurable: true, writable: true, value: scroll });
    settings.update(value => ({ ...value, reducedMotion: true }));
    const offer = { payload: { network: saved.network, provider_wallet: provider, peer_id: 'peer', deployment_id: 'a11y-test', endpoint: 'https://provider.test', model: saved.model, price: saved.price, min_funding_wei: '1000', fee_bps: 50, expires_at: 4102444800, revision: 1 }, signature: '0x00' };
    native.handlers = { discover: () => [offer], provider_info: () => ({}), pending_accounts: () => ({ pending: [] }) };
    mounted = mount(Marketplace, { target }); await settle();
    [...target.querySelectorAll<HTMLButtonElement>('button')].find(button => /Discover providers/.test(text(button)))!.click(); await settle();
    [...target.querySelectorAll<HTMLButtonElement>('button')].find(button => /Inspect provider and funding terms/.test(text(button)))!.click(); await settle();
    expect(scroll).toHaveBeenCalled();
    expect(scroll.mock.calls.at(-1)?.[0]).toMatchObject({ behavior: 'auto' });
  });
});
