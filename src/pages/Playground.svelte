<script lang="ts">
  import { session } from '$lib/stores';
  import { onDestroy, onMount } from 'svelte';
  import { RefreshCw, MessageSquare, TextCursorInput, Binary, Send, Square, Loader2, CheckCircle2, XCircle, CircleDot, Ban, Sparkles, Store, ReceiptText, Wallet } from 'lucide-svelte';
  import { command, stream, cancelStream, errorText, formatCfx, formatCount, textDelta, type Account, type StreamEvent } from '$lib/native';
  import { toasts } from '$lib/toastStore';
  import Result from '$lib/components/Result.svelte';
  import ServingProfile from '$lib/components/ServingProfile.svelte';
  import PageHeader from '$lib/components/ui/PageHeader.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  import EmptyState from '$lib/components/ui/EmptyState.svelte';
  import { statusBadge, shortAddress } from '$lib/components/ui/tones';
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
      // Only an upper bound for a small requested count: a limit beyond 2^53 - 1 exceeds every safe count either way.
      const limit = Number(account.model.max_output_tokens);
      if (limit > 0) maxTokens = Math.max(1, Math.min(maxTokens, limit));
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
    // With a request ID the request can be reconciled here; without one (a dispatch failure)
    // the only handle is the local request key listed under Usage › Requests. Nothing is resent.
    if (event.kind === 'error') { error = String(data.message ?? 'Inference failed'); status = requestId ? 'Failed — inspect request status before retrying' : 'Failed — inspect Usage before retrying'; busy = false; }
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
  // Nothing is read before an explicit profile is loaded (A.1.3); a profile change remounts the page.
  onMount(() => { if ($session.configured) void load(); });
  onDestroy(() => { destroyed = true; cancellationRequested = true; if (streamId && busy) void cancelStream(streamId).catch(() => {}); });

  // ---- presentation helpers (read-only) ----
  const kindMeta = { chat: { label: 'Chat', icon: MessageSquare }, completions: { label: 'Text completion', icon: TextCursorInput }, embeddings: { label: 'Embeddings', icon: Binary } } as const;
  const usable = $derived(!!account && ['active', 'free'].includes(account.status) && account.model.capabilities.includes(kind));
  const statusView = $derived(
    status === 'Completed' ? { cls: 'badge-green', icon: CheckCircle2, spin: false }
    : status.startsWith('Failed') ? { cls: 'badge-red', icon: XCircle, spin: false }
    : status.startsWith('Cancel') ? { cls: 'badge-amber', icon: Ban, spin: status === 'Cancelling…' }
    : status === 'Idle' ? { cls: 'badge-gray', icon: CircleDot, spin: false }
    : { cls: 'badge-primary', icon: Loader2, spin: true });
</script>

<section class="page" aria-labelledby="playground-title">
  <PageHeader id="playground-title" title="Playground" subtitle="Use the selected model's advertised text endpoints. Output is untrusted data; no HTML or tool calls are executed. Inputs go to the provider and are not persisted by this UI." />

  {#if !loading && !accounts.length}
    <div class="card">
      <EmptyState icon={Sparkles} title="No local provider accounts">
        <a class="link" href="#/marketplace">Choose and fund a provider</a> first.
      </EmptyState>
    </div>
  {/if}

  <div class="grid grid-cols-1 lg:grid-cols-[minmax(0,380px)_minmax(0,1fr)] gap-4 items-start">
    <div class="space-y-4 lg:sticky lg:top-20">
      <Card icon={Wallet} title="Session" subtitle="Provider account used for these requests">
        {#snippet actions()}
          <button class="icon-btn" onclick={load} disabled={busy || loading || !$session.configured} title="Refresh accounts" aria-label="Refresh accounts">
            <RefreshCw class="w-4 h-4 {loading ? 'animate-spin' : ''}" />
          </button>
        {/snippet}
        <div class="space-y-4">
          <label class="block">
            <span class="field-label">Provider account</span>
            <select class="input" bind:value={contractId} onchange={() => { resetResult(); void select(); }} disabled={busy}>
              <option value="">Choose account</option>
              {#each accounts as item}<option value={item.contract_id}>{item.model.id} · {shortAddress(item.provider_wallet)} · {item.contract_id.slice(0, 10)}</option>{/each}
            </select>
          </label>

          {#if account}
            <div class="rounded-xl bg-gradient-to-r from-primary-600 to-primary-700 text-white p-4">
              <div class="flex items-center justify-between gap-2">
                <span class="text-primary-100 text-xs uppercase tracking-wider">Available</span>
                <span class="px-2 py-0.5 rounded-full bg-white/20 text-xs font-medium">{account.status}</span>
              </div>
              <div class="mt-1 flex items-baseline gap-1.5"><span class="text-2xl font-bold tabular-nums break-all">{formatCfx(account.available_wei)}</span><span class="text-primary-100 text-sm">CFX</span></div>
              <p class="mt-2 text-xs font-mono text-primary-100 break-all">Provider: {account.provider_wallet}</p>
            </div>
            {#if !['active', 'free'].includes(account.status)}<Callout kind="warning">This account is {account.status}. Frozen or orphaned funding cannot be spent. Review the Usage page before sending another payment.</Callout>{/if}
            {#if account.available_wei === '0' && (account.price.input_wei_per_1k !== '0' || account.price.output_wei_per_1k !== '0')}<Callout kind="warning">Insufficient balance for paid inference. Top up this provider's account from <a class="link" href="#/usage">Usage</a>.</Callout>{/if}
          {/if}

          <div>
            <span class="field-label" id="endpoint-label">Endpoint</span>
            <div class="grid grid-cols-3 rounded-lg border border-gray-200 dark:border-gray-600 overflow-hidden text-xs" role="radiogroup" aria-labelledby="endpoint-label">
              {#each kinds as value}
                {@const meta = kindMeta[value]}
                {@const supported = !!account?.model.capabilities.includes(value)}
                <button type="button" role="radio" aria-checked={kind === value} disabled={busy || !supported}
                  title={supported ? meta.label : 'Not advertised by this model'}
                  onclick={() => { if (kind !== value) { kind = value; resetResult(); } }}
                  class="flex flex-col items-center gap-1 px-2 py-2 transition-colors disabled:opacity-40 disabled:cursor-not-allowed {kind === value && supported ? 'bg-primary-600 text-white' : 'bg-white dark:bg-gray-800 text-gray-600 dark:text-gray-300 enabled:hover:bg-gray-50 dark:enabled:hover:bg-gray-700'}">
                  <meta.icon class="w-4 h-4" />{meta.label}
                </button>
              {/each}
            </div>
          </div>

          {#if kind !== 'embeddings'}
            <label class="block">
              <span class="field-label">Maximum output tokens</span>
              <input form="playground-form" class="input" type="number" min="1" max={account?.model.max_output_tokens ?? 1} bind:value={maxTokens} required disabled={busy || !account} />
              {#if account}<span class="field-hint block">Up to {formatCount(account.model.max_output_tokens)} for this model.</span>{/if}
            </label>
          {/if}
        </div>
      </Card>
      {#if account}<ServingProfile model={account.model} compact />{/if}
    </div>

    <div class="space-y-4 min-w-0">
      <Card icon={kindMeta[kind].icon} title={kind === 'embeddings' ? 'Embedding inputs' : 'Prompt'} subtitle={account ? account.model.id : 'Choose a provider account'}>
        {#snippet actions()}
          <span class="{statusView.cls} text-xs" role="status"><statusView.icon class="w-3 h-3 {statusView.spin ? 'animate-spin' : ''}" />{status}</span>
        {/snippet}
        <form id="playground-form" class="space-y-3" onsubmit={e => { e.preventDefault(); void generate(); }}>
          <label class="block">
            <span class="field-label">{kind === 'embeddings' ? 'Text inputs — one per line, up to 32' : 'Prompt'}</span>
            <textarea class="input font-sans resize-y min-h-40" bind:value={prompt} rows="7" required maxlength="100000" disabled={busy}
              placeholder={kind === 'embeddings' ? 'One input per line' : kind === 'chat' ? 'Ask the model something…' : 'Text to continue…'}></textarea>
          </label>
          {#if kind === 'embeddings'}<p class="field-hint">Embeddings are non-streamed. The preview shows the first eight components; SDK responses contain complete vectors. If you leave before completion, inspect the request and charge in Usage.</p>{/if}
          <div class="flex flex-wrap items-center gap-2">
            <button class="btn-primary" disabled={busy || !usable}>
              {#if busy}<Loader2 class="w-4 h-4 animate-spin" />{:else}<Send class="w-4 h-4" />{/if}{kind === 'embeddings' ? 'Embed text' : 'Generate'}
            </button>
            {#if kind !== 'embeddings'}<button type="button" class="btn-secondary" disabled={!busy} onclick={cancel}><Square class="w-4 h-4" />Cancel stream</button>{/if}
          </div>
        </form>
      </Card>

      {#if error}<Callout kind="error" role="alert">{error}</Callout>{/if}

      {#if kind !== 'embeddings'}
        <Card icon={Sparkles} tone="purple" title="Model response" subtitle="Rendered as plain text">
          {#if output}
            <div class="subpanel p-4 max-h-[32rem] overflow-auto"><pre class="model-output">{output}</pre></div>
          {:else}
            <p class="text-sm text-gray-400 dark:text-gray-500 italic">{busy ? 'Waiting for the first tokens…' : 'Responses stream here.'}</p>
          {/if}
        </Card>
      {/if}
      <Result value={embeddingPreview} title="Embedding preview — first eight components" />
      {#if !requestId && status === 'Failed — inspect Usage before retrying'}
        <div class="card px-4 py-3 text-xs text-gray-600 dark:text-gray-400">
          No request ID was received, so this request cannot be reconciled here, and it may still have reached the provider. Do not send it again: open <a class="link" href="#/usage">Usage › Requests</a>, where this account's local request keys are listed, and read its status there before retrying.
        </div>
      {/if}
      {#if requestId}
        <div class="card px-4 py-3 flex flex-wrap items-center justify-between gap-3">
          <p class="text-xs text-gray-600 dark:text-gray-400 min-w-0">Request: <span class="mono">{requestId}</span></p>
          <button class="btn-secondary btn-sm" onclick={checkRequest}><ReceiptText class="w-3.5 h-3.5" />Reconcile request status and billing</button>
        </div>
      {/if}
      <Result value={usage} title="Usage / request accounting" />
      {#if !account && accounts.length}<p class="text-sm text-gray-500 flex items-center gap-2"><Store class="w-4 h-4" />Choose an account to start.</p>{/if}
    </div>
  </div>
</section>
