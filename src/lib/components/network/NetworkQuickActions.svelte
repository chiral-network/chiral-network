<script lang="ts">
  import { t } from 'svelte-i18n';
  import Button from '$lib/components/ui/button.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Card from '$lib/components/ui/card.svelte';
  import { 
    Search, 
    UserPlus, 
    Clipboard, 
    Link, 
    Play,
    Square,
    Zap 
  } from 'lucide-svelte';
  import { showToast } from '$lib/toast';
  import { createEventDispatcher } from 'svelte';
  import type { DhtHealth } from '$lib/dht';

  // Props
  export let dhtPeerId: string | null = null;
  export let dhtHealth: DhtHealth | null = null;
  export let dhtStatus: 'disconnected' | 'connecting' | 'connected' = 'disconnected';
  export let discoveryRunning = false;
  const dispatch = createEventDispatcher();
  let newPeerAddress = '';

  // Computed
  $: isConnected = dhtStatus === 'connected';
  $: isConnecting = dhtStatus === 'connecting';
  $: multiaddress = dhtHealth?.observedAddrs?.[0] ?? null;

  // Actions
  async function copyPeerId() {
    if (!dhtPeerId) {
      showToast($t('network.quickActions.copyPeerId.tooltipUnavailable'), 'warning');
      return;
    }
    try {
      await navigator.clipboard.writeText(dhtPeerId);
      showToast($t('network.quickActions.copyPeerId.success'), 'success');
    } catch {
      showToast('Failed to copy', 'error');
    }
  }

  async function copyMultiaddress() {
    if (!multiaddress) {
      showToast($t('network.quickActions.copyMultiaddr.tooltipUnavailable'), 'warning');
      return;
    }
    try {
      await navigator.clipboard.writeText(multiaddress);
      showToast($t('network.quickActions.copyMultiaddr.success'), 'success');
    } catch {
      showToast('Failed to copy', 'error');
    }
  }

  function handleDiscoverPeers() {
    dispatch('discover');
  }

  function handleAddPeer() {
    const address = newPeerAddress.trim();
    if (address) {
      dispatch('addPeer', { address });
      newPeerAddress = '';
    }
  }

  function handleStartDht() {
    dispatch('startDht');
  }

  function handleStopDht() {
    dispatch('stopDht');
  }
</script>

<Card class="p-4">
  <div class="flex items-center justify-between mb-4">
    <div class="flex items-center gap-2">
      <Zap class="h-5 w-5 text-primary" />
      <h3 class="font-semibold">{$t('network.quickActions.title')}</h3>
    </div>
    <Badge variant="outline">{$t('network.quickActions.badge')}</Badge>
  </div>

  <div class="flex flex-wrap gap-2">
    <!-- Copy Peer ID -->
    <Button 
      variant="outline" 
      size="sm"
      disabled={!dhtPeerId}
      on:click={copyPeerId}
      title={dhtPeerId 
        ? $t('network.quickActions.copyPeerId.tooltip')
        : $t('network.quickActions.copyPeerId.tooltipUnavailable')}
    >
      <Clipboard class="h-4 w-4 mr-1.5" />
      {$t('network.quickActions.copyPeerId.button')}
    </Button>

    <!-- Copy Multiaddress -->
    <Button 
      variant="outline" 
      size="sm"
      disabled={!multiaddress}
      on:click={copyMultiaddress}
      title={multiaddress 
        ? $t('network.quickActions.copyMultiaddr.tooltip')
        : $t('network.quickActions.copyMultiaddr.tooltipUnavailable')}
    >
      <Link class="h-4 w-4 mr-1.5" />
      {$t('network.quickActions.copyMultiaddr.button')}
    </Button>

    <!-- Discover Peers -->
    <Button 
      variant="outline" 
      size="sm"
      disabled={!isConnected || discoveryRunning}
      on:click={handleDiscoverPeers}
      title={$t('network.quickActions.discoverPeers.tooltip')}
    >
      <Search class="h-4 w-4 mr-1.5" />
      {discoveryRunning 
        ? $t('network.quickActions.discoverPeers.discovering')
        : $t('network.quickActions.discoverPeers.button')}
    </Button>

    <!-- Start/Stop DHT -->
    {#if isConnected || isConnecting}
      <Button 
        variant="outline" 
        size="sm"
        on:click={handleStopDht}
        title={$t('network.quickActions.startStopDht.stopTooltip')}
        class="text-red-600 border-red-200 hover:bg-red-50 hover:text-red-700 dark:text-red-400 dark:border-red-800 dark:hover:bg-red-950"
      >
        <Square class="h-4 w-4 mr-1.5" />
        {isConnecting 
          ? $t('network.quickActions.startStopDht.cancelButton')
          : $t('network.quickActions.startStopDht.stopButton')}
      </Button>
    {:else}
      <Button 
        variant="outline" 
        size="sm"
        on:click={handleStartDht}
        title={$t('network.quickActions.startStopDht.startTooltip')}
        class="text-emerald-600 border-emerald-200 hover:bg-emerald-50 hover:text-emerald-700 dark:text-emerald-400 dark:border-emerald-800 dark:hover:bg-emerald-950"
      >
        <Play class="h-4 w-4 mr-1.5" />
        {$t('network.quickActions.startStopDht.startButton')}
      </Button>
    {/if}
  </div>

  <!-- Add Peer Input Row -->
  <div class="flex gap-2 mt-3">
    <Input 
      placeholder={$t('network.quickActions.addPeer.placeholder')}
      class="h-9 text-sm flex-1" 
      bind:value={newPeerAddress}
      on:keydown={(e) => (e as unknown as KeyboardEvent).key === 'Enter' && handleAddPeer()}
    />
    <Button 
      size="sm" 
      variant="secondary" 
      disabled={!newPeerAddress.trim() || !isConnected}
      on:click={handleAddPeer}
      title={$t('network.quickActions.addPeer.tooltip')}
    >
      <UserPlus class="h-4 w-4" />
    </Button>
  </div>
</Card>

