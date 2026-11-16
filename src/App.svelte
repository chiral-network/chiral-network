<script lang="ts">
  import { onMount } from 'svelte';
  import { AlertTriangle, Loader2 } from 'lucide-svelte';
  import Marketplace from './pages/Marketplace.svelte';
  import Playground from './pages/Playground.svelte';
  import Provider from './pages/Provider.svelte';
  import Usage from './pages/Usage.svelte';
  import Wallet from './pages/Wallet.svelte';
  import Account from './pages/Account.svelte';
  import Network from './pages/Network.svelte';
  import Settings from './pages/Settings.svelte';
  import Diagnostics from './pages/Diagnostics.svelte';
  import Toast from '$lib/components/Toast.svelte';
  import UpdateGate from '$lib/components/UpdateGate.svelte';
  import Navbar from '$lib/components/Navbar.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  import { settings, session, sessionEpoch, clearSession } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import { command, errorText, nativeAvailable } from '$lib/native';
  import { initVersionStore, stopVersionStore } from '$lib/stores/versionStore';
  import { applyColorTheme } from '$lib/services/colorThemeService';
  import { navItems, type PageKey } from '$lib/navigation';
  const pages: Record<PageKey, typeof Marketplace> = { marketplace: Marketplace, playground: Playground, provider: Provider, usage: Usage, wallet: Wallet, account: Account, network: Network, settings: Settings, diagnostics: Diagnostics };
  type Page = PageKey;
  function currentRoute(): Page { const name = window.location.hash.replace(/^#\//, ''); return Object.hasOwn(pages, name) ? name as Page : 'marketplace'; }
  let page = $state<Page>(currentRoute()), signingOut = $state(false), closeDialog = $state<HTMLDialogElement>();
  let darkSystem = $state(false), allowClose = false, sidebarCollapsed = $state(false);
  const CurrentPage = $derived(pages[page]);
  const pageLabel = $derived(navItems.find(item => item.key === page)?.label ?? 'Marketplace');
  $effect(() => {
    document.documentElement.classList.toggle('dark', $settings.theme === 'dark' || ($settings.theme === 'system' && darkSystem));
    document.documentElement.classList.toggle('reduce-motion', $settings.reducedMotion);
    applyColorTheme($settings.colorTheme);
  });
  async function logout() {
    signingOut = true;
    try { await command('logout'); clearSession(); window.location.hash = '/settings'; toasts.show('Native session closed; active requests cancelled. Wallet files remain on disk.', 'info'); }
    catch (e) { toasts.show(errorText(e), 'error'); }
    finally { signingOut = false; }
  }
  async function close() {
    signingOut = true;
    try {
      await command('logout'); clearSession(); allowClose = true;
      const { getCurrentWindow } = await import('@tauri-apps/api/window'); await getCurrentWindow().destroy();
    } catch (e) { allowClose = false; toasts.show(errorText(e), 'error'); }
    finally { signingOut = false; }
  }
  onMount(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)'); darkSystem = media.matches;
    const changeTheme = () => darkSystem = media.matches;
    const navigate = () => { page = currentRoute(); window.scrollTo({ top: 0 }); document.getElementById('main')?.focus({ preventScroll: true }); };
    media.addEventListener('change', changeTheme); window.addEventListener('hashchange', navigate);
    let disposed = false, unlisten: (() => void) | undefined;
    if (nativeAvailable()) {
      initVersionStore();
      void import('@tauri-apps/api/window').then(async ({ getCurrentWindow }) => {
        const stop = await getCurrentWindow().onCloseRequested(event => { if (!allowClose) { event.preventDefault(); closeDialog?.showModal(); } });
        if (disposed) stop(); else unlisten = stop;
      }).catch(e => toasts.show(errorText(e), 'error'));
    }
    return () => { disposed = true; unlisten?.(); stopVersionStore(); media.removeEventListener('change', changeTheme); window.removeEventListener('hashchange', navigate); };
  });
</script>
<svelte:head><title>{pageLabel} | Chiral Network</title><meta name="description" content="Verified model discovery and provider-specific prepaid inference." /></svelte:head>
<a href="#main" class="skip-link" onclick={event => { event.preventDefault(); document.getElementById('main')?.focus(); }}>Skip to content</a>

{#if $settings.navStyle === 'sidebar'}
  <Sidebar {page} bind:collapsed={sidebarCollapsed} {signingOut} onLogout={logout} />
{:else}
  <Navbar {page} {signingOut} onLogout={logout} />
{/if}

<main id="main" tabindex="-1" class="focus:outline-none focus-visible:ring-0 transition-[margin] duration-200 {$settings.navStyle === 'sidebar' ? (sidebarCollapsed ? 'md:ml-16' : 'md:ml-52') : ''}">
  {#if !nativeAvailable() || !$session.configured}
    <div class="max-w-[1400px] mx-auto px-4 sm:px-6 pt-4 sm:pt-6 -mb-2 sm:-mb-4">
      {#if !nativeAvailable()}
        <Callout kind="warning" role="status" title="Browser preview — native functionality unavailable.">
          Install the desktop app or use the CLI/SDK. This preview does not simulate wallets, providers, payments, credentials, or inference.
        </Callout>
      {:else}
        <Callout kind="warning" title="No network selected yet.">
          Load your explicit consumer profile in <a class="link" href="#/settings">Settings</a>. No network or provider is selected automatically.
        </Callout>
      {/if}
    </div>
  {/if}
  {#key `${page}:${$sessionEpoch}`}<CurrentPage />{/key}
</main>

<dialog bind:this={closeDialog} aria-labelledby="close-title" class="w-full max-w-md rounded-2xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 p-6 shadow-2xl">
  <div class="flex items-center gap-3 mb-3">
    <div class="p-2 bg-amber-100 dark:bg-amber-900/40 rounded-lg"><AlertTriangle class="w-5 h-5 text-amber-600 dark:text-amber-400" /></div>
    <h2 id="close-title" class="text-lg font-semibold text-gray-900 dark:text-white">Close Chiral?</h2>
  </div>
  <p class="text-sm text-gray-600 dark:text-gray-400 mb-5 sm:ml-[52px]">Active inference streams will be cancelled. Generated tokens can still be charged. Pending funding remains in the native journal for recovery; closing does not refund provider balances.</p>
  <div class="flex flex-wrap gap-3 justify-end">
    <button type="button" class="btn-secondary" disabled={signingOut} onclick={() => closeDialog?.close()}>Keep open</button>
    <button type="button" class="btn-danger" disabled={signingOut} onclick={close}>
      {#if signingOut}<Loader2 class="w-4 h-4 animate-spin" />{/if}Cancel requests and close
    </button>
  </div>
</dialog>
<UpdateGate />
{#each $toasts as toast, index (toast.id)}<Toast {...toast} {index} onClose={() => toasts.remove(toast.id)} />{/each}
