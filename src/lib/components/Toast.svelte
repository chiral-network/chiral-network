<script lang="ts">
  import { fly, fade } from 'svelte/transition';
  import { X, CheckCircle2, AlertCircle, Info, AlertTriangle } from 'lucide-svelte';
  import { onMount } from 'svelte';

  interface Props {
    message: string;
    description?: string;
    type?: 'success' | 'error' | 'info' | 'warning';
    duration?: number;
    index?: number;
    onClose: () => void;
  }

  let { message, description, type = 'info', duration = 5000, index = 0, onClose }: Props = $props();

  let progress = $state(100);
  let paused = $state(false);

  const icons = { success: CheckCircle2, error: AlertCircle, info: Info, warning: AlertTriangle };
  const Icon = $derived(icons[type]);

  const styles = {
    success: {
      bg: 'bg-gray-900 dark:bg-gray-900',
      border: 'border-emerald-500/30',
      icon: 'text-emerald-400',
      bar: 'bg-emerald-400',
    },
    error: {
      bg: 'bg-gray-900 dark:bg-gray-900',
      border: 'border-red-500/30',
      icon: 'text-red-400',
      bar: 'bg-red-400',
    },
    info: {
      bg: 'bg-gray-900 dark:bg-gray-900',
      border: 'border-blue-500/30',
      icon: 'text-blue-400',
      bar: 'bg-blue-400',
    },
    warning: {
      bg: 'bg-gray-900 dark:bg-gray-900',
      border: 'border-amber-500/30',
      icon: 'text-amber-400',
      bar: 'bg-amber-400',
    },
  };

  const s = $derived(styles[type]);

  let topOffset = $derived(16 + index * (description ? 88 : 68));

  onMount(() => {
    const start = performance.now();
    let raf: number;

    function tick() {
      if (paused) {
        raf = requestAnimationFrame(tick);
        return;
      }
      const elapsed = performance.now() - start;
      progress = Math.max(0, 100 - (elapsed / duration) * 100);
      if (progress > 0) {
        raf = requestAnimationFrame(tick);
      }
    }

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });
</script>

<div
  transition:fly={{ x: 80, duration: 250 }}
  class="fixed right-4 z-50 w-[380px] max-w-[calc(100vw-2rem)] overflow-hidden rounded-lg border {s.border} {s.bg} shadow-2xl"
  style="top: {topOffset}px;"
  role="alert"
  onmouseenter={() => paused = true}
  onmouseleave={() => paused = false}
>
  <div class="flex items-start gap-3 px-4 py-3">
    <div class="mt-0.5 shrink-0 {s.icon}">
      <Icon size={18} strokeWidth={2.5} />
    </div>

    <div class="min-w-0 flex-1">
      <p class="text-sm font-medium text-gray-100">{message}</p>
      {#if description}
        <p class="mt-0.5 text-xs text-gray-400 leading-relaxed">{description}</p>
      {/if}
    </div>

    <button
      onclick={onClose}
      class="shrink-0 rounded p-0.5 text-gray-500 hover:text-gray-300 hover:bg-white/5 transition-colors"
      aria-label="Dismiss"
    >
      <X size={14} />
    </button>
  </div>

  <!-- Progress bar -->
  <div class="h-[2px] w-full bg-white/5">
    <div
      class="h-full {s.bar} transition-none"
      style="width: {progress}%; opacity: 0.6;"
    ></div>
  </div>
</div>
