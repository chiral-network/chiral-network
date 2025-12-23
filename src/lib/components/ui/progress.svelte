<script lang="ts">
  import { cn } from '$lib/utils'
  
  export let value: number = 0
  export let max: number = 100
  let className: string | null | undefined = undefined
  export let indicatorClass: string | null | undefined = 'bg-primary'
  export { className as class }

  // When max=100, assume value is already a percentage (0-100)
  // Otherwise, calculate percentage from value/max ratio
  $: percentage = max === 100 && value <= 100
    ? Math.min(100, Math.max(0, value))
    : Math.min(100, Math.max(0, (value / max) * 100))
</script>

<div
  class={cn('relative h-4 w-full overflow-hidden rounded-full bg-primary/10', className)}
  {...$$restProps}
>
  <div
    class={cn('h-full rounded-full transition-all duration-200', indicatorClass)}
    style="width: {percentage}%"
    role="progressbar"
    aria-valuemin={0}
    aria-valuemax={max}
    aria-valuenow={value}
  ></div>
</div>