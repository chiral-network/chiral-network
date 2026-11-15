<script lang="ts">
  import { command, errorText, nativeAvailable } from '$lib/native';
  import { settings, session, sessionEpoch, type ThemeMode, type ColorTheme } from '$lib/stores';
  import { availableThemes } from '$lib/services/colorThemeService';
  import { refreshVersionStatus } from '$lib/stores/versionStore';
  let configPath = $state(''), busy = $state(false), error = $state(''), notice = $state('');
  async function configure() {
    busy = true; error = ''; notice = '';
    try {
      const result = await command<{ address: string | null }>('configure', { path: configPath });
      session.set({ address: result.address, configured: true });
      await refreshVersionStatus();
      sessionEpoch.update(value => value + 1);
      notice = 'Native configuration loaded. Endpoints and chain identity come only from this explicit profile.';
    } catch (e) { error = errorText(e); } finally { busy = false; }
  }
</script>
<section><h1>Settings</h1><section class="panel"><h2>Explicit native network profile</h2><p>Choose the JSON consumer configuration used by the CLI. It defines network.identity (chain_id, genesis_hash, deployment_id), rpc_url, optional rpc_bearer_file/faucet_url/explorer_url/policy_url, data_dir, catalog_url, and optional DHT configuration. Provider endpoints are selected from native-verified offers, not arbitrary webview URLs.</p>
  <form onsubmit={e => { e.preventDefault(); void configure(); }}><label>Consumer configuration file path<input bind:value={configPath} required autocomplete="off" placeholder="/path/to/consumer.json" /></label><button disabled={busy || !nativeAvailable()}>{busy ? 'Loading profile…' : 'Load native configuration'}</button></form>
  <p>Switching profiles cancels active streams and clears displayed credentials. Wallets, transaction journals, contracts, and bearer files remain scoped by the configured network. No public testnet profile or funding source is invented.</p>
  {#if error}<p class="error" role="alert">{error}</p>{/if}{#if notice}<p class="success" role="status">{notice}</p>{/if}
</section><section class="panel"><h2>Appearance</h2><label>Theme<select value={$settings.theme} onchange={e => settings.update(s => ({ ...s, theme: e.currentTarget.value as ThemeMode }))}><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></label><label>Accent<select value={$settings.colorTheme} onchange={e => settings.update(s => ({ ...s, colorTheme: e.currentTarget.value as ColorTheme }))}>{#each availableThemes as theme}<option value={theme.value}>{theme.label}</option>{/each}</select></label><label class="check"><input type="checkbox" checked={$settings.reducedMotion} onchange={e => settings.update(s => ({ ...s, reducedMotion: e.currentTarget.checked }))} />Reduce motion</label></section>
<section class="panel"><h2>Notifications</h2>{#each ['inference', 'funding', 'networkStatus'] as key}<label class="check"><input type="checkbox" checked={$settings.notifications[key as keyof typeof $settings.notifications]} onchange={e => settings.update(s => ({ ...s, notifications: { ...s.notifications, [key]: e.currentTarget.checked } }))} />{key}</label>{/each}</section>
</section>
