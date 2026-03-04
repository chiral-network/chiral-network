<script lang="ts">
  import { Copy, ExternalLink, X, Plus, Globe, Link } from 'lucide-svelte';
  import { getFileIcon, getFileColor } from '$lib/utils/fileIcons';
  import { driveStore, type DriveItem, type DriveManifest } from '$lib/stores/driveStore';
  import { networkConnected, walletAccount } from '$lib/stores';
  import { toasts } from '$lib/toastStore';

  let {
    manifest,
    onAddFiles,
  }: {
    manifest: DriveManifest;
    onAddFiles: (protocol: 'WebRTC' | 'BitTorrent', priceChi: string) => void;
  } = $props();

  let selectedProtocol = $state<'WebRTC' | 'BitTorrent'>('WebRTC');
  let filePrice = $state('');
  let expandedFileId = $state<string | null>(null);

  const seedingItems = $derived(driveStore.getSeedingItems(manifest));

  function formatFileSize(bytes?: number): string {
    if (!bytes) return '0 B';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function formatDate(date: number): string {
    return new Intl.DateTimeFormat('en-US', {
      month: 'short', day: 'numeric', year: 'numeric',
      hour: '2-digit', minute: '2-digit',
    }).format(new Date(date));
  }

  function generateMagnetLink(item: DriveItem): string {
    const encodedName = encodeURIComponent(item.name);
    return `magnet:?xt=urn:btih:${item.merkleRoot}&dn=${encodedName}&xl=${item.size || 0}`;
  }

  async function copyToClipboard(text: string, label: string) {
    try {
      await navigator.clipboard.writeText(text);
      toasts.show(`${label} copied to clipboard`, 'success');
    } catch {
      toasts.show('Failed to copy to clipboard', 'error');
    }
  }

  async function handleStopSeeding(item: DriveItem) {
    await driveStore.stopSeeding(item.id);
    toasts.show(`Stopped seeding ${item.name}`, 'info');
  }

  async function handleExportTorrent(item: DriveItem) {
    const path = await driveStore.exportTorrent(item.id);
    if (path) {
      toasts.show(`Torrent file saved to ${path}`, 'success');
    } else {
      toasts.show('Failed to export torrent file', 'error');
    }
  }

  function getProtocolColor(protocol?: string): string {
    return protocol === 'BitTorrent'
      ? 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400'
      : 'bg-blue-100 text-blue-800 dark:bg-blue-900/30 dark:text-blue-400';
  }

  function handleAddFiles() {
    if (!$networkConnected) {
      toasts.show('Please connect to the network first', 'error');
      return;
    }
    onAddFiles(selectedProtocol, filePrice);
  }
</script>

<div class="space-y-4">
  <!-- Protocol selector + Price input + Add files button -->
  <div class="bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl p-4">
    <div class="flex flex-wrap items-end gap-4">
      <!-- Protocol toggle -->
      <div>
        <label class="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5">Protocol</label>
        <div class="flex rounded-lg overflow-hidden border border-gray-200 dark:border-gray-600">
          <button
            onclick={() => selectedProtocol = 'WebRTC'}
            class="px-3 py-1.5 text-sm font-medium transition {selectedProtocol === 'WebRTC'
              ? 'bg-blue-600 text-white'
              : 'bg-gray-50 dark:bg-gray-700 text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-600'}"
          >
            WebRTC
          </button>
          <button
            onclick={() => selectedProtocol = 'BitTorrent'}
            class="px-3 py-1.5 text-sm font-medium transition {selectedProtocol === 'BitTorrent'
              ? 'bg-green-600 text-white'
              : 'bg-gray-50 dark:bg-gray-700 text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-600'}"
          >
            BitTorrent
          </button>
        </div>
      </div>

      <!-- Price input -->
      <div>
        <label class="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5">Price (CHI)</label>
        <input
          type="number"
          step="0.001"
          min="0"
          placeholder="Free"
          bind:value={filePrice}
          class="w-28 px-3 py-1.5 text-sm bg-gray-50 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg text-gray-900 dark:text-white placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
      </div>

      <!-- Add files button -->
      <button
        onclick={handleAddFiles}
        disabled={!$networkConnected}
        class="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 disabled:bg-gray-400 text-white rounded-lg transition text-sm font-medium"
      >
        <Plus class="w-4 h-4" />
        Add Files to Seed
      </button>
    </div>

    {#if filePrice && parseFloat(filePrice) > 0 && !$walletAccount}
      <p class="mt-2 text-xs text-amber-600 dark:text-amber-400">
        Connect your wallet on the Account page to receive payments.
      </p>
    {/if}
  </div>

  <!-- Seeding files list -->
  {#if seedingItems.length === 0}
    <div class="text-center py-12 text-gray-500 dark:text-gray-400">
      <Globe class="w-12 h-12 mx-auto mb-3 opacity-40" />
      <p class="text-sm font-medium">No files being seeded</p>
      <p class="text-xs mt-1">Add files above or right-click any file in "All Files" and choose "Seed to Network"</p>
    </div>
  {:else}
    <div class="space-y-2">
      {#each seedingItems as item (item.id)}
        {@const Icon = getFileIcon(item.name)}
        <div class="bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl p-4">
          <div class="flex items-start gap-3">
            <!-- File icon -->
            <div class="flex-shrink-0 w-10 h-10 rounded-lg bg-gray-50 dark:bg-gray-700 flex items-center justify-center">
              <svelte:component this={Icon} class="w-5 h-5 {getFileColor(item.name)}" />
            </div>

            <!-- File info -->
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 flex-wrap">
                <span class="text-sm font-medium text-gray-900 dark:text-white truncate">{item.name}</span>
                <span class="px-2 py-0.5 text-xs font-medium rounded {getProtocolColor(item.protocol)}">
                  {item.protocol || 'WebRTC'}
                </span>
                {#if item.priceChi && item.priceChi !== '0'}
                  <span class="px-2 py-0.5 text-xs font-medium rounded bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400">
                    {item.priceChi} CHI
                  </span>
                {:else}
                  <span class="px-2 py-0.5 text-xs font-medium rounded bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400">
                    Free
                  </span>
                {/if}
              </div>

              <div class="flex items-center gap-3 mt-1 text-xs text-gray-500 dark:text-gray-400">
                <span>{formatFileSize(item.size)}</span>
                <span>{formatDate(item.modifiedAt)}</span>
              </div>

              <!-- Merkle hash -->
              {#if item.merkleRoot}
                <div class="flex items-center gap-2 mt-2">
                  <span class="text-xs text-gray-500 dark:text-gray-400 font-mono truncate">
                    {item.merkleRoot}
                  </span>
                  <button
                    onclick={() => copyToClipboard(item.merkleRoot!, 'Hash')}
                    class="flex-shrink-0 p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded"
                    title="Copy hash"
                  >
                    <Copy class="w-3.5 h-3.5 text-gray-400" />
                  </button>
                </div>
              {/if}
            </div>

            <!-- Actions -->
            <div class="flex items-center gap-1 flex-shrink-0">
              <button
                onclick={() => expandedFileId = expandedFileId === item.id ? null : item.id}
                class="p-1.5 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition"
                title="Share options"
              >
                <Link class="w-4 h-4 text-gray-500" />
              </button>
              <button
                onclick={() => handleStopSeeding(item)}
                class="p-1.5 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition"
                title="Stop seeding"
              >
                <X class="w-4 h-4 text-red-500" />
              </button>
            </div>
          </div>

          <!-- Expanded share options -->
          {#if expandedFileId === item.id && item.merkleRoot}
            <div class="mt-3 pt-3 border-t border-gray-100 dark:border-gray-700 space-y-2">
              <!-- Magnet link -->
              <div class="flex items-center gap-2">
                <span class="text-xs text-gray-500 dark:text-gray-400 w-16 flex-shrink-0">Magnet</span>
                <input
                  type="text"
                  readonly
                  value={generateMagnetLink(item)}
                  class="flex-1 text-xs font-mono bg-gray-50 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded px-2 py-1 text-gray-600 dark:text-gray-300"
                />
                <button
                  onclick={() => copyToClipboard(generateMagnetLink(item), 'Magnet link')}
                  class="flex-shrink-0 p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded"
                >
                  <Copy class="w-3.5 h-3.5 text-gray-400" />
                </button>
              </div>

              <!-- Hash -->
              <div class="flex items-center gap-2">
                <span class="text-xs text-gray-500 dark:text-gray-400 w-16 flex-shrink-0">Hash</span>
                <input
                  type="text"
                  readonly
                  value={item.merkleRoot}
                  class="flex-1 text-xs font-mono bg-gray-50 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded px-2 py-1 text-gray-600 dark:text-gray-300"
                />
                <button
                  onclick={() => copyToClipboard(item.merkleRoot!, 'Hash')}
                  class="flex-shrink-0 p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded"
                >
                  <Copy class="w-3.5 h-3.5 text-gray-400" />
                </button>
              </div>

              <!-- Export torrent -->
              <button
                onclick={() => handleExportTorrent(item)}
                class="flex items-center gap-1.5 text-xs text-blue-600 dark:text-blue-400 hover:underline"
              >
                <ExternalLink class="w-3.5 h-3.5" />
                Export .torrent file
              </button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
