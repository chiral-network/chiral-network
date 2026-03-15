<script lang="ts">
 import { onMount } from'svelte';
 import { settings, isDarkMode, type ThemeMode, type NotificationSettings, type ColorTheme, type NavStyle } from'$lib/stores';
 import { availableThemes } from'$lib/services/colorThemeService';
 import { toasts } from'$lib/toastStore';
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
 } from'lucide-svelte';

 let isTauri = $state(false);
 let displayDownloadDir = $state('');

 function checkTauriAvailability(): boolean {
 return typeof window !=='undefined' && ('__TAURI__' in window ||'__TAURI_INTERNALS__' in window);
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
 toasts.show('Download directory updated','success');
 }
 } catch (e) {
 toasts.show(`Failed to set directory: ${e}`,'error');
 }
 }

 async function resetDownloadDirectory() {
 try {
 const { invoke } = await import('@tauri-apps/api/core');
 await invoke('set_download_directory', { path: null });
 settings.update((s) => ({ ...s, downloadDirectory:'' }));
 displayDownloadDir = await invoke<string>('get_download_directory');
 toasts.show('Download directory reset to system default','info');
 } catch (e) {
 toasts.show(`Failed to reset directory: ${e}`,'error');
 }
 }

 // Theme options
 const themeOptions: { value: ThemeMode; label: string; icon: typeof Sun }[] = [
 { value:'light', label:'Light', icon: Sun },
 { value:'dark', label:'Dark', icon: Moon },
 { value:'system', label:'System', icon: Monitor }
 ];

 function setTheme(theme: ThemeMode) {
 settings.update((s) => ({ ...s, theme }));
 toasts.show(`Theme set to ${theme}`,'success');
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
 { value:'navbar', label:'Top Bar', icon: PanelTop },
 { value:'sidebar', label:'Sidebar', icon: PanelLeft }
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
 { key:'downloadComplete', label:'Download Complete', description:'When a file finishes downloading' },
 { key:'downloadFailed', label:'Download Failed', description:'When a file download fails' },
 { key:'peerConnected', label:'Peer Connected', description:'When a new peer connects to you' },
 { key:'peerDisconnected', label:'Peer Disconnected', description:'When a peer disconnects from you' },
 { key:'miningBlock', label:'Mining Block Found', description:'When you mine a new block' },
 { key:'paymentReceived', label:'Payment Received', description:'When you receive a CHI payment for a file' },
 { key:'networkStatus', label:'Network Status', description:'Connection and disconnection events' },
 { key:'fileShared', label:'File Shared', description:'When someone starts downloading your shared file' }
 ];

 function resetSettings() {
 settings.reset();
 if (isTauri) {
 resetDownloadDirectory();
 }
 toasts.show('Settings reset to defaults','info');
 }
</script>

<svelte:head><title>Settings | Chiral Network</title></svelte:head>

<div class="p-4 sm:p-6 max-w-6xl mx-auto">
 <div class="mb-6">
 <h1 class="text-2xl font-light tracking-tight">Settings</h1>
 <p class="text-[var(--text-secondary)] mt-1">Customize your Chiral Network experience</p>
 </div>

 <!-- Appearance Section -->
 <div class=" p-6 mb-6">
 <div class="flex items-center gap-3 mb-6">
 <div class="p-2 bg-purple-100 rounded-lg">
 <Palette class="w-5 h-5 text-purple-400" />
 </div>
 <div>
 <h2 class="font-semibold text-lg">Appearance</h2>
 <p class="text-sm text-[var(--text-secondary)]">Customize how the app looks</p>
 </div>
 </div>

 <!-- Theme Selection -->
 <div class="mb-6">
 <span class="block text-sm font-medium text-white/70 mb-3">Theme</span>
 <div class="grid grid-cols-3 gap-3">
 {#each themeOptions as option}
 {@const Icon = option.icon}
 <button
 onclick={() => setTheme(option.value)}
 class="relative flex flex-col items-center gap-2 p-4 rounded-lg border-2 transition-all
 {$settings.theme === option.value
 ?'border-blue-400 bg-blue-500/[0.06]'
 :'border-[var(--border)]/60 hover:border-[var(--border)] bg-[var(--surface-1)]'}"
 >
 {#if $settings.theme === option.value}
 <div class="absolute top-2 right-2">
 <Check class="w-4 h-4 text-blue-400" />
 </div>
 {/if}
 <Icon class="w-6 h-6 {$settings.theme === option.value ?'text-blue-400' :'text-[var(--text-secondary)]'}" />
 <span class="text-sm font-medium {$settings.theme === option.value ?'text-blue-400' :'text-white/70'}">
 {option.label}
 </span>
 </button>
 {/each}
 </div>
 <p class="text-xs text-[var(--text-secondary)] mt-2">
 {#if $settings.theme ==='system'}
 Currently using {$isDarkMode ?'dark' :'light'} mode based on your system preference
 {:else}
 Using {$settings.theme} mode
 {/if}
 </p>
 </div>

 <!-- Accent Color -->
 <div class="mb-6">
 <span class="block text-sm font-medium text-white/70 mb-3">Accent Color</span>
 <div class="flex gap-3">
 {#each availableThemes as ct}
 <button
 onclick={() => setColorTheme(ct.value)}
 class="relative w-10 h-10 rounded-full transition-all
 {$settings.colorTheme === ct.value
 ?'scale-110 border-2 border-blue-400/40'
 :'hover:scale-105'}"
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
 <div class="flex items-center justify-between py-4 border-t border-[var(--border)]/60">
 <div class="flex items-center gap-3">
 <LayoutGrid class="w-5 h-5 text-[var(--text-secondary)]" />
 <div>
 <p class="font-medium text-white">Compact Mode</p>
 <p class="text-sm text-[var(--text-secondary)]">Use smaller spacing and font sizes</p>
 </div>
 </div>
 <button
 onclick={toggleCompactMode}
 class="relative w-12 h-6 rounded-full transition-colors
 {$settings.compactMode ?'bg-blue-500/[0.06]0' :'bg-[var(--surface-1)]'}"
 role="switch"
 aria-checked={$settings.compactMode}
 aria-label="Toggle compact mode"
 >
 <span
 class="absolute top-0.5 left-0.5 w-5 h-5 bg-[var(--surface-0)] rounded-full shadow transition-transform
 {$settings.compactMode ?'translate-x-6' :'translate-x-0'}"
 ></span>
 </button>
 </div>

 <!-- Navigation Style -->
 <div class="py-4 border-t border-[var(--border)]/60">
 <span class="block text-sm font-medium text-white/70 mb-3">Navigation Style</span>
 <div class="grid grid-cols-2 gap-3">
 {#each navStyleOptions as option}
 {@const Icon = option.icon}
 <button
 onclick={() => setNavStyle(option.value)}
 class="relative flex flex-col items-center gap-2 p-4 rounded-lg border-2 transition-all
 {$settings.navStyle === option.value
 ?'border-blue-400 bg-blue-500/[0.06]'
 :'border-[var(--border)]/60 hover:border-[var(--border)] bg-[var(--surface-1)]'}"
 >
 {#if $settings.navStyle === option.value}
 <div class="absolute top-2 right-2">
 <Check class="w-4 h-4 text-blue-400" />
 </div>
 {/if}
 <Icon class="w-6 h-6 {$settings.navStyle === option.value ?'text-blue-400' :'text-[var(--text-secondary)]'}" />
 <span class="text-sm font-medium {$settings.navStyle === option.value ?'text-blue-400' :'text-white/70'}">
 {option.label}
 </span>
 </button>
 {/each}
 </div>
 </div>

 <!-- Preview -->
 <div class="pt-4 border-t border-[var(--border)]/60">
 <span class="block text-sm font-medium text-white/70 mb-3">Preview</span>
 <div class="p-4 rounded-lg bg-[var(--surface-1)] border border-[var(--border)]/60">
 <div class="flex items-center gap-3 mb-3">
 <div class="w-10 h-10 rounded-full bg-blue-500/[0.06]0"></div>
 <div>
 <p class="font-medium text-white">Sample User</p>
 <p class="text-sm text-[var(--text-secondary)]">0x1234...5678</p>
 </div>
 </div>
 <div class="grid grid-cols-2 gap-3">
 <div class="p-3 rounded-lg bg-[var(--surface-0)] border border-[var(--border)]/60">
 <p class="text-xs text-[var(--text-secondary)]">Balance</p>
 <p class="text-lg font-bold text-white tabular-nums">100.00 CHI</p>
 </div>
 <div class="p-3 rounded-lg bg-[var(--surface-0)] border border-[var(--border)]/60">
 <p class="text-xs text-[var(--text-secondary)]">Peers</p>
 <p class="text-lg font-bold text-white">12</p>
 </div>
 </div>
 </div>
 </div>
 </div>

 <!-- Storage Section -->
 {#if isTauri}
 <div class=" p-6 mb-6">
 <div class="flex items-center gap-3 mb-6">
 <div class="p-2 bg-blue-400/[0.06] rounded-lg">
 <HardDrive class="w-5 h-5 text-blue-400" />
 </div>
 <div>
 <h2 class="font-semibold text-lg">Storage</h2>
 <p class="text-sm text-[var(--text-secondary)]">Configure where downloaded files are saved</p>
 </div>
 </div>

 <!-- Download Directory -->
 <div>
 <span class="block text-sm font-medium text-white/70 mb-2">Download Directory</span>
 <div class="flex items-center gap-3">
 <div class="flex-1 flex items-center gap-2 px-3 py-2.5 bg-[var(--surface-1)] border border-[var(--border)]/60 rounded-lg">
 <FolderOpen class="w-4 h-4 text-[var(--text-secondary)] flex-shrink-0" />
 <span class="text-sm text-white/70 truncate font-mono">
 {displayDownloadDir ||'Loading...'}
 </span>
 </div>
 <button
 onclick={browseDownloadDirectory}
 class="px-4 py-2.5 bg-blue-400 text-white text-sm font-medium rounded-lg hover:bg-blue-500 transition-colors flex-shrink-0 focus:outline-none focus:border-blue-400/40"
 >
 Browse
 </button>
 {#if $settings.downloadDirectory}
 <button
 onclick={resetDownloadDirectory}
 class="p-2.5 text-[var(--text-secondary)] hover:text-red-500 transition-colors flex-shrink-0"
 title="Reset to system default"
 >
 <X class="w-4 h-4" />
 </button>
 {/if}
 </div>
 <p class="text-xs text-[var(--text-secondary)] mt-2">
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
 <div class=" p-6 mb-6">
 <div class="flex items-center gap-3 mb-4">
 <div class="p-2 bg-amber-100 rounded-lg">
 <Bell class="w-5 h-5 text-amber-400" />
 </div>
 <div>
 <h2 class="font-semibold text-lg">Notifications</h2>
 <p class="text-sm text-[var(--text-secondary)]">Toggle which toast notifications to show</p>
 </div>
 </div>

 <div class="grid grid-cols-2 gap-x-6 gap-y-2">
 {#each notificationOptions as option}
 <button
 onclick={() => toggleNotification(option.key)}
 class="flex items-center justify-between gap-3 py-2 px-3 rounded-lg hover:bg-[var(--surface-1)] transition-colors group"
 role="switch"
 aria-checked={$settings.notifications?.[option.key] ?? true}
 title={option.description}
 >
 <span class="text-sm text-white/70 text-left">{option.label}</span>
 <div class="relative w-9 h-5 rounded-full shrink-0 transition-colors
 {$settings.notifications?.[option.key] ?'bg-blue-500/[0.06]0' :'bg-[var(--surface-1)]'}">
 <span
 class="absolute top-0.5 left-0.5 w-4 h-4 bg-[var(--surface-0)] rounded-full shadow transition-transform
 {$settings.notifications?.[option.key] ?'translate-x-4' :'translate-x-0'}"
 ></span>
 </div>
 </button>
 {/each}
 </div>
 </div>

 <!-- Reset Section -->
 <div class=" p-6">
 <div class="flex items-center justify-between">
 <div>
 <h3 class="font-semibold text-white">Reset Settings</h3>
 <p class="text-sm text-[var(--text-secondary)]">Restore all settings to their default values</p>
 </div>
 <button
 onclick={resetSettings}
 class="flex items-center gap-2 px-4 py-2 text-red-400 border border-red-800 rounded-lg hover:bg-red-900/30 transition-colors focus:outline-none focus:ring-red-500/30"
 >
 <RotateCcw class="w-4 h-4" />
 Reset
 </button>
 </div>
 </div>
</div>
