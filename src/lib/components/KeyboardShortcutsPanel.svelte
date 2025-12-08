<!-- Keyboard Shortcuts Help Panel -->
<script lang="ts">
  import { t } from 'svelte-i18n';
  import { X } from 'lucide-svelte';
  
  type Props = {
    isOpen: boolean;
    onClose: () => void;
  };
  
  let { isOpen, onClose }: Props = $props();
  
  // Keyboard shortcut categories
  const shortcuts = [
    {
      category: 'shortcuts.categories.navigation',
      items: [
        { keys: ['Ctrl', 'D'], description: 'shortcuts.navigation.download' },
        { keys: ['Ctrl', 'U'], description: 'shortcuts.navigation.upload' },
        { keys: ['Ctrl', 'N'], description: 'shortcuts.navigation.network' },
        { keys: ['Ctrl', 'M'], description: 'shortcuts.navigation.mining' },
        { keys: ['Ctrl', 'A'], description: 'shortcuts.navigation.account' },
        { keys: ['Ctrl', ','], description: 'shortcuts.navigation.settings' },
      ]
    },
    {
      category: 'shortcuts.categories.application',
      items: [
        { keys: ['Ctrl', 'K'], description: 'shortcuts.application.commandPalette' },
        { keys: ['?'], description: 'shortcuts.application.showHelp' },
        { keys: ['F1'], description: 'shortcuts.application.showHelp' },
        { keys: ['Esc'], description: 'shortcuts.application.closeModal' },
        { keys: ['Ctrl', 'R'], description: 'shortcuts.application.refresh' },
        { keys: ['F5'], description: 'shortcuts.application.reload' },
      ]
    },
    {
      category: 'shortcuts.categories.window',
      items: [
        { keys: ['F11'], description: 'shortcuts.window.fullscreen' },
        { keys: ['Ctrl', 'Q'], description: 'shortcuts.window.quit' },
      ]
    }
  ];
  
  // Handle escape key to close
  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && isOpen) {
      onClose();
    }
  }
  
  // Close on background click
  function handleBackgroundClick(event: MouseEvent) {
    if (event.target === event.currentTarget) {
      onClose();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

{#if isOpen}
  <!-- Modal Overlay -->
  <div 
    class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center p-4 backdrop-blur-sm"
    onclick={handleBackgroundClick}
    onkeydown={(e) => e.key === 'Escape' && (isOpen = false)}
    role="dialog"
    aria-modal="true"
    aria-labelledby="shortcuts-title"
    tabindex="-1"
  >
    <!-- Modal Content -->
    <div class="bg-white dark:bg-gray-800 rounded-lg shadow-2xl max-w-2xl w-full max-h-[80vh] overflow-y-auto">
      <!-- Header -->
      <div class="sticky top-0 bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 px-6 py-4 flex items-center justify-between">
        <h2 id="shortcuts-title" class="text-xl font-bold text-gray-900 dark:text-white">
          {$t('shortcuts.title')}
        </h2>
        <button
          onclick={onClose}
          class="p-2 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
          aria-label={$t('shortcuts.close')}
        >
          <X class="h-5 w-5" />
        </button>
      </div>
      
      <!-- Shortcuts List -->
      <div class="p-6 space-y-8">
        {#each shortcuts as section}
          <div>
            <h3 class="text-sm font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wide mb-3">
              {$t(section.category)}
            </h3>
            <div class="space-y-2">
              {#each section.items as shortcut}
                <div class="flex items-center justify-between py-2 px-3 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors">
                  <span class="text-gray-700 dark:text-gray-300">
                    {$t(shortcut.description)}
                  </span>
                  <div class="flex items-center gap-1">
                    {#each shortcut.keys as key, index}
                      {#if index > 0}
                        <span class="text-gray-400 mx-1">+</span>
                      {/if}
                      <kbd class="px-2.5 py-1.5 text-sm font-semibold text-gray-800 dark:text-gray-200 bg-gray-100 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-md shadow-sm min-w-[2.5rem] text-center">
                        {key}
                      </kbd>
                    {/each}
                  </div>
                </div>
              {/each}
            </div>
          </div>
        {/each}
        
        <!-- Footer Tip -->
        <div class="pt-4 border-t border-gray-200 dark:border-gray-700">
          <p class="text-sm text-gray-500 dark:text-gray-400 text-center">
            {$t('shortcuts.tip')}
          </p>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Smooth animations */
  div[role="dialog"] {
    animation: fadeIn 0.15s ease-out;
  }
  
  div[role="dialog"] > div {
    animation: slideUp 0.2s ease-out;
  }
  
  @keyframes fadeIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
  
  @keyframes slideUp {
    from {
      transform: translateY(20px);
      opacity: 0;
    }
    to {
      transform: translateY(0);
      opacity: 1;
    }
  }
  
  /* Custom scrollbar */
  div[role="dialog"] > div {
    scrollbar-width: thin;
    scrollbar-color: #cbd5e0 transparent;
  }
  
  div[role="dialog"] > div::-webkit-scrollbar {
    width: 8px;
  }
  
  div[role="dialog"] > div::-webkit-scrollbar-track {
    background: transparent;
  }
  
  div[role="dialog"] > div::-webkit-scrollbar-thumb {
    background: #cbd5e0;
    border-radius: 4px;
  }
  
  div[role="dialog"] > div::-webkit-scrollbar-thumb:hover {
    background: #a0aec0;
  }
</style>
