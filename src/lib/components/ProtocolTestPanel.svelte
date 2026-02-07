<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { downloadDir, join } from '@tauri-apps/api/path';
  import Button from '$lib/components/ui/button.svelte';
  import Card from '$lib/components/ui/card.svelte';
  import { CheckCircle, XCircle, Loader2, ChevronDown, ChevronUp, FlaskConical } from 'lucide-svelte';
  import { showToast } from '$lib/toast';
  import { parseEd2kLink } from '$lib/utils/ed2kParser';

  // Check if running inside Tauri desktop app
  const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  let isExpanded = false;
  let ftpTestStatus: 'idle' | 'testing' | 'success' | 'error' = 'idle';
  let ftpTestMessage = '';

  let ed2kTestStatus: 'idle' | 'testing' | 'success' | 'error' = 'idle';
  let ed2kTestMessage = '';

  async function testFtpDownload() {
    // FTP requires Tauri backend - cannot work in browser
    if (!isTauri) {
      ftpTestStatus = 'error';
      ftpTestMessage = '[X] FTP downloads require the Tauri desktop app. Run with: npm run tauri:dev';
      showToast('FTP requires the desktop app', 'error');
      return;
    }

    ftpTestStatus = 'testing';
    ftpTestMessage = 'Downloading from ftp.gnu.org...';

    try {
      // Use cross-platform Downloads folder with non-conflicting filename
      const downloadsPath = await downloadDir();
      const timestamp = Date.now();
      const filename = `chiral-ftp-test-${timestamp}.tar.gz`;
      const outputPath = await join(downloadsPath, filename);

      await invoke('start_ftp_download', {
        url: 'ftp://ftp.gnu.org/gnu/hello/hello-2.12.tar.gz',
        outputPath: outputPath,
        username: null,
        password: null
      });

      ftpTestStatus = 'success';
      ftpTestMessage = `[OK] Success! Saved: ${outputPath} (~1 MB)`;
      showToast('FTP test successful!', 'success');
    } catch (error) {
      ftpTestStatus = 'error';
      ftpTestMessage = `[X] Error: ${error}`;
      showToast(`FTP test failed: ${error}`, 'error');
    }
  }

  function testEd2kParsing() {
    ed2kTestStatus = 'testing';
    ed2kTestMessage = 'Parsing ED2K link...';

    try {
      // Use browser-native parser (works in both browser and Tauri)
      const result = parseEd2kLink(
        'ed2k://|file|ubuntu-22.04-desktop-amd64.iso|3654957056|31D6CFE0D16AE931B73C59D7E0C089C0|/'
      );

      if (result.type === 'file') {
        const { file_name, file_size } = result.data;
        ed2kTestStatus = 'success';
        ed2kTestMessage = `[OK] Success! Parsed: ${file_name} (${(file_size / 1024 / 1024 / 1024).toFixed(2)} GB)`;
        showToast('ED2K parsing successful!', 'success');
      }
    } catch (error) {
      ed2kTestStatus = 'error';
      ed2kTestMessage = `[X] Error: ${error}`;
      showToast(`ED2K test failed: ${error}`, 'error');
    }
  }

  function resetTests() {
    ftpTestStatus = 'idle';
    ftpTestMessage = '';
    ed2kTestStatus = 'idle';
    ed2kTestMessage = '';
  }
</script>

<!-- DEV ONLY: Collapsible Protocol Tests -->
<Card class="border-dashed border-muted-foreground/30">
  <!-- Collapsed Header -->
  <button
    on:click={() => isExpanded = !isExpanded}
    class="w-full px-4 py-3 flex items-center justify-between text-left hover:bg-muted/50 transition-colors"
  >
    <div class="flex items-center gap-2 text-sm text-muted-foreground">
      <FlaskConical class="h-4 w-4" />
      <span class="font-medium">Developer: Protocol Tests</span>
      <span class="text-xs opacity-60">(FTP & ED2K)</span>
    </div>
    <div class="flex items-center gap-2">
      {#if ftpTestStatus === 'success' || ed2kTestStatus === 'success'}
        <CheckCircle class="h-4 w-4 text-green-600" />
      {/if}
      {#if isExpanded}
        <ChevronUp class="h-4 w-4" />
      {:else}
        <ChevronDown class="h-4 w-4" />
      {/if}
    </div>
  </button>

  <!-- Expanded Content -->
  {#if isExpanded}
    <div class="px-4 pb-4 space-y-3 border-t">
      <p class="text-xs text-muted-foreground pt-3">
        Test protocol handlers with real servers (for development only)
      </p>

      <!-- FTP Test -->
      <div class="space-y-2">
        <Button
          on:click={testFtpDownload}
          disabled={ftpTestStatus === 'testing'}
          size="sm"
          variant="outline"
          class="w-full"
        >
          {#if ftpTestStatus === 'testing'}
            <Loader2 class="h-3 w-3 mr-2 animate-spin" />
            Testing FTP...
          {:else if ftpTestStatus === 'success'}
            <CheckCircle class="h-3 w-3 mr-2" />
            FTP Test Passed
          {:else if ftpTestStatus === 'error'}
            <XCircle class="h-3 w-3 mr-2" />
            FTP Test Failed - Retry?
          {:else}
            [NET] Test FTP Download (GNU Hello - ~1MB)
          {/if}
        </Button>

        {#if ftpTestMessage}
          <p class="text-xs px-2 py-1 rounded bg-muted/50 border text-muted-foreground">
            {ftpTestMessage}
          </p>
        {/if}
      </div>

      <!-- ED2K Test -->
      <div class="space-y-2">
        <Button
          on:click={testEd2kParsing}
          disabled={ed2kTestStatus === 'testing'}
          size="sm"
          variant="outline"
          class="w-full"
        >
          {#if ed2kTestStatus === 'testing'}
            <Loader2 class="h-3 w-3 mr-2 animate-spin" />
            Testing ED2K...
          {:else if ed2kTestStatus === 'success'}
            <CheckCircle class="h-3 w-3 mr-2" />
            ED2K Test Passed
          {:else if ed2kTestStatus === 'error'}
            <XCircle class="h-3 w-3 mr-2" />
            ED2K Test Failed - Retry?
          {:else}
            [LINK] Test ED2K Link Parsing (Ubuntu ISO)
          {/if}
        </Button>

        {#if ed2kTestMessage}
          <p class="text-xs px-2 py-1 rounded bg-muted/50 border text-muted-foreground">
            {ed2kTestMessage}
          </p>
        {/if}
      </div>

      <div class="pt-2 border-t space-y-1">
        <div class="flex items-center justify-between">
          <p class="text-xs text-muted-foreground">
            Remove this panel before production
          </p>
          {#if ftpTestStatus !== 'idle' || ed2kTestStatus !== 'idle'}
            <Button size="sm" variant="ghost" on:click={resetTests} class="h-6 text-xs">
              Reset
            </Button>
          {/if}
        </div>
        <p class="text-[10px] text-muted-foreground/70 font-mono">
          src/lib/components/ProtocolTestPanel.svelte
        </p>
      </div>
    </div>
  {/if}
</Card>
