<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { onMount, onDestroy } from 'svelte';
  import { Upload, Play, Pause, CheckCircle2, Copy, Share2 } from 'lucide-svelte';
  import { toast } from '$lib/stores/toastStore';

  // --- Types and State ---
  type TorrentStatus = 'downloading' | 'paused' | 'seeding' | 'complete' | 'error';
  type TorrentSource = 'Chiral' | 'BitTorrent';

  interface Torrent {
    id: string;
    name: string;
    status: TorrentStatus;
    progress: number; // 0-100
    peers: number;
    eta: string;
    source: TorrentSource;
    magnetLink?: string;
  }

  let torrents: Torrent[] = [];
  let activeFilter: 'all' | TorrentSource = 'all';
  $: filteredTorrents = torrents.filter(t => activeFilter === 'all' || t.source === activeFilter);

  let magnetLink = '';
  let downloadFileInput: HTMLInputElement;
  let downloadSelectedFileName: string | null = null;
  
  let seedFileInput: HTMLInputElement;
  let seedSelectedFileName: string | null = null;
  let newlySeededMagnet: string | null = null;

  // --- Mocking for UI Development ---
  const isDevelopment = import.meta.env.DEV;

  async function mockInvoke(command: string, args?: any) {
    console.log(`[MOCK] Calling command: ${command}`, args);
    
    if (command === 'download_torrent') {
      const newId = `mock_${Date.now()}`;
      const isChiral = Math.random() > 0.5;
      const newTorrent: Torrent = {
        id: newId,
        name: args.identifier.substring(0, 40) + '...',
        status: 'downloading',
        progress: 0,
        peers: Math.floor(Math.random() * 50),
        eta: 'calculating...',
        source: isChiral ? 'Chiral' : 'BitTorrent',
      };
      torrents = [...torrents, newTorrent];
      simulateProgress(newId);
      return Promise.resolve();
    }
    
    if (command === 'seed_file') {
      const mockMagnet = `magnet:?xt=urn:btih:${Math.random().toString(36).substring(2)}&dn=${encodeURIComponent(args.fileName)}`;
      newlySeededMagnet = mockMagnet;
      torrents = [...torrents, {
        id: `seed_${Date.now()}`,
        name: args.fileName,
        status: 'seeding',
        progress: 100,
        peers: 0,
        eta: '∞',
        source: 'BitTorrent',
        magnetLink: mockMagnet,
      }];
      return Promise.resolve({ magnet_link: mockMagnet });
    }

    return Promise.reject(`Mock command "${command}" not found.`);
  }

  // Use the mock function in development, otherwise use the real one.
  const invokeCommand = isDevelopment ? mockInvoke : invoke;

  function handleDownloadFileSelect(event: Event) {
    const target = event.target as HTMLInputElement;
    const file = target.files?.[0];
    if (file && file.name.endsWith('.torrent')) {
      downloadSelectedFileName = file.name;
      // In a real implementation, you would read the file content
      // and pass it to the backend, likely as a base64 string.
      console.log("Selected .torrent file:", file);
    } else {
      alert("Please select a valid .torrent file.");
      downloadSelectedFileName = null;
      if (target) target.value = ''; // Reset the input
    }
  }

  function handleSeedFileSelect(event: Event) {
    const target = event.target as HTMLInputElement;
    const file = target.files?.[0];
    if (file) {
      seedSelectedFileName = file.name;
    } else {
      seedSelectedFileName = null;
      if (target) target.value = '';
    }
  }

  async function startSeeding() {
    if (!seedSelectedFileName) {
      alert('Please select a file to seed.');
      return;
    }
    try {
      // In a real implementation, you'd pass the full file path.
      const result = await invokeCommand('seed_file', { fileName: seedSelectedFileName });
      toast.success(`Successfully started seeding ${seedSelectedFileName}!`);
      seedSelectedFileName = null;
      if (seedFileInput) seedFileInput.value = '';
    } catch (error) {
      console.error("Failed to start seeding:", error);
      toast.error(`Error seeding file: ${error}`);
    }
  }

  async function startDownload() {
    let identifier: string | null = null;

    if (magnetLink.trim()) {
      identifier = magnetLink.trim();
    } else if (downloadSelectedFileName) {
      // This is a placeholder. The real implementation would pass file content.
      identifier = `file://${downloadSelectedFileName}`;
    }

    if (identifier) {
      try {
        await invokeCommand('download_torrent', { identifier });
        // Clear inputs on success
        magnetLink = '';
        downloadSelectedFileName = null;
        if (downloadFileInput) downloadFileInput.value = '';
      } catch (error) {
        console.error("Failed to start download:", error);
        alert(`Error: ${error}`);
      }
    } else {
      alert("Please provide a magnet link or select a .torrent file.");
    }
  }

  // --- Lifecycle and Event Handling ---
  let unlisten: UnlistenFn | undefined;

  onMount(() => {
    const setupListener = async () => {
      unlisten = await listen('torrent_complete', (event) => {
        console.log('Received torrent_complete event:', event.payload);
        const { torrentId, finalName } = event.payload as { torrentId: string, finalName: string };
        
        torrents = torrents.map(t => 
          t.id === torrentId 
            ? { ...t, status: 'complete', progress: 100, name: finalName } 
            : t
        );
        toast.success(`Download complete: ${finalName}`);
      });
    };
    setupListener();
  });

  onDestroy(() => {
    if (unlisten) {
      unlisten();
    }
  });

  function simulateProgress(torrentId: string) {
    const interval = setInterval(() => {
      torrents = torrents.map(t => {
        if (t.id === torrentId && t.status === 'downloading') {
          const newProgress = Math.min(t.progress + Math.random() * 5, 100);
          const remaining = 100 - newProgress;
          const etaSeconds = remaining * 2;
          
          if (newProgress >= 100) {
            clearInterval(interval);
            // In a real app, the backend would send this event.
            // We simulate it here for demonstration.
            if (isDevelopment) {
              invoke('tauri', {
                __tauriModule: 'Event',
                message: {
                  cmd: 'emit',
                  event: 'torrent_complete',
                  payload: { torrentId, finalName: `Completed ${t.name}` }
                }
              });
            }
            return { ...t, status: 'complete', progress: 100, eta: '0s' };
          }
          
          return { ...t, progress: newProgress, eta: `${Math.round(etaSeconds)}s` };
        }
        return t;
      });
    }, 1000);
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    toast.info('Copied to clipboard!');
  }

</script>

<div class="space-y-8">
  <h1 class="text-2xl font-bold">Torrents</h1>

  <div class="p-6 bg-card border rounded-lg">
    <h2 class="text-lg font-semibold mb-2">From Magnet Link</h2>
    <p class="text-muted-foreground mb-4">Paste a magnet link to start downloading.</p>
    <div class="flex items-center gap-2">
      <input 
        type="text" 
        bind:value={magnetLink} 
        placeholder="magnet:?xt=urn:btih:..."
        class="flex-grow p-2 border rounded-md bg-background"
      />
      <button on:click={startDownload} disabled={!magnetLink.trim()} class="px-4 py-2 bg-primary text-primary-foreground rounded-md disabled:opacity-50">Download</button>
    </div>
  </div>

  <div class="relative">
    <div class="absolute inset-0 flex items-center">
      <span class="w-full border-t"></span>
    </div>
    <div class="relative flex justify-center text-xs uppercase">
      <span class="bg-background px-2 text-muted-foreground">Or</span>
    </div>
  </div>

  <div class="p-6 bg-card border rounded-lg">
    <h2 class="text-lg font-semibold mb-2">From .torrent File</h2>
    <p class="text-muted-foreground mb-4">Upload a .torrent file from your computer.</p>
    <div class="flex items-center gap-2">
      <label for="torrent-file-input" class="flex-grow p-2 border border-dashed rounded-md cursor-pointer text-center text-muted-foreground hover:bg-accent">
        <Upload class="inline-block h-4 w-4 mr-2" />
        {downloadSelectedFileName || 'Choose a .torrent file...'}
      </label>
      <input 
        id="torrent-file-input"
        type="file" 
        accept=".torrent"
        bind:this={downloadFileInput}
        on:change={handleDownloadFileSelect}
        class="hidden"
      />
      <button on:click={startDownload} disabled={!downloadSelectedFileName} class="px-4 py-2 bg-primary text-primary-foreground rounded-md disabled:opacity-50">Download</button>
    </div>
  </div>

  <!-- Seeding UI -->
  <div class="p-6 bg-card border rounded-lg">
    <h2 class="text-lg font-semibold mb-2 flex items-center gap-2"><Share2 class="h-5 w-5" /> Create & Seed Torrent</h2>
    <p class="text-muted-foreground mb-4">Select a local file to share with the network.</p>
    <div class="flex items-center gap-2">
      <label for="seed-file-input" class="flex-grow p-2 border border-dashed rounded-md cursor-pointer text-center text-muted-foreground hover:bg-accent">
        <Upload class="inline-block h-4 w-4 mr-2" />
        {seedSelectedFileName || 'Choose a file to seed...'}
      </label>
      <input 
        id="seed-file-input"
        type="file" 
        bind:this={seedFileInput}
        on:change={handleSeedFileSelect}
        class="hidden"
      />
      <button on:click={startSeeding} disabled={!seedSelectedFileName} class="px-4 py-2 bg-green-600 text-white rounded-md disabled:opacity-50">Seed File</button>
    </div>
    {#if newlySeededMagnet}
      <div class="mt-4 p-3 bg-background rounded-md border">
        <p class="text-sm font-semibold">Share this magnet link:</p>
        <div class="flex items-center gap-2 mt-1">
          <input type="text" readonly value={newlySeededMagnet} class="flex-grow p-1 text-xs bg-muted rounded-sm" />
          <button on:click={() => copyToClipboard(newlySeededMagnet)} class="p-2 hover:bg-accent rounded-md">
            <Copy class="h-4 w-4" />
          </button>
        </div>
      </div>
    {/if}
  </div>

  <!-- Download Progress View -->
  <div class="space-y-4">
    <div class="flex justify-between items-center">
      <h2 class="text-xl font-bold">Active Transfers</h2>
      <div class="flex items-center gap-1 p-1 bg-muted rounded-md">
        <button class:bg-background={activeFilter === 'all'} on:click={() => activeFilter = 'all'} class="px-3 py-1 text-sm rounded-md">All</button>
        <button class:bg-background={activeFilter === 'Chiral'} on:click={() => activeFilter = 'Chiral'} class="px-3 py-1 text-sm rounded-md">Chiral</button>
        <button class:bg-background={activeFilter === 'BitTorrent'} on:click={() => activeFilter = 'BitTorrent'} class="px-3 py-1 text-sm rounded-md">BitTorrent</button>
      </div>
    </div>

    {#if filteredTorrents.length === 0}
      <div class="text-center py-10 border-2 border-dashed rounded-lg">
        <p class="text-muted-foreground">No active transfers.</p>
      </div>
    {:else}
      <div class="space-y-3">
        {#each filteredTorrents as torrent (torrent.id)}
          <div class="p-4 bg-card border rounded-lg">
            <div class="flex justify-between items-start">
              <p class="font-semibold truncate pr-4" title={torrent.name}>{torrent.name}</p>
              <div class="flex items-center gap-2 text-sm">
                <span class="px-2 py-0.5 rounded-full text-xs" class:bg-blue-100={torrent.source === 'Chiral'} class:text-blue-800={torrent.source === 'Chiral'} class:bg-orange-100={torrent.source === 'BitTorrent'} class:text-orange-800={torrent.source === 'BitTorrent'}>{torrent.source}</span>
                {#if torrent.status === 'downloading'} <Play class="h-5 w-5 text-green-500" />
                {:else if torrent.status === 'paused'} <Pause class="h-5 w-5 text-yellow-500" />
                {:else if torrent.status === 'seeding'} <Upload class="h-5 w-5 text-blue-500" />
                {:else if torrent.status === 'complete'} <CheckCircle2 class="h-5 w-5 text-green-600" />
                {/if}
              </div>
            </div>
            <div class="mt-2">
              <div class="w-full bg-muted rounded-full h-2.5">
                <div class="bg-primary h-2.5 rounded-full" style="width: {torrent.progress}%"></div>
              </div>
              <div class="flex justify-between text-xs text-muted-foreground mt-1">
                <span>{torrent.progress.toFixed(1)}%</span>
                <span>Peers: {torrent.peers}</span>
                <span>ETA: {torrent.eta}</span>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>