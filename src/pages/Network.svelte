<script lang="ts">
  import { Globe, RefreshCw, Link2, Blocks, ShieldCheck, Lock, Loader2, Settings, Hash, Clock } from 'lucide-svelte';
  import { command, errorText } from '$lib/native';
  import { session } from '$lib/stores';
  import Result from '$lib/components/Result.svelte';
  import PageHeader from '$lib/components/ui/PageHeader.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import StatTile from '$lib/components/ui/StatTile.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  let value = $state<unknown>(null), busy = $state(false), error = $state('');
  async function refresh() { busy = true; error = ''; try { value = await command('network_status'); } catch (e) { error = errorText(e); } finally { busy = false; } }
  // ---- presentation only ----
  type Block = { height?: number; hash?: string; timestamp?: number };
  type Status = { chainId?: number; genesis?: string; observedAtUnix?: number; syncing?: boolean; head?: Block; safe?: Block; finalized?: Block; acceptance?: string };
  const status = $derived(value && typeof value === 'object' ? value as Status : null);
  const height = (b?: Block) => (typeof b?.height === 'number' ? b.height.toLocaleString() : '—');
</script>

<section class="page" aria-labelledby="network-title">
  <PageHeader id="network-title" title="Network" subtitle="Native CFX testnet settlement: chain identity, settlement frontier and RPC health.">
    {#snippet actions()}
      <button class="btn-primary" disabled={busy || !$session.configured} onclick={refresh}>
        {#if busy}<Loader2 class="w-4 h-4 animate-spin" />Checking identity and frontier…{:else}<RefreshCw class="w-4 h-4" />Check configured network{/if}
      </button>
    {/snippet}
  </PageHeader>

  <div class="card p-2">
    <div class="grid grid-cols-2 lg:grid-cols-4 gap-2">
      <StatTile icon={Globe} label="Chain ID" value={status?.chainId ?? '—'} title="Conflux eSpace testnet is chain 71" />
      <StatTile icon={Blocks} tone="sky" label="Head" value={height(status?.head)} title={status?.head?.hash} />
      <StatTile icon={ShieldCheck} tone="green" label="Safe frontier" value={height(status?.safe)} title={status?.safe?.hash} />
      <StatTile icon={Lock} tone="purple" label="Finalized" value={height(status?.finalized)} title={status?.finalized?.hash} />
    </div>
  </div>

  {#if error}<Callout kind="error" role="alert">{error}</Callout>{/if}

  <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 items-start">
    <Card icon={Link2} title="Settlement identity" subtitle="Pinned by your explicit consumer profile">
      <div class="space-y-3">
        <p class="prose-muted">Native CFX testnet settlement pins Conflux eSpace chain 71, its genesis hash and an explicit deployment namespace. There is no legacy-chain fallback and no background node or miner startup.</p>
        {#if status}
          <dl class="kv subpanel p-4 text-sm">
            <dt class="flex items-center gap-1.5"><Hash class="w-3.5 h-3.5" />Genesis</dt><dd class="mono">{status.genesis ?? '—'}</dd>
            <dt>Acceptance level</dt><dd><span class="badge-green">{status.acceptance ?? '—'}</span></dd>
            <dt>Syncing</dt><dd>{status.syncing === false ? 'No' : status.syncing ? 'Yes' : '—'}</dd>
            <dt class="flex items-center gap-1.5"><Clock class="w-3.5 h-3.5" />Observed</dt><dd>{status.observedAtUnix ? new Date(status.observedAtUnix * 1000).toLocaleString() : '—'}</dd>
          </dl>
        {/if}
        <a class="link text-sm inline-flex items-center gap-1.5" href="#/settings"><Settings class="w-4 h-4" />Configure network, catalog, and DHT endpoints</a>
      </div>
    </Card>
    <Card icon={ShieldCheck} tone="amber" title="When credit becomes spendable">
      <Callout kind="warning">First execution is not spendable credit. Funding requires a canonical successful receipt at the safe frontier; finalized is reported separately. Syncing, stale or unavailable RPC nodes withhold new funding. Proven orphaned credits may freeze an account; a missing receipt alone does not prove a reversal. Existing settled balances can remain usable during temporary RPC outages.</Callout>
    </Card>
  </div>

  <Result {value} title="Native chain identity and settlement status" open={false} />
</section>
