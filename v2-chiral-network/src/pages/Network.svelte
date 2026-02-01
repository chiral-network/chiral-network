<script lang="ts">
  import { peers, networkStats, networkConnected } from '$lib/stores';
  import { dhtService } from '$lib/dhtService';
  import { Play, Square } from 'lucide-svelte';
  
  let isConnecting = false;
  let error = '';
  
  async function connectToNetwork() {
    isConnecting = true;
    error = '';
    try {
      await dhtService.start();
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to connect';
      console.error('Failed to connect:', err);
    } finally {
      isConnecting = false;
    }
  }
  
  async function disconnectFromNetwork() {
    try {
      await dhtService.stop();
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to disconnect';
      console.error('Failed to disconnect:', err);
    }
  }
  
  function formatDate(date: Date | number): string {
    const d = typeof date === 'number' ? new Date(date) : date;
    return d.toLocaleString();
  }
</script>

<div class="p-6">
  <h1 class="text-3xl font-bold mb-6">Network</h1>
  
  {#if error}
    <div class="bg-red-50 border-l-4 border-red-400 p-4 mb-6">
      <p class="text-sm text-red-800">{error}</p>
    </div>
  {/if}
  
  <div class="grid grid-cols-1 md:grid-cols-2 gap-6 mb-6">
    <div class="bg-white rounded-lg shadow p-6">
      <h2 class="text-xl font-semibold mb-4">Connection Status</h2>
      <div class="flex items-center gap-3 mb-4">
        <div class="w-3 h-3 rounded-full {$networkConnected ? 'bg-green-500' : 'bg-red-500'}"></div>
        <span class="font-medium">{$networkConnected ? 'Connected' : 'Disconnected'}</span>
      </div>
      
      {#if $networkConnected}
        <button
          on:click={disconnectFromNetwork}
          class="flex items-center gap-2 px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition"
        >
          <Square class="w-4 h-4" />
          <span>Disconnect</span>
        </button>
      {:else}
        <button
          on:click={connectToNetwork}
          disabled={isConnecting}
          class="flex items-center gap-2 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition disabled:opacity-50"
        >
          <Play class="w-4 h-4" />
          <span>{isConnecting ? 'Connecting...' : 'Connect'}</span>
        </button>
      {/if}
    </div>
    
    <div class="bg-white rounded-lg shadow p-6">
      <h2 class="text-xl font-semibold mb-4">Network Statistics</h2>
      <div class="space-y-2">
        <div class="flex justify-between">
          <span class="text-gray-600">Connected Peers:</span>
          <span class="font-medium">{$networkStats.connectedPeers}</span>
        </div>
        <div class="flex justify-between">
          <span class="text-gray-600">Total Peers:</span>
          <span class="font-medium">{$networkStats.totalPeers}</span>
        </div>
      </div>
    </div>
  </div>
  
  <div class="bg-white rounded-lg shadow p-6">
    <h2 class="text-xl font-semibold mb-4">Connected Peers</h2>
    
    {#if $peers.length === 0}
      <p class="text-gray-600">No peers connected</p>
    {:else}
      <div class="space-y-2">
        {#each $peers as peer}
          <div class="p-3 bg-gray-50 rounded-lg border border-gray-200">
            <div class="font-mono text-sm">{peer.id}</div>
            {#if peer.address}
              <div class="text-xs text-gray-500 mt-1">Address: {peer.address}</div>
            {/if}
            <div class="text-xs text-gray-500 mt-1">
              Last seen: {formatDate(peer.lastSeen)}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
