<script lang="ts">
  import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
  import { isAuthenticated, isDarkMode, networkConnected, settings, walletAccount } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import { dhtService } from '$lib/dhtService';
  import { gethService } from '$lib/services/gethService';
  import { applyColorTheme } from '$lib/services/colorThemeService';
  import { onMount } from 'svelte';
  import Navbar from '$lib/components/Navbar.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import Toast from '$lib/components/Toast.svelte';
  import WalletPage from './pages/Wallet.svelte';
  import DownloadPage from './pages/Download.svelte';
  import UploadPage from './pages/Upload.svelte';
  import ChiralDropPage from './pages/ChiralDrop.svelte';
  import AccountPage from './pages/Account.svelte';
  import NetworkPage from './pages/Network.svelte';
  import MiningPage from './pages/Mining.svelte';
  import DiagnosticsPage from './pages/Diagnostics.svelte';
  import SettingsPage from './pages/Settings.svelte';
  import HostingPage from './pages/Hosting.svelte';
  import DrivePage from './pages/Drive.svelte';
  import ReputationPage from './pages/Reputation.svelte';
  import HostsPage from './pages/Hosts.svelte';

  let currentPath = $state('/wallet');
  let sidebarCollapsed = $state(false);

  // Apply dark mode class to document
  $effect(() => {
    if (typeof document !== 'undefined') {
      if ($isDarkMode) {
        document.documentElement.classList.add('dark');
      } else {
        document.documentElement.classList.remove('dark');
      }
    }
  });

  // Apply color theme
  $effect(() => {
    if (typeof document !== 'undefined') {
      applyColorTheme($settings.colorTheme);
    }
  });

  // Unpublish shared files from DHT when the window is closing
  onMount(async () => {
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const { invoke } = await import('@tauri-apps/api/core');
      getCurrentWindow().onCloseRequested(async () => {
        try {
          await invoke('unpublish_all_shared_files');
        } catch {
          // DHT may already be stopped — let the window close
        }
      });
    }
  });

  const authenticatedRoutes: RouteConfig[] = [
    {
      path: '/download',
      component: DownloadPage
    },
    {
      path: '/upload',
      component: UploadPage
    },
    {
      path: '/chiraldrop',
      component: ChiralDropPage
    },
    {
      path: '/account',
      component: AccountPage
    },
    {
      path: '/network',
      component: NetworkPage
    },
    {
      path: '/mining',
      component: MiningPage
    },
    {
      path: '/diagnostics',
      component: DiagnosticsPage
    },
    {
      path: '/hosting',
      component: HostingPage
    },
    {
      path: '/drive',
      component: DrivePage
    },
    {
      path: '/reputation',
      component: ReputationPage
    },
    {
      path: '/hosts',
      component: HostsPage
    },
    {
      path: '/settings',
      component: SettingsPage
    },
    {
      path: '/',
      component: NetworkPage
    }
  ];
  
  const unauthenticatedRoutes: RouteConfig[] = [
    {
      path: '/wallet',
      component: WalletPage
    },
    {
      path: '/',
      component: WalletPage
    },
    {
      path: '*',
      component: WalletPage
    }
  ];
  
  // Track current path for navbar highlighting
  $effect(() => {
    currentPath = window.location.pathname || '/';
  });
  
  // Redirect to network page when authenticated
  $effect(() => {
    if ($isAuthenticated) {
      const path = window.location.pathname;
      if (path === '/wallet' || path === '/') {
        goto('/network');
      }
    }
  });

  // Sync download directory setting to backend on startup
  let downloadDirSynced = false;
  $effect(() => {
    if (!downloadDirSynced && typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window)) {
      downloadDirSynced = true;
      const dir = $settings.downloadDirectory;
      if (dir) {
        import('@tauri-apps/api/core').then(({ invoke }) => {
          invoke('set_download_directory', { path: dir }).catch(() => {});
        });
      }
    }
  });

  // Auto-connect DHT once when user logs in
  let dhtAutoConnected = false;
  $effect(() => {
    if ($isAuthenticated && !dhtAutoConnected) {
      dhtAutoConnected = true;
      dhtService.start().catch((err) => {
        const msg = err instanceof Error ? err.message : String(err);
        if (msg.includes('already running')) {
          networkConnected.set(true);
        }
      });
    }
    if (!$isAuthenticated) {
      dhtAutoConnected = false;
    }
  });

  // Auto-start Geth node once when user logs in
  let gethAutoStarted = false;
  $effect(() => {
    if ($isAuthenticated && !gethAutoStarted) {
      gethAutoStarted = true;
      (async () => {
        try {
          const installed = await gethService.isInstalled();
          if (!installed) return;
          const status = await gethService.getStatus();
          if (status.running) {
            // Already running — just start polling
            gethService.startStatusPolling();
            return;
          }
          // Start Geth with wallet address as miner
          const addr = $walletAccount?.address;
          await gethService.start(addr || undefined);
        } catch (err) {
          // Silently handle — user can start manually from Network page
          console.warn('Geth auto-start failed:', err);
        }
      })();
    }
    if (!$isAuthenticated) {
      gethAutoStarted = false;
    }
  });
</script>

{#if $isAuthenticated}
  {#if $settings.navStyle === 'sidebar'}
    <div class="min-h-screen bg-gray-50 dark:bg-gray-900 transition-colors">
      <Sidebar currentPage={currentPath} bind:collapsed={sidebarCollapsed} />
      <div class="transition-[margin] duration-200 hidden md:block {sidebarCollapsed ? 'md:ml-16' : 'md:ml-48'}">
        <Router routes={authenticatedRoutes} />
      </div>
      <div class="md:hidden">
        <Router routes={authenticatedRoutes} />
      </div>
    </div>
  {:else}
    <div class="min-h-screen bg-gray-50 dark:bg-gray-900 transition-colors">
      <Navbar currentPage={currentPath} />
      <Router routes={authenticatedRoutes} />
    </div>
  {/if}
{:else}
  <div class="dark:bg-gray-900 min-h-screen transition-colors">
    <Router routes={unauthenticatedRoutes} />
  </div>
{/if}

<!-- Toast notifications -->
{#each $toasts as toast, index (toast.id)}
  <Toast
    message={toast.message}
    type={toast.type}
    {index}
    onClose={() => toasts.remove(toast.id)}
  />
{/each}

<style>
</style>
