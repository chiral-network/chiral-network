<script lang="ts">
  import { Server, Power, PowerOff, Loader2 } from 'lucide-svelte';
  import { buildHostedLocalUrl } from '$lib/utils/hostingPageUtils';

  interface Props {
    serverRunning: boolean;
    serverAddress: string | null;
    port: number;
    isStartingServer: boolean;
    onStartServer: () => void;
    onStopServer: () => void;
    onPortChange: (port: number) => void;
  }

  let {
    serverRunning,
    serverAddress,
    port,
    isStartingServer,
    onStartServer,
    onStopServer,
    onPortChange,
  }: Props = $props();

  function localUrl(): string {
    return buildHostedLocalUrl(serverAddress, port);
  }
</script>

<div class="rounded-2xl border p-4 transition-colors
  {serverRunning
    ? 'border-green-400/20 bg-green-500/10 dark:border-green-600/15 dark:bg-green-500/5'
    : 'border-white/20 bg-white/10 dark:border-white/10 dark:bg-white/5'}
  shadow-sm backdrop-blur">
  <div class="flex items-center justify-between gap-4 flex-wrap">
    <div class="flex items-center gap-3">
      <!-- Status indicator -->
      <div class="relative flex h-10 w-10 items-center justify-center rounded-xl
        {serverRunning ? 'bg-green-500/15 dark:bg-green-500/10' : 'bg-white/10 dark:bg-white/5'}">
        <Server class="h-5 w-5 {serverRunning ? 'text-green-600 dark:text-green-400' : 'text-gray-400 dark:text-gray-500'}" />
        {#if serverRunning}
          <span class="absolute -top-0.5 -right-0.5 flex h-3 w-3">
            <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-green-400 opacity-75"></span>
            <span class="relative inline-flex h-3 w-3 rounded-full bg-green-500"></span>
          </span>
        {/if}
      </div>
      <div>
        <h2 class="text-sm font-semibold text-gray-900 dark:text-white flex items-center gap-2">
          Local HTTP Server
          {#if serverRunning}
            <span class="inline-flex items-center gap-1 rounded-full bg-green-100 px-2 py-0.5 text-[10px] font-medium text-green-700 dark:bg-green-900/40 dark:text-green-300 uppercase tracking-wide">
              Online
            </span>
          {/if}
        </h2>
        {#if serverRunning}
          <p class="text-xs text-green-600 dark:text-green-400 mt-0.5">
            Serving at <a href={localUrl()} target="_blank" rel="noopener noreferrer" class="font-mono underline decoration-green-300 dark:decoration-green-700 hover:decoration-green-500 transition-colors">{localUrl()}</a>
          </p>
        {:else}
          <p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">Server is stopped. Start it to host content.</p>
        {/if}
      </div>
    </div>

    <div class="flex items-center gap-3">
      {#if !serverRunning}
        <div class="flex items-center gap-2">
          <label for="hosting-port" class="text-xs text-gray-500 dark:text-gray-400 font-medium">Port</label>
          <input
            id="hosting-port"
            type="number"
            value={port}
            oninput={(e) => onPortChange(Number(e.currentTarget.value))}
            min="1024"
            max="65535"
            class="w-20 rounded-lg border border-white/20 bg-white/10 px-2.5 py-1.5 text-sm text-gray-900 text-center tabular-nums
              focus:border-primary-400 focus:outline-none focus:ring-2 focus:ring-primary-400/30
              dark:border-white/10 dark:bg-white/5 dark:text-white"
          />
        </div>
      {/if}

      {#if serverRunning}
        <button
          onclick={onStopServer}
          class="flex items-center gap-2 rounded-xl bg-red-500 px-4 py-2 text-sm font-medium text-white transition-colors
            hover:bg-red-600 focus:outline-none focus:ring-2 focus:ring-red-500/50 focus:ring-offset-2 dark:focus:ring-offset-gray-900"
        >
          <PowerOff class="h-4 w-4" />
          Stop
        </button>
      {:else}
        <button
          onclick={onStartServer}
          disabled={isStartingServer}
          class="flex items-center gap-2 rounded-xl bg-primary-500 px-4 py-2 text-sm font-medium text-white transition-colors
            hover:bg-primary-600 focus:outline-none focus:ring-2 focus:ring-primary-500/50 focus:ring-offset-2 dark:focus:ring-offset-gray-900
            disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {#if isStartingServer}
            <Loader2 class="h-4 w-4 animate-spin" />
            Starting...
          {:else}
            <Power class="h-4 w-4" />
            Start Server
          {/if}
        </button>
      {/if}
    </div>
  </div>
</div>
