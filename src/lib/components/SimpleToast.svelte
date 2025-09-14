<script lang="ts">
  import { toasts } from '../toast'
  import { slide } from 'svelte/transition'
  import { X } from 'lucide-svelte'
  
  function removeToast(id: string) {
    toasts.update(currentToasts => currentToasts.filter(toast => toast.id !== id))
  }
</script>

<div class="fixed top-4 right-4 z-50 space-y-2">
  {#each $toasts as toast (toast.id)}
    <div 
      transition:slide={{ duration: 300 }}
      class="flex items-center gap-3 p-4 rounded-lg shadow-lg text-white min-w-[300px] max-w-[400px] {toast.type === 'success' ? 'bg-green-600' : 'bg-red-600'}"
    >
      <!-- Message content -->
      <div class="flex-1 text-sm font-medium">
        {toast.message}
      </div>
      
      <!-- Close button -->
      <button 
        class="p-1 rounded hover:bg-white/20 transition-colors"
        on:click={() => removeToast(toast.id)}
        title="Close"
      >
        <X class="h-4 w-4" />
      </button>
    </div>
  {/each}
</div>