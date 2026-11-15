<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { command, stream, cancelStream, errorText, formatCfx, textDelta, type Account, type StreamEvent } from '$lib/native';
  import { toasts } from '$lib/toastStore';
  import Result from '$lib/components/Result.svelte';
  import ServingProfile from '$lib/components/ServingProfile.svelte';
  let accounts = $state<Account[]>([]), contractId = $state(''), account = $state<Account | null>(null);
  const kinds = ['chat', 'completions', 'embeddings'] as const;
  let kind = $state<typeof kinds[number]>('chat'), embeddingPreview = $state<unknown>(null);
  let prompt = $state(''), output = $state(''), maxTokens = $state(256), busy = $state(false), loading = $state(false);
  let error = $state(''), status = $state('Idle'), requestId = $state(''), streamId = $state(''), usage = $state<unknown>(null);
  let destroyed = false, cancellationRequested = false;
  async function load() {
    loading = true; error = '';
    try { accounts = await command<Account[]>('accounts'); if (!contractId && accounts.length) contractId = accounts[0].contract_id; if (contractId) await select(); }
    catch (e) { error = errorText(e); } finally { loading = false; }
  }
  async function select() {
    account = null;
    try {
      account = await command<Account>('account', { contract_id: contractId });
      if (!account.model.capabilities.includes(kind)) kind = kinds.find(value => account!.model.capabilities.includes(value)) ?? 'chat';
      if (account.model.max_output_tokens > 0) maxTokens = Math.max(1, Math.min(maxTokens, account.model.max_output_tokens));
    }
    catch (e) { error = errorText(e); }
  }
  function resetResult() {
    output = ''; usage = null; embeddingPreview = null; error = ''; requestId = ''; status = 'Idle';
  }
  function event(event: StreamEvent) {
    if (destroyed) return;
    const data = event.data && typeof event.data === 'object' && !Array.isArray(event.data) ? event.data as Record<string, unknown> : {};
    if (event.kind === 'started') { requestId = String(data.request_id ?? ''); status = 'Generating'; }
    if (event.kind === 'chunk' && !cancellationRequested) {
      const delta = textDelta(data);
      if (output.length + delta.length > 1_000_000) {
        error = 'Response exceeded the desktop display limit; cancellation requested. Inspect actual usage before retrying.';
        void cancel();
        return;
      }
      output += delta;
      if (data.usage) usage = data.usage;
    }
    if (event.kind === 'error') { error = String(data.message ?? 'Inference failed'); status = 'Failed — inspect request status before retrying'; busy = false; }
    if (event.kind === 'done' || event.kind === 'cancelled') {
      status = event.kind === 'done' ? 'Completed' : 'Cancelled — generated tokens may be billed'; busy = false;
      toasts.notify('inference', status, 'info'); void select();
    }
  }
  async function generate() {
    if (!account || busy) return;
    const selectedKind = kind;
    busy = true; resetResult(); streamId = ''; status = 'Connecting'; cancellationRequested = false;
    try {
      if (selectedKind === 'embeddings') {
        const input = prompt.split(/\r?\n/);
        if (input.length > 32 || input.some(value => !value.length)) throw new Error('Enter 1–32 non-empty lines, one embedding input per line.');
        const response = await command<{ data: { index: number; embedding: number[] }[]; usage: unknown }>('infer', {
          contract_id: contractId, kind: selectedKind, request: { model: account.model.id, input, encoding_format: 'float' },
        });
        if (destroyed) return;
        embeddingPreview = response.data.map(item => ({ index: item.index, dimensions: item.embedding.length, first_components: item.embedding.slice(0, 8) }));
        usage = response.usage; status = 'Completed'; busy = false;
        toasts.notify('inference', 'Embeddings completed', 'success'); void select();
        return;
      }
      const input = selectedKind === 'chat' ? { messages: [{ role: 'user', content: prompt }] } : { prompt };
      const id = await stream(contractId, { model: account.model.id, ...input, max_tokens: maxTokens, stream: true, stream_options: { include_usage: true } }, event, selectedKind);
      streamId = id;
      if (destroyed || cancellationRequested) await cancelStream(id);
    } catch (e) { if (!destroyed) { error = errorText(e); status = 'Failed — inspect Usage before retrying'; busy = false; } }
  }
  async function cancel() {
    cancellationRequested = true; status = 'Cancelling…';
    if (streamId) { try { await cancelStream(streamId); } catch (e) { error = errorText(e); } }
  }
  async function checkRequest() {
    try { usage = await command('request_status', { contract_id: contractId, request_id: requestId }); }
    catch (e) { error = errorText(e); }
  }
  onMount(() => { void load(); });
  onDestroy(() => { destroyed = true; cancellationRequested = true; if (streamId && busy) void cancelStream(streamId).catch(() => {}); });
</script>
<section>
  <h1>Playground</h1><p>Use the selected model's advertised text endpoints. Output is untrusted data; no HTML or tool calls are executed. Inputs go to the provider and are not persisted by this UI.</p>
  <div class="row"><label>Provider account<select bind:value={contractId} onchange={() => { resetResult(); void select(); }} disabled={busy}><option value="">Choose account</option>{#each accounts as item}<option value={item.contract_id}>{item.model.id} · {item.provider_wallet} · {item.contract_id.slice(0, 10)}</option>{/each}</select></label><button onclick={load} disabled={busy || loading}>{loading ? 'Loading…' : 'Refresh accounts'}</button></div>
  {#if !loading && !accounts.length}<p class="empty">No local provider accounts. <a href="#/marketplace">Choose and fund a provider</a> first.</p>{/if}
  {#if account}<p>Provider: <span class="mono">{account.provider_wallet}</span> · Available {formatCfx(account.available_wei)} CFX · {account.status}</p>{#if !['active', 'free'].includes(account.status)}<p class="warning">This account is {account.status}. Frozen or orphaned funding cannot be spent. Review the Usage page before sending another payment.</p>{/if}{#if account.available_wei === '0' && (account.price.input_wei_per_1k !== '0' || account.price.output_wei_per_1k !== '0')}<p class="warning">Insufficient balance for paid inference. Top up this provider's account from Usage.</p>{/if}{/if}
  {#if account}<ServingProfile model={account.model} />{/if}
  <form onsubmit={e => { e.preventDefault(); void generate(); }}>
    <label>Endpoint<select bind:value={kind} onchange={resetResult} disabled={busy || !account}>{#each kinds as value}{#if account?.model.capabilities.includes(value)}<option value={value}>{value === 'chat' ? 'Chat' : value === 'completions' ? 'Text completion' : 'Embeddings'}</option>{/if}{/each}</select></label>
    <label>{kind === 'embeddings' ? 'Text inputs — one per line, up to 32' : 'Prompt'}<textarea bind:value={prompt} rows="7" required maxlength="100000" disabled={busy}></textarea></label>
    {#if kind !== 'embeddings'}<label>Maximum output tokens<input type="number" min="1" max={account?.model.max_output_tokens ?? 1} bind:value={maxTokens} required disabled={busy || !account} /></label>{:else}<p>Embeddings are non-streamed. The preview shows the first eight components; SDK responses contain complete vectors. If you leave before completion, inspect the request and charge in Usage.</p>{/if}
    <div class="row"><button disabled={busy || !account || !['active', 'free'].includes(account.status) || !account.model.capabilities.includes(kind)}>{kind === 'embeddings' ? 'Embed text' : 'Generate'}</button>{#if kind !== 'embeddings'}<button type="button" disabled={!busy} onclick={cancel}>Cancel stream</button>{/if}</div>
  </form>
  <p role="status">{status}</p>{#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if output}<section class="panel"><h2>Model response</h2><pre class="model-output">{output}</pre></section>{/if}
  <Result value={embeddingPreview} title="Embedding preview — first eight components" />
  {#if requestId}<p class="mono">Request: {requestId}</p><button onclick={checkRequest}>Reconcile request status and billing</button>{/if}
  <Result value={usage} title="Usage / request accounting" />
</section>
