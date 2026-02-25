<script lang="ts">
  import { Copy, Check, X } from 'lucide-svelte';
  import type { DriveItem } from '$lib/stores/driveStore';

  let { item, onClose }: { item: DriveItem; onClose: () => void } = $props();

  let copied = $state(false);

  async function copyHash() {
    if (!item.hash) return;
    try {
      await navigator.clipboard.writeText(item.hash);
      copied = true;
      setTimeout(() => copied = false, 2000);
    } catch {
      // fallback
      const ta = document.createElement('textarea');
      ta.value = item.hash;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand('copy');
      document.body.removeChild(ta);
      copied = true;
      setTimeout(() => copied = false, 2000);
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onclick={onClose}>
  <div
    class="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-md mx-4 p-6"
    onclick={(e) => e.stopPropagation()}
  >
    <div class="flex items-center justify-between mb-4">
      <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Share File</h3>
      <button onclick={onClose} class="p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition">
        <X class="w-5 h-5 text-gray-500" />
      </button>
    </div>

    <p class="text-sm text-gray-600 dark:text-gray-400 mb-4">
      Share this hash with others. They can paste it into the Download page to find and download this file via P2P.
    </p>

    <label class="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wide">File Hash</label>
    <div class="mt-1 flex items-center gap-2">
      <code class="flex-1 text-xs bg-gray-100 dark:bg-gray-700 text-gray-800 dark:text-gray-200 px-3 py-2.5 rounded-lg font-mono break-all select-all">
        {item.hash || 'No hash available'}
      </code>
      <button
        onclick={copyHash}
        class="shrink-0 flex items-center gap-1.5 px-3 py-2.5 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition text-sm font-medium"
        disabled={!item.hash}
      >
        {#if copied}
          <Check class="w-4 h-4" />
          Copied
        {:else}
          <Copy class="w-4 h-4" />
          Copy
        {/if}
      </button>
    </div>

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
