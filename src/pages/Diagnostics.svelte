<script lang="ts">
  import { command, errorText, nativeAvailable } from '$lib/native';
  import Result from '$lib/components/Result.svelte';
  let value = $state<unknown>(null), error = $state(''), busy = $state(false);
  async function inspect() { busy = true; error = ''; try { value = await command('diagnostics'); } catch (e) { error = errorText(e); } finally { busy = false; } }
</script>
<section><h1>Diagnostics</h1><p>Native runtime: {nativeAvailable() ? 'available' : 'unavailable (browser preview)'}. No synthetic providers, balances, or success responses are supplied.</p><button disabled={busy || !nativeAvailable()} onclick={inspect}>{busy ? 'Collecting…' : 'Collect native diagnostics'}</button>{#if error}<p class="error" role="alert">{error}</p>{/if}<Result {value} title="Runtime and network status" /><p>For request billing, use Usage or the playground request-status action. For engine readiness, queue failures and fee-remittance liabilities, use the authenticated Provider dashboard. Diagnostics do not contain wallet keys, API bearer secrets, prompts, or generated text.</p></section>
