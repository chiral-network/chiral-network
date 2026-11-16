<script lang="ts">
  import { session } from '$lib/stores';
  import { nativeAvailable } from '$lib/native';
  let { compact = false }: { compact?: boolean } = $props();
  const native = nativeAvailable();
  const state = $derived(!native ? 'preview' : $session.configured ? 'configured' : 'unconfigured');
  const view = $derived({
    preview: { box: 'bg-gray-100 dark:bg-gray-700', dot: 'bg-gray-400', text: 'text-gray-600 dark:text-gray-300', label: 'Browser preview', title: 'Native runtime unavailable in a browser preview' },
    unconfigured: { box: 'bg-amber-50 dark:bg-amber-900/30', dot: 'bg-amber-500', text: 'text-amber-700 dark:text-amber-400', label: 'No profile', title: 'Load an explicit consumer profile in Settings' },
    configured: { box: 'bg-green-50 dark:bg-green-900/30', dot: 'bg-green-500', text: 'text-green-700 dark:text-green-400', label: 'Profile loaded', title: 'Native consumer profile loaded' },
  }[state]);
</script>

<a href={state === 'configured' ? '#/network' : '#/settings'} class="flex items-center gap-1.5 px-2 py-1 rounded-full no-underline {view.box}" title={view.title}>
  <span class="w-2 h-2 rounded-full shrink-0 {view.dot}"></span>
  {#if compact}<span class="sr-only">{view.label}</span>{:else}<span class="text-xs font-medium whitespace-nowrap {view.text}">{view.label}</span>{/if}
</a>
