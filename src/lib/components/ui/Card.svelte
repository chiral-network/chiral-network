<script lang="ts">
  import type { Snippet } from 'svelte';
  import { pillTone, type IconComponent, type Tone } from './tones';
  interface Props {
    title?: string;
    subtitle?: string;
    icon?: IconComponent;
    tone?: Tone;
    class?: string;
    tag?: 'section' | 'article' | 'div';
    label?: string;
    actions?: Snippet;
    children?: Snippet;
  }
  let { title = '', subtitle = '', icon: Icon = undefined, tone = 'primary', class: extra = '', tag = 'section', label = undefined, actions = undefined, children = undefined }: Props = $props();
</script>

<svelte:element this={tag} class="card p-5 {extra}" aria-label={label}>
  {#if title || Icon || actions}
    <div class="flex items-start justify-between gap-3 {children ? 'mb-4' : ''}">
      <div class="flex items-center gap-3 min-w-0">
        {#if Icon}
          <div class="p-2 rounded-lg shrink-0 {pillTone[tone]}"><Icon class="w-5 h-5" /></div>
        {/if}
        <div class="min-w-0">
          {#if title}<h2 class="card-title break-words">{title}</h2>{/if}
          {#if subtitle}<p class="card-subtitle mt-0.5">{subtitle}</p>{/if}
        </div>
      </div>
      {#if actions}<div class="flex flex-wrap items-center gap-2 shrink-0">{@render actions()}</div>{/if}
    </div>
  {/if}
  {@render children?.()}
</svelte:element>
