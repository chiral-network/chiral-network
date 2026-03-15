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
 success:'bg-[var(--surface-0)]/[0.07] border border-green-800 text-green-400',
 error:'bg-[var(--surface-0)]/[0.07] border border-red-800 text-red-400',
 info:'bg-[var(--surface-0)]/[0.07] border border-indigo-800 text-indigo-400',
 warning:'bg-[var(--surface-0)]/[0.07] border border-yellow-800 text-yellow-800'
 };

 // Calculate vertical offset based on index (each toast is ~56px tall + 8px gap)
 let topOffset = $derived(16 + (index * 64));
</script>

<div
 transition:fly={{ y: -20, duration: 300 }}
 class="fixed right-4 z-50 flex items-center gap-3 {typeStyles[type]} px-6 py-3 rounded-xl max-w-md"
 style="top: {topOffset}px;"
>
 <span class="flex-1">{message}</span>
 <button
 onclick={onClose}
 class="text-[var(--text-secondary)] hover:text-[var(--text-secondary)] transition-colors"
 aria-label="Close toast"
 >
 <X size={18} />
 </button>
</div>
