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
  import Badge from '$lib/components/ui/badge.svelte';
  import RelayErrorMonitor from '$lib/components/RelayErrorMonitor.svelte';
  import { Settings as SettingsIcon } from 'lucide-svelte';

  // Relay server status
  let relayServerEnabled = false;
  let dhtIsRunning: boolean | null = null;
  let relayServerAlias = '';
  let dhtHealth: DhtHealth | null = null;

  // AutoRelay client settings
  let autoRelayEnabled = true;

  let settingsUnsubscribe: (() => void) | null = null;

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

</script>

<div class="space-y-6">
  <div class="mb-8">
    <h1 class="text-3xl font-bold">{$t('relay.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('relay.subtitle')}</p>
  </div>

  <div class="mb-6">
    <!-- AutoRelay Client Settings (managed from Network page) -->
    <Card class="p-6">
      <div class="flex items-start gap-3 mb-4">
        <SettingsIcon class="w-6 h-6 text-purple-600" />
        <div>
          <h2 class="text-xl font-bold text-gray-900">{$t('relay.client.title')}</h2>
          <p class="text-sm text-gray-600">{$t('relay.client.subtitle')}</p>
        </div>
        <Badge class="ml-auto" variant={autoRelayEnabled ? 'default' : 'secondary'}>
          {autoRelayEnabled ? $t('network.dht.relay.enabled') : $t('network.dht.relay.disabled')}
        </Badge>
      </div>

      <div class="space-y-3">
        <p class="text-sm text-muted-foreground">
          AutoRelay can be toggled from the Network page. Current status is displayed here for monitoring.
        </p>
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


