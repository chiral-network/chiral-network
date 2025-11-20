<!-- Command Palette - Quick Actions Menu -->
<script lang="ts">
  import { t } from 'svelte-i18n';
  import { 
    Download, Upload, Wallet, Globe, BarChart3, Settings, Cpu, 
    Star, Server, Database, Search, ArrowRight, Clock
  } from 'lucide-svelte';
  import { goto } from '@mateothegreat/svelte5-router';
  
  type Props = {
    isOpen: boolean;
    onClose: () => void;
  };
  
  let { isOpen, onClose }: Props = $props();
  
  type Action = {
    id: string;
    labelKey: string;
    icon: any;
    category: 'navigation' | 'recent';
    action: () => void;
    shortcut?: string;
  };
  
  let searchQuery = $state('');
  let selectedIndex = $state(0);
  let searchInput = $state<HTMLInputElement | undefined>(undefined);
  
  // Navigation actions with label keys instead of immediate translation
  const navigationActions: Action[] = [
    {
      id: 'nav:download',
      labelKey: 'commandPalette.actions.goToDownload',
      icon: Download,
      category: 'navigation',
      action: () => {
        goto('/download');
        onClose();
      },
      shortcut: 'Ctrl+D'
    },
    {
      id: 'nav:upload',
      labelKey: 'commandPalette.actions.goToUpload',
      icon: Upload,
      category: 'navigation',
      action: () => {
        goto('/upload');
        onClose();
      },
      shortcut: 'Ctrl+U'
    },
    {
      id: 'nav:network',
      labelKey: 'commandPalette.actions.goToNetwork',
      icon: Globe,
      category: 'navigation',
      action: () => {
        goto('/network');
        onClose();
      },
      shortcut: 'Ctrl+N'
    },
    {
      id: 'nav:mining',
      labelKey: 'commandPalette.actions.goToMining',
      icon: Cpu,
      category: 'navigation',
      action: () => {
        goto('/mining');
        onClose();
      },
      shortcut: 'Ctrl+M'
    },
    {
      id: 'nav:relay',
      labelKey: 'commandPalette.actions.goToRelay',
      icon: Server,
      category: 'navigation',
      action: () => {
        goto('/relay');
        onClose();
      }
    },
    {
      id: 'nav:analytics',
      labelKey: 'commandPalette.actions.goToAnalytics',
      icon: BarChart3,
      category: 'navigation',
      action: () => {
        goto('/analytics');
        onClose();
      }
    },
    {
      id: 'nav:reputation',
      labelKey: 'commandPalette.actions.goToReputation',
      icon: Star,
      category: 'navigation',
      action: () => {
        goto('/reputation');
        onClose();
      }
    },
    {
      id: 'nav:blockchain',
      labelKey: 'commandPalette.actions.goToBlockchain',
      icon: Database,
      category: 'navigation',
      action: () => {
        goto('/blockchain');
        onClose();
      }
    },
    {
      id: 'nav:account',
      labelKey: 'commandPalette.actions.goToAccount',
      icon: Wallet,
      category: 'navigation',
      action: () => {
        goto('/account');
        onClose();
      },
      shortcut: 'Ctrl+A'
    },
    {
      id: 'nav:settings',
      labelKey: 'commandPalette.actions.goToSettings',
      icon: Settings,
      category: 'navigation',
      action: () => {
        goto('/settings');
        onClose();
      },
      shortcut: 'Ctrl+,'
    }
  ];
  
  // Fuzzy search function
  function fuzzyMatch(query: string, text: string): boolean {
    if (!query) return true;
    
    const lowerQuery = query.toLowerCase();
    const lowerText = text.toLowerCase();
    
    // Simple fuzzy matching: check if all characters appear in order
    let queryIndex = 0;
    for (let i = 0; i < lowerText.length && queryIndex < lowerQuery.length; i++) {
      if (lowerText[i] === lowerQuery[queryIndex]) {
        queryIndex++;
      }
    }
    return queryIndex === lowerQuery.length;
  }
  
  // Filter actions based on search query
  $effect(() => {
    selectedIndex = 0; // Reset selection when query changes
  });
  
  const filteredActions = $derived(
    navigationActions.filter(action => 
      fuzzyMatch(searchQuery, $t(action.labelKey))
    )
  );
  
  // Handle keyboard navigation
  function handleKeydown(event: KeyboardEvent) {
    if (!isOpen) return;
    
    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        onClose();
        break;
      case 'ArrowDown':
        event.preventDefault();
        selectedIndex = Math.min(selectedIndex + 1, filteredActions.length - 1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        selectedIndex = Math.max(selectedIndex - 1, 0);
        break;
      case 'Enter':
        event.preventDefault();
        if (filteredActions[selectedIndex]) {
          filteredActions[selectedIndex].action();
        }
        break;
    }
  }
  
  // Focus search input when opened
  $effect(() => {
    if (isOpen && searchInput) {
      setTimeout(() => searchInput?.focus(), 50);
    }
  });
  
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
    class="fixed inset-0 bg-black/50 z-50 flex items-start justify-center pt-[15vh] p-4 backdrop-blur-sm"
    onclick={handleBackgroundClick}
    role="dialog"
    aria-modal="true"
    aria-labelledby="command-palette-title"
  >
    <!-- Modal Content -->
    <div class="bg-white dark:bg-gray-800 rounded-lg shadow-2xl max-w-2xl w-full overflow-hidden">
      <!-- Search Input -->
      <div class="p-4 border-b border-gray-200 dark:border-gray-700">
        <div class="relative">
          <Search class="absolute left-3 top-1/2 transform -translate-y-1/2 h-5 w-5 text-gray-400" />
          <input
            bind:this={searchInput}
            bind:value={searchQuery}
            type="text"
            placeholder={$t('commandPalette.searchPlaceholder')}
            class="w-full pl-10 pr-4 py-3 bg-gray-50 dark:bg-gray-900 border-0 rounded-lg text-gray-900 dark:text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
            aria-label={$t('commandPalette.searchLabel')}
          />
        </div>
      </div>
      
      <!-- Actions List -->
      <div class="max-h-[400px] overflow-y-auto">
        {#if filteredActions.length > 0}
          <div class="p-2">
            {#each filteredActions as action, index}
              <button
                onclick={() => action.action()}
                onmouseenter={() => selectedIndex = index}
                class="w-full flex items-center justify-between px-4 py-3 rounded-lg transition-colors {
                  selectedIndex === index 
                    ? 'bg-blue-50 dark:bg-blue-900/20' 
                    : 'hover:bg-gray-50 dark:hover:bg-gray-700/50'
                }"
              >
                <div class="flex items-center gap-3">
                  <div class="flex items-center justify-center w-8 h-8 rounded-lg bg-gray-100 dark:bg-gray-700 {
                    selectedIndex === index ? 'bg-blue-100 dark:bg-blue-800' : ''
                  }">
                    {#if action.icon}
                      <action.icon class="h-4 w-4 text-gray-600 dark:text-gray-300" />
                    {/if}
                  </div>
                  <span class="text-gray-900 dark:text-white font-medium">
                    {$t(action.labelKey)}
                  </span>
                </div>
                <div class="flex items-center gap-2">
                  {#if action.shortcut}
                    <span class="text-xs text-gray-500 dark:text-gray-400 px-2 py-1 bg-gray-100 dark:bg-gray-700 rounded">
                      {action.shortcut}
                    </span>
                  {/if}
                  {#if selectedIndex === index}
                    <ArrowRight class="h-4 w-4 text-blue-500" />
                  {/if}
                </div>
              </button>
            {/each}
          </div>
        {:else}
          <div class="p-8 text-center text-gray-500 dark:text-gray-400">
            <Search class="h-12 w-12 mx-auto mb-3 opacity-50" />
            <p class="font-medium">{$t('commandPalette.noResults')}</p>
            <p class="text-sm mt-1">{$t('commandPalette.tryDifferentQuery')}</p>
          </div>
        {/if}
      </div>
      
      <!-- Footer -->
      <div class="px-4 py-3 bg-gray-50 dark:bg-gray-900 border-t border-gray-200 dark:border-gray-700">
        <div class="flex items-center justify-between text-xs text-gray-500 dark:text-gray-400">
          <div class="flex items-center gap-4">
            <span class="flex items-center gap-1">
              <kbd class="px-1.5 py-0.5 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded text-xs">↑↓</kbd>
              {$t('commandPalette.navigate')}
            </span>
            <span class="flex items-center gap-1">
              <kbd class="px-1.5 py-0.5 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded text-xs">↵</kbd>
              {$t('commandPalette.select')}
            </span>
            <span class="flex items-center gap-1">
              <kbd class="px-1.5 py-0.5 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded text-xs">Esc</kbd>
              {$t('commandPalette.close')}
            </span>
          </div>
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
    animation: slideDown 0.2s ease-out;
  }
  
  @keyframes fadeIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
  
  @keyframes slideDown {
    from {
      transform: translateY(-20px);
      opacity: 0;
    }
    to {
      transform: translateY(0);
      opacity: 1;
    }
  }
  
  /* Custom scrollbar */
  .overflow-y-auto {
    scrollbar-width: thin;
    scrollbar-color: #cbd5e0 transparent;
  }
  
  .overflow-y-auto::-webkit-scrollbar {
    width: 8px;
  }
  
  .overflow-y-auto::-webkit-scrollbar-track {
    background: transparent;
  }
  
  .overflow-y-auto::-webkit-scrollbar-thumb {
    background: #cbd5e0;
    border-radius: 4px;
  }
  
  .overflow-y-auto::-webkit-scrollbar-thumb:hover {
    background: #a0aec0;
  }
</style>
