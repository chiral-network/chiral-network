<script lang="ts">
  import { LogOut, Menu, X, ChevronDown, Wallet, Loader2 } from 'lucide-svelte';
  import { session } from '$lib/stores';
  import { navItems, MAX_VISIBLE, type PageKey } from '$lib/navigation';
  import { shortAddress } from '$lib/components/ui/tones';
  import ConnectionPill from './ConnectionPill.svelte';

  let { page, signingOut = false, onLogout }: { page: PageKey; signingOut?: boolean; onLogout: () => void } = $props();

  let mobileMenuOpen = $state(false);
  let moreMenuOpen = $state(false);
  const visibleItems = navItems.slice(0, MAX_VISIBLE);
  const moreItems = navItems.slice(MAX_VISIBLE);
  const isMoreActive = $derived(moreItems.some(item => item.key === page));

  const itemClass = (active: boolean) => active
    ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400'
    : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700';

  function closeMenus() { mobileMenuOpen = false; moreMenuOpen = false; }

  $effect(() => {
    if (!moreMenuOpen) return;
    const onClick = (event: MouseEvent) => { if (!(event.target as HTMLElement).closest('.more-menu-container')) moreMenuOpen = false; };
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') moreMenuOpen = false; };
    document.addEventListener('click', onClick);
    document.addEventListener('keydown', onKey);
    return () => { document.removeEventListener('click', onClick); document.removeEventListener('keydown', onKey); };
  });
</script>

<nav aria-label="Main navigation" class="sticky top-0 z-50 bg-white dark:bg-gray-800 shadow-md border-b border-gray-200 dark:border-gray-700">
  <div class="max-w-7xl mx-auto px-3 sm:px-4">
    <div class="flex items-center justify-between h-14 gap-2">
      <a href="#/marketplace" class="flex items-center gap-2 shrink-0 no-underline" onclick={closeMenus}>
        <img src="/logo.png" alt="" class="w-7 h-7 rounded-lg" />
        <span class="text-lg font-bold text-gray-900 dark:text-white hidden sm:inline">Chiral Network</span>
        <span class="px-1.5 py-0.5 rounded bg-amber-100 dark:bg-amber-900/40 text-amber-800 dark:text-amber-300 text-[10px] font-semibold tracking-wider">TESTNET</span>
      </a>

      <!-- md–xl: icon-only, every item -->
      <div class="hidden md:flex xl:hidden items-center gap-0.5 shrink-0">
        {#each navItems as item (item.key)}
          {@const Icon = item.icon}
          <a href={`#/${item.key}`} title={item.label} aria-current={page === item.key ? 'page' : undefined}
            class="flex items-center px-2 py-1.5 rounded-lg transition text-sm no-underline {itemClass(page === item.key)}">
            <Icon class="w-4 h-4" /><span class="sr-only">{item.label}</span>
          </a>
        {/each}
      </div>

      <!-- xl+: labels + More dropdown -->
      <div class="hidden xl:flex items-center gap-0.5 shrink-0">
        {#each visibleItems as item (item.key)}
          {@const Icon = item.icon}
          <a href={`#/${item.key}`} title={item.label} aria-current={page === item.key ? 'page' : undefined}
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg transition text-sm whitespace-nowrap no-underline {itemClass(page === item.key)}">
            <Icon class="w-4 h-4 shrink-0" /><span class="font-medium">{item.short}</span>
          </a>
        {/each}
        {#if moreItems.length}
          <div class="relative more-menu-container">
            <button type="button" onclick={() => moreMenuOpen = !moreMenuOpen} aria-expanded={moreMenuOpen} aria-haspopup="true"
              class="flex items-center gap-1 px-3 py-1.5 rounded-lg transition text-sm whitespace-nowrap {itemClass(isMoreActive)}">
              <span class="font-medium">More</span>
              <ChevronDown class="w-4 h-4 transition-transform {moreMenuOpen ? 'rotate-180' : ''}" />
            </button>
            {#if moreMenuOpen}
              <div class="absolute right-0 top-full mt-1 w-48 bg-white dark:bg-gray-800 rounded-lg shadow-lg border border-gray-200 dark:border-gray-700 py-1 z-50">
                {#each moreItems as item (item.key)}
                  {@const Icon = item.icon}
                  <a href={`#/${item.key}`} onclick={closeMenus} aria-current={page === item.key ? 'page' : undefined}
                    class="flex items-center gap-2.5 w-full px-3 py-2 text-sm transition no-underline {itemClass(page === item.key)}">
                    <Icon class="w-4 h-4" /><span class="font-medium">{item.label}</span>
                  </a>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <div class="flex items-center gap-2 sm:gap-3 shrink-0">
        {#if $session.address}
          <span class="hidden md:inline-flex items-center gap-1 px-2 py-1 bg-gray-100 dark:bg-gray-700 rounded-lg text-xs font-mono text-gray-600 dark:text-gray-400" title={$session.address}>
            <Wallet class="w-3 h-3" />{shortAddress($session.address)}
          </span>
        {/if}
        {#if $session.configured}
          <button type="button" onclick={onLogout} disabled={signingOut} title="Close the native session"
            class="hidden sm:flex items-center gap-1.5 px-3 py-1.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm disabled:opacity-50">
            {#if signingOut}<Loader2 class="w-4 h-4 animate-spin" />{:else}<LogOut class="w-4 h-4" />{/if}
            <span class="hidden lg:inline">{signingOut ? 'Closing session…' : 'Log out'}</span>
          </button>
        {/if}
        <ConnectionPill />
        <button type="button" onclick={() => mobileMenuOpen = !mobileMenuOpen} aria-expanded={mobileMenuOpen} aria-label="Toggle navigation menu"
          class="md:hidden p-1.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition">
          {#if mobileMenuOpen}<X class="w-5 h-5" />{:else}<Menu class="w-5 h-5" />{/if}
        </button>
      </div>
    </div>
  </div>

  {#if mobileMenuOpen}
    <div class="md:hidden border-t border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 max-h-[calc(100vh-3.5rem)] overflow-y-auto">
      <div class="px-3 py-2 space-y-1">
        {#each navItems as item (item.key)}
          {@const Icon = item.icon}
          <a href={`#/${item.key}`} onclick={closeMenus} aria-current={page === item.key ? 'page' : undefined}
            class="flex items-center gap-3 w-full px-3 py-2.5 rounded-lg transition text-sm no-underline {itemClass(page === item.key)}">
            <Icon class="w-4 h-4" /><span class="font-medium">{item.label}</span>
          </a>
        {/each}
        {#if $session.configured}
          <hr class="border-gray-200 dark:border-gray-700" />
          <button type="button" onclick={() => { closeMenus(); onLogout(); }} disabled={signingOut}
            class="flex items-center gap-3 w-full px-3 py-2.5 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition text-sm">
            <LogOut class="w-4 h-4" /><span class="font-medium">{signingOut ? 'Closing session…' : 'Log out'}</span>
          </button>
        {/if}
      </div>
    </div>
  {/if}
</nav>
