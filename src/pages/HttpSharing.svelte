<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { Copy, Upload, Download, Network, Server, CheckCircle, XCircle, Globe } from 'lucide-svelte';

  let networkInfo: any = null;
  let isLoadingNetwork = false;
  let networkError = '';

  let uploadFilePath = '';
  let uploadResult: any = null;
  let isUploading = false;
  let uploadError = '';

  let sharedFiles: any[] = [];
  let isLoadingFiles = false;

  let downloadHash = '';
  let downloadPath = '';
  let isDownloading = false;
  let downloadError = '';
  let downloadSuccess = '';

  let serverUrl = 'http://localhost:8080';

  let tunnelInfo: any = null;
  let isTunnelStarting = false;
  let tunnelError = '';

  async function setupNetwork() {
    isLoadingNetwork = true;
    networkError = '';
    try {
      networkInfo = await invoke('get_network_info', { port: 8080 });
      console.log('Network info:', networkInfo);

      // Also load shared files
      await loadSharedFiles();
    } catch (error: any) {
      networkError = error.toString();
      console.error('Network setup error:', error);
    } finally {
      isLoadingNetwork = false;
    }
  }

  async function selectFile() {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
      });
      if (selected && typeof selected === 'string') {
        uploadFilePath = selected;
      }
    } catch (error) {
      console.error('File selection error:', error);
    }
  }

  async function uploadFile() {
    if (!uploadFilePath) {
      uploadError = 'Please select a file first';
      return;
    }

    isUploading = true;
    uploadError = '';
    uploadResult = null;

    try {
      uploadResult = await invoke('upload_file_http', {
        filePath: uploadFilePath,
        serverUrl: 'http://localhost:8080'
      });
      console.log('Upload result:', uploadResult);

      // Refresh file list
      await loadSharedFiles();

      // Clear input
      uploadFilePath = '';
    } catch (error: any) {
      uploadError = error.toString();
      console.error('Upload error:', error);
    } finally {
      isUploading = false;
    }
  }

  async function loadSharedFiles() {
    isLoadingFiles = true;
    try {
      sharedFiles = await invoke('list_files_http', {
        serverUrl: 'http://localhost:8080'
      });
      console.log('Shared files:', sharedFiles);
    } catch (error) {
      console.error('Error loading files:', error);
    } finally {
      isLoadingFiles = false;
    }
  }

  async function downloadFile() {
    if (!downloadHash || !downloadPath) {
      downloadError = 'Please provide both file hash and download path';
      return;
    }

    isDownloading = true;
    downloadError = '';
    downloadSuccess = '';

    try {
      const result = await invoke('download_file_http', {
        fileHash: downloadHash,
        outputPath: downloadPath,
        serverUrl: serverUrl
      });
      downloadSuccess = `File downloaded to: ${result}`;
      downloadHash = '';
      downloadPath = '';
    } catch (error: any) {
      downloadError = error.toString();
      console.error('Download error:', error);
    } finally {
      isDownloading = false;
    }
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text).then(() => {
      alert('Copied to clipboard!');
    }).catch(err => {
      console.error('Copy failed:', err);
    });
  }

  function getPublicUrl(downloadUrl: string | undefined) {
    if (!downloadUrl) return '';
    if (networkInfo && networkInfo.publicIp) {
      return downloadUrl.replace('0.0.0.0', networkInfo.publicIp);
    }
    return downloadUrl;
  }

  function formatBytes(bytes: number) {
    if (bytes === 0) return '0 Bytes';
    const k = 1024;
    const sizes = ['Bytes', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return Math.round((bytes / Math.pow(k, i)) * 100) / 100 + ' ' + sizes[i];
  }

  function formatTimestamp(timestamp: number) {
    return new Date(timestamp * 1000).toLocaleString();
  }

  async function startTunnel() {
    isTunnelStarting = true;
    tunnelError = '';
    try {
      const publicUrl = await invoke('start_tunnel_auto', { port: 8080 });
      console.log('🌐 Tunnel started:', publicUrl);

      // Get tunnel info
      tunnelInfo = await invoke('get_tunnel_info');
      console.log('Tunnel info:', tunnelInfo);
    } catch (error: any) {
      tunnelError = error.toString();
      console.error('Tunnel error:', error);
    } finally {
      isTunnelStarting = false;
    }
  }

  async function stopTunnel() {
    try {
      await invoke('stop_tunnel');
      tunnelInfo = null;
      console.log('🛑 Tunnel stopped');
    } catch (error: any) {
      tunnelError = error.toString();
      console.error('Stop tunnel error:', error);
    }
  }

  async function refreshTunnelInfo() {
    try {
      tunnelInfo = await invoke('get_tunnel_info');
    } catch (error) {
      console.error('Error getting tunnel info:', error);
    }
  }
</script>

<div class="container mx-auto p-6 max-w-6xl">
  <div class="mb-8">
    <h1 class="text-3xl font-bold mb-2">HTTP File Sharing</h1>
    <p class="text-muted-foreground">Share files directly over HTTP with automatic port forwarding (UPnP)</p>
  </div>

  <!-- Network Setup Section -->
  <div class="card mb-6">
    <div class="card-header">
      <h2 class="text-xl font-semibold flex items-center gap-2">
        <Network class="w-5 h-5" />
        Network Configuration
      </h2>
    </div>
    <div class="card-body">
      <button
        on:click={setupNetwork}
        disabled={isLoadingNetwork}
        class="btn btn-primary mb-4"
      >
        {#if isLoadingNetwork}
          Setting up network...
        {:else}
          Setup Network & UPnP
        {/if}
      </button>

      {#if networkError}
        <div class="alert alert-error mb-4">
          <XCircle class="w-5 h-5" />
          <span>{networkError}</span>
        </div>
      {/if}

      {#if networkInfo}
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div class="info-box">
            <div class="text-sm text-muted-foreground mb-1">Public IP</div>
            <div class="font-mono font-semibold flex items-center gap-2">
              <Globe class="w-4 h-4" />
              {networkInfo.publicIp}
              <button on:click={() => copyToClipboard(networkInfo.publicIp)} class="btn-icon">
                <Copy class="w-4 h-4" />
              </button>
            </div>
          </div>

          <div class="info-box">
            <div class="text-sm text-muted-foreground mb-1">Local IP</div>
            <div class="font-mono font-semibold">{networkInfo.localIp}</div>
          </div>

          <div class="info-box">
            <div class="text-sm text-muted-foreground mb-1">HTTP Server URL</div>
            <div class="font-mono text-sm break-all">{networkInfo.httpServerUrl}</div>
          </div>

          <div class="info-box">
            <div class="text-sm text-muted-foreground mb-1">UPnP Status</div>
            <div class="flex items-center gap-2">
              {#if networkInfo.upnpEnabled && networkInfo.portForwarded}
                <CheckCircle class="w-5 h-5 text-green-500" />
                <span class="text-green-500 font-semibold">Active</span>
              {:else}
                <XCircle class="w-5 h-5 text-red-500" />
                <span class="text-red-500 font-semibold">Inactive</span>
              {/if}
            </div>
          </div>
        </div>

        {#if !networkInfo.upnpEnabled || !networkInfo.portForwarded}
          <div class="alert alert-warning mt-4">
            <span>UPnP failed. You may need to manually configure port forwarding on your router.</span>
          </div>
        {/if}
      {/if}
    </div>
  </div>

  <!-- Internet Tunnel Section -->
  <div class="card mb-6">
    <div class="card-header">
      <h2 class="text-xl font-semibold flex items-center gap-2">
        <Globe class="w-5 h-5" />
        Internet Sharing Tunnel
      </h2>
    </div>
    <div class="card-body">
      <p class="text-sm text-muted-foreground mb-4">
        Enable a tunnel to share files over the internet even behind firewalls. This uses localtunnel to create a public URL.
      </p>

      {#if !tunnelInfo || !tunnelInfo.is_active}
        <button
          on:click={startTunnel}
          disabled={isTunnelStarting}
          class="btn btn-primary mb-4"
        >
          {#if isTunnelStarting}
            Starting Tunnel...
          {:else}
            Start Internet Tunnel
          {/if}
        </button>
      {:else}
        <div class="space-y-4">
          <div class="alert alert-success">
            <CheckCircle class="w-5 h-5" />
            <div class="flex-1">
              <div class="font-semibold mb-2">Tunnel Active!</div>
              <div class="text-sm">
                <div><strong>Public URL:</strong></div>
                <div class="flex items-center gap-2 mt-1">
                  <input
                    type="text"
                    value={tunnelInfo.public_url || ''}
                    readonly
                    class="input input-sm flex-1 font-mono text-xs"
                  />
                  <button
                    on:click={() => copyToClipboard(tunnelInfo.public_url || '')}
                    class="btn btn-sm"
                  >
                    <Copy class="w-4 h-4" />
                  </button>
                </div>
                <div class="mt-2 text-xs text-muted-foreground">
                  Share this URL with anyone to let them access your files over the internet.
                </div>
              </div>
            </div>
          </div>

          <button
            on:click={stopTunnel}
            class="btn btn-secondary"
          >
            Stop Tunnel
          </button>
        </div>
      {/if}

      {#if tunnelError}
        <div class="alert alert-error">
          <XCircle class="w-5 h-5" />
          <div class="flex-1">
            <div class="font-semibold mb-1">Tunnel Error</div>
            <div class="text-sm">{tunnelError}</div>
            {#if tunnelError.includes('not installed')}
              <div class="mt-2 text-xs">
                Install localtunnel with: <code class="bg-black/10 px-1 py-0.5 rounded">npm install -g localtunnel</code>
              </div>
            {/if}
          </div>
        </div>
      {/if}
    </div>
  </div>

  <!-- Upload Section -->
  <div class="card mb-6">
    <div class="card-header">
      <h2 class="text-xl font-semibold flex items-center gap-2">
        <Upload class="w-5 h-5" />
        Upload & Share File
      </h2>
    </div>
    <div class="card-body">
      <div class="flex gap-2 mb-4">
        <input
          type="text"
          bind:value={uploadFilePath}
          placeholder="File path or click Select File..."
          class="input flex-1"
          readonly
        />
        <button on:click={selectFile} class="btn btn-secondary">
          Select File
        </button>
      </div>

      <button
        on:click={uploadFile}
        disabled={isUploading || !uploadFilePath}
        class="btn btn-primary mb-4"
      >
        {#if isUploading}
          Uploading...
        {:else}
          Upload File
        {/if}
      </button>

      {#if uploadError}
        <div class="alert alert-error mb-4">
          <XCircle class="w-5 h-5" />
          <span>{uploadError}</span>
        </div>
      {/if}

      {#if uploadResult}
        <div class="alert alert-success mb-4">
          <CheckCircle class="w-5 h-5" />
          <div class="flex-1">
            <div class="font-semibold mb-2">Upload Successful!</div>
            <div class="space-y-1 text-sm">
              <div><strong>File:</strong> {uploadResult.file_name}</div>
              <div><strong>Size:</strong> {formatBytes(uploadResult.file_size)}</div>
              <div><strong>Hash:</strong> <span class="font-mono text-xs">{uploadResult.file_hash}</span></div>
              <div class="mt-2">
                <strong>Download URL:</strong>
                <div class="flex items-center gap-2 mt-1">
                  <input
                    type="text"
                    value={getPublicUrl(uploadResult.download_url)}
                    readonly
                    class="input input-sm flex-1 font-mono text-xs"
                  />
                  <button
                    on:click={() => copyToClipboard(getPublicUrl(uploadResult.download_url))}
                    class="btn btn-sm"
                  >
                    <Copy class="w-4 h-4" />
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      {/if}
    </div>
  </div>

  <!-- Shared Files List -->
  <div class="card mb-6">
    <div class="card-header">
      <h2 class="text-xl font-semibold flex items-center gap-2">
        <Server class="w-5 h-5" />
        Shared Files ({sharedFiles.length})
      </h2>
      <button on:click={loadSharedFiles} class="btn btn-sm" disabled={isLoadingFiles}>
        {isLoadingFiles ? 'Loading...' : 'Refresh'}
      </button>
    </div>
    <div class="card-body">
      {#if sharedFiles.length === 0}
        <div class="text-center text-muted-foreground py-8">
          No files shared yet. Upload a file to get started!
        </div>
      {:else}
        <div class="space-y-2">
          {#each sharedFiles as file}
            <div class="file-item">
              <div class="flex-1">
                <div class="font-semibold">{file.file_name}</div>
                <div class="text-sm text-muted-foreground">
                  {formatBytes(file.file_size)} • Uploaded {formatTimestamp(file.upload_time)}
                </div>
                <div class="text-xs font-mono text-muted-foreground mt-1">
                  Hash: {file.file_hash.substring(0, 16)}...
                </div>
              </div>
              <button
                on:click={() => copyToClipboard(getPublicUrl(file.download_url))}
                class="btn btn-sm"
                title="Copy public URL"
              >
                <Copy class="w-4 h-4" />
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>

  <!-- Download Section -->
  <div class="card">
    <div class="card-header">
      <h2 class="text-xl font-semibold flex items-center gap-2">
        <Download class="w-5 h-5" />
        Download File
      </h2>
    </div>
    <div class="card-body">
      <div class="space-y-4">
        <div>
          <label class="label">Server URL</label>
          <input
            type="text"
            bind:value={serverUrl}
            placeholder="http://example.com:8080"
            class="input"
          />
        </div>

        <div>
          <label class="label">File Hash</label>
          <input
            type="text"
            bind:value={downloadHash}
            placeholder="Enter file hash..."
            class="input font-mono"
          />
        </div>

        <div>
          <label class="label">Save to Path</label>
          <input
            type="text"
            bind:value={downloadPath}
            placeholder="/path/to/save/file.txt"
            class="input"
          />
        </div>

        <button
          on:click={downloadFile}
          disabled={isDownloading || !downloadHash || !downloadPath}
          class="btn btn-primary"
        >
          {#if isDownloading}
            Downloading...
          {:else}
            Download File
          {/if}
        </button>

        {#if downloadError}
          <div class="alert alert-error">
            <XCircle class="w-5 h-5" />
            <span>{downloadError}</span>
          </div>
        {/if}

        {#if downloadSuccess}
          <div class="alert alert-success">
            <CheckCircle class="w-5 h-5" />
            <span>{downloadSuccess}</span>
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .card {
    @apply bg-card rounded-lg border border-border shadow-sm;
  }

  .card-header {
    @apply px-6 py-4 border-b border-border flex items-center justify-between;
  }

  .card-body {
    @apply px-6 py-4;
  }

  .info-box {
    @apply bg-muted/50 p-4 rounded-lg border border-border;
  }

  .file-item {
    @apply flex items-center gap-4 p-4 bg-muted/30 rounded-lg border border-border hover:bg-muted/50 transition-colors;
  }

  .btn {
    @apply px-4 py-2 rounded-md font-medium transition-colors inline-flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed;
  }

  .btn-primary {
    @apply bg-primary text-primary-foreground hover:bg-primary/90;
  }

  .btn-secondary {
    @apply bg-secondary text-secondary-foreground hover:bg-secondary/80;
  }

  .btn-sm {
    @apply px-3 py-1.5 text-sm;
  }

  .btn-icon {
    @apply p-1 hover:bg-muted rounded transition-colors;
  }

  .input {
    @apply w-full px-3 py-2 bg-background border border-input rounded-md focus:outline-none focus:ring-2 focus:ring-ring;
  }

  .input-sm {
    @apply px-2 py-1 text-sm;
  }

  .label {
    @apply block text-sm font-medium mb-2;
  }

  .alert {
    @apply flex items-start gap-3 p-4 rounded-lg border;
  }

  .alert-error {
    @apply bg-red-500/10 border-red-500/20 text-red-600 dark:text-red-400;
  }

  .alert-success {
    @apply bg-green-500/10 border-green-500/20 text-green-600 dark:text-green-400;
  }

  .alert-warning {
    @apply bg-yellow-500/10 border-yellow-500/20 text-yellow-600 dark:text-yellow-400;
  }
</style>
