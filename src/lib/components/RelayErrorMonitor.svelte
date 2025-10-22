<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from 'svelte-i18n';
  import { relayErrorService, RelayConnectionState, RelayErrorType } from '$lib/services/relayErrorService';
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import { AlertTriangle, CheckCircle, XCircle, Clock, Activity, WifiOff } from 'lucide-svelte';

  let monitorInterval: number | null = null;

  // Subscribe to relay stores
  $: relayPool = relayErrorService.relayPool;
  $: activeRelay = relayErrorService.activeRelay;
  $: errorLog = relayErrorService.errorLog;
  $: healthyRelays = relayErrorService.healthyRelays;
  $: relayStats = relayErrorService.relayStats;

  onMount(() => {
    // Monitor reservations every 30 seconds
    monitorInterval = setInterval(() => {
      relayErrorService.monitorReservations();
    }, 30000) as unknown as number;
  });

  onDestroy(() => {
    if (monitorInterval !== null) {
      clearInterval(monitorInterval);
    }
  });

  function getStateIcon(state: RelayConnectionState) {
    switch (state) {
      case RelayConnectionState.CONNECTED:
      case RelayConnectionState.RESERVED:
        return CheckCircle;
      case RelayConnectionState.CONNECTING:
      case RelayConnectionState.RESERVING:
      case RelayConnectionState.RETRYING:
        return Clock;
      case RelayConnectionState.FAILED:
        return XCircle;
      case RelayConnectionState.FALLBACK:
        return Activity;
      default:
        return WifiOff;
    }
  }

  function getStateColor(state: RelayConnectionState): string {
    switch (state) {
      case RelayConnectionState.CONNECTED:
      case RelayConnectionState.RESERVED:
        return 'text-green-600';
      case RelayConnectionState.CONNECTING:
      case RelayConnectionState.RESERVING:
      case RelayConnectionState.RETRYING:
        return 'text-yellow-600';
      case RelayConnectionState.FAILED:
        return 'text-red-600';
      case RelayConnectionState.FALLBACK:
        return 'text-blue-600';
      default:
        return 'text-gray-600';
    }
  }

  function getHealthColor(score: number): string {
    if (score >= 80) return 'bg-green-500';
    if (score >= 60) return 'bg-yellow-500';
    if (score >= 40) return 'bg-orange-500';
    return 'bg-red-500';
  }

  function getErrorTypeLabel(type: RelayErrorType): string {
    switch (type) {
      case RelayErrorType.CONNECTION_REFUSED:
        return 'Connection Refused';
      case RelayErrorType.CONNECTION_TIMEOUT:
        return 'Timeout';
      case RelayErrorType.RESERVATION_FAILED:
        return 'Reservation Failed';
      case RelayErrorType.RESERVATION_EXPIRED:
        return 'Expired';
      case RelayErrorType.RELAY_OVERLOADED:
        return 'Overloaded';
      case RelayErrorType.RELAY_UNREACHABLE:
        return 'Unreachable';
      case RelayErrorType.NETWORK_ERROR:
        return 'Network Error';
      case RelayErrorType.AUTHENTICATION_FAILED:
        return 'Auth Failed';
      case RelayErrorType.PROTOCOL_ERROR:
        return 'Protocol Error';
      default:
        return 'Unknown';
    }
  }

  function formatTimestamp(timestamp: number | null): string {
    if (!timestamp) return 'Never';
    const date = new Date(timestamp);
    const now = Date.now();
    const diff = now - timestamp;

    if (diff < 60000) return 'Just now';
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
    if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
    return date.toLocaleString();
  }

  function formatReservationExpiry(expiry: number | null): string {
    if (!expiry) return 'N/A';
    const remaining = expiry - Date.now();
    if (remaining < 0) return 'Expired';
    if (remaining < 60000) return `${Math.floor(remaining / 1000)}s`;
    if (remaining < 3600000) return `${Math.floor(remaining / 60000)}m`;
    return `${Math.floor(remaining / 3600000)}h`;
  }

  function truncateId(id: string): string {
    if (id.length <= 12) return id;
    return `${id.slice(0, 6)}...${id.slice(-4)}`;
  }
</script>

<div class="space-y-6">
  <!-- Stats Overview -->
  <Card class="p-6">
    <h3 class="text-lg font-semibold mb-4">Relay Pool Statistics</h3>
    <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
      <div class="text-center">
        <div class="text-3xl font-bold text-blue-600">{$relayStats.totalRelays}</div>
        <div class="text-sm text-gray-600">Total Relays</div>
      </div>
      <div class="text-center">
        <div class="text-3xl font-bold text-green-600">{$relayStats.connectedRelays}</div>
        <div class="text-sm text-gray-600">Connected</div>
      </div>
      <div class="text-center">
        <div class="text-3xl font-bold text-yellow-600">{$relayStats.healthyRelays}</div>
        <div class="text-sm text-gray-600">Healthy</div>
      </div>
      <div class="text-center">
        <div class="text-3xl font-bold text-red-600">{$relayStats.totalErrors}</div>
        <div class="text-sm text-gray-600">Total Errors</div>
      </div>
    </div>

    {#if $relayStats.avgHealthScore}
      <div class="mt-4">
        <div class="flex items-center justify-between mb-2">
          <span class="text-sm font-medium">Average Health Score</span>
          <span class="text-sm font-bold">{$relayStats.avgHealthScore.toFixed(1)}%</span>
        </div>
        <div class="w-full bg-gray-200 rounded-full h-2.5">
          <div
            class={`h-2.5 rounded-full ${getHealthColor($relayStats.avgHealthScore)}`}
            style="width: {$relayStats.avgHealthScore}%"
          ></div>
        </div>
      </div>
    {/if}
  </Card>

  <!-- Active Relay -->
  {#if $activeRelay}
    <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <h3 class="text-lg font-semibold">Active Relay</h3>
        <Badge variant="default" class="bg-green-500">Connected</Badge>
      </div>
      <div class="space-y-2">
        <div class="flex justify-between">
          <span class="text-sm text-gray-600">Peer ID:</span>
          <span class="text-sm font-mono">{truncateId($activeRelay.id)}</span>
        </div>
        <div class="flex justify-between">
          <span class="text-sm text-gray-600">Health Score:</span>
          <span class="text-sm font-semibold">{$activeRelay.healthScore}%</span>
        </div>
        <div class="flex justify-between">
          <span class="text-sm text-gray-600">Avg Latency:</span>
          <span class="text-sm">{$activeRelay.avgLatency.toFixed(0)}ms</span>
        </div>
        <div class="flex justify-between">
          <span class="text-sm text-gray-600">Reservation:</span>
          <span class="text-sm">{formatReservationExpiry($activeRelay.reservationExpiry)}</span>
        </div>
        <div class="flex justify-between">
          <span class="text-sm text-gray-600">Success Rate:</span>
          <span class="text-sm">
            {$activeRelay.totalAttempts > 0
              ? (($activeRelay.totalSuccesses / $activeRelay.totalAttempts) * 100).toFixed(1)
              : 0}%
          </span>
        </div>
      </div>
    </Card>
  {/if}

  <!-- Relay Pool -->
  <Card class="p-6">
    <h3 class="text-lg font-semibold mb-4">Relay Pool ({$relayPool.size})</h3>
    {#if $relayPool.size === 0}
      <div class="text-center py-8 text-gray-500">
        <WifiOff class="w-12 h-12 mx-auto mb-2 opacity-50" />
        <p>No relays in pool</p>
      </div>
    {:else}
      <div class="space-y-3">
        {#each Array.from($relayPool.values()) as relay}
          <div class="border rounded-lg p-4 hover:bg-gray-50 transition-colors">
            <div class="flex items-start justify-between mb-2">
              <div class="flex items-center gap-2">
                <svelte:component
                  this={getStateIcon(relay.state)}
                  class="w-5 h-5 {getStateColor(relay.state)}"
                />
                <span class="font-mono text-sm font-medium">{truncateId(relay.id)}</span>
                {#if relay.isPrimary}
                  <Badge variant="default" class="text-xs">Primary</Badge>
                {/if}
              </div>
              <div class="flex items-center gap-2">
                <div class="w-16 bg-gray-200 rounded-full h-2">
                  <div
                    class={`h-2 rounded-full ${getHealthColor(relay.healthScore)}`}
                    style="width: {relay.healthScore}%"
                  ></div>
                </div>
                <span class="text-xs font-medium w-8 text-right">{relay.healthScore}</span>
              </div>
            </div>

            <div class="grid grid-cols-2 gap-2 text-xs text-gray-600 mb-2">
              <div>State: <span class="font-medium">{relay.state}</span></div>
              <div>Latency: <span class="font-medium">{relay.avgLatency.toFixed(0)}ms</span></div>
              <div>
                Attempts: <span class="font-medium">{relay.totalAttempts}</span> / Successes: <span
                  class="font-medium">{relay.totalSuccesses}</span
                >
              </div>
              <div>Failures: <span class="font-medium text-red-600">{relay.consecutiveFailures}</span></div>
            </div>

            {#if relay.lastSuccess}
              <div class="text-xs text-gray-500">
                Last success: {formatTimestamp(relay.lastSuccess)}
              </div>
            {/if}

            {#if relay.errors.length > 0}
              <div class="mt-2 pt-2 border-t">
                <div class="text-xs font-medium text-red-600 mb-1">Recent Errors:</div>
                {#each relay.errors.slice(0, 3) as error}
                  <div class="text-xs text-gray-600 flex items-center gap-1">
                    <AlertTriangle class="w-3 h-3 text-red-500" />
                    <span>{getErrorTypeLabel(error.type)}: {error.message.substring(0, 40)}...</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </Card>

  <!-- Recent Errors -->
  {#if $errorLog.length > 0}
    <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <h3 class="text-lg font-semibold">Recent Errors</h3>
        <button
          class="text-sm text-blue-600 hover:text-blue-800"
          on:click={() => relayErrorService.clearErrorLog()}
        >
          Clear
        </button>
      </div>
      <div class="space-y-2 max-h-64 overflow-y-auto">
        {#each $errorLog.slice(0, 10) as error}
          <div class="border-l-4 border-red-500 pl-3 py-2 bg-red-50">
            <div class="flex items-center justify-between mb-1">
              <Badge variant="destructive" class="text-xs">{getErrorTypeLabel(error.type)}</Badge>
              <span class="text-xs text-gray-500">{formatTimestamp(error.timestamp)}</span>
            </div>
            <div class="text-sm text-gray-700">{error.message}</div>
            <div class="text-xs text-gray-500 mt-1">
              Relay: {truncateId(error.relayId)} • Retry: {error.retryCount}
            </div>
          </div>
        {/each}
      </div>
    </Card>
  {/if}
</div>
