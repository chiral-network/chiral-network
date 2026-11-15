<script lang="ts">
  import { command, errorText, formatCfx, type Account } from '$lib/native';
  import { session } from '$lib/stores';
  let accounts = $state<Account[]>([]), error = $state(''), busy = $state(false), loaded = $state(false);
  async function refresh() { busy = true; error = ''; try { accounts = await command<Account[]>('accounts'); loaded = true; } catch (e) { error = errorText(e); } finally { busy = false; } }
</script>
<section><h1>Account</h1><p>Wallet identity: <span class="mono">{$session.address ?? 'No native wallet loaded'}</span></p><p>Provider balances are separate, non-refundable accounts. They cannot be transferred between providers or treated as wallet funds.</p><button disabled={busy || !$session.configured} onclick={refresh}>{busy ? 'Loading…' : 'Load local account index'}</button>{#if error}<p class="error" role="alert">{error}</p>{/if}{#if loaded && !accounts.length}<p class="empty">No accounts in this deployment namespace.</p>{/if}
  {#each accounts as account}<article class="panel"><h2>{account.model.id}</h2><p class="mono">{account.provider_wallet}</p><p>Last recorded status: {account.status}. Available: {formatCfx(account.available_wei)} CFX.</p><p class="mono">{account.contract_id}</p><a href="#/usage">Refresh live balance, usage, receipt, and credentials</a></article>{/each}
  <p><a href="#/wallet">Wallet and private backup</a> · <a href="#/marketplace">Discover providers</a></p>
</section>
