<script lang="ts">
  import type { Snippet } from 'svelte';
  import { AlertTriangle, AlertCircle, CheckCircle2, Info } from 'lucide-svelte';
  type Kind = 'warning' | 'error' | 'success' | 'info';
  interface Props { kind?: Kind; title?: string; role?: 'alert' | 'status'; class?: string; children?: Snippet }
  let { kind = 'info', title = '', role = undefined, class: extra = '', children = undefined }: Props = $props();
  const styles: Record<Kind, { box: string; icon: string; title: string; body: string }> = {
    warning: { box: 'bg-amber-50 dark:bg-amber-900/20 border-amber-200 dark:border-amber-800', icon: 'text-amber-600 dark:text-amber-400', title: 'text-amber-900 dark:text-amber-200', body: 'text-amber-800 dark:text-amber-300' },
    error: { box: 'bg-red-50 dark:bg-red-900/20 border-red-200 dark:border-red-800', icon: 'text-red-600 dark:text-red-400', title: 'text-red-900 dark:text-red-200', body: 'text-red-800 dark:text-red-300' },
    success: { box: 'bg-green-50 dark:bg-green-900/20 border-green-200 dark:border-green-800', icon: 'text-green-600 dark:text-green-400', title: 'text-green-900 dark:text-green-200', body: 'text-green-800 dark:text-green-300' },
    info: { box: 'bg-primary-50 dark:bg-primary-900/20 border-primary-200 dark:border-primary-800', icon: 'text-primary-600 dark:text-primary-400', title: 'text-primary-900 dark:text-primary-200', body: 'text-gray-700 dark:text-gray-300' },
  };
  const icons = { warning: AlertTriangle, error: AlertCircle, success: CheckCircle2, info: Info };
  const s = $derived(styles[kind]);
  const Icon = $derived(icons[kind]);
</script>

<div class="flex items-start gap-3 px-4 py-3 rounded-xl border {s.box} {extra}" {role}>
  <Icon class="w-4 h-4 mt-0.5 shrink-0 {s.icon}" />
  <!-- Exact wei, wallets and endpoints have no break points; wrap them at phone widths instead of overflowing. -->
  <div class="min-w-0 flex-1 text-sm leading-relaxed break-words {s.body} {kind === 'error' ? 'whitespace-pre-wrap' : ''}">
    {#if title}<p class="font-semibold {s.title}">{title}</p>{/if}
    {@render children?.()}
  </div>
</div>
