<script lang="ts">
  import { ShieldCheck, ShieldAlert, ShieldX } from 'lucide-svelte';

  // Props: The component can accept a raw score or a pre-calculated trust level.
  export let score: number | undefined = undefined;
  export let trustLevel: 'Trusted' | 'Medium' | 'Low' | undefined = undefined;
  export let size: number = 20;

  // Reactive properties for dynamic calculation
  let calculatedTrustLevel: 'Trusted' | 'Medium' | 'Low';
  let color: string;
  let title: string;

  // This reactive block recalculates whenever the props change.
  $: {
    calculatedTrustLevel = trustLevel ?? 'Medium';

    // Set color and title based on the final trust level
    switch (calculatedTrustLevel) {
      case 'Trusted':
        color = 'text-green-500';
        title = `Trusted (Score: ${score ?? 'N/A'})`;
        break;
      case 'Low':
        color = 'text-red-500';
        title = `Low Trust (Score: ${score ?? 'N/A'})`;
        break;
      case 'Medium':
      default:
        color = 'text-yellow-500';
        title = `Medium Trust (Score: ${score ?? 'N/A'})`;
        break;
    }
  }
</script>

<div class="inline-block" {title}>
  {#if calculatedTrustLevel === 'Trusted'}
    <ShieldCheck class={color} {size} />
  {:else if calculatedTrustLevel === 'Medium'}
    <ShieldAlert class={color} {size} />
  {:else}
    <ShieldX class={color} {size} />
  {/if}
</div>