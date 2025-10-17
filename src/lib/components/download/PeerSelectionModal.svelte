<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import { Server, Zap, TrendingUp, DollarSign, Clock, X } from 'lucide-svelte';
  import { toHumanReadableSize } from '$lib/utils';

  export let show = false;
  export let fileName: string;
  export let fileSize: number;
  export let peers: PeerInfo[];
  export let mode: 'auto' | 'manual' = 'auto';

  const dispatch = createEventDispatcher<{
    confirm: void;
    cancel: void;
  }>();

  export interface PeerInfo {
    peerId: string;
    location?: string;
    latency_ms?: number;
    bandwidth_kbps?: number;
    reliability_score: number;
    price_per_mb: number;
    selected: boolean;
    percentage: number;
  }

  // Calculate total cost
  $: totalCost = peers
    .filter(p => p.selected)
    .reduce((sum, p) => {
      const peerCost = (fileSize / 1024 / 1024) * p.price_per_mb;
      return sum + (mode === 'manual' ? peerCost * (p.percentage / 100) : peerCost / peers.filter(p => p.selected).length);
    }, 0);

  // Format speed
  function formatSpeed(kbps?: number): string {
    if (!kbps) return 'Unknown';
    if (kbps > 1024) return `${(kbps / 1024).toFixed(1)} MB/s`;
    return `${kbps.toFixed(0)} KB/s`;
  }

  // Get reputation stars
  function getStars(score: number): string {
    const stars = Math.round(score * 5);
    return '★'.repeat(stars) + '☆'.repeat(5 - stars);
  }

  // Auto-balance percentages when a peer is toggled
  function rebalancePercentages() {
    const selectedPeers = peers.filter(p => p.selected);
    if (selectedPeers.length === 0) return;

    const equal = Math.floor(100 / selectedPeers.length);
    const remainder = 100 - (equal * selectedPeers.length);

    selectedPeers.forEach((peer, i) => {
      peer.percentage = equal + (i === 0 ? remainder : 0);
    });
  }

  function togglePeer(peerId: string) {
    const peer = peers.find(p => p.peerId === peerId);
    if (peer) {
      peer.selected = !peer.selected;
      if (mode === 'manual') {
        rebalancePercentages();
      }
    }
  }

  function handleCancel() {
    dispatch('cancel');
  }

  function handleConfirm() {
    dispatch('confirm');
  }

  // Calculate total allocation for validation
  $: totalAllocation = mode === 'manual'
    ? peers.filter(p => p.selected).reduce((sum, p) => sum + p.percentage, 0)
    : 100;

  $: isValidAllocation = totalAllocation === 100;
  $: selectedPeerCount = peers.filter(p => p.selected).length;
</script>

{#if show}
  <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
    <Card class="w-full max-w-5xl max-h-[90vh] overflow-auto p-6 relative">
      <button
        on:click={handleCancel}
        class="absolute top-4 right-4 p-2 hover:bg-muted rounded-full transition-colors"
        aria-label="Close"
      >
        <X class="h-5 w-5 text-muted-foreground" />
      </button>

      <div class="space-y-6">
        <!-- Header -->
        <div>
          <h2 class="text-2xl font-bold mb-2">Select Download Peers</h2>
          <div class="flex items-center gap-2 text-muted-foreground flex-wrap">
            <span class="font-medium">{fileName}</span>
            <span>•</span>
            <span>{toHumanReadableSize(fileSize)}</span>
            <span>•</span>
            <Badge class="bg-blue-100 dark:bg-blue-900/30 text-blue-800 dark:text-blue-300">
              {peers.length} {peers.length === 1 ? 'Peer' : 'Peers'} Available
            </Badge>
          </div>
        </div>

        <!-- Mode Toggle -->
        <div class="flex gap-2">
          <Button
            variant={mode === 'auto' ? 'default' : 'outline'}
            size="sm"
            on:click={() => { mode = 'auto'; peers.forEach(p => p.selected = true); }}
          >
            <Zap class="h-4 w-4 mr-2" />
            Auto-select (Recommended)
          </Button>
          <Button
            variant={mode === 'manual' ? 'default' : 'outline'}
            size="sm"
            on:click={() => { mode = 'manual'; rebalancePercentages(); }}
          >
            Manual Selection
          </Button>
        </div>

        <!-- Auto Mode Description -->
        {#if mode === 'auto'}
          <div class="bg-blue-50 dark:bg-blue-900/20 p-4 rounded-lg border border-blue-200 dark:border-blue-800">
            <p class="text-sm text-blue-900 dark:text-blue-100">
              <span class="font-semibold">Smart Selection:</span> The system will automatically choose the best peers
              based on speed, reliability, and cost. All available peers are shown below for transparency.
            </p>
          </div>
        {/if}

        <!-- Peer Table -->
        <div class="border rounded-lg overflow-hidden">
          <div class="overflow-x-auto">
            <table class="w-full">
              <thead class="bg-muted">
                <tr>
                  {#if mode === 'manual'}
                    <th class="p-3 text-left text-xs font-medium uppercase tracking-wide">Select</th>
                  {/if}
                  <th class="p-3 text-left text-xs font-medium uppercase tracking-wide">Peer ID</th>
                  <th class="p-3 text-left text-xs font-medium uppercase tracking-wide">Speed</th>
                  <th class="p-3 text-left text-xs font-medium uppercase tracking-wide">Reputation</th>
                  <th class="p-3 text-left text-xs font-medium uppercase tracking-wide">Latency</th>
                  <th class="p-3 text-left text-xs font-medium uppercase tracking-wide">Price/MB</th>
                  {#if mode === 'manual'}
                    <th class="p-3 text-left text-xs font-medium uppercase tracking-wide">Share %</th>
                  {/if}
                </tr>
              </thead>
              <tbody>
                {#each peers as peer, index}
                  <tr class="border-t hover:bg-muted/50 transition-colors {mode === 'auto' ? 'bg-muted/30' : ''}">
                    {#if mode === 'manual'}
                      <td class="p-3">
                        <input
                          type="checkbox"
                          checked={peer.selected}
                          on:change={() => togglePeer(peer.peerId)}
                          class="h-4 w-4 rounded border-gray-300 text-primary focus:ring-2 focus:ring-primary cursor-pointer"
                        />
                      </td>
                    {/if}
                    <td class="p-3">
                      <div class="flex items-center gap-2">
                        <div class="h-2 w-2 rounded-full bg-emerald-500"></div>
                        <code class="font-mono text-sm">{peer.peerId.slice(0, 12)}...</code>
                      </div>
                    </td>
                    <td class="p-3">
                      <div class="flex items-center gap-1 text-sm">
                        <TrendingUp class="h-3.5 w-3.5 text-muted-foreground" />
                        {formatSpeed(peer.bandwidth_kbps)}
                      </div>
                    </td>
                    <td class="p-3">
                      <span class="text-yellow-500 text-sm">
                        {getStars(peer.reliability_score)}
                      </span>
                    </td>
                    <td class="p-3">
                      <div class="flex items-center gap-1 text-sm">
                        <Clock class="h-3.5 w-3.5 text-muted-foreground" />
                        {peer.latency_ms ? `${peer.latency_ms}ms` : 'Unknown'}
                      </div>
                    </td>
                    <td class="p-3">
                      <div class="flex items-center gap-1 text-sm">
                        <DollarSign class="h-3.5 w-3.5 text-muted-foreground" />
                        {peer.price_per_mb.toFixed(4)}
                      </div>
                    </td>
                    {#if mode === 'manual'}
                      <td class="p-3">
                        {#if peer.selected}
                          <div class="flex items-center gap-1">
                            <input
                              type="number"
                              bind:value={peer.percentage}
                              min="1"
                              max="100"
                              class="w-16 px-2 py-1 border rounded text-sm"
                            />
                            <span class="text-sm">%</span>
                          </div>
                        {:else}
                          <span class="text-muted-foreground text-sm">—</span>
                        {/if}
                      </td>
                    {/if}
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>

        <!-- Summary -->
        <div class="bg-muted/50 p-4 rounded-lg border space-y-2">
          <div class="flex justify-between items-center">
            <span class="font-medium text-sm">Selected Peers:</span>
            <Badge variant="secondary">
              <Server class="h-3.5 w-3.5 mr-1" />
              {selectedPeerCount} of {peers.length}
            </Badge>
          </div>
          <div class="flex justify-between items-center">
            <span class="font-medium text-sm">Estimated Cost:</span>
            <span class="text-green-600 dark:text-green-400 font-bold">
              ${totalCost.toFixed(4)} CHR
            </span>
          </div>
          {#if mode === 'manual'}
            <div class="flex justify-between items-center">
              <span class="font-medium text-sm">Total Allocation:</span>
              <span class:text-red-500={!isValidAllocation} class="font-semibold">
                {totalAllocation}%
              </span>
            </div>
            {#if !isValidAllocation}
              <p class="text-xs text-red-500 mt-1">
                Total allocation must equal 100%
              </p>
            {/if}
          {/if}
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-3 pt-2">
          <Button
            variant="outline"
            on:click={handleCancel}
          >
            Cancel
          </Button>
          <Button
            on:click={handleConfirm}
            disabled={mode === 'manual' && (!isValidAllocation || selectedPeerCount === 0)}
          >
            <Server class="h-4 w-4 mr-2" />
            Start Download ({selectedPeerCount} {selectedPeerCount === 1 ? 'peer' : 'peers'})
          </Button>
        </div>
      </div>
    </Card>
  </div>
{/if}
