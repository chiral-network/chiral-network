<script lang="ts">
  import { confirmDialog } from '$lib/confirmDialog';
  import { fade, scale } from 'svelte/transition';
  import { AlertTriangle, X } from 'lucide-svelte';
  import Button from './ui/button.svelte';

  $: isOpen = $confirmDialog.isOpen;
  $: title = $confirmDialog.title;
  $: message = $confirmDialog.message;
  $: confirmText = $confirmDialog.confirmText || 'Confirm';
  $: cancelText = $confirmDialog.cancelText || 'Cancel';
  $: confirmVariant = $confirmDialog.confirmVariant || 'primary';
  $: isProcessing = $confirmDialog.isProcessing;

  function handleConfirm() {
    if ($confirmDialog.onConfirm && !isProcessing) {
      $confirmDialog.onConfirm();
    }
  }

  function handleCancel() {
    if ($confirmDialog.onCancel && !isProcessing) {
      $confirmDialog.onCancel();
    } else {
      confirmDialog.close();
    }
  }

  function handleBackdropClick() {
    if (!isProcessing) {
      handleCancel();
    }
  }

  function handleBackdropKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' || event.key === ' ') {
      handleBackdropClick();
    }
  }

  // Handle escape key
  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && !isProcessing) {
      handleCancel();
    }
  }
</script>

{#if isOpen}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center p-4"
    transition:fade={{ duration: 150 }}
    on:click={handleBackdropClick}
    on:keydown={handleKeydown}
    role="presentation"
  >
    <!-- Backdrop -->
    <div 
      class="absolute inset-0 bg-black bg-opacity-60"
      on:click={handleBackdropClick}
      on:keydown={handleBackdropKeydown}
      role="button"
      tabindex="-1"
      aria-label="Close dialog"
    ></div>

    <!-- Dialog -->
    <div
      class="relative bg-gray-800 rounded-lg shadow-xl max-w-md w-full"
      transition:scale={{ duration: 150, start: 0.95 }}
      on:click|stopPropagation
      on:keydown|stopPropagation
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="confirm-dialog-title"
    >
      <!-- Close button -->
      {#if !isProcessing}
        <button
          class="absolute top-4 right-4 text-gray-400 hover:text-white transition-colors"
          on:click={handleCancel}
          aria-label="Close dialog"
        >
          <X class="h-5 w-5" />
        </button>
      {/if}

      <!-- Content -->
      <div class="p-6">
        <!-- Icon and Title -->
        <div class="flex items-start gap-4 mb-4">
          {#if confirmVariant === 'danger'}
            <div class="flex-shrink-0 w-12 h-12 rounded-full bg-red-900/50 flex items-center justify-center">
              <AlertTriangle class="h-6 w-6 text-red-400" />
            </div>
          {:else if confirmVariant === 'warning'}
            <div class="flex-shrink-0 w-12 h-12 rounded-full bg-yellow-900/50 flex items-center justify-center">
              <AlertTriangle class="h-6 w-6 text-yellow-400" />
            </div>
          {:else}
            <div class="flex-shrink-0 w-12 h-12 rounded-full bg-blue-900/50 flex items-center justify-center">
              <AlertTriangle class="h-6 w-6 text-blue-400" />
            </div>
          {/if}

          <div class="flex-1 min-w-0">
            <h3 id="confirm-dialog-title" class="text-xl font-semibold text-white mb-2">
              {title}
            </h3>
            <p class="text-gray-300 text-sm leading-relaxed">
              {message}
            </p>
          </div>
        </div>

        <!-- Actions -->
        <div class="flex gap-3 justify-end mt-6">
          <Button
            variant="secondary"
            on:click={handleCancel}
            disabled={isProcessing}
          >
            {cancelText}
          </Button>

          <Button
            variant={confirmVariant === 'danger' ? 'destructive' : 'default'}
            on:click={handleConfirm}
            disabled={isProcessing}
            class={confirmVariant === 'warning' ? 'bg-yellow-600 hover:bg-yellow-700' : ''}
          >
            {#if isProcessing}
              <span class="inline-block animate-spin mr-2">⏳</span>
            {/if}
            {confirmText}
          </Button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Ensure backdrop prevents interaction with underlying content */
  .fixed {
    touch-action: none;
  }
</style>