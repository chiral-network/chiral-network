<script lang="ts">
  import { Wallet, RefreshCw, History, Copy, Check, ShieldCheck, Download, Droplets, Plus, Upload, KeyRound, Loader2, Inbox, ExternalLink } from 'lucide-svelte';
  import { command, errorText, formatCfx } from '$lib/native';
  import { session } from '$lib/stores';
  import Result from '$lib/components/Result.svelte';
  import PageHeader from '$lib/components/ui/PageHeader.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  import { statusBadge, shortAddress } from '$lib/components/ui/tones';
  let importPath = $state(''), backupPath = $state(''), busy = $state(false), error = $state(''), notice = $state('');
  let balance = $state<unknown>(null), history = $state<unknown>(null);
  let profile = $state<unknown>(null);
  async function run(operation: string, args: Record<string, unknown> = {}) {
    busy = true; error = ''; notice = ''; running = operation;
    try {
      const result = await command(operation, args);
      if (operation === 'wallet_create' || operation === 'wallet_import') { session.update(s => ({ ...s, address: String(result) })); notice = 'Wallet ready. Signing keys stay in protected native storage.'; }
      if (operation === 'balance') balance = result;
      if (operation === 'history') history = result;
      if (operation === 'network_profile') profile = result;
      if (operation === 'wallet_export') notice = 'Private backup file written with owner-only permissions. Chiral does not encrypt this file; keep it offline on encrypted storage.';
    } catch (e) { error = errorText(e); } finally { busy = false; running = ''; }
  }
  // ---- presentation only ----
  let running = $state(''), copied = $state(false);
  type Tx = { operation_id?: string; tx_hash?: string; to?: string; value_wei?: string; nonce?: number; status?: string; error?: string | null; created_at?: number };
  const balanceWei = $derived(balance && typeof balance === 'object' && typeof (balance as { balance_wei?: unknown }).balance_wei === 'string' ? (balance as { balance_wei: string }).balance_wei : null);
  const transactions = $derived(history && typeof history === 'object' && Array.isArray((history as { transactions?: unknown }).transactions) ? (history as { transactions: Tx[] }).transactions : null);
  const links = $derived(profile && typeof profile === 'object' ? profile as { faucet_url?: string | null; explorer_url?: string | null; rpc_url?: string } : null);
  async function copyAddress() {
    if (!$session.address) return;
    try { await navigator.clipboard.writeText($session.address); copied = true; setTimeout(() => copied = false, 1500); } catch { /* clipboard unavailable */ }
  }
</script>

{#snippet faucet()}
<Card icon={Droplets} tone="sky" title="eSpace testnet funding" subtitle="Testnet funds have no mainnet value">
  <div class="space-y-3">
    <p class="prose-muted">Request testnet CFX for the public hex address above using the configured Conflux eSpace faucet. Reserve enough for network gas as well as small provider prepayments. Never send a wallet backup or private key to a faucet. No faucet authority is embedded in this app.</p>
    <button class="btn-secondary" disabled={busy || !$session.configured} onclick={() => run('network_profile')}><ExternalLink class="w-4 h-4" />Show configured faucet and explorer</button>
    {#if links}
      <dl class="kv subpanel p-3 text-xs">
        <dt>Faucet</dt><dd class="mono">{links.faucet_url ?? 'Not configured'}</dd>
        <dt>Explorer</dt><dd class="mono">{links.explorer_url ?? 'Not configured'}</dd>
      </dl>
    {/if}
    <Result value={profile} title="Explicit eSpace profile (public endpoints only)" open={false} />
    <p class="text-xs text-gray-500 dark:text-gray-400 leading-relaxed">Mining pools normally pay Core Space addresses. Core and eSpace balances are separate: use an explicit native Core-to-eSpace transfer to fund an eSpace wallet. Changing an address's text is not a transfer.</p>
  </div>
</Card>
{/snippet}

<section class="page" aria-labelledby="wallet-title">
  <PageHeader id="wallet-title" title="Wallet" subtitle="Wallet keys never enter JavaScript. Generate or import a key through native commands; this wallet uses Conflux eSpace's hex address format." />

  {#if !$session.configured}<Callout kind="warning">Load an explicit network/consumer configuration in <a class="link" href="#/settings">Settings</a> first.</Callout>{/if}
  {#if error}<Callout kind="error" role="alert">{error}</Callout>{/if}
  {#if notice}<Callout kind="success" role="status">{notice}</Callout>{/if}

  {#if $session.address}
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 items-start">
      <div class="card overflow-hidden">
        <div class="bg-gradient-to-r from-primary-600 to-primary-700 p-6 text-white">
          <div class="flex items-center justify-between gap-3 mb-4">
            <div class="flex items-center gap-3">
              <div class="p-3 bg-white/20 rounded-full"><Wallet class="w-8 h-8" /></div>
              <div><h2 class="text-xl font-semibold">Public address</h2><p class="text-primary-100 text-sm">Conflux eSpace · keys stay in native storage</p></div>
            </div>
            <span class="px-3 py-1 bg-white/20 rounded-full text-sm">Testnet</span>
          </div>
          <div class="bg-white/10 rounded-xl p-4">
            <div class="flex items-center justify-between gap-3">
              <div class="min-w-0">
                <p class="text-primary-100 text-sm mb-1">Verified-network balance</p>
                {#if running === 'balance'}<Loader2 class="w-6 h-6 animate-spin" />
                {:else if balanceWei !== null}<div class="flex items-baseline gap-2"><span class="text-3xl font-bold tabular-nums break-all">{formatCfx(balanceWei)}</span><span class="text-primary-100">CFX</span></div>
                {:else}<div class="flex items-baseline gap-2"><span class="text-xl font-bold text-primary-200/70">--</span><span class="text-primary-200/70 text-sm">Not checked yet</span></div>{/if}
              </div>
              <button class="p-2 hover:bg-white/10 rounded-lg transition-colors disabled:opacity-50" disabled={busy} onclick={() => run('balance')} title="Refresh verified-network balance" aria-label="Refresh verified-network balance">
                <RefreshCw class="w-5 h-5 {running === 'balance' ? 'animate-spin' : ''}" />
              </button>
            </div>
          </div>
          <div class="flex items-center gap-2 mt-4">
            <span class="font-mono text-sm sm:text-base break-all" title={$session.address}>{$session.address}</span>
            <button class="p-1.5 hover:bg-white/10 rounded-lg shrink-0" onclick={copyAddress} title="Copy public address" aria-label="Copy public address">
              {#if copied}<Check class="w-4 h-4" />{:else}<Copy class="w-4 h-4" />{/if}
            </button>
            <span class="sr-only" role="status">{copied ? 'Address copied' : ''}</span>
          </div>
        </div>
        <div class="p-5 flex flex-wrap gap-2">
          <button class="btn-primary" disabled={busy} onclick={() => run('balance')}><RefreshCw class="w-4 h-4" />Refresh verified-network balance</button>
          <button class="btn-secondary" disabled={busy} onclick={() => run('history')}><History class="w-4 h-4" />Load local transaction journal</button>
        </div>
      </div>

      <div class="space-y-4">
        <Card icon={ShieldCheck} tone="amber" title="Back up wallet" subtitle="Copies the private key to a new, owner-only file">
          <form class="space-y-3" onsubmit={e => { e.preventDefault(); void run('wallet_export', { path: backupPath }); }}>
            <Callout kind="warning">It is not encrypted by Chiral: choose an encrypted offline volume. Never upload this file.</Callout>
            <label class="block"><span class="field-label">New backup file path</span><input class="input font-mono" bind:value={backupPath} required autocomplete="off" placeholder="/Volumes/encrypted/chiral-wallet.key" /></label>
            <button class="btn-secondary" disabled={busy || !backupPath}><Download class="w-4 h-4" />Export private backup to file</button>
          </form>
        </Card>
        {@render faucet()}
      </div>
    </div>

    <Card icon={History} tone="gray" title="Local submitted transactions" subtitle="This native consumer's durable submitted-transaction journal">
      <div class="space-y-3">
        <p class="prose-muted text-xs">Transaction history covers this native consumer's durable submitted-transaction journal, not an invented complete chain history. Pending, rejected, and orphaned transactions are not spendable provider credit.</p>
        {#if transactions}
          {#if transactions.length}
            <div class="overflow-x-auto rounded-xl border border-gray-200 dark:border-gray-700">
              <table class="w-full text-sm">
                <thead class="bg-gray-50 dark:bg-gray-900/40 text-left"><tr>{#each ['Transaction', 'To', 'Value', 'Nonce', 'Status', 'Created'] as h}<th class="stat-label font-medium px-3 py-2">{h}</th>{/each}</tr></thead>
                <tbody class="divide-y divide-gray-100 dark:divide-gray-700">
                  {#each transactions as tx}
                    <tr class="hover:bg-gray-50 dark:hover:bg-gray-700/30">
                      <td class="px-3 py-2 font-mono text-xs" title={tx.tx_hash}>{shortAddress(tx.tx_hash, 10, 6) || '—'}</td>
                      <td class="px-3 py-2 font-mono text-xs" title={tx.to}>{shortAddress(tx.to) || '—'}</td>
                      <td class="px-3 py-2 tabular-nums whitespace-nowrap">{tx.value_wei ? `${formatCfx(tx.value_wei)} CFX` : '—'}</td>
                      <td class="px-3 py-2 tabular-nums">{tx.nonce ?? '—'}</td>
                      <td class="px-3 py-2"><span class={statusBadge(tx.status)} title={tx.error ?? undefined}>{tx.status ?? 'unknown'}</span></td>
                      <td class="px-3 py-2 text-xs text-gray-500 whitespace-nowrap">{tx.created_at ? new Date(tx.created_at * 1000).toLocaleString() : '—'}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {:else}<p class="text-sm text-gray-500 flex items-center gap-2"><Inbox class="w-4 h-4" />No submitted transactions in the local journal.</p>{/if}
        {/if}
        <Result value={history} title="Local submitted transactions (raw)" open={false} />
        <Result value={balance} title="Network wallet balance (raw)" open={false} />
      </div>
    </Card>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-4 items-start">
      <Card icon={Plus} title="Generate native wallet" subtitle="Create a fresh key in protected native storage">
        <div class="space-y-3">
          <p class="prose-muted">Explicit key import does not convert old CHI or CHR balances into CFX.</p>
          <button class="btn-primary w-full" disabled={busy || !$session.configured} onclick={() => run('wallet_create')}>
            {#if running === 'wallet_create'}<Loader2 class="w-4 h-4 animate-spin" />{:else}<KeyRound class="w-4 h-4" />{/if}Generate native wallet
          </button>
        </div>
      </Card>
      <Card icon={Upload} tone="purple" title="Import key file" subtitle="Use a protected private-key file you already have">
        <form class="space-y-3" onsubmit={e => { e.preventDefault(); void run('wallet_import', { path: importPath }); }}>
          <label class="block"><span class="field-label">Protected private-key file path</span><input class="input font-mono" bind:value={importPath} required autocomplete="off" placeholder="/path/to/wallet.key" /></label>
          <button class="btn-secondary w-full" disabled={busy || !$session.configured || !importPath}><Upload class="w-4 h-4" />Import key file</button>
        </form>
      </Card>
      <div class="md:col-span-2">{@render faucet()}</div>
    </div>
  {/if}
</section>
