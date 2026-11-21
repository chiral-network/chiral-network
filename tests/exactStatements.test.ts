import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { mount, tick, unmount } from 'svelte';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { formatCount, type Account } from '$lib/native';

// inference-account-snapshot task 2.5. The desktop delivers integers beyond 2^53 - 1 in
// account, receipt, request and usage results as exact decimal strings, because the webview
// parses IPC JSON into doubles. The fixture is the exact JSON that
// `desktop::tests::account_and_receipt_statements_reach_the_webview_as_the_shared_exact_fixture`
// asserts the native boundary delivers; every handler returns a fresh `JSON.parse` of that text,
// as the webview receives it. Test-only stand-in for the native boundary; nothing reaches src/.
const native = vi.hoisted(() => ({
  handlers: {} as Record<string, (args: Record<string, unknown>) => unknown>,
}));
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

import Usage from '../src/pages/Usage.svelte';
import AccountPage from '../src/pages/Account.svelte';
import Playground from '../src/pages/Playground.svelte';
import { session } from '$lib/stores';

const fixtureText = readFileSync('tests/fixtures/desktop-exact-statements.json', 'utf8');
const delivered = () => JSON.parse(fixtureText) as {
  account: Account; receipt: { payload: { requests: unknown[] } }; stream: { kind: string; data: { usage?: unknown } }[];
};
const U64_MAX = '18446744073709551615';
const TWO_53_PLUS_1 = '9007199254740993';
const U128_MAX_CFX = '340282366920938463463.374607431768211455';
// Grouping follows the runtime locale, as `toLocaleString` does on the page.
const grouped = (digits: string) => BigInt(digits).toLocaleString();
const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
const byText = <T extends Element>(root: ParentNode, selector: string, pattern: RegExp) =>
  [...root.querySelectorAll<T>(selector)].find(element => pattern.test(text(element)));
const statTile = (root: ParentNode, label: string) =>
  [...root.querySelectorAll('.stat-label')].find(element => text(element) === label)?.nextElementSibling;
const definition = (root: ParentNode, term: string) =>
  [...root.querySelectorAll('dt')].find(element => text(element) === term)?.nextElementSibling;
const resultPanel = (root: ParentNode, title: string) =>
  [...root.querySelectorAll('details')].find(details => text(details.querySelector('summary')).startsWith(title))?.querySelector('pre');
async function settle() { for (let i = 0; i < 6; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeEach(() => {
  native.handlers = {};
  target = document.createElement('div'); document.body.append(target);
  session.set({ address: '0x' + '2'.repeat(40), configured: true });
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  session.set({ address: null, configured: false });
  clearMocks();
  const runtime = window as unknown as { __TAURI_INTERNALS__?: object; __TAURI_EVENT_PLUGIN_INTERNALS__?: object };
  delete runtime.__TAURI_INTERNALS__; delete runtime.__TAURI_EVENT_PLUGIN_INTERNALS__;
  delete (globalThis as { isTauri?: boolean }).isTauri;
});

function usageHandlers(account: Account) {
  native.handlers = {
    accounts: () => [account],
    pending_accounts: () => ({ pending: [] }),
    account: () => account,
    usage: () => delivered().receipt.payload.requests,
    receipt: () => delivered().receipt,
    credentials: () => [],
    funding: () => [],
    local_requests: () => ({ requests: [], scope: 'local_dispatch_keys_not_settlement' }),
    agreement: ({ contract_id }) => ({ contract_id, provider_wallet: account.provider_wallet, endpoint: 'https://provider.test', peer_id: 'peer', model: account.model.id, fee_bps: 50 }),
  };
}

describe('exact unsigned 64-bit counts (formatCount)', () => {
  it('shows safe numbers and exact decimal strings up to 2^64 - 1 with all their digits', () => {
    expect(formatCount(0)).toBe('0');
    expect(formatCount('0')).toBe('0');
    expect(formatCount(Number.MAX_SAFE_INTEGER)).toBe(grouped('9007199254740991'));
    expect(formatCount(TWO_53_PLUS_1)).toBe(grouped(TWO_53_PLUS_1));
    expect(formatCount(U64_MAX)).toBe(grouped(U64_MAX));
    expect(formatCount(U64_MAX).replace(/\D/g, '')).toBe(U64_MAX);
  });
  it('never shows a JSON number that parsing may already have rounded', () => {
    // What the webview made of these counters when they crossed as JSON numbers.
    const rounded = JSON.parse(`{"prompt_tokens":${U64_MAX},"completion_tokens":${TWO_53_PLUS_1}}`);
    expect(rounded.prompt_tokens).toBe(18446744073709552000);
    expect(rounded.completion_tokens).toBe(9007199254740992);
    expect(formatCount(rounded.prompt_tokens)).toBe('unavailable');
    expect(formatCount(rounded.completion_tokens)).toBe('unavailable');
    expect(formatCount(2 ** 53)).toBe('unavailable');
  });
  it('refuses out-of-range, signed, fractional and non-canonical counts', () => {
    for (const value of ['18446744073709551616', '-1', '01', '00', '1.0', '1e3', '+1', ' 1', '', '0x10', -1, 1.5, NaN, Infinity, null, undefined, {}, 1n]) {
      expect(formatCount(value), String(value)).toBe('unavailable');
    }
  });
});

describe('account and receipt presentation at 2^53 + 1, 2^64 - 1 and u128::MAX wei', () => {
  it('Usage shows the exact token counters, wei, opaque status and signed receipt', async () => {
    usageHandlers(delivered().account);
    mounted = mount(Usage, { target }); await settle();

    const counters = `${grouped(U64_MAX)} / ${grouped(TWO_53_PLUS_1)}`;
    expect(text(statTile(target, 'Input / output tokens'))).toBe(counters);
    expect(text(definition(target, 'Input / output tokens'))).toBe(counters);
    expect(text(target)).not.toContain('18,446,744,073,709,552,000');
    expect(text(target)).not.toContain('unavailable');
    expect(text(definition(target, 'Funded'))).toBe(`${U128_MAX_CFX} CFX`);
    expect(text(definition(target, 'Spent'))).toBe('340282366920938463463.37460743176821144 CFX');
    expect(text(definition(target, 'Inference charges waived'))).toBe(`${U128_MAX_CFX} CFX`);
    expect(text(definition(target, 'Reserved in flight'))).toBe('0.000000000000000003 CFX');
    expect(text(definition(target, 'Platform fee'))).toBe('0.000000000000000005 CFX');
    expect(text(definition(target, 'Available'))).toBe(`${U128_MAX_CFX} CFX`);
    expect(text(byText(target, '.rounded-xl', /^Funding may be frozen or reversed/)))
      .toContain(`Reversed: 0.000000000000000007 CFX. Provider loss: ${U128_MAX_CFX} CFX.`);
    // An unfamiliar status is shown as the provider stated it, not coerced.
    expect(text(byText(target, '[role="status"]', /^Account status:/))).toBe('Account status: provider_review');

    byText<HTMLButtonElement>(target, 'button', /Fetch and verify signed receipt/)!.click(); await settle();
    const receipt = resultPanel(target, 'Native-verified usage receipt');
    expect(receipt).toBeDefined();
    expect(JSON.parse(receipt!.textContent ?? '')).toEqual(delivered().receipt);
    const raw = receipt!.textContent ?? '';
    for (const exact of [`"sequence": "${U64_MAX}"`, `"prompt_tokens": "${U64_MAX}"`, `"completion_tokens": "${TWO_53_PLUS_1}"`,
      `"ttft_us": "${U64_MAX}"`, `"settled_at": "9007199254740992"`, `"elapsed_us": 9007199254740991`, `"waived_wei": "340282366920938463463374607431768211455"`]) {
      expect(raw).toContain(exact);
    }
    expect(raw).not.toMatch(/18446744073709552000|e\+/);

    byText<HTMLButtonElement>(target, '[role="tab"]', /Requests/)!.click(); await settle();
    const requests = resultPanel(target, 'Requests and metered usage');
    expect(JSON.parse(requests!.textContent ?? '')).toEqual(delivered().receipt.payload.requests);
    expect(requests!.textContent).toContain(`"max_output_tokens": "${U64_MAX}"`);
  });

  it('Usage shows an already-rounded counter as unavailable, never as a wrong count', async () => {
    const rounded = JSON.parse(fixtureText.replace(`"prompt_tokens": "${U64_MAX}"`, `"prompt_tokens": ${U64_MAX}`)
      .replace(`"completion_tokens": "${TWO_53_PLUS_1}"`, `"completion_tokens": ${TWO_53_PLUS_1}`)).account as Account;
    expect(rounded.usage.prompt_tokens).toBe(18446744073709552000);
    usageHandlers(rounded);
    mounted = mount(Usage, { target }); await settle();
    expect(text(statTile(target, 'Input / output tokens'))).toBe('unavailable / unavailable');
    expect(text(definition(target, 'Input / output tokens'))).toBe('unavailable / unavailable');
    expect(text(target)).not.toMatch(/18,446,744,073,709,552,000|9,007,199,254,740,992/);
  });

  it('Account shows the exact u128::MAX available balance and the stated status', async () => {
    native.handlers = { accounts: () => [delivered().account] };
    mounted = mount(AccountPage, { target }); await settle();
    byText<HTMLButtonElement>(target, 'button', /Load local account index/)!.click(); await settle();
    const card = target.querySelector('article');
    expect(text(card?.querySelector('.subpanel'))).toBe(`Available ${U128_MAX_CFX} CFX`);
    expect(text(card)).toContain('provider_review');
  });

  it('Playground states the exact output-token limit and keeps the requested count', async () => {
    native.handlers = { accounts: () => [delivered().account], account: () => delivered().account };
    mounted = mount(Playground, { target }); await settle();
    expect(text(byText(target, '.field-hint', /^Up to /))).toBe(`Up to ${grouped(U64_MAX)} for this model.`);
    const limit = target.querySelector<HTMLInputElement>('input[type="number"][min="1"]')!;
    expect(limit.getAttribute('max')).toBe(U64_MAX);
    expect(limit.value).toBe('256');
  });

  it('Playground shows streamed and reconciled usage exactly through the real stream channel', async () => {
    // The fixture account with only its status changed, so the page offers generation.
    const account = { ...delivered().account, status: 'active' } as Account;
    native.handlers = { accounts: () => [account], account: () => account, request_status: () => delivered().receipt.payload.requests[0] };
    const events = delivered().stream;
    let requested: Record<string, unknown> | undefined;
    // The real `stream()` and its Channel. Each desktop event text is delivered by evaluating
    // the exact script tauri 2.9 builds for a small channel message (`format_raw_js`), so JSON
    // numbers in it become JavaScript doubles just as in the webview.
    (globalThis as { isTauri?: boolean }).isTauri = true;
    mockIPC((cmd, args) => {
      if (cmd !== 'inference_stream') throw new Error(`unexpected IPC ${cmd}`);
      const { onEvent, request } = args as { onEvent: { id: number }; request: Record<string, unknown> };
      requested = request;
      setTimeout(() => events.forEach((event, index) => {
        (0, eval)(`window.__TAURI_INTERNALS__.runCallback(${onEvent.id}, { message: ${JSON.stringify(event)}, index: ${index} })`);
      }), 0);
      return 'stream-exact';
    });
    mounted = mount(Playground, { target }); await settle();
    const prompt = target.querySelector('textarea')!;
    prompt.value = 'hello'; prompt.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    byText<HTMLButtonElement>(target, 'button', /^Generate$/)!.click(); await settle();

    expect(requested?.max_tokens).toBe(256);
    expect(text(target.querySelector('.model-output'))).toBe('hi');
    const panel = resultPanel(target, 'Usage / request accounting');
    expect(JSON.parse(panel!.textContent ?? '')).toEqual(events[1].data.usage);
    expect(panel!.textContent).toContain(`"prompt_tokens": "${U64_MAX}"`);
    expect(panel!.textContent).toContain(`"completion_tokens": "${TWO_53_PLUS_1}"`);
    expect(text(target)).not.toMatch(/18446744073709552000|9007199254740992\b/);
    // The provider's own event text, before the desktop's copy, evaluated by the same script.
    const providerText = JSON.stringify(events[1]).replace(`"prompt_tokens":"${U64_MAX}"`, `"prompt_tokens":${U64_MAX}`);
    expect(((0, eval)(`(${providerText})`) as { data: { usage: { prompt_tokens: number } } }).data.usage.prompt_tokens)
      .toBe(18446744073709552000);

    byText<HTMLButtonElement>(target, 'button', /Reconcile request status and billing/)!.click(); await settle();
    const reconciled = resultPanel(target, 'Usage / request accounting');
    expect(JSON.parse(reconciled!.textContent ?? '')).toEqual(delivered().receipt.payload.requests[0]);
    expect(reconciled!.textContent).toContain(`"completion_tokens": "${U64_MAX}"`);
  });
});
