<script lang="ts">
  import { ChevronRight, Braces } from 'lucide-svelte';
  let { value, title = 'Response', open = true }: { value: unknown; title?: string; open?: boolean } = $props();
</script>
{#if value !== null && value !== undefined}
  <details class="group card overflow-hidden" {open}>
    <summary class="flex items-center gap-2.5 px-4 py-3 select-none list-none text-sm font-medium text-gray-900 dark:text-white hover:bg-gray-50 dark:hover:bg-gray-700/40 transition-colors [&::-webkit-details-marker]:hidden">
      <ChevronRight class="w-4 h-4 text-gray-400 transition-transform group-open:rotate-90 shrink-0" />
      <span class="p-1 rounded-md bg-primary-100 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400 shrink-0"><Braces class="w-3.5 h-3.5" /></span>
      <span class="flex-1 min-w-0 truncate">{title}</span>
      <span class="stat-label shrink-0">JSON</span>
    </summary>
    <pre class="code-block border-t border-gray-200 dark:border-gray-700">{JSON.stringify(value, null, 2)}</pre>
  </details>
{/if}
