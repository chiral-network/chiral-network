<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { invoke } from '@tauri-apps/api/core';
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import RelayReputationLeaderboard from '$lib/components/RelayReputationLeaderboard.svelte';
  import { Wifi, WifiOff, Server, Settings as SettingsIcon } from 'lucide-svelte';

  // Relay server status
  let relayServerEnabled = false;
  let relayServerRunning = false;
  let isStarting = false;

  // AutoRelay client settings
  let autoRelayEnabled = true;
  let preferredRelaysText = '';

  // Alias management
  interface RelayAlias {
    peerId: string;
    alias: string;
  }
  let aliases: RelayAlias[] = [];
  let editingAliasId: string | null = null;
  let editingAliasValue = '';

  async function loadRelayStatus() {
    try {
      // Check if DHT is running and if relay server is enabled
      const events = await invoke<string[]>('get_dht_events');
      relayServerRunning = events.length > 0; // Simplified check
    } catch (error) {
      console.error('Failed to load relay status:', error);
    }
  }

  async function toggleRelayServer() {
    if (relayServerRunning) {
      await stopRelayServer();
    } else {
      await startRelayServer();
    }
  }

  async function startRelayServer() {
    isStarting = true;
    try {
      // TODO: Need to implement dedicated relay server start command
      // For now, this would require restarting DHT with relay server enabled
      console.log('Starting relay server...');
      relayServerRunning = true;
      relayServerEnabled = true;
    } catch (error) {
      console.error('Failed to start relay server:', error);
    } finally {
      isStarting = false;
    }
  }

  async function stopRelayServer() {
    try {
      console.log('Stopping relay server...');
      relayServerRunning = false;
      relayServerEnabled = false;
    } catch (error) {
      console.error('Failed to stop relay server:', error);
    }
  }

  function updatePreferredRelays() {
    const relays = preferredRelaysText
      .split('\n')
      .map((r) => r.trim())
      .filter((r) => r.length > 0);
    console.log('Updated preferred relays:', relays);
  }

  onMount(() => {
    loadRelayStatus();
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

        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <input type="checkbox" id="enable-relay-server" bind:checked={relayServerEnabled} />
            <Label for="enable-relay-server" class="cursor-pointer">
              {$t('relay.server.enable')}
            </Label>
          </div>

          <Button
            on:click={toggleRelayServer}
            disabled={!relayServerEnabled || isStarting}
            variant={relayServerRunning ? 'destructive' : 'default'}
            class="min-w-32"
          >
            {#if isStarting}
              {$t('relay.server.starting')}
            {:else if relayServerRunning}
              <WifiOff class="w-4 h-4 mr-2" />
              {$t('relay.server.stop')}
            {:else}
              <Wifi class="w-4 h-4 mr-2" />
              {$t('relay.server.start')}
            {/if}
          </Button>
        </div>

        {#if relayServerRunning}
          <div class="bg-green-50 border border-green-200 rounded-lg p-3">
            <p class="text-sm font-semibold text-green-900">
              {$t('relay.server.activeMessage')}
            </p>
            <p class="text-xs text-green-700 mt-1">
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
          <input type="checkbox" id="enable-autorelay" bind:checked={autoRelayEnabled} />
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

  <!-- Relay Reputation Leaderboard -->
  <div class="mb-6">
    <RelayReputationLeaderboard />
  </div>
</div>
