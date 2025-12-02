<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Progress from '$lib/components/ui/progress.svelte';
  import { 
    Server, 
    CheckCircle, 
    XCircle, 
    AlertTriangle,
    RefreshCw,
    Activity,
    Clock,
    Zap,
    Globe
  } from 'lucide-svelte';

  interface BootstrapNodeHealth {
    enode: string;
    description: string;
    region: string;
    reachable: boolean;
    latency_ms: number | null;
    error: string | null;
    consecutive_failures: number;
    last_success: number | null;
    last_checked: number | null;
  }

  interface BootstrapHealthReport {
    total_nodes: number;
    reachable_nodes: number;
    unreachable_nodes: number;
    nodes: BootstrapNodeHealth[];
    timestamp: number;
    healthy: boolean;
    recommendation: string | null;
  }

  let report: BootstrapHealthReport | null = null;
  let loading = false;
  let error = '';
  let autoRefresh = true;
  let refreshInterval: NodeJS.Timeout;
  let lastRefreshTime: Date | null = null;
  let checking = false;

  async function checkHealth(useCache = false) {
    try {
      checking = true;
      error = '';
      
      if (useCache) {
        const cached = await invoke<BootstrapHealthReport | null>('get_cached_bootstrap_health');
        if (cached) {
          report = cached;
          lastRefreshTime = new Date();
          return;
        }
      }
      
      report = await invoke<BootstrapHealthReport>('check_bootstrap_health');
      lastRefreshTime = new Date();
    } catch (e) {
      error = String(e);
      console.error('Failed to check bootstrap health:', e);
    } finally {
      checking = false;
    }
  }

  async function clearCache() {
    try {
      await invoke('clear_bootstrap_cache');
      await checkHealth(false);
    } catch (e) {
      error = `Failed to clear cache: ${e}`;
    }
  }

  async function reconnectBootstrap() {
    try {
      checking = true;
      await invoke('reconnect_geth_bootstrap');
      await new Promise(resolve => setTimeout(resolve, 2000)); // Wait for reconnection
      await checkHealth(false);
    } catch (e) {
      error = `Failed to reconnect: ${e}`;
    } finally {
      checking = false;
    }
  }

  function formatLatency(ms: number | null): string {
    if (ms === null) return 'N/A';
    if (ms < 100) return `${ms}ms`;
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  }

  function getLatencyColor(ms: number | null): string {
    if (ms === null) return 'text-muted-foreground';
    if (ms < 100) return 'text-green-500';
    if (ms < 300) return 'text-yellow-500';
    return 'text-red-500';
  }

  function formatLastChecked(timestamp: number | null): string {
    if (!timestamp) return 'Never';
    const date = new Date(timestamp * 1000);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffSecs = Math.floor(diffMs / 1000);
    const diffMins = Math.floor(diffSecs / 60);
    
    if (diffSecs < 60) return `${diffSecs}s ago`;
    if (diffMins < 60) return `${diffMins}m ago`;
    return date.toLocaleTimeString();
  }

  onMount(() => {
    loading = true;
    checkHealth(true).finally(() => loading = false);
    
    // Auto-refresh every 30 seconds
    refreshInterval = setInterval(() => {
      if (autoRefresh && !checking) {
        checkHealth(false);
      }
    }, 30000);
  });

  onDestroy(() => {
    if (refreshInterval) {
      clearInterval(refreshInterval);
    }
  });

  $: healthPercent = report ? Math.round((report.reachable_nodes / report.total_nodes) * 100) : 0;
  $: avgLatency = report 
    ? (report.nodes
        .filter(n => n.latency_ms !== null)
        .reduce((sum, n) => sum + (n.latency_ms || 0), 0) / (report.nodes.filter(n => n.latency_ms !== null).length || 1) || 0)
    : 0;
</script>

<div class="space-y-6">
  <!-- Header -->
  <div class="flex items-center justify-between">
    <div class="flex gap-2">
      <Button 
        variant="outline" 
        size="sm"
        on:click={() => checkHealth(false)}
        disabled={checking}
      >
        {#if checking}
          <RefreshCw class="h-4 w-4 mr-2 animate-spin" />
          Checking...
        {:else}
          <RefreshCw class="h-4 w-4 mr-2" />
          Refresh
        {/if}
      </Button>
      <Button
        variant="outline"
        size="sm"
        on:click={() => autoRefresh = !autoRefresh}
      >
        <Activity class="h-4 w-4 mr-2 {autoRefresh ? 'text-green-500' : ''}" />
        Auto: {autoRefresh ? 'On' : 'Off'}
      </Button>
    </div>
  </div>

  {#if error}
    <Card class="p-4 border-destructive">
      <div class="flex items-center gap-2">
        <XCircle class="h-5 w-5 text-destructive" />
        <span class="text-destructive font-medium">{error}</span>
      </div>
    </Card>
  {/if}

  {#if loading}
    <Card class="p-8">
      <div class="flex flex-col items-center justify-center gap-4">
        <RefreshCw class="h-8 w-8 animate-spin text-primary" />
        <p class="text-muted-foreground">Checking bootstrap nodes...</p>
      </div>
    </Card>
  {:else if report}
    <!-- Overall Status -->
    <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
      <Card class="p-4">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm text-muted-foreground">Total Nodes</p>
            <p class="text-2xl font-bold mt-1">{report.total_nodes}</p>
          </div>
          <Server class="h-8 w-8 text-muted-foreground" />
        </div>
      </Card>

      <Card class="p-4">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm text-muted-foreground">Reachable</p>
            <p class="text-2xl font-bold mt-1 text-green-500">{report.reachable_nodes}</p>
          </div>
          <CheckCircle class="h-8 w-8 text-green-500" />
        </div>
        <Progress value={healthPercent} max={100} class="mt-2 h-1" />
      </Card>

      <Card class="p-4">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm text-muted-foreground">Unreachable</p>
            <p class="text-2xl font-bold mt-1 text-red-500">{report.unreachable_nodes}</p>
          </div>
          <XCircle class="h-8 w-8 text-red-500" />
        </div>
      </Card>

      <Card class="p-4">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm text-muted-foreground">Avg Latency</p>
            <p class="text-2xl font-bold mt-1">{Math.round(avgLatency)}ms</p>
          </div>
          <Zap class="h-8 w-8 text-yellow-500" />
        </div>
      </Card>
    </div>

    <!-- Health Status -->
    <Card class="p-6 border-l-4 {report.healthy ? 'border-green-500' : 'border-red-500'}">
      <div class="flex items-start gap-3">
        {#if report.healthy}
          <CheckCircle class="h-6 w-6 text-green-500 flex-shrink-0" />
        {:else}
          <AlertTriangle class="h-6 w-6 text-red-500 flex-shrink-0" />
        {/if}
        <div class="flex-1">
          <h3 class="font-semibold mb-1">
            {report.healthy ? 'System Healthy' : 'Action Required'}
          </h3>
          {#if report.recommendation}
            <p class="text-sm text-muted-foreground">{report.recommendation}</p>
          {:else if report.reachable_nodes === report.total_nodes}
            <p class="text-sm text-muted-foreground">
              All bootstrap nodes are reachable. Network connectivity is good.
            </p>
          {:else if report.reachable_nodes > 0}
            <p class="text-sm text-muted-foreground">
              {report.reachable_nodes} of {report.total_nodes} bootstrap nodes are reachable. Connection available but some nodes are down.
            </p>
          {:else}
            <p class="text-sm text-muted-foreground">
              No bootstrap nodes are reachable. Network connectivity may be impaired.
            </p>
          {/if}
          {#if !report.healthy}
            <div class="flex gap-2 mt-3">
              <Button size="sm" on:click={reconnectBootstrap} disabled={checking}>
                <RefreshCw class="h-4 w-4 mr-2" />
                Reconnect Bootstrap
              </Button>
              <Button size="sm" variant="outline" on:click={clearCache}>
                Clear Cache
              </Button>
            </div>
          {/if}
        </div>
      </div>
    </Card>

    <!-- Node Details -->
    <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <h3 class="text-lg font-semibold">Node Details</h3>
        {#if lastRefreshTime}
          <div class="flex items-center gap-2 text-sm text-muted-foreground">
            <Clock class="h-4 w-4" />
            Last checked: {lastRefreshTime.toLocaleTimeString()}
          </div>
        {/if}
      </div>

      <div class="space-y-3">
        {#each report.nodes as node}
          <div class="p-4 border rounded-lg {node.reachable ? 'bg-secondary/50 border-border' : 'bg-muted/30 border-muted-foreground/20'}">
            <div class="flex items-start justify-between gap-4">
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2 mb-2">
                  {#if node.reachable}
                    <CheckCircle class="h-5 w-5 text-green-500 flex-shrink-0" />
                  {:else}
                    <XCircle class="h-5 w-5 text-red-500 flex-shrink-0" />
                  {/if}
                  <h4 class="font-medium truncate">{node.description}</h4>
                  <Badge variant="outline" class="flex-shrink-0">
                    <Globe class="h-3 w-3 mr-1" />
                    {node.region}
                  </Badge>
                </div>

                <div class="grid grid-cols-1 md:grid-cols-3 gap-2 text-sm mt-2">
                  <div>
                    <span class="text-muted-foreground">Latency:</span>
                    <span class="ml-2 font-medium {getLatencyColor(node.latency_ms)}">
                      {formatLatency(node.latency_ms)}
                    </span>
                  </div>
                  <div>
                    <span class="text-muted-foreground">Last Check:</span>
                    <span class="ml-2 font-medium">
                      {formatLastChecked(node.last_checked)}
                    </span>
                  </div>
                  {#if node.consecutive_failures > 0}
                    <div>
                      <span class="text-muted-foreground">Failures:</span>
                      <span class="ml-2 font-medium text-red-500">
                        {node.consecutive_failures}
                      </span>
                    </div>
                  {/if}
                </div>

                {#if node.error}
                  <div class="mt-2 text-sm text-red-600 dark:text-red-400">
                    ⚠️ {node.error}
                  </div>
                {/if}

                <details class="mt-2">
                  <summary class="text-xs text-muted-foreground cursor-pointer hover:text-foreground">
                    Show enode URL
                  </summary>
                  <code class="text-xs mt-1 block bg-muted p-2 rounded break-all">
                    {node.enode}
                  </code>
                </details>
              </div>
            </div>
          </div>
        {/each}
      </div>
    </Card>
  {/if}
</div>
