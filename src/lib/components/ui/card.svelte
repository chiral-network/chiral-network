<script lang="ts">
  import { cn } from '$lib/utils'
  import { createEventDispatcher } from 'svelte'

  let className: string | null | undefined = undefined
  export { className as class }

  const dispatch = createEventDispatcher()

  function handleDragEvent(event: DragEvent) {
    // Dispatch the original drag event
    dispatch(event.type, event)
  }
</script>

<div
  class={cn(
    'rounded-lg border bg-card text-card-foreground shadow-sm',
    className
  )}
  {...$$restProps}
  on:drop={handleDragEvent}
  on:dragover={handleDragEvent}
  on:dragenter={handleDragEvent}
  on:dragleave={handleDragEvent}
>
  <slot />
</div>