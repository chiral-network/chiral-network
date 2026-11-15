<script lang="ts">
  import { onMount } from 'svelte';
  import { command, errorText, formatCfx, parseCfx, type Account } from '$lib/native';
  import Result from '$lib/components/Result.svelte';
  import ServingProfile from '$lib/components/ServingProfile.svelte';
  type PendingFunding = { pending: { contract_id: string; topup_wei: string | null }[] };
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
  async function topup() {
    if (!accept) return;
    await run(async () => { const amountWei = parseCfx(amount); pending = true; account = await command<Account>('topup', { contract_id: contractId, amount_wei: amountWei }); pending = false; accept = false; });
  }
  async function resume() { await run(async () => { account = await command<Account>('resume', { contract_id: contractId }); pending = false; }); }
  async function recover() {
    await run(async () => { account = await command<Account>('resume', { contract_id: recoveryId }); contractId = recoveryId; accounts = await command<Account[]>('accounts'); pendingAccounts = await command<PendingFunding>('pending_accounts'); });
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
</script>
<section>
  <h1>Usage & API credentials</h1>
  <div class="row"><label>Provider account<select bind:value={contractId} onchange={select} disabled={busy}><option value="">Choose account</option>{#each accounts as item}<option value={item.contract_id}>{item.model.id} · {item.contract_id}</option>{/each}</select></label><button disabled={busy} onclick={refresh}>{busy ? 'Loading…' : 'Refresh'}</button></div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if !busy && !accounts.length}<p class="empty">No accounts in this network's local journal. <a href="#/marketplace">Open a provider account</a>.</p>{/if}
  <section class="panel">
    <h2>Recover journaled funding</h2>
    <p>Pending intents and top-ups survive restarts. Resume the existing transaction instead of sending a duplicate payment.</p>
    <Result value={pendingAccounts} title="Native pending funding journal" />
    <form onsubmit={e => { e.preventDefault(); void recover(); }}>
      <label>Journaled contract / quote ID<input bind:value={recoveryId} required /></label>
      <button disabled={busy || !recoveryId}>Resume journaled funding</button>
    </form>
    <button disabled={busy || !recoveryId} onclick={inspectRecovery}>Inspect wallet recovery history</button>
    <Result value={recoveryHistory} title="Wallet funding operations (including historical cancellations)" />
    <details><summary>Cancel an unresolved funding transaction</summary>
      <p class="warning">Cancellation spends gas on a same-nonce replacement. The original payment may win the race and remains non-refundable. The wallet keeps both signed transactions and requires a canonical successful replacement receipt at the eSpace safe frontier before declaring cancellation complete.</p>
      <p>Blank operation selects the current pending top-up, or an opening that has not created an account. For an existing account with no pending top-up, select an exact operation ID from wallet recovery history. Historical cancellations and orphaned openings remain recoverable after a reorg. Existing credit remains usable; this action is not a refund.</p>
      <label>Exact wallet operation ID (optional)<input bind:value={recoveryOperation} placeholder="topup:contract-id:operation-id" disabled={busy} /></label>
      <label class="check"><input type="checkbox" bind:checked={cancelAccepted} disabled={busy} />I accept the gas cost and the risk that the original payment executes first.</label>
      <button disabled={busy || !recoveryId || !cancelAccepted} onclick={cancelFunding}>Submit or resume cancellation</button>
      <Result value={cancellation} title="Canonical cancellation outcome" />
    </details>
  </section>
  {#if account}
    <article class="panel"><h2>{account.model.id}</h2><p class="mono">Provider {account.provider_wallet}</p><p role="status">Account status: <strong>{account.status}</strong></p>
      {#if !['active', 'free'].includes(account.status) || account.reversed_wei !== '0'}<p class="warning">Funding may be frozen or reversed after a chain reorganization. Reversed: {formatCfx(account.reversed_wei)} CFX. Provider loss: {formatCfx(account.provider_loss_wei)} CFX. Available credit never includes pending funding.</p>{/if}
      <dl><dt>Funded</dt><dd>{formatCfx(account.funded_wei)} CFX</dd><dt>Platform fee</dt><dd>{formatCfx(account.fee_wei)} CFX</dd><dt>Spent</dt><dd>{formatCfx(account.spent_wei)} CFX</dd><dt>Inference charges waived</dt><dd>{formatCfx(account.waived_wei)} CFX</dd><dt>Reserved in flight</dt><dd>{formatCfx(account.reserved_wei)} CFX</dd><dt>Available</dt><dd>{formatCfx(account.available_wei)} CFX</dd><dt>Input / output tokens</dt><dd>{account.usage.prompt_tokens} / {account.usage.completion_tokens}</dd></dl>
      <p>Waived inference charges remain in this provider balance; they are not an on-chain refund or transferable credit. Funding fees and network gas are unchanged. Request records and signed receipts identify performance eligibility, timing and remedy.</p>
      <ServingProfile model={account.model} />
    </article>
    <section class="panel"><h2>Top up this provider</h2><p class="warning">Non-refundable. Only provider <span class="mono">{account.provider_wallet}</span> can serve this credit. Use a small balance; gas is extra and reorg risk remains.</p>
      <form onsubmit={e => { e.preventDefault(); void topup(); }}><label>Amount (CFX)<input bind:value={amount} inputmode="decimal" required disabled={busy || pending} /></label><label class="check"><input type="checkbox" bind:checked={accept} disabled={busy || pending} />I accept this provider-specific non-refundable top-up.</label><button disabled={busy || pending || !accept}>Sign and top up</button></form>
      {#if pending}<p class="warning">Funding unresolved. Resume the existing journaled payment or explicitly cancel it above after accepting the gas and race risk. Do not create a duplicate payment.</p><button disabled={busy} onclick={resume}>Resume pending funding</button>{/if}
    </section>
    <Result value={usage} title="Requests and metered usage" />
    <Result value={funding} title="Funding settlement and reorg evidence" />
    <Result value={localRequests} title="Local request keys (including ambiguous dispatches)" />
    <section class="panel"><h2>Reconcile a request</h2><label>Request ID or local request key<input bind:value={requestId} /></label><div class="row"><button disabled={busy || !requestId} onclick={() => run(async () => { requestResult = await command('request_status', { contract_id: contractId, request_id: requestId }); })}>Read status</button><button disabled={busy || !requestId} onclick={() => run(async () => { requestResult = await command('cancel_request', { contract_id: contractId, request_id: requestId }); })}>Cancel provider request</button></div><Result value={requestResult} title="Request result" /></section>
    <button disabled={busy} onclick={() => run(async () => { receipt = await command('receipt', { contract_id: contractId }); })}>Fetch and verify signed receipt</button><Result value={receipt} title="Native-verified usage receipt" />
    <section class="panel"><h2>API credentials</h2><p>Keys remain in protected native storage unless you explicitly reveal one here for an SDK. Never share a key or send it to a different provider. Hiding a revealed key does not revoke it.</p><Result value={credentials} title="Credential metadata (no secrets)" />
      <p>Resume never rotates keys. Explicitly issuing an inference-capable credential restores the native default if it is missing, revoked or expired; a valid existing default is preserved. Read-only credentials never become native inference defaults.</p>
      <label>Credential ID (blank selects the local default for reveal)<input bind:value={credentialId} oninput={() => secret = null} /></label><label>New credential lifetime (seconds)<input type="number" min="60" max="2592000" bind:value={ttl} /></label>
      <div class="row"><button disabled={busy} onclick={() => credentialAction('credential_issue')}>Issue API credential</button><button disabled={busy || !credentialId} onclick={() => credentialAction('credential_rotate')}>Rotate selected credential</button><button disabled={busy || !credentialId} onclick={() => credentialAction('credential_revoke')}>Revoke selected credential</button><button disabled={busy} onclick={reveal}>Explicitly reveal SDK credential</button></div>
      {#if secret}<section class="secret"><h3>Revealed API credential</h3><label>API key<input type="password" readonly value={secret.api_key} aria-label="Revealed API key; select to copy" /></label><p class="mono">Base URL: {secret.base_url}</p><p>Model: {secret.model}</p><p>Select the password field to copy the key into your SDK's secure environment. Do not place it in source control.</p><button onclick={() => secret = null}>Hide credential</button></section>{/if}
    </section>
    <section class="panel"><h2>Use a standard OpenAI SDK</h2><p>For a chat-capable account, install <code>openai</code> with pip or npm. Set CHIRAL_API_KEY, CHIRAL_BASE_URL and CHIRAL_MODEL in your private process environment using the explicit credential export above. These examples send real paid requests to that provider; they do not retry or switch providers.</p>
      <details><summary>Python</summary><pre class="data">{`import os
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
    stream.close()`}</pre></details>
      <details><summary>JavaScript</summary><pre class="data">{`import OpenAI from "openai";

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
}`}</pre></details>
    </section>
    <section class="panel"><h2>Payment-backed provider feedback</h2><p>Submit your subjective score against this account's verified funding evidence. Revision must increase when replacing earlier feedback.</p><form onsubmit={e => { e.preventDefault(); void run(async () => { feedbackResult = await command('feedback', { contract_id: contractId, score: feedbackScore, revision: feedbackRevision }); }); }}><label>Score (0–100)<input type="number" min="0" max="100" bind:value={feedbackScore} required /></label><label>Revision<input type="number" min="1" bind:value={feedbackRevision} required /></label><button disabled={busy}>Sign and submit feedback</button></form><Result value={feedbackResult} title="Feedback result" /></section>
  {/if}
</section>
