

<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import { Server, Zap, TrendingUp, Clock, X, Download } from 'lucide-svelte';
  import { toHumanReadableSize } from '$lib/utils';

  import { costFromPricePerMb, pickLowestPricePeer, weightedTotalCost } from '$lib/utils/pricing';
  import type { CompleteFileMetadata } from '$lib/dht';
  import { Protocol } from '$lib/services/contentProtocols/types';
  import PeerSelectionService, { type PeerMetrics } from '$lib/services/peerSelectionService';
  import { PROTOCOL_BADGES } from '$lib/stores';
  export interface PeerInfo {
    peerId: string;
    walletAddress?: string;
    location?: string;
    latency_ms?: number;
    bandwidth_kbps?: number;
    reliability_score: number;
    price_per_mb: number;
    offerSource?: 'seeder' | 'fallback';
    selected: boolean;
    percentage: number;
    supportedProtocols: Protocol[];
  }
  interface Props {
    meta: CompleteFileMetadata,
    isSeeding:boolean,
    availableProtocols: Protocol[],
    cancel: ()=> void
    confirm: ( selectedPeerIds: string[],
    selectedProtocol: Protocol) => void,
    showPeerSelectionModal: boolean,
  }
  
  let {
    meta,
    isSeeding,
    availableProtocols,
    cancel,
    confirm,
    showPeerSelectionModal = $bindable()
  }: Props = $props();

  function buildPeerInfoList(
    meta: CompleteFileMetadata,
    options: {
      metrics?: PeerMetrics[];
    } = {},
  ): PeerInfo[] {
    console.log(options)
    const metricsById = new Map(
      (options.metrics ?? []).map((metric) => [metric.peer_id, metric]),
    );
    console.log(metricsById)

    return Object.entries(meta.seederInfo).map(([peerId, seederMeta]) => {
      console.log(peerId)
      const metric = metricsById.get(peerId);
      console.log(metric)
      return {
        peerId,
        walletAddress: seederMeta.general.walletAddress,
        price_per_mb:
        seederMeta.fileSpecific.pricePerMb ?? seederMeta.general.defaultPricePerMb,
        selected: false,
        percentage: 0,
        supportedProtocols: seederMeta.fileSpecific.supportedProtocols as Protocol[],
        reliability_score: metric?.reliability_score ?? 0,
        latency_ms: metric?.latency_ms,
        bandwidth_kbps: metric?.bandwidth_kbps,
      };
    });
  }

  // derived from completeFileMetadata's SeederCompleteMetadata's supportedProtocols.
  // let availableProtocols: Protocol[] = $derived.by(getProtocolsFromMeta(meta));

  // derive to availableProtocols[0], sorted alphabetically, user can change
  let selectedProtocol: Protocol | null = $state(availableProtocols[0] ?? null);
  let selectedProtocolLabel = $derived(
    selectedProtocol ? selectedProtocol.toUpperCase() : "",
  );

  // true if any one in seeder has same peerId as dht.getPeerId

  // true if selectedProtocol is BitTorrent
  let isTorrent = $derived(selectedProtocol === Protocol.BitTorrent)

  // derived from SeederCompleteMetadata, populate peerId, walletAddress, price_per_mb, default selected to false, percentage to 0;
  let peers: PeerInfo[] = $state([]);

  let mode = $state("auto");
  let paymentPeer = $derived(pickLowestPricePeer(peers));
  let weightedCost = $derived(weightedTotalCost({ bytes: meta.dhtRecord.fileSize, peers }));

  let estimatedPayment = $derived.by(()=>{
    if (isSeeding) return 0;
    if (!paymentPeer) return null;
    return costFromPricePerMb({ bytes: meta.dhtRecord.fileSize, pricePerMb: paymentPeer.price_per_mb });
  }
  )


  let totalAllocation = $derived(mode === 'manual'
    ? peers.filter(p => p.selected).reduce((sum, p) => sum + p.percentage, 0)
    : 100);

  let isValidAllocation = $derived(totalAllocation === 100);
  let selectedPeerCount = $derived(peers.filter(p => p.selected).length);
  let didInitPeers = $state(false);

  $effect(() => {
    if (!showPeerSelectionModal) {
      didInitPeers = false;
      return;
    }
    if (didInitPeers) return;
    didInitPeers = true;
    void initPeersForModal(meta);
  });

  $effect(() => {
    if (mode !== 'auto') return;
    if (peers.length === 0) return;
    if (peers.some((peer) => peer.selected)) return;
    peers = applyAutoSelection(peers);
  });

  $effect(() => {
    if (!selectedProtocol && availableProtocols.length > 0) {
      selectedProtocol = availableProtocols[0];
      return;
    }
    if (selectedProtocol && !availableProtocols.includes(selectedProtocol)) {
      selectedProtocol = availableProtocols[0] ?? null;
    }
  });

  function formatSpeed(kbps?: number): string {
    if (!kbps) return 'Unknown';
    if (kbps > 1024) return `${(kbps / 1024).toFixed(1)} MB/s`;
    return `${kbps.toFixed(0)} KB/s`;
  }

  async function initPeersForModal(currentMeta: CompleteFileMetadata) {
    try {
      const metrics = await PeerSelectionService.getPeerMetrics();
      console.log(metrics)
      peers = buildPeerInfoList(currentMeta, { metrics });
    } catch (error) {
      console.error('[download] failed to load peer metrics', error);
      peers = buildPeerInfoList(currentMeta);
    }
  }

  function applyAutoSelection(nextPeers: PeerInfo[]): PeerInfo[] {
    if (nextPeers.length === 0) return nextPeers;
    let lowest = nextPeers[0];
    for (const peer of nextPeers) {
      if (peer.price_per_mb < lowest.price_per_mb) {
        lowest = peer;
      }
    }
    return nextPeers.map((peer) => ({
      ...peer,
      selected: peer.peerId === lowest.peerId,
      percentage: peer.peerId === lowest.peerId ? 100 : 0,
    }));
  }

  function handleConfirm() {
    console.log("[download] confirm", {
      fileName: meta.dhtRecord.fileName,
      selectedProtocol,
      availableProtocols,
      selectedPeerIds: peers.filter((p) => p.selected).map((p) => p.peerId),
    });
    confirm(
      peers.filter((p)=>p.selected).map((p)=>p.peerId),
      selectedProtocol ?? Protocol.UNKNOWN,
    );
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
    cancel();
  }

</script>

{#if showPeerSelectionModal}
<div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
  <Card class="w-full max-w-5xl max-h-[90vh] overflow-auto p-6 relative">
    <button
      onclick={handleCancel}
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
          <span class="font-medium">{meta.dhtRecord.fileName}</span>
          {#if meta.dhtRecord.fileSize > 0}
            <span>•</span>
            <span>{toHumanReadableSize(meta.dhtRecord.fileSize )}</span>
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
        <div class="flex gap-2 flex-wrap">
          {#each availableProtocols as proto}
            {@const badge = PROTOCOL_BADGES[proto] ?? PROTOCOL_BADGES[Protocol.UNKNOWN]}
            {@const Icon = badge.icon}

            <Button
              variant="outline"
              size="sm"
              class={`${selectedProtocol === proto ?  'bg-blue-100 text-blue-800 border-transparent' : ''} ${selectedProtocol === proto ? 'ring-2 ring-primary/50 ring-offset-2 ring-offset-background shadow-sm' : 'hover:ring-1 hover:ring-foreground/20'} transition-shadow`}
              aria-pressed={selectedProtocol === proto}
              onclick={() => (selectedProtocol = proto)}
            >
              <Icon class="h-4 w-4 mr-2" />
              {badge.name}
            </Button>
          {/each}

        </div>
        {#if availableProtocols.length === 0}
          <p class="text-xs text-destructive">No download protocols available for this file.</p>
        {/if}
      </div>

      <!-- Peer Selection Mode -->
      <div class="space-y-2">
        <div class="text-sm font-semibold text-foreground/90">Peer Selection Mode</div>
        <div class="flex gap-2">
          <Button
            variant={mode === 'auto' ? 'default' : 'outline'}
            size="sm"
            onclick={() => { mode = 'auto'; peers = applyAutoSelection(peers); }}
          >
            <Zap class="h-4 w-4 mr-2" />
            Auto-select Peers (Recommended)
          </Button>
          <Button
            variant={mode === 'manual' ? 'default' : 'outline'}
            size="sm"
            onclick={() => { mode = 'manual'; rebalancePercentages(); }}
          >
            <Server class="h-4 w-4 mr-2" />
            Manual Peer Selection
          </Button>
        </div>
        <p class="text-xs text-muted-foreground">
          {#if mode === 'auto'}
            Automatically selects the best peers based on speed, reliability, and cost.
          {:else}
            Manually choose which peers to download from and set bandwidth allocation.
          {/if}
        </p>
      </div>

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
              {@const supportsProtocol = selectedProtocol ? peer.supportedProtocols.includes(selectedProtocol) : false}
                <tr class="border-t transition-colors {mode === 'auto' ? 'bg-muted/30' : ''} opacity-40" title={supportsProtocol || !selectedProtocol ? '' : `This peer does not support ${selectedProtocolLabel}`}>
                  {#if mode === 'manual'}
                    <td class="p-3">
                      <label class="sr-only" for="peer-select-{peer.peerId.slice(0, 12)}">Select peer {peer.peerId.slice(0, 12)}...</label>
                      <input
                        id="peer-select-{peer.peerId.slice(0, 12)}"
                        type="checkbox"
                        checked={peer.selected}
                        onchange={() => togglePeer(peer.peerId)}
                        disabled={!supportsProtocol}
                        class="h-4 w-4 rounded border-gray-300 text-primary focus:ring-2 focus:ring-primary {supportsProtocol ? 'cursor-pointer' : 'cursor-not-allowed opacity-50'}"
                      />
                    </td>
                  {/if}
                  <td class="p-3">
                    <div class="flex items-center gap-2">
                      <div class="h-2 w-2 rounded-full {supportsProtocol ? 'bg-emerald-500' : 'bg-gray-400'}"></div>
                      <code class="font-mono text-sm">{peer.peerId.slice(0, 12)}...</code>
                      {#if !supportsProtocol && selectedProtocol}
                        <span class="text-xs text-muted-foreground">(no {selectedProtocolLabel})</span>
                      {/if}
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
                      {peer.price_per_mb.toFixed(4)} Chiral
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
            <span class="font-medium text-sm">Estimated Payment:</span>
            <span class="text-green-600 dark:text-green-400 font-bold">
              {#if isSeeding}
                Free
              {:else if estimatedPayment === null}
                Loading...
              {:else}
                {estimatedPayment.toFixed(4)} Chiral
              {/if}
            </span>
          </div>
          {#if !isSeeding && !isTorrent}
            <div class="flex justify-between items-center">
              <span class="font-medium text-sm">Paying:</span>
              <span class="text-sm text-muted-foreground">
                {#if paymentPeer}
                  <code class="font-mono">{paymentPeer.peerId.slice(0, 12)}...</code>
                  <span class="ml-2">@ {paymentPeer.price_per_mb.toFixed(4)} / MB</span>
                {:else}
                  —
                {/if}
              </span>
            </div>
            <div class="flex justify-between items-center">
              <span class="font-medium text-sm">Blended Estimate:</span>
              <span class="text-sm text-muted-foreground">{Math.max(weightedCost, 0).toFixed(4)} Chiral</span>
            </div>
          {/if}
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
          onclick={handleCancel}
        >
          Cancel
        </Button>
        <Button
          onclick={handleConfirm}
        >
          <Download class="h-4 w-4 mr-2" />
          {`Start Download (${selectedPeerCount} ${selectedPeerCount === 1 ? 'peer' : 'peers'})`}
        </Button>
      </div>
    </div>
  </Card>
</div>
{/if}
