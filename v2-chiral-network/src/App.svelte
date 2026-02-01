<script lang="ts">
  import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
  import { isAuthenticated } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import Navbar from '$lib/components/Navbar.svelte';
  import Toast from '$lib/components/Toast.svelte';
  import WalletPage from './pages/Wallet.svelte';
  import DownloadPage from './pages/Download.svelte';
  import UploadPage from './pages/Upload.svelte';
  import AccountPage from './pages/Account.svelte';
  import NetworkPage from './pages/Network.svelte';
  import SettingsPage from './pages/Settings.svelte';
  
  let currentPath = $state('/wallet');
  
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
      path: '/account',
      component: AccountPage
    },
    {
      path: '/network',
      component: NetworkPage
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
</script>

{#if $isAuthenticated}
  <div class="min-h-screen bg-gray-50">
    <Navbar currentPage={currentPath} />
    <Router routes={authenticatedRoutes} />
  </div>
{:else}
  <Router routes={unauthenticatedRoutes} />
{/if}

<!-- Toast notifications -->
{#each $toasts as toast (toast.id)}
  <Toast
    message={toast.message}
    type={toast.type}
    onClose={() => toasts.remove(toast.id)}
  />
{/each}

<style>
</style>
