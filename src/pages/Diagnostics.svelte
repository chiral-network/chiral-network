<script lang="ts">
  import { Bug, Cpu, Tag, Radio, KeyRound, Link2, RefreshCw, Loader2, Info } from 'lucide-svelte';
  import { command, errorText, nativeAvailable } from '$lib/native';
  import Result from '$lib/components/Result.svelte';
  import PageHeader from '$lib/components/ui/PageHeader.svelte';
  import StatTile from '$lib/components/ui/StatTile.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  let value = $state<unknown>(null), error = $state(''), busy = $state(false);
  async function inspect() { busy = true; error = ''; try { value = await command('diagnostics'); } catch (e) { error = errorText(e); } finally { busy = false; } }
  // ---- presentation only ----
  type Diag = { version?: string; active_streams?: number; wallet_loaded?: boolean; chain_status?: { Ok?: unknown; Err?: string } };
  const d = $derived(value && typeof value === 'object' ? value as Diag : null);
  const native = nativeAvailable();
  const chain = $derived(!d?.chain_status ? '—' : 'Err' in d.chain_status ? 'Unavailable' : 'Reachable');
</script>

<section class="page" aria-labelledby="diag-title">
  <PageHeader id="diag-title" title="Diagnostics" subtitle="Live status of the native runtime. No synthetic providers, balances, or success responses are supplied.">
    {#snippet actions()}
      <button class="btn-primary" disabled={busy || !native} onclick={inspect}>
        {#if busy}<Loader2 class="w-4 h-4 animate-spin" />Collecting…{:else}<RefreshCw class="w-4 h-4" />Collect native diagnostics{/if}
      </button>
    {/snippet}
  </PageHeader>

  <div class="card p-2">
    <div class="grid grid-cols-2 lg:grid-cols-5 gap-2">
      <StatTile icon={Cpu} tone={native ? 'green' : 'gray'} label="Native runtime" value={native ? 'Available' : 'Unavailable (browser preview)'} />
      <StatTile icon={Tag} label="Version" value={d?.version ? `v${d.version}` : '—'} />
      <StatTile icon={Link2} tone={chain === 'Reachable' ? 'green' : chain === 'Unavailable' ? 'red' : 'gray'} label="Configured chain" value={chain} />
      <StatTile icon={Radio} tone="purple" label="Active streams" value={d?.active_streams ?? '—'} />
      <StatTile icon={KeyRound} tone={d?.wallet_loaded ? 'green' : 'gray'} label="Wallet" value={d ? (d.wallet_loaded ? 'Loaded' : 'Not loaded') : '—'} />
    </div>
  </div>

  {#if error}<Callout kind="error" role="alert">{error}</Callout>{/if}
  <Result {value} title="Runtime and network status" />
  {#if !value}
    <div class="card p-5 flex items-center gap-3 text-sm text-gray-500 dark:text-gray-400"><Bug class="w-5 h-5" />Collect diagnostics to inspect the runtime and configured network.</div>
  {/if}

  <Callout kind="info">For request billing, use <a class="link" href="#/usage">Usage</a> or the playground request-status action. For engine readiness, queue failures and fee-remittance liabilities, use the authenticated <a class="link" href="#/provider">Provider dashboard</a>. Diagnostics do not contain wallet keys, API bearer secrets, prompts, or generated text.</Callout>
</section>
