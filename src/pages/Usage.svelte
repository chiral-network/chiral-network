<script lang="ts">
  import { onMount } from 'svelte';
  import { RefreshCw, Wallet, Coins, Flame, Hourglass, Gift, Hash, LayoutDashboard, KeyRound, ListChecks, LifeBuoy, ThumbsUp, Bot, PlusCircle, RotateCcw, Search, Ban, ReceiptText, Eye, EyeOff, RotateCw, Trash2, Code, Star, Loader2, ShieldAlert, Inbox, ChevronRight } from 'lucide-svelte';
  import { command, errorText, formatCfx, parseCfx, type Account } from '$lib/native';
  import Result from '$lib/components/Result.svelte';
  import ServingProfile from '$lib/components/ServingProfile.svelte';
  import PageHeader from '$lib/components/ui/PageHeader.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import StatTile from '$lib/components/ui/StatTile.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  import EmptyState from '$lib/components/ui/EmptyState.svelte';
  import { statusBadge } from '$lib/components/ui/tones';
  import { stageLabel, unresolvedPayments, type PendingFunding as JournalEntry } from '$lib/funding';
  type PendingFunding = { pending: JournalEntry[] };
  let accounts = $state<Account[]>([]), contractId = $state(''), account = $state<Account | null>(null);
  let usage = $state<unknown>(null), receipt = $state<unknown>(null), credentials = $state<unknown>(null);
  let secret = $state<{ api_key: string; base_url: string; model: string } | null>(null);
  let credentialId = $state(''), amount = $state(''), accept = $state(false), busy = $state(false), error = $state('');
  let pending = $state(false), ttl = $state(86400), feedbackScore = $state(50), feedbackRevision = $state(1), feedbackResult = $state<unknown>(null);
  let pendingAccounts = $state<PendingFunding | null>(null), recoveryId = $state(''), funding = $state<unknown>(null), localRequests = $state<unknown>(null), requestId = $state(''), requestResult = $state<unknown>(null);
  let cancelAccepted = $state(false), cancellation = $state<unknown>(null);
  let recoveryOperation = $state(''), recoveryHistory = $state<unknown>(null);
  async function run(action: () => Promise<void>) { busy = true; error = ''; try { await action(); } catch (e) { error = errorText(e); } finally { busy = false; } }
  async function select() {
    secret = null; usage = null; receipt = null; credentials = null; account = null; pending = false; accept = false; funding = null; localRequests = null; requestResult = null;
    if (!contractId) return;
    await run(async () => {
      account = await command<Account>('account', { contract_id: contractId });
      pending = pendingAccounts?.pending.some(item => item.contract_id === contractId && item.topup_wei !== null) ?? false;
      if (!recoveryId) recoveryId = contractId;
      [usage, credentials, funding, localRequests] = await Promise.all([command('usage', { contract_id: contractId }), command('credentials', { contract_id: contractId }), command('funding', { contract_id: contractId }), command('local_requests', { contract_id: contractId })]);
    });
  }
  async function refresh() {
    await run(async () => { [accounts, pendingAccounts] = await Promise.all([command<Account[]>('accounts'), command<PendingFunding>('pending_accounts')]); if (!contractId && accounts.length) contractId = accounts[0].contract_id; });
    if (contractId) await select();
  }
  // A funding attempt, then a re-read of the native funding journal whether it succeeded or
  // failed: after an error, timeout or lost reply the journaled stage and hash show without
  // Refresh, and a resolved operation leaves no stale notice.
  async function attempt(action: () => Promise<void>) {
    busy = true; error = '';
    try { await action(); } catch (e) { error = errorText(e); }
    try { pendingAccounts = await command<PendingFunding>('pending_accounts'); }
    catch (e) { error = [error, `Pending funding journal unavailable: ${errorText(e)}`].filter(Boolean).join(' '); }
    finally { busy = false; }
  }
  async function topup() {
    if (!accept || topupBlocked) return;
    await attempt(async () => { const amountWei = parseCfx(amount); pending = true; account = await command<Account>('topup', { contract_id: contractId, amount_wei: amountWei }); pending = false; accept = false; });
  }
  async function resume() { await attempt(async () => { account = await command<Account>('resume', { contract_id: contractId }); pending = false; }); }
  async function recover() {
    await attempt(async () => { account = await command<Account>('resume', { contract_id: recoveryId }); contractId = recoveryId; accounts = await command<Account[]>('accounts'); });
    if (account) await select();
  }
  async function inspectRecovery() {
    await run(async () => { recoveryHistory = await command('funding_recovery', { contract_id: recoveryId }); });
  }
  async function cancelFunding() {
    if (!cancelAccepted || !recoveryId) return;
    await run(async () => {
      const result = await command<{ operation_id?: string }>('cancel_funding', { contract_id: recoveryId, operation_id: recoveryOperation || null, accept_gas_and_race_risk: true });
      cancellation = result;
      if (result.operation_id) recoveryOperation = result.operation_id;
      cancelAccepted = false;
      pendingAccounts = await command<PendingFunding>('pending_accounts');
      recoveryHistory = await command('funding_recovery', { contract_id: recoveryId });
      if (contractId === recoveryId) {
        account = await command<Account>('account', { contract_id: contractId });
        pending = pendingAccounts.pending.some(item => item.contract_id === contractId && item.topup_wei !== null);
      }
    });
  }
  async function credentialAction(action: string) {
    secret = null;
    await run(async () => {
      await command(action, { contract_id: contractId, credential_id: credentialId, options: { expires_at: Math.floor(Date.now() / 1000) + ttl, spend_cap_wei: null, max_concurrency: 1, scopes: ['infer', 'read'] } });
      credentials = await command('credentials', { contract_id: contractId });
    });
  }
  async function reveal() { await run(async () => { secret = await command('credential_export', { contract_id: contractId, credential_id: credentialId || null }); }); }
  onMount(() => { void refresh(); });

  // ---- presentation only ----
  type Tab = 'overview' | 'keys' | 'requests' | 'recovery' | 'feedback';
  let tab = $state<Tab>('overview');
  const tabs: { id: Tab; label: string; icon: typeof LayoutDashboard }[] = [
    { id: 'overview', label: 'Overview', icon: LayoutDashboard },
    { id: 'keys', label: 'API keys', icon: KeyRound },
    { id: 'requests', label: 'Requests', icon: ListChecks },
    { id: 'recovery', label: 'Funding & recovery', icon: LifeBuoy },
    { id: 'feedback', label: 'Feedback', icon: ThumbsUp },
  ];
  const pendingCount = $derived(pendingAccounts?.pending.length ?? 0);
  // Unresolved paid operations of other agreements, or of this account's opening: while any
  // exists, this account's top-up is not offered either (A.1.7). The selected account's own
  // pending top-up keeps its existing Resume path.
  const otherUnresolved = $derived(unresolvedPayments(pendingAccounts?.pending ?? []).filter(item => !(item.contract_id === contractId && item.topup_wei !== null)));
  const topupBlocked = $derived(pending || otherUnresolved.length > 0);
  const python = `import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["CHIRAL_API_KEY"],
    base_url=os.environ["CHIRAL_BASE_URL"],
    max_retries=0,
)
stream = client.chat.completions.create(
    model=os.environ["CHIRAL_MODEL"],
    messages=[{"role": "user", "content": "Hello"}],
    max_tokens=128,
    stream=True,
    stream_options={"include_usage": True},
)
try:
    for chunk in stream:
        if chunk.choices:
            print(chunk.choices[0].delta.content or "", end="", flush=True)
        if chunk.usage:
            print(chunk.usage)
finally:
    stream.close()`;
  const javascript = `import OpenAI from "openai";

const client = new OpenAI({
  apiKey: process.env.CHIRAL_API_KEY,
  baseURL: process.env.CHIRAL_BASE_URL,
  maxRetries: 0,
});
const stream = await client.chat.completions.create({
  model: process.env.CHIRAL_MODEL,
  messages: [{ role: "user", content: "Hello" }],
  max_tokens: 128,
  stream: true,
  stream_options: { include_usage: true },
});
try {
  for await (const chunk of stream) {
    process.stdout.write(chunk.choices[0]?.delta?.content ?? "");
    if (chunk.usage) console.log(chunk.usage);
  }
} finally {
  stream.controller.abort();
}`;
</script>

{#snippet needAccount()}
  <div class="card"><EmptyState icon={Wallet} title="Select a provider account">Choose an account above to see this section.</EmptyState></div>
{/snippet}

<section class="page" aria-labelledby="usage-title">
  <PageHeader id="usage-title" title="Usage & API credentials" subtitle="Balances, metered usage, API keys and funding recovery for each provider account.">
    {#snippet actions()}
      <button class="btn-secondary" disabled={busy} onclick={refresh}><RefreshCw class="w-4 h-4 {busy ? 'animate-spin' : ''}" />{busy ? 'Loading…' : 'Refresh'}</button>
    {/snippet}
  </PageHeader>

  <div class="card p-4 flex flex-col sm:flex-row sm:items-end gap-3">
    <label class="flex-1 min-w-0">
      <span class="field-label">Provider account</span>
      <select class="input" bind:value={contractId} onchange={select} disabled={busy}>
        <option value="">Choose account</option>
        {#each accounts as item}<option value={item.contract_id}>{item.model.id} · {item.contract_id}</option>{/each}
      </select>
    </label>
    {#if account}<span class="{statusBadge(account.status)} self-start sm:self-center text-sm" role="status">Account status: <strong>{account.status}</strong></span>{/if}
  </div>

  {#if error}<Callout kind="error" role="alert">{error}</Callout>{/if}
  {#if !busy && !accounts.length}<Callout kind="info">No accounts in this network's local journal. <a class="link" href="#/marketplace">Open a provider account</a>.</Callout>{/if}
  {#if pendingCount && tab !== 'recovery'}
    <Callout kind="warning" title="{pendingCount} journaled funding operation{pendingCount === 1 ? '' : 's'} pending">
      Resume the existing transaction instead of sending a duplicate payment.
      <button class="ml-1 link inline-flex items-center gap-0.5" onclick={() => tab = 'recovery'}>Open Funding & recovery<ChevronRight class="w-3.5 h-3.5" /></button>
    </Callout>
  {/if}

  {#if account}
    <div class="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-6 gap-2">
      <StatTile icon={Wallet} label="Available" value="{formatCfx(account.available_wei)} CFX" valueClass="text-primary-600 dark:text-primary-400" />
      <StatTile icon={Coins} tone="green" label="Funded" value="{formatCfx(account.funded_wei)} CFX" />
      <StatTile icon={Flame} tone="orange" label="Spent" value="{formatCfx(account.spent_wei)} CFX" />
      <StatTile icon={Hourglass} tone="amber" label="Reserved in flight" value="{formatCfx(account.reserved_wei)} CFX" />
      <StatTile icon={Gift} tone="purple" label="Charges waived" value="{formatCfx(account.waived_wei)} CFX" />
      <StatTile icon={Hash} tone="sky" label="Input / output tokens" value="{account.usage.prompt_tokens.toLocaleString()} / {account.usage.completion_tokens.toLocaleString()}" />
    </div>
  {/if}

  <div class="border-b border-gray-200 dark:border-gray-700 flex overflow-x-auto" role="tablist" aria-label="Usage sections">
    {#each tabs as t}
      <button role="tab" type="button" aria-selected={tab === t.id} class={tab === t.id ? 'tab-active' : 'tab-idle'} onclick={() => tab = t.id}>
        <t.icon class="w-4 h-4" />{t.label}
        {#if t.id === 'recovery' && pendingCount}<span class="badge-amber px-1.5">{pendingCount}</span>{/if}
      </button>
    {/each}
  </div>

  <div role="tabpanel" class="space-y-4">
    {#if tab === 'overview'}
      {#if account}
        <div class="grid grid-cols-1 xl:grid-cols-[minmax(0,1fr)_minmax(0,420px)] gap-4 items-start">
          <Card tag="article" icon={Bot} title={account.model.id} subtitle="Provider {account.provider_wallet}">
            <div class="space-y-4">
              {#if !['active', 'free'].includes(account.status) || account.reversed_wei !== '0'}<Callout kind="warning">Funding may be frozen or reversed after a chain reorganization. Reversed: {formatCfx(account.reversed_wei)} CFX. Provider loss: {formatCfx(account.provider_loss_wei)} CFX. Available credit never includes pending funding.</Callout>{/if}
              <dl class="kv subpanel p-4">
                <dt>Funded</dt><dd>{formatCfx(account.funded_wei)} CFX</dd>
                <dt>Platform fee</dt><dd>{formatCfx(account.fee_wei)} CFX</dd>
                <dt>Spent</dt><dd>{formatCfx(account.spent_wei)} CFX</dd>
                <dt>Inference charges waived</dt><dd>{formatCfx(account.waived_wei)} CFX</dd>
                <dt>Reserved in flight</dt><dd>{formatCfx(account.reserved_wei)} CFX</dd>
                <dt>Available</dt><dd class="font-semibold text-primary-600 dark:text-primary-400">{formatCfx(account.available_wei)} CFX</dd>
                <dt>Input / output tokens</dt><dd>{account.usage.prompt_tokens} / {account.usage.completion_tokens}</dd>
                <dt>Contract</dt><dd class="mono">{account.contract_id}</dd>
              </dl>
              <p class="prose-muted text-xs">Waived inference charges remain in this provider balance; they are not an on-chain refund or transferable credit. Funding fees and network gas are unchanged. Request records and signed receipts identify performance eligibility, timing and remedy.</p>
              <ServingProfile model={account.model} />
            </div>
          </Card>
          <div class="space-y-4">
            <Card icon={PlusCircle} tone="green" title="Top up this provider">
              <div class="space-y-4">
                <Callout kind="warning">Non-refundable. Only provider <span class="font-mono break-all">{account.provider_wallet}</span> can serve this credit. Use a small balance; gas is extra. Chain reorganizations can freeze or reverse credit.</Callout>
                <form class="space-y-3" onsubmit={e => { e.preventDefault(); void topup(); }}>
                  <label class="block"><span class="field-label">Amount (CFX)</span>
                    <span class="relative block"><input class="input pr-14" bind:value={amount} inputmode="decimal" required disabled={busy || topupBlocked} /><span class="absolute right-3 top-1/2 -translate-y-1/2 text-xs font-semibold text-gray-400">CFX</span></span>
                  </label>
                  <label class="check-row"><input class="checkbox mt-0.5" type="checkbox" bind:checked={accept} disabled={busy || topupBlocked} />I accept this provider-specific non-refundable top-up and that chain reorganizations can freeze or reverse credit.</label>
                  <button class="btn-primary w-full" disabled={busy || topupBlocked || !accept}><PlusCircle class="w-4 h-4" />Sign and top up</button>
                </form>
                {#if !pending && otherUnresolved.length}
                  <Callout kind="warning" role="status">Another journaled payment is unresolved: <span class="font-mono break-all">{otherUnresolved[0].contract_id}</span> ({otherUnresolved[0].topup_wei !== null ? 'top-up' : 'opening'}, stage: {stageLabel(otherUnresolved[0].funding_stage)}){otherUnresolved.length > 1 ? ` and ${otherUnresolved.length - 1} more` : ''}. Resume or reconcile it before a new top-up. Do not create a duplicate payment.
                    <div class="mt-3"><button class="btn-secondary btn-sm" onclick={() => tab = 'recovery'}><LifeBuoy class="w-3.5 h-3.5" />Open Funding &amp; recovery</button></div>
                  </Callout>
                {/if}
                {#if pending}
                  <Callout kind="warning">Funding unresolved. Resume the existing journaled payment or explicitly cancel it under Funding & recovery after accepting the gas and race risk. Do not create a duplicate payment.
                    <div class="mt-3"><button class="btn-secondary btn-sm" disabled={busy} onclick={resume}><RotateCcw class="w-3.5 h-3.5" />Resume pending funding</button></div>
                  </Callout>
                {/if}
              </div>
            </Card>
            <Card icon={ReceiptText} tone="sky" title="Signed usage receipt" subtitle="Provider-signed statement, verified natively">
              <button class="btn-secondary w-full" disabled={busy} onclick={() => run(async () => { receipt = await command('receipt', { contract_id: contractId }); })}><ReceiptText class="w-4 h-4" />Fetch and verify signed receipt</button>
            </Card>
            <Result value={receipt} title="Native-verified usage receipt" />
          </div>
        </div>
      {:else}{@render needAccount()}{/if}

    {:else if tab === 'keys'}
      {#if account}
        <Card icon={KeyRound} title="API credentials" subtitle="Keys remain in protected native storage unless you explicitly reveal one here for an SDK.">
          <div class="space-y-4">
            <p class="prose-muted">Never share a key or send it to a different provider. Hiding a revealed key does not revoke it. Resume never rotates keys. Explicitly issuing an inference-capable credential restores the native default if it is missing, revoked or expired; a valid existing default is preserved. Read-only credentials never become native inference defaults.</p>
            <Result value={credentials} title="Credential metadata (no secrets)" />
            <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
              <label class="block"><span class="field-label">Credential ID</span><input class="input font-mono" bind:value={credentialId} oninput={() => secret = null} placeholder="Blank selects the local default for reveal" /></label>
              <label class="block"><span class="field-label">New credential lifetime (seconds)</span><input class="input" type="number" min="60" max="2592000" bind:value={ttl} /></label>
            </div>
            <div class="flex flex-wrap gap-2">
              <button class="btn-primary" disabled={busy} onclick={() => credentialAction('credential_issue')}><PlusCircle class="w-4 h-4" />Issue API credential</button>
              <button class="btn-secondary" disabled={busy || !credentialId} onclick={() => credentialAction('credential_rotate')}><RotateCw class="w-4 h-4" />Rotate selected credential</button>
              <button class="btn-secondary text-red-600 dark:text-red-400" disabled={busy || !credentialId} onclick={() => credentialAction('credential_revoke')}><Trash2 class="w-4 h-4" />Revoke selected credential</button>
              <button class="btn-secondary" disabled={busy} onclick={reveal}><Eye class="w-4 h-4" />Explicitly reveal SDK credential</button>
            </div>
            {#if secret}
              <section class="rounded-xl border-2 border-amber-400 dark:border-amber-600 bg-amber-50/60 dark:bg-amber-900/10 p-4 space-y-3">
                <h3 class="font-semibold text-gray-900 dark:text-white flex items-center gap-2"><ShieldAlert class="w-4 h-4 text-amber-600" />Revealed API credential</h3>
                <label class="block"><span class="field-label">API key</span><input class="input font-mono" type="password" readonly value={secret.api_key} aria-label="Revealed API key; select to copy" /></label>
                <dl class="kv text-xs"><dt>Base URL</dt><dd class="mono">{secret.base_url}</dd><dt>Model</dt><dd>{secret.model}</dd></dl>
                <p class="text-xs text-gray-600 dark:text-gray-400">Select the password field to copy the key into your SDK's secure environment. Do not place it in source control.</p>
                <button class="btn-secondary btn-sm" onclick={() => secret = null}><EyeOff class="w-3.5 h-3.5" />Hide credential</button>
              </section>
            {/if}
          </div>
        </Card>
        <Card icon={Code} tone="gray" title="Use a standard OpenAI SDK">
          <div class="space-y-3">
            <p class="prose-muted">For a chat-capable account, install <code>openai</code> with pip or npm. Set CHIRAL_API_KEY, CHIRAL_BASE_URL and CHIRAL_MODEL in your private process environment using the explicit credential export above. These examples send real paid requests to that provider; they do not retry or switch providers.</p>
            {#each [['Python', python], ['JavaScript', javascript]] as [name, code]}
              <details class="group card overflow-hidden shadow-none">
                <summary class="flex items-center gap-2 px-4 py-2.5 text-sm font-medium list-none [&::-webkit-details-marker]:hidden hover:bg-gray-50 dark:hover:bg-gray-700/40"><ChevronRight class="w-4 h-4 text-gray-400 transition-transform group-open:rotate-90" />{name}</summary>
                <pre class="code-block border-t border-gray-200 dark:border-gray-700">{code}</pre>
              </details>
            {/each}
          </div>
        </Card>
      {:else}{@render needAccount()}{/if}

    {:else if tab === 'requests'}
      {#if account}
        <Card icon={Search} title="Reconcile a request" subtitle="Read the provider's authoritative status, or cancel an in-flight request">
          <div class="flex flex-col sm:flex-row gap-2 sm:items-end">
            <label class="flex-1 min-w-0"><span class="field-label">Request ID or local request key</span><input class="input font-mono" bind:value={requestId} /></label>
            <button class="btn-secondary py-2.5" disabled={busy || !requestId} onclick={() => run(async () => { requestResult = await command('request_status', { contract_id: contractId, request_id: requestId }); })}><Search class="w-4 h-4" />Read status</button>
            <button class="btn-secondary py-2.5 text-red-600 dark:text-red-400" disabled={busy || !requestId} onclick={() => run(async () => { requestResult = await command('cancel_request', { contract_id: contractId, request_id: requestId }); })}><Ban class="w-4 h-4" />Cancel provider request</button>
          </div>
        </Card>
        <Result value={requestResult} title="Request result" />
        <Result value={usage} title="Requests and metered usage" />
        <Result value={localRequests} title="Local request keys (including ambiguous dispatches)" open={false} />
      {:else}{@render needAccount()}{/if}

    {:else if tab === 'recovery'}
      <Card icon={LifeBuoy} tone="amber" title="Recover journaled funding" subtitle="Pending intents and top-ups survive restarts. Resume the existing transaction instead of sending a duplicate payment.">
        <div class="space-y-4">
          {#if pendingAccounts && !pendingCount}<p class="text-sm text-gray-500 flex items-center gap-2"><Inbox class="w-4 h-4" />No pending funding in the native journal.</p>{/if}
          <Result value={pendingAccounts} title="Native pending funding journal" open={pendingCount > 0} />
          <form class="flex flex-col sm:flex-row gap-2 sm:items-end" onsubmit={e => { e.preventDefault(); void recover(); }}>
            <label class="flex-1 min-w-0"><span class="field-label">Journaled contract / quote ID</span><input class="input font-mono" bind:value={recoveryId} required /></label>
            <button class="btn-primary py-2.5" disabled={busy || !recoveryId}>{#if busy}<Loader2 class="w-4 h-4 animate-spin" />{:else}<RotateCcw class="w-4 h-4" />{/if}Resume journaled funding</button>
            <button type="button" class="btn-secondary py-2.5" disabled={busy || !recoveryId} onclick={inspectRecovery}><Search class="w-4 h-4" />Inspect wallet recovery history</button>
          </form>
          <Result value={recoveryHistory} title="Wallet funding operations (including historical cancellations)" />
          <details class="group rounded-xl border border-red-200 dark:border-red-900/60 overflow-hidden">
            <summary class="flex items-center gap-2 px-4 py-3 text-sm font-medium text-red-700 dark:text-red-400 list-none [&::-webkit-details-marker]:hidden hover:bg-red-50/60 dark:hover:bg-red-900/10"><ChevronRight class="w-4 h-4 transition-transform group-open:rotate-90" /><Ban class="w-4 h-4" />Cancel an unresolved funding transaction</summary>
            <div class="p-4 pt-0 space-y-3">
              <Callout kind="warning">Cancellation spends gas on a same-nonce replacement. The original payment may win the race and remains non-refundable. The wallet keeps both signed transactions and requires a canonical successful replacement receipt at the eSpace safe frontier before declaring cancellation complete.</Callout>
              <p class="prose-muted">Blank operation selects the current pending top-up, or an opening that has not created an account. For an existing account with no pending top-up, select an exact operation ID from wallet recovery history. Historical cancellations and orphaned openings remain recoverable after a reorg. Existing credit remains usable; this action is not a refund.</p>
              <label class="block"><span class="field-label">Exact wallet operation ID (optional)</span><input class="input font-mono" bind:value={recoveryOperation} placeholder="topup:contract-id:operation-id" disabled={busy} /></label>
              <label class="check-row"><input class="checkbox mt-0.5" type="checkbox" bind:checked={cancelAccepted} disabled={busy} />I accept the gas cost and the risk that the original payment executes first.</label>
              <button class="btn-danger" disabled={busy || !recoveryId || !cancelAccepted} onclick={cancelFunding}><Ban class="w-4 h-4" />Submit or resume cancellation</button>
              <Result value={cancellation} title="Canonical cancellation outcome" />
            </div>
          </details>
        </div>
      </Card>
      {#if account}<Result value={funding} title="Funding settlement and reorg evidence" />{/if}

    {:else if tab === 'feedback'}
      {#if account}
        <Card icon={Star} tone="amber" title="Payment-backed provider feedback" subtitle="Submit your subjective score against this account's verified funding evidence. Revision must increase when replacing earlier feedback.">
          <form class="space-y-4" onsubmit={e => { e.preventDefault(); void run(async () => { feedbackResult = await command('feedback', { contract_id: contractId, score: feedbackScore, revision: feedbackRevision }); }); }}>
            <label class="block">
              <span class="field-label flex items-center justify-between">Score (0–100)<span class="text-lg font-bold text-primary-600 dark:text-primary-400 tabular-nums">{feedbackScore}</span></span>
              <input class="w-full accent-primary-600" type="range" min="0" max="100" bind:value={feedbackScore} aria-hidden="true" tabindex="-1" />
              <input class="input mt-2 max-w-[8rem]" type="number" min="0" max="100" bind:value={feedbackScore} required />
            </label>
            <label class="block max-w-[12rem]"><span class="field-label">Revision</span><input class="input" type="number" min="1" bind:value={feedbackRevision} required /></label>
            <button class="btn-primary" disabled={busy}><ThumbsUp class="w-4 h-4" />Sign and submit feedback</button>
          </form>
        </Card>
        <Result value={feedbackResult} title="Feedback result" />
      {:else}{@render needAccount()}{/if}
    {/if}
  </div>
</section>
