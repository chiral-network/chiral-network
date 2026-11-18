<script lang="ts">
  import { tick } from 'svelte';
  import { Search, Store, Bot, Coins, ArrowDownToLine, ArrowUpFromLine, Loader2, ArrowRight, CircleDollarSign, Activity, HeartPulse, Star, FileSignature, ShieldCheck, Sparkles, KeyRound, RotateCcw, Globe, Check } from 'lucide-svelte';
  import { command, errorText, formatCfx, isCanonicalWei, parseCfx, type Offer, type Signed, type Quote, type Account } from '$lib/native';
  import { session } from '$lib/stores';
  import { stageLabel, unresolvedPayments, type PendingFunding } from '$lib/funding';
  import { scrollBehavior } from '$lib/motion';
  import { toasts } from '$lib/toastStore';
  import Result from '$lib/components/Result.svelte';
  import ServingProfile from '$lib/components/ServingProfile.svelte';
  import PageHeader from '$lib/components/ui/PageHeader.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import StatTile from '$lib/components/ui/StatTile.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  import EmptyState from '$lib/components/ui/EmptyState.svelte';
  import { shortAddress } from '$lib/components/ui/tones';
  let providerInfo = $state<unknown>(null);
  let offers = $state<Signed<Offer>[]>([]), selected = $state<Signed<Offer> | null>(null);
  let quote = $state<Signed<Quote> | null>(null), account = $state<Account | null>(null);
  let model = $state(''), amount = $state(''), accepted = $state(false), busy = $state(false);
  let searched = $state(false), error = $state(''), pendingId = $state('');
  let journal = $state<PendingFunding[]>([]), journalRead = $state(false), journalUnavailable = $state(false);
  let fundingPanel = $state<HTMLElement>();
  // Every journaled paid operation that is still unresolved, including one left by an
  // earlier session: while any exists, no new quote or payment is offered (A.1.7).
  const unresolved = $derived(unresolvedPayments(journal));
  const blocked = $derived(!journalRead || journalUnavailable || unresolved.length > 0);
  const fundingStage = $derived(!pendingId ? '' : journalUnavailable ? 'journal unavailable'
    : stageLabel(journal.find(item => item.contract_id === pendingId)?.funding_stage ?? 'unresolved'));
  // Re-read the native journal; a failure keeps the native text beside any action error.
  async function readJournal() {
    try {
      journal = (await command<{ pending: PendingFunding[] }>('pending_accounts')).pending;
      journalUnavailable = false;
    } catch (e) {
      journalUnavailable = true;
      error = [error, errorText(e)].filter(Boolean).join('\n');
    } finally { journalRead = true; }
  }
  $effect(() => {
    if ($session.configured) void readJournal();
    else { journal = []; journalRead = false; journalUnavailable = false; }
  });
  async function discover() {
    busy = true; error = ''; selected = null; quote = null; account = null;
    try { offers = await command<Signed<Offer>[]>('discover', { model: model || null }); searched = true; }
    catch (e) { error = errorText(e); } finally { busy = false; }
  }
  async function select(offer: Signed<Offer>) {
    selected = offer; amount = formatCfx(offer.payload.min_funding_wei); quote = null; account = null; accepted = false; error = ''; pendingId = ''; providerInfo = null;
    busy = true;
    await tick(); fundingPanel?.scrollIntoView({ behavior: scrollBehavior(), block: 'start' });
    try { providerInfo = await command('provider_info', { offer }); }
    catch (e) { error = errorText(e); } finally { busy = false; }
  }
  async function propose() {
    if (blocked) return;
    busy = true; error = ''; quote = null; accepted = false;
    try { quote = await command<Signed<Quote>>('quote', { offer: selected, funding_wei: parseCfx(amount) }); }
    catch (e) { error = errorText(e); } finally { busy = false; }
  }
  async function fund() {
    if (!quote || !selected || !accepted || blocked) return;
    busy = true; error = ''; pendingId = quote.payload.id;
    try { account = await command<Account>('fund', { offer: selected, quote }); pendingId = ''; toasts.notify('funding', 'Provider account opened', 'success'); }
    catch (e) { error = errorText(e); } finally { await readJournal(); busy = false; }
  }
  async function resume() {
    busy = true; error = '';
    try { account = await command<Account>('resume', { contract_id: pendingId }); pendingId = ''; }
    catch (e) { error = errorText(e); } finally { await readJournal(); busy = false; }
  }
  // Resume an operation journaled earlier, possibly by another session or before a restart.
  async function resumeJournaled(contractId: string) {
    busy = true; error = '';
    try {
      const resumed = await command<Account>('resume', { contract_id: contractId });
      if (contractId === quote?.payload.id) { account = resumed; pendingId = ''; }
      toasts.show(`Journaled funding resumed: account ${resumed.status}`, 'success');
    } catch (e) { error = errorText(e); } finally { await readJournal(); busy = false; }
  }

  // ---- presentation helpers (read-only) ----
  function lowest(field: 'input_wei_per_1k' | 'output_wei_per_1k'): string {
    const values = offers.map(o => o.payload.price[field]).filter(isCanonicalWei).map(v => BigInt(v));
    if (!values.length) return '—';
    return `${formatCfx(values.reduce((a, b) => (b < a ? b : a)).toString())} CFX`;
  }
  const modelCount = $derived(new Set(offers.map(o => o.payload.model.id)).size);
  type Probe = { reachable?: boolean; live?: boolean; ready?: boolean; error?: string };
  type Info = { observed_at?: number; health?: Probe; readiness?: Probe; reputation?: { available?: boolean; error?: string; snapshot?: { elo?: number; vote_count?: number } } };
  const info = $derived(providerInfo && typeof providerInfo === 'object' ? providerInfo as Info : null);
  const step = $derived(account ? 3 : quote ? 2 : 1);
</script>

<section aria-labelledby="market-title" class="page">
  <PageHeader id="market-title" title="Model marketplace" subtitle="Discover signed offers, verified by the native consumer against your configured network." />

  <div class="grid grid-cols-2 lg:grid-cols-4 gap-2">
    <StatTile icon={Store} label="Verified offers" value={searched ? offers.length.toLocaleString() : '—'} />
    <StatTile icon={Bot} tone="purple" label="Models" value={searched ? modelCount.toLocaleString() : '—'} />
    <StatTile icon={ArrowDownToLine} tone="green" label="Lowest input / 1k" value={lowest('input_wei_per_1k')} />
    <StatTile icon={ArrowUpFromLine} tone="orange" label="Lowest output / 1k" value={lowest('output_wei_per_1k')} />
  </div>

  <Card icon={Search} title="Discover providers" subtitle="Discovery is a bounded candidate search, not an exhaustive directory.">
    <form class="flex flex-col sm:flex-row gap-2 sm:items-end" onsubmit={e => { e.preventDefault(); void discover(); }}>
      <label class="flex-1 min-w-0">
        <span class="field-label">Model filter</span>
        <span class="relative block">
          <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400 pointer-events-none" />
          <input class="input pl-9" bind:value={model} placeholder="All models" />
        </span>
      </label>
      <button class="btn-primary py-2.5" disabled={busy || !$session.configured}>
        {#if busy}<Loader2 class="w-4 h-4 animate-spin" />Working…{:else}<Search class="w-4 h-4" />Discover providers{/if}
      </button>
    </form>
  </Card>

  {#if error}<Callout kind="error" role="alert">{error}</Callout>{/if}

  {#if unresolved.some(item => item.contract_id !== pendingId)}
    <Callout kind="warning" role="status" title="Unresolved funding in the native journal">
      Do not send another payment. New quotes and funding stay disabled until each journaled payment below is resumed or reconciled under Usage › Funding &amp; recovery. An error or a restart does not prove that no money moved.
      <ul class="mt-3 space-y-2">
        {#each unresolved.filter(item => item.contract_id !== pendingId) as item (item.contract_id)}
          <li class="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span class="font-mono break-all">{item.contract_id}</span>
            <span>{item.topup_wei !== null ? 'Top-up' : 'Opening'}{item.model ? ` · ${item.model}` : ''} · stage: {stageLabel(item.funding_stage)}</span>
            {#if item.tx_hash}<span class="font-mono break-all text-xs">tx {item.tx_hash}</span>{/if}
            <button class="btn-secondary btn-sm" disabled={busy} onclick={() => resumeJournaled(item.contract_id)}><RotateCcw class="w-3.5 h-3.5" />Resume</button>
          </li>
        {/each}
      </ul>
      <div class="mt-3"><a class="link" href="#/usage">Open Usage › Funding &amp; recovery</a></div>
    </Callout>
  {:else if $session.configured && journalUnavailable}
    <Callout kind="warning" role="status" title="Funding journal unavailable">New quotes and funding stay disabled until the native funding journal can be read, because an unresolved payment could not be ruled out.</Callout>
  {/if}

  {#if searched && !offers.length}
    <div class="card"><EmptyState icon={Store} title="No verified, unexpired offers found">Check the catalog/DHT configuration or try another model.</EmptyState></div>
  {:else if !searched}
    <div class="card"><EmptyState icon={Sparkles} title="Find a model to run">Search the network for signed provider offers. Offers are verified natively against your configured chain identity before they appear here.</EmptyState></div>
  {/if}

  {#if offers.length}
    <div class="grid grid-cols-1 xl:grid-cols-2 gap-4">
      {#each offers as offer}
        {@const p = offer.payload}
        <article class="card p-5 flex flex-col gap-4 {selected === offer ? 'ring-2 ring-primary-500 border-primary-300 dark:border-primary-700' : ''}">
          <div class="flex items-start justify-between gap-3">
            <div class="flex items-center gap-3 min-w-0">
              <div class="p-2 rounded-lg shrink-0 bg-primary-100 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400"><Bot class="w-5 h-5" /></div>
              <div class="min-w-0">
                <h2 class="card-title break-all">{p.model.id}</h2>
                <p class="text-xs font-mono text-gray-500 dark:text-gray-400 truncate" title={p.provider_wallet}>{p.provider_wallet}</p>
              </div>
            </div>
            <span class="badge-gray shrink-0" title="Offer revision">rev {p.revision}</span>
          </div>

          <p class="flex items-center gap-1.5 text-xs text-gray-600 dark:text-gray-400 break-all"><Globe class="w-3.5 h-3.5 shrink-0" />{p.endpoint}</p>

          <div class="grid grid-cols-3 gap-2">
            <div class="subpanel px-3 py-2"><div class="stat-label">Input / 1k tokens</div><div class="stat-value">{formatCfx(p.price.input_wei_per_1k)} CFX</div></div>
            <div class="subpanel px-3 py-2"><div class="stat-label">Output / 1k tokens</div><div class="stat-value">{formatCfx(p.price.output_wei_per_1k)} CFX</div></div>
            <div class="subpanel px-3 py-2"><div class="stat-label">Context</div><div class="stat-value">{p.model.context_length.toLocaleString()} tokens</div></div>
          </div>

          <div class="flex flex-wrap items-center gap-1.5">
            <span class="stat-label mr-1">Capabilities</span>
            {#each p.model.capabilities as capability}<span class="badge-primary">{capability}</span>{/each}
          </div>

          <dl class="kv text-xs">
            <dt>Revision</dt><dd class="mono">{p.model.revision}</dd>
            <dt>Minimum funding</dt><dd>{formatCfx(p.min_funding_wei)} CFX</dd>
            <dt>Expires</dt><dd>{new Date(p.expires_at * 1000).toLocaleString()}</dd>
          </dl>

          <ServingProfile model={p.model} compact />

          <p class="mono text-gray-500 dark:text-gray-400">Conflux eSpace testnet {p.network.chain_id} · {p.network.deployment_id}<br />Genesis {p.network.genesis_hash}</p>

          <button class="btn-primary w-full mt-auto" onclick={() => select(offer)} disabled={busy}>
            Inspect provider and funding terms<ArrowRight class="w-4 h-4" />
          </button>
        </article>
      {/each}
    </div>
  {/if}

  {#if selected}
    {@const s = selected.payload}
    <section bind:this={fundingPanel} class="card overflow-hidden scroll-mt-20" aria-label="Provider funding">
      <div class="bg-gradient-to-r from-primary-600 to-primary-700 px-6 py-5 text-white">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-center gap-3 min-w-0">
            <div class="p-2.5 bg-white/20 rounded-full shrink-0"><CircleDollarSign class="w-6 h-6" /></div>
            <div class="min-w-0">
              <h2 class="text-xl font-semibold break-all">Prepay {s.model.id}</h2>
              <p class="text-primary-100 text-sm font-mono truncate" title={s.provider_wallet}>{shortAddress(s.provider_wallet, 10, 8)} · {s.endpoint}</p>
            </div>
          </div>
          <ol class="flex items-center gap-2 text-xs font-medium" aria-label="Funding progress">
            {#each ['Quote', 'Fund', 'Open'] as label, i}
              <li class="flex items-center gap-1.5 px-2.5 py-1 rounded-full {step === i + 1 ? 'bg-white text-primary-700 shadow-sm' : step > i + 1 ? 'bg-white/25 text-white' : 'bg-white/10 text-primary-100'}" aria-current={step === i + 1 ? 'step' : undefined}>
                <span class="w-4 h-4 rounded-full grid place-items-center text-[10px] {step === i + 1 ? 'bg-primary-600 text-white' : 'bg-white/25'}">{#if step > i + 1}<Check class="w-3 h-3" />{:else}{i + 1}{/if}</span>{label}
              </li>
            {/each}
          </ol>
        </div>
      </div>

      <div class="p-6 space-y-5">
        <Callout kind="warning" title="Non-refundable, provider-specific prepayment">
          Funds are non-refundable and usable only with provider <strong class="font-mono break-all">{s.provider_wallet}</strong> at {s.endpoint}. Start small. The provider receives your prompts in plaintext; signed metadata does not prove model execution. The provider remits a {s.fee_bps / 100}% platform fee. Network gas is additional. Chain reorganizations can freeze or reverse credit.
        </Callout>

        <div class="space-y-3">
          <h3 class="font-semibold text-gray-900 dark:text-white flex items-center gap-2"><Activity class="w-4 h-4 text-primary-600 dark:text-primary-400" />Observed readiness and experimental payment-backed reputation</h3>
          {#if busy && !info}
            <p class="flex items-center gap-2 text-sm text-gray-500"><Loader2 class="w-4 h-4 animate-spin" />Probing provider…</p>
          {:else if info}
            <div class="grid grid-cols-1 sm:grid-cols-3 gap-2">
              <div class="subpanel px-3 py-2.5 flex items-center gap-3">
                <HeartPulse class="w-4 h-4 shrink-0 {info.health?.live ? 'text-green-600' : 'text-red-500'}" />
                <div class="min-w-0"><div class="stat-label">Health</div><div class="text-sm font-medium">{info.health?.live ? 'Live' : info.health?.reachable ? 'Reachable, not live' : 'Unreachable'}</div>{#if info.health?.error}<div class="text-xs text-gray-500 truncate" title={info.health.error}>{info.health.error}</div>{/if}</div>
              </div>
              <div class="subpanel px-3 py-2.5 flex items-center gap-3">
                <ShieldCheck class="w-4 h-4 shrink-0 {info.readiness?.ready ? 'text-green-600' : 'text-amber-500'}" />
                <div class="min-w-0"><div class="stat-label">Readiness</div><div class="text-sm font-medium">{info.readiness?.ready ? 'Ready' : info.readiness?.reachable ? 'Not ready' : 'Unreachable'}</div>{#if info.readiness?.error}<div class="text-xs text-gray-500 truncate" title={info.readiness.error}>{info.readiness.error}</div>{/if}</div>
              </div>
              <div class="subpanel px-3 py-2.5 flex items-center gap-3">
                <Star class="w-4 h-4 shrink-0 {info.reputation?.available ? 'text-amber-500' : 'text-gray-400'}" />
                <div class="min-w-0"><div class="stat-label">Reputation (testnet)</div>
                  {#if info.reputation?.available && info.reputation.snapshot}
                    <div class="text-sm font-medium tabular-nums">Elo {info.reputation.snapshot.elo?.toFixed(1)} <span class="text-xs text-gray-500">· {info.reputation.snapshot.vote_count} votes</span></div>
                  {:else}<div class="text-sm font-medium">Unavailable</div>{#if info.reputation?.error}<div class="text-xs text-gray-500 truncate" title={info.reputation.error}>{info.reputation.error}</div>{/if}{/if}
                </div>
              </div>
            </div>
          {/if}
          <p class="prose-muted text-xs">Readiness is a point-in-time probe, not a guarantee. Subjective reputation is separate from availability, scoped to this testnet and weakened by faucet-funded identities.</p>
          <Result value={providerInfo} title="Raw readiness and reputation response" open={false} />
        </div>

        <div class="border-t border-gray-200 dark:border-gray-700 pt-5 space-y-4">
          <h3 class="font-semibold text-gray-900 dark:text-white flex items-center gap-2"><Coins class="w-4 h-4 text-primary-600 dark:text-primary-400" />1 · Request a signed quote</h3>
          <form class="flex flex-col sm:flex-row gap-2 sm:items-end" onsubmit={e => { e.preventDefault(); void propose(); }}>
            <label class="flex-1 min-w-0 max-w-sm">
              <span class="field-label">Funding amount (CFX)</span>
              <span class="relative block">
                <input class="input pr-14" inputmode="decimal" bind:value={amount} required disabled={busy || !!pendingId || blocked} />
                <span class="absolute right-3 top-1/2 -translate-y-1/2 text-xs font-semibold text-gray-400">CFX</span>
              </span>
            </label>
            <button class="btn-secondary py-2.5" disabled={busy || !!pendingId || blocked || !$session.address}><FileSignature class="w-4 h-4" />Request signed quote</button>
          </form>
        </div>

        {#if quote}
          <div class="border-t border-gray-200 dark:border-gray-700 pt-5 space-y-4">
            <h3 class="font-semibold text-gray-900 dark:text-white flex items-center gap-2"><ShieldCheck class="w-4 h-4 text-primary-600 dark:text-primary-400" />2 · Review and fund</h3>
            <div class="subpanel p-4">
              <dl class="kv">
                <dt>Quoted funding</dt><dd class="text-lg font-bold text-primary-600 dark:text-primary-400 tabular-nums">{formatCfx(quote.payload.funding_wei)} CFX</dd>
                <dt>Contract</dt><dd class="mono">{quote.payload.id}</dd>
                <dt>Quote expiry</dt><dd>{new Date(quote.payload.expires_at * 1000).toLocaleString()}</dd>
              </dl>
            </div>
            <p class="prose-muted"><strong>Residual registration risk:</strong> Your client broadcasts the payment only after a durable provider acknowledgement of the signed funding intent. The provider already receives the signed bytes, which anyone holding them can broadcast, so it could broadcast them before or without acknowledging; this ordering is not cryptographic escrow. An error, timeout or missing reply does not prove that no money moved: inspect the pending funding stage and resume instead of paying again.</p>
            <p class="prose-muted">An acknowledged intent retains these prices while settlement is pending. Credit requires a successful canonical receipt at eSpace's safe frontier; execution alone is not spendable credit or PoS finality.</p>
            <label class="check-row"><input class="checkbox mt-0.5" type="checkbox" bind:checked={accepted} disabled={busy || !!pendingId || blocked} />I accept this provider-specific non-refundable prepayment, the registration risk above, and that chain reorganizations can freeze or reverse credit.</label>
            <button class="btn-primary" disabled={busy || !accepted || !!pendingId || blocked} onclick={fund}>
              {#if busy}<Loader2 class="w-4 h-4 animate-spin" />Registering or settling funding…{:else}<CircleDollarSign class="w-4 h-4" />Register intent, fund, and open account{/if}
            </button>
          </div>
        {/if}

        {#if pendingId}
          <Callout kind="warning" role="status" title="Funding stage: {fundingStage || 'preparing'}">
            Do not send another payment. The provider already holds the signed bytes and could broadcast them before or without acknowledging; this ordering is not cryptographic escrow. An error, timeout or missing reply does not prove that no money moved. Inspect the funding stage above and resume this journaled intent; your client broadcasts only the original payment, and only after a durable provider acknowledgement.
            <div class="mt-3"><button class="btn-secondary btn-sm" disabled={busy} onclick={resume}><RotateCcw class="w-3.5 h-3.5" />Resume funding</button></div>
          </Callout>
        {/if}
        {#if account}
          <Callout kind="success" role="status" title="Account {account.status}">
            Available: {formatCfx(account.available_wei)} CFX.
            <div class="mt-3 flex flex-wrap gap-2">
              <a class="btn-primary btn-sm no-underline" href="#/playground"><Sparkles class="w-3.5 h-3.5" />Open playground</a>
              <a class="btn-secondary btn-sm no-underline" href="#/usage"><KeyRound class="w-3.5 h-3.5" />Manage API credentials</a>
            </div>
          </Callout>
        {/if}
      </div>
    </section>
  {/if}
</section>
