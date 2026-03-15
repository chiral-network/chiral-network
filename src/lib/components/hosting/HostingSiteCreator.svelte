<script lang="ts">
  import { Plus, FolderOpen, HardDrive, X, File as FileIcon, Loader2 } from 'lucide-svelte';
  import { formatHostedFileSize as formatFileSize } from '$lib/utils/hostingPageUtils';

  interface SelectedFile {
    name: string;
    path: string;
    size: number;
  }

  interface Props {
    newSiteName: string;
    selectedFiles: SelectedFile[];
    isDragOver: boolean;
    isCreating: boolean;
    onNameChange: (name: string) => void;
    onSelectFiles: () => void;
    onSelectFromDrive: () => void;
    onRemoveFile: (index: number) => void;
    onCreateSite: () => void;
  }

  let {
    newSiteName,
    selectedFiles,
    isDragOver,
    isCreating,
    onNameChange,
    onSelectFiles,
    onSelectFromDrive,
    onRemoveFile,
    onCreateSite,
  }: Props = $props();

  let canCreate = $derived(newSiteName.trim().length > 0 && selectedFiles.length > 0 && !isCreating);
  let totalSize = $derived(selectedFiles.reduce((s, f) => s + f.size, 0));
</script>

<div class="rounded-2xl border border-cyan-500/15 bg-gray-900/90 p-5 shadow-[0_0_10px_rgba(6,182,212,0.05)] backdrop-blur-sm">
  <h2 class="mb-4 text-base font-semibold text-gray-100 flex items-center gap-2">
    <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-cyan-500/10">
      <Plus class="h-3.5 w-3.5 text-cyan-400" />
    </div>
    Create New Site
  </h2>

  <div class="space-y-4">
    <!-- Site Name -->
    <div>
      <label for="site-name" class="mb-1.5 block text-xs font-medium text-gray-400 uppercase tracking-wide">Site Name</label>
      <input
        id="site-name"
        type="text"
        value={newSiteName}
        oninput={(e) => onNameChange(e.currentTarget.value)}
        placeholder="e.g. My Portfolio"
        class="w-full rounded-xl border border-cyan-500/15 bg-gray-800 px-4 py-2.5 text-sm text-gray-100 placeholder-gray-400 transition-all
          focus:border-cyan-500 focus:bg-gray-900 focus:outline-none focus:ring-2 focus:ring-cyan-500/20
          text-gray-100"
      />
    </div>

    <!-- File source buttons -->
    <div class="flex gap-2">
      <button
        onclick={onSelectFromDrive}
        class="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-cyan-400 border border-primary-200
          rounded-lg hover:bg-cyan-500/10 transition-colors
          focus:outline-none focus:ring-2 focus:ring-cyan-500/30"
      >
        <HardDrive class="w-3.5 h-3.5" />
        Add from Drive
      </button>
      <button
        onclick={onSelectFiles}
        class="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-gray-400 border border-cyan-500/20
          rounded-lg hover:bg-cyan-500/5 transition-colors
          focus:outline-none focus:ring-2 focus:ring-cyan-500/30"
      >
        <FolderOpen class="w-3.5 h-3.5" />
        Browse Files
      </button>
    </div>

    <!-- Drop Zone -->
    <div
      role="button"
      tabindex="0"
      onclick={onSelectFiles}
      onkeydown={(e) => e.key === 'Enter' && onSelectFiles()}
      class="flex cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed py-8 px-4 transition-all
        {isDragOver
          ? 'border-cyan-500/40 bg-cyan-500/10 scale-[1.01]'
          : 'border-cyan-500/15 bg-gray-800 hover:border-cyan-500/20 hover:bg-cyan-500/5'}"
    >
      <div class="flex h-12 w-12 items-center justify-center rounded-full bg-gray-800/60">
        <FolderOpen class="h-6 w-6 text-gray-500" />
      </div>
      <p class="text-sm font-medium text-gray-400">
        {isDragOver ? 'Release to add files' : 'Drop files here'}
      </p>
      <p class="text-xs text-gray-500">
        HTML, CSS, JS, images, photos, documents
      </p>
    </div>

    <!-- Selected Files -->
    {#if selectedFiles.length > 0}
      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <p class="text-xs font-medium text-gray-400 uppercase tracking-wide">
            {selectedFiles.length} file{selectedFiles.length === 1 ? '' : 's'} selected
          </p>
          <p class="text-xs text-gray-400 tabular-nums">{formatFileSize(totalSize)}</p>
        </div>
        <div class="max-h-40 overflow-y-auto rounded-xl border border-cyan-500/10 divide-y divide-cyan-500/10">
          {#each selectedFiles as file, i (file.path)}
            <div class="flex items-center justify-between px-3 py-2 hover:bg-cyan-500/5 transition-colors group">
              <div class="flex items-center gap-2.5 min-w-0">
                <FileIcon class="h-4 w-4 flex-shrink-0 text-gray-400" />
                <span class="truncate text-sm text-gray-300">{file.name}</span>
                <span class="text-xs text-gray-400 tabular-nums flex-shrink-0">{formatFileSize(file.size)}</span>
              </div>
              <button
                onclick={(e: MouseEvent) => { e.stopPropagation(); onRemoveFile(i); }}
                aria-label="Remove {file.name}"
                class="ml-2 flex-shrink-0 rounded-md p-1 text-gray-300 transition-colors
                  hover:bg-red-500/10 hover:text-red-500
                  opacity-0 group-hover:opacity-100 focus:opacity-100"
              >
                <X class="h-3.5 w-3.5" />
              </button>
            </div>
          {/each}
        </div>
      </div>

      <!-- Create Button -->
      <button
        onclick={onCreateSite}
        disabled={!canCreate}
        class="flex w-full items-center justify-center gap-2 rounded-xl px-4 py-3 text-sm font-semibold text-white transition-all
          {canCreate
            ? 'bg-cyan-500 hover:bg-cyan-600/80 shadow-[0_0_10px_rgba(6,182,212,0.05)] shadow-primary-500/20 hover:shadow-[0_0_15px_rgba(6,182,212,0.08)] hover:shadow-primary-500/25 active:scale-[0.98]'
            : 'bg-gray-300 cursor-not-allowed'}
          focus:outline-none focus:ring-2 focus:ring-cyan-500/50 focus:ring-offset-2"
      >
        {#if isCreating}
          <Loader2 class="h-4 w-4 animate-spin" />
          Creating...
        {:else}
          <Plus class="h-4 w-4" />
          Create Site
        {/if}
      </button>
    {/if}
  </div>
</div>
