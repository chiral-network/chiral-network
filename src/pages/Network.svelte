<script lang="ts">
  import { command, errorText } from '$lib/native';
  import { session } from '$lib/stores';
  import Result from '$lib/components/Result.svelte';
  let value = $state<unknown>(null), busy = $state(false), error = $state('');
  async function refresh() { busy = true; error = ''; try { value = await command('network_status'); } catch (e) { error = errorText(e); } finally { busy = false; } }
</script>
<section><h1>Network</h1><p>Native CFX testnet settlement pins Conflux eSpace chain 71, its genesis hash and an explicit deployment namespace. There is no legacy-chain fallback and no background node or miner startup.</p><button disabled={busy || !$session.configured} onclick={refresh}>{busy ? 'Checking identity and frontier…' : 'Check configured network'}</button>{#if error}<p class="error" role="alert">{error}</p>{/if}<Result {value} title="Native chain identity and settlement status" /><p class="warning">First execution is not spendable credit. Funding requires a canonical successful receipt at the safe frontier; finalized is reported separately. Syncing, stale or unavailable RPC nodes withhold new funding. Proven orphaned credits may freeze an account; a missing receipt alone does not prove a reversal. Existing settled balances can remain usable during temporary RPC outages.</p><a href="#/settings">Configure network, catalog, and DHT endpoints</a></section>
