import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import type { Account, StreamEvent } from '$lib/native';

// Test-only stand-in for the native boundary (A.1.7: reconcile, never resend).
const native = vi.hoisted(() => ({
  handlers: {} as Record<string, (args: Record<string, unknown>) => unknown>,
  calls: [] as { operation: string; args: Record<string, unknown> }[],
  events: [] as StreamEvent[],
}));
vi.mock('$lib/native', async () => {
  const actual = await vi.importActual<typeof import('$lib/native')>('$lib/native');
  return {
    ...actual,
    nativeAvailable: () => true,
    command: vi.fn(async (operation: string, args: Record<string, unknown> = {}) => {
      native.calls.push({ operation, args });
      const handler = native.handlers[operation];
      if (!handler) throw new Error(`unexpected native operation ${operation}`);
      return handler(args);
    }),
    stream: vi.fn(async (contractId: string, request: Record<string, unknown>, onEvent: (event: StreamEvent) => void) => {
      native.calls.push({ operation: 'inference_stream', args: { contract_id: contractId, request } });
      setTimeout(() => native.events.forEach(onEvent), 0);
      return 'stream-1';
    }),
    cancelStream: vi.fn(async () => { native.calls.push({ operation: 'inference_cancel', args: {} }); }),
  };
});

import Playground from '../src/pages/Playground.svelte';
import Usage from '../src/pages/Usage.svelte';
import { session } from '$lib/stores';

const provider = '0x' + '1'.repeat(40);
const saved: Account = {
  contract_id: 'contract-under-test', provider_wallet: provider, consumer_wallet: '0x' + '2'.repeat(40),
  network: { chain_id: 71, genesis_hash: '0x' + '3'.repeat(64), deployment_id: 'recovery-test' },
  model: { id: 'test/model', revision: 'a'.repeat(40), tokenizer_revision: 'b'.repeat(40), chat_template_sha256: 'c'.repeat(64), quantization: null, context_length: 4096, max_output_tokens: 256, capabilities: ['chat'], serving: null },
  price: { input_wei_per_1k: '1000', output_wei_per_1k: '2000' }, status: 'active', funded_wei: '1000', fee_wei: '5', spent_wei: '0',
  waived_wei: '0', reserved_wei: '0', available_wei: '995', reversed_wei: '0', provider_loss_wei: '0', usage: { prompt_tokens: 0, completion_tokens: 0 },
};
const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
const button = (root: ParentNode, pattern: RegExp) => [...root.querySelectorAll<HTMLButtonElement>('button')].find(b => pattern.test(text(b)));
async function settle() { for (let i = 0; i < 6; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeEach(() => {
  native.calls = []; native.events = [];
  native.handlers = { accounts: () => [saved], account: () => saved };
  session.set({ address: saved.consumer_wallet, configured: true });
  target = document.createElement('div'); document.body.append(target);
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  session.set({ address: null, configured: false });
});

async function generate(events: StreamEvent[]) {
  native.events = events;
  mounted = mount(Playground, { target }); await settle();
  const prompt = target.querySelector<HTMLTextAreaElement>('textarea')!;
  prompt.value = 'Hello'; prompt.dispatchEvent(new Event('input', { bubbles: true })); await settle();
  target.querySelector<HTMLFormElement>('#playground-form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  await settle();
}
const streams = () => native.calls.filter(call => call.operation === 'inference_stream');

describe('Playground failures are reconciled, never resent (A.1.7, book line 913)', () => {
  it('a failure before a request ID points to Usage and sends nothing again', async () => {
    await generate([{ kind: 'error', data: { message: 'provider dispatch failed: connection reset' } }]);
    expect(text(target.querySelector('[role="status"]'))).toBe('Failed — inspect Usage before retrying');
    expect(text(target)).toContain('provider dispatch failed: connection reset');
    const link = [...target.querySelectorAll<HTMLAnchorElement>('a[href="#/usage"]')].find(a => /Usage › Requests/.test(text(a)));
    expect(link).toBeDefined();
    expect(text(link!.parentElement)).toMatch(/local request keys/);
    expect(button(target, /Reconcile request status and billing/)).toBeUndefined();
    expect(streams()).toHaveLength(1);
  });

  it('a failure after a request ID offers Reconcile and sends nothing again', async () => {
    native.handlers.request_status = () => ({ status: 'completed' });
    await generate([{ kind: 'started', data: { request_id: 'request-1' } }, { kind: 'error', data: { message: 'stream interrupted' } }]);
    expect(text(target.querySelector('[role="status"]'))).toBe('Failed — inspect request status before retrying');
    expect([...target.querySelectorAll('a[href="#/usage"]')].some(a => /Usage › Requests/.test(text(a)))).toBe(false);
    button(target, /Reconcile request status and billing/)!.click(); await settle();
    expect(native.calls.filter(call => call.operation === 'request_status').map(call => call.args)).toEqual([{ contract_id: 'contract-under-test', request_id: 'request-1' }]);
    expect(streams()).toHaveLength(1);
  });
});

describe('Usage cancellation by request ID is presented as possibly billed (book lines 355 and 931)', () => {
  it('states that cancelling is not free and keeps Read status available', async () => {
    Object.assign(native.handlers, {
      pending_accounts: () => ({ pending: [] }), usage: () => ({}), credentials: () => ({}), funding: () => ({}), local_requests: () => ({}),
      agreement: () => ({ contract_id: saved.contract_id, provider_wallet: provider, endpoint: 'https://provider.test', peer_id: 'peer', model: 'test/model', fee_bps: 50 }),
      cancel_request: () => ({ status: 'cancelling' }), request_status: () => ({ status: 'cancelled', usage: { prompt_tokens: 5, completion_tokens: 3 } }),
    });
    mounted = mount(Usage, { target }); await settle();
    button(target, /^Requests$/)!.click(); await settle();
    const id = target.querySelector<HTMLInputElement>('input.font-mono')!;
    id.value = 'request-1'; id.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    expect(text(target)).toMatch(/does not make generated work free/);
    expect(text(target)).toMatch(/may still be billed for the tokens already generated/);
    expect(text(target)).toMatch(/not a refund/);
    button(target, /Cancel provider request/)!.click(); await settle();
    const read = button(target, /^Read status$/)!;
    expect(read.disabled).toBe(false);
    read.click(); await settle();
    expect(native.calls.map(call => call.operation).filter(op => op === 'cancel_request' || op === 'request_status')).toEqual(['cancel_request', 'request_status']);
  });
});
