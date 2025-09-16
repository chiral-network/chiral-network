<script lang="ts">
  import { cn } from '$lib/utils'
  
  type Variant = 'default' | 'secondary' | 'destructive' | 'outline'
  
  export let variant: Variant = 'default'
  let className: string | null | undefined = undefined
  export { className as class }
  
  const variants: Record<Variant, string> = {
    default: 'border-transparent bg-primary text-primary-foreground',
    secondary: 'border-transparent bg-secondary text-secondary-foreground',
    destructive: 'border-transparent bg-destructive text-destructive-foreground',
    outline: 'text-foreground',
  }
</script>

<div
  class={cn(
    'inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2',
    variants[variant],
    className
  )}
  {...$$restProps}
>
  <slot />
</div>