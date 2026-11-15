<script lang="ts">
  import { command, errorText } from '$lib/native';
  import { session } from '$lib/stores';
  import Result from '$lib/components/Result.svelte';
  let endpoint = $state('http://127.0.0.1:9421'), tokenFile = $state('');
  let result = $state<unknown>(null), busy = $state(false), error = $state(''), actionName = $state('');
  const actions = [
    ['status', 'Readiness & status'], ['offers', 'Published offers'], ['accounts', 'Prepaid liabilities'],
    ['requests', 'Request diagnostics'], ['fees', 'Fee remittance'], ['drain', 'Drain new requests'],
    ['resume', 'Resume admission'], ['refresh', 'Probe & refresh offers'], ['remit', 'Run fee outbox'],
  ];
  async function admin(action: string) {
    busy = true; error = ''; result = null; actionName = action;
    try { result = await command('provider_admin', { endpoint, token_file: tokenFile, action }); }
    catch (e) { error = errorText(e); } finally { busy = false; }
  }
</script>
<section><h1>Provider dashboard</h1><p>Administer a Linux GPU provider through its authenticated, literal-loopback operator listener. For a remote provider, establish an SSH tunnel first. The public inference endpoint never exposes these controls.</p>
  <section class="panel"><label>Operator endpoint<input bind:value={endpoint} type="url" required /></label><label>Native admin-token file path<input bind:value={tokenFile} autocomplete="off" required /></label><p>The native consumer reads the protected token file; the token is never returned to this UI.</p></section>
  <div class="row">{#each actions as [action, label]}<button disabled={busy || !tokenFile || !$session.configured} onclick={() => admin(action)}>{label}</button>{/each}</div>
  {#if busy}<p role="status">Requesting {actionName}…</p>{/if}{#if error}<p class="error" role="alert">{error}</p>{/if}<Result value={result} title={`Operator ${actionName || 'response'}`} />
  <p class="warning">Engine-failed / not-ready models must not admit paid requests. Drain before changing the provider's native configuration, then restart and probe. Existing account prices and model revisions remain pinned. This dashboard does not install GPU runtimes or expose a shell.</p>
</section>
