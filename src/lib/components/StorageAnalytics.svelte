<script lang="ts">
import { invoke } from '@tauri-apps/api/core';
import Card from '$lib/components/ui/card.svelte';
import Progress from '$lib/components/ui/progress.svelte';
import Button from '$lib/components/ui/button.svelte';
import { HardDrive, Trash2, Database, FolderOpen, FileArchive } from 'lucide-svelte';
import { onMount } from 'svelte';
import { t } from 'svelte-i18n';
import { settings } from '$lib/stores';
import { showToast } from '$lib/toast';
import { toHumanReadableSize } from '$lib/utils';

type StorageUsage = {
  totalBytes: number;
  downloadsBytes: number;
  blockstoreBytes: number;
  tempBytes: number;
  chunkStorageBytes: number;
  availableBytes: number;
  timestamp: number;
};

type CleanupReport = {
  bytesFreed: number;
  filesDeleted: number;
  durationMs: number;
  errors: string[];
  downloadsFreed: number;
  tempFreed: number;
  orphanedFreed: number;
};

let storageUsage: StorageUsage | null = null;
let loading = false;
let cleaning = false;
let lastCleanupReport: CleanupReport | null = null;
let updateInterval: number;


function getUsagePercentage(): number {
  if (!storageUsage) return 0;
  const maxBytes = $settings.maxStorageSize * 1024 * 1024 * 1024; // GB to bytes
  return (storageUsage.totalBytes / maxBytes) * 100;
}

function getUsageColor(): string {
  const percentage = getUsagePercentage();
  if (percentage >= 90) return 'text-red-600';
  if (percentage >= 75) return 'text-yellow-600';
  return 'text-green-600';
}

function getProgressColor(): string {
  const percentage = getUsagePercentage();
  if (percentage >= 90) return 'bg-red-500';
  if (percentage >= 75) return 'bg-yellow-500';
  return 'bg-green-500';
}

async function fetchStorageUsage() {
  try {
    loading = true;
    storageUsage = await invoke<StorageUsage>('get_storage_usage');
  } catch (error) {
    console.error('Failed to fetch storage usage:', error);
    showToast(`Failed to load storage data: ${error}`, 'error');
  } finally {
    loading = false;
  }
}

async function performCleanup() {
  try {
    cleaning = true;
    showToast('Starting storage cleanup...', 'info');

    const report = await invoke<CleanupReport>('force_storage_cleanup');
    lastCleanupReport = report;

    if (report.bytesFreed > 0) {
      showToast(`Cleanup complete! Freed ${toHumanReadableSize(report.bytesFreed)}`, 'success');
    } else {
      showToast('No cleanup needed - storage is clean!', 'info');
    }

    // Refresh usage after cleanup
    await fetchStorageUsage();
  } catch (error) {
    console.error('Cleanup failed:', error);
    showToast(`Cleanup failed: ${error}`, 'error');
  } finally {
    cleaning = false;
  }
}

onMount(() => {
  fetchStorageUsage();

  // Update every 30 seconds
  updateInterval = window.setInterval(() => {
    fetchStorageUsage();
  }, 30000);

  return () => {
    clearInterval(updateInterval);
  };
});

// Calculate breakdown percentages for chart
$: breakdownData = storageUsage ? [
  {
    name: 'Downloads',
    value: storageUsage.downloadsBytes,
    color: 'rgb(59, 130, 246)',
    icon: FolderOpen
  },
  {
    name: 'Blockstore',
    value: storageUsage.blockstoreBytes,
    color: 'rgb(168, 85, 247)',
    icon: Database
  },
  {
    name: 'Temp Files',
    value: storageUsage.tempBytes,
    color: 'rgb(234, 179, 8)',
    icon: FileArchive
  },
  {
    name: 'Chunks',
    value: storageUsage.chunkStorageBytes,
    color: 'rgb(34, 197, 94)',
    icon: Database
  },
].filter(item => item.value > 0) : [];

$: totalNonZero = breakdownData.reduce((sum, item) => sum + item.value, 0);
</script>

<div class="grid grid-cols-1 md:grid-cols-2 gap-6">
  <!-- Storage Overview Card -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">{$t('analytics.storage.title')}</h2>
      <div class="p-2 bg-purple-500/10 rounded-lg">
        <HardDrive class="h-5 w-5 text-purple-500" />
      </div>
    </div>

    {#if loading && !storageUsage}
      <div class="flex items-center justify-center py-8">
        <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
      </div>
    {:else if storageUsage}
      <div class="space-y-4">
        <!-- Total Usage -->
        <div>
          <div class="flex justify-between items-baseline mb-2">
            <span class="text-sm text-muted-foreground">{$t('analytics.storage.totalUsage')}</span>
            <span class="text-2xl font-bold {getUsageColor()}">
              {toHumanReadableSize(storageUsage.totalBytes)}
            </span>
          </div>
          <Progress
            value={getUsagePercentage()}
            max={100}
            class="h-2"
            indicatorClass={getProgressColor()}
          />
          <div class="flex justify-between text-xs text-muted-foreground mt-1">
            <span>{getUsagePercentage().toFixed(1)}% of {$settings.maxStorageSize} GB limit</span>
            <span>{toHumanReadableSize(storageUsage.availableBytes)} available</span>
          </div>
        </div>

        <!-- Cleanup Status -->
        {#if getUsagePercentage() >= $settings.cleanupThreshold}
          <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-3">
            <div class="flex items-center gap-2 text-yellow-800 text-sm">
              <Trash2 class="h-4 w-4" />
              <span class="font-medium">{$t('analytics.storage.cleanupNeeded')}</span>
            </div>
            <p class="text-xs text-yellow-700 mt-1">
              {$t('analytics.storage.cleanupHint', { values: { threshold: $settings.cleanupThreshold } })}
            </p>
          </div>
        {/if}

        <!-- Cleanup Button -->
        <Button
          class="w-full"
          on:click={performCleanup}
          disabled={cleaning}
        >
          {#if cleaning}
            <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-white mr-2"></div>
            {$t('analytics.storage.cleaning')}
          {:else}
            <Trash2 class="h-4 w-4 mr-2" />
            {$t('analytics.storage.cleanupNow')}
          {/if}
        </Button>

        <!-- Last Cleanup Info -->
        {#if lastCleanupReport}
          <div class="border-t pt-3 text-xs text-muted-foreground">
            <p class="font-medium mb-1">{$t('analytics.storage.lastCleanup')}</p>
            <div class="space-y-0.5">
              <p>• {$t('analytics.storage.filesDeleted')}: {lastCleanupReport.filesDeleted}</p>
              <p>• {$t('analytics.storage.spaceFreed')}: {toHumanReadableSize(lastCleanupReport.bytesFreed)}</p>
              {#if lastCleanupReport.errors.length > 0}
                <p class="text-red-600">• {$t('analytics.storage.errors')}: {lastCleanupReport.errors.length}</p>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {:else}
      <p class="text-sm text-muted-foreground text-center py-8">
        {$t('analytics.storage.noData')}
      </p>
    {/if}
  </Card>

  <!-- Storage Breakdown Card -->
  <Card class="p-6">
    <h2 class="text-lg font-semibold mb-4">{$t('analytics.storage.breakdown')}</h2>

    {#if loading && !storageUsage}
      <div class="flex items-center justify-center py-8">
        <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
      </div>
    {:else if storageUsage && breakdownData.length > 0}
      <!-- Bar Chart -->
      <div class="space-y-3 mb-6">
        {#each breakdownData as item}
          {@const percentage = (item.value / totalNonZero) * 100}
          <div>
            <div class="flex items-center justify-between mb-1">
              <div class="flex items-center gap-2">
                <svelte:component this={item.icon} class="h-4 w-4" style="color: {item.color}" />
                <span class="text-sm">{item.name}</span>
              </div>
              <span class="text-sm font-medium">{toHumanReadableSize(item.value)}</span>
            </div>
            <div class="relative h-2 bg-muted rounded-full overflow-hidden">
              <div
                class="absolute left-0 top-0 h-full rounded-full transition-all"
                style="width: {percentage}%; background-color: {item.color}"
              ></div>
            </div>
            <div class="text-xs text-muted-foreground mt-0.5 text-right">
              {percentage.toFixed(1)}%
            </div>
          </div>
        {/each}
      </div>

      <!-- Summary Stats -->
      <div class="grid grid-cols-2 gap-3 pt-3 border-t">
        <div class="bg-slate-50 rounded-lg p-3">
          <p class="text-xs text-muted-foreground mb-1">{$t('analytics.storage.autoCleanup')}</p>
          <p class="text-sm font-semibold">
            {$settings.autoCleanup ? $t('analytics.storage.enabled') : $t('analytics.storage.disabled')}
          </p>
        </div>
        <div class="bg-slate-50 rounded-lg p-3">
          <p class="text-xs text-muted-foreground mb-1">{$t('analytics.storage.threshold')}</p>
          <p class="text-sm font-semibold">{$settings.cleanupThreshold}%</p>
        </div>
      </div>
    {:else if storageUsage && breakdownData.length === 0}
      <p class="text-sm text-muted-foreground text-center py-8">
        {$t('analytics.storage.noFilesStored')}
      </p>
    {:else}
      <p class="text-sm text-muted-foreground text-center py-8">
        {$t('analytics.storage.noData')}
      </p>
    {/if}
  </Card>
</div>
