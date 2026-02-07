<script lang="ts">
  import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
  import { isAuthenticated, isDarkMode, networkConnected } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import { dhtService } from '$lib/dhtService';
  import Navbar from '$lib/components/Navbar.svelte';
  import Toast from '$lib/components/Toast.svelte';
  import WalletPage from './pages/Wallet.svelte';
  import DownloadPage from './pages/Download.svelte';
  import UploadPage from './pages/Upload.svelte';
  import ChiralDropPage from './pages/ChiralDrop.svelte';
  import AccountPage from './pages/Account.svelte';
  import NetworkPage from './pages/Network.svelte';
  import MiningPage from './pages/Mining.svelte';
  import SettingsPage from './pages/Settings.svelte';

  let currentPath = $state('/wallet');

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

  // Auto-connect DHT when user logs in
  $effect(() => {
    if ($isAuthenticated && !$networkConnected) {
      dhtService.start().catch((err) => {
        const msg = err instanceof Error ? err.message : String(err);
        if (msg.includes('already running')) {
          networkConnected.set(true);
        }
      });
    }
  });
</script>

{#if $isAuthenticated}
  <div class="min-h-screen bg-gray-50 dark:bg-gray-900 transition-colors">
    <Navbar currentPage={currentPath} />
    <Router routes={authenticatedRoutes} />
  </div>
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
