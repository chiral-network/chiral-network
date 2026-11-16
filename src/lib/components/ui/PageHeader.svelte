<script lang="ts">
  import type { Snippet } from 'svelte';
  let { title, subtitle = '', id = undefined, actions = undefined }: { title: string; subtitle?: string; id?: string; actions?: Snippet } = $props();
</script>

<header class="flex flex-wrap items-start justify-between gap-3">
  <div class="min-w-0">
    <h1 {id} class="page-title">{title}</h1>
    {#if subtitle}<p class="page-subtitle">{subtitle}</p>{/if}
  </div>
  {#if actions}<div class="flex flex-wrap items-center gap-2 shrink-0">{@render actions()}</div>{/if}
</header>
