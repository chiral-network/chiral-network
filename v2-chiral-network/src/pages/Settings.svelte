<script lang="ts">
  import { onMount } from 'svelte';
  import { settings, isDarkMode, type ThemeMode, type NotificationSettings, type ColorTheme, type NavStyle } from '$lib/stores';
  import { availableThemes } from '$lib/services/colorThemeService';
  import { toasts } from '$lib/toastStore';
  import {
    Sun,
    Moon,
    Monitor,
    Palette,
    LayoutGrid,
    PanelTop,
    PanelLeft,
    RotateCcw,
    Check,
    FolderOpen,
    HardDrive,
    X,
    Bell
  } from 'lucide-svelte';

  let isTauri = $state(false);
  let displayDownloadDir = $state('');

  function checkTauriAvailability(): boolean {
    return typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);
  }

  onMount(async () => {
    isTauri = checkTauriAvailability();
    if (isTauri) {
      await loadDownloadDirectory();
    }
  });

  async function loadDownloadDirectory() {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      displayDownloadDir = await invoke<string>('get_download_directory');
    } catch (e) {
      console.error('Failed to load download directory:', e);
    }
  }

  async function browseDownloadDirectory() {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const picked = await invoke<string | null>('pick_download_directory');
      if (picked) {
        await invoke('set_download_directory', { path: picked });
        settings.update((s) => ({ ...s, downloadDirectory: picked }));
        displayDownloadDir = picked;
        toasts.show('Download directory updated', 'success');
      }
    } catch (e) {
      toasts.show(`Failed to set directory: ${e}`, 'error');
    }
  }

  async function resetDownloadDirectory() {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('set_download_directory', { path: null });
      settings.update((s) => ({ ...s, downloadDirectory: '' }));
      displayDownloadDir = await invoke<string>('get_download_directory');
      toasts.show('Download directory reset to system default', 'info');
    } catch (e) {
      toasts.show(`Failed to reset directory: ${e}`, 'error');
    }
  }

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

  function setColorTheme(color: ColorTheme) {
    settings.update((s) => ({ ...s, colorTheme: color }));
  }

  function toggleCompactMode() {
    settings.update((s) => ({ ...s, compactMode: !s.compactMode }));
  }

  function setNavStyle(style: NavStyle) {
    settings.update((s) => ({ ...s, navStyle: style }));
  }

  const navStyleOptions: { value: NavStyle; label: string; icon: typeof PanelTop }[] = [
    { value: 'navbar', label: 'Top Bar', icon: PanelTop },
    { value: 'sidebar', label: 'Sidebar', icon: PanelLeft }
  ];

  function toggleNotification(key: keyof NotificationSettings) {
    settings.update((s) => ({
      ...s,
      notifications: {
        ...s.notifications,
        [key]: !s.notifications[key]
      }
    }));
  }

  const notificationOptions: { key: keyof NotificationSettings; label: string; description: string }[] = [
    { key: 'downloadComplete', label: 'Download Complete', description: 'When a file finishes downloading' },
    { key: 'downloadFailed', label: 'Download Failed', description: 'When a file download fails' },
    { key: 'peerConnected', label: 'Peer Connected', description: 'When a new peer connects to you' },
    { key: 'peerDisconnected', label: 'Peer Disconnected', description: 'When a peer disconnects from you' },
    { key: 'miningBlock', label: 'Mining Block Found', description: 'When you mine a new block' },
    { key: 'paymentReceived', label: 'Payment Received', description: 'When you receive a CHR payment for a file' },
    { key: 'networkStatus', label: 'Network Status', description: 'Connection and disconnection events' },
    { key: 'fileShared', label: 'File Shared', description: 'When someone starts downloading your shared file' }
  ];

  function resetSettings() {
    settings.reset();
    if (isTauri) {
      resetDownloadDirectory();
    }
    toasts.show('Settings reset to defaults', 'info');
  }
</script>

<div class="p-6">
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
                ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30'
                : 'border-gray-200 dark:border-gray-600 hover:border-gray-300 dark:hover:border-gray-500 bg-gray-50 dark:bg-gray-700'}"
          >
            {#if $settings.theme === option.value}
              <div class="absolute top-2 right-2">
                <Check class="w-4 h-4 text-primary-500" />
              </div>
            {/if}
            <Icon class="w-6 h-6 {$settings.theme === option.value ? 'text-primary-500' : 'text-gray-500 dark:text-gray-400'}" />
            <span class="text-sm font-medium {$settings.theme === option.value ? 'text-primary-700 dark:text-primary-300' : 'text-gray-700 dark:text-gray-300'}">
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

    <!-- Accent Color -->
    <div class="mb-6">
      <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Accent Color</span>
      <div class="flex gap-3">
        {#each availableThemes as ct}
          <button
            onclick={() => setColorTheme(ct.value)}
            class="relative w-10 h-10 rounded-full transition-all
              {$settings.colorTheme === ct.value
                ? 'scale-110 ring-2 ring-offset-2 ring-gray-400 dark:ring-gray-500 dark:ring-offset-gray-800'
                : 'hover:scale-105'}"
            style="background-color: {ct.previewHex}"
            title={ct.label}
            aria-label="Set accent color to {ct.label}"
          >
            {#if $settings.colorTheme === ct.value}
              <Check class="w-5 h-5 text-white absolute inset-0 m-auto drop-shadow" />
            {/if}
          </button>
        {/each}
      </div>
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
          {$settings.compactMode ? 'bg-primary-500' : 'bg-gray-300 dark:bg-gray-600'}"
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

    <!-- Navigation Style -->
    <div class="py-4 border-t border-gray-200 dark:border-gray-700">
      <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Navigation Style</span>
      <div class="grid grid-cols-2 gap-3">
        {#each navStyleOptions as option}
          {@const Icon = option.icon}
          <button
            onclick={() => setNavStyle(option.value)}
            class="relative flex flex-col items-center gap-2 p-4 rounded-lg border-2 transition-all
              {$settings.navStyle === option.value
                ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30'
                : 'border-gray-200 dark:border-gray-600 hover:border-gray-300 dark:hover:border-gray-500 bg-gray-50 dark:bg-gray-700'}"
          >
            {#if $settings.navStyle === option.value}
              <div class="absolute top-2 right-2">
                <Check class="w-4 h-4 text-primary-500" />
              </div>
            {/if}
            <Icon class="w-6 h-6 {$settings.navStyle === option.value ? 'text-primary-500' : 'text-gray-500 dark:text-gray-400'}" />
            <span class="text-sm font-medium {$settings.navStyle === option.value ? 'text-primary-700 dark:text-primary-300' : 'text-gray-700 dark:text-gray-300'}">
              {option.label}
            </span>
          </button>
        {/each}
      </div>
    </div>

    <!-- Preview -->
    <div class="pt-4 border-t border-gray-200 dark:border-gray-700">
      <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Preview</span>
      <div class="p-4 rounded-lg bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-700">
        <div class="flex items-center gap-3 mb-3">
          <div class="w-10 h-10 rounded-full bg-primary-500"></div>
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
  </div>

  <!-- Storage Section -->
  {#if isTauri}
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6 mb-6">
      <div class="flex items-center gap-3 mb-6">
        <div class="p-2 bg-primary-100 dark:bg-primary-900 rounded-lg">
          <HardDrive class="w-5 h-5 text-primary-600 dark:text-primary-400" />
        </div>
        <div>
          <h2 class="font-semibold text-lg dark:text-white">Storage</h2>
          <p class="text-sm text-gray-500 dark:text-gray-400">Configure where downloaded files are saved</p>
        </div>
      </div>

      <!-- Download Directory -->
      <div>
        <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">Download Directory</span>
        <div class="flex items-center gap-3">
          <div class="flex-1 flex items-center gap-2 px-3 py-2.5 bg-gray-50 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg">
            <FolderOpen class="w-4 h-4 text-gray-400 flex-shrink-0" />
            <span class="text-sm text-gray-700 dark:text-gray-300 truncate font-mono">
              {displayDownloadDir || 'Loading...'}
            </span>
          </div>
          <button
            onclick={browseDownloadDirectory}
            class="px-4 py-2.5 bg-primary-600 text-white text-sm font-medium rounded-lg hover:bg-primary-700 transition-colors flex-shrink-0"
          >
            Browse
          </button>
          {#if $settings.downloadDirectory}
            <button
              onclick={resetDownloadDirectory}
              class="p-2.5 text-gray-400 hover:text-red-500 dark:hover:text-red-400 transition-colors flex-shrink-0"
              title="Reset to system default"
            >
              <X class="w-4 h-4" />
            </button>
          {/if}
        </div>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-2">
          {#if $settings.downloadDirectory}
            Using custom directory
          {:else}
            Using system default Downloads folder
          {/if}
        </p>
      </div>
    </div>
  {/if}

  <!-- Notification Settings Section -->
  <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6 mb-6">
    <div class="flex items-center gap-3 mb-4">
      <div class="p-2 bg-amber-100 dark:bg-amber-900 rounded-lg">
        <Bell class="w-5 h-5 text-amber-600 dark:text-amber-400" />
      </div>
      <div>
        <h2 class="font-semibold text-lg dark:text-white">Notifications</h2>
        <p class="text-sm text-gray-500 dark:text-gray-400">Toggle which toast notifications to show</p>
      </div>
    </div>

    <div class="grid grid-cols-2 gap-x-6 gap-y-2">
      {#each notificationOptions as option}
        <button
          onclick={() => toggleNotification(option.key)}
          class="flex items-center justify-between gap-3 py-2 px-3 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors group"
          role="switch"
          aria-checked={$settings.notifications?.[option.key] ?? true}
          title={option.description}
        >
          <span class="text-sm text-gray-700 dark:text-gray-300 text-left">{option.label}</span>
          <div class="relative w-9 h-5 rounded-full shrink-0 transition-colors
            {$settings.notifications?.[option.key] ? 'bg-primary-500' : 'bg-gray-300 dark:bg-gray-600'}">
            <span
              class="absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform
                {$settings.notifications?.[option.key] ? 'translate-x-4' : 'translate-x-0'}"
            ></span>
          </div>
        </button>
      {/each}
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
