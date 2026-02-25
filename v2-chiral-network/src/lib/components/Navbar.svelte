<script lang="ts">
  import { Download, Upload, Wallet, Globe, Settings, LogOut, Send, Pickaxe, Bug, Menu, X, Server, ChevronDown } from 'lucide-svelte';
  import { goto } from '@mateothegreat/svelte5-router';
  import { isAuthenticated, walletAccount, networkConnected } from '$lib/stores';

  let { currentPage = 'download' }: { currentPage?: string } = $props();

  let mobileMenuOpen = $state(false);
  let moreMenuOpen = $state(false);

  function handleLogout() {
    isAuthenticated.set(false);
    walletAccount.set(null);
    window.location.href = '/';
  }

  function navigate(path: string) {
    goto(path);
    mobileMenuOpen = false;
    moreMenuOpen = false;
  }

  const MAX_VISIBLE = 6;

  const navItems = [
    { path: '/download', label: 'Download', icon: Download },
    { path: '/upload', label: 'Upload', icon: Upload },
    { path: '/chiraldrop', label: 'ChiralDrop', icon: Send },
    { path: '/account', label: 'Account', icon: Wallet },
    { path: '/network', label: 'Network', icon: Globe },
    { path: '/mining', label: 'Mining', icon: Pickaxe },
    { path: '/hosting', label: 'Hosting', icon: Server },
    { path: '/diagnostics', label: 'Diagnostics', icon: Bug },
    { path: '/settings', label: 'Settings', icon: Settings }
  ];

  const visibleItems = navItems.slice(0, MAX_VISIBLE);
  const moreItems = navItems.slice(MAX_VISIBLE);

  $effect(() => {
    if (!moreMenuOpen) return;
    function handleClick(e: MouseEvent) {
      const target = e.target as HTMLElement;
      if (!target.closest('.more-menu-container')) {
        moreMenuOpen = false;
      }
    }
    document.addEventListener('click', handleClick);
    return () => document.removeEventListener('click', handleClick);
  });

  const isMoreActive = $derived(moreItems.some(item => item.path === currentPage));
</script>

<nav class="sticky top-0 z-50 bg-white dark:bg-gray-800 shadow-md border-b border-gray-200 dark:border-gray-700">
  <div class="max-w-7xl mx-auto px-3 sm:px-4">
    <div class="flex items-center justify-between h-14 gap-2">
      <!-- Logo -->
      <div class="flex items-center gap-2 shrink-0">
        <img src="/logo.png" alt="Chiral Network" class="w-7 h-7 rounded-lg" />
        <span class="text-lg font-bold dark:text-white hidden xl:inline">Chiral Network</span>
        <span class="text-lg font-bold dark:text-white hidden sm:inline xl:hidden">Chiral</span>
      </div>

      <!-- Desktop nav: icon-only, all items (md to xl) -->
      <div class="hidden md:flex xl:hidden items-center gap-0.5 shrink-0">
        {#each navItems as item}
          <button
            onclick={() => navigate(item.path)}
            class="flex items-center px-2 py-1.5 rounded-lg transition text-sm
              {currentPage === item.path
                ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400'
                : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'}"
            title={item.label}
          >
            <svelte:component this={item.icon} class="w-4 h-4" />
          </button>
        {/each}
      </div>

      <!-- Desktop nav: with labels + More dropdown (xl+) -->
      <div class="hidden xl:flex items-center gap-0.5 shrink-0">
        {#each visibleItems as item}
          <button
            onclick={() => navigate(item.path)}
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg transition text-sm whitespace-nowrap
              {currentPage === item.path
                ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400'
                : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'}"
            title={item.label}
          >
            <svelte:component this={item.icon} class="w-4 h-4 shrink-0" />
            <span class="font-medium">{item.label}</span>
          </button>
        {/each}

        {#if moreItems.length > 0}
          <div class="relative more-menu-container">
            <button
              onclick={() => moreMenuOpen = !moreMenuOpen}
              class="flex items-center gap-1 px-3 py-1.5 rounded-lg transition text-sm whitespace-nowrap
                {isMoreActive
                  ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400'
                  : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'}"
              title="More"
            >
              <span class="font-medium">More</span>
              <ChevronDown class="w-4 h-4 transition-transform {moreMenuOpen ? 'rotate-180' : ''}" />
            </button>

            {#if moreMenuOpen}
              <div class="absolute right-0 top-full mt-1 w-48 bg-white dark:bg-gray-800 rounded-lg shadow-lg border border-gray-200 dark:border-gray-700 py-1 z-50">
                {#each moreItems as item}
                  <button
                    onclick={() => navigate(item.path)}
                    class="flex items-center gap-2.5 w-full px-3 py-2 text-sm transition
                      {currentPage === item.path
                        ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400'
                        : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'}"
                  >
                    <svelte:component this={item.icon} class="w-4 h-4" />
                    <span class="font-medium">{item.label}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <!-- Right side: logout + status + hamburger -->
      <div class="flex items-center gap-2 sm:gap-3 shrink-0">
        <button
          onclick={handleLogout}
          class="hidden sm:flex items-center gap-1.5 px-3 py-1.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm"
          title="Logout"
        >
          <LogOut class="w-4 h-4" />
          <span class="hidden lg:inline">Logout</span>
        </button>

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
                ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400'
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
