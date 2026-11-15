<script lang="ts">
  import { onMount } from 'svelte';
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
  import { settings, session, sessionEpoch, clearSession } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import { command, errorText, nativeAvailable } from '$lib/native';
  import { initVersionStore, stopVersionStore } from '$lib/stores/versionStore';
  import { applyColorTheme } from '$lib/services/colorThemeService';
  const pages = { marketplace: Marketplace, playground: Playground, provider: Provider, usage: Usage, wallet: Wallet, account: Account, network: Network, settings: Settings, diagnostics: Diagnostics };
  type Page = keyof typeof pages;
  const labels: Record<Page, string> = { marketplace: 'Marketplace', playground: 'Playground', provider: 'Provider', usage: 'Usage & API keys', wallet: 'Wallet', account: 'Account', network: 'Network', settings: 'Settings', diagnostics: 'Diagnostics' };
  function currentRoute(): Page { const name = window.location.hash.replace(/^#\//, ''); return Object.hasOwn(pages, name) ? name as Page : 'marketplace'; }
  let page = $state<Page>(currentRoute()), signingOut = $state(false), closeDialog = $state<HTMLDialogElement>();
  let darkSystem = $state(false), allowClose = false;
  const CurrentPage = $derived(pages[page]);
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
    const navigate = () => { page = currentRoute(); document.getElementById('main')?.focus(); };
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
<svelte:head><title>Chiral — LLM inference testnet</title><meta name="description" content="Verified model discovery and provider-specific prepaid inference." /></svelte:head>
<a href="#main" class="skip-link" onclick={event => { event.preventDefault(); document.getElementById('main')?.focus(); }}>Skip to content</a>
<header><a class="brand" href="#/marketplace">Chiral <span class="badge">TESTNET</span></a><span class="identity">{$session.address ? $session.address.slice(0, 10) + '…' : 'Native wallet not loaded'}</span>{#if $session.configured}<button disabled={signingOut} onclick={logout}>{signingOut ? 'Closing session…' : 'Log out'}</button>{/if}</header>
<div class="shell"><nav aria-label="Main navigation">{#each Object.keys(pages) as name}<a href={`#/${name}`} aria-current={page === name ? 'page' : undefined}>{labels[name as Page]}</a>{/each}</nav>
<main id="main" tabindex="-1">
  {#if !nativeAvailable()}<aside class="warning" role="status"><strong>Browser preview — native functionality unavailable.</strong> Install the desktop app or use the CLI/SDK. This preview does not simulate wallets, providers, payments, credentials, or inference.</aside>{:else if !$session.configured}<aside class="warning">Load your explicit consumer profile in <a href="#/settings">Settings</a>. No network or provider is selected automatically.</aside>{/if}
  {#key `${page}:${$sessionEpoch}`}<CurrentPage />{/key}
</main></div>
<dialog bind:this={closeDialog} aria-labelledby="close-title"><h2 id="close-title">Close Chiral?</h2><p>Active inference streams will be cancelled. Generated tokens can still be charged. Pending funding remains in the native journal for recovery; closing does not refund provider balances.</p><div class="row"><button disabled={signingOut} onclick={() => closeDialog?.close()}>Keep open</button><button disabled={signingOut} onclick={close}>Cancel requests and close</button></div></dialog>
<UpdateGate />
{#each $toasts as toast, index (toast.id)}<Toast {...toast} {index} onClose={() => toasts.remove(toast.id)} />{/each}
