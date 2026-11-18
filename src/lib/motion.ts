// One place for the combined reduced-motion preference (A.1.9): the operating system's
// `prefers-reduced-motion: reduce` or the in-app setting (A.5.8). CSS covers stylesheet
// motion through the `reduce-motion` class; script-driven motion must ask here.
import { get } from 'svelte/store';
import { settings } from './stores';

export function prefersReducedMotion(): boolean {
  const system = typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  return system || get(settings).reducedMotion;
}
/** A transition duration that is zero, so Svelte skips the animation, under reduced motion. */
export const motionDuration = (milliseconds: number) => (prefersReducedMotion() ? 0 : milliseconds);
/** Scrolling behavior: instant under reduced motion. */
export const scrollBehavior = (): ScrollBehavior => (prefersReducedMotion() ? 'auto' : 'smooth');
