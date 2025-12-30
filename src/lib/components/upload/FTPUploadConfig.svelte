<script lang="ts">
  import { Server, Lock, Eye, EyeOff, TestTube, CheckCircle, XCircle, Loader2, AlertCircle } from 'lucide-svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { showToast } from '$lib/toast';

  // FTP server configuration
  export let ftpUrl = '';
  export let ftpUsername = '';
  export let ftpPassword = '';
  export let ftpUseFTPS = false;
  export let ftpPassiveMode = true;

  let showPassword = false;
  let testStatus: 'idle' | 'testing' | 'success' | 'error' = 'idle';
  let testMessage = '';

  // Validate FTP URL format
  function validateFtpUrl(url: string): boolean {
    if (!url) return false;
    const ftpUrlPattern = /^ftp:\/\/.+/i;
    return ftpUrlPattern.test(url);
  }

  // Test FTP connection
  async function testConnection() {
    if (!ftpUrl) {
      showToast('Please enter an FTP server URL', 'error');
      return;
    }

    if (!validateFtpUrl(ftpUrl)) {
      showToast('Invalid FTP URL format. Must start with ftp://', 'error');
      return;
    }

    testStatus = 'testing';
    testMessage = 'Testing connection to FTP server...';

    try {
      // Test connection using backend
      await invoke('test_ftp_connection', {
        url: ftpUrl,
        username: ftpUsername || null,
        password: ftpPassword || null,
        useFtps: ftpUseFTPS,
        passiveMode: ftpPassiveMode,
      });

      testStatus = 'success';
      testMessage = 'Connection successful!';
      showToast('FTP connection test successful', 'success');
    } catch (error) {
      testStatus = 'error';
      testMessage = `Connection failed: ${error}`;
      showToast(`FTP test failed: ${error}`, 'error');
    }
  }

  // Reset test status when URL changes
  $: if (ftpUrl) {
    testStatus = 'idle';
    testMessage = '';
  }
</script>

<div class="space-y-4">
  <!-- Important Notice -->
  <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-3">
    <div class="flex items-start gap-2">
      <AlertCircle class="h-5 w-5 text-yellow-900 flex-shrink-0 mt-0.5" />
      <div class="space-y-1">
        <p class="text-sm font-semibold text-yellow-900">
          FTP Server Required
        </p>
        <p class="text-xs text-yellow-700 mt-1">
          FTP upload requires access to an <strong>external FTP server</strong> (web hosting, university server, or self-hosted).
          <strong>Most users should use P2P protocols</strong> (WebRTC, Bitswap, BitTorrent) which don't need a server.
        </p>
        <details class="text-xs text-yellow-700 mt-2">
          <summary class="cursor-pointer hover:underline">How to get FTP access?</summary>
          <ul class="list-disc list-inside mt-1 space-y-0.5 ml-2">
            <li>Web hosting providers (shared hosting, VPS)</li>
            <li>University/company FTP servers</li>
            <li>Local FTP server for testing: <code class="bg-yellow-100 px-1 rounded text-yellow-900">python -m pyftpdlib</code></li>
          </ul>
        </details>
      </div>
    </div>
  </div>

  <div class="flex items-center gap-2 text-sm text-muted-foreground">
    <Server class="h-4 w-4" />
    <span>Configure your FTP server to upload files</span>
  </div>

  <!-- FTP Server URL -->
  <div class="space-y-2">
    <Label for="ftp-url">FTP Server URL *</Label>
    <Input
      id="ftp-url"
      type="text"
      placeholder="ftp://ftp.example.com/uploads/"
      bind:value={ftpUrl}
      class="font-mono text-sm"
    />
    <p class="text-xs text-muted-foreground">
      Example: ftp://ftp.example.com/path/to/directory/
    </p>
  </div>

  <!-- Username (Optional) -->
  <div class="space-y-2">
    <Label for="ftp-username">Username (optional)</Label>
    <Input
      id="ftp-username"
      type="text"
      placeholder="username"
      bind:value={ftpUsername}
    />
  </div>

  <!-- Password (Optional) -->
  <div class="space-y-2">
    <Label for="ftp-password">Password (optional)</Label>
    <div class="relative">
      <Input
        id="ftp-password"
        type={showPassword ? 'text' : 'password'}
        placeholder="password"
        bind:value={ftpPassword}
        class="pr-10"
      />
      <button
        type="button"
        on:click={() => showPassword = !showPassword}
        class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
        aria-label={showPassword ? 'Hide password' : 'Show password'}
      >
        {#if showPassword}
          <EyeOff class="h-4 w-4" />
        {:else}
          <Eye class="h-4 w-4" />
        {/if}
      </button>
    </div>
  </div>

  <!-- FTP Options -->
  <div class="space-y-3 pt-2 border-t">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Lock class="h-4 w-4 text-muted-foreground" />
        <Label for="ftp-ftps" class="cursor-pointer">Use FTPS (Secure FTP)</Label>
      </div>
      <input
        id="ftp-ftps"
        type="checkbox"
        bind:checked={ftpUseFTPS}
        class="h-4 w-4 rounded border-gray-300"
      />
    </div>

    <div class="flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Server class="h-4 w-4 text-muted-foreground" />
        <Label for="ftp-passive" class="cursor-pointer">Passive Mode (Recommended)</Label>
      </div>
      <input
        id="ftp-passive"
        type="checkbox"
        bind:checked={ftpPassiveMode}
        class="h-4 w-4 rounded border-gray-300"
      />
    </div>
  </div>

  <!-- Test Connection Button -->
  <div class="pt-2">
    <Button
      on:click={testConnection}
      disabled={!ftpUrl || testStatus === 'testing'}
      size="sm"
      variant="outline"
      class="w-full"
    >
      {#if testStatus === 'testing'}
        <Loader2 class="h-4 w-4 mr-2 animate-spin" />
        Testing Connection...
      {:else if testStatus === 'success'}
        <CheckCircle class="h-4 w-4 mr-2 text-green-600" />
        Connection Successful
      {:else if testStatus === 'error'}
        <XCircle class="h-4 w-4 mr-2 text-red-600" />
        Test Failed - Retry?
      {:else}
        <TestTube class="h-4 w-4 mr-2" />
        Test Connection
      {/if}
    </Button>

    {#if testMessage}
      <p class="text-xs mt-2 px-2 py-1.5 rounded bg-muted/50 border {testStatus === 'success' ? 'text-green-700 border-green-200' : testStatus === 'error' ? 'text-red-700 border-red-200' : 'text-muted-foreground'}">
        {testMessage}
      </p>
    {/if}
  </div>

  <!-- Info Section -->
  <div class="pt-2 border-t">
    <p class="text-xs text-muted-foreground">
      <strong>Note:</strong> Files will be uploaded to your FTP server. The FTP URL will be added to the file's metadata so others can download from your server.
    </p>
  </div>
</div>
