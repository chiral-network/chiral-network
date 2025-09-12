<script lang="ts">
  import { cn } from '$lib/utils'
  
  export let value: number = 0
  export let max: number = 100
  let className: string | null | undefined = undefined
  export { className as class }
  
  $: percentage = Math.min(100, Math.max(0, (value / max) * 100))
</script>

<div
  class={cn('relative h-4 w-full overflow-hidden rounded-full bg-primary/10', className)}
  {...$$restProps}
>
  <div
    class="h-full bg-primary rounded-full transition-all duration-200"
    style="width: {percentage}%"
    role="progressbar"
    aria-valuemin={0}
    aria-valuemax={max}
    aria-valuenow={value}
  ></div>
</div>