<script lang="ts">
  import { cn } from '$lib/utils'
  import type { HTMLInputAttributes } from 'svelte/elements'
  import { createEventDispatcher } from 'svelte'

  type $$Props = HTMLInputAttributes & {
    class?: string | null | undefined
    value?: any
  }

  let className: string | null | undefined = undefined
  export { className as class }
  export let value: any = undefined

  const dispatch = createEventDispatcher<{ keydown: KeyboardEvent }>()

  function handleKeydown(event: KeyboardEvent) {
    dispatch('keydown', event)
  }
</script>

<input
  {...$$restProps}
  bind:value
  on:keydown={handleKeydown}
  on:keydown
  class={cn(
    'flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50',
    className
  )}
/>