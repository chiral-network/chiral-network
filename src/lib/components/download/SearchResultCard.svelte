<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import { FileIcon, Copy, Download, Server, DollarSign, Globe, Blocks } from 'lucide-svelte';
  import { createEventDispatcher, onMount } from 'svelte';
  import { dhtService, type FileMetadata } from '$lib/dht';
  import { formatRelativeTime, toHumanReadableSize } from '$lib/utils';
  import { files, wallet } from '$lib/stores';
  import { get } from 'svelte/store';
  import { showToast } from '$lib/toast';
  import { paymentService } from '$lib/services/paymentService';

  const dispatch = createEventDispatcher<{ download: FileMetadata; copy: string }>();

  export let metadata: FileMetadata;
  export let isBusy = false;
  export let isBitswap: boolean = false;

  let canAfford = true;
  let checkingBalance = false;
  let hashCopied = false;
  let seederCopiedIndex: number | null = null;
  let showSeedingNotice = false;
  let showDecryptDialog = false;
  let showDownloadConfirmDialog = false;
  let showPaymentConfirmDialog = false;
  let showSeedersSelection = false;
  let selectedSeederIndex: number | null = null;

  // Use reactive wallet balance from store
  $: userBalance = $wallet.balance;

  function formatFileSize(bytes: number): string {
    return toHumanReadableSize(bytes);
  }

  $: seederCount = metadata?.seeders?.length ?? 0;
  $: createdLabel = metadata?.createdAt
    ? formatRelativeTime(new Date(metadata.createdAt * 1000))
    : null;

  // Check if user is already seeding this file
  $: isSeeding = !!get(files).find(f => f.hash === metadata.fileHash && f.status === 'seeding');

  function copyHash() {
    navigator.clipboard.writeText(metadata.fileHash).then(() => {
      hashCopied = true;
      dispatch('copy', metadata.fileHash);
      setTimeout(() => (hashCopied = false), 1500);
    });
  }

  function copySeeder(address: string, index: number) {
    navigator.clipboard.writeText(address).then(() => {
      seederCopiedIndex = index;
      dispatch('copy', address);
      setTimeout(() => {
        if (seederCopiedIndex === index) {
          seederCopiedIndex = null;
        }
      }, 1500);
    });
  }

  async function handleDownload() {
    // Always show initial download confirmation dialog first
    // showDownloadConfirmDialog = true;
    const freshSeeders = await dhtService.getSeedersForFile(metadata.fileHash);
    metadata.seeders = freshSeeders; 
    console.log("🔍 DEBUG: Seeders fetched:", freshSeeders);
    showSeedersSelection = true

  }

  async function confirmSeeder() {
    showSeedersSelection = false;
    console.log("SELECTED SEEDER: ", selectedSeederIndex);

    showDownloadConfirmDialog = true;
  }

  async function confirmDownload() {
    showDownloadConfirmDialog = false;
    
    // If already seeding and paid, show payment confirmation
    if (isSeeding && metadata.price && metadata.price > 0) {
      showPaymentConfirmDialog = true;
    }
    // If already seeding and free, proceed directly with download/decrypt
    else if (isSeeding) {
      confirmDecryptAndQueue();
    }
    // Show payment confirmation if file has a price (not seeding case)
    else if (metadata.price && metadata.price > 0) {
      showPaymentConfirmDialog = true;
    } else {
      // Free file - download directly
      proceedWithDownload();
    }
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
    console.log("🔍 DEBUG: Dispatched download event for file:", metadata.fileName);
  }

  async function confirmPayment() {
    showPaymentConfirmDialog = false;

    if (!paymentService.isValidWalletAddress(metadata.uploaderAddress)) {
      showToast('Cannot process payment: uploader wallet address is missing or invalid', 'error');
      return;
    }

    try {
      const seederPeerId = metadata.seeders?.[0];
      const paymentResult = await paymentService.processDownloadPayment(
        metadata.fileHash,
        metadata.fileName,
        metadata.fileSize,
        metadata.uploaderAddress,
        seederPeerId
      );

      if (!paymentResult.success) {
        const errorMessage = paymentResult.error || 'Unknown error';
        showToast(`Payment failed: ${errorMessage}`, 'error');
        return;
      }

      if (paymentResult.transactionHash) {
        showToast(
          `Payment successful! Transaction: ${paymentResult.transactionHash.substring(0, 10)}...`,
          'success'
        );
      } else {
        showToast('Payment successful!', 'success');
      }

      // Refresh balance after payment to reflect the deduction
      await checkBalance();

      // Proceed with download after successful payment
      await proceedWithDownload();
    } catch (error: any) {
      console.error('Payment processing failed:', error);
      const message = error?.message || error?.toString() || 'Unknown error';
      showToast(`Payment failed: ${message}`, 'error');
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
    if (metadata.price && metadata.price > 0) {
      checkingBalance = true;
      try {
        // Use wallet store balance instead of invoking backend
        const currentBalance = get(wallet).balance;
        canAfford = currentBalance >= metadata.price;
        console.log('💰 Balance check:', { currentBalance, price: metadata.price, canAfford });
      } catch (error) {
        console.error('Failed to check balance:', error);
        canAfford = false;
      } finally {
        checkingBalance = false;
      }
    }
  }

  // Reactive check for affordability when balance or price changes
  $: if (metadata.price && metadata.price > 0) {
    canAfford = $wallet.balance >= metadata.price;
  }

  // Check balance when component mounts
  onMount(() => {
    console.log("💰 SearchResultCard metadata:", metadata);
    console.log("💰 Price from metadata:", metadata.price);
    checkBalance();
  });
</script>

<Card class="p-5 space-y-5">
  <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
    <div class="flex items-start gap-3">
      <div class="mt-1 h-9 w-9 rounded-md bg-muted flex items-center justify-center">
        <FileIcon class="h-5 w-5 text-muted-foreground" />
      </div>
      <div class="space-y-1">
        <h3 class="text-lg font-semibold break-all">{metadata.fileName}</h3>
        <div class="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
          <span>{formatFileSize(metadata.fileSize)}</span>
          {#if createdLabel}
            <span>•</span>
            <span>Published {createdLabel}</span>
          {/if}
          {#if metadata.mimeType}
            <span>•</span>
            <span>{metadata.mimeType}</span>
          {/if}
        </div>
      </div>
    </div>

    <div class="flex items-center gap-2 flex-wrap">
      {#if metadata.cids && metadata.cids.length > 0}
        <Badge class="bg-purple-500/10 text-purple-600 dark:text-purple-300 border border-purple-500/30">
          <Blocks class="h-3.5 w-3.5 mr-1" />
          Bitswap
        </Badge>
      {:else}
        <Badge class="bg-blue-500/10 text-blue-600 dark:text-blue-300 border border-blue-500/30">
          <Globe class="h-3.5 w-3.5 mr-1" />
          WebRTC
        </Badge>
      {/if}
      <Badge class="bg-emerald-500/10 text-emerald-600 dark:text-emerald-300 border border-emerald-500/30">
        <Server class="h-3.5 w-3.5 mr-1" />
        {seederCount} {seederCount === 1 ? 'Seeder' : 'Seeders'}
      </Badge>
      {#if metadata.price && metadata.price > 0}
        <Badge class="bg-blue-500/10 text-blue-600 dark:text-blue-300 border border-blue-500/30">
          <DollarSign class="h-3.5 w-3.5 mr-1" />
          {metadata.price} Chiral
        </Badge>
      {:else}
        <Badge class="bg-gray-500/10 text-gray-600 dark:text-gray-300 border border-gray-500/30">
          Free
        </Badge>
      {/if}
    </div>
  </div>

  <div class="grid gap-4 md:grid-cols-2">
    <div class="space-y-3">
      <div>
        <p class="text-xs uppercase tracking-wide text-muted-foreground mb-1">File hash</p>
        <div class="flex items-center gap-2">
          <code class="text-sm font-mono break-all flex-1">{metadata.fileHash}</code>
          <Button variant="outline" size="icon" on:click={copyHash} class="h-8 w-8">
            <Copy class="h-4 w-4" />
            <span class="sr-only">Copy hash</span>
          </Button>
        </div>
        {#if hashCopied}
          <p class="mt-1 text-xs text-emerald-600">Copied!</p>
        {/if}
      </div>

      {#if metadata.seeders?.length}
        <div class="space-y-2">
          <p class="text-xs uppercase tracking-wide text-muted-foreground">Available peers</p>
          <div class="space-y-2 max-h-40 overflow-auto pr-1">
            {#each seederIds as seeder, index}
              <div class="flex items-start gap-2 rounded-md border border-border/50 bg-muted/40 p-2">
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
              {#if seederCopiedIndex === index}
                <p class="ml-6 text-xs text-emerald-600">Copied</p>
              {/if}
            {/each}
          </div>
        </div>
      {:else}
        <p class="text-xs text-muted-foreground italic">No seeders reported yet for this file.</p>
      {/if}
    </div>

    <div class="space-y-3">
      <p class="text-xs uppercase tracking-wide text-muted-foreground">Details</p>
      <ul class="space-y-2 text-sm text-foreground">
        <li class="flex items-center justify-between">
          <span class="text-muted-foreground">Seeder count</span>
          <span>{seederCount}</span>
        </li>
        <li class="flex items-center justify-between">
          <span class="text-muted-foreground">Estimated size</span>
          <span>{formatFileSize(metadata.fileSize)}</span>
        </li>
        <li class="flex items-center justify-between">
          <span class="text-muted-foreground">Price</span>
          <span class="font-semibold {metadata.price && metadata.price > 0 ? 'text-emerald-600' : 'text-muted-foreground'}">
            {#if metadata.price && metadata.price > 0}
              {metadata.price} Chiral
            {:else}
              Free
            {/if}
          </span>
        </li>
        <li class="flex items-center justify-between">
          <span class="text-muted-foreground">Hash prefix</span>
          <span>{metadata.fileHash.slice(0, 10)}…</span>
        </li>
      </ul>
    </div>
  </div>

  <div class="flex flex-col sm:flex-row gap-3 sm:items-center sm:justify-between">
    <div class="text-xs text-muted-foreground">
      {#if isSeeding}
        <span class="text-emerald-600 font-semibold">You are seeding this file</span>
        {#if metadata.isEncrypted}
          <span class="ml-2 text-xs text-amber-600">(encrypted)</span>
        {/if}
      {:else if !canAfford && metadata.price && metadata.price > 0}
        <span class="text-red-600 font-semibold">Insufficient balance to download this file</span>
      {:else if metadata.seeders?.length}
        {metadata.seeders.length > 1 ? 'Choose any seeder to initiate a download.' : 'Single seeder available for download.'}
      {:else}
        Waiting for peers to announce this file.
      {/if}
    </div>
    <div class="flex items-center gap-2">
      <Button
        on:click={handleDownload}
        disabled={isBusy || checkingBalance || (!canAfford && metadata.price && metadata.price > 0)}
        class={!canAfford && metadata.price && metadata.price > 0 ? 'opacity-50 cursor-not-allowed' : ''}
      >
        <Download class="h-4 w-4 mr-2" />
        {#if checkingBalance}
          Checking balance...
        {:else if !canAfford && metadata.price && metadata.price > 0}
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

        {#if metadata.price && metadata.price > 0}
          <div class="p-4 bg-blue-500/10 rounded-lg border-2 border-blue-500/30">
            <div class="text-center">
              <p class="text-sm text-muted-foreground mb-1">Price</p>
              <p class="text-2xl font-bold text-blue-600">{metadata.price} Chiral</p>
            </div>
          </div>
          {#if isSeeding}
            <div class="p-3 bg-amber-500/10 rounded-lg border border-amber-500/30">
              <p class="text-xs text-amber-600 text-center">
                You're already seeding this file. Downloading will create a decrypted local copy.
              </p>
            </div>
          {/if}
        {:else}
          <div class="p-4 bg-emerald-500/10 rounded-lg border-2 border-emerald-500/30">
            <div class="text-center">
              <p class="text-sm text-muted-foreground mb-1">Price</p>
              <p class="text-2xl font-bold text-emerald-600">Free</p>
            </div>
          </div>
          {#if isSeeding}
            <div class="p-3 bg-amber-500/10 rounded-lg border border-amber-500/30">
              <p class="text-xs text-amber-600 text-center">
                You're already seeding this file. Downloading will create a decrypted local copy.
              </p>
            </div>
          {/if}
        {/if}
      </div>

      <p class="text-sm text-muted-foreground text-center mb-6">
        {#if metadata.price && metadata.price > 0}
          {isSeeding
            ? `Do you want to download a local copy for ${metadata.price} Chiral?`
            : `You will be charged ${metadata.price} Chiral. Continue?`}
        {:else}
          {isSeeding
            ? 'Do you want to download a local decrypted copy?'
            : 'Are you sure you want to download this file?'}
        {/if}
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
          <span class="text-lg font-bold">{userBalance.toFixed(8)} Chiral</span>
        </div>

        <div class="flex justify-between items-center p-3 bg-blue-500/10 rounded-lg border border-blue-500/30">
          <span class="text-sm text-muted-foreground">File Price</span>
          <span class="text-lg font-bold text-blue-600">{(metadata.price || 0).toFixed(8)} Chiral</span>
        </div>

        <div class="flex justify-between items-center p-3 bg-muted/50 rounded-lg border-2 border-border">
          <span class="text-sm font-semibold">Balance After Purchase</span>
          <span class="text-lg font-bold {canAfford ? 'text-emerald-600' : 'text-red-600'}">
            {(userBalance - (metadata.price || 0)).toFixed(8)} Chiral
          </span>
        </div>
      </div>

      {#if !canAfford}
        <div class="mb-4 p-3 bg-red-500/10 border border-red-500/30 rounded-lg">
          <p class="text-sm text-red-600 font-semibold text-center">
            Insufficient balance! You need {(metadata.price || 0) - userBalance} more Chiral.
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
          Found {metadata.seeders.length} available peer{metadata.seeders.length === 1 ? '' : 's'}. Choose one to start the download.
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
