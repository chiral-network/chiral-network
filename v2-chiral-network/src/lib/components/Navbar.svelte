<script lang="ts">
  import { Download, Upload, Wallet, Globe, Settings, LogOut, Send, Pickaxe, Bug, Menu, X } from 'lucide-svelte';
  import { goto } from '@mateothegreat/svelte5-router';
  import { isAuthenticated, walletAccount, networkConnected } from '$lib/stores';

  let { currentPage = 'download' }: { currentPage?: string } = $props();

  let mobileMenuOpen = $state(false);

  function handleLogout() {
    isAuthenticated.set(false);
    walletAccount.set(null);
    window.location.href = '/';
  }

  function navigate(path: string) {
    goto(path);
    mobileMenuOpen = false;
  }

  const navItems = [
    { path: '/download', label: 'Download', icon: Download },
    { path: '/upload', label: 'Upload', icon: Upload },
    { path: '/chiraldrop', label: 'ChiralDrop', icon: Send },
    { path: '/account', label: 'Account', icon: Wallet },
    { path: '/network', label: 'Network', icon: Globe },
    { path: '/mining', label: 'Mining', icon: Pickaxe },
    { path: '/diagnostics', label: 'Diagnostics', icon: Bug },
    { path: '/settings', label: 'Settings', icon: Settings }
  ];
</script>

<nav class="sticky top-0 z-50 bg-white dark:bg-gray-800 shadow-md border-b border-gray-200 dark:border-gray-700">
  <div class="max-w-7xl mx-auto px-3 sm:px-4">
    <div class="flex items-center justify-between h-14">
      <!-- Logo + Nav Items -->
      <div class="flex items-center gap-2 lg:gap-6 min-w-0">
        <div class="flex items-center gap-2 shrink-0">
          <img src="/logo.png" alt="Chiral Network" class="w-7 h-7 rounded-lg" />
          <span class="text-lg font-bold dark:text-white hidden xl:inline">Chiral Network</span>
          <span class="text-lg font-bold dark:text-white hidden sm:inline xl:hidden">Chiral</span>
        </div>

        <!-- Desktop nav: icons + labels at xl, icons only at md-xl -->
        <div class="hidden md:flex gap-0.5">
          {#each navItems as item}
            <button
              onclick={() => navigate(item.path)}
              class="flex items-center gap-1.5 px-2 xl:px-3 py-1.5 rounded-lg transition text-sm
                {currentPage === item.path
                  ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                  : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'}"
              title={item.label}
            >
              <svelte:component this={item.icon} class="w-4 h-4 shrink-0" />
              <span class="font-medium hidden xl:inline">{item.label}</span>
            </button>
          {/each}
        </div>
      </div>

      <!-- Right side: status + logout + hamburger -->
      <div class="flex items-center gap-2 sm:gap-3 shrink-0">
        <div class="flex items-center gap-1.5 px-2 py-1 rounded-full
          {$networkConnected
            ? 'bg-green-50 dark:bg-green-900/30'
            : 'bg-red-50 dark:bg-red-900/30'}">
          <div class="w-2 h-2 rounded-full {$networkConnected ? 'bg-green-500' : 'bg-red-500'}"></div>
          <span class="text-xs font-medium hidden sm:inline
            {$networkConnected
              ? 'text-green-700 dark:text-green-400'
              : 'text-red-700 dark:text-red-400'}">
            {$networkConnected ? 'Connected' : 'Offline'}
          </span>
        </div>

        <button
          onclick={handleLogout}
          class="hidden sm:flex items-center gap-1.5 px-3 py-1.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm"
          title="Logout"
        >
          <LogOut class="w-4 h-4" />
          <span class="hidden lg:inline">Logout</span>
        </button>

        <!-- Mobile hamburger -->
        <button
          onclick={() => mobileMenuOpen = !mobileMenuOpen}
          class="md:hidden p-1.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition"
        >
          {#if mobileMenuOpen}
            <X class="w-5 h-5" />
          {:else}
            <Menu class="w-5 h-5" />
          {/if}
        </button>
      </div>
    </div>
  </div>

  <!-- Mobile menu dropdown -->
  {#if mobileMenuOpen}
    <div class="md:hidden border-t border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800">
      <div class="px-3 py-2 space-y-1">
        {#each navItems as item}
          <button
            onclick={() => navigate(item.path)}
            class="flex items-center gap-3 w-full px-3 py-2.5 rounded-lg transition text-sm
              {currentPage === item.path
                ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'}"
          >
            <svelte:component this={item.icon} class="w-4 h-4" />
            <span class="font-medium">{item.label}</span>
          </button>
        {/each}
        <hr class="border-gray-200 dark:border-gray-700" />
        <button
          onclick={handleLogout}
          class="flex items-center gap-3 w-full px-3 py-2.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm"
        >
          <LogOut class="w-4 h-4" />
          <span class="font-medium">Logout</span>
        </button>
      </div>
    </div>
  {/if}
</nav>
