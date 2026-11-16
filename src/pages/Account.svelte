<script lang="ts">
  import { CircleUser, RefreshCw, Bot, Wallet, Store, ArrowRight, Layers, CheckCircle2, Loader2, Info } from 'lucide-svelte';
  import { command, errorText, formatCfx, type Account } from '$lib/native';
  import { session } from '$lib/stores';
  import PageHeader from '$lib/components/ui/PageHeader.svelte';
  import StatTile from '$lib/components/ui/StatTile.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  import EmptyState from '$lib/components/ui/EmptyState.svelte';
  import { statusBadge } from '$lib/components/ui/tones';
  let accounts = $state<Account[]>([]), error = $state(''), busy = $state(false), loaded = $state(false);
  async function refresh() { busy = true; error = ''; try { accounts = await command<Account[]>('accounts'); loaded = true; } catch (e) { error = errorText(e); } finally { busy = false; } }
  const activeCount = $derived(accounts.filter(a => ['active', 'free'].includes(a.status)).length);
</script>

<section class="page" aria-labelledby="account-title">
  <PageHeader id="account-title" title="Account" subtitle="Your wallet identity and the provider accounts it has opened.">
    {#snippet actions()}
      <button class="btn-primary" disabled={busy || !$session.configured} onclick={refresh}>
        {#if busy}<Loader2 class="w-4 h-4 animate-spin" />Loading…{:else}<RefreshCw class="w-4 h-4" />Load local account index{/if}
      </button>
    {/snippet}
  </PageHeader>

  <div class="card overflow-hidden">
    <div class="bg-gradient-to-r from-primary-600 to-primary-700 p-6 text-white flex flex-wrap items-center justify-between gap-4">
      <div class="flex items-center gap-3 min-w-0">
        <div class="p-3 bg-white/20 rounded-full shrink-0"><CircleUser class="w-8 h-8" /></div>
        <div class="min-w-0">
          <h2 class="text-xl font-semibold">Wallet identity</h2>
          <p class="font-mono text-sm text-primary-100 break-all">{$session.address ?? 'No native wallet loaded'}</p>
        </div>
      </div>
      <div class="flex flex-wrap gap-2">
        <a class="px-3 py-1.5 rounded-lg bg-white/15 hover:bg-white/25 text-sm font-medium inline-flex items-center gap-1.5 no-underline" href="#/wallet"><Wallet class="w-4 h-4" />Wallet and private backup</a>
        <a class="px-3 py-1.5 rounded-lg bg-white text-primary-700 hover:bg-primary-50 text-sm font-medium inline-flex items-center gap-1.5 no-underline" href="#/marketplace"><Store class="w-4 h-4" />Discover providers</a>
      </div>
    </div>
    <div class="px-6 py-4">
      <p class="prose-muted flex items-start gap-2"><Info class="w-4 h-4 mt-0.5 shrink-0 text-primary-500" />Provider balances are separate, non-refundable accounts. They cannot be transferred between providers or treated as wallet funds.</p>
    </div>
  </div>

  {#if loaded}
    <div class="grid grid-cols-2 gap-2 max-w-xl">
      <StatTile icon={Layers} label="Provider accounts" value={accounts.length.toLocaleString()} />
      <StatTile icon={CheckCircle2} tone="green" label="Active or free" value={activeCount.toLocaleString()} />
    </div>
  {/if}

  {#if error}<Callout kind="error" role="alert">{error}</Callout>{/if}
  {#if loaded && !accounts.length}<div class="card"><EmptyState icon={Layers} title="No accounts in this deployment namespace"><a class="link" href="#/marketplace">Discover a provider</a> to open one.</EmptyState></div>{/if}

  {#if accounts.length}
    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
      {#each accounts as account}
        <article class="card p-5 flex flex-col gap-3">
          <div class="flex items-start justify-between gap-3">
            <div class="flex items-center gap-3 min-w-0">
              <div class="p-2 rounded-lg shrink-0 bg-primary-100 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400"><Bot class="w-5 h-5" /></div>
              <div class="min-w-0"><h2 class="card-title break-all">{account.model.id}</h2><p class="text-xs font-mono text-gray-500 dark:text-gray-400 truncate" title={account.provider_wallet}>{account.provider_wallet}</p></div>
            </div>
            <span class={statusBadge(account.status)} title="Last recorded status">{account.status}</span>
          </div>
          <div class="subpanel px-3 py-2.5">
            <div class="stat-label">Available</div>
            <div class="text-lg font-bold text-gray-900 dark:text-white tabular-nums break-all">{formatCfx(account.available_wei)} <span class="text-sm font-medium text-gray-500">CFX</span></div>
          </div>
          <p class="mono text-gray-500 dark:text-gray-400">{account.contract_id}</p>
          <a class="link text-sm mt-auto inline-flex items-center gap-1" href="#/usage">Refresh live balance, usage, receipt, and credentials<ArrowRight class="w-3.5 h-3.5" /></a>
        </article>
      {/each}
    </div>
  {/if}
</section>
