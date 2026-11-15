import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import Result from '$lib/components/Result.svelte';

let target: HTMLDivElement;
let mounted: Record<string, unknown> | undefined;
beforeEach(() => { target = document.createElement('div'); document.body.append(target); });
afterEach(async () => { if (mounted) await unmount(mounted); mounted = undefined; target.remove(); });

describe('rendered product surfaces without native runtime', () => {
  it('escapes provider-supplied metadata rather than interpreting HTML', async () => {
    mounted = mount(Result, { target, props: { value: { model: '<img src=x onerror="alert(1)">' } } }); await tick();
    expect(target.querySelector('img')).toBeNull();
    expect(target.querySelector('pre')?.textContent).toContain('<img');
  });
});
