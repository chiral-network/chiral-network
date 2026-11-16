<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { IconComponent } from './tones';
  let { icon: Icon, title, children = undefined }: { icon: IconComponent; title: string; children?: Snippet } = $props();
</script>

<div class="flex flex-col items-center text-center py-10 px-4">
  <div class="p-3 rounded-full bg-gray-100 dark:bg-gray-700 mb-3">
    <Icon class="w-7 h-7 text-gray-400 dark:text-gray-500" />
  </div>
  <p class="text-sm font-medium text-gray-700 dark:text-gray-200">{title}</p>
  {#if children}<div class="text-xs text-gray-500 dark:text-gray-400 mt-1 max-w-md">{@render children()}</div>{/if}
</div>
