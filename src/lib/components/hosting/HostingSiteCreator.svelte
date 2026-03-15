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

<div class="rounded-2xl backdrop-blur-xl bg-white/10 dark:bg-white/5 p-5 shadow-lg shadow-black/5 border border-white/20 dark:border-white/10 ring-1 ring-white/10">
  <h2 class="mb-4 text-base font-semibold text-gray-900 dark:text-white flex items-center gap-2">
    <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-primary-500/15 dark:bg-primary-500/10">
      <Plus class="h-3.5 w-3.5 text-primary-600 dark:text-primary-400" />
    </div>
    Create New Site
  </h2>

  <div class="space-y-4">
    <!-- Site Name -->
    <div>
      <label for="site-name" class="mb-1.5 block text-xs font-medium text-gray-600 dark:text-gray-400 uppercase tracking-wide">Site Name</label>
      <input
        id="site-name"
        type="text"
        value={newSiteName}
        oninput={(e) => onNameChange(e.currentTarget.value)}
        placeholder="e.g. My Portfolio"
        class="w-full rounded-xl border border-gray-200 bg-white/5 px-4 py-2.5 text-sm text-gray-900 placeholder-gray-400 transition-all
          focus:border-primary-400 focus:bg-white/10 focus:outline-none focus:ring-2 focus:ring-primary-400/20
          dark:border-white/10 dark:bg-white/5 dark:text-white dark:placeholder-gray-500 dark:focus:bg-white/5"
      />
    </div>

    <!-- File source buttons -->
    <div class="flex gap-2">
      <button
        onclick={onSelectFromDrive}
        class="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-primary-600 dark:text-primary-400 border border-primary-200 dark:border-primary-800/60
          rounded-lg hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-colors
          focus:outline-none focus:ring-2 focus:ring-primary-400/30"
      >
        <HardDrive class="w-3.5 h-3.5" />
        Add from Drive
      </button>
      <button
        onclick={onSelectFiles}
        class="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-gray-600 dark:text-gray-400 border border-white/20 dark:border-white/10
          rounded-lg hover:bg-white/10 dark:hover:bg-white/5/50 transition-colors
          focus:outline-none focus:ring-2 focus:ring-gray-400/30"
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
      class="flex cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border border-dashed py-8 px-4 transition-all
        {isDragOver
          ? 'border-primary-400/40 bg-primary-500/15 dark:border-primary-500/30 dark:bg-primary-500/10 scale-[1.01]'
          : 'border-white/20 bg-white/5 hover:border-white/30 hover:bg-white/8 dark:border-white/10 dark:bg-white/5 dark:hover:border-white/15'}"
    >
      <div class="flex h-12 w-12 items-center justify-center rounded-full bg-white/10 dark:bg-white/5">
        <FolderOpen class="h-6 w-6 text-gray-400 dark:text-gray-500" />
      </div>
      <p class="text-sm font-medium text-gray-600 dark:text-gray-400">
        {isDragOver ? 'Release to add files' : 'Drop files here'}
      </p>
      <p class="text-xs text-gray-400 dark:text-gray-500">
        HTML, CSS, JS, images, photos, documents
      </p>
    </div>

    <!-- Selected Files -->
    {#if selectedFiles.length > 0}
      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <p class="text-xs font-medium text-gray-600 dark:text-gray-400 uppercase tracking-wide">
            {selectedFiles.length} file{selectedFiles.length === 1 ? '' : 's'} selected
          </p>
          <p class="text-xs text-gray-400 tabular-nums">{formatFileSize(totalSize)}</p>
        </div>
        <div class="max-h-40 overflow-y-auto rounded-xl border border-white/15 dark:border-white/10 divide-y divide-white/10 dark:divide-white/5">
          {#each selectedFiles as file, i (file.path)}
            <div class="flex items-center justify-between px-3 py-2 hover:bg-white/8 dark:hover:bg-white/5 transition-colors group">
              <div class="flex items-center gap-2.5 min-w-0">
                <FileIcon class="h-4 w-4 flex-shrink-0 text-gray-400" />
                <span class="truncate text-sm text-gray-700 dark:text-gray-300">{file.name}</span>
                <span class="text-xs text-gray-400 tabular-nums flex-shrink-0">{formatFileSize(file.size)}</span>
              </div>
              <button
                onclick={(e: MouseEvent) => { e.stopPropagation(); onRemoveFile(i); }}
                aria-label="Remove {file.name}"
                class="ml-2 flex-shrink-0 rounded-md p-1 text-gray-300 transition-colors
                  hover:bg-red-100 hover:text-red-500 dark:hover:bg-red-900/30 dark:text-gray-500
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
            ? 'bg-primary-500 hover:bg-primary-500/90 dark:hover:bg-primary-600/80 shadow-sm shadow-primary-500/20 hover:shadow-md hover:shadow-primary-500/25 active:scale-[0.98]'
            : 'bg-white/20 dark:bg-white/10 cursor-not-allowed'}
          focus:outline-none focus:ring-2 focus:ring-primary-500/50 focus:ring-offset-2 "
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
