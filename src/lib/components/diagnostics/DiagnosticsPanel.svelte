<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { diagnosticLogger, LogLevel } from '$lib/diagnostics/logger';
  import type { LogEntry } from '$lib/diagnostics/logger';

  let logs: LogEntry[] = [];
  let filterLevel: LogLevel | 'ALL' = 'ALL';
  let filterComponent: string = '';
  let autoRefresh = true;
  let refreshInterval: number;

  onMount(() => {
    refreshLogs();

    if (autoRefresh) {
      refreshInterval = setInterval(refreshLogs, 1000) as unknown as number;
    }
  });

  onDestroy(() => {
    if (refreshInterval) {
      clearInterval(refreshInterval);
    }
  });

  function refreshLogs() {
    const allLogs = diagnosticLogger.getLogs();
    
    logs = allLogs.filter(log => {
      if (filterLevel !== 'ALL' && log.level !== filterLevel) return false;
      if (filterComponent && !log.component.toLowerCase().includes(filterComponent.toLowerCase())) return false;
      return true;
    });
  }

  function clearLogs() {
    diagnosticLogger.clearLogs();
    refreshLogs();
  }

  function exportLogs() {
    const exported = diagnosticLogger.exportLogs();
    const blob = new Blob([exported], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `diagnostics-${new Date().toISOString()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function getLevelColor(level: LogLevel): string {
    switch (level) {
      case LogLevel.DEBUG:
        return 'text-gray-500';
      case LogLevel.INFO:
        return 'text-blue-500';
      case LogLevel.WARN:
        return 'text-yellow-500';
      case LogLevel.ERROR:
        return 'text-red-500';
    }
  }

  $: if (filterLevel || filterComponent) {
    refreshLogs();
  }
</script>

<div class="p-6 bg-gray-900 text-white rounded-lg">
  <div class="flex items-center justify-between mb-6">
    <h2 class="text-2xl font-bold">Diagnostic Logs</h2>
    <div class="flex gap-2">
      <button
        on:click={() => (autoRefresh = !autoRefresh)}
        class="px-4 py-2 bg-blue-600 hover:bg-blue-700 rounded"
      >
        {autoRefresh ? 'Pause' : 'Resume'} Auto-refresh
      </button>
      <button
        on:click={exportLogs}
        class="px-4 py-2 bg-green-600 hover:bg-green-700 rounded"
      >
        Export
      </button>
      <button
        on:click={clearLogs}
        class="px-4 py-2 bg-red-600 hover:bg-red-700 rounded"
      >
        Clear
      </button>
    </div>
  </div>

  <div class="mb-4 flex gap-4">
    <div class="flex-1">
      <label class="block text-sm font-medium mb-2" for="filter-level">Filter by Level</label>
      <select
        id="filter-level"
        bind:value={filterLevel}
        class="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded"
      >
        <option value="ALL">All Levels</option>
        <option value={LogLevel.DEBUG}>Debug</option>
        <option value={LogLevel.INFO}>Info</option>
        <option value={LogLevel.WARN}>Warning</option>
        <option value={LogLevel.ERROR}>Error</option>
      </select>
    </div>
    <div class="flex-1">
      <label class="block text-sm font-medium mb-2" for="filter-component">Filter by Component</label>
      <input
        id="filter-component"
        bind:value={filterComponent}
        type="text"
        placeholder="e.g., DHT, FILE_OPS, NETWORK"
        class="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded"
      />
    </div>
  </div>

  <div class="bg-gray-800 rounded border border-gray-700 overflow-hidden">
    <div class="max-h-96 overflow-y-auto">
      {#if logs.length === 0}
        <div class="p-4 text-gray-500 text-center">No logs to display</div>
      {/if}

      {#each logs as log (log.timestamp)}
        <div class="border-b border-gray-700 p-3 text-xs font-mono hover:bg-gray-700">
          <div class="flex justify-between items-start gap-2">
            <span class="text-gray-400">{log.timestamp}</span>
            <span class={`font-bold ${getLevelColor(log.level)}`}>{log.level}</span>
            <span class="text-purple-400">[{log.component}]</span>
            <span class="flex-1">{log.message}</span>
          </div>
          {#if log.data}
            <div class="mt-1 ml-4 text-gray-500">
              {JSON.stringify(log.data)}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  </div>

  <div class="mt-4 text-sm text-gray-400">
    Showing {logs.length} of {diagnosticLogger.getLogs().length} total logs
  </div>
</div>
