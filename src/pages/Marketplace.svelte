<script lang="ts">
  import { settings, walletAccount } from '$lib/stores';
  import {
    discoverOffers,
    discoverOffersViaGateway,
    openContract,
    type ResourceClass,
    type ResourceOffer,
    type OpenedContract,
  } from '$lib/exchangeService';
  import { toasts } from '$lib/toastStore';
  import { goto } from '@mateothegreat/svelte5-router';
  import { Store, Boxes, Search, Settings as SettingsIcon, Loader2 } from 'lucide-svelte';

  const classes: { id: ResourceClass; label: string; icon: typeof Store }[] = [
    { id: 'storage', label: 'Storage', icon: Store },
    { id: 'container', label: 'Container', icon: Boxes },
  ];

  let selectedClass = $state<ResourceClass>('storage');
  let offers = $state<ResourceOffer[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let searched = $state(false);

  // Per-offer open-contract state (keyed by the offer's provider wallet).
  let openTarget = $state<ResourceOffer | null>(null);
  let fundingChi = $state('1.0');
  let opening = $state(false);
  let opened = $state<OpenedContract | null>(null);

  const short = (addr: string) => (addr.length > 12 ? `${addr.slice(0, 8)}…${addr.slice(-4)}` : addr);

  async function discover() {
    error = null;
    opened = null;
    openTarget = null;
    loading = true;
    searched = true;
    try {
      // Primary (both modes): the local DHT — thin queries as a Kademlia client,
      // full as a server. Dormant fallback (thin only): a configured gateway, used
      // if the direct DHT path fails (e.g. libp2p blocked). Offers are re-verified
      // in the backend on either path.
      try {
        offers = await discoverOffers(selectedClass);
      } catch (dhtErr) {
        const gateway = ($settings.gatewayUrl || '').trim();
        if ($settings.appMode === 'thin' && gateway) {
          offers = await discoverOffersViaGateway(gateway, selectedClass);
        } else {
          throw dhtErr;
        }
      }
    } catch (e) {
      offers = [];
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function confirmOpen() {
    if (!openTarget) return;
    const acct = $walletAccount;
    if (!acct?.address || !acct?.privateKey) {
      toasts.show('Unlock your wallet before opening a contract.', 'error');
      return;
    }
    opening = true;
    error = null;
    try {
      opened = await openContract(openTarget, fundingChi, acct.address, acct.privateKey);
      toasts.detail(
        'Contract opened',
        `Deposit sent (tx ${short(opened.txHash)}). Credential issued.`,
        'success',
      );
      openTarget = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      opening = false;
    }
  }

  function priceEntries(offer: ResourceOffer): [string, string][] {
    return Object.entries(offer.price_schedule ?? {}).map(([k, v]) => [k, String(v)]);
  }
</script>

<div class="max-w-5xl mx-auto p-4 md:p-6 space-y-6">
  <header class="flex items-center gap-3">
    <div class="p-2 bg-primary-100 dark:bg-primary-900/40 rounded-lg">
      <Store class="w-5 h-5 text-primary-600 dark:text-primary-400" />
    </div>
    <div>
      <h1 class="text-xl font-semibold text-gray-900 dark:text-white">Marketplace</h1>
      <p class="text-sm text-gray-500 dark:text-gray-400">
        Discover providers and open a prepaid contract. The deposit is non-refundable once sent.
      </p>
    </div>
  </header>

  <!-- Class selector + discover -->
  <div class="flex flex-wrap items-center gap-2">
    {#each classes as c}
      {@const Icon = c.icon}
      <button
        onclick={() => (selectedClass = c.id)}
        class="flex items-center gap-2 py-2 px-3 rounded-lg border text-sm font-medium transition-colors
          {selectedClass === c.id
            ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-300'
            : 'border-gray-200 dark:border-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700/50'}"
      >
        <Icon class="w-4 h-4" />
        {c.label}
      </button>
    {/each}
    <button
      onclick={discover}
      disabled={loading}
      class="ml-auto flex items-center gap-2 py-2 px-4 rounded-lg bg-primary-600 hover:bg-primary-700 disabled:opacity-60 text-white text-sm font-medium transition-colors"
    >
      {#if loading}
        <Loader2 class="w-4 h-4 animate-spin" />
      {:else}
        <Search class="w-4 h-4" />
      {/if}
      Discover
    </button>
  </div>

  {#if error}
    <div class="rounded-xl border border-red-200 dark:border-red-900/50 bg-red-50/50 dark:bg-red-900/10 p-4 text-sm text-red-700 dark:text-red-300">
      {error}
      {#if error.includes('gateway')}
        <button class="ml-2 underline inline-flex items-center gap-1" onclick={() => goto('/settings')}>
          <SettingsIcon class="w-3.5 h-3.5" /> Open Settings
        </button>
      {/if}
    </div>
  {/if}

  <!-- Result of a successful open -->
  {#if opened}
    <div class="rounded-xl border border-emerald-200 dark:border-emerald-900/50 bg-emerald-50/50 dark:bg-emerald-900/10 p-4 space-y-1 text-sm">
      <p class="font-semibold text-emerald-800 dark:text-emerald-200">Contract opened</p>
      <p class="text-gray-700 dark:text-gray-300">Contract: <code>{opened.contractId}</code></p>
      <p class="text-gray-700 dark:text-gray-300">Balance: {opened.balanceWei} wei · tx <code>{short(opened.txHash)}</code></p>
      <p class="text-gray-700 dark:text-gray-300">
        Credential: bucket <code>{opened.credential.bucket || '—'}</code>, bearer
        <code>{short(opened.credential.bearer)}</code>
      </p>
    </div>
  {/if}

  <!-- Offers -->
  {#if searched && !loading && offers.length === 0 && !error}
    <p class="text-sm text-gray-500 dark:text-gray-400">No verified offers found for {selectedClass}.</p>
  {/if}

  <div class="space-y-3">
    {#each offers as offer}
      <div class="rounded-2xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div class="min-w-0">
            <p class="text-sm font-medium text-gray-900 dark:text-white break-all">{offer.endpoint}</p>
            <p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
              provider <code>{short(offer.provider_wallet)}</code>
              {#if offer.region}· {offer.region}{/if}
              · min {offer.min_funding_wei} wei
            </p>
            <div class="flex flex-wrap gap-x-4 gap-y-1 mt-2">
              {#each priceEntries(offer) as [k, v]}
                <span class="text-xs text-gray-600 dark:text-gray-300"><span class="text-gray-400">{k}:</span> {v}</span>
              {/each}
            </div>
          </div>
          <button
            onclick={() => {
              openTarget = openTarget?.provider_wallet === offer.provider_wallet ? null : offer;
              opened = null;
            }}
            class="shrink-0 py-1.5 px-3 rounded-lg border border-primary-300 dark:border-primary-800 text-primary-700 dark:text-primary-300 text-sm font-medium hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-colors"
          >
            {openTarget?.provider_wallet === offer.provider_wallet ? 'Cancel' : 'Open contract'}
          </button>
        </div>

        {#if openTarget?.provider_wallet === offer.provider_wallet}
          <div class="mt-3 pt-3 border-t border-gray-100 dark:border-gray-700 flex flex-wrap items-end gap-3">
            <div>
              <label for="funding" class="block text-xs text-gray-500 dark:text-gray-400 mb-1">Deposit (CHI, non-refundable)</label>
              <input
                id="funding"
                type="text"
                bind:value={fundingChi}
                class="w-40 px-3 py-2 text-sm rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 text-gray-900 dark:text-white"
              />
            </div>
            <button
              onclick={confirmOpen}
              disabled={opening}
              class="flex items-center gap-2 py-2 px-4 rounded-lg bg-primary-600 hover:bg-primary-700 disabled:opacity-60 text-white text-sm font-medium transition-colors"
            >
              {#if opening}<Loader2 class="w-4 h-4 animate-spin" />{/if}
              Fund &amp; open
            </button>
          </div>
        {/if}
      </div>
    {/each}
  </div>
</div>
