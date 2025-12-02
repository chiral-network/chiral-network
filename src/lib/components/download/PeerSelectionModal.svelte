<script context="module" lang="ts">
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
</script>

<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import { Server, Zap, TrendingUp, DollarSign, Clock, X, Download, Globe, Wifi } from 'lucide-svelte';
  import { toHumanReadableSize } from '$lib/utils';

  export let show = false;
  export let fileName: string;
  export let fileSize: number;
  export let peers: PeerInfo[];
  export let mode: 'auto' | 'manual' = 'auto';
  export let autoSelectionInfo: Array<{peerId: string; score: number; metrics: any}> | null = null;
  export let protocol: 'http' | 'webrtc' | 'bitswap' | 'bittorrent' | 'ed2k' | 'ftp' = 'http'; 
  export let isTorrent = false; // Flag to indicate torrent download (no peer selection needed)

  const dispatch = createEventDispatcher<{
    confirm: void;
    cancel: void;
  }>();

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
          <h2 class="text-2xl font-bold mb-2">{isTorrent ? 'Confirm Download' : 'Select Download Peers'}</h2>
          <div class="flex items-center gap-2 text-muted-foreground flex-wrap">
            <span class="font-medium">{fileName}</span>
            {#if fileSize > 0}
              <span>•</span>
              <span>{toHumanReadableSize(fileSize)}</span>
            {/if}
            {#if !isTorrent}
              <span>•</span>
              <Badge variant="secondary">
                {peers.length} {peers.length === 1 ? 'Peer' : 'Peers'} Available
              </Badge>
            {/if}
          </div>
        </div>

        {#if isTorrent}
          <!-- Simple torrent confirmation -->
          <div class="bg-muted/30 p-4 rounded-lg border">
            <p class="text-sm text-muted-foreground">
              Ready to start BitTorrent download. The torrent client will connect to peers automatically.
            </p>
          </div>
        {:else}
        <!-- Protocol Selection -->
        <div class="space-y-2">
          <div class="text-sm font-semibold text-foreground/90">Transfer Protocol</div>
          <div class="flex gap-2">
            <Button
              variant={protocol === 'http' ? 'default' : 'outline'}
              size="sm"
              on:click={() => protocol = 'http'}
            >
              <Globe class="h-4 w-4 mr-2" />
              HTTP
            </Button>
            <Button
              variant={protocol === 'webrtc' ? 'default' : 'outline'}
              size="sm"
              on:click={() => protocol = 'webrtc'}
            >
              <Wifi class="h-4 w-4 mr-2" />
              WebRTC
            </Button>
            <Button
              variant={protocol === 'bitswap' ? 'default' : 'outline'}
              size="sm"
              on:click={() => protocol = 'bitswap'}
            >
              <Zap class="h-4 w-4 mr-2" />
              Bitswap
            </Button>
          </div>
          <p class="text-xs text-muted-foreground">
            {#if protocol === 'http'}
              HTTP: Simple and fast for nodes with public IP addresses
            {:else if protocol === 'webrtc'}
              WebRTC: Peer-to-peer with NAT traversal (works behind firewalls)
            {:else if protocol === 'bitswap'}
              Bitswap: A peer-to-peer data exchange protocol for content-addressed data.
            {/if}
          </p>
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
          <div class="bg-muted/30 p-4 rounded-lg border space-y-3">
            <p class="text-sm">
              <span class="font-semibold">Smart Selection:</span> The system automatically chose the best peers
              based on speed, reliability, and cost.
            </p>

            {#if autoSelectionInfo && autoSelectionInfo.length > 0}
              <div class="space-y-2">
                <p class="text-xs font-semibold text-foreground/70 uppercase tracking-wide">
                  Selection Reasoning:
                </p>
                {#each autoSelectionInfo as info, index}
                  {@const peer = peers.find(p => p.peerId === info.peerId)}
                  {#if peer}
                    <div class="bg-card p-3 rounded-md border">
                      <div class="flex items-center justify-between mb-2">
                        <div class="flex items-center gap-2">
                          <Badge variant="default" class="text-xs">
                            Peer {index + 1}
                          </Badge>
                          <code class="text-xs font-mono text-muted-foreground">{info.peerId.slice(0, 12)}...</code>
                        </div>
                        <div class="text-xs font-semibold">
                          Score: {info.score.toFixed(3)}
                        </div>
                      </div>

                      <div class="grid grid-cols-2 gap-2 text-xs">
                        <div class="flex items-center gap-1">
                          <TrendingUp class="h-3 w-3 text-muted-foreground" />
                          <span class="text-muted-foreground">Speed:</span>
                          <span class="font-medium">{formatSpeed(peer.bandwidth_kbps)}</span>
                        </div>
                        <div class="flex items-center gap-1">
                          <Clock class="h-3 w-3 text-muted-foreground" />
                          <span class="text-muted-foreground">Latency:</span>
                          <span class="font-medium">{peer.latency_ms ? `${peer.latency_ms}ms` : 'Unknown'}</span>
                        </div>
                        <div>
                          <span class="text-muted-foreground">Reputation:</span>
                          <span class="text-yellow-500 ml-1">{getStars(peer.reliability_score)}</span>
                        </div>
                        <div class="flex items-center gap-1">
                          <DollarSign class="h-3 w-3 text-muted-foreground" />
                          <span class="text-muted-foreground">Price:</span>
                          <span class="font-medium">${peer.price_per_mb.toFixed(4)}/MB</span>
                        </div>
                      </div>

                      <div class="mt-2 pt-2 border-t">
                        <div class="flex items-center justify-between text-xs">
                          <span class="text-muted-foreground">Target Allocation:</span>
                          <span class="font-bold">{peer.percentage}%</span>
                        </div>
                      </div>
                    </div>
                  {/if}
                {/each}
              </div>

              <p class="text-xs text-muted-foreground mt-2">
                💡 Allocation percentages are weighted by peer quality scores. Better peers receive higher allocations.
              </p>
            {/if}
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
                {#each peers as peer}
                  <tr class="border-t hover:bg-muted/50 transition-colors {mode === 'auto' ? 'bg-muted/30' : ''}">
                    {#if mode === 'manual'}
                      <td class="p-3">
                        <label class="sr-only" for="peer-select-{peer.peerId.slice(0, 12)}">Select peer {peer.peerId.slice(0, 12)}...</label>
                        <input
                          id="peer-select-{peer.peerId.slice(0, 12)}"
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
                            <label class="sr-only" for="peer-percentage-{peer.peerId.slice(0, 12)}">Allocation percentage for peer {peer.peerId.slice(0, 12)}...</label>
                            <input
                              id="peer-percentage-{peer.peerId.slice(0, 12)}"
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

        {/if}
        <!-- End of conditional peer selection content -->

        <!-- Summary -->
        {#if !isTorrent}
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
        {/if}

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
            disabled={!isTorrent && mode === 'manual' && (!isValidAllocation || selectedPeerCount === 0)}
          >
            <Download class="h-4 w-4 mr-2" />
            {isTorrent ? 'Start Download' : `Start Download (${selectedPeerCount} ${selectedPeerCount === 1 ? 'peer' : 'peers'})`}
          </Button>
        </div>
      </div>
    </Card>
  </div>
{/if}
