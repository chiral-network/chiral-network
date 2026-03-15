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
    success: 'bg-gray-950 border border-emerald-500/30 text-emerald-300',
    error: 'bg-gray-950 border border-red-500/30 text-red-300',
    info: 'bg-gray-950 border border-cyan-500/30 text-cyan-300',
    warning: 'bg-gray-950 border border-amber-500/30 text-amber-300'
  };

  // Calculate vertical offset based on index (each toast is ~56px tall + 8px gap)
  let topOffset = $derived(16 + (index * 64));
</script>

<div
  transition:fly={{ y: -20, duration: 300 }}
  class="fixed right-4 z-50 flex items-center gap-3 {typeStyles[type]} px-6 py-3 rounded-lg max-w-md"
  style="top: {topOffset}px;"
>
  <span class="flex-1 text-gray-100">{message}</span>
  <button
    onclick={onClose}
    class="text-gray-500 hover:text-gray-300 transition-colors"
    aria-label="Close toast"
  >
    <X size={18} />
  </button>
</div>
