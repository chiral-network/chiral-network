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
    success: 'backdrop-blur-xl bg-green-500/70 border border-green-400/30 ring-1 ring-white/10',
    error: 'backdrop-blur-xl bg-red-500/70 border border-red-400/30 ring-1 ring-white/10',
    info: 'backdrop-blur-xl bg-blue-500/70 border border-blue-400/30 ring-1 ring-white/10',
    warning: 'backdrop-blur-xl bg-yellow-500/70 border border-yellow-400/30 ring-1 ring-white/10'
  };

  // Calculate vertical offset based on index (each toast is ~56px tall + 8px gap)
  let topOffset = $derived(16 + (index * 64));
</script>

<div
  transition:fly={{ y: -20, duration: 300 }}
  class="fixed right-4 z-50 flex items-center gap-3 {typeStyles[type]} text-white px-6 py-3 rounded-lg shadow-lg shadow-black/10 max-w-md"
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
