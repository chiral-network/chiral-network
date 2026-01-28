<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import { FileIcon, Copy, Download, Star } from 'lucide-svelte';
  import { formatRelativeTime, toHumanReadableSize } from '$lib/utils';
  import { files, wallet, type ProgressiveSearchState, type ProtocolDetails, type SeederInfo } from '$lib/stores';
  import { t } from 'svelte-i18n';
  import { showToast } from '$lib/toast';
  import { costFromPricePerMb, minPricePerMb } from '$lib/utils/pricing';
  import { isLoading } from 'svelte-i18n';

  type TranslateParams = { values?: Record<string, unknown>; default?: string };
  const tr = (key: string, params?: TranslateParams): string =>
  $t(key, params);

  interface Props {
    searchState: ProgressiveSearchState,
    isSeeding: boolean,
    availableProtocols: ProtocolDetails[],
    download: ()=>void
  }
  
  let {
    searchState,
    isSeeding,
    availableProtocols,
    download
  }: Props = $props();

  let showSeederDetailsModal = $state(false);
  let selectedSeederDetails:SeederInfo|null = $state(null);
  let metadata = $derived(searchState.basicMetadata);
  let seeders = $derived(searchState.seeders);

  const minOfferPricePerMb = $derived.by(() => {
    const candidates = searchState.seeders.filter((s) => s.hasFileInfo && s.hasGeneralInfo);
    if (candidates.length === 0) return null;
    return candidates.reduce((s1, s2) =>
      (s1.pricePerMb ?? 0) > (s2.pricePerMb ?? 0) ? s1 : s2,
    ).pricePerMb ?? 0;
  });

  const minOfferTotal = $derived.by(() =>
    (searchState.basicMetadata && minOfferPricePerMb !== null)
      ? costFromPricePerMb({ bytes: searchState.basicMetadata?.fileSize, pricePerMb: minOfferPricePerMb })
      : null,
  );

  const isBusy = $derived(searchState.status==="searching");

  const loadingSeederCount = $derived.by(() => {
    const pending = searchState.providers.length - searchState.seeders.length;
    if (searchState.status === 'searching') return Math.max(1, pending);
    return Math.max(0, pending);
  });

  const canAfford = $derived.by(() => {
    if (isSeeding) return true;
    if (minOfferTotal === null) return true;
    return $wallet.balance >= minOfferTotal;
  });

  const isPriceLoading = $derived(minOfferTotal === null);

  function formatFileSize(bytes: number): string {
    return toHumanReadableSize(bytes);
  }

  // once search finishes, show real number of seederCount, providers are from DHT, seeders are providers who have responded
  let seederCount = $derived(
    (searchState.status=="complete" || searchState.status=="timeout") ? searchState.seeders.length : searchState.providers.length 
  );

  let createdLabel = $derived(searchState.basicMetadata?.createdAt
    ? formatRelativeTime(new Date(searchState.basicMetadata?.createdAt * 1000))
    : null);



  function copyFrom(data:string) {
    return () =>
    navigator.clipboard.writeText(data);
  }

  function handleDownload() {
    // Check if download should proceed
    if (isBusy) {
      return;
    }

    if (!canAfford && minOfferTotal !== null && minOfferTotal > 0 && !isSeeding) {
      return;
    }

    // Dispatch download event - parent component handles protocol/peer selection
    download();
  }


  function showSeederInfo(peerId: string) {
    // Find the seeder details by peer ID
    const details = seeders.find(s => s.peerId === peerId);
    if (details) {
      selectedSeederDetails = details;
      showSeederDetailsModal = true;
    }
  }

  function closeSeederDetailsModal() {
    showSeederDetailsModal = false;
    selectedSeederDetails = null;
  }

  let seederIds = $derived(seeders?.map((s, index) => ({
    id: `${s.peerId}-${index}`,
    address: s.peerId,
    details: s
  })) ?? []);

</script>

<Card class="p-5 space-y-5">
  {#if metadata}
    <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
      <div class="flex items-start gap-3">
        <div class="w-12 h-12 rounded-md bg-muted flex items-center justify-center">
          <FileIcon class="h-6 w-6 text-muted-foreground" />
        </div>
        <div class="flex-1">
          <h3 class="text-lg font-semibold break-all">{metadata.fileName}</h3>
          <div class="flex flex-wrap items-center gap-2 text-sm text-muted-foreground mt-1">
            {#if createdLabel}
              <span>Published {createdLabel}</span>
            {/if}
            {#if metadata.mimeType}
              {#if createdLabel}
                <span>•</span>
              {/if}
              <span>{metadata.mimeType}</span>
            {/if}
          </div>
        </div>
      </div>

      <div class="flex items-center gap-2 flex-wrap">
        {#each availableProtocols as protocol}
          <Badge class={protocol.colorClass}>
            {@const IconComponent = protocol.icon}
            <IconComponent class="h-3.5 w-3.5 mr-1" />
            {protocol.name}
          </Badge>
        {/each}
      </div>
    </div>

    <div class="grid gap-4 md:grid-cols-2">
      <div class="space-y-3">
        <div>
          <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">File hash</p>
          <div class="flex items-center gap-2 rounded-md border border-border/50 bg-muted/40 py-1 px-1.5 overflow-hidden">
            <code class="flex-1 text-xs font-mono break-all text-muted-foreground overflow-hidden" style="word-break: break-all;">{metadata.fileHash}</code>
            <Button
              variant="ghost"
              size="icon"
              class="h-7 w-7"
              on:click={copyFrom(metadata.fileHash)}
            >
              <Copy class="h-3.5 w-3.5" />
              <span class="sr-only">Copy hash</span>
            </Button>
          </div>
        </div>

        <div class="space-y-3">
          <p class="text-xs uppercase tracking-wide text-muted-foreground">Details</p>
          <ul class="space-y-2 text-sm text-foreground">
            <li class="flex items-center justify-between">
              <span class="text-muted-foreground">Seeder count</span>
              <span>{seederCount}</span>
            </li>
            <li class="flex items-center justify-between">
              <span class="text-muted-foreground">Size</span>
              <span>{formatFileSize(metadata.fileSize)}</span>
            </li>
            <li class="flex items-center justify-between">
              <span class="text-muted-foreground">Min file price</span>
              <span class="font-semibold text-emerald-600">
                {#if isSeeding}
                  Free
                {:else if isPriceLoading}
                  Loading...
                {:else if minOfferTotal !== null}
                  {minOfferTotal.toFixed(4)} Chiral
                {:else}
                  0.0001 Chiral
                {/if}
              </span>
            </li>
            <li class="text-xs text-muted-foreground text-center col-span-2">
              Min price calculated from currently known seeder offers
            </li>
          </ul>
        </div>
      </div>

      <div class="space-y-3">
        {#if searchState.providers.length > 0 || searchState.status === 'searching'}
          <p class="text-xs uppercase tracking-wide text-muted-foreground">Available peers</p>
          <div class="space-y-2 max-h-40 overflow-auto pr-1">
            {#each seederIds as seeder, index}
              <button
                type="button"
                class="w-full flex items-start gap-2 rounded-md border border-border/50 bg-muted/40 p-2 overflow-hidden hover:bg-muted/60 transition-colors cursor-pointer text-left"
                on:click={() => showSeederInfo(seeder.address)}
                title={seeder.details?.hasGeneralInfo ? 'Click to view seeder details' : 'Seeder info loading...'}
              >
                <div class="mt-0.5 h-2 w-2 rounded-full bg-emerald-500 flex-shrink-0"></div>
                <div class="space-y-1 flex-1 min-w-0">
                  <code class="text-xs font-mono break-words block">{seeder.address}</code>
                  <div class="flex items-center gap-2 text-xs text-muted-foreground">
                    <span>Seed #{index + 1}</span>
                    {#if seeder.details?.hasGeneralInfo}
                      <span class="text-emerald-600">• Info available</span>
                    {:else}
                      <span class="text-amber-600">• Loading...</span>
                    {/if}
                  </div>
                  {#if seeder.details?.walletAddress}
                    <div class="text-xs text-muted-foreground truncate">
                      {seeder.details.walletAddress.slice(0, 10)}...
                    </div>
                  {/if}
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  on:click={copyFrom(seeder.address)}
                >
                  <Copy class="h-3.5 w-3.5" />
                  <span class="sr-only">Copy seeder address</span>
                </Button>
              </button>
            {/each}
            
            {#if loadingSeederCount > 0}
              {#each Array(loadingSeederCount) as _}
                <div class="flex items-start gap-2 rounded-md border border-border/50 bg-muted/40 p-2 overflow-hidden animate-pulse">
                  <div class="mt-0.5 h-2 w-2 rounded-full bg-gray-300 flex-shrink-0"></div>
                  <div class="space-y-1 flex-1">
                    <div class="h-4 bg-gray-300 rounded w-3/4"></div>
                    <div class="h-3 bg-gray-200 rounded w-1/4"></div>
                  </div>
                </div>
              {/each}
            {/if}
          </div>
          {#if loadingSeederCount > 0}
            <p class="text-xs text-muted-foreground text-center">Loading seeder information...</p>
          {/if}
        {:else}
          <div class="space-y-2">
            <p class="text-xs uppercase tracking-wide text-muted-foreground">Available peers</p>
            <p class="text-xs text-muted-foreground italic">No seeders reported yet for this file.</p>
          </div>
        {/if}
      </div>
    </div>

    <div class="flex flex-col sm:flex-row gap-3 sm:items-center sm:justify-between">
      <div class="text-xs text-muted-foreground">
        {#if isSeeding}
          <span class="text-emerald-600 font-semibold">You are seeding this file</span>
        {:else if !canAfford && minOfferTotal !== null && minOfferTotal > 0}
          <span class="text-red-600 font-semibold">Insufficient balance to download this file</span>
        {:else if seeders.length > 0}
          {seeders.length > 1 ? '' : 'Single seeder available.'}
        {:else}
          Waiting for peers to announce this file.
        {/if}
      </div>
      <div class="flex items-center gap-2">
        <Button
          on:click={handleDownload}
          disabled={isBusy || (!canAfford && minOfferTotal !== null && minOfferTotal > 0 && !isSeeding)}
          class={!canAfford && minOfferTotal !== null && minOfferTotal > 0 && !isSeeding ? 'opacity-50 cursor-not-allowed' : ''}
        >
          <Download class="h-4 w-4 mr-2" />
          {#if !canAfford && minOfferTotal !== null && minOfferTotal > 0}
            Insufficient funds
          {:else}
            Download
          {/if}
        </Button>
      </div>
    </div>
  {:else}
    <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
      <div class="flex items-start gap-3">
        <div class="w-12 h-12 rounded-md bg-muted flex items-center justify-center animate-pulse">
          <FileIcon class="h-6 w-6 text-muted-foreground" />
        </div>
        <div class="flex-1 space-y-2">
          <div class="h-6 bg-muted rounded w-3/4 animate-pulse"></div>
          <div class="flex gap-2">
            <div class="h-4 bg-muted rounded w-1/4 animate-pulse"></div>
            <div class="h-4 bg-muted rounded w-1/4 animate-pulse"></div>
          </div>
        </div>
      </div>
    </div>

    <div class="grid gap-4 md:grid-cols-2">
      <div class="space-y-3">
        <div>
          <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">File hash</p>
          <div class="flex items-center gap-2 rounded-md border border-border/50 bg-muted/40 py-1 px-1.5 overflow-hidden">
            <div class="flex-1 h-4 bg-muted rounded animate-pulse"></div>
            <div class="h-7 w-7 bg-muted rounded animate-pulse"></div>
          </div>
        </div>

        <div>
          <p class="text-xs uppercase tracking-wide text-muted-foreground">Details</p>
          <ul class="space-y-2 text-sm text-foreground">
            <li class="flex items-center justify-between">
              <span class="text-muted-foreground">Seeder count</span>
              <div class="h-4 w-8 bg-muted rounded animate-pulse"></div>
            </li>
            <li class="flex items-center justify-between">
              <span class="text-muted-foreground">Size</span>
              <div class="h-4 w-20 bg-muted rounded animate-pulse"></div>
            </li>
            <li class="flex items-center justify-between">
              <span class="text-muted-foreground">Min file price</span>
              <div class="h-4 w-24 bg-muted rounded animate-pulse"></div>
            </li>
          </ul>
        </div>
      </div>

      <div class="space-y-3">
        <p class="text-xs uppercase tracking-wide text-muted-foreground">Available peers</p>
        <div class="space-y-2 max-h-40 overflow-auto pr-1">
          {#each Array(loadingSeederCount) as _}
            <div class="flex items-start gap-2 rounded-md border border-border/50 bg-muted/40 p-2 overflow-hidden animate-pulse">
              <div class="mt-0.5 h-2 w-2 rounded-full bg-gray-300 flex-shrink-0"></div>
              <div class="space-y-1 flex-1">
                <div class="h-4 bg-gray-300 rounded w-3/4"></div>
                <div class="h-3 bg-gray-200 rounded w-1/4"></div>
              </div>
            </div>
          {/each}
        </div>
        <p class="text-xs text-muted-foreground text-center">Loading seeder information...</p>
      </div>
    </div>
  {/if}

  <!-- for debugging -->
  {#if showSeederDetailsModal && selectedSeederDetails}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
    <div class="bg-background rounded-lg shadow-lg p-6 w-full max-w-md border border-border">
      <h2 class="text-xl font-bold mb-4">Seeder Details</h2>

      <div class="space-y-4">
        <!-- Peer ID -->
        <div>
          <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">Peer ID</p>
          <div class="flex items-center gap-2 rounded-md border border-border/50 bg-muted/40 p-2">
            <code class="flex-1 text-xs font-mono break-all">{selectedSeederDetails?.peerId}</code>
            <Button
              variant="ghost"
              size="icon"
              class="h-7 w-7"
              on:click={() => {
                navigator.clipboard.writeText(selectedSeederDetails?.peerId || '');
                showToast('Peer ID copied', 'success');
              }}
            >
              <Copy class="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>

        {#if selectedSeederDetails?.hasGeneralInfo}
          <!-- Wallet Address -->
          {#if selectedSeederDetails?.walletAddress}
            <div>
              <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">Wallet Address</p>
              <div class="flex items-center gap-2 rounded-md border border-border/50 bg-muted/40 p-2">
                <code class="flex-1 text-xs font-mono break-all">{selectedSeederDetails?.walletAddress}</code>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  on:click={() => {
                    navigator.clipboard.writeText(selectedSeederDetails?.walletAddress || '');
                    showToast('Wallet address copied', 'success');
                  }}
                >
                  <Copy class="h-3.5 w-3.5" />
                </Button>
              </div>
            </div>
          {/if}

          <!-- Price -->
          {#if selectedSeederDetails?.pricePerMb !== undefined}
            <div>
              <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">Price per MB</p>
              <div class="rounded-md border border-border/50 bg-muted/40 p-3">
                <p class="text-lg font-bold text-emerald-600">
                  {selectedSeederDetails?.pricePerMb?.toFixed(6)} Chiral
                </p>
                <p class="text-xs text-muted-foreground mt-1">
                  Total: {((selectedSeederDetails?.pricePerMb || 0) * ((metadata?.fileSize ?? 0) / (1024 * 1024))).toFixed(4)} Chiral
                </p>
              </div>
            </div>
          {/if}

          <!-- Supported Protocols -->
          {#if selectedSeederDetails?.protocols && selectedSeederDetails.protocols.length > 0}
            <div>
              <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">Supported Protocols</p>
              <div class="flex flex-wrap gap-2">
                {#each selectedSeederDetails?.protocols || [] as protocol}
                  <Badge class="bg-blue-100 text-blue-800">
                    {protocol}
                  </Badge>
                {/each}
              </div>
            </div>
          {/if}

          <!-- Protocol Details -->
          {#if selectedSeederDetails?.hasFileInfo && selectedSeederDetails?.protocolDetails}
            <div>
              <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">Protocol Details</p>
              <div class="rounded-md border border-border/50 bg-muted/40 p-3 max-h-40 overflow-auto">
                <pre class="text-xs font-mono whitespace-pre-wrap break-all">{JSON.stringify(selectedSeederDetails?.protocolDetails, null, 2)}</pre>
              </div>
            </div>
          {/if}
        {:else}
          <div class="p-4 bg-amber-500/10 rounded-lg border border-amber-500/30">
            <p class="text-sm text-amber-600 text-center">
              Seeder information is still loading...
            </p>
          </div>
        {/if}
      </div>

      <div class="flex justify-end gap-2 mt-6">
        <Button variant="outline" on:click={closeSeederDetailsModal}>
          Close
        </Button>
      </div>
    </div>
  </div>
{/if}
</Card>
