<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Expandable from '$lib/components/ui/Expandable.svelte';
  import BootstrapHealthDashboard from '$lib/components/network/BootstrapHealthDashboard.svelte';
  import { RefreshCw, HardDrive, Activity, FolderOpen, AlertCircle, Server, Wifi } from 'lucide-svelte';
  import { onDestroy, onMount, createEventDispatcher } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { get } from 'svelte/store';
  import { t } from 'svelte-i18n';
  import { getStatus, type GethStatus } from '$lib/services/gethService';

  export let dataDir: string;
  export let logLines = 40;
  export let refreshIntervalMs = 10000;

  const dispatch = createEventDispatcher<{ status: GethStatus }>();
  const tr = (k: string, params?: Record<string, any>): string => (get(t) as (key: string, params?: any) => string)(k, params);
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  let status: GethStatus | null = null;
  let loading = false;
  let error: string | null = null;
  let intervalId: ReturnType<typeof setInterval> | null = null;
  let unlistenProgress: (() => void) | null = null;
  let componentMounted = false;
  let previousDataDir: string | undefined;

  let bootstrapExpanded = false;

  $: lastUpdatedLabel = status
    ? tr('network.geth.lastUpdated', {
        values: {
          time: new Date(status.lastUpdated * 1000).toLocaleTimeString([], {
            hour: '2-digit',
            minute: '2-digit',
          }),
        },
      })
    : null;

  const positiveBadgeClass =
    'bg-emerald-500/10 text-emerald-600 dark:text-emerald-300 border border-emerald-500/40';
  const negativeBadgeClass =
    'bg-destructive text-destructive-foreground border border-destructive/40';

  async function loadStatus(triggeredByUser = false) {
    if (!isTauri) {
      return;
    }

    if (loading && !triggeredByUser) {
      return;
    }

    loading = true;
    error = null;

    try {
      const result = await getStatus(dataDir, logLines);
      status = result;
      dispatch('status', result);
    } catch (err) {
      console.error('Failed to load geth status:', err);
      error = tr('network.geth.errorGeneric');
    } finally {
      loading = false;
    }
  }

  export async function refresh() {
    await loadStatus(true);
  }

  onMount(async () => {
    componentMounted = true;

    if (!isTauri) {
      error = tr('network.geth.desktopOnly');
      return;
    }

    previousDataDir = dataDir;
    await loadStatus(false);

    if (refreshIntervalMs > 0) {
      intervalId = setInterval(() => {
        loadStatus(false);
      }, refreshIntervalMs);
    }

    unlistenProgress = await listen('geth-download-progress', async (event) => {
      const payload = event.payload as { percentage?: number; status?: string } | undefined;
      if (!payload) {
        return;
      }

      if ((payload.percentage ?? 0) >= 100 || /complete/i.test(payload.status ?? '')) {
        await loadStatus(false);
      }
    });
  });

  onDestroy(() => {
    componentMounted = false;

    if (intervalId) {
      clearInterval(intervalId);
      intervalId = null;
    }

    if (unlistenProgress) {
      unlistenProgress();
      unlistenProgress = null;
    }
  });

  $: if (componentMounted && isTauri && dataDir && dataDir !== previousDataDir) {
    previousDataDir = dataDir;
    loadStatus(false);
  }

  $: logTruncatedLabel = status
    ? tr('network.geth.logTruncated', { values: { count: status.logLines } })
    : null;
</script>

<Card class="p-6 space-y-5">
  <div class="flex flex-wrap items-start justify-between gap-4">
    <div class="space-y-1">
      <div class="flex items-center gap-2">
        <Server class="h-5 w-5 text-muted-foreground" />
        <h2 class="text-lg font-semibold">{tr('network.geth.title')}</h2>
      </div>
      <p class="text-sm text-muted-foreground max-w-2xl">
        {tr('network.geth.subtitle')}
      </p>
      {#if lastUpdatedLabel}
        <p class="text-xs text-muted-foreground">{lastUpdatedLabel}</p>
      {/if}
    </div>
    <Button
      variant="outline"
      class="flex items-center gap-2"
      on:click={refresh}
      disabled={!isTauri || loading}
    >
      <RefreshCw class={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
      {tr('network.geth.refresh')}
    </Button>
  </div>

  {#if !isTauri}
    <p class="text-sm text-muted-foreground">{tr('network.geth.desktopOnly')}</p>
  {:else}
    {#if error}
      <div class="flex items-start gap-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
        <AlertCircle class="h-4 w-4 mt-0.5" />
        <span>{error}</span>
      </div>
    {/if}

    <div class:opacity-50={loading} class="space-y-4">
      <div class="grid gap-4 md:grid-cols-2">
        <div class="space-y-3">
          <div class="space-y-2">
            <p class="text-sm font-medium text-muted-foreground">
              {tr('network.geth.installed')}
            </p>
            <Badge class={status?.installed ? positiveBadgeClass : negativeBadgeClass}>
              {status?.installed
                ? tr('network.geth.installedYes')
                : tr('network.geth.installedNo')}
            </Badge>
            <div class="flex items-start gap-2 text-sm text-muted-foreground">
              <HardDrive class="mt-0.5 h-4 w-4" />
              <span>
                {#if status?.binaryPath}
                  <code class="break-all">{status.binaryPath}</code>
                {:else}
                  {tr('network.geth.binaryMissing')}
                {/if}
              </span>
            </div>
          </div>

          <div class="space-y-2">
            <p class="text-sm font-medium text-muted-foreground">
              {tr('network.geth.running')}
            </p>
            <Badge class={status?.running ? positiveBadgeClass : negativeBadgeClass}>
              {status?.running
                ? tr('network.geth.runningYes')
                : tr('network.geth.runningNo')}
            </Badge>
            <div class="flex items-center gap-2 text-sm text-muted-foreground">
              <Activity class="h-4 w-4" />
              <span>{status?.version ?? tr('network.geth.versionUnknown')}</span>
            </div>
          </div>
        </div>

        <div class="space-y-3">
          <p class="text-sm font-medium text-muted-foreground">
            {tr('network.geth.dataDir')}
          </p>
          <div class="flex items-start gap-2 text-sm text-muted-foreground">
            <FolderOpen class="mt-0.5 h-4 w-4" />
            <span class="break-all">
              <code>{status?.dataDir ?? dataDir}</code>
            </span>
          </div>
          {#if status && !status.dataDirExists}
            <div class="mt-1 flex items-start gap-2 text-xs text-destructive">
              <AlertCircle class="mt-0.5 h-3 w-3" />
              <span>{tr('network.geth.dataDirMissing')}</span>
            </div>
          {/if}
          <div class="text-xs text-muted-foreground">
            {#if status?.logAvailable}
              {logTruncatedLabel}
            {:else}
              {tr('network.geth.logUnavailable')}
            {/if}
          </div>
        </div>
      </div>

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <p class="text-sm font-medium text-muted-foreground">
            {tr('network.geth.logHeading')}
          </p>
        </div>
        <div class="h-48 overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-xs">
          {#if status?.logAvailable}
            {#if status.lastLogs.length > 0}
              {#each status.lastLogs as line, index}
                <p class="whitespace-pre-wrap" data-index={index}>{line}</p>
              {/each}
            {:else}
              <p class="text-muted-foreground">{tr('network.geth.logEmpty')}</p>
            {/if}
          {:else}
            <p class="text-muted-foreground">{tr('network.geth.logUnavailable')}</p>
          {/if}
        </div>
      </div>

      <!-- Bootstrap Node Health Check -->
      <Expandable bind:isOpen={bootstrapExpanded}>
        <div slot="title" class="flex items-center gap-2">
          <Wifi class="h-4 w-4" />
          <span>{tr('network.geth.bootstrap.title')}</span>
        </div>

        <BootstrapHealthDashboard />
      </Expandable>
    </div>
  {/if}
</Card>