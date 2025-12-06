<script lang="ts">
  import { get } from 'svelte/store';
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { settings } from '$lib/stores';
  import type { AppSettings } from '$lib/stores';
  import { dhtService, type DhtHealth } from '$lib/dht';
  import { relayErrorService } from '$lib/services/relayErrorService';
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import RelayErrorMonitor from '$lib/components/RelayErrorMonitor.svelte';
  import { Wifi, WifiOff, Server, Settings as SettingsIcon } from 'lucide-svelte';

  // Relay server status
  let relayServerEnabled = false;
  let relayServerRunning = false;
  let isToggling = false;
  let dhtIsRunning: boolean | null = null;
  let relayServerAlias = '';
  let isRestartingAutorelay = false;
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
      const isRunning = await invoke<boolean>('is_dht_running').catch(() => false);
      dhtIsRunning = isRunning;
      
      // If DHT is running and relay server is enabled in settings, mark it as running
      if (isRunning && relayServerEnabled) {
        relayServerRunning = true;
      } else {
        relayServerRunning = false;
      }
    } catch (error) {
      console.error('Failed to check DHT status:', error);
      dhtIsRunning = false;
      relayServerRunning = false;
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
    });

    relayServerEnabled = currentSettings.enableRelayServer ?? relayServerEnabled;
    relayServerRunning = currentSettings.enableRelayServer ?? false;
    autoRelayEnabled = currentSettings.enableAutorelay ?? autoRelayEnabled;
    dhtIsRunning = true;

    return currentSettings;
  }

  async function toggleRelayServer() {
    if (!dhtIsRunning) {
      alert($t('relay.errors.dhtNotRunning'));
      return;
    }

    isToggling = true;
    try {
      // Toggle the setting
      relayServerEnabled = !relayServerEnabled;

      // Save to settings
      await saveSettings();

      // Restart DHT with new settings
      console.log('Restarting DHT with relay server:', relayServerEnabled);
      await restartDhtWithSettings();

      console.log(`Relay server ${relayServerEnabled ? 'enabled' : 'disabled'}`);
    } catch (error) {
      console.error('Failed to toggle relay server:', error);
      alert($t('relay.errors.toggleFailed', { values: { error } }));
      // Revert on error
      relayServerEnabled = !relayServerEnabled;
      await saveSettings();
    } finally {
      isToggling = false;
    }
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

  <div class="grid grid-cols-1 lg:grid-cols-2 gap-6 mb-6">
    <!-- Relay Server Control -->
    <Card class="p-6">
      <div class="flex items-start justify-between mb-4">
        <div class="flex items-center gap-3">
          <Server class="w-6 h-6 text-blue-600" />
          <div>
            <h2 class="text-xl font-bold text-gray-900">{$t('relay.server.title')}</h2>
            <p class="text-sm text-gray-600">{$t('relay.server.subtitle')}</p>
          </div>
        </div>
        <div
          class="px-3 py-1 rounded-full text-xs font-semibold"
          class:bg-green-100={relayServerRunning}
          class:text-green-800={relayServerRunning}
          class:bg-gray-100={!relayServerRunning}
          class:text-gray-800={!relayServerRunning}
        >
          {relayServerRunning ? $t('relay.server.running') : $t('relay.server.stopped')}
        </div>
      </div>

      <div class="space-y-4">
        <div class="bg-blue-50 border border-blue-200 rounded-lg p-4">
          <p class="text-sm text-blue-900">
            {$t('relay.server.description')}
          </p>
          <ul class="mt-2 text-sm text-blue-800 space-y-1">
            <li>• {$t('relay.server.benefit1')}</li>
            <li>• {$t('relay.server.benefit2')}</li>
            <li>• {$t('relay.server.benefit3')}</li>
          </ul>
        </div>

        <div>
          <Label for="relay-alias">{$t('relay.server.aliasLabel')}</Label>
          <input
            type="text"
            id="relay-alias"
            bind:value={relayServerAlias}
            on:blur={saveSettings}
            placeholder={$t('relay.server.aliasPlaceholder')}
            maxlength="50"
            class="w-full border rounded-md p-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
          <p class="text-xs text-gray-500 mt-1">
            {$t('relay.server.aliasHint')}
          </p>
        </div>

        {#if dhtIsRunning === false}
          <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-3">
            <p class="text-sm font-semibold text-yellow-900">
              {$t('relay.server.dhtNotRunning')}
            </p>
            <p class="text-xs text-yellow-700 mt-1">
              {$t('relay.server.dhtNotRunningHint')}
            </p>
          </div>
        {:else if dhtIsRunning === null}
          <div class="bg-blue-50 border border-blue-200 rounded-lg p-3">
            <p class="text-sm font-semibold text-blue-900">
              Network Not Started
            </p>
            <p class="text-xs text-blue-700 mt-1">
              Start the network from the Network page to enable relay functionality.
            </p>
          </div>
        {/if}

        <div class="flex items-center justify-between">
          <Button
            on:click={toggleRelayServer}
            disabled={dhtIsRunning !== true || isToggling}
            variant={relayServerEnabled ? 'destructive' : 'default'}
            class="w-full"
          >
            {#if isToggling}
              {relayServerEnabled ? $t('relay.server.disabling') : $t('relay.server.enabling')}
            {:else if relayServerEnabled}
              <WifiOff class="w-4 h-4 mr-2" />
              {$t('relay.server.disable')}
            {:else}
              <Wifi class="w-4 h-4 mr-2" />
              {$t('relay.server.enable')}
            {/if}
          </Button>
        </div>

        {#if relayServerRunning}
          <div class="bg-green-50 border border-green-200 rounded-lg p-4">
            <p class="text-sm font-semibold text-green-900">
              {$t('relay.server.activeMessage')}
            </p>
            {#if relayServerAlias.trim()}
              <div class="mt-2 flex items-center gap-2">
                <span class="text-xs text-green-700">{$t('relay.server.broadcastingAs')}</span>
                <span class="text-sm font-bold text-green-900 bg-green-100 px-2 py-1 rounded">
                  {relayServerAlias}
                </span>
              </div>
            {/if}
            <p class="text-xs text-green-700 mt-2">
              {$t('relay.server.earningReputation')}
            </p>
          </div>
        {/if}
      </div>
    </Card>

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
</div>
