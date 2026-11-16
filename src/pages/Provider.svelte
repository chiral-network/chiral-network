<script lang="ts">
  import { Server, Plug, Activity, Tags, Wallet, ListChecks, Receipt, PauseCircle, PlayCircle, RefreshCw, Send, Loader2, Wrench, Lock } from 'lucide-svelte';
  import { command, errorText } from '$lib/native';
  import { session } from '$lib/stores';
  import Result from '$lib/components/Result.svelte';
  import PageHeader from '$lib/components/ui/PageHeader.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Callout from '$lib/components/ui/Callout.svelte';
  import type { IconComponent } from '$lib/components/ui/tones';
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
  // ---- presentation only ----
  const meta: Record<string, { icon: IconComponent; hint: string; tone: string }> = {
    status: { icon: Activity, hint: 'Engine, admission and listener health', tone: 'text-green-600 dark:text-green-400' },
    offers: { icon: Tags, hint: 'Signed offers currently advertised', tone: 'text-primary-600 dark:text-primary-400' },
    accounts: { icon: Wallet, hint: 'Consumer balances you owe service for', tone: 'text-purple-600 dark:text-purple-400' },
    requests: { icon: ListChecks, hint: 'Recent request outcomes and failures', tone: 'text-sky-600 dark:text-sky-400' },
    fees: { icon: Receipt, hint: 'Platform-fee obligations and remittances', tone: 'text-orange-600 dark:text-orange-400' },
    drain: { icon: PauseCircle, hint: 'Stop admitting new paid requests', tone: 'text-amber-600 dark:text-amber-400' },
    resume: { icon: PlayCircle, hint: 'Re-open admission after a drain', tone: 'text-green-600 dark:text-green-400' },
    refresh: { icon: RefreshCw, hint: 'Re-probe models and republish offers', tone: 'text-primary-600 dark:text-primary-400' },
    remit: { icon: Send, hint: 'Submit pending platform-fee payments', tone: 'text-orange-600 dark:text-orange-400' },
  };
  const label = $derived(actions.find(([key]) => key === actionName)?.[1] ?? 'response');
</script>

<section class="page" aria-labelledby="provider-title">
  <PageHeader id="provider-title" title="Provider dashboard" subtitle="Administer a Linux GPU provider through its authenticated, literal-loopback operator listener." />

  <Callout kind="info">For a remote provider, establish an SSH tunnel first. The public inference endpoint never exposes these controls.</Callout>

  <div class="grid grid-cols-1 xl:grid-cols-[minmax(0,420px)_minmax(0,1fr)] gap-4 items-start">
    <Card icon={Plug} title="Operator connection" subtitle="Loopback listener and protected admin token">
      <div class="space-y-4">
        <label class="block"><span class="field-label">Operator endpoint</span><input class="input font-mono" bind:value={endpoint} type="url" required /></label>
        <label class="block"><span class="field-label">Native admin-token file path</span><input class="input font-mono" bind:value={tokenFile} autocomplete="off" required placeholder="/path/to/operator-token" /></label>
        <p class="field-hint flex items-start gap-1.5"><Lock class="w-3.5 h-3.5 mt-0.5 shrink-0" />The native consumer reads the protected token file; the token is never returned to this UI.</p>
      </div>
    </Card>

    <Card icon={Wrench} tone="purple" title="Operator actions" subtitle={tokenFile ? 'Each action calls the operator listener once' : 'Enter the admin-token file path to enable'}>
      <div class="grid grid-cols-1 sm:grid-cols-2 2xl:grid-cols-3 gap-2">
        {#each actions as [action, text]}
          {@const m = meta[action]}
          <button type="button" disabled={busy || !tokenFile || !$session.configured} onclick={() => admin(action)}
            class="group flex items-center gap-3 text-left px-3 py-2.5 rounded-xl border transition-colors disabled:opacity-50 disabled:cursor-not-allowed {actionName === action && (busy || result || error) ? 'border-primary-400 bg-primary-50 dark:bg-primary-900/20 dark:border-primary-700' : 'border-gray-200 dark:border-gray-700 enabled:hover:border-gray-300 enabled:hover:bg-gray-50 dark:enabled:hover:bg-gray-700/50'}">
            <span class="p-2 rounded-lg bg-gray-100 dark:bg-gray-700 shrink-0 {m.tone}">
              {#if busy && actionName === action}<Loader2 class="w-4 h-4 animate-spin" />{:else}<m.icon class="w-4 h-4" />{/if}
            </span>
            <span class="min-w-0">
              <span class="block text-sm font-medium text-gray-900 dark:text-white">{text}</span>
              <span class="block text-xs text-gray-500 dark:text-gray-400 truncate">{m.hint}</span>
            </span>
          </button>
        {/each}
      </div>
    </Card>
  </div>

  {#if busy}<p role="status" class="flex items-center gap-2 text-sm text-gray-600 dark:text-gray-400"><Loader2 class="w-4 h-4 animate-spin" />Requesting {actionName}…</p>{/if}
  {#if error}<Callout kind="error" role="alert">{error}</Callout>{/if}
  <Result value={result} title={`Operator ${actionName ? label.toLowerCase() : 'response'}`} />

  <Callout kind="warning" title="Change control">Engine-failed / not-ready models must not admit paid requests. Drain before changing the provider's native configuration, then restart and probe. Existing account prices and model revisions remain pinned. This dashboard does not install GPU runtimes or expose a shell.</Callout>
  <p class="text-xs text-gray-500 dark:text-gray-400 flex items-center gap-1.5"><Server class="w-3.5 h-3.5" />Provider daemons are run with <code>chiral_daemon --config &lt;provider.json&gt;</code>.</p>
</section>
