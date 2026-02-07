<script lang="ts">
  import { X, FileText, ExternalLink } from 'lucide-svelte';
  import { fade, scale } from 'svelte/transition';
  import { getFileType, type PreviewType } from '$lib/utils/fileTypeDetector';
  import { convertFileSrc } from '@tauri-apps/api/core';

  export let isOpen = false;
  export let fileName: string = '';
  export let filePath: string = '';
  export let fileSize: number = 0;
  
  let previewType: PreviewType = 'unsupported';
  let fileUrl: string = '';
  let textContent: string = '';
  let isLoading = true;
  let loadError: string = '';

  $: if (isOpen && fileName) {
    loadPreview();
  }

  async function loadPreview() {
    isLoading = true;
    loadError = '';
    
    const fileInfo = getFileType(fileName);
    previewType = fileInfo.type;

    try {
      if (previewType === 'text') {
        // For text files, read the content
        const { readTextFile } = await import('@tauri-apps/plugin-fs');
        textContent = await readTextFile(filePath);
        isLoading = false;
      } else if (previewType === 'image') {
        // For images, read as binary and convert to base64 data URL
        const { readFile } = await import('@tauri-apps/plugin-fs');
        const imageData = await readFile(filePath);
        const base64 = btoa(String.fromCharCode(...imageData));
        fileUrl = `data:${fileInfo.mimeType};base64,${base64}`;
        console.log('[OK] Image loaded as base64, size:', imageData.length, 'bytes');
        isLoading = false;
      } else if (['video', 'audio', 'pdf'].includes(previewType)) {
        // For video/audio/pdf, use asset protocol
        fileUrl = convertFileSrc(filePath);
        console.log('[LINK] Original path:', filePath);
        console.log('[LINK] Converted URL:', fileUrl);
        isLoading = false;
      } else {
        loadError = 'Preview not supported for this file type';
        isLoading = false;
      }
    } catch (error) {
      console.error('Failed to load file preview:', error);
      loadError = error instanceof Error ? error.message : 'Failed to load preview';
      isLoading = false;
    }
  }

  function close() {
    isOpen = false;
    textContent = '';
    fileUrl = '';
    loadError = '';
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      close();
    }
  }

  function formatFileSize(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${(bytes / Math.pow(k, i)).toFixed(2)} ${sizes[i]}`;
  }

  async function openInDefault() {
    try {
      const { Command } = await import('@tauri-apps/plugin-shell');
      await Command.create('explorer', [filePath]).execute();
    } catch (error) {
      console.error('Failed to open file:', error);
    }
  }
</script>

{#if isOpen}
  <!-- Backdrop -->
  <div
    class="fixed inset-0 bg-black/70 backdrop-blur-sm z-50 flex items-center justify-center p-4"
    on:click={handleBackdropClick}
    on:keydown={(e) => e.key === 'Escape' && close()}
    role="button"
    tabindex="-1"
    transition:fade={{ duration: 200 }}
  >
    <!-- Modal -->
    <div
      class="bg-gradient-to-br from-gray-900 to-gray-800 rounded-xl shadow-2xl w-full max-w-5xl max-h-[90vh] flex flex-col border border-gray-700"
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="preview-title"
      transition:scale={{ duration: 200, start: 0.95 }}
      on:click={(e) => e.stopPropagation()}
      on:keydown={(e) => e.stopPropagation()}
    >
      <!-- Header -->
      <div class="flex items-center justify-between p-4 border-b border-gray-700 bg-gray-800/50">
        <div class="flex items-center gap-3 flex-1 min-w-0">
          <FileText class="w-5 h-5 text-blue-400 flex-shrink-0" />
          <div class="flex-1 min-w-0">
            <h3 id="preview-title" class="text-lg font-semibold text-white truncate">
              {fileName}
            </h3>
            <p class="text-sm text-gray-400">
              {formatFileSize(fileSize)}
            </p>
          </div>
        </div>
        
        <div class="flex items-center gap-2">
          <button
            on:click={openInDefault}
            class="p-2 hover:bg-gray-700 rounded-lg transition-colors text-gray-400 hover:text-white"
            title="Open in default application"
          >
            <ExternalLink class="w-5 h-5" />
          </button>
          <button
            on:click={close}
            class="p-2 hover:bg-gray-700 rounded-lg transition-colors text-gray-400 hover:text-white"
            aria-label="Close preview"
          >
            <X class="w-5 h-5" />
          </button>
        </div>
      </div>

      <!-- Content -->
      <div class="flex-1 overflow-auto p-6">
        {#if isLoading}
          <div class="flex items-center justify-center h-full">
            <div class="text-center">
              <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500 mx-auto mb-4"></div>
              <p class="text-gray-400">Loading preview...</p>
            </div>
          </div>
        {:else if loadError}
          <div class="flex items-center justify-center h-full">
            <div class="text-center">
              <FileText class="w-16 h-16 text-gray-600 mx-auto mb-4" />
              <p class="text-red-400 mb-2">{loadError}</p>
              <p class="text-gray-500 text-sm">Try opening the file in its default application</p>
            </div>
          </div>
        {:else if previewType === 'image'}
          <div class="flex items-center justify-center h-full">
            <img 
              src={fileUrl} 
              alt={fileName}
              class="max-w-full max-h-full object-contain rounded-lg"
              on:load={() => console.log('[OK] Image loaded successfully:', fileUrl)}
              on:error={(e) => {
                console.error('[X] Image failed to load:', fileUrl, e);
                loadError = 'Failed to load image. The file may be corrupted or in an unsupported format.';
              }}
            />
          </div>
        {:else if previewType === 'video'}
          <div class="flex items-center justify-center h-full">
            <video 
              src={fileUrl}
              controls
              class="max-w-full max-h-full rounded-lg"
            >
              <track kind="captions" />
              Your browser doesn't support video playback.
            </video>
          </div>
        {:else if previewType === 'audio'}
          <div class="flex items-center justify-center h-full">
            <div class="w-full max-w-2xl">
              <div class="bg-gray-800 p-8 rounded-lg text-center mb-6">
                <FileText class="w-16 h-16 text-blue-400 mx-auto mb-4" />
                <h4 class="text-xl font-semibold text-white mb-2">{fileName}</h4>
                <p class="text-gray-400">{formatFileSize(fileSize)}</p>
              </div>
              <audio 
                src={fileUrl}
                controls
                class="w-full"
              >
                Your browser doesn't support audio playback.
              </audio>
            </div>
          </div>
        {:else if previewType === 'text'}
          <pre class="bg-gray-800 p-4 rounded-lg text-sm text-gray-300 overflow-x-auto font-mono whitespace-pre-wrap break-words">{textContent}</pre>
        {:else if previewType === 'pdf'}
          <iframe
            src={fileUrl}
            title="PDF Preview"
            class="w-full h-full min-h-[600px] rounded-lg"
          ></iframe>
        {/if}
      </div>

      <!-- Footer -->
      <div class="flex items-center justify-end gap-3 p-4 border-t border-gray-700 bg-gray-800/50">
        <button
          on:click={openInDefault}
          class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-colors flex items-center gap-2"
        >
          <ExternalLink class="w-4 h-4" />
          Open in Default App
        </button>
        <button
          on:click={close}
          class="px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white rounded-lg transition-colors"
        >
          Close
        </button>
      </div>
    </div>
  </div>
{/if}
