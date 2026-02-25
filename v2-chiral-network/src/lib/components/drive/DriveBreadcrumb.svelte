<script lang="ts">
  import { ChevronRight, HardDrive } from 'lucide-svelte';
  import type { DriveItem } from '$lib/stores/driveStore';

  let { breadcrumb = [], onNavigate }: { breadcrumb: DriveItem[]; onNavigate: (id: string | null) => void } = $props();
</script>

<nav class="flex items-center gap-1 text-sm text-gray-600 dark:text-gray-400 flex-wrap">
  <button
    onclick={() => onNavigate(null)}
    class="flex items-center gap-1 hover:text-gray-900 dark:hover:text-white transition font-medium px-1.5 py-0.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700"
  >
    <HardDrive class="w-4 h-4" />
    <span>My Drive</span>
  </button>

  {#each breadcrumb as crumb, i}
    <ChevronRight class="w-3 h-3 text-gray-400 shrink-0" />
    <button
      onclick={() => onNavigate(crumb.id)}
      class="hover:text-gray-900 dark:hover:text-white transition font-medium px-1.5 py-0.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700
        {i === breadcrumb.length - 1 ? 'text-gray-900 dark:text-white' : ''}"
    >
      {crumb.name}
    </button>
  {/each}
</nav>
