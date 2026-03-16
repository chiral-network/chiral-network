<script lang="ts">
 import { Server, Power, PowerOff, Loader2 } from'lucide-svelte';
 import { buildHostedLocalUrl } from'$lib/utils/hostingPageUtils';

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

<div class="rounded-xl border p-4 transition-colors
 {serverRunning
 ?'border-green-400/20 bg-green-500/10'
 :'border-gray-200/60 dark:border-white/[0.06] bg-white/70 dark:bg-white/[0.05]'}">
 
 <div class="flex items-center justify-between gap-4 flex-wrap">
 <div class="flex items-center gap-3">
 <!-- Status indicator -->
 <div class="relative flex h-10 w-10 items-center justify-center rounded-xl
 {serverRunning ?'bg-green-500/15' :'bg-white/70 dark:bg-white/[0.05]'}">
 <Server class="h-5 w-5 {serverRunning ?'text-green-600 dark:text-green-400' :'text-gray-500 dark:text-white/50'}" />
 {#if serverRunning}
 <span class="absolute -top-0.5 -right-0.5 flex h-3 w-3">
 <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-green-400 opacity-75"></span>
 <span class="relative inline-flex h-3 w-3 rounded-full bg-green-500"></span>
 </span>
 {/if}
 </div>
 <div>
 <h2 class="text-sm font-semibold text-gray-900 dark:text-white/90 flex items-center gap-2">
 Local HTTP Server
 {#if serverRunning}
 <span class="inline-flex items-center gap-1 rounded-full bg-green-100 dark:bg-green-500/[0.15] px-2 py-0.5 text-[10px] font-medium text-green-600 dark:text-green-400 uppercase tracking-wide">
 Online
 </span>
 {/if}
 </h2>
 {#if serverRunning}
 <p class="text-xs text-green-600 dark:text-green-400 mt-0.5">
 Serving at <a href={localUrl()} target="_blank" rel="noopener noreferrer" class="font-mono underline decoration-green-300 hover:decoration-green-500 transition-colors">{localUrl()}</a>
 </p>
 {:else}
 <p class="text-xs text-gray-400 dark:text-white/40 mt-0.5">Server is stopped. Start it to host content.</p>
 {/if}
 </div>
 </div>

 <div class="flex items-center gap-3">
 {#if !serverRunning}
 <div class="flex items-center gap-2">
 <label for="hosting-port" class="text-xs text-gray-400 dark:text-white/40 font-medium">Port</label>
 <input
 id="hosting-port"
 type="number"
 value={port}
 oninput={(e) => onPortChange(Number(e.currentTarget.value))}
 min="1024"
 max="65535"
 class="w-20 rounded-lg border border-gray-200/60 dark:border-white/[0.06] bg-white/70 dark:bg-white/[0.05] px-2.5 py-1.5 text-sm text-gray-900 dark:text-white/90 text-center tabular-nums
 focus:border-primary-400 focus:outline-none 
"
 />
 </div>
 {/if}

 {#if serverRunning}
 <button
 onclick={onStopServer}
 class="flex items-center gap-2 rounded-xl bg-red-500/[0.1]0/[0.1]0 px-4 py-2 text-sm font-medium text-white transition-colors
 hover:bg-red-500/[0.1]0/[0.15]0/80 focus:outline-none"
 >
 <PowerOff class="h-4 w-4" />
 Stop
 </button>
 {:else}
 <button
 onclick={onStartServer}
 disabled={isStartingServer}
 class="flex items-center gap-2 rounded-xl bg-violet-500 px-4 py-2 text-sm font-medium text-white transition-colors
 hover:bg-violet-500/90 focus:outline-none 
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
