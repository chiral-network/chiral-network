<script lang="ts">
  import { settings, isDarkMode, type ThemeMode } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import {
    Sun,
    Moon,
    Monitor,
    Palette,
    Zap,
    LayoutGrid,
    RotateCcw,
    Check
  } from 'lucide-svelte';

  // Theme options
  const themeOptions: { value: ThemeMode; label: string; icon: typeof Sun }[] = [
    { value: 'light', label: 'Light', icon: Sun },
    { value: 'dark', label: 'Dark', icon: Moon },
    { value: 'system', label: 'System', icon: Monitor }
  ];

  function setTheme(theme: ThemeMode) {
    settings.update((s) => ({ ...s, theme }));
    toasts.show(`Theme set to ${theme}`, 'success');
  }

  function toggleReducedMotion() {
    settings.update((s) => ({ ...s, reducedMotion: !s.reducedMotion }));
  }

  function toggleCompactMode() {
    settings.update((s) => ({ ...s, compactMode: !s.compactMode }));
  }

  function resetSettings() {
    settings.reset();
    toasts.show('Settings reset to defaults', 'info');
  }
</script>

<div class="p-6 max-w-4xl mx-auto">
  <div class="mb-8">
    <h1 class="text-3xl font-bold dark:text-white">Settings</h1>
    <p class="text-gray-600 dark:text-gray-400 mt-1">Customize your Chiral Network experience</p>
  </div>

  <!-- Appearance Section -->
  <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6 mb-6">
    <div class="flex items-center gap-3 mb-6">
      <div class="p-2 bg-purple-100 dark:bg-purple-900 rounded-lg">
        <Palette class="w-5 h-5 text-purple-600 dark:text-purple-400" />
      </div>
      <div>
        <h2 class="font-semibold text-lg dark:text-white">Appearance</h2>
        <p class="text-sm text-gray-500 dark:text-gray-400">Customize how the app looks</p>
      </div>
    </div>

    <!-- Theme Selection -->
    <div class="mb-6">
      <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Theme</span>
      <div class="grid grid-cols-3 gap-3">
        {#each themeOptions as option}
          {@const Icon = option.icon}
          <button
            onclick={() => setTheme(option.value)}
            class="relative flex flex-col items-center gap-2 p-4 rounded-lg border-2 transition-all
              {$settings.theme === option.value
                ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/30'
                : 'border-gray-200 dark:border-gray-600 hover:border-gray-300 dark:hover:border-gray-500 bg-gray-50 dark:bg-gray-700'}"
          >
            {#if $settings.theme === option.value}
              <div class="absolute top-2 right-2">
                <Check class="w-4 h-4 text-blue-500" />
              </div>
            {/if}
            <Icon class="w-6 h-6 {$settings.theme === option.value ? 'text-blue-500' : 'text-gray-500 dark:text-gray-400'}" />
            <span class="text-sm font-medium {$settings.theme === option.value ? 'text-blue-700 dark:text-blue-300' : 'text-gray-700 dark:text-gray-300'}">
              {option.label}
            </span>
          </button>
        {/each}
      </div>
      <p class="text-xs text-gray-500 dark:text-gray-400 mt-2">
        {#if $settings.theme === 'system'}
          Currently using {$isDarkMode ? 'dark' : 'light'} mode based on your system preference
        {:else}
          Using {$settings.theme} mode
        {/if}
      </p>
    </div>

    <!-- Reduced Motion -->
    <div class="flex items-center justify-between py-4 border-t border-gray-200 dark:border-gray-700">
      <div class="flex items-center gap-3">
        <Zap class="w-5 h-5 text-gray-500 dark:text-gray-400" />
        <div>
          <p class="font-medium text-gray-900 dark:text-white">Reduced Motion</p>
          <p class="text-sm text-gray-500 dark:text-gray-400">Minimize animations throughout the app</p>
        </div>
      </div>
      <button
        onclick={toggleReducedMotion}
        class="relative w-12 h-6 rounded-full transition-colors
          {$settings.reducedMotion ? 'bg-blue-500' : 'bg-gray-300 dark:bg-gray-600'}"
        role="switch"
        aria-checked={$settings.reducedMotion}
        aria-label="Toggle reduced motion"
      >
        <span
          class="absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform
            {$settings.reducedMotion ? 'translate-x-6' : 'translate-x-0'}"
        ></span>
      </button>
    </div>

    <!-- Compact Mode -->
    <div class="flex items-center justify-between py-4 border-t border-gray-200 dark:border-gray-700">
      <div class="flex items-center gap-3">
        <LayoutGrid class="w-5 h-5 text-gray-500 dark:text-gray-400" />
        <div>
          <p class="font-medium text-gray-900 dark:text-white">Compact Mode</p>
          <p class="text-sm text-gray-500 dark:text-gray-400">Use smaller spacing and font sizes</p>
        </div>
      </div>
      <button
        onclick={toggleCompactMode}
        class="relative w-12 h-6 rounded-full transition-colors
          {$settings.compactMode ? 'bg-blue-500' : 'bg-gray-300 dark:bg-gray-600'}"
        role="switch"
        aria-checked={$settings.compactMode}
        aria-label="Toggle compact mode"
      >
        <span
          class="absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform
            {$settings.compactMode ? 'translate-x-6' : 'translate-x-0'}"
        ></span>
      </button>
    </div>
  </div>

  <!-- Preview Section -->
  <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6 mb-6">
    <h3 class="font-semibold text-lg dark:text-white mb-4">Preview</h3>
    <div class="p-4 rounded-lg bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-700">
      <div class="flex items-center gap-3 mb-3">
        <div class="w-10 h-10 rounded-full bg-gradient-to-br from-blue-500 to-purple-600"></div>
        <div>
          <p class="font-medium text-gray-900 dark:text-white">Sample User</p>
          <p class="text-sm text-gray-500 dark:text-gray-400">0x1234...5678</p>
        </div>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="p-3 rounded-lg bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700">
          <p class="text-xs text-gray-500 dark:text-gray-400">Balance</p>
          <p class="text-lg font-bold text-gray-900 dark:text-white">100.00 CHR</p>
        </div>
        <div class="p-3 rounded-lg bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700">
          <p class="text-xs text-gray-500 dark:text-gray-400">Peers</p>
          <p class="text-lg font-bold text-gray-900 dark:text-white">12</p>
        </div>
      </div>
    </div>
  </div>

  <!-- Reset Section -->
  <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
    <div class="flex items-center justify-between">
      <div>
        <h3 class="font-semibold text-gray-900 dark:text-white">Reset Settings</h3>
        <p class="text-sm text-gray-500 dark:text-gray-400">Restore all settings to their default values</p>
      </div>
      <button
        onclick={resetSettings}
        class="flex items-center gap-2 px-4 py-2 text-red-600 dark:text-red-400 border border-red-200 dark:border-red-800 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/30 transition-colors"
      >
        <RotateCcw class="w-4 h-4" />
        Reset
      </button>
    </div>
  </div>
</div>
