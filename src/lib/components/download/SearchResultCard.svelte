<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import { FileIcon, Copy, Download, Server } from 'lucide-svelte';
  import { createEventDispatcher } from 'svelte';
  import { dhtService, type FileMetadata } from '$lib/dht';
  import { formatRelativeTime, toHumanReadableSize } from '$lib/utils';
  import { files } from '$lib/stores';
  import { get } from 'svelte/store';
  import { showToast } from '$lib/toast';

  const dispatch = createEventDispatcher<{ download: FileMetadata; copy: string }>();

  export let metadata: FileMetadata;
  export let isBusy = false;
  export let isBitswap: boolean = false;

  let hashCopied = false;
  let seederCopiedIndex: number | null = null;
  let showSeedingNotice = false;
  let showDecryptDialog = false;

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
    if (isSeeding) {
      showDecryptDialog = true;
    } else {
      if (isBitswap) {
        console.log("🔍 DEBUG: Initiating Bitswap download for file:", metadata.fileName);
        await dhtService.downloadFile(metadata);
        showToast(
          `The file "${metadata.fileName}" has been added to your download folder via Bitswap.`,
        );
      }
      else {
        console.log("🔍 DEBUG: Initiating WebRTC download for file:", metadata.fileName);
        dispatch('download', metadata);
      }
    }
  }

  async function confirmDecryptAndQueue() {
    showDecryptDialog = false;
    await dhtService.downloadFile(metadata);
      if (isBitswap) {
        console.log("🔍 DEBUG: Initiating Bitswap download for file:", metadata.fileName);
      
        await dhtService.downloadFile(metadata);
        showToast(
          `The file "${metadata.fileName}" has been added to your download folder via Bitswap.`,
        );
      }
      else {
        console.log("🔍 DEBUG: Initiating WebRTC download for file:", metadata.fileName);
        dispatch('download', metadata);
      }
  }

  function cancelDecryptDialog() {
    showDecryptDialog = false;
  }

  const seederIds = metadata.seeders?.map((address, index) => ({
    id: `${metadata.fileHash}-${index}`,
    address,
  })) ?? [];
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

    <div class="flex items-center gap-2">
      <Badge class="bg-emerald-500/10 text-emerald-600 dark:text-emerald-300 border border-emerald-500/30">
        <Server class="h-3.5 w-3.5 mr-1" />
        {seederCount} {seederCount === 1 ? 'Seeder' : 'Seeders'}
      </Badge>
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
      {:else if metadata.seeders?.length}
        {metadata.seeders.length > 1 ? 'Choose any seeder to initiate a download.' : 'Single seeder available for download.'}
      {:else}
        Waiting for peers to announce this file.
      {/if}
    </div>
    <div class="flex items-center gap-2">
      <Button on:click={handleDownload} disabled={isBusy}>
        <Download class="h-4 w-4 mr-2" />
        Add to queue
      </Button>
    </div>
  </div>

  {#if showDecryptDialog}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
    <div class="bg-background rounded-lg shadow-lg p-6 w-full max-w-md border border-border">
      <h2 class="text-lg font-semibold mb-2">Already Seeding</h2>
      <p class="mb-4 text-sm text-muted-foreground">
        You’re already seeding this file{metadata.isEncrypted ? ' (encrypted)' : ''}.<br />
        Would you like to decrypt and save a local readable copy?
      </p>
      <div class="flex justify-end gap-2 mt-4">
        <Button variant="outline" on:click={cancelDecryptDialog}>Cancel</Button>
        <Button on:click={confirmDecryptAndQueue}>Add to queue</Button>
      </div>
    </div>
  </div>
{/if}
</Card>
