<script lang="ts">
  import { ChevronsUpDown, Check } from "lucide-svelte";
  import { createEventDispatcher, onMount, onDestroy } from "svelte";

  // Define pool type
  interface Pool {
    value: string;
    label: string;
  }

  // Props
  export let pools: Pool[] = [];
  export let value: string;
  export let disabled = false;

  // Typed event dispatcher
  const dispatch = createEventDispatcher<{ change: { value: string } }>();

  let open = false;

  function selectPool(pool: Pool) {
    value = pool.value;
    dispatch("change", { value }); // type-safe event
    open = false;
  }

  // Click outside to close
  let container: HTMLDivElement;

  function handleClickOutside(event: MouseEvent) {
    const path = event.composedPath();
    if (!path.includes(container)) {
      open = false;
    }
  }

  onMount(() => {
    document.addEventListener("click", handleClickOutside);
  });

  onDestroy(() => {
    document.removeEventListener("click", handleClickOutside);
  });
</script>

<div class="relative" bind:this={container}>
  <button
    type="button"
    class="appearance-none mt-2 w-full flex items-center justify-between rounded-lg border px-3 py-2 text-sm bg-background hover:bg-accent focus:outline-none focus:ring-2 focus:ring-primary disabled:opacity-50"
    on:click={() => (open = !open)}
    disabled={disabled}
  >
    <span>{pools.find((p) => p.value === value)?.label ?? "Select a pool"}</span>
    <ChevronsUpDown class="h-4 w-4 text-muted-foreground" />
  </button>

  {#if open}
    <div class="absolute z-10 mt-1 w-full rounded-lg border bg-popover shadow-lg">
      {#each pools as pool}
        <button
          type="button"
          class="w-full flex items-center justify-between px-3 py-2 text-sm hover:bg-accent"
          on:click={() => selectPool(pool)}
        >
          <span>{pool.label}</span>
          {#if pool.value === value}
            <Check class="h-4 w-4 text-primary" />
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
