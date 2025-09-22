<script lang="ts">
  import { ChevronsUpDown, Check } from "lucide-svelte";
  import { createEventDispatcher, onMount, onDestroy } from "svelte";

  interface Option {
    value: string;
    label: string;
  }

  // Props
  export let options: Option[] = [];
  export let value: string = '';
  export let id: string = '';
  export let disabled: boolean = false;
  export let placeholder: string = '';
  export let className: string = '';

  const dispatch = createEventDispatcher<{ change: { value: string } }>();
  let open = false;
  let container: HTMLDivElement;

  function selectOption(option: Option) {
    value = option.value;
    dispatch("change", { value });  // optional for event handling
    open = false;
  }

  function handleClickOutside(event: MouseEvent) {
    if (!event.composedPath().includes(container)) open = false;
  }

  onMount(() => document.addEventListener("click", handleClickOutside));
  onDestroy(() => document.removeEventListener("click", handleClickOutside));
</script>

<!-- Default export for the component -->
<svelte:options accessors={true} />

<div class="relative mt-2" bind:this={container}>
  <button
    type="button"
    id={id}
    class="w-full px-3 py-2 border rounded-md bg-white text-left flex items-center justify-between hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:opacity-50 {className}"
    on:click={() => (open = !open)}
    disabled={disabled}
  >
    <span>{options.find(o => o.value === value)?.label ?? placeholder}</span>
    <ChevronsUpDown class="h-4 w-4 text-gray-400" />
  </button>

  {#if open}
    <div class="absolute z-10 mt-1 w-full rounded-md border bg-white shadow-lg">
      {#each options as option}
        <button
          type="button"
          class="w-full text-left px-3 py-2 text-sm hover:bg-gray-100 flex justify-between items-center"
          on:click={() => selectOption(option)}
        >
          <span>{option.label}</span>
          {#if option.value === value}
            <Check class="h-4 w-4 text-blue-500" />
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
