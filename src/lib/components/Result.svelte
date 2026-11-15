<script lang="ts">
  let { value, title = 'Response' }: { value: unknown; title?: string } = $props();
</script>
{#if value !== null && value !== undefined}
  <details class="panel" open><summary>{title}</summary><pre class="data">{JSON.stringify(value, null, 2)}</pre></details>
{/if}
