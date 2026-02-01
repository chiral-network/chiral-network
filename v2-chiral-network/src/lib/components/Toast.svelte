<script lang="ts">
  import { fly } from 'svelte/transition';
  import { X } from 'lucide-svelte';

  interface Props {
    message: string;
    type?: 'success' | 'error' | 'info';
    index?: number;
    onClose: () => void;
  }

  let { message, type = 'info', index = 0, onClose }: Props = $props();

  const typeStyles = {
    success: 'bg-green-500',
    error: 'bg-red-500',
    info: 'bg-blue-500'
  };

  // Calculate vertical offset based on index (each toast is ~56px tall + 8px gap)
  let topOffset = $derived(16 + (index * 64));
</script>

<div
  transition:fly={{ y: -20, duration: 300 }}
  class="fixed right-4 z-50 flex items-center gap-3 {typeStyles[type]} text-white px-6 py-3 rounded-lg shadow-lg max-w-md"
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
