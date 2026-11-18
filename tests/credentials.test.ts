import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import type { Account } from '$lib/native';

// Test-only stand-in for the native boundary (A.5.3 credential management).
const native = vi.hoisted(() => ({
  handlers: {} as Record<string, (args: Record<string, unknown>) => unknown>,
  calls: [] as { operation: string; args: Record<string, unknown> }[],
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
  };
});

import Usage from '../src/pages/Usage.svelte';
import { session } from '$lib/stores';

const provider = '0x' + '1'.repeat(40);
const saved: Account = {
  contract_id: 'contract-under-test', provider_wallet: provider, consumer_wallet: '0x' + '2'.repeat(40),
  network: { chain_id: 71, genesis_hash: '0x' + '3'.repeat(64), deployment_id: 'credential-test' },
  model: { id: 'test/model', revision: 'a'.repeat(40), tokenizer_revision: 'b'.repeat(40), chat_template_sha256: 'c'.repeat(64), quantization: null, context_length: 4096, max_output_tokens: 256, capabilities: ['chat'], serving: null },
  price: { input_wei_per_1k: '1000', output_wei_per_1k: '2000' }, status: 'active', funded_wei: '1000', fee_wei: '5', spent_wei: '0',
  waived_wei: '0', reserved_wei: '0', available_wei: '995', reversed_wei: '0', provider_loss_wei: '0', usage: { prompt_tokens: 0, completion_tokens: 0 },
};
const secretKey = 'chiral_sk_test_' + 'z'.repeat(32);
const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
const button = (pattern: RegExp) => [...target.querySelectorAll<HTMLButtonElement>('button')].find(b => pattern.test(text(b)))!;
async function settle() { for (let i = 0; i < 6; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }
const issued = () => native.calls.filter(call => call.operation === 'credential_issue' || call.operation === 'credential_rotate');

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeEach(async () => {
  native.calls = [];
  native.handlers = {
    accounts: () => [saved], pending_accounts: () => ({ pending: [] }), account: () => saved,
    usage: () => ({}), credentials: () => ({ credentials: [] }), funding: () => ({}), local_requests: () => ({}),
    agreement: () => ({ contract_id: saved.contract_id, provider_wallet: provider, endpoint: 'https://provider.test', peer_id: 'peer', model: 'test/model', fee_bps: 50 }),
    credential_issue: () => ({ id: 'credential-2' }), credential_rotate: () => ({ id: 'credential-3' }),
    credential_export: () => ({ api_key: secretKey, base_url: 'https://provider.test/v1', model: 'test/model' }),
  };
  session.set({ address: saved.consumer_wallet, configured: true });
  target = document.createElement('div'); document.body.append(target);
  mounted = mount(Usage, { target }); await settle();
  button(/^API keys$/).click(); await settle();
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  session.set({ address: null, configured: false });
  vi.restoreAllMocks();
});

async function setLifetime(value: string) {
  const field = [...target.querySelectorAll<HTMLInputElement>('input[type="number"]')].find(input => /lifetime/i.test(text(input.closest('label'))))!;
  field.value = value; field.dispatchEvent(new Event('input', { bubbles: true })); await settle();
}

describe('credential issuance options (A.5.3, book line 930)', () => {
  it('refuses a lifetime outside 60 s to 30 days, or not whole seconds, before any native call', async () => {
    const credentialId = [...target.querySelectorAll<HTMLInputElement>('input.font-mono')].find(input => /Credential ID/.test(text(input.closest('label'))))!;
    credentialId.value = 'credential-1'; credentialId.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    for (const value of ['5', '2592001', '1.5', '']) {
      await setLifetime(value);
      button(/Issue API credential/).click(); await settle();
      button(/Rotate selected credential/).click(); await settle();
      expect(issued(), value).toEqual([]);
      expect(text(target.querySelector('[role="alert"]')), value).toMatch(/whole number of seconds from 60 to 2,592,000/);
    }
  });

  it('issues the default one-day credential with no spend cap, concurrency 1 and infer and read scopes', async () => {
    const before = Math.floor(Date.now() / 1000);
    button(/Issue API credential/).click(); await settle();
    expect(issued()).toHaveLength(1);
    const options = issued()[0].args.options as Record<string, unknown>;
    expect(options.expires_at as number - before).toBeGreaterThanOrEqual(86_400);
    expect(options.expires_at as number - before).toBeLessThanOrEqual(86_402);
    expect(options).toMatchObject({ spend_cap_wei: null, max_concurrency: 1, scopes: ['infer', 'read'] });
  });
});

describe('a revealed key is copied while it stays masked (A.1.1, A.5.3)', () => {
  it('writes the key to the clipboard only after Copy key, keeps it masked and stores nothing', async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    button(/Explicitly reveal SDK credential/).click(); await settle();
    const field = target.querySelector<HTMLInputElement>('input[aria-label^="Revealed API key"]')!;
    expect(field.type).toBe('password');
    expect(text(target)).not.toMatch(/Select the password field/);
    expect(writeText).not.toHaveBeenCalled();
    button(/Copy key/).click(); await settle();
    expect(writeText).toHaveBeenCalledWith(secretKey);
    expect(field.type).toBe('password');
    expect([...target.querySelectorAll('[role="status"]')].some(node => /Key copied to the clipboard/.test(text(node)))).toBe(true);
    const stored = [localStorage, sessionStorage].flatMap(storage => Object.keys(storage).map(key => storage.getItem(key) ?? ''));
    expect(stored.some(value => value.includes(secretKey))).toBe(false);
    expect(location.href).not.toContain(secretKey);
    button(/Hide credential/).click(); await settle();
    expect(target.querySelector('input[aria-label^="Revealed API key"]')).toBeNull();
  });

  it('reports a failed copy as an error', async () => {
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: vi.fn(async () => { throw new Error('denied'); }) } });
    button(/Explicitly reveal SDK credential/).click(); await settle();
    button(/Copy key/).click(); await settle();
    expect([...target.querySelectorAll('[role="alert"]')].some(node => /key was not copied/.test(text(node)))).toBe(true);
  });
});
