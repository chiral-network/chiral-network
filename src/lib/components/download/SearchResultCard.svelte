<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import { FileIcon, Copy, Download, Server, Globe, Blocks } from 'lucide-svelte';
  import { createEventDispatcher, onMount } from 'svelte';
  import { dhtService, type FileMetadata } from '$lib/dht';
  import { formatRelativeTime, toHumanReadableSize } from '$lib/utils';
  import { files, wallet } from '$lib/stores';
  import { get } from 'svelte/store';
  import { t } from 'svelte-i18n';
  import { showToast } from '$lib/toast';
  import { paymentService } from '$lib/services/paymentService';

  type TranslateParams = { values?: Record<string, unknown>; default?: string };
  const tr = (key: string, params?: TranslateParams): string =>
  $t(key, params);

  const dispatch = createEventDispatcher<{ download: FileMetadata; copy: string }>();

  export let metadata: FileMetadata;
  export let isBusy = false;

  let canAfford = true;
  let checkingBalance = true; // Start as true since we check on mount
  let currentPrice: number | null = null;
  let showDecryptDialog = false;
  let showDownloadConfirmDialog = false;
  let showPaymentConfirmDialog = false;
  let showSeedersSelection = false;
  let selectedSeederIndex: number | null = 0;

  // Use reactive wallet balance from store
  $: userBalance = $wallet.balance;

  function formatFileSize(bytes: number): string {
    return toHumanReadableSize(bytes);
  }

  $: seederCount = metadata?.seeders?.length ?? 0;
  $: createdLabel = metadata?.createdAt
    ? formatRelativeTime(new Date(metadata.createdAt * 1000))
    : null;


  // Helper function to determine available protocols for a file
  // Files can be downloaded via multiple protocols if they were uploaded with multiple protocols
  $: availableProtocols = (() => {
    const protocols = [];
    
    // Determine what metadata exists
    const hasCids = !!(metadata.cids && metadata.cids.length > 0);
    const hasInfoHash = !!metadata.infoHash;
    const hasHttpSources = !!(metadata.httpSources && metadata.httpSources.length > 0);
    const hasFtpSources = !!(metadata.ftpSources && metadata.ftpSources.length > 0);
    const hasEd2kSources = !!(metadata.ed2kSources && metadata.ed2kSources.length > 0);
    const hasSeeders = !!(metadata.seeders && metadata.seeders.length > 0);
    
    // WebRTC is only available if file was uploaded via WebRTC (has seeders but NO CIDs or other protocol indicators)
    // Files uploaded via Bitswap have CIDs and must be downloaded via Bitswap, not WebRTC
    const isWebRTCUpload = hasSeeders && !hasCids && !hasInfoHash && !hasHttpSources && !hasFtpSources && !hasEd2kSources;

    // Bitswap is available if there are CIDs (content identifiers for IPFS blocks) AND seeders
    const isBitswapAvailable = hasCids && hasSeeders;

    // Check for Bitswap (has CIDs and seeders)
    if (isBitswapAvailable) {
      protocols.push({
        id: 'bitswap',
        name: 'Bitswap',
        icon: Blocks,
        colorClass: 'bg-purple-100 text-purple-800'
      });
    }

    // Check for WebRTC (uploaded via WebRTC - has seeders but no other protocol indicators)
    if (isWebRTCUpload) {
      protocols.push({
        id: 'webrtc',
        name: 'WebRTC',
        icon: Globe,
        colorClass: 'bg-blue-100 text-blue-800'
      });
    }

    // Check for BitTorrent (has info_hash)
    if (hasInfoHash) {
      protocols.push({
        id: 'bittorrent',
        name: 'BitTorrent',
        icon: Server,
        colorClass: 'bg-green-100 text-green-800'
      });
    }

    // Check for HTTP (has HTTP sources)
    if (hasHttpSources) {
      protocols.push({
        id: 'http',
        name: 'HTTP',
        icon: Globe,
        colorClass: 'bg-gray-100 text-gray-800'
      });
    }

    // Check for FTP (has FTP sources)
    if (hasFtpSources) {
      protocols.push({
        id: 'ftp',
        name: 'FTP',
        icon: Server,
        colorClass: 'bg-gray-100 text-gray-800'
      });
    }

    // Check for ED2K (has ED2K sources)
    if (hasEd2kSources) {
      protocols.push({
        id: 'ed2k',
        name: 'ED2K',
        icon: Server,
        colorClass: 'bg-orange-100 text-orange-800'
      });
    }

    return protocols;
  })();

  // Check if user is already seeding this file
  $: isSeeding = !!get(files).find(f => f.hash === metadata.fileHash && f.status === 'seeding');

  function copyHash() {
    navigator.clipboard.writeText(metadata.fileHash).then(() => {
      dispatch('copy', metadata.fileHash);
    });
  }

  function copySeeder(address: string, _index: number) {
    navigator.clipboard.writeText(address).then(() => {
      dispatch('copy', address);
    });
  }

  function copyMagnetLink(link: string) {
    navigator.clipboard.writeText(link).then(() => {
      dispatch('copy', link);
    });
  }

  function copyEd2kLink(link: string) {
    navigator.clipboard.writeText(link).then(() => {
      dispatch('copy', link);
    });
  }

  function copyFtpLink(link: string) {
    navigator.clipboard.writeText(link).then(() => {
      dispatch('copy', link);
    });
  }

  function copyHttpLink(link: string) {
    navigator.clipboard.writeText(link).then(() => {
      dispatch('copy', link);
    });
  }

  async function handleDownload() {
    // Skipping payment confirmation for now
    // Always show initial download confirmation dialog first
    // showDownloadConfirmDialog = true;

    const freshSeeders = await dhtService.getSeedersForFile(metadata.fileHash);

    // Also check stores for WebRTC seeder addresses
    const existingFile = get(files).find(f => f.hash === metadata.fileHash);
    const webrtcSeeders = existingFile?.seederAddresses ?? [];

    // Combine DHT seeders with WebRTC seeders
    const allSeeders = [...new Set([...freshSeeders, ...webrtcSeeders])];
    metadata.seeders = allSeeders;

    // Bitswap note: manual seeder selection was for demo purposes to show
    // peer-selection capability; now switching to intelligent peer selection.
    // showSeedersSelection = true

    proceedWithDownload();

  }

  async function confirmSeeder() {
    showSeedersSelection = false;
    console.log("SELECTED SEEDER: ", selectedSeederIndex);

    showDownloadConfirmDialog = true;
  }

  async function confirmDownload() {
    showDownloadConfirmDialog = false;

    // Skip payment for files the user is seeding (they already paid hosting costs)
    if (isSeeding) {
      showDecryptDialog = true;
      return;
    }

    // All downloads require payment (minimum 0.0001 Chiral)
    // Always show payment confirmation
    showPaymentConfirmDialog = true;
  }

  function cancelDownload() {
    showDownloadConfirmDialog = false;
  }

  async function proceedWithDownload() {
    // Just dispatch the download event - let Download.svelte handle starting the actual download
    // This ensures the file is added to the store before chunks start arriving
    const copy = structuredClone(metadata);
    copy.seeders = [copy.seeders[selectedSeederIndex?selectedSeederIndex:0]];
    dispatch("download", metadata);
  }

  async function confirmPayment() {
    showPaymentConfirmDialog = false;

    if (!paymentService.isValidWalletAddress(metadata.uploaderAddress)) {
      // showToast('Cannot process payment: uploader wallet address is missing or invalid', 'error');
      showToast(tr('toasts.download.payment.invalidAddress'), 'error');
      return;
    }

    try {
      const seederPeerId = metadata.seeders?.[0];
      const paymentResult = await paymentService.processDownloadPayment(
        metadata.fileHash,
        metadata.fileName,
        metadata.fileSize,
        metadata.uploaderAddress || '',
        seederPeerId
      );

      if (!paymentResult.success) {
        const errorMessage = paymentResult.error || 'Unknown error';
        // showToast(`Payment failed: ${errorMessage}`, 'error');
        showToast(tr('toasts.download.payment.failed', { values: { error: errorMessage } }), 'error');
        return;
      }

      if (paymentResult.transactionHash) {
        showToast(
          // `Payment successful! Transaction: ${paymentResult.transactionHash.substring(0, 10)}...`,
          tr('toasts.download.payment.successWithHash', {
            values: { hash: paymentResult.transactionHash.substring(0, 10) }
          }),
          'success'
        );
      } else {
        // showToast('Payment successful!', 'success');
        showToast(tr('toasts.download.payment.success'), 'success');
      }

      // Refresh balance after payment to reflect the deduction
      await checkBalance();

      // Proceed with download after successful payment
      await proceedWithDownload();
    } catch (error: any) {
      console.error('Payment processing failed:', error);
      const message = error?.message || error?.toString() || 'Unknown error';
      // showToast(`Payment failed: ${message}`, 'error');
      showToast(tr('toasts.download.payment.failed', { values: { error: message } }), 'error');
    }
  }

  function cancelPayment() {
    showPaymentConfirmDialog = false;
  }

  async function confirmDecryptAndQueue() {
    showDecryptDialog = false;
    // Dispatch for both protocols - let Download.svelte handle the actual download
    dispatch('download', metadata);
    console.log("🔍 DEBUG: Dispatched decrypt and download event for file:", metadata.fileName);
  }

  function cancelDecryptDialog() {
    showDecryptDialog = false;
  }

  const seederIds = metadata.seeders?.map((address, index) => ({
    id: `${metadata.fileHash}-${index}`,
    address,
  })) ?? [];

  // Check if user can afford the download when price is set
  async function checkBalance() {
    if (metadata.fileSize && metadata.fileSize > 0) {
      checkingBalance = true;
      try {
        // Calculate current dynamic price instead of using static metadata.price
        const currentDynamicPrice = await paymentService.calculateDownloadCost(metadata.fileSize);
        const currentBalance = get(wallet).balance;
        canAfford = currentBalance >= currentDynamicPrice;

        // Store the dynamic price for display
        currentPrice = currentDynamicPrice;

      } catch (error) {
        console.error('Failed to check balance:', error);
        canAfford = false;
      } finally {
        checkingBalance = false;
      }
    }
  }

  // Trigger balance check when metadata or wallet balance changes
  $: if (metadata.fileSize && metadata.fileSize > 0) {
    checkBalance();
  }

  // Reactive check for affordability when balance changes and we have a current price
  $: if (currentPrice !== null && currentPrice > 0) {
    canAfford = $wallet.balance >= currentPrice;
  }

  // Check balance when component mounts
  onMount(() => {
    checkBalance();
  });
</script>

<Card class="p-5 space-y-5">
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
          <svelte:component this={protocol.icon} class="h-3.5 w-3.5 mr-1" />
          {protocol.name}
        </Badge>
      {/each}
    </div>
  </div>

  <div class="grid gap-4 md:grid-cols-2">
    <!-- Left Column: All technical identifiers and details -->
    <div class="space-y-3">
      <div>
        <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">Merkle hash</p>
        <div class="flex items-center gap-2 rounded-md border border-border/50 bg-muted/40 py-1 px-1.5 overflow-hidden">
          <code class="flex-1 text-xs font-mono break-all text-muted-foreground overflow-hidden" style="word-break: break-all;">{metadata.fileHash}</code>
          <Button
            variant="ghost"
            size="icon"
            class="h-7 w-7"
            on:click={copyHash}
          >
            <Copy class="h-3.5 w-3.5" />
            <span class="sr-only">Copy hash</span>
          </Button>
        </div>
      </div>

      {#if metadata.infoHash}
        {@const magnetLink = `magnet:?xt=urn:btih:${metadata.infoHash}${metadata.trackers && metadata.trackers.length > 0 ? '&tr=' + metadata.trackers.join('&tr=') : ''}`}
        <div>
          <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">Magnet Link</p>
          <div class="flex items-center gap-2 rounded-md border border-border/50 bg-muted/40 p-1.5 overflow-hidden">
            <code class="flex-1 text-xs font-mono break-all text-muted-foreground overflow-hidden" style="word-break: break-all;">{magnetLink}</code>
            <Button
              variant="ghost"
              size="icon"
              class="h-7 w-7"
              on:click={() => copyMagnetLink(magnetLink)}
            >
              <Copy class="h-3.5 w-3.5" />
              <span class="sr-only">Copy magnet link</span>
            </Button>
          </div>
        </div>
      {/if}

      {#if metadata.ed2kSources && metadata.ed2kSources.length > 0}
        {@const ed2kSource = metadata.ed2kSources[0]}
        {@const ed2kLink = `ed2k://|file|${metadata.fileName}|${metadata.fileSize}|${ed2kSource.file_hash}|/`}
        <div>
          <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">ED2K Link</p>
          <div class="flex items-center gap-2 rounded-md border border-border/50 bg-muted/40 p-1.5 overflow-hidden">
            <code class="flex-1 text-xs font-mono break-all text-muted-foreground overflow-hidden" style="word-break: break-all;">{ed2kLink}</code>
            <Button
              variant="ghost"
              size="icon"
              class="h-7 w-7"
              on:click={() => copyEd2kLink(ed2kLink)}
            >
              <Copy class="h-3.5 w-3.5" />
              <span class="sr-only">Copy ED2K link</span>
            </Button>
          </div>
        </div>
      {/if}

      {#if metadata.ftpSources && metadata.ftpSources.length > 0}
        {@const ftpSource = metadata.ftpSources[0]}
        <div>
          <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">FTP Link</p>
          <div class="flex items-center gap-2 rounded-md border border-border/50 bg-muted/40 p-1.5 overflow-hidden">
            <code class="flex-1 text-xs font-mono break-all text-muted-foreground overflow-hidden" style="word-break: break-all;">{ftpSource.url}</code>
            <Button
              variant="ghost"
              size="icon"
              class="h-7 w-7"
              on:click={() => copyFtpLink(ftpSource.url)}
            >
              <Copy class="h-3.5 w-3.5" />
              <span class="sr-only">Copy FTP link</span>
            </Button>
          </div>
        </div>
      {/if}

      {#if metadata.httpSources && metadata.httpSources.length > 0}
        {@const httpSource = metadata.httpSources[0]}
        <div>
          <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">HTTP Link</p>
          <div class="flex items-center gap-2 rounded-md border border-border/50 bg-muted/40 p-1.5 overflow-hidden">
            <code class="flex-1 text-xs font-mono break-all text-muted-foreground overflow-hidden" style="word-break: break-all;">{httpSource.url}</code>
            <Button
              variant="ghost"
              size="icon"
              class="h-7 w-7"
              on:click={() => copyHttpLink(httpSource.url)}
            >
              <Copy class="h-3.5 w-3.5" />
              <span class="sr-only">Copy HTTP link</span>
            </Button>
          </div>
        </div>
      {/if}

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
            <span class="text-muted-foreground">Price</span>
            <span class="font-semibold text-emerald-600">
              {#if isSeeding}
                Free
              {:else if checkingBalance}
                Calculating...
              {:else if currentPrice !== null}
                {currentPrice.toFixed(4)} Chiral
              {:else}
                0.0001 Chiral
              {/if}
            </span>
          </li>
          <li class="text-xs text-muted-foreground text-center col-span-2">
            Price calculated based on current network conditions
          </li>
        </ul>
      </div>
    </div>

    <!-- Right Column: Available peers -->
    <div class="space-y-3">
      {#if metadata.seeders?.length}
        <div class="space-y-2">
          <p class="text-xs uppercase tracking-wide text-muted-foreground">Available peers</p>
          <div class="space-y-2 max-h-40 overflow-auto pr-1">
            {#each seederIds as seeder, index}
              <div class="flex items-start gap-2 rounded-md border border-border/50 bg-muted/40 p-2 overflow-hidden">
                <div class="mt-0.5 h-2 w-2 rounded-full bg-emerald-500 flex-shrink-0"></div>
                <div class="space-y-1 flex-1">
                  <code class="text-xs font-mono break-words block">{seeder.address}</code>
                  <div class="flex items-center gap-1 text-xs text-muted-foreground">
                    <span>Seed #{index + 1}</span>
                  </div>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  on:click={() => copySeeder(seeder.address, index)}
                >
                  <Copy class="h-3.5 w-3.5" />
                  <span class="sr-only">Copy seeder address</span>
                </Button>
              </div>
            {/each}
          </div>
        </div>
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
        {#if metadata.isEncrypted}
          <span class="ml-2 text-xs text-amber-600">(encrypted)</span>
        {/if}
      {:else if !canAfford && currentPrice && currentPrice > 0}
        <span class="text-red-600 font-semibold">Insufficient balance to download this file</span>
      {:else}
        Waiting for peers to announce this file.
      {/if}
    </div>
    <div class="flex items-center gap-2">
      <Button
        on:click={handleDownload}
        disabled={isBusy || checkingBalance || (!canAfford && currentPrice && currentPrice > 0 && !isSeeding)}
        class={!canAfford && currentPrice && currentPrice > 0 && !isSeeding ? 'opacity-50 cursor-not-allowed' : ''}
      >
        <Download class="h-4 w-4 mr-2" />
        {#if checkingBalance}
          Checking balance...
        {:else if !canAfford && currentPrice && currentPrice > 0 && !isSeeding}
          Insufficient funds
        {:else}
          Download
        {/if}
      </Button>
    </div>
  </div>

  {#if showDownloadConfirmDialog}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
    <div class="bg-background rounded-lg shadow-lg p-6 w-full max-w-md border border-border">
      <h2 class="text-xl font-bold mb-4 text-center">
        {isSeeding ? 'Download Local Copy' : 'Confirm Download'}
      </h2>

      <div class="space-y-4 mb-6">
        <div class="p-4 bg-muted/50 rounded-lg border border-border">
          <div class="space-y-2">
            <div>
              <p class="text-xs text-muted-foreground mb-1">File Name</p>
              <p class="text-sm font-semibold break-all">{metadata.fileName}</p>
            </div>
            <div class="flex justify-between items-center pt-2 border-t border-border/50">
              <span class="text-xs text-muted-foreground">Size</span>
              <span class="text-sm font-medium">{formatFileSize(metadata.fileSize)}</span>
            </div>
            {#if isSeeding}
              <div class="flex justify-between items-center pt-2 border-t border-border/50">
                <span class="text-xs text-muted-foreground">Status</span>
                <span class="text-sm font-medium text-emerald-600">Already Seeding</span>
              </div>
              {#if metadata.isEncrypted}
                <div class="flex justify-between items-center pt-2 border-t border-border/50">
                  <span class="text-xs text-muted-foreground">Encryption</span>
                  <span class="text-sm font-medium text-amber-600">Encrypted</span>
                </div>
              {/if}
            {/if}
          </div>
        </div>

        <div class="p-4 bg-blue-500/10 rounded-lg border-2 border-blue-500/30">
          <div class="text-center">
            <p class="text-sm text-muted-foreground mb-1">Price</p>
            <p class="text-2xl font-bold text-blue-600">
              {#if checkingBalance}
                Calculating...
              {:else}
                {(currentPrice ?? 0.0001).toFixed(4)} Chiral
              {/if}
            </p>
          </div>
        </div>
        {#if isSeeding}
          <div class="p-3 bg-amber-500/10 rounded-lg border border-amber-500/30">
            <p class="text-xs text-amber-600 text-center">
              You're already seeding this file. Downloading will create a decrypted local copy.
            </p>
          </div>
        {/if}
      </div>

      <p class="text-sm text-muted-foreground text-center mb-6">
        {isSeeding
          ? `Download a local copy for free (you're already seeding this file)`
          : `You will be charged ${(currentPrice ?? 0.0001).toFixed(4)} Chiral. Continue?`}
      </p>

      <div class="flex gap-3">
        <Button variant="outline" on:click={cancelDownload} class="flex-1">
          Cancel
        </Button>
        <Button on:click={confirmDownload} class="flex-1 bg-blue-600 hover:bg-blue-700">
          Confirm
        </Button>
      </div>
    </div>
  </div>
{/if}

{#if showDecryptDialog}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
    <div class="bg-background rounded-lg shadow-lg p-6 w-full max-w-md border border-border">
      <h2 class="text-lg font-semibold mb-2">Already Seeding</h2>
      <p class="mb-4 text-sm text-muted-foreground">
        You're already seeding this file{metadata.isEncrypted ? ' (encrypted)' : ''}.<br />
        Would you like to decrypt and save a local readable copy?
      </p>
      <div class="flex justify-end gap-2 mt-4">
        <Button variant="outline" on:click={cancelDecryptDialog}>Cancel</Button>
        <Button on:click={confirmDecryptAndQueue}>Download</Button>
      </div>
    </div>
  </div>
{/if}

{#if showPaymentConfirmDialog}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
    <div class="bg-background rounded-lg shadow-lg p-6 w-full max-w-md border border-border">
      <h2 class="text-xl font-bold mb-4 text-center">Confirm Payment</h2>

      <div class="space-y-4 mb-6">
        <div class="flex justify-between items-center p-3 bg-muted/50 rounded-lg">
          <span class="text-sm text-muted-foreground">Your Balance</span>
          <span class="text-lg font-bold">{userBalance.toFixed(4)} Chiral</span>
        </div>

        <div class="flex justify-between items-center p-3 bg-blue-500/10 rounded-lg border border-blue-500/30">
          <span class="text-sm text-muted-foreground">File Price</span>
          <span class="text-lg font-bold text-blue-600">{(currentPrice || 0).toFixed(4)} Chiral</span>
        </div>

        <div class="flex justify-between items-center p-3 bg-muted/50 rounded-lg border-2 border-border">
          <span class="text-sm font-semibold">Balance After Purchase</span>
          <span class="text-lg font-bold {canAfford ? 'text-emerald-600' : 'text-red-600'}">
            {(userBalance - (currentPrice || 0)).toFixed(4)} Chiral
          </span>
        </div>
      </div>

      {#if !canAfford}
        <div class="mb-4 p-3 bg-red-500/10 border border-red-500/30 rounded-lg">
          <p class="text-sm text-red-600 font-semibold text-center">
            Insufficient balance! You need {(currentPrice || 0) - userBalance} more Chiral.
          </p>
        </div>
      {/if}

      <p class="text-sm text-muted-foreground text-center mb-6">
        {canAfford
          ? 'Proceed with payment to download this file?'
          : 'You do not have enough Chiral to download this file.'}
      </p>

      <div class="flex gap-3">
        <Button variant="outline" on:click={cancelPayment} class="flex-1">
          Cancel
        </Button>
        <Button
          on:click={confirmPayment}
          disabled={!canAfford}
          class="flex-1 {!canAfford ? 'opacity-50 cursor-not-allowed' : 'bg-blue-600 hover:bg-blue-700'}"
        >
          {canAfford ? 'Confirm Payment' : 'Insufficient Funds'}
        </Button>
      </div>
    </div>
  </div>
{/if}

{#if showSeedersSelection}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
    <div class="bg-background rounded-lg shadow-lg p-6 w-full max-w-md border border-border">
      <h2 class="text-xl font-bold mb-4 text-center">Select a Seeder</h2>

      {#if metadata.seeders && metadata.seeders.length > 0}
        <p class="text-sm text-muted-foreground text-center mb-4">
          Found {metadata.seeders.length} available peer{metadata.seeders.length === 1 ? '' : 's'}.
        </p>
        <div class="space-y-2 max-h-60 overflow-auto pr-1 mb-6">
          {#each metadata.seeders as seeder, index}
            <label
              class="flex items-center gap-3 p-3 rounded-lg border cursor-pointer transition-colors {selectedSeederIndex !== null && +selectedSeederIndex === index ? 'bg-blue-500/10 border-blue-500/50' : 'border-border hover:bg-muted/50'}"
            >
              <input
                type="radio"
                name="seeder-selection"
                value={index}
                bind:group={selectedSeederIndex}
                class="h-4 w-4 mt-1 text-blue-600 focus:ring-blue-500 border-gray-300"
              />
              <div class="flex-1">
                <code class="text-xs font-mono break-all">{seeder}</code>
              </div>
            </label>
          {/each}
        </div>
      {:else}
        <div class="p-4 bg-red-500/10 rounded-lg border border-red-500/30 mb-6">
          <p class="text-sm text-red-600 text-center">
            No online seeders found for this file at the moment. Please try again later.
          </p>
        </div>
      {/if}

      <div class="flex gap-3">
        <Button variant="outline" on:click={() => { showSeedersSelection = false; selectedSeederIndex = null; }} class="flex-1">
          Cancel
        </Button>
        <Button on:click={confirmSeeder} disabled={selectedSeederIndex === null || metadata.seeders?.length === 0} class="flex-1 bg-blue-600 hover:bg-blue-700">
          Confirm
        </Button>
      </div>
    </div>
  </div>
{/if}
</Card>
