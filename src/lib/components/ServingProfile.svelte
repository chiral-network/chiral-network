<script lang="ts">
  import { Gauge, Cpu, Layers, MapPin, Info, ChevronRight, ShieldCheck } from 'lucide-svelte';
  import type { ModelSpec } from '$lib/native';
  // `compact` folds the qualification/remedy terms into a disclosure so a
  // grid of offers stays scannable; the full text is one click away.
  let { model, compact = false }: { model: ModelSpec; compact?: boolean } = $props();
  const profile = $derived(model.serving);
</script>

{#snippet terms()}
  <p>Qualification requires at least 95% joint attainment of time to first token ≤2 seconds and the decode target on warmed generation, with prompts ≤2,048 tokens and a 256-token output workload within a 4,096-token workload envelope. Capacity alone does not establish speed. Provider-observed timing does not guarantee Internet delivery latency.</p>
  <p>Eligible performance misses waive this request's inference charge. Request records identify eligibility, measured timing and remedy; funding fees, network gas and the non-refundable prepayment policy are unchanged. Client cancellation/backpressure and requests outside the agreed workload have separate outcomes.</p>
{/snippet}

{#if profile}
  <section aria-label="Provider performance commitment" class="subpanel p-4 space-y-3">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex items-center gap-2 text-sm font-semibold text-gray-900 dark:text-white">
        <Gauge class="w-4 h-4 text-primary-600 dark:text-primary-400" />Provider-qualified target
      </div>
      <span class="badge-primary text-sm"><strong>{profile.output_tokens_per_second}</strong>&nbsp;output tokens/sec per admitted request</span>
    </div>

    <dl class="grid grid-cols-1 sm:grid-cols-2 gap-2">
      <div class="rounded-lg bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 px-3 py-2">
        <dt class="stat-label flex items-center gap-1"><Layers class="w-3 h-3" />Capacity tier</dt>
        <dd class="text-sm font-medium text-gray-900 dark:text-white">{profile.capacity_tier === 'higher' ? '48–80 GB+ / local multi-GPU' : profile.capacity_tier.toUpperCase()}</dd>
      </div>
      <div class="rounded-lg bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 px-3 py-2">
        <dt class="stat-label flex items-center gap-1"><Gauge class="w-3 h-3" />Admitted load</dt>
        <dd class="text-sm font-medium text-gray-900 dark:text-white">{profile.max_concurrency} concurrent requests, {profile.arrival_milli_rps / 1000} requests/sec</dd>
      </div>
      <div class="rounded-lg bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 px-3 py-2">
        <dt class="stat-label flex items-center gap-1"><Cpu class="w-3 h-3" />Engine / quantization</dt>
        <dd class="text-sm font-medium text-gray-900 dark:text-white break-words">{profile.engine} {profile.engine_version} · {profile.platform}/{profile.backend} · {model.quantization ?? 'unquantized'}</dd>
      </div>
      <div class="rounded-lg bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 px-3 py-2">
        <dt class="stat-label flex items-center gap-1"><MapPin class="w-3 h-3" />Location</dt>
        <dd class="text-sm font-medium text-gray-900 dark:text-white">{profile.region}</dd>
      </div>
      <div class="sm:col-span-2 rounded-lg bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 px-3 py-2">
        <dt class="stat-label flex items-center gap-1"><Cpu class="w-3 h-3" />Measured hardware</dt>
        <dd class="text-sm text-gray-900 dark:text-white">
          {#each profile.devices as device}
            <div class="font-medium">{device.name} · {(Number(device.memory_mib) / 1024).toFixed(1)} GiB ({device.engine_device})</div>
          {/each}
          <div class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
            {profile.memory_kind === 'unified' ? 'Shared unified memory, counted once' : 'Dedicated device memory'};
            {(Number(profile.memory_reserve_mib) / 1024).toFixed(2)} GiB reserved per device
          </div>
        </dd>
      </div>
    </dl>

    {#if compact}
      <details class="group text-xs text-gray-600 dark:text-gray-400">
        <summary class="flex items-center gap-1 font-medium text-gray-700 dark:text-gray-300 list-none [&::-webkit-details-marker]:hidden">
          <ChevronRight class="w-3.5 h-3.5 transition-transform group-open:rotate-90" />Qualification & remedy terms
        </summary>
        <div class="mt-2 space-y-2 leading-relaxed">{@render terms()}</div>
      </details>
    {:else}
      <div class="space-y-2 text-xs text-gray-600 dark:text-gray-400 leading-relaxed">{@render terms()}</div>
    {/if}

    <details class="group text-xs">
      <summary class="flex items-center gap-1 font-medium text-gray-700 dark:text-gray-300 list-none [&::-webkit-details-marker]:hidden">
        <ChevronRight class="w-3.5 h-3.5 transition-transform group-open:rotate-90" /><ShieldCheck class="w-3.5 h-3.5" />Signed qualification provenance
      </summary>
      <dl class="kv mt-2 text-xs">
        <dt>HF model / tokenizer</dt><dd>{profile.hf_repo} / {profile.tokenizer_repo}</dd>
        <dt>Model revision</dt><dd class="mono">{model.revision}</dd>
        <dt>Tokenizer revision</dt><dd class="mono">{model.tokenizer_revision}</dd>
        <dt>Evidence date</dt><dd>{new Date(Number(profile.qualification.measured_at) * 1000).toLocaleString()}</dd>
        <dt>Evidence SHA-256</dt><dd class="mono">{profile.qualification.evidence_sha256}</dd>
        <dt>Artifact SHA-256</dt><dd class="mono">{profile.artifact_manifest_sha256}</dd>
        <dt>Native runtime SHA-256</dt><dd class="mono">{profile.runtime_sha256}</dd>
        <dt>Execution settings</dt><dd>{profile.gpu_layers} GPU layers · {profile.split_mode} split · KV {profile.cache_type_k}/{profile.cache_type_v} · batch {profile.batch_size}/{profile.ubatch_size} · flash attention {profile.flash_attention ? 'on' : 'off'}</dd>
      </dl>
      <p class="mt-2 text-gray-500 dark:text-gray-400">These are attributable provider measurements, not independent model-execution or hardware attestations.</p>
    </details>
  </section>
{:else}
  <p class="flex items-start gap-2 text-xs text-gray-500 dark:text-gray-400"><Info class="w-3.5 h-3.5 mt-0.5 shrink-0" />No generation-speed guarantee applies to this embedding or local calibration profile.</p>
{/if}
