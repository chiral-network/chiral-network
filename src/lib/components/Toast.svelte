<script lang="ts">
  import { fly } from 'svelte/transition';
  import { X } from 'lucide-svelte';

  interface Props {
    message: string;
    type?: 'success' | 'error' | 'info' | 'warning';
    index?: number;
    onClose: () => void;
  }

  let { message, type = 'info', index = 0, onClose }: Props = $props();

  const typeStyles = {
    success: 'bg-gray-900 border border-emerald-500/40 shadow-[0_0_15px_rgba(52,211,153,0.2)]',
    error: 'bg-gray-900 border border-red-500/40 shadow-[0_0_15px_rgba(248,113,113,0.2)]',
    info: 'bg-gray-900 border border-cyan-500/40 shadow-[0_0_15px_rgba(6,182,212,0.2)]',
    warning: 'bg-gray-900 border border-yellow-500/40 shadow-[0_0_15px_rgba(234,179,8,0.2)]'
  };

  // Calculate vertical offset based on index (each toast is ~56px tall + 8px gap)
  let topOffset = $derived(16 + (index * 64));
</script>

<div
  transition:fly={{ y: -20, duration: 300 }}
  class="fixed right-4 z-50 flex items-center gap-3 {typeStyles[type]} text-gray-100 px-6 py-3 rounded-lg max-w-md"
  style="top: {topOffset}px;"
>
  <span class="flex-1">{message}</span>
  <button
    onclick={onClose}
    class="text-white hover:text-gray-200 transition-colors"
    aria-label="Close toast"
  >
    <X size={18} />
  </button>
</div>
