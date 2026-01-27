<script lang="ts">
  import { get } from 'svelte/store';
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { settings } from '$lib/stores';
  import type { AppSettings } from '$lib/stores';
  import { dhtService, type DhtHealth } from '$lib/dht';
  import { relayErrorService } from '$lib/services/relayErrorService';
  import { showToast } from '$lib/toast';
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import RelayErrorMonitor from '$lib/components/RelayErrorMonitor.svelte';
  import { Settings as SettingsIcon, RefreshCw } from 'lucide-svelte';

  // Relay server status
  let relayServerEnabled = false;
  let dhtIsRunning: boolean | null = null;
  let relayServerAlias = '';
  let isRestartingAutorelay = false;
  let dhtHealth: DhtHealth | null = null;

  // AutoRelay client settings
  let autoRelayEnabled = true;

  let settingsUnsubscribe: (() => void) | null = null;

  let healthCheckInterval = 30; // seconds
  let isHealthCheckRunning = false;

  function applySettingsState(source: Partial<AppSettings>) {
    if (typeof source.enableRelayServer === 'boolean') {
      relayServerEnabled = source.enableRelayServer;
    }
    if (typeof source.enableAutorelay === 'boolean') {
      autoRelayEnabled = source.enableAutorelay;
    }
    if (typeof source.relayServerAlias === 'string') {
      relayServerAlias = source.relayServerAlias;
    }
  }

  async function loadSettings() {
    // Start with current store values
    applySettingsState(get(settings));

    // Load settings from localStorage
    const stored = localStorage.getItem('chiralSettings');
    if (stored) {
      try {
        const loadedSettings = JSON.parse(stored) as Partial<AppSettings>;
        applySettingsState(loadedSettings);
        // Keep the shared settings store in sync with what we loaded
        settings.update((prev) => ({ ...prev, ...loadedSettings }));
      } catch (e) {
        console.error('Failed to load settings:', e);
      }
    }

    // Check if DHT is actually running
    await checkDhtStatus();

    // If DHT is running, trust the live health snapshot for AutoRelay state
    if (dhtIsRunning) {
      try {
        const health = await dhtService.getHealth();
        if (health) {
          autoRelayEnabled = health.autorelayEnabled;
          await saveSettings();
        }
      } catch (error) {
        console.error('Failed to sync AutoRelay state from DHT health:', error);
      }
    }
  }

  async function checkDhtStatus() {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      dhtIsRunning = await invoke<boolean>('is_dht_running').catch(() => false);
    } catch (error) {
      console.error('Failed to check DHT status:', error);
      dhtIsRunning = false;
    }
  }

  async function saveSettings() {
    const stored = localStorage.getItem('chiralSettings');
    let currentSettings = {};
    if (stored) {
      try {
        currentSettings = JSON.parse(stored);
      } catch (e) {
        console.error('Failed to parse settings:', e);
      }
    }

    currentSettings = {
      ...currentSettings,
      enableRelayServer: relayServerEnabled,
      enableAutorelay: autoRelayEnabled,
      relayServerAlias: relayServerAlias.trim(),
    };

    localStorage.setItem('chiralSettings', JSON.stringify(currentSettings));
    settings.set(currentSettings as any);
  }

  async function restartDhtWithSettings() {
    const currentSettings = JSON.parse(localStorage.getItem('chiralSettings') || '{}');

    const bootstrapNodes =
      currentSettings.customBootstrapNodes && currentSettings.customBootstrapNodes.length > 0
        ? currentSettings.customBootstrapNodes
        : [];

    await dhtService.stop();
    await new Promise((resolve) => setTimeout(resolve, 500));

    await dhtService.start({
      port: currentSettings.port || 4001,
      bootstrapNodes,
      enableAutonat: currentSettings.enableAutonat,
      autonatProbeIntervalSeconds: currentSettings.autonatProbeInterval,
      autonatServers: currentSettings.autonatServers || [],
      enableAutorelay: currentSettings.enableAutorelay,
      preferredRelays: currentSettings.preferredRelays || [],
      enableRelayServer: currentSettings.enableRelayServer,
      relayServerAlias: currentSettings.relayServerAlias || '',
      chunkSizeKb: currentSettings.chunkSize,
      cacheSizeMb: currentSettings.cacheSize,
      enableUpnp: currentSettings.enableUPnP,
      pureClientMode: currentSettings.pureClientMode,
      forceServerMode: currentSettings.forceServerMode,
    });

    relayServerEnabled = currentSettings.enableRelayServer ?? relayServerEnabled;
    autoRelayEnabled = currentSettings.enableAutorelay ?? autoRelayEnabled;
    dhtIsRunning = true;

    return currentSettings;
  }

  async function handleAutorelayToggle(event: Event) {
    const target = event.target as HTMLInputElement;
    const newValue = target.checked;
    const previousValue = autoRelayEnabled;

    isRestartingAutorelay = true;
    try {
      autoRelayEnabled = newValue;
      await saveSettings();

      const { invoke } = await import('@tauri-apps/api/core');
      const isRunning = await invoke<boolean>('is_dht_running').catch(() => false);

      if (isRunning) {
        await restartDhtWithSettings();
      }
    } catch (error) {
      console.error('Failed to toggle AutoRelay:', error);
      autoRelayEnabled = previousValue;
      await saveSettings();
      alert($t('relay.errors.toggleFailed', { values: { error } }));
    } finally {
      isRestartingAutorelay = false;
    }
  }

  let statusCheckInterval: number | undefined;
  let healthPollInterval: number | undefined;

  const formatNatTimestamp = (epoch?: number | null) => {
    if (!epoch) return $t('network.dht.health.never');
    return new Date(epoch * 1000).toLocaleString();
  };

  const relayErrorLog = relayErrorService.errorLog;
  const formatRelayErrorTimestamp = (ms: number) => new Date(ms).toLocaleString();
  let relayErrorClearedAt = 0;
  $: filteredRelayErrors = $relayErrorLog.filter((err) => err.timestamp >= relayErrorClearedAt);
  const formatHealthMessage = (value: string | null | undefined) => value ?? $t('network.dht.health.none');

  type SnapshotRelayError = { message: string; type: string; timestamp: number; relayId: string; retryCount?: number };
  const SNAPSHOT_STORAGE_KEY = 'relaySnapshotHistory';

  function loadSnapshotHistory(): SnapshotRelayError[] {
    try {
      const raw = localStorage.getItem(SNAPSHOT_STORAGE_KEY);
      if (!raw) return [];
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) return parsed;
    } catch (e) {
      console.error('Failed to load relay snapshot history', e);
    }
    return [];
  }

  function persistSnapshotHistory(history: SnapshotRelayError[]) {
    try {
      localStorage.setItem(SNAPSHOT_STORAGE_KEY, JSON.stringify(history));
    } catch (e) {
      console.error('Failed to persist relay snapshot history', e);
    }
  }

  let snapshotHistory: SnapshotRelayError[] = loadSnapshotHistory();

  $: snapshotRelayError = (() => {
    if (!dhtHealth) return null;
    const message = formatHealthMessage(dhtHealth.lastRelayError || dhtHealth.lastError);
    if (!message || message === $t('network.dht.health.none')) return null;

    const atMs =
      (dhtHealth.lastRelayErrorAt ?? dhtHealth.lastErrorAt ?? 0) * 1000;
    if (relayErrorClearedAt && atMs < relayErrorClearedAt) return null;
    return {
      message,
      type: dhtHealth.lastRelayErrorType ?? 'relay_error',
      timestamp: atMs || Date.now(),
      relayId: dhtHealth.activeRelayPeerId ?? 'unknown'
    };
  })();

  // Accumulate snapshot-derived relay errors instead of replacing them
  $: {
    if (snapshotRelayError && snapshotRelayError.timestamp >= relayErrorClearedAt) {
      const exists = snapshotHistory.some(
        (e) =>
          e.timestamp === snapshotRelayError.timestamp &&
          e.message === snapshotRelayError.message &&
          e.relayId === snapshotRelayError.relayId
      );
      if (!exists) {
        snapshotHistory = [snapshotRelayError, ...snapshotHistory].slice(0, 100);
        persistSnapshotHistory(snapshotHistory);
      }
    }
  }

  $: combinedRelayErrors = [...snapshotHistory, ...filteredRelayErrors];
  $: dedupRelayErrors = (() => {
    const seen = new Set<string>();
    const out: typeof combinedRelayErrors = [];
    for (const err of combinedRelayErrors) {
      const key = `${err.relayId}-${err.type}-${err.message}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(err);
    }
    return out;
  })();

  function clearRelayErrors() {
    relayErrorClearedAt = Date.now();
    relayErrorService.clearErrorLog();
    snapshotHistory = [];
    persistSnapshotHistory(snapshotHistory);
  }

  onMount(() => {
    settingsUnsubscribe = settings.subscribe(applySettingsState);

    // Load settings and start status checking
    (async () => {
      await loadSettings();
      await pollHealth();

      // Periodically check DHT status (every 3 seconds)
      statusCheckInterval = window.setInterval(checkDhtStatus, 3000);
      // Poll health snapshot (every 5 seconds) to reflect backend relay state
      healthPollInterval = window.setInterval(pollHealth, 5000);

      // Initialize relay error service with preferred relays
      const preferredRelays = get(settings).preferredRelays || [];

      if (preferredRelays.length > 0 || autoRelayEnabled) {
        await relayErrorService.initialize(preferredRelays, autoRelayEnabled);

        // Attempt to connect to best relay if AutoRelay is enabled
        const stats = get(relayErrorService.relayStats);
        const hasRelays = stats.totalRelays > 0;
        if (autoRelayEnabled && dhtIsRunning && hasRelays) {
          try {
            const result = await relayErrorService.connectToRelay();
            if (!result.success) {
              console.warn('Failed to connect to relay:', result.error);
            }
          } catch (error) {
            console.error('Error connecting to relay:', error);
          }
        } else if (autoRelayEnabled && !hasRelays) {
          console.info('AutoRelay enabled but no preferred relays configured; skipping relay connection attempt.');
        }
      }

      // Load saved health check interval
      try {
        const saved = localStorage.getItem('relayHealthCheckInterval');
        if (saved) {
          healthCheckInterval = parseInt(saved);
          relayErrorService.setHealthCheckInterval(healthCheckInterval);
        }
      } catch (e) {
        console.warn('Failed to load health check interval:', e);
      }

      // Start health checks
      relayErrorService.startHealthChecks();
      isHealthCheckRunning = true;
    })();

    // Cleanup interval on unmount
    return () => {
      if (statusCheckInterval !== undefined) {
        clearInterval(statusCheckInterval);
      }
      if (healthPollInterval !== undefined) {
        clearInterval(healthPollInterval);
      }
      settingsUnsubscribe?.();

      // Stop health checks
      relayErrorService.stopHealthChecks();
    };
  });

  async function pollHealth() {
    if (!dhtIsRunning) return;

    try {
      const health = await dhtService.getHealth();
      if (health) {
        dhtHealth = health;
        autoRelayEnabled = health.autorelayEnabled;
        // Keep relay error service in sync with backend active relay
        relayErrorService.syncFromHealthSnapshot(health);
      }
    } catch (error) {
      console.error('Failed to poll DHT health:', error);
    }
  }

  function updateHealthCheckInterval() {
    if (healthCheckInterval < 10) healthCheckInterval = 10;
    if (healthCheckInterval > 300) healthCheckInterval = 300;
    
    relayErrorService.setHealthCheckInterval(healthCheckInterval);
    
    // Save to localStorage
    try {
      localStorage.setItem('relayHealthCheckInterval', healthCheckInterval.toString());
      showToast(`Health check interval updated to ${healthCheckInterval}s`, 'success');
    } catch (e) {
      console.warn('Failed to save health check interval:', e);
    }
  }

  function toggleHealthChecks() {
    if (isHealthCheckRunning) {
      relayErrorService.stopHealthChecks();
      isHealthCheckRunning = false;
      showToast('Health checks stopped', 'info');
    } else {
      relayErrorService.startHealthChecks();
      isHealthCheckRunning = true;
      showToast('Health checks started', 'success');
    }
  }
</script>

<div class="space-y-6">
  <div class="mb-8">
    <h1 class="text-3xl font-bold">{$t('relay.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('relay.subtitle')}</p>
  </div>

  <div class="mb-6">
    <!-- AutoRelay Client Settings -->
    <Card class="p-6">
      <div class="flex items-start gap-3 mb-4">
        <SettingsIcon class="w-6 h-6 text-purple-600" />
        <div>
          <h2 class="text-xl font-bold text-gray-900">{$t('relay.client.title')}</h2>
          <p class="text-sm text-gray-600">{$t('relay.client.subtitle')}</p>
        </div>
      </div>

      <div class="space-y-4">
        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="enable-autorelay"
            bind:checked={autoRelayEnabled}
            on:change={handleAutorelayToggle}
            disabled={isRestartingAutorelay}
          />
          <Label for="enable-autorelay" class="cursor-pointer">
            {$t('relay.client.enableAutorelay')}
          </Label>
        </div>

        {#if autoRelayEnabled}
          <div class="bg-purple-50 border border-purple-200 rounded-lg p-3">
            <p class="text-sm text-purple-900">
              <strong>{$t('relay.client.howItWorks')}</strong>
            </p>
            <p class="text-xs text-purple-700 mt-1">
              {$t('relay.client.description')}
            </p>
          </div>
        {/if}
      </div>
    </Card>
  </div>

  <Card class="p-6">
    <h3 class="text-lg font-semibold mb-4 flex items-center gap-2">
      <RefreshCw class="h-5 w-5" />
      Health Check Configuration
    </h3>

    <div class="space-y-4">
      <div class="flex items-center justify-between">
        <div>
          <Label class="text-sm font-medium">Health Check Status</Label>
          <p class="text-xs text-muted-foreground mt-1">
            {isHealthCheckRunning ? 'Automatically checking relay health' : 'Health checks paused'}
          </p>
        </div>
        <button
          on:click={toggleHealthChecks}
          class="px-4 py-2 rounded-md text-sm font-medium transition-colors {isHealthCheckRunning
            ? 'bg-green-100 text-green-700 hover:bg-green-200'
            : 'bg-gray-100 text-gray-700 hover:bg-gray-200'}"
        >
          {isHealthCheckRunning ? 'Running' : 'Stopped'}
        </button>
      </div>

      <div class="space-y-2">
        <Label for="health-check-interval" class="text-sm font-medium">
          Check Interval (seconds)
        </Label>
        <div class="flex items-center gap-3">
          <input
            id="health-check-interval"
            type="number"
            min="10"
            max="300"
            step="5"
            bind:value={healthCheckInterval}
            class="flex-1 px-3 py-2 border border-input rounded-md text-sm"
            disabled={!isHealthCheckRunning}
          />
          <button
            on:click={updateHealthCheckInterval}
            disabled={!isHealthCheckRunning}
            class="px-4 py-2 bg-primary text-primary-foreground rounded-md text-sm font-medium hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Apply
          </button>
        </div>
        <p class="text-xs text-muted-foreground">
          How often to check relay connectivity (10-300 seconds). Lower values detect issues faster but use more resources.
        </p>
      </div>

      <div class="pt-4 border-t border-border">
        <div class="flex items-center justify-between text-sm">
          <span class="text-muted-foreground">Next check in:</span>
          <span class="font-medium">
            {isHealthCheckRunning ? `~${healthCheckInterval}s` : 'N/A'}
          </span>
        </div>
      </div>
    </div>
  </Card>

  {#if dhtHealth}
    <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <div>
          <p class="text-xs uppercase text-muted-foreground">Relay status</p>
          <h3 class="text-lg font-semibold text-foreground">Active relay snapshot</h3>
        </div>
        <div class="px-3 py-1 rounded-full text-xs font-semibold"
          class:bg-green-100={dhtHealth.autorelayEnabled}
          class:text-green-800={dhtHealth.autorelayEnabled}
          class:bg-gray-100={!dhtHealth.autorelayEnabled}
          class:text-gray-800={!dhtHealth.autorelayEnabled}
        >
          {dhtHealth.autorelayEnabled ? $t('network.dht.relay.enabled') : $t('network.dht.relay.disabled')}
        </div>
      </div>

      <div class="grid gap-3 md:grid-cols-2 lg:grid-cols-3">
        <div class="bg-muted/40 rounded-lg p-3 border border-muted/40">
          <p class="text-xs uppercase text-muted-foreground">Active relay</p>
          <p class="text-sm font-mono mt-1 break-all">{dhtHealth.activeRelayPeerId ?? $t('network.dht.relay.noPeer')}</p>
          <p class="text-xs text-muted-foreground mt-1">
            Status: {dhtHealth.relayReservationStatus ?? $t('network.dht.relay.pending')}
          </p>
        </div>
        <div class="bg-muted/40 rounded-lg p-3 border border-muted/40">
          <p class="text-xs uppercase text-muted-foreground">Pool</p>
          <p class="text-sm font-medium mt-1">
            {dhtHealth.totalRelaysInPool ?? 0} total · {dhtHealth.activeRelayCount ?? 0} active
          </p>
          <p class="text-xs text-muted-foreground mt-1">Renewals: {dhtHealth.reservationRenewals ?? 0}</p>
        </div>
        <div class="bg-muted/40 rounded-lg p-3 border border-muted/40">
          <p class="text-xs uppercase text-muted-foreground">Health</p>
          <p class="text-sm font-medium mt-1">
            {#if typeof dhtHealth.relayHealthScore === 'number'}
              {(dhtHealth.relayHealthScore * 100).toFixed(0)}%
            {:else}
              N/A
            {/if}
          </p>
          <p class="text-xs text-muted-foreground mt-1">
            Last renewal: {dhtHealth.lastReservationRenewal ? formatNatTimestamp(dhtHealth.lastReservationRenewal) : $t('network.dht.health.never')}
          </p>
        </div>
      </div>
    </Card>
  {/if}

  <!-- Relay Error Monitor -->
  {#if autoRelayEnabled && dhtIsRunning === true}
    <div class="mt-6">
      <h2 class="text-2xl font-bold text-gray-900 mb-4">{$t('relay.monitoring.title')}</h2>
      <RelayErrorMonitor />
    </div>
  {/if}

  <!-- Relay Error Log -->
  <Card class="p-6 mt-6">
    <div class="flex items-center justify-between mb-4">
      <h3 class="text-lg font-semibold text-foreground">Relay Error Log</h3>
      <Button size="sm" variant="outline" on:click={clearRelayErrors}>
        Clear
      </Button>
    </div>
    {#if dedupRelayErrors.length > 0}
      <div class="max-h-72 overflow-y-auto space-y-2">
        {#each dedupRelayErrors as error}
          <div class="border-l-4 border-red-500 pl-3 py-2 bg-red-50 rounded">
            <div class="flex items-center justify-between">
              <span class="text-xs font-semibold text-red-700">{error.type}</span>
              <span class="text-xs text-muted-foreground">{formatRelayErrorTimestamp(error.timestamp)}</span>
            </div>
            <p class="text-sm text-gray-800 break-words">{error.message}</p>
            <p class="text-xs text-gray-600 mt-1">
              Relay {error.relayId} {#if typeof error.retryCount === 'number'}• Retry {error.retryCount}{/if}
            </p>
          </div>
        {/each}
      </div>
    {:else}
      <p class="text-sm text-muted-foreground">No relay errors recorded.</p>
    {/if}
  </Card>
</div>

