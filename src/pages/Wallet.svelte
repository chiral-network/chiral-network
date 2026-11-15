<script lang="ts">
  import { command, errorText } from '$lib/native';
  import { session } from '$lib/stores';
  import Result from '$lib/components/Result.svelte';
  let importPath = $state(''), backupPath = $state(''), busy = $state(false), error = $state(''), notice = $state('');
  let balance = $state<unknown>(null), history = $state<unknown>(null);
  let profile = $state<unknown>(null);
  async function run(operation: string, args: Record<string, unknown> = {}) {
    busy = true; error = ''; notice = '';
    try {
      const result = await command(operation, args);
      if (operation === 'wallet_create' || operation === 'wallet_import') { session.update(s => ({ ...s, address: String(result) })); notice = 'Wallet ready. Signing keys stay in protected native storage.'; }
      if (operation === 'balance') balance = result;
      if (operation === 'history') history = result;
      if (operation === 'network_profile') profile = result;
      if (operation === 'wallet_export') notice = 'Private backup file written with owner-only permissions. Chiral does not encrypt this file; keep it offline on encrypted storage.';
    } catch (e) { error = errorText(e); } finally { busy = false; }
  }
</script>
<section><h1>Wallet</h1><p>Wallet keys never enter JavaScript. Generate a new wallet or import a protected key file using native commands. This wallet uses Conflux eSpace's hex address format. Explicit key import does not convert old CHI or CHR balances into CFX.</p>
  {#if !$session.configured}<p class="warning">Load an explicit network/consumer configuration in <a href="#/settings">Settings</a> first.</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}{#if notice}<p class="success" role="status">{notice}</p>{/if}
  {#if $session.address}<section class="panel"><h2>Public address</h2><p class="mono">{$session.address}</p><div class="row"><button disabled={busy} onclick={() => run('balance')}>Refresh verified-network balance</button><button disabled={busy} onclick={() => run('history')}>Load local transaction journal</button></div></section>
    <section class="panel"><h2>Back up wallet</h2><p>The native command copies the private wallet key to a new, owner-only file. It is not encrypted by Chiral: choose an encrypted offline volume. Never upload this file.</p><form onsubmit={e => { e.preventDefault(); void run('wallet_export', { path: backupPath }); }}><label>New backup file path<input bind:value={backupPath} required autocomplete="off" /></label><button disabled={busy || !backupPath}>Export private backup to file</button></form></section>
  {:else}<section class="panel"><h2>Set up wallet</h2><button disabled={busy || !$session.configured} onclick={() => run('wallet_create')}>Generate native wallet</button><form onsubmit={e => { e.preventDefault(); void run('wallet_import', { path: importPath }); }}><label>Protected private-key file path<input bind:value={importPath} required autocomplete="off" /></label><button disabled={busy || !$session.configured || !importPath}>Import key file</button></form></section>{/if}
  <Result value={balance} title="Network wallet balance" /><p>Transaction history covers this native consumer's durable submitted-transaction journal, not an invented complete chain history. Pending, rejected, and orphaned transactions are not spendable provider credit.</p><Result value={history} title="Local submitted transactions" />
  <section class="panel"><h2>eSpace testnet funding</h2><p>Request testnet CFX for the public hex address above using the configured Conflux eSpace faucet. Reserve enough for network gas as well as small provider prepayments. Never send a wallet backup or private key to a faucet. No faucet authority is embedded in this app.</p><button disabled={busy || !$session.configured} onclick={() => run('network_profile')}>Show configured faucet and explorer</button><Result value={profile} title="Explicit eSpace profile (public endpoints only)" /><p>Mining pools normally pay Core Space addresses. Core and eSpace balances are separate: use an explicit native Core-to-eSpace transfer to fund an eSpace wallet. Changing an address's text is not a transfer. Testnet funds have no mainnet value.</p></section>
</section>
