<script lang="ts">
  import { onMount } from 'svelte';
  import {
    settings,
    isDarkMode,
    type ThemeMode,
    type NotificationSettings,
    type ColorTheme,
    type NavStyle,
  } from '$lib/stores';
  import { availableThemes } from '$lib/services/colorThemeService';
  import { toasts } from '$lib/toastStore';
  import {
    Sun,
    Moon,
    Monitor,
    Palette,
    Pickaxe,
    PanelTop,
    PanelLeft,
    RotateCcw,
    Check,
    FolderOpen,
    HardDrive,
    X,
    Bell,
    Network as NetworkIcon,
    Search,
    Sliders,
    AlertTriangle,
  } from 'lucide-svelte';

  type SectionId = 'appearance' | 'notifications' | 'network' | 'storage' | 'startup' | 'advanced';

  // ---------- State ----------

  let isTauri = $state(false);
  let displayDownloadDir = $state('');
  let activeSection = $state<SectionId>('appearance');
  let sidebarSearch = $state('');
  let resetConfirmOpen = $state(false);

  type NetworkInfo = { name: string; displayName: string; chainId: number };
  let activeNetwork = $state<NetworkInfo | null>(null);
  let availableNetworks = $state<NetworkInfo[]>([]);
  let pendingNetwork = $state<string | null>(null);

  function checkTauriAvailability(): boolean {
    return typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);
  }

  onMount(async () => {
    isTauri = checkTauriAvailability();
    if (isTauri) {
      await loadDownloadDirectory();
      await loadNetworks();
    }
  });

  // ---------- Network ----------

  async function loadNetworks() {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      activeNetwork = await invoke<NetworkInfo>('get_active_network');
      availableNetworks = await invoke<NetworkInfo[]>('list_networks');
    } catch (e) {
      console.error('Failed to load networks:', e);
    }
  }

  async function selectNetwork(name: string) {
    if (!activeNetwork || name === activeNetwork.name) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('set_active_network', { name });
      pendingNetwork = name;
      toasts.detail(
        'Network change pending',
        'Restart the app to finish switching networks. Chain state, DHT identity, and wallet history will swap together.',
        'info',
        8000,
      );
    } catch (e) {
      toasts.detail('Failed to change network', String(e), 'error');
    }
  }

  // ---------- Storage ----------

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
      }
    } catch (e) {
      toasts.detail('Failed to set directory', String(e), 'error');
    }
  }

  async function resetDownloadDirectory() {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('set_download_directory', { path: null });
      settings.update((s) => ({ ...s, downloadDirectory: '' }));
      displayDownloadDir = await invoke<string>('get_download_directory');
    } catch (e) {
      toasts.detail('Failed to reset directory', String(e), 'error');
    }
  }

  // ---------- Appearance ----------

  const themeOptions: { value: ThemeMode; label: string; icon: typeof Sun }[] = [
    { value: 'light', label: 'Light', icon: Sun },
    { value: 'dark', label: 'Dark', icon: Moon },
    { value: 'system', label: 'System', icon: Monitor },
  ];

  function setTheme(theme: ThemeMode) {
    settings.update((s) => ({ ...s, theme }));
  }

  function setColorTheme(color: ColorTheme) {
    settings.update((s) => ({ ...s, colorTheme: color }));
  }

  function setNavStyle(style: NavStyle) {
    settings.update((s) => ({ ...s, navStyle: style }));
  }

  const navStyleOptions: { value: NavStyle; label: string; icon: typeof PanelTop }[] = [
    { value: 'navbar', label: 'Top Bar', icon: PanelTop },
    { value: 'sidebar', label: 'Sidebar', icon: PanelLeft },
  ];

  // ---------- Startup ----------

  function toggleAutoStartMining() {
    settings.update((s) => ({ ...s, autoStartMining: !s.autoStartMining }));
  }

  // ---------- Notifications ----------

  function toggleNotification(key: keyof NotificationSettings) {
    settings.update((s) => ({
      ...s,
      notifications: {
        ...s.notifications,
        [key]: !s.notifications[key],
      },
    }));
  }

  function setAllNotifications(value: boolean) {
    settings.update((s) => {
      const next = { ...s.notifications };
      for (const opt of notificationOptions) (next as any)[opt.key] = value;
      return { ...s, notifications: next };
    });
    toasts.show(value ? 'All notifications on' : 'All notifications off', 'success');
  }

  const notificationOptions: { key: keyof NotificationSettings; label: string; description: string }[] = [
    { key: 'downloadComplete', label: 'Download Complete', description: 'When a file finishes downloading' },
    { key: 'downloadFailed', label: 'Download Failed', description: 'When a file download fails' },
    { key: 'peerConnected', label: 'Peer Connected', description: 'When a new peer connects to you' },
    { key: 'peerDisconnected', label: 'Peer Disconnected', description: 'When a peer disconnects from you' },
    { key: 'miningBlock', label: 'Mining Block Found', description: 'When you mine a new block' },
    { key: 'paymentReceived', label: 'Payment Received', description: 'When you receive a CHI payment for a file' },
    { key: 'networkStatus', label: 'Network Status', description: 'Connection and disconnection events' },
    { key: 'fileShared', label: 'File Shared', description: 'When someone starts downloading your shared file' },
  ];

  let enabledNotificationCount = $derived(
    notificationOptions.filter((o) => $settings.notifications?.[o.key]).length
  );

  // ---------- Reset ----------

  function resetSettings() {
    settings.reset();
    if (isTauri) resetDownloadDirectory();
    resetConfirmOpen = false;
    toasts.show('Settings reset to defaults', 'success');
  }

  // ---------- Sidebar ----------

  type Section = {
    id: SectionId;
    label: string;
    icon: typeof Sun;
    keywords: string[];
    showOnlyOnTauri?: boolean;
  };

  const allSections: Section[] = [
    {
      id: 'appearance',
      label: 'Appearance',
      icon: Palette,
      keywords: ['theme', 'dark', 'light', 'color', 'accent', 'nav', 'sidebar', 'top bar'],
    },
    {
      id: 'notifications',
      label: 'Notifications',
      icon: Bell,
      keywords: ['toast', 'alert', 'download', 'peer', 'mining', 'payment'],
    },
    {
      id: 'network',
      label: 'Network',
      icon: NetworkIcon,
      keywords: ['chain', 'mainnet', 'testnet', 'freshnet', 'switch'],
      showOnlyOnTauri: true,
    },
    {
      id: 'storage',
      label: 'Storage',
      icon: HardDrive,
      keywords: ['download', 'directory', 'folder', 'path'],
      showOnlyOnTauri: true,
    },
    {
      id: 'startup',
      label: 'Startup',
      icon: Pickaxe,
      keywords: ['auto', 'mining', 'login', 'launch'],
    },
    {
      id: 'advanced',
      label: 'Advanced',
      icon: Sliders,
      keywords: ['reset', 'defaults'],
    },
  ];

  let visibleSections = $derived.by(() => {
    const q = sidebarSearch.trim().toLowerCase();
    return allSections.filter((s) => {
      if (s.showOnlyOnTauri && !isTauri) return false;
      if (!q) return true;
      return (
        s.label.toLowerCase().includes(q) || s.keywords.some((k) => k.toLowerCase().includes(q))
      );
    });
  });

  // If search hides the active section, jump to the first visible one.
  $effect(() => {
    if (visibleSections.length === 0) return;
    if (!visibleSections.some((s) => s.id === activeSection)) {
      activeSection = visibleSections[0].id;
    }
  });
</script>

<svelte:head><title>Settings | Chiral Network</title></svelte:head>

<div class="p-4 sm:p-6 max-w-[1400px] mx-auto">
  <div class="mb-4">
    <h1 class="text-2xl font-bold text-gray-900 dark:text-white">Settings</h1>
    <p class="text-sm text-gray-500 dark:text-gray-400 mt-0.5">
      Customize the app's appearance, notifications, network, and behavior.
    </p>
  </div>

  <div class="grid grid-cols-1 lg:grid-cols-[260px_1fr] gap-6">
    <!-- Sidebar -->
    <aside class="lg:sticky lg:top-4 lg:self-start space-y-2">
      <div class="relative">
        <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
        <input
          type="search"
          bind:value={sidebarSearch}
          placeholder="Search settings…"
          class="w-full pl-9 pr-3 py-2 text-sm bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg text-gray-900 dark:text-white placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-primary-500/30 focus:border-primary-500"
        />
      </div>

      <nav
        class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-2 lg:flex lg:flex-col gap-1 flex flex-row overflow-x-auto"
      >
        {#each visibleSections as s (s.id)}
          {@const Icon = s.icon}
          <button
            onclick={() => (activeSection = s.id)}
            class="flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors text-left whitespace-nowrap shrink-0
              {activeSection === s.id
                ? 'bg-primary-100 dark:bg-primary-900/40 text-primary-700 dark:text-primary-300'
                : 'text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700/60'}"
          >
            <Icon class="w-4 h-4 shrink-0" />
            <span>{s.label}</span>
          </button>
        {/each}
        {#if visibleSections.length === 0}
          <p class="text-xs text-gray-500 dark:text-gray-400 p-3">No matches</p>
        {/if}
      </nav>
    </aside>

    <!-- Content -->
    <div class="min-w-0">
      {#if activeSection === 'appearance'}
        <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
          <header class="flex items-center gap-3 mb-6">
            <div class="p-2 bg-purple-100 dark:bg-purple-900/40 rounded-lg">
              <Palette class="w-5 h-5 text-purple-600 dark:text-purple-400" />
            </div>
            <div>
              <h2 class="font-semibold text-lg dark:text-white">Appearance</h2>
              <p class="text-sm text-gray-500 dark:text-gray-400">Theme, accent color, and navigation layout.</p>
            </div>
          </header>

          <!-- Theme -->
          <div class="mb-6">
            <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Theme</span>
            <div class="grid grid-cols-3 gap-3">
              {#each themeOptions as option (option.value)}
                {@const Icon = option.icon}
                <button
                  onclick={() => setTheme(option.value)}
                  class="relative flex flex-col items-center gap-2 p-4 rounded-lg border-2 transition-all
                    {$settings.theme === option.value
                      ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30'
                      : 'border-gray-200 dark:border-gray-600 hover:border-gray-300 dark:hover:border-gray-500 bg-gray-50 dark:bg-gray-700/30'}"
                >
                  {#if $settings.theme === option.value}
                    <div class="absolute top-2 right-2"><Check class="w-4 h-4 text-primary-500" /></div>
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

          <!-- Accent color -->
          <div class="mb-6">
            <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Accent color</span>
            <div class="flex flex-wrap gap-3">
              {#each availableThemes as ct (ct.value)}
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

          <!-- Nav style -->
          <div class="mb-6">
            <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Navigation layout</span>
            <div class="grid grid-cols-2 gap-3 max-w-md">
              {#each navStyleOptions as option (option.value)}
                {@const Icon = option.icon}
                <button
                  onclick={() => setNavStyle(option.value)}
                  class="relative flex flex-col items-center gap-2 p-4 rounded-lg border-2 transition-all
                    {$settings.navStyle === option.value
                      ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30'
                      : 'border-gray-200 dark:border-gray-600 hover:border-gray-300 dark:hover:border-gray-500 bg-gray-50 dark:bg-gray-700/30'}"
                >
                  {#if $settings.navStyle === option.value}
                    <div class="absolute top-2 right-2"><Check class="w-4 h-4 text-primary-500" /></div>
                  {/if}
                  <Icon class="w-6 h-6 {$settings.navStyle === option.value ? 'text-primary-500' : 'text-gray-500 dark:text-gray-400'}" />
                  <span class="text-sm font-medium {$settings.navStyle === option.value ? 'text-primary-700 dark:text-primary-300' : 'text-gray-700 dark:text-gray-300'}">
                    {option.label}
                  </span>
                </button>
              {/each}
            </div>
          </div>

          <!-- Compact preview -->
          <div class="pt-4 border-t border-gray-200 dark:border-gray-700">
            <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Preview</span>
            <div class="flex items-center gap-4 p-3 rounded-lg bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-700">
              <div class="w-10 h-10 rounded-full bg-primary-500 shrink-0"></div>
              <div class="flex-1 min-w-0">
                <p class="font-medium text-gray-900 dark:text-white">Sample User</p>
                <p class="text-xs text-gray-500 dark:text-gray-400 font-mono">0x1234…5678</p>
              </div>
              <div class="text-right shrink-0">
                <p class="text-xs text-gray-500 dark:text-gray-400">Balance</p>
                <p class="text-sm font-bold text-gray-900 dark:text-white tabular-nums">100.00 CHI</p>
              </div>
            </div>
          </div>
        </section>

      {:else if activeSection === 'notifications'}
        <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
          <header class="flex items-center justify-between gap-3 mb-6">
            <div class="flex items-center gap-3">
              <div class="p-2 bg-amber-100 dark:bg-amber-900/40 rounded-lg">
                <Bell class="w-5 h-5 text-amber-600 dark:text-amber-400" />
              </div>
              <div>
                <h2 class="font-semibold text-lg dark:text-white">Notifications</h2>
                <p class="text-sm text-gray-500 dark:text-gray-400">
                  {enabledNotificationCount} of {notificationOptions.length} enabled
                </p>
              </div>
            </div>
            <div class="flex items-center gap-1">
              <button
                onclick={() => setAllNotifications(true)}
                class="text-xs px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300"
              >
                All on
              </button>
              <button
                onclick={() => setAllNotifications(false)}
                class="text-xs px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300"
              >
                All off
              </button>
            </div>
          </header>

          <div class="grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-1">
            {#each notificationOptions as option (option.key)}
              {@const enabled = $settings.notifications?.[option.key] ?? true}
              <button
                onclick={() => toggleNotification(option.key)}
                class="flex items-center justify-between gap-3 py-3 px-3 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors text-left"
                role="switch"
                aria-checked={enabled}
                title={option.description}
              >
                <div class="min-w-0 flex-1">
                  <span class="block text-sm font-medium text-gray-900 dark:text-white">{option.label}</span>
                  <span class="block text-xs text-gray-500 dark:text-gray-400 mt-0.5 truncate">{option.description}</span>
                </div>
                <div
                  class="relative w-9 h-5 rounded-full shrink-0 transition-colors
                    {enabled ? 'bg-primary-500' : 'bg-gray-300 dark:bg-gray-600'}"
                >
                  <span
                    class="absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform
                      {enabled ? 'translate-x-4' : 'translate-x-0'}"
                  ></span>
                </div>
              </button>
            {/each}
          </div>
        </section>

      {:else if activeSection === 'network' && isTauri && activeNetwork}
        <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
          <header class="flex items-center gap-3 mb-6">
            <div class="p-2 bg-blue-100 dark:bg-blue-900/40 rounded-lg">
              <NetworkIcon class="w-5 h-5 text-blue-600 dark:text-blue-400" />
            </div>
            <div>
              <h2 class="font-semibold text-lg dark:text-white">Network</h2>
              <p class="text-sm text-gray-500 dark:text-gray-400">Choose which Chiral Network to connect to.</p>
            </div>
          </header>

          {#if pendingNetwork}
            <div class="mb-4 flex items-start gap-3 p-3 rounded-lg bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800">
              <AlertTriangle class="w-4 h-4 text-amber-600 dark:text-amber-400 shrink-0 mt-0.5" />
              <p class="text-sm text-amber-800 dark:text-amber-200">
                Network change pending — restart the app to finish switching to <strong>{pendingNetwork}</strong>.
              </p>
            </div>
          {/if}

          <div class="space-y-2">
            {#each availableNetworks as net (net.name)}
              {@const isActive = activeNetwork.name === net.name}
              {@const isPending = pendingNetwork === net.name}
              <button
                onclick={() => selectNetwork(net.name)}
                disabled={isActive && !pendingNetwork}
                class="w-full flex items-center justify-between p-4 rounded-lg border-2 transition-all text-left
                  {isActive
                    ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30'
                    : 'border-gray-200 dark:border-gray-600 hover:border-gray-300 dark:hover:border-gray-500 bg-gray-50 dark:bg-gray-700/30'}"
              >
                <div>
                  <div class="font-medium {isActive ? 'text-primary-700 dark:text-primary-300' : 'text-gray-800 dark:text-gray-200'}">
                    {net.displayName}
                  </div>
                  <div class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                    Chain ID {net.chainId} · {net.name}
                  </div>
                </div>
                <div class="flex items-center gap-2">
                  {#if isPending}
                    <span class="text-xs text-amber-600 dark:text-amber-400 font-medium">Restart required</span>
                  {:else if isActive}
                    <span class="text-xs text-primary-600 dark:text-primary-400 font-medium">Active</span>
                    <Check class="w-4 h-4 text-primary-500" />
                  {/if}
                </div>
              </button>
            {/each}
          </div>

          <p class="text-xs text-gray-500 dark:text-gray-400 mt-4">
            Geth chain state, DHT identity, wallet transaction history, and Drive files are kept separate per
            network — testnet and mainnet never mix.
          </p>
        </section>

      {:else if activeSection === 'storage' && isTauri}
        <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
          <header class="flex items-center gap-3 mb-6">
            <div class="p-2 bg-primary-100 dark:bg-primary-900/40 rounded-lg">
              <HardDrive class="w-5 h-5 text-primary-600 dark:text-primary-400" />
            </div>
            <div>
              <h2 class="font-semibold text-lg dark:text-white">Storage</h2>
              <p class="text-sm text-gray-500 dark:text-gray-400">Where downloaded files are saved on your computer.</p>
            </div>
          </header>

          <div>
            <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">Download directory</span>
            <div class="flex items-center gap-2">
              <button
                class="flex-1 flex items-center gap-2 px-3 py-2.5 bg-gray-50 dark:bg-gray-700/50 border border-gray-200 dark:border-gray-600 rounded-lg hover:border-primary-400 dark:hover:border-primary-500 transition-colors cursor-pointer text-left"
                title="Click to copy path"
                onclick={() => {
                  if (displayDownloadDir) {
                    navigator.clipboard.writeText(displayDownloadDir);
                    toasts.show('Path copied', 'success');
                  }
                }}
              >
                <FolderOpen class="w-4 h-4 text-gray-400 shrink-0" />
                <span class="text-sm text-gray-700 dark:text-gray-300 truncate font-mono">
                  {displayDownloadDir || 'Loading…'}
                </span>
              </button>
              <button
                onclick={browseDownloadDirectory}
                class="px-4 py-2.5 bg-primary-600 text-white text-sm font-medium rounded-lg hover:bg-primary-700 transition-colors shrink-0 focus:outline-none focus:ring-2 focus:ring-primary-500/30"
              >
                Browse
              </button>
              {#if $settings.downloadDirectory}
                <button
                  onclick={resetDownloadDirectory}
                  class="p-2.5 text-gray-400 hover:text-red-500 dark:hover:text-red-400 transition-colors shrink-0"
                  title="Reset to system default"
                >
                  <X class="w-4 h-4" />
                </button>
              {/if}
            </div>
            <p class="text-xs text-gray-500 dark:text-gray-400 mt-2">
              {#if $settings.downloadDirectory}
                Custom directory — click the directory pill to copy the path, or × to reset to system default.
              {:else}
                Using the system Downloads folder. Click <strong>Browse</strong> to pick a different one.
              {/if}
            </p>
          </div>
        </section>

      {:else if activeSection === 'startup'}
        <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
          <header class="flex items-center gap-3 mb-6">
            <div class="p-2 bg-emerald-100 dark:bg-emerald-900/40 rounded-lg">
              <Pickaxe class="w-5 h-5 text-emerald-600 dark:text-emerald-400" />
            </div>
            <div>
              <h2 class="font-semibold text-lg dark:text-white">Startup</h2>
              <p class="text-sm text-gray-500 dark:text-gray-400">What runs automatically when the app launches.</p>
            </div>
          </header>

          <button
            onclick={toggleAutoStartMining}
            class="w-full flex items-center justify-between gap-4 py-3 px-4 rounded-xl border border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors text-left"
            role="switch"
            aria-checked={$settings.autoStartMining}
          >
            <div>
              <p class="text-sm font-medium text-gray-900 dark:text-white">Auto-start mining on login</p>
              <p class="text-sm text-gray-500 dark:text-gray-400">
                Begins mining as soon as your node is online after login. Default: off.
              </p>
            </div>
            <div
              class="relative w-11 h-6 rounded-full shrink-0 transition-colors
                {$settings.autoStartMining ? 'bg-primary-500' : 'bg-gray-300 dark:bg-gray-600'}"
            >
              <span
                class="absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform
                  {$settings.autoStartMining ? 'translate-x-5' : 'translate-x-0'}"
              ></span>
            </div>
          </button>
        </section>

      {:else if activeSection === 'advanced'}
        <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
          <header class="flex items-center gap-3 mb-6">
            <div class="p-2 bg-gray-100 dark:bg-gray-700 rounded-lg">
              <Sliders class="w-5 h-5 text-gray-600 dark:text-gray-300" />
            </div>
            <div>
              <h2 class="font-semibold text-lg dark:text-white">Advanced</h2>
              <p class="text-sm text-gray-500 dark:text-gray-400">Destructive operations live here. Read carefully.</p>
            </div>
          </header>

          <div class="rounded-xl border border-red-200 dark:border-red-900/50 bg-red-50/50 dark:bg-red-900/10 p-4">
            <div class="flex items-start gap-3 mb-3">
              <AlertTriangle class="w-5 h-5 text-red-600 dark:text-red-400 shrink-0 mt-0.5" />
              <div>
                <h3 class="font-semibold text-red-900 dark:text-red-200">Reset all settings</h3>
                <p class="text-sm text-red-700/80 dark:text-red-300/80 mt-1">
                  Restores theme, accent color, navigation layout, notification toggles, startup behavior, and the
                  custom download directory to defaults. Your wallet, files, and DHT identity are <em>not</em> touched.
                </p>
              </div>
            </div>
            <button
              onclick={() => (resetConfirmOpen = true)}
              class="flex items-center gap-2 px-4 py-2 text-sm font-medium text-red-700 dark:text-red-300 border border-red-300 dark:border-red-800 rounded-lg hover:bg-red-100 dark:hover:bg-red-900/30 transition-colors"
            >
              <RotateCcw class="w-4 h-4" />
              Reset settings to defaults…
            </button>
          </div>
        </section>
      {/if}
    </div>
  </div>
</div>

<!-- Reset confirmation modal -->
{#if resetConfirmOpen}
  <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 bg-black/60 z-50 flex items-center justify-center p-4"
    onclick={(e) => {
      if (e.target === e.currentTarget) resetConfirmOpen = false;
    }}
  >
    <div class="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl max-w-md w-full p-6">
      <div class="flex items-start gap-3 mb-4">
        <div class="p-2 bg-red-100 dark:bg-red-900/30 rounded-lg shrink-0">
          <AlertTriangle class="w-5 h-5 text-red-600 dark:text-red-400" />
        </div>
        <div>
          <h3 class="font-semibold text-lg dark:text-white">Reset all settings?</h3>
          <p class="text-sm text-gray-600 dark:text-gray-400 mt-1">
            Theme, accent, navigation, notifications, startup, and the download directory will return to defaults.
            Wallet, files, and DHT identity are kept.
          </p>
        </div>
      </div>
      <div class="flex justify-end gap-2 mt-6">
        <button
          onclick={() => (resetConfirmOpen = false)}
          class="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-200 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg transition-colors"
        >
          Cancel
        </button>
        <button
          onclick={resetSettings}
          class="flex items-center gap-2 px-4 py-2 text-sm font-medium text-white bg-red-600 hover:bg-red-700 rounded-lg transition-colors"
        >
          <RotateCcw class="w-4 h-4" />
          Reset
        </button>
      </div>
    </div>
  </div>
{/if}
