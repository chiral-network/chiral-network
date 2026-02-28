<script lang="ts">
  import { Download, Upload, Wallet, Globe, Settings, LogOut, Send, Pickaxe, Bug, Menu, X, ChevronLeft, ChevronRight, Server, HardDrive, Star, Users } from 'lucide-svelte';
  import { goto } from '@mateothegreat/svelte5-router';
  import { isAuthenticated, walletAccount, networkConnected } from '$lib/stores';

  let { currentPage = 'download', collapsed = $bindable(false) }: { currentPage?: string; collapsed?: boolean } = $props();

  let mobileOpen = $state(false);

  const COLLAPSED_KEY = 'chiral-sidebar-collapsed';
  // Initialize from localStorage on first render
  if (typeof window !== 'undefined' && localStorage.getItem(COLLAPSED_KEY) === 'true') {
    collapsed = true;
  }

  function toggleCollapse() {
    collapsed = !collapsed;
    if (typeof window !== 'undefined') {
      localStorage.setItem(COLLAPSED_KEY, String(collapsed));
    }
  }

  function handleLogout() {
    isAuthenticated.set(false);
    walletAccount.set(null);
    window.location.href = '/';
  }

  function navigate(path: string) {
    goto(path);
    mobileOpen = false;
  }

  const navItems = [
    { path: '/download', label: 'Download', icon: Download },
    { path: '/upload', label: 'Upload', icon: Upload },
    { path: '/chiraldrop', label: 'ChiralDrop', icon: Send },
    { path: '/account', label: 'Account', icon: Wallet },
    { path: '/network', label: 'Network', icon: Globe },
    { path: '/mining', label: 'Mining', icon: Pickaxe },
    { path: '/hosting', label: 'Hosting', icon: Server },
    { path: '/drive', label: 'Drive', icon: HardDrive },
    { path: '/reputation', label: 'Reputation', icon: Star },
    { path: '/hosts', label: 'Hosts', icon: Users },
    { path: '/diagnostics', label: 'Diagnostics', icon: Bug },
    { path: '/settings', label: 'Settings', icon: Settings }
  ];
</script>

<!-- Mobile top bar -->
<div class="md:hidden sticky top-0 z-50 flex items-center justify-between h-14 px-3 bg-white dark:bg-gray-800 shadow-md border-b border-gray-200 dark:border-gray-700">
  <div class="flex items-center gap-2">
    <img src="/logo.png" alt="Chiral Network" class="w-7 h-7 rounded-lg" />
    <span class="text-lg font-bold dark:text-white">Chiral</span>
  </div>
  <div class="flex items-center gap-2">
    <div class="flex items-center gap-1.5 px-2 py-1 rounded-full
      {$networkConnected
        ? 'bg-green-50 dark:bg-green-900/30'
        : 'bg-red-50 dark:bg-red-900/30'}">
      <div class="w-2 h-2 rounded-full {$networkConnected ? 'bg-green-500' : 'bg-red-500'}"></div>
    </div>
    <button
      onclick={() => mobileOpen = !mobileOpen}
      class="p-1.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition"
    >
      {#if mobileOpen}
        <X class="w-5 h-5" />
      {:else}
        <Menu class="w-5 h-5" />
      {/if}
    </button>
  </div>
</div>

<!-- Mobile overlay -->
{#if mobileOpen}
  <div class="md:hidden fixed inset-0 z-40 bg-black/50" onclick={() => mobileOpen = false}></div>
  <div class="md:hidden fixed top-14 left-0 right-0 z-50 bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 shadow-lg">
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

<!-- Desktop sidebar -->
<aside
  class="hidden md:flex fixed top-0 left-0 z-40 h-screen flex-col bg-white dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 shadow-sm transition-[width] duration-200
    {collapsed ? 'w-16' : 'w-48'}"
>
  <!-- Logo + collapse toggle -->
  <div class="flex items-center h-14 px-3 border-b border-gray-200 dark:border-gray-700 shrink-0 overflow-hidden
    {collapsed ? 'justify-center' : 'justify-between'}">
    <div class="flex items-center gap-2 overflow-hidden">
      <img src="/logo.png" alt="Chiral Network" class="w-7 h-7 rounded-lg shrink-0" />
      {#if !collapsed}
        <span class="text-sm font-bold dark:text-white whitespace-nowrap">Chiral Network</span>
      {/if}
    </div>
    {#if !collapsed}
      <button
        onclick={toggleCollapse}
        class="p-1 text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-md transition shrink-0"
        title="Collapse sidebar"
      >
        <ChevronLeft class="w-4 h-4" />
      </button>
    {/if}
  </div>

  <!-- Network status -->
  <div class="flex items-center gap-3 px-3 py-2 border-b border-gray-200 dark:border-gray-700 shrink-0
    {collapsed ? 'justify-center' : ''}">
    <div class="w-2 h-2 rounded-full shrink-0 {$networkConnected ? 'bg-green-500' : 'bg-red-500'}"></div>
    {#if !collapsed}
      <span class="text-xs font-medium whitespace-nowrap
        {$networkConnected
          ? 'text-green-700 dark:text-green-400'
          : 'text-red-700 dark:text-red-400'}">
        {$networkConnected ? 'Connected' : 'Offline'}
      </span>
    {/if}
  </div>

  <!-- Nav items -->
  <nav class="flex-1 px-2 py-3 space-y-1 overflow-y-auto overflow-x-hidden">
    {#each navItems as item}
      <button
        onclick={() => navigate(item.path)}
        class="flex items-center gap-3 w-full py-2.5 rounded-lg transition text-sm
          {collapsed ? 'justify-center px-0' : 'px-3'}
          {currentPage === item.path
            ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400'
            : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'}"
        title={collapsed ? item.label : ''}
      >
        <svelte:component this={item.icon} class="w-4 h-4 shrink-0" />
        {#if !collapsed}
          <span class="font-medium whitespace-nowrap">{item.label}</span>
        {/if}
      </button>
    {/each}
  </nav>

  <!-- Bottom section -->
  <div class="px-2 pb-3 space-y-1 border-t border-gray-200 dark:border-gray-700 pt-3 shrink-0">
    <!-- Expand toggle (only when collapsed) -->
    {#if collapsed}
      <button
        onclick={toggleCollapse}
        class="flex items-center justify-center w-full py-2 text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm"
        title="Expand sidebar"
      >
        <ChevronRight class="w-4 h-4 shrink-0" />
      </button>
    {/if}

    <!-- Logout -->
    <button
      onclick={handleLogout}
      class="flex items-center gap-3 w-full py-2 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm
        {collapsed ? 'justify-center px-0' : 'px-3'}"
      title={collapsed ? 'Logout' : ''}
    >
      <LogOut class="w-4 h-4 shrink-0" />
      {#if !collapsed}
        <span class="font-medium whitespace-nowrap">Logout</span>
      {/if}
    </button>
  </div>
</aside>
