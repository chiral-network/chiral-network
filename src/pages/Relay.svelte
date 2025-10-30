<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { invoke } from '@tauri-apps/api/core';
  import { settings } from '$lib/stores';
  import { dhtService } from '$lib/dht';
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
  let dhtIsRunning = false;
  let relayServerAlias = '';

  // AutoRelay client settings
  let autoRelayEnabled = true;
  let preferredRelaysText = '';

  async function loadSettings() {
    // Load settings from localStorage
    const stored = localStorage.getItem('chiralSettings');
    if (stored) {
      try {
        const loadedSettings = JSON.parse(stored);
        relayServerEnabled = loadedSettings.enableRelayServer ?? false;
        autoRelayEnabled = loadedSettings.enableAutorelay ?? true;
        preferredRelaysText = (loadedSettings.preferredRelays || []).join('\n');
        relayServerAlias = loadedSettings.relayServerAlias ?? '';
      } catch (e) {
        console.error('Failed to load settings:', e);
      }
    }

    // Check if DHT is running
    try {
      const peerId = await invoke<string | null>('get_dht_peer_id');
      dhtIsRunning = peerId !== null;
      relayServerRunning = dhtIsRunning && relayServerEnabled;
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
      preferredRelays: preferredRelaysText
        .split('\n')
        .map((r) => r.trim())
        .filter((r) => r.length > 0),
      relayServerAlias: relayServerAlias.trim(),
    };

    localStorage.setItem('chiralSettings', JSON.stringify(currentSettings));
    settings.set(currentSettings as any);
  }

  async function toggleRelayServer() {
    if (!dhtIsRunning) {
      alert('DHT is not running. Please start the network first from the Network page.');
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

      // Get current DHT config
      const currentSettings = JSON.parse(localStorage.getItem('chiralSettings') || '{}');

      // Stop DHT
      await dhtService.stop();

      // Wait a bit for cleanup
      await new Promise(resolve => setTimeout(resolve, 500));

      // Start with new config
      await dhtService.start({
        port: currentSettings.port || 4001,
        bootstrapNodes: [], // Will use default bootstrap nodes
        enableAutonat: currentSettings.enableAutonat,
        autonatProbeIntervalSeconds: currentSettings.autonatProbeInterval,
        autonatServers: currentSettings.autonatServers || [],
        enableAutorelay: currentSettings.enableAutorelay,
        preferredRelays: currentSettings.preferredRelays || [],
        enableRelayServer: relayServerEnabled,
        relayServerAlias: currentSettings.relayServerAlias || '',
        chunkSizeKb: currentSettings.chunkSize,
        cacheSizeMb: currentSettings.cacheSize,
      });

      relayServerRunning = relayServerEnabled;
      dhtIsRunning = true;

      console.log(`Relay server ${relayServerEnabled ? 'enabled' : 'disabled'}`);
    } catch (error) {
      console.error('Failed to toggle relay server:', error);
      alert(`Failed to toggle relay server: ${error}`);
      // Revert on error
      relayServerEnabled = !relayServerEnabled;
      await saveSettings();
    } finally {
      isToggling = false;
    }
  }

  function updatePreferredRelays() {
    saveSettings();
  }

  onMount(async () => {
    await loadSettings();

    // Initialize relay error service with preferred relays
    const preferredRelays = preferredRelaysText
      .split('\n')
      .map((r) => r.trim())
      .filter((r) => r.length > 0);

    if (preferredRelays.length > 0 || autoRelayEnabled) {
      await relayErrorService.initialize(preferredRelays, autoRelayEnabled);

      // Attempt to connect to best relay if AutoRelay is enabled
      if (autoRelayEnabled && dhtIsRunning) {
        try {
          const result = await relayErrorService.connectToRelay();
          if (result.success) {
            console.log('Successfully connected to relay via error service');
          } else {
            console.warn('Failed to connect to relay:', result.error);
          }
        } catch (error) {
          console.error('Error connecting to relay:', error);
        }
      }
    }
  });
</script>

<div class="p-6 max-w-7xl mx-auto">
  <div class="mb-8">
    <h1 class="text-3xl font-bold text-gray-900">{$t('relay.title')}</h1>
    <p class="mt-2 text-gray-600">{$t('relay.subtitle')}</p>
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
          <Label for="relay-alias">Relay Server Alias (Public Name)</Label>
          <input
            type="text"
            id="relay-alias"
            bind:value={relayServerAlias}
            on:blur={saveSettings}
            placeholder="e.g., Alice's Fast Relay 🚀"
            maxlength="50"
            class="w-full border rounded-md p-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
          <p class="text-xs text-gray-500 mt-1">
            This friendly name will appear in logs and when other nodes bootstrap through your relay
          </p>
        </div>

        {#if !dhtIsRunning}
          <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-3">
            <p class="text-sm font-semibold text-yellow-900">
              DHT Network Not Running
            </p>
            <p class="text-xs text-yellow-700 mt-1">
              Please start the network from the Network page first.
            </p>
          </div>
        {/if}

        <div class="flex items-center justify-between">
          <Button
            on:click={toggleRelayServer}
            disabled={!dhtIsRunning || isToggling}
            variant={relayServerEnabled ? 'destructive' : 'default'}
            class="w-full"
          >
            {#if isToggling}
              {relayServerEnabled ? 'Disabling...' : 'Enabling...'}
            {:else if relayServerEnabled}
              <WifiOff class="w-4 h-4 mr-2" />
              Disable Relay Server
            {:else}
              <Wifi class="w-4 h-4 mr-2" />
              Enable Relay Server
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
                <span class="text-xs text-green-700">Broadcasting as:</span>
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
            on:change={saveSettings}
          />
          <Label for="enable-autorelay" class="cursor-pointer">
            {$t('relay.client.enableAutorelay')}
          </Label>
        </div>

        {#if autoRelayEnabled}
          <div>
            <Label for="preferred-relays">{$t('relay.client.preferredRelays')}</Label>
            <textarea
              id="preferred-relays"
              bind:value={preferredRelaysText}
              on:blur={updatePreferredRelays}
              placeholder="/ip4/relay.example.com/tcp/4001/p2p/QmRelayId&#10;One multiaddr per line"
              rows="4"
              class="font-mono text-sm w-full border rounded-md p-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
            ></textarea>
            <p class="text-xs text-gray-500 mt-1">
              {$t('relay.client.preferredRelaysHint')}
            </p>
          </div>

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

  <!-- Relay Error Monitor -->
  {#if autoRelayEnabled && dhtIsRunning}
    <div class="mt-6">
      <h2 class="text-2xl font-bold text-gray-900 mb-4">Relay Health & Monitoring</h2>
      <RelayErrorMonitor />
    </div>
  {/if}
</div>
