<script lang="ts">
  import { Copy, Check, X, Link, Lock, Globe, Trash2, Loader2 } from 'lucide-svelte';
  import { driveStore, type DriveItem, type DriveManifest } from '$lib/stores/driveStore';
  import type { ShareLink } from '$lib/services/driveApiService';
  import { toasts } from '$lib/toastStore';

  let {
    item,
    manifest,
    onClose,
  }: {
    item: DriveItem;
    manifest: DriveManifest;
    onClose: () => void;
  } = $props();

  let password = $state('');
  let isPublic = $state(true);
  let creating = $state(false);
  let copied = $state<string | null>(null);

  const existingShares = $derived(driveStore.getSharesForItem(item.id, manifest));

  async function createLink() {
    creating = true;
    try {
      const share = await driveStore.createShareLink(
        item.id,
        password || undefined,
        isPublic,
      );
      if (share) {
        toasts.show('Share link created', 'success');
        password = '';
      }
    } finally {
      creating = false;
    }
  }

  async function copyUrl(share: ShareLink) {
    const url = driveStore.getShareUrl(share.id);
    try {
      await navigator.clipboard.writeText(url);
      copied = share.id;
      setTimeout(() => copied = null, 2000);
    } catch {
      toasts.show('Failed to copy URL', 'error');
    }
  }

  async function revokeLink(share: ShareLink) {
    await driveStore.revokeShareLink(share.id);
    toasts.show('Share link revoked', 'success');
  }

  function formatDate(ts: number): string {
    return new Date(ts).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onclick={onClose}>
  <div
    class="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-lg mx-4 p-6"
    onclick={(e) => e.stopPropagation()}
  >
    <div class="flex items-center justify-between mb-4">
      <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Share "{item.name}"</h3>
      <button onclick={onClose} class="p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition">
        <X class="w-5 h-5 text-gray-500" />
      </button>
    </div>

    <!-- Create new share link -->
    <div class="space-y-3 mb-6">
      <p class="text-sm text-gray-600 dark:text-gray-400">
        Create a shareable link. Anyone with the link can download this {item.type}.
      </p>

      <div class="flex items-center gap-3">
        <label class="flex items-center gap-2 cursor-pointer">
          <input type="checkbox" bind:checked={isPublic} class="rounded border-gray-300 dark:border-gray-600" />
          <Globe class="w-4 h-4 text-gray-500" />
          <span class="text-sm text-gray-700 dark:text-gray-300">Public</span>
        </label>
      </div>

      <div class="flex items-center gap-2">
        <Lock class="w-4 h-4 text-gray-400 shrink-0" />
        <input
          type="password"
          placeholder="Optional password"
          bind:value={password}
          class="flex-1 px-3 py-2 bg-gray-100 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg text-sm text-gray-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
      </div>

      <button
        onclick={createLink}
        disabled={creating}
        class="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white rounded-lg transition text-sm font-medium"
      >
        {#if creating}
          <Loader2 class="w-4 h-4 animate-spin" />
          Creating...
        {:else}
          <Link class="w-4 h-4" />
          Create Link
        {/if}
      </button>
    </div>

    <!-- Existing share links -->
    {#if existingShares.length > 0}
      <div class="border-t border-gray-200 dark:border-gray-700 pt-4">
        <h4 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">
          Active Links ({existingShares.length})
        </h4>
        <div class="space-y-2 max-h-48 overflow-y-auto">
          {#each existingShares as share (share.id)}
            <div class="flex items-center gap-2 p-2 bg-gray-50 dark:bg-gray-700/50 rounded-lg">
              <div class="flex-1 min-w-0">
                <code class="text-xs text-gray-600 dark:text-gray-400 font-mono truncate block">
                  {driveStore.getShareUrl(share.id)}
                </code>
                <div class="flex items-center gap-2 mt-0.5">
                  <span class="text-xs text-gray-400">
                    Created {formatDate(share.createdAt)}
                  </span>
                  {#if share.hasPassword}
                    <Lock class="w-3 h-3 text-yellow-500" />
                  {/if}
                  {#if share.isPublic}
                    <Globe class="w-3 h-3 text-green-500" />
                  {/if}
                  <span class="text-xs text-gray-400">
                    {share.downloadCount} download{share.downloadCount !== 1 ? 's' : ''}
                  </span>
                </div>
              </div>
              <button
                onclick={() => copyUrl(share)}
                class="p-1.5 hover:bg-gray-200 dark:hover:bg-gray-600 rounded transition"
                title="Copy link"
              >
                {#if copied === share.id}
                  <Check class="w-4 h-4 text-green-500" />
                {:else}
                  <Copy class="w-4 h-4 text-gray-500" />
                {/if}
              </button>
              <button
                onclick={() => revokeLink(share)}
                class="p-1.5 hover:bg-red-100 dark:hover:bg-red-900/30 rounded transition"
                title="Revoke link"
              >
                <Trash2 class="w-4 h-4 text-red-500" />
              </button>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    <div class="mt-4 p-3 bg-blue-50 dark:bg-blue-900/20 rounded-lg">
      <p class="text-xs text-blue-700 dark:text-blue-300">
        <strong>File:</strong> {item.name}
        {#if item.size}
          <span class="ml-2 text-blue-500">({(item.size / (1024 * 1024)).toFixed(1)} MB)</span>
        {/if}
      </p>
    </div>
  </div>
</div>
