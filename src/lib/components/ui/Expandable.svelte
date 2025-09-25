<script lang="ts">
  import { slide } from "svelte/transition";
  import { ChevronDown } from "lucide-svelte";

  export let isOpen = false;
</script>

<div class="border rounded-md">
  <button
    on:click={() => (isOpen = !isOpen)}
    class="flex items-center justify-between w-full p-4 text-left"
  >
    <div class="font-semibold">
      <slot name="title" />
    </div>
    <ChevronDown
      class="transform transition-transform duration-200 {isOpen
        ? 'rotate-180'
        : ''}"
    />
  </button>
  {#if isOpen}
    <div transition:slide={{ duration: 200 }} class="p-4 border-t">
      <slot />
    </div>
  {/if}
</div>