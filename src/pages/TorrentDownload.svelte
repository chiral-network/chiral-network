<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Upload } from 'lucide-svelte';

  // For now, we mock it to develop the UI in isolation.
  const isDevelopment = import.meta.env.DEV;

  async function mockInvoke(command: string, args?: any) {
    console.log(`[MOCK] Calling command: ${command}`, args);
    
    if (command === 'download_torrent') {
      // Simulate a successful call
      alert(`Started "download" for: ${args.identifier}`);
      return Promise.resolve();
    }
    
    return Promise.reject(`Mock command "${command}" not found.`);
  }

  // Use the mock function in development, otherwise use the real one.
  const invokeCommand = isDevelopment ? mockInvoke : invoke;

  let magnetLink = '';
  let selectedFileName: string | null = null;
  let fileInput: HTMLInputElement;

  function handleFileSelect(event: Event) {
    const target = event.target as HTMLInputElement;
    const file = target.files?.[0];
    if (file && file.name.endsWith('.torrent')) {
      selectedFileName = file.name;
      // In a real implementation, you would read the file content
      // and pass it to the backend, likely as a base64 string.
      console.log("Selected .torrent file:", file);
    } else {
      alert("Please select a valid .torrent file.");
      selectedFileName = null;
      if (target) target.value = ''; // Reset the input
    }
  }

  async function startDownload() {
    let identifier: string | null = null;

    if (magnetLink.trim()) {
      identifier = magnetLink.trim();
    } else if (selectedFileName) {
      // This is a placeholder. The real implementation would pass file content.
      identifier = `file://${selectedFileName}`;
    }

    if (identifier) {
      try {
        await invokeCommand('download_torrent', { identifier });
        // Clear inputs on success
        magnetLink = '';
        selectedFileName = null;
        if (fileInput) fileInput.value = '';
      } catch (error) {
        console.error("Failed to start download:", error);
        alert(`Error: ${error}`);
      }
    } else {
      alert("Please provide a magnet link or select a .torrent file.");
    }
  }
</script>

<div class="space-y-8">
  <h1 class="text-2xl font-bold">Add New Torrent</h1>

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
        {selectedFileName || 'Choose a .torrent file...'}
      </label>
      <input 
        id="torrent-file-input"
        type="file" 
        accept=".torrent"
        bind:this={fileInput}
        on:change={handleFileSelect}
        class="hidden"
      />
      <button on:click={startDownload} disabled={!selectedFileName} class="px-4 py-2 bg-primary text-primary-foreground rounded-md disabled:opacity-50">Download</button>
    </div>
  </div>
</div>