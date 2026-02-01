<script lang="ts">
  import { fly } from 'svelte/transition';
  import { X } from 'lucide-svelte';
  
  interface Props {
    message: string;
    type?: 'success' | 'error' | 'info';
    onClose: () => void;
  }
  
  let { message, type = 'info', onClose }: Props = $props();
  
  const typeStyles = {
    success: 'bg-green-500',
    error: 'bg-red-500',
    info: 'bg-blue-500'
  };
</script>

<div
  transition:fly={{ y: -20, duration: 300 }}
  class="fixed top-4 right-4 z-50 flex items-center gap-3 {typeStyles[type]} text-white px-6 py-3 rounded-lg shadow-lg max-w-md"
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
