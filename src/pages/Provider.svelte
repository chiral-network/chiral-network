<script lang="ts">
  import { walletAccount, settings } from '$lib/stores';
  import {
    startProvider,
    stopProvider,
    getProviderStatus,
    type ProviderStatus,
    type StartProviderRequest,
  } from '$lib/providerService';
  import type { ResourceClass } from '$lib/exchangeService';
  import { toasts } from '$lib/toastStore';
  import { onMount } from 'svelte';
  import { Server, Store, Boxes, Loader2, AlertTriangle } from 'lucide-svelte';

  let status = $state<ProviderStatus>({ running: false });
  let busy = $state(false);
  let error = $state<string | null>(null);

  // Form
  let providerClass = $state<ResourceClass>('storage');
  let endpoint = $state('https://');
  let controlBind = $state('0.0.0.0:8443');
  let dataBind = $state('0.0.0.0:8444');
  let minFundingWei = $state('100000000000000000'); // 0.1 CHI
  // storage prices (wei)
  let perGbEgress = $state('1000000000000000');
  let perGbMonth = $state('10000000000000000');
  // container prices (wei)
  let perVcpuHour = $state('1000000000000000');
  let perGbMemHour = $state('500000000000000');
  let perGpuHour = $state('50000000000000000');

  const short = (a?: string) => (a && a.length > 12 ? `${a.slice(0, 8)}…${a.slice(-4)}` : (a ?? ''));

  onMount(async () => {
    try {
      status = await getProviderStatus();
    } catch (e) {
      // ignore — backend not ready / not tauri
    }
  });

  function priceSchedule(): Record<string, string> {
    return providerClass === 'storage'
      ? { per_gb_egress: perGbEgress, per_gb_month: perGbMonth }
      : {
          per_vcpu_hour_wei: perVcpuHour,
          per_gb_mem_hour_wei: perGbMemHour,
          per_gpu_hour_wei: perGpuHour,
        };
  }

  async function start() {
    error = null;
    const acct = $walletAccount;
    if (!acct?.privateKey) {
      toasts.show('Unlock your wallet first.', 'error');
      return;
    }
    if (!endpoint.startsWith('http')) {
      error = 'Endpoint must be a public http(s) URL your provider is reachable at.';
      return;
    }
    busy = true;
    try {
      const req: StartProviderRequest = {
        class: providerClass,
        endpoint: endpoint.trim(),
        priceSchedule: priceSchedule(),
        minFundingWei: minFundingWei.trim(),
        controlBind: controlBind.trim(),
        dataBind: providerClass === 'storage' ? dataBind.trim() : '',
        privateKey: acct.privateKey,
        offerNonce: Date.now(),
      };
      status = await startProvider(req);
      toasts.detail('Provider started', `Advertising ${status.class} at ${status.endpoint}`, 'success');
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function stop() {
    busy = true;
    error = null;
    try {
      await stopProvider();
      status = { running: false };
      toasts.show('Provider stopped.', 'success');
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="max-w-3xl mx-auto p-4 md:p-6 space-y-6">
  <header class="flex items-center gap-3">
    <div class="p-2 bg-primary-100 dark:bg-primary-900/40 rounded-lg">
      <Server class="w-5 h-5 text-primary-600 dark:text-primary-400" />
    </div>
    <div>
      <h1 class="text-xl font-semibold text-gray-900 dark:text-white">Become a provider</h1>
      <p class="text-sm text-gray-500 dark:text-gray-400">
        Serve storage or container compute and earn CHI. Your app publishes a signed offer and meters usage;
        payments are verified over RPC (no full node needed).
      </p>
    </div>
  </header>

  <!-- Reachability warning -->
  <div class="rounded-xl border border-amber-200 dark:border-amber-900/50 bg-amber-50/50 dark:bg-amber-900/10 p-4 flex items-start gap-3 text-sm">
    <AlertTriangle class="w-5 h-5 text-amber-600 dark:text-amber-400 shrink-0 mt-0.5" />
    <div class="text-amber-800 dark:text-amber-200">
      A provider must be <strong>publicly reachable</strong> at its endpoint (a public IP/domain with the bind
      port forwarded, ideally behind TLS). No NAT traversal in this version — consumers connect to the endpoint
      directly. Deposits are non-refundable, so price and reachability matter.
    </div>
  </div>

  {#if status.running}
    <div class="rounded-2xl border border-emerald-200 dark:border-emerald-900/50 bg-emerald-50/50 dark:bg-emerald-900/10 p-4 space-y-1 text-sm">
      <p class="font-semibold text-emerald-800 dark:text-emerald-200">Provider running</p>
      <p class="text-gray-700 dark:text-gray-300">Class <code>{status.class}</code> · endpoint <code>{status.endpoint}</code></p>
      <p class="text-gray-700 dark:text-gray-300">
        offer <code>{short(status.offerRef)}</code> · wallet <code>{short(status.providerWallet)}</code>
        · control <code>{status.controlBind}</code>{#if status.dataBind} · data <code>{status.dataBind}</code>{/if}
      </p>
      <button
        onclick={stop}
        disabled={busy}
        class="mt-2 flex items-center gap-2 py-2 px-4 rounded-lg bg-red-600 hover:bg-red-700 disabled:opacity-60 text-white text-sm font-medium transition-colors"
      >
        {#if busy}<Loader2 class="w-4 h-4 animate-spin" />{/if}
        Stop provider
      </button>
    </div>
  {:else}
    <div class="rounded-2xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-5 space-y-4">
      <!-- class -->
      <div class="flex gap-2">
        <button
          onclick={() => (providerClass = 'storage')}
          class="flex-1 flex items-center justify-center gap-2 py-2 px-3 rounded-lg border text-sm font-medium transition-colors
            {providerClass === 'storage' ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-300' : 'border-gray-200 dark:border-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700/50'}"
        ><Store class="w-4 h-4" /> Storage</button>
        <button
          onclick={() => (providerClass = 'container')}
          class="flex-1 flex items-center justify-center gap-2 py-2 px-3 rounded-lg border text-sm font-medium transition-colors
            {providerClass === 'container' ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-300' : 'border-gray-200 dark:border-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700/50'}"
        ><Boxes class="w-4 h-4" /> Container</button>
      </div>

      {#snippet field(label: string, value: string, set: (v: string) => void, placeholder = '')}
        <label class="block">
          <span class="block text-xs text-gray-500 dark:text-gray-400 mb-1">{label}</span>
          <input
            type="text"
            {value}
            {placeholder}
            oninput={(e) => set((e.currentTarget as HTMLInputElement).value)}
            class="w-full px-3 py-2 text-sm rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 text-gray-900 dark:text-white"
          />
        </label>
      {/snippet}

      {@render field('Public endpoint (advertised URL)', endpoint, (v) => (endpoint = v), 'https://me.example')}
      {@render field('Min deposit (wei)', minFundingWei, (v) => (minFundingWei = v))}

      {#if providerClass === 'storage'}
        {@render field('Price per GB egress (wei)', perGbEgress, (v) => (perGbEgress = v))}
        {@render field('Price per GB-month (wei)', perGbMonth, (v) => (perGbMonth = v))}
        <div class="grid grid-cols-2 gap-3">
          {@render field('Control bind', controlBind, (v) => (controlBind = v))}
          {@render field('S3 data bind', dataBind, (v) => (dataBind = v))}
        </div>
      {:else}
        {@render field('Price per vCPU-hour (wei)', perVcpuHour, (v) => (perVcpuHour = v))}
        {@render field('Price per GB-mem-hour (wei)', perGbMemHour, (v) => (perGbMemHour = v))}
        {@render field('Price per GPU-hour (wei)', perGpuHour, (v) => (perGpuHour = v))}
        {@render field('Bind address', controlBind, (v) => (controlBind = v))}
      {/if}

      {#if error}
        <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
      {/if}

      <button
        onclick={start}
        disabled={busy}
        class="w-full flex items-center justify-center gap-2 py-2.5 px-4 rounded-lg bg-primary-600 hover:bg-primary-700 disabled:opacity-60 text-white text-sm font-medium transition-colors"
      >
        {#if busy}<Loader2 class="w-4 h-4 animate-spin" />{/if}
        Start provider
      </button>
      {#if $settings.appMode !== 'full'}
        <p class="text-xs text-gray-400">
          Provider mode is an advanced feature; enable Full mode in Settings for the full node experience.
        </p>
      {/if}
    </div>
  {/if}
</div>
