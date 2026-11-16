<script lang="ts">
  import { LogOut, Menu, X, ChevronLeft, ChevronRight, Loader2, Power } from 'lucide-svelte';
  import { session } from '$lib/stores';
  import { navItems, type PageKey } from '$lib/navigation';
  import { shortAddress } from '$lib/components/ui/tones';
  import ConnectionPill from './ConnectionPill.svelte';

  // onQuit is provided only in the native runtime (book A.1.10: closing the window minimizes; Quit exits).
  let { page, collapsed = $bindable(false), signingOut = false, onLogout, onQuit = undefined }: { page: PageKey; collapsed?: boolean; signingOut?: boolean; onLogout: () => void; onQuit?: () => void } = $props();

  let mobileOpen = $state(false);
  const COLLAPSED_KEY = 'chiral-sidebar-collapsed';
  try { if (localStorage.getItem(COLLAPSED_KEY) === 'true') collapsed = true; } catch { /* per-viewer convenience only */ }

  function toggleCollapse() {
    collapsed = !collapsed;
    try { localStorage.setItem(COLLAPSED_KEY, String(collapsed)); } catch { /* per-viewer convenience only */ }
  }

  const itemClass = (active: boolean) => active
    ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400'
    : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700';
</script>

<!-- Mobile top bar -->
<div class="md:hidden sticky top-0 z-50 flex items-center justify-between h-14 px-3 bg-white dark:bg-gray-800 shadow-md border-b border-gray-200 dark:border-gray-700">
  <a href="#/marketplace" class="flex items-center gap-2 no-underline">
    <img src="/logo.png" alt="" class="w-7 h-7 rounded-lg" />
    <span class="text-lg font-bold text-gray-900 dark:text-white">Chiral</span>
  </a>
  <div class="flex items-center gap-2">
    <ConnectionPill compact />
    <button type="button" onclick={() => mobileOpen = !mobileOpen} aria-expanded={mobileOpen} aria-label="Toggle navigation menu"
      class="p-1.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition">
      {#if mobileOpen}<X class="w-5 h-5" />{:else}<Menu class="w-5 h-5" />{/if}
    </button>
  </div>
</div>

{#if mobileOpen}
  <button type="button" class="md:hidden fixed inset-0 z-40 bg-black/50 cursor-default" aria-label="Close navigation menu" onclick={() => mobileOpen = false}></button>
  <nav aria-label="Main navigation" class="md:hidden fixed top-14 left-0 right-0 z-50 bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 shadow-lg max-h-[calc(100vh-3.5rem)] overflow-y-auto">
    <div class="px-3 py-2 space-y-1">
      {#each navItems as item (item.key)}
        {@const Icon = item.icon}
        <a href={`#/${item.key}`} onclick={() => mobileOpen = false} aria-current={page === item.key ? 'page' : undefined}
          class="flex items-center gap-3 w-full px-3 py-2.5 rounded-lg transition text-sm no-underline {itemClass(page === item.key)}">
          <Icon class="w-4 h-4" /><span class="font-medium">{item.label}</span>
        </a>
      {/each}
      {#if $session.configured}
        <hr class="border-gray-200 dark:border-gray-700" />
        <button type="button" onclick={() => { mobileOpen = false; onLogout(); }} disabled={signingOut}
          class="flex items-center gap-3 w-full px-3 py-2.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm">
          <LogOut class="w-4 h-4" /><span class="font-medium">{signingOut ? 'Closing session…' : 'Log out'}</span>
        </button>
      {/if}
      {#if onQuit}
        {#if !$session.configured}<hr class="border-gray-200 dark:border-gray-700" />{/if}
        <button type="button" onclick={() => { mobileOpen = false; onQuit(); }} disabled={signingOut}
          class="flex items-center gap-3 w-full px-3 py-2.5 text-gray-700 dark:text-gray-300 hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-900/20 dark:hover:text-red-400 rounded-lg transition text-sm">
          <Power class="w-4 h-4" /><span class="font-medium">Quit Chiral</span>
        </button>
      {/if}
    </div>
  </nav>
{/if}

<!-- Desktop sidebar -->
<aside class="hidden md:flex fixed top-0 left-0 z-40 h-screen flex-col bg-white dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 shadow-sm transition-[width] duration-200 {collapsed ? 'w-16' : 'w-52'}">
  <div class="flex items-center h-14 px-3 border-b border-gray-200 dark:border-gray-700 shrink-0 overflow-hidden {collapsed ? 'justify-center' : 'justify-between'}">
    <a href="#/marketplace" class="flex items-center gap-2 overflow-hidden no-underline">
      <img src="/logo.png" alt="" class="w-7 h-7 rounded-lg shrink-0" />
      {#if !collapsed}<span class="text-sm font-bold text-gray-900 dark:text-white whitespace-nowrap">Chiral Network</span>{/if}
    </a>
    {#if !collapsed}
      <button type="button" onclick={toggleCollapse} title="Collapse sidebar" aria-label="Collapse sidebar"
        class="p-1 text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-md transition shrink-0">
        <ChevronLeft class="w-4 h-4" />
      </button>
    {/if}
  </div>

  <div class="flex items-center px-3 py-2 border-b border-gray-200 dark:border-gray-700 shrink-0 {collapsed ? 'justify-center' : ''}">
    <ConnectionPill compact={collapsed} />
  </div>

  <nav aria-label="Main navigation" class="flex-1 px-2 py-3 space-y-1 overflow-y-auto overflow-x-hidden">
    {#each navItems as item (item.key)}
      {@const Icon = item.icon}
      <a href={`#/${item.key}`} title={collapsed ? item.label : undefined} aria-current={page === item.key ? 'page' : undefined}
        class="flex items-center gap-3 w-full py-2.5 rounded-lg transition text-sm no-underline {collapsed ? 'justify-center px-0' : 'px-3'} {itemClass(page === item.key)}">
        <Icon class="w-4 h-4 shrink-0" />
        {#if collapsed}<span class="sr-only">{item.label}</span>{:else}<span class="font-medium whitespace-nowrap">{item.label}</span>{/if}
      </a>
    {/each}
  </nav>

  <div class="px-2 pb-3 space-y-1 border-t border-gray-200 dark:border-gray-700 pt-3 shrink-0">
    {#if collapsed}
      <button type="button" onclick={toggleCollapse} title="Expand sidebar" aria-label="Expand sidebar"
        class="flex items-center justify-center w-full py-2 text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm">
        <ChevronRight class="w-4 h-4 shrink-0" />
      </button>
    {/if}
    {#if $session.address && !collapsed}
      <div class="px-3 py-1.5"><span class="text-[11px] font-mono text-gray-500 truncate block" title={$session.address}>{shortAddress($session.address)}</span></div>
    {/if}
    {#if $session.configured}
      <button type="button" onclick={onLogout} disabled={signingOut} title={collapsed ? 'Log out' : undefined}
        class="flex items-center gap-3 w-full py-2 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm disabled:opacity-50 {collapsed ? 'justify-center px-0' : 'px-3'}">
        {#if signingOut}<Loader2 class="w-4 h-4 shrink-0 animate-spin" />{:else}<LogOut class="w-4 h-4 shrink-0" />{/if}
        {#if collapsed}<span class="sr-only">Log out</span>{:else}<span class="font-medium whitespace-nowrap">{signingOut ? 'Closing…' : 'Log out'}</span>{/if}
      </button>
    {/if}
    {#if onQuit}
      <button type="button" onclick={onQuit} disabled={signingOut} title={collapsed ? 'Quit Chiral' : 'Quit Chiral (closing the window keeps it running)'}
        class="flex items-center gap-3 w-full py-2 text-gray-700 dark:text-gray-300 hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-900/20 dark:hover:text-red-400 rounded-lg transition text-sm disabled:opacity-50 {collapsed ? 'justify-center px-0' : 'px-3'}">
        <Power class="w-4 h-4 shrink-0" />
        {#if collapsed}<span class="sr-only">Quit Chiral</span>{:else}<span class="font-medium whitespace-nowrap">Quit</span>{/if}
      </button>
    {/if}
  </div>
</aside>
