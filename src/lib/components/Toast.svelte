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
    success: 'bg-white dark:bg-gray-900 border border-green-200 dark:border-green-800 text-green-800 dark:text-green-300',
    error: 'bg-white dark:bg-gray-900 border border-red-200 dark:border-red-800 text-red-800 dark:text-red-300',
    info: 'bg-white dark:bg-gray-900 border border-indigo-200 dark:border-indigo-800 text-indigo-800 dark:text-indigo-300',
    warning: 'bg-white dark:bg-gray-900 border border-yellow-200 dark:border-yellow-800 text-yellow-800 dark:text-yellow-300'
  };

  // Calculate vertical offset based on index (each toast is ~56px tall + 8px gap)
  let topOffset = $derived(16 + (index * 64));
</script>

<div
  transition:fly={{ y: -20, duration: 300 }}
  class="fixed right-4 z-50 flex items-center gap-3 {typeStyles[type]} px-6 py-3 rounded-xl shadow-sm max-w-md"
  style="top: {topOffset}px;"
>
  <span class="flex-1">{message}</span>
  <button
    onclick={onClose}
    class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors"
    aria-label="Close toast"
  >
    <X size={18} />
  </button>
</div>
