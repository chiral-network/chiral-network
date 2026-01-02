<script lang="ts">
  import { AlertTriangle, DownloadCloud, Lock, PlugZap } from 'lucide-svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Card from '$lib/components/ui/card.svelte';

  export let isExiting = false;
  export let isLocking = false;
  export let onStay: () => void;
  export let onLockAccount: () => void;
  export let onExit: () => void;
  export let error: string | null = null;
</script>

<div class="fixed inset-0 z-50 bg-background/90 backdrop-blur-lg flex items-center justify-center p-4">
  <Card class="w-full max-w-2xl p-8 space-y-6 shadow-2xl border-primary/30">
    <div class="space-y-2 text-center">
      <p class="text-xs uppercase tracking-[0.3em] text-primary font-semibold">
        Exit Chiral
      </p>
      <h2 class="text-3xl font-bold">Ready to leave?</h2>
      <p class="text-muted-foreground">
        Closing the app will stop active uploads and downloads. Stay online to keep your transfers
        healthy and continue earning reputation.
      </p>
    </div>

    <div class="grid gap-3 md:grid-cols-2">
      <div class="flex items-start gap-3 p-4 rounded-lg bg-muted/70 border border-border/60">
        <div class="h-10 w-10 rounded-full bg-primary/10 text-primary flex items-center justify-center">
          <DownloadCloud class="h-5 w-5" />
        </div>
        <div class="space-y-1">
          <p class="font-semibold">Transfers pause</p>
          <p class="text-sm text-muted-foreground">
            Downloads and uploads halt as soon as you quit. Resume later from the same machine.
          </p>
        </div>
      </div>

      <div class="flex items-start gap-3 p-4 rounded-lg bg-muted/70 border border-border/60">
        <div class="h-10 w-10 rounded-full bg-amber-500/10 text-amber-500 flex items-center justify-center">
          <AlertTriangle class="h-5 w-5" />
        </div>
        <div class="space-y-1">
          <p class="font-semibold">Network presence</p>
          <p class="text-sm text-muted-foreground">
            Going offline removes you from the relay network until you reopen Chiral.
          </p>
        </div>
      </div>
    </div>

    <div class="flex items-center gap-3 p-4 rounded-lg bg-accent/60 border border-accent">
      <div class="h-10 w-10 rounded-full bg-secondary text-secondary-foreground flex items-center justify-center">
        <PlugZap class="h-5 w-5" />
      </div>
      <div class="space-y-1">
        <p class="font-semibold">Better option: minimize</p>
        <p class="text-sm text-muted-foreground">
          Keep Chiral running in the background to finish transfers and stay reachable.
        </p>
      </div>
    </div>

    {#if error}
      <div class="rounded-md border border-destructive/30 bg-destructive/10 px-4 py-2 text-sm text-destructive">
        {error}
      </div>
    {/if}

    <div class="flex flex-col sm:flex-row gap-3">
      <Button class="flex-1" variant="secondary" on:click={onStay} disabled={isExiting || isLocking}>
        Stay online
      </Button>
      <Button class="flex-1" variant="outline" on:click={onLockAccount} disabled={isExiting || isLocking}>
        <div class="flex items-center justify-center gap-2">
          <Lock class="h-4 w-4" />
          {isLocking ? 'Locking...' : 'Lock account'}
        </div>
      </Button>
      <Button class="flex-1" variant="destructive" on:click={onExit} disabled={isExiting}>
        {isExiting ? 'Shutting down...' : 'Quit Chiral'}
      </Button>
    </div>
  </Card>
</div>
