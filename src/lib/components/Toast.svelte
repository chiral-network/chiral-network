<script lang="ts">
 import { fly } from'svelte/transition';
 import { X } from'lucide-svelte';

 interface Props {
 message: string;
 type?:'success' |'error' |'info' |'warning';
 index?: number;
 onClose: () => void;
 }

 let { message, type ='info', index = 0, onClose }: Props = $props();

 const typeStyles = {
 success:'bg-[var(--surface-1)] border border-emerald-500/20 text-emerald-600 dark:text-emerald-400',
 error:'bg-[var(--surface-1)] border border-red-500/20 text-red-600 dark:text-red-400',
 info:'bg-[var(--surface-1)] border border-violet-500/20 text-violet-600 dark:text-violet-400',
 warning:'bg-[var(--surface-1)] border border-yellow-500/20 text-yellow-600 dark:text-yellow-400'
 };

 // Calculate vertical offset based on index (each toast is ~56px tall + 8px gap)
 let topOffset = $derived(16 + (index * 64));
</script>

<div
 transition:fly={{ y: -20, duration: 300 }}
 class="fixed right-4 z-50 flex items-center gap-3 {typeStyles[type]} px-5 py-3 rounded-lg max-w-md shadow-md dark:shadow-none"
 style="top: {topOffset}px;"
>
 <span class="flex-1 text-sm">{message}</span>
 <button
 onclick={onClose}
 class="text-[var(--text-tertiary)] hover:text-[var(--text-secondary)] transition-colors"
 aria-label="Close toast"
 >
 <X size={16} />
 </button>
</div>
