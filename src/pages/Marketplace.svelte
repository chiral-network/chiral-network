<script lang="ts">
  import { command, errorText, formatCfx, parseCfx, type Offer, type Signed, type Quote, type Account } from '$lib/native';
  import { session } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import Result from '$lib/components/Result.svelte';
  import ServingProfile from '$lib/components/ServingProfile.svelte';
  let providerInfo = $state<unknown>(null);
  let offers = $state<Signed<Offer>[]>([]), selected = $state<Signed<Offer> | null>(null);
  let quote = $state<Signed<Quote> | null>(null), account = $state<Account | null>(null);
  let model = $state(''), amount = $state(''), accepted = $state(false), busy = $state(false);
  let searched = $state(false), error = $state(''), pendingId = $state('');
  let fundingStage = $state('');
  async function readFundingStage() {
    if (!pendingId) { fundingStage = ''; return; }
    try {
      const journal = await command<{ pending: { contract_id: string; funding_stage: string }[] }>('pending_accounts');
      fundingStage = journal.pending.find(item => item.contract_id === pendingId)?.funding_stage.replaceAll('_', ' ') ?? 'unresolved';
    } catch { fundingStage = 'journal unavailable'; }
  }
  async function discover() {
    busy = true; error = ''; selected = null; quote = null; account = null;
    try { offers = await command<Signed<Offer>[]>('discover', { model: model || null }); searched = true; }
    catch (e) { error = errorText(e); } finally { busy = false; }
  }
  async function select(offer: Signed<Offer>) {
    selected = offer; amount = formatCfx(offer.payload.min_funding_wei); quote = null; account = null; accepted = false; error = ''; pendingId = ''; providerInfo = null;
    busy = true;
    try { providerInfo = await command('provider_info', { offer }); }
    catch (e) { error = errorText(e); } finally { busy = false; }
  }
  async function propose() {
    busy = true; error = ''; quote = null; accepted = false;
    try { quote = await command<Signed<Quote>>('quote', { offer: selected, funding_wei: parseCfx(amount) }); }
    catch (e) { error = errorText(e); } finally { busy = false; }
  }
  async function fund() {
    if (!quote || !selected || !accepted) return;
    busy = true; error = ''; pendingId = quote.payload.id;
    try { account = await command<Account>('fund', { offer: selected, quote }); pendingId = ''; toasts.notify('funding', 'Provider account opened', 'success'); }
    catch (e) { error = errorText(e); } finally { await readFundingStage(); busy = false; }
  }
  async function resume() {
    busy = true; error = '';
    try { account = await command<Account>('resume', { contract_id: pendingId }); pendingId = ''; }
    catch (e) { error = errorText(e); } finally { await readFundingStage(); busy = false; }
  }
</script>
<section aria-labelledby="market-title">
  <h1 id="market-title">Model marketplace</h1>
  <p>Discover signed offers, verified by the native consumer against your configured network. Discovery is a bounded candidate search, not an exhaustive directory.</p>
  <form class="row" onsubmit={e => { e.preventDefault(); void discover(); }}>
    <label>Model filter<input bind:value={model} placeholder="All models" /></label>
    <button disabled={busy || !$session.configured}>{busy ? 'Working…' : 'Discover providers'}</button>
  </form>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if searched && !offers.length}<p class="empty">No verified, unexpired offers found. Check the catalog/DHT configuration or try another model.</p>{/if}
  <div class="grid">
    {#each offers as offer}
      <article class="panel">
        <h2>{offer.payload.model.id}</h2><p class="mono">{offer.payload.provider_wallet}</p>
        <p>{offer.payload.endpoint}</p>
        <dl><dt>Input / 1k tokens</dt><dd>{formatCfx(offer.payload.price.input_wei_per_1k)} CFX</dd><dt>Output / 1k tokens</dt><dd>{formatCfx(offer.payload.price.output_wei_per_1k)} CFX</dd><dt>Context</dt><dd>{offer.payload.model.context_length} tokens</dd><dt>Capabilities</dt><dd>{offer.payload.model.capabilities.join(', ')}</dd><dt>Revision</dt><dd class="mono">{offer.payload.model.revision}</dd><dt>Expires</dt><dd>{new Date(offer.payload.expires_at * 1000).toLocaleString()}</dd></dl>
        <ServingProfile model={offer.payload.model} />
        <p class="mono">Conflux eSpace testnet {offer.payload.network.chain_id} · {offer.payload.network.deployment_id}<br />Genesis {offer.payload.network.genesis_hash}</p>
        <button onclick={() => select(offer)} disabled={busy}>Inspect provider and funding terms</button>
      </article>
    {/each}
  </div>
  {#if selected}
    <section class="panel" aria-label="Provider funding">
      <h2>Prepay {selected.payload.model.id}</h2>
      <p class="warning">Funds are non-refundable and usable only with provider <strong class="mono">{selected.payload.provider_wallet}</strong> at {selected.payload.endpoint}. Start small. The provider receives your prompts in plaintext; signed metadata does not prove model execution. The provider remits a {selected.payload.fee_bps / 100}% platform fee. Network gas is additional. Chain reorganizations can freeze or reverse credit.</p>
      <Result value={providerInfo} title="Observed readiness and experimental payment-backed reputation" />
      <p>Readiness is a point-in-time probe, not a guarantee. Subjective reputation is separate from availability, scoped to this testnet and weakened by faucet-funded identities.</p>
      <form class="row" onsubmit={e => { e.preventDefault(); void propose(); }}>
        <label>Funding amount (CFX)<input inputmode="decimal" bind:value={amount} required disabled={busy || !!pendingId} /></label>
        <button disabled={busy || !!pendingId || !$session.address}>Request signed quote</button>
      </form>
      {#if quote}
        <dl><dt>Quoted funding</dt><dd>{formatCfx(quote.payload.funding_wei)} CFX</dd><dt>Contract</dt><dd class="mono">{quote.payload.id}</dd><dt>Quote expiry</dt><dd>{new Date(quote.payload.expires_at * 1000).toLocaleString()}</dd></dl>
        <p>The provider must acknowledge the signed funding intent before payment is broadcast. An acknowledged intent retains these prices while settlement is pending. Credit requires a successful canonical receipt at eSpace's safe frontier; execution alone is not spendable credit or PoS finality.</p>
        <label class="check"><input type="checkbox" bind:checked={accepted} disabled={busy || !!pendingId} />I accept this provider-specific non-refundable prepayment and residual reorg risk.</label>
        <button disabled={busy || !accepted || !!pendingId} onclick={fund}>{busy ? 'Registering or settling funding…' : 'Register intent, fund, and open account'}</button>
      {/if}
      {#if pendingId}<p class="warning" role="status">Funding stage: {fundingStage || 'preparing'}. Do not send another payment. Resume this journaled intent; payment is broadcast only after the provider’s signed acknowledgement.</p><button disabled={busy} onclick={resume}>Resume funding</button>{/if}
      {#if account}<p class="success" role="status">Account {account.status}. Available: {formatCfx(account.available_wei)} CFX.</p><a href="#/playground">Open playground</a> · <a href="#/usage">Manage API credentials</a>{/if}
    </section>
  {/if}
</section>
