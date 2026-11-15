<script lang="ts">
  import type { ModelSpec } from '$lib/native';
  let { model }: { model: ModelSpec } = $props();
  const profile = $derived(model.serving);
</script>

{#if profile}
  <section aria-label="Provider performance commitment">
    <p><strong>Provider-qualified target: {profile.output_tokens_per_second} output tokens/sec per admitted request</strong></p>
    <dl>
      <dt>Capacity tier</dt><dd>{profile.capacity_tier === 'higher' ? '48–80 GB+ / local multi-GPU' : profile.capacity_tier.toUpperCase()}</dd>
      <dt>Measured hardware</dt>
      <dd>
        {#each profile.devices as device}
          <div>{device.name} · {(device.memory_mib / 1024).toFixed(1)} GiB ({device.engine_device})</div>
        {/each}
        {profile.memory_kind === 'unified' ? 'Shared unified memory, counted once' : 'Dedicated device memory'};
        {(profile.memory_reserve_mib / 1024).toFixed(2)} GiB reserved per device
      </dd>
      <dt>Admitted load</dt><dd>{profile.max_concurrency} concurrent requests, {profile.arrival_milli_rps / 1000} requests/sec</dd>
      <dt>Engine / quantization</dt><dd>{profile.engine} {profile.engine_version} · {profile.platform}/{profile.backend} · {model.quantization ?? 'unquantized'}</dd>
      <dt>Location</dt><dd>{profile.region}</dd>
    </dl>
    <p>Qualification requires at least 95% joint attainment of time to first token ≤2 seconds and the decode target on warmed generation, with prompts ≤2,048 tokens and a 256-token output workload within a 4,096-token workload envelope. Capacity alone does not establish speed. Provider-observed timing does not guarantee Internet delivery latency.</p>
    <p>Eligible performance misses waive this request's inference charge. Request records identify eligibility, measured timing and remedy; funding fees, network gas and the non-refundable prepayment policy are unchanged. Client cancellation/backpressure and requests outside the agreed workload have separate outcomes.</p>
    <details>
      <summary>Signed qualification provenance</summary>
      <dl>
        <dt>HF model / tokenizer</dt><dd>{profile.hf_repo} / {profile.tokenizer_repo}</dd>
        <dt>Model revision</dt><dd class="mono">{model.revision}</dd>
        <dt>Tokenizer revision</dt><dd class="mono">{model.tokenizer_revision}</dd>
        <dt>Evidence date</dt><dd>{new Date(profile.qualification.measured_at * 1000).toLocaleString()}</dd>
        <dt>Evidence SHA-256</dt><dd class="mono">{profile.qualification.evidence_sha256}</dd>
        <dt>Artifact SHA-256</dt><dd class="mono">{profile.artifact_manifest_sha256}</dd>
        <dt>Native runtime SHA-256</dt><dd class="mono">{profile.runtime_sha256}</dd>
        <dt>Execution settings</dt><dd>{profile.gpu_layers} GPU layers · {profile.split_mode} split · KV {profile.cache_type_k}/{profile.cache_type_v} · batch {profile.batch_size}/{profile.ubatch_size} · flash attention {profile.flash_attention ? 'on' : 'off'}</dd>
      </dl>
      <p>These are attributable provider measurements, not independent model-execution or hardware attestations.</p>
    </details>
  </section>
{:else}
  <p>No generation-speed guarantee applies to this embedding or local calibration profile.</p>
{/if}
