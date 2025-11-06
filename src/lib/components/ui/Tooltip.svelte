<script lang="ts">
  import { onDestroy } from 'svelte';
  import { fade } from 'svelte/transition';

  export let text: string;
  export let position: 'top' | 'bottom' | 'left' | 'right' = 'top';
  export let delay: number = 200; // ms delay before showing
  export let maxWidth: string = '200px';
  export let disabled: boolean = false;

  let triggerElement: HTMLElement;
  let tooltipElement: HTMLElement;
  let isVisible = false;
  let showTimeout: ReturnType<typeof setTimeout>;
  let tooltipStyles = '';

  function handleMouseEnter() {
    if (disabled) return;
    
    showTimeout = setTimeout(() => {
      isVisible = true;
      // Calculate position after tooltip is rendered
      setTimeout(updatePosition, 0);
    }, delay);
  }

  function handleMouseLeave() {
    clearTimeout(showTimeout);
    isVisible = false;
  }

  function handleFocus() {
    if (disabled) return;
    isVisible = true;
    setTimeout(updatePosition, 0);
  }

  function handleBlur() {
    isVisible = false;
  }

  function updatePosition() {
    if (!triggerElement || !tooltipElement) return;

    const triggerRect = triggerElement.getBoundingClientRect();
    const tooltipRect = tooltipElement.getBoundingClientRect();
    const spacing = 8; // Gap between trigger and tooltip

    let top = 0;
    let left = 0;

    switch (position) {
      case 'top':
        top = triggerRect.top - tooltipRect.height - spacing;
        left = triggerRect.left + (triggerRect.width - tooltipRect.width) / 2;
        break;
      case 'bottom':
        top = triggerRect.bottom + spacing;
        left = triggerRect.left + (triggerRect.width - tooltipRect.width) / 2;
        break;
      case 'left':
        top = triggerRect.top + (triggerRect.height - tooltipRect.height) / 2;
        left = triggerRect.left - tooltipRect.width - spacing;
        break;
      case 'right':
        top = triggerRect.top + (triggerRect.height - tooltipRect.height) / 2;
        left = triggerRect.right + spacing;
        break;
    }

    // Keep tooltip within viewport bounds
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;
    const padding = 8;

    if (left < padding) left = padding;
    if (left + tooltipRect.width > viewportWidth - padding) {
      left = viewportWidth - tooltipRect.width - padding;
    }
    if (top < padding) top = padding;
    if (top + tooltipRect.height > viewportHeight - padding) {
      top = viewportHeight - tooltipRect.height - padding;
    }

    tooltipStyles = `top: ${top}px; left: ${left}px; max-width: ${maxWidth};`;
  }

  onDestroy(() => {
    clearTimeout(showTimeout);
  });
</script>

<div
  class="tooltip-wrapper inline-block"
  bind:this={triggerElement}
  on:mouseenter={handleMouseEnter}
  on:mouseleave={handleMouseLeave}
  on:focus={handleFocus}
  on:blur={handleBlur}
  role="button"
  tabindex="0"
>
  <slot />
</div>

{#if isVisible && !disabled}
  <div
    bind:this={tooltipElement}
    class="tooltip-content"
    style={tooltipStyles}
    transition:fade={{ duration: 150 }}
    role="tooltip"
  >
    {text}
    <div class="tooltip-arrow {position}"></div>
  </div>
{/if}

<style>
  .tooltip-wrapper {
    position: relative;
  }

  .tooltip-content {
    position: fixed;
    z-index: 9999;
    padding: 0.5rem 0.75rem;
    background-color: #1f2937;
    color: white;
    border-radius: 0.375rem;
    font-size: 0.875rem;
    line-height: 1.25rem;
    box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1), 0 2px 4px -1px rgba(0, 0, 0, 0.06);
    pointer-events: none;
    white-space: normal;
    word-wrap: break-word;
  }

  .tooltip-arrow {
    position: absolute;
    width: 0;
    height: 0;
    border-style: solid;
  }

  .tooltip-arrow.top {
    bottom: -4px;
    left: 50%;
    transform: translateX(-50%);
    border-width: 4px 4px 0 4px;
    border-color: #1f2937 transparent transparent transparent;
  }

  .tooltip-arrow.bottom {
    top: -4px;
    left: 50%;
    transform: translateX(-50%);
    border-width: 0 4px 4px 4px;
    border-color: transparent transparent #1f2937 transparent;
  }

  .tooltip-arrow.left {
    right: -4px;
    top: 50%;
    transform: translateY(-50%);
    border-width: 4px 0 4px 4px;
    border-color: transparent transparent transparent #1f2937;
  }

  .tooltip-arrow.right {
    left: -4px;
    top: 50%;
    transform: translateY(-50%);
    border-width: 4px 4px 4px 0;
    border-color: transparent #1f2937 transparent transparent;
  }
</style>