<script lang="ts">
  import { Download, Wallet, Globe, Settings, LogOut, Send, Pickaxe, Bug, Menu, X, ChevronLeft, ChevronRight, Server, HardDrive } from 'lucide-svelte';
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
    { path: '/drive', label: 'Drive', icon: HardDrive },
    { path: '/account', label: 'Account', icon: Wallet },

    { path: '/hosts', label: 'Hosts', icon: Server },
    { path: '/network', label: 'Network', icon: Globe },
    { path: '/settings', label: 'Settings', icon: Settings },
    { path: '/chiraldrop', label: 'ChiralDrop', icon: Send },
    { path: '/diagnostics', label: 'Diagnostics', icon: Bug },
    { path: '/mining', label: 'Mining', icon: Pickaxe },
  ];
</script>

<!-- Mobile top bar -->
<div class="md:hidden sticky top-0 z-50 flex items-center justify-between h-14 px-3 bg-gray-950 border-b border-cyan-500/20 shadow-[0_0_15px_rgba(6,182,212,0.08)]">
  <div class="flex items-center gap-2">
    <img src="/logo.png" alt="Chiral Network" class="w-7 h-7 rounded-lg" />
    <span class="text-lg font-bold text-cyan-400 neon-text">Chiral</span>
  </div>
  <div class="flex items-center gap-2">
    <div class="flex items-center gap-1.5 px-2 py-1 rounded-full
      {$networkConnected
        ? 'bg-emerald-500/10 border border-emerald-500/30'
        : 'bg-red-500/10 border border-red-500/30'}">
      <div class="w-2 h-2 rounded-full {$networkConnected ? 'bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.6)]' : 'bg-red-400 shadow-[0_0_6px_rgba(248,113,113,0.6)]'}"></div>
    </div>
    <button
      onclick={() => mobileOpen = !mobileOpen}
      class="p-1.5 text-gray-300 hover:bg-cyan-500/10 rounded-lg transition"
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
  <div class="md:hidden fixed top-14 left-0 right-0 z-50 bg-gray-950 border-b border-cyan-500/20 shadow-[0_0_15px_rgba(6,182,212,0.08)] max-h-[calc(100vh-3.5rem)] overflow-y-auto">
    <div class="px-3 py-2 space-y-1">
      {#each navItems as item}
        <button
          onclick={() => navigate(item.path)}
          class="flex items-center gap-3 w-full px-3 py-2.5 rounded-lg transition text-sm
            {currentPage === item.path
              ? 'bg-cyan-500/10 text-cyan-400 border border-cyan-500/30'
              : 'text-gray-400 hover:bg-cyan-500/5 hover:text-cyan-300'}"
        >
          <svelte:component this={item.icon} class="w-4 h-4" />
          <span class="font-medium">{item.label}</span>
        </button>
      {/each}
      <hr class="border-cyan-500/10" />
      <button
        onclick={handleLogout}
        class="flex items-center gap-3 w-full px-3 py-2.5 text-gray-400 hover:bg-red-500/10 hover:text-red-400 rounded-lg transition text-sm"
      >
        <LogOut class="w-4 h-4" />
        <span class="font-medium">Logout</span>
      </button>
    </div>
  </div>
{/if}

<!-- Desktop sidebar -->
<aside
  class="hidden md:flex fixed top-0 left-0 z-40 h-screen flex-col bg-gray-950 border-r border-cyan-500/20 shadow-[0_0_15px_rgba(6,182,212,0.08)] transition-[width] duration-200
    {collapsed ? 'w-16' : 'w-48'}"
>
  <!-- Logo + collapse toggle -->
  <div class="flex items-center h-14 px-3 border-b border-cyan-500/20 shrink-0 overflow-hidden
    {collapsed ? 'justify-center' : 'justify-between'}">
    <div class="flex items-center gap-2 overflow-hidden">
      <img src="/logo.png" alt="Chiral Network" class="w-7 h-7 rounded-lg shrink-0" />
      {#if !collapsed}
        <span class="text-sm font-bold text-cyan-400 whitespace-nowrap">Chiral Network</span>
      {/if}
    </div>
    {#if !collapsed}
      <button
        onclick={toggleCollapse}
        class="p-1 text-gray-500 hover:bg-cyan-500/10 hover:text-cyan-400 rounded-md transition shrink-0"
        title="Collapse sidebar"
      >
        <ChevronLeft class="w-4 h-4" />
      </button>
    {/if}
  </div>

  <!-- Network status -->
  <div class="flex items-center gap-3 px-3 py-2 border-b border-cyan-500/20 shrink-0
    {collapsed ? 'justify-center' : ''}">
    <div class="w-2 h-2 rounded-full shrink-0 {$networkConnected ? 'bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.6)]' : 'bg-red-400 shadow-[0_0_6px_rgba(248,113,113,0.6)]'}"></div>
    {#if !collapsed}
      <span class="text-xs font-medium whitespace-nowrap
        {$networkConnected
          ? 'text-emerald-400'
          : 'text-red-400'}">
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
            ? 'bg-cyan-500/10 text-cyan-400 border border-cyan-500/30 shadow-[0_0_10px_rgba(6,182,212,0.1)]'
            : 'text-gray-400 hover:bg-cyan-500/5 hover:text-cyan-300 border border-transparent'}"
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
  <div class="px-2 pb-3 space-y-1 border-t border-cyan-500/20 pt-3 shrink-0">
    <!-- Expand toggle (only when collapsed) -->
    {#if collapsed}
      <button
        onclick={toggleCollapse}
        class="flex items-center justify-center w-full py-2 text-gray-500 hover:bg-cyan-500/10 hover:text-cyan-400 rounded-lg transition text-sm"
        title="Expand sidebar"
      >
        <ChevronRight class="w-4 h-4 shrink-0" />
      </button>
    {/if}

    <!-- Logout -->
    <button
      onclick={handleLogout}
      class="flex items-center gap-3 w-full py-2 text-gray-400 hover:bg-red-500/10 hover:text-red-400 rounded-lg transition text-sm
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
