import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import type { Account, ModelSpec, Offer, Quote, Signed } from '$lib/native';

// Test-only stand-in for the native boundary. The real exact-money helpers are kept;
// only `command` answers with the fixture each test installs. Nothing here reaches src/.
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

import Marketplace from '../src/pages/Marketplace.svelte';
import Usage from '../src/pages/Usage.svelte';
import { session } from '$lib/stores';

const provider = '0x' + '1'.repeat(40);
const consumer = '0x' + '2'.repeat(40);
const network = { chain_id: 71, genesis_hash: '0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804', deployment_id: 'disclosure-test' };
const model: ModelSpec = {
  id: 'test/model', revision: 'a'.repeat(40), tokenizer_revision: 'b'.repeat(40), chat_template_sha256: 'c'.repeat(64),
  quantization: null, context_length: 4096, max_output_tokens: 256, capabilities: ['chat'], serving: null,
};
const price = { input_wei_per_1k: '1000', output_wei_per_1k: '2000' };
const expiresAt = Math.floor(Date.now() / 1000) + 3600;
const offer: Signed<Offer> = {
  payload: { network, provider_wallet: provider, peer_id: 'peer', deployment_id: 'disclosure-test', endpoint: 'https://provider.test', model, price, min_funding_wei: '1000000000000000000', fee_bps: 50, expires_at: expiresAt, revision: 1 },
  signature: '0x00',
};
const quote: Signed<Quote> = {
  payload: { id: 'quote-under-test', provider_wallet: provider, consumer_wallet: consumer, model, price, funding_wei: '1000000000000000000', fee_bps: 50, expires_at: expiresAt },
  signature: '0x00',
};
function account(overrides: Partial<Account> = {}): Account {
  return {
    contract_id: 'contract-under-test', provider_wallet: provider, consumer_wallet: consumer, network, model, price,
    status: 'active', funded_wei: '1000', fee_wei: '5', spent_wei: '0', waived_wei: '0', reserved_wei: '0',
    available_wei: '995', reversed_wei: '0', provider_loss_wei: '0', usage: { prompt_tokens: 0, completion_tokens: 0 },
    ...overrides,
  };
}

// Statements requirement 2 (book line 489) needs on every opening surface.
const registrationRisk: [string, RegExp][] = [
  ['client broadcasts only after a durable acknowledgement', /\bclient broadcasts\b[^.;]*\bonly after a durable provider acknowledgement/i],
  ['provider holds signed bytes', /provider already (?:receives|holds) the signed bytes/i],
  ['provider could broadcast before or without acknowledging', /could broadcast them before or without acknowledging/i],
  ['ordering is not escrow', /this ordering is not cryptographic escrow/i],
  ['an error does not prove that no money moved', /an error, timeout or missing reply does not prove that no money moved/i],
  ['inspect and resume', /inspect the (?:pending )?funding stage[^.]*\bresume\b/i],
];
const reorganization = /chain reorganizations can freeze or reverse credit/i;
// No sentence may say that payment can be broadcast only after the acknowledgement unless
// it is about this client's own broadcast (the spec's permitted statement).
function orderingOverstatements(text: string): string[] {
  const sentences = text.split(/(?<=[.;:])\s+/);
  return sentences.filter(sentence =>
    /before (?:the |any )?(?:payment|transaction|funds?) (?:is|are|can be) (?:broadcast|sent)/i.test(sentence)
    || /(?:payment|transaction|bytes) (?:is|are|can be|will be) broadcast only after/i.test(sentence)
    || (/\bonly\b[^.;:]*\b(?:after|once)\b/i.test(sentence) && /acknowledg/i.test(sentence) && !/\bclient\b/i.test(sentence))
    || (/\b(?:cannot|can't|never)\b[^.;:]*\b(?:broadcast|sent)\b[^.;:]*\b(?:until|without|before)\b/i.test(sentence) && /acknowledg/i.test(sentence) && !/\bclient\b/i.test(sentence)));
}
const text = (element: Element | null | undefined) => (element?.textContent ?? '').replace(/\s+/g, ' ').trim();
const byText = <T extends Element>(root: ParentNode, selector: string, pattern: RegExp) =>
  [...root.querySelectorAll<T>(selector)].find(element => pattern.test(text(element)));
async function settle() { for (let i = 0; i < 5; i++) { await tick(); await new Promise(resolve => setTimeout(resolve, 0)); } }

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeAll(() => {
  // jsdom has no layout; the funding panel scrolls itself into view on selection.
  if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => {};
});
beforeEach(() => {
  native.handlers = {}; native.calls = [];
  target = document.createElement('div'); document.body.append(target);
  session.set({ address: consumer, configured: true });
});
afterEach(async () => {
  if (mounted) await unmount(mounted);
  mounted = undefined; target.remove();
  session.set({ address: null, configured: false });
});

async function openFundingPanel() {
  native.handlers = {
    discover: () => [offer],
    provider_info: () => ({ health: { reachable: true, live: true }, readiness: { reachable: true, ready: true }, reputation: { available: false, error: 'no votes' } }),
    quote: () => quote,
  };
  mounted = mount(Marketplace, { target });
  await settle();
  byText<HTMLButtonElement>(target, 'button', /Discover providers/)!.click();
  await settle();
  byText<HTMLButtonElement>(target, 'button', /Inspect provider and funding terms/)!.click();
  await settle();
  byText<HTMLButtonElement>(target, 'button', /Request signed quote/)!.click();
  await settle();
  const panel = target.querySelector<HTMLElement>('section[aria-label="Provider funding"]');
  const fundButton = byText<HTMLButtonElement>(target, 'button', /Register intent, fund, and open account/);
  expect(panel).not.toBeNull();
  expect(fundButton).toBeDefined();
  return { panel: panel!, fundButton: fundButton! };
}

describe('Marketplace opening funding disclosure (requirements 2 and 3)', () => {
  it('discloses residual registration risk and reorganizations, and keeps funding disabled until accepted', async () => {
    const { panel, fundButton } = await openFundingPanel();
    // The acceptance below refers to "the registration risk above", so the review step must
    // present the disclosure under that name (book line 489, "Residual registration risk").
    const named = byText(fundButton.parentElement!, 'p', /^Residual registration risk:/);
    expect(named, 'review step names the registration risk the acceptance refers to').toBeDefined();
    for (const [statement, pattern] of registrationRisk) expect(text(named), statement).toMatch(pattern);
    expect(orderingOverstatements(text(panel))).toEqual([]);

    // A.1.6 callout is retained and the reorganization statement is on the panel.
    expect(text(byText(panel, '.rounded-xl', /^Non-refundable, provider-specific prepayment/))).toMatch(reorganization);
    const checkbox = panel.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    const acceptance = text(checkbox.closest('label'));
    expect(acceptance).toMatch(/non-refundable prepayment/i);
    expect(acceptance).toMatch(/registration risk/i);
    expect(acceptance).toMatch(reorganization);

    expect(fundButton.disabled).toBe(true);
    fundButton.click(); await settle();
    expect(native.calls.map(call => call.operation)).not.toContain('fund');
    checkbox.click(); await settle();
    expect(checkbox.checked).toBe(true);
    expect(fundButton.disabled).toBe(false);
  });

  it('presents an interrupted opening as journaled funding to resume, with the full disclosure', async () => {
    const { panel, fundButton } = await openFundingPanel();
    native.handlers.fund = () => { throw new Error('control request interrupted; resume with the same account/transaction, never fund again'); };
    native.handlers.pending_accounts = () => ({ pending: [{ contract_id: quote.payload.id, funding_stage: 'settlement_pending' }] });
    panel.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click(); await settle();
    fundButton.click(); await settle();

    const notice = byText(panel, '[role="status"]', /Funding stage:/);
    expect(notice).toBeDefined();
    const pending = text(notice);
    expect(pending).toMatch(/Funding stage: settlement pending/);
    expect(pending).toMatch(/Do not send another payment/);
    for (const [statement, pattern] of registrationRisk) expect(pending, statement).toMatch(pattern);
    expect(orderingOverstatements(pending)).toEqual([]);
    expect(orderingOverstatements(text(panel))).toEqual([]);

    // No second payment can be prepared while the opening is unresolved; resume reuses it.
    expect(fundButton.disabled).toBe(true);
    expect(byText<HTMLButtonElement>(panel, 'button', /Request signed quote/)!.disabled).toBe(true);
    native.handlers.resume = () => account({ contract_id: quote.payload.id });
    native.handlers.pending_accounts = () => ({ pending: [] });
    byText<HTMLButtonElement>(notice!, 'button', /Resume funding/)!.click(); await settle();
    expect(native.calls.filter(call => call.operation === 'resume').map(call => call.args)).toEqual([{ contract_id: quote.payload.id }]);
    expect(native.calls.filter(call => call.operation === 'fund')).toHaveLength(1);
  });
});

async function openUsage(saved: Account, pending: { contract_id: string; topup_wei: string | null }[] = []) {
  native.handlers = {
    accounts: () => [saved],
    pending_accounts: () => ({ pending }),
    account: () => saved,
    usage: () => ({}),
    credentials: () => ({}),
    funding: () => ({}),
    local_requests: () => ({}),
  };
  mounted = mount(Usage, { target });
  await settle();
  const card = [...target.querySelectorAll('section.card')].find(section => text(section.querySelector('h2')) === 'Top up this provider');
  expect(card).toBeDefined();
  return card!;
}

describe('Usage top-up disclosure (requirements 2 and 3)', () => {
  it('states that reorganizations can freeze or reverse credit and requires accepting it before signing', async () => {
    const card = await openUsage(account());
    const warning = text(byText(card, '.rounded-xl', /^Non-refundable\./));
    expect(warning).toMatch(/Non-refundable/);
    expect(warning).toMatch(reorganization);
    const checkbox = card.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    const acceptance = text(checkbox.closest('label'));
    expect(acceptance).toMatch(/non-refundable top-up/i);
    expect(acceptance).toMatch(reorganization);

    // A top-up has no registration step: none of the opening registration-risk wording.
    expect(text(card)).not.toMatch(/acknowledg|escrow|before or without|registration/i);

    const button = byText<HTMLButtonElement>(card, 'button', /Sign and top up/)!;
    const amount = card.querySelector<HTMLInputElement>('input[inputmode="decimal"]')!;
    amount.value = '0.5'; amount.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    expect(button.disabled).toBe(true);
    button.click(); await settle();
    expect(native.calls.map(call => call.operation)).not.toContain('topup');
    checkbox.click(); await settle();
    expect(button.disabled).toBe(false);
  });

  it('presents a failed top-up as unresolved funding to resume and never says that no money moved', async () => {
    const card = await openUsage(account());
    native.handlers.topup = () => { throw new Error('control request interrupted; resume with the same account/transaction, never fund again'); };
    const amount = card.querySelector<HTMLInputElement>('input[inputmode="decimal"]')!;
    amount.value = '0.5'; amount.dispatchEvent(new Event('input', { bubbles: true }));
    card.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click(); await settle();
    byText<HTMLButtonElement>(card, 'button', /Sign and top up/)!.click(); await settle();

    expect(native.calls.filter(call => call.operation === 'topup').map(call => call.args)).toEqual([{ contract_id: 'contract-under-test', amount_wei: '500000000000000000' }]);
    expect(text(target.querySelector('[role="alert"]'))).toMatch(/interrupted/);
    expect(text(card)).toMatch(/Funding unresolved\. Resume the existing journaled payment/);
    expect(byText<HTMLButtonElement>(card, 'button', /Sign and top up/)!.disabled).toBe(true);
    expect(text(target)).not.toMatch(/no money|nothing (?:was )?(?:sent|charged|moved)|not charged/i);

    native.handlers.resume = () => account({ funded_wei: '500000000000001000' });
    byText<HTMLButtonElement>(card, 'button', /Resume pending funding/)!.click(); await settle();
    expect(native.calls.filter(call => call.operation === 'resume').map(call => call.args)).toEqual([{ contract_id: 'contract-under-test' }]);
    expect(native.calls.filter(call => call.operation === 'topup')).toHaveLength(1);
  });

  it('keeps a journaled pending top-up unresolved on load and offers only resume', async () => {
    const card = await openUsage(account(), [{ contract_id: 'contract-under-test', topup_wei: '500000000000000000' }]);
    expect(text(card)).toMatch(/Funding unresolved/);
    expect(byText(card, 'button', /Resume pending funding/)).toBeDefined();
    expect(byText<HTMLButtonElement>(card, 'button', /Sign and top up/)!.disabled).toBe(true);
    expect(card.querySelector<HTMLInputElement>('input[type="checkbox"]')!.disabled).toBe(true);
    expect(text(target)).not.toMatch(/no money/i);
  });
});

describe('affected account warning (requirement 3, A.5.3)', () => {
  const warningOf = (root: Element) => text(byText(root, 'article .rounded-xl', /chain reorganization/i));
  it('shows the reversed and provider-loss amounts for a frozen account', async () => {
    await openUsage(account({ status: 'frozen', reversed_wei: '1000', provider_loss_wei: '505', available_wei: '0', fee_wei: '0', funded_wei: '1000' }));
    const warning = warningOf(target);
    expect(warning).toMatch(/frozen or reversed after a chain reorganization/i);
    expect(warning).toMatch(/Reversed: 0\.000000000000001 CFX/);
    expect(warning).toMatch(/Provider loss: 0\.000000000000000505 CFX/);
  });
  it('shows the warning for an active account with nonzero reversed funding, and not for a clean one', async () => {
    await openUsage(account({ reversed_wei: '7', provider_loss_wei: '2' }));
    expect(warningOf(target)).toMatch(/Reversed: 0\.000000000000000007 CFX\. Provider loss: 0\.000000000000000002 CFX/);
    await unmount(mounted!); mounted = undefined; target.replaceChildren();
    await openUsage(account());
    expect(warningOf(target)).toBe('');
  });
});
