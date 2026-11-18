<script lang="ts">
  import { Network, Palette, Bell, Sun, Moon, Monitor, Check, PanelTop, PanelLeft, FileCog, Loader2, Info } from 'lucide-svelte';
  import { command, errorText, nativeAvailable } from '$lib/native';
  import { settings, session, sessionEpoch, type ThemeMode, type ColorTheme, type NavStyle, type NotificationSettings } from '$lib/stores';
  import { availableThemes } from '$lib/services/colorThemeService';
  import { refreshVersionStatus, resetVersionStatus } from '$lib/stores/versionStore';
  import PageHeader from '$lib/components/ui/PageHeader.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  import { shortAddress } from '$lib/components/ui/tones';
  import { toasts } from '$lib/toastStore';
  let configPath = $state(''), busy = $state(false), error = $state('');
  async function configure() {
    busy = true; error = '';
    try {
      const result = await command<{ address: string | null }>('configure', { path: configPath });
      session.set({ address: result.address, configured: true });
      // Drop the previous network's status before this profile's status arrives.
      resetVersionStatus();
      await refreshVersionStatus();
      sessionEpoch.update(value => value + 1);
      // The session change remounts this page, so the confirmation lives in the app-level
      // notification stack (role="status" for success), not in this instance's state.
      toasts.show('Native configuration loaded. Endpoints and chain identity come only from this explicit profile.', 'success');
    } catch (e) { error = errorText(e); } finally { busy = false; }
  }
  // ---- presentation only ----
  type Section = 'profile' | 'appearance' | 'notifications';
  let active = $state<Section>('profile');
  const sections = [
    { id: 'profile' as const, label: 'Network profile', icon: Network },
    { id: 'appearance' as const, label: 'Appearance', icon: Palette },
    { id: 'notifications' as const, label: 'Notifications', icon: Bell },
  ];
  const themeOptions: { value: ThemeMode; label: string; icon: typeof Sun }[] = [
    { value: 'light', label: 'Light', icon: Sun }, { value: 'dark', label: 'Dark', icon: Moon }, { value: 'system', label: 'System', icon: Monitor },
  ];
  const navOptions: { value: NavStyle; label: string; icon: typeof Sun }[] = [
    { value: 'navbar', label: 'Top Bar', icon: PanelTop }, { value: 'sidebar', label: 'Sidebar', icon: PanelLeft },
  ];
  const notificationOptions: { key: keyof NotificationSettings; label: string; description: string }[] = [
    { key: 'inference', label: 'Inference', description: 'Stream completion, cancellation and embedding results' },
    { key: 'funding', label: 'Funding', description: 'Provider account opened and funding updates' },
    { key: 'networkStatus', label: 'Network status', description: 'Chain and network status changes' },
  ];
  const optionClass = (selected: boolean) => selected
    ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30'
    : 'border-gray-200 dark:border-gray-600 hover:border-gray-300 dark:hover:border-gray-500 bg-gray-50 dark:bg-gray-700/30';
  const setAll = (value: boolean) => settings.update(s => ({ ...s, notifications: { inference: value, funding: value, networkStatus: value } }));
</script>

<section class="page" aria-labelledby="settings-title">
  <PageHeader id="settings-title" title="Settings" subtitle="Native network profile, appearance, and notifications." />

  <div class="grid grid-cols-1 lg:grid-cols-[260px_minmax(0,1fr)] gap-6">
    <aside class="lg:sticky lg:top-20 lg:self-start">
      <nav aria-label="Settings sections" class="card p-2 flex flex-row lg:flex-col gap-1 overflow-x-auto">
        {#each sections as s (s.id)}
          <button type="button" onclick={() => active = s.id} aria-current={active === s.id ? 'true' : undefined}
            class="flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors text-left whitespace-nowrap shrink-0 {active === s.id ? 'bg-primary-100 dark:bg-primary-900/40 text-primary-700 dark:text-primary-300' : 'text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700/60'}">
            <s.icon class="w-4 h-4 shrink-0" />{s.label}
            {#if s.id === 'profile'}<span class="ml-auto w-2 h-2 rounded-full {$session.configured ? 'bg-green-500' : 'bg-amber-500'}" title={$session.configured ? 'Profile loaded' : 'No profile loaded'} aria-hidden="true"></span><span class="sr-only">{$session.configured ? 'Profile loaded' : 'No profile loaded'}</span>{/if}
          </button>
        {/each}
      </nav>
    </aside>

    <div class="min-w-0">
      {#if active === 'profile'}
        <section class="card p-6 space-y-5" aria-labelledby="profile-title">
          <header class="flex items-center gap-3">
            <div class="p-2 bg-primary-100 dark:bg-primary-900/40 rounded-lg"><FileCog class="w-5 h-5 text-primary-600 dark:text-primary-400" /></div>
            <div><h2 id="profile-title" class="text-lg font-semibold text-gray-900 dark:text-white">Explicit native network profile</h2><p class="text-sm text-gray-500 dark:text-gray-400">The JSON consumer configuration used by the CLI.</p></div>
          </header>

          {#if $session.configured}
            <div class="flex items-center gap-3 px-4 py-3 rounded-xl border border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/20 text-sm">
              <span class="w-2 h-2 rounded-full bg-green-500"></span>
              <span class="text-green-800 dark:text-green-300 font-medium">Profile loaded</span>
              {#if $session.address}<span class="ml-auto font-mono text-xs text-green-700 dark:text-green-400" title={$session.address}>{shortAddress($session.address)}</span>{/if}
            </div>
          {/if}

          <p class="prose-muted">It defines <code>network.identity</code> (chain_id, genesis_hash, deployment_id), <code>rpc_url</code>, optional rpc_bearer_file/faucet_url/explorer_url/policy_url, <code>data_dir</code>, <code>catalog_url</code>, and optional DHT configuration. Provider endpoints are selected from native-verified offers, not arbitrary webview URLs.</p>

          <form class="flex flex-col sm:flex-row gap-2 sm:items-end" onsubmit={e => { e.preventDefault(); void configure(); }}>
            <label class="flex-1 min-w-0"><span class="field-label">Consumer configuration file path</span><input class="input font-mono" bind:value={configPath} required autocomplete="off" placeholder="/path/to/consumer.json" /></label>
            <button class="btn-primary py-2.5" disabled={busy || !nativeAvailable()}>
              {#if busy}<Loader2 class="w-4 h-4 animate-spin" />Loading profile…{:else}<FileCog class="w-4 h-4" />Load native configuration{/if}
            </button>
          </form>
          {#if error}<Callout kind="error" role="alert">{error}</Callout>{/if}
          <p class="text-xs text-gray-500 dark:text-gray-400 flex items-start gap-2"><Info class="w-3.5 h-3.5 mt-0.5 shrink-0" />Switching profiles cancels active streams and clears displayed credentials. Wallets, transaction journals, contracts, and bearer files remain scoped by the configured network. No public testnet profile or funding source is invented.</p>
        </section>

      {:else if active === 'appearance'}
        <section class="card p-6" aria-labelledby="appearance-title">
          <header class="flex items-center gap-3 mb-6">
            <div class="p-2 bg-purple-100 dark:bg-purple-900/40 rounded-lg"><Palette class="w-5 h-5 text-purple-600 dark:text-purple-400" /></div>
            <div><h2 id="appearance-title" class="text-lg font-semibold text-gray-900 dark:text-white">Appearance</h2><p class="text-sm text-gray-500 dark:text-gray-400">Theme, accent color, and navigation layout.</p></div>
          </header>

          <fieldset class="mb-6">
            <legend class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Theme</legend>
            <div class="grid grid-cols-3 gap-3">
              {#each themeOptions as option (option.value)}
                {@const selected = $settings.theme === option.value}
                <button type="button" aria-pressed={selected} onclick={() => settings.update(s => ({ ...s, theme: option.value }))}
                  class="relative flex flex-col items-center gap-2 p-4 rounded-lg border-2 transition-all {optionClass(selected)}">
                  {#if selected}<div class="absolute top-2 right-2"><Check class="w-4 h-4 text-primary-500" /></div>{/if}
                  <option.icon class="w-6 h-6 {selected ? 'text-primary-500' : 'text-gray-500 dark:text-gray-400'}" />
                  <span class="text-sm font-medium {selected ? 'text-primary-700 dark:text-primary-300' : 'text-gray-700 dark:text-gray-300'}">{option.label}</span>
                </button>
              {/each}
            </div>
          </fieldset>

          <fieldset class="mb-6">
            <legend class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Accent color</legend>
            <div class="flex flex-wrap gap-3">
              {#each availableThemes as ct (ct.value)}
                {@const selected = $settings.colorTheme === ct.value}
                <button type="button" aria-pressed={selected} onclick={() => settings.update(s => ({ ...s, colorTheme: ct.value as ColorTheme }))}
                  class="relative w-10 h-10 rounded-full transition-all {selected ? 'scale-110 ring-2 ring-offset-2 ring-gray-400 dark:ring-gray-500 dark:ring-offset-gray-800' : 'hover:scale-105'}"
                  style="background-color: {ct.previewHex}" title={ct.label} aria-label="Set accent color to {ct.label}">
                  {#if selected}<Check class="w-5 h-5 text-white absolute inset-0 m-auto drop-shadow" />{/if}
                </button>
              {/each}
            </div>
          </fieldset>

          <fieldset class="mb-6">
            <legend class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Navigation layout</legend>
            <div class="grid grid-cols-2 gap-3 max-w-md">
              {#each navOptions as option (option.value)}
                {@const selected = $settings.navStyle === option.value}
                <button type="button" aria-pressed={selected} onclick={() => settings.update(s => ({ ...s, navStyle: option.value }))}
                  class="relative flex flex-col items-center gap-2 p-4 rounded-lg border-2 transition-all {optionClass(selected)}">
                  {#if selected}<div class="absolute top-2 right-2"><Check class="w-4 h-4 text-primary-500" /></div>{/if}
                  <option.icon class="w-6 h-6 {selected ? 'text-primary-500' : 'text-gray-500 dark:text-gray-400'}" />
                  <span class="text-sm font-medium {selected ? 'text-primary-700 dark:text-primary-300' : 'text-gray-700 dark:text-gray-300'}">{option.label}</span>
                </button>
              {/each}
            </div>
          </fieldset>

          <div class="pt-4 border-t border-gray-200 dark:border-gray-700">
            <button type="button" role="switch" aria-checked={$settings.reducedMotion} onclick={() => settings.update(s => ({ ...s, reducedMotion: !s.reducedMotion }))}
              class="flex w-full max-w-md items-center justify-between gap-3 py-2 text-left">
              <span><span class="block text-sm font-medium text-gray-900 dark:text-white">Reduce motion</span><span class="block text-xs text-gray-500 dark:text-gray-400 mt-0.5">Minimize animations and transitions</span></span>
              <span class="relative w-9 h-5 rounded-full shrink-0 transition-colors {$settings.reducedMotion ? 'bg-primary-500' : 'bg-gray-300 dark:bg-gray-600'}"><span class="absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform {$settings.reducedMotion ? 'translate-x-4' : 'translate-x-0'}"></span></span>
            </button>
          </div>
        </section>

      {:else}
        <section class="card p-6" aria-labelledby="notifications-title">
          <header class="flex flex-wrap items-center justify-between gap-3 mb-4">
            <div class="flex items-center gap-3">
              <div class="p-2 bg-amber-100 dark:bg-amber-900/40 rounded-lg"><Bell class="w-5 h-5 text-amber-600 dark:text-amber-400" /></div>
              <div><h2 id="notifications-title" class="text-lg font-semibold text-gray-900 dark:text-white">Notifications</h2><p class="text-sm text-gray-500 dark:text-gray-400">Choose which event-driven toasts appear.</p></div>
            </div>
            <div class="flex items-center gap-1">
              <button type="button" onclick={() => setAll(true)} class="text-xs px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300">All on</button>
              <button type="button" onclick={() => setAll(false)} class="text-xs px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300">All off</button>
            </div>
          </header>
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-1">
            {#each notificationOptions as option (option.key)}
              {@const enabled = $settings.notifications[option.key]}
              <button type="button" role="switch" aria-checked={enabled} title={option.description}
                onclick={() => settings.update(s => ({ ...s, notifications: { ...s.notifications, [option.key]: !enabled } }))}
                class="flex items-center justify-between gap-3 py-3 px-3 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors text-left">
                <span class="min-w-0 flex-1"><span class="block text-sm font-medium text-gray-900 dark:text-white">{option.label}</span><span class="block text-xs text-gray-500 dark:text-gray-400 mt-0.5 truncate">{option.description}</span></span>
                <span class="relative w-9 h-5 rounded-full shrink-0 transition-colors {enabled ? 'bg-primary-500' : 'bg-gray-300 dark:bg-gray-600'}"><span class="absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform {enabled ? 'translate-x-4' : 'translate-x-0'}"></span></span>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    </div>
  </div>
</section>
