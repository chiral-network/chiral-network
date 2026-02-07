<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from 'svelte-i18n';
  import PeerSelectionService, { type PeerMetrics } from '$lib/services/peerSelectionService';

  let peers: PeerMetrics[] = [];
  let loading = true;
  let error: string | null = null;
  let refreshInterval: number | null = null;
  let sortBy: keyof PeerMetrics = 'reliability_score';
  let sortOrder: 'asc' | 'desc' = 'desc';

  // Reactive sorted peers
  $: sortedPeers = [...peers].sort((a, b) => {
    const aVal = a[sortBy];
    const bVal = b[sortBy];
    
    let comparison = 0;
    if (typeof aVal === 'number' && typeof bVal === 'number') {
      comparison = aVal - bVal;
    } else {
      comparison = String(aVal).localeCompare(String(bVal));
    }
    
    return sortOrder === 'asc' ? comparison : -comparison;
  });

  async function loadPeerMetrics() {
    try {
      loading = true;
      error = null;
      peers = await PeerSelectionService.getPeerMetrics();
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to load peer metrics';
      console.error('Error loading peer metrics:', err);
    } finally {
      loading = false;
    }
  }

  function toggleSort(column: keyof PeerMetrics) {
    if (sortBy === column) {
      sortOrder = sortOrder === 'asc' ? 'desc' : 'asc';
    } else {
      sortBy = column;
      sortOrder = 'desc';
    }
  }

  function getSortIcon(column: keyof PeerMetrics): string {
    if (sortBy !== column) return '[SORT]';
    return sortOrder === 'asc' ? '^' : 'v';
  }

  async function cleanupInactivePeers() {
    try {
      await PeerSelectionService.cleanupInactivePeers(3600); // 1 hour
      await loadPeerMetrics(); // Refresh after cleanup
    } catch (err) {
      console.error('Error cleaning up peers:', err);
    }
  }

  function getHealthColor(score: number): string {
    if (score >= 80) return 'text-green-600';
    if (score >= 60) return 'text-yellow-600';
    if (score >= 40) return 'text-orange-600';
    return 'text-red-600';
  }

  onMount(() => {
    loadPeerMetrics();
    
    // Auto-refresh every 30 seconds
    refreshInterval = window.setInterval(loadPeerMetrics, 30000);
  });

  onDestroy(() => {
    if (refreshInterval) {
      clearInterval(refreshInterval);
    }
  });
</script>

<div class="peer-metrics-panel">
  <div class="header">
    <h2>{$t('network.smartPeerSelection')}</h2>
    <div class="actions">
      <button
        on:click={loadPeerMetrics}
        disabled={loading}
        class="btn btn-secondary"
      >
        {loading ? $t('network.peerMetrics.refreshing') : $t('network.peerMetrics.refresh')}
      </button>
      <button
        on:click={cleanupInactivePeers}
        class="btn btn-warning"
      >
        {$t('network.peerMetrics.cleanupInactive')}
      </button>
    </div>
  </div>

  {#if error}
    <div class="error">
      <p>[X] {$t('network.peerMetrics.error')}: {error}</p>
    </div>
  {/if}

  {#if loading && peers.length === 0}
    <div class="loading">
      <p>{$t('network.peerMetrics.loading')}</p>
    </div>
  {:else if peers.length === 0}
    <div class="empty-state">
      <p>{$t('network.peerMetrics.noPeers')}</p>
    </div>
  {:else}
    <div class="peer-stats">
      <div class="stat">
        <span class="label">{$t('network.peerMetrics.totalPeers')}:</span>
        <span class="value">{peers.length}</span>
      </div>
      <div class="stat">
        <span class="label">{$t('network.peerMetrics.encryptionCapable')}:</span>
        <span class="value">{peers.filter(p => p.encryption_support).length}</span>
      </div>
      <div class="stat">
        <span class="label">{$t('network.peerMetrics.avgReliability')}:</span>
        <span class="value">
          {Math.round(peers.reduce((sum, p) => sum + p.reliability_score, 0) / peers.length * 100)}%
        </span>
      </div>
    </div>

    <div class="table-container">
      <table class="peer-table">
        <thead>
          <tr>
            <th on:click={() => toggleSort('peer_id')}>
              {$t('network.peerMetrics.peerId')} {getSortIcon('peer_id')}
            </th>
            <th on:click={() => toggleSort('address')}>
              {$t('network.peerMetrics.address')} {getSortIcon('address')}
            </th>
            <th on:click={() => toggleSort('latency_ms')}>
              {$t('network.peerMetrics.latency')} {getSortIcon('latency_ms')}
            </th>
            <th on:click={() => toggleSort('bandwidth_kbps')}>
              {$t('network.peerMetrics.bandwidth')} {getSortIcon('bandwidth_kbps')}
            </th>
            <th on:click={() => toggleSort('reliability_score')}>
              {$t('network.peerMetrics.reliability')} {getSortIcon('reliability_score')}
            </th>
            <th on:click={() => toggleSort('success_rate')}>
              {$t('network.peerMetrics.successRate')} {getSortIcon('success_rate')}
            </th>
            <th on:click={() => toggleSort('transfer_count')}>
              {$t('network.peerMetrics.transfers')} {getSortIcon('transfer_count')}
            </th>
            <th on:click={() => toggleSort('total_bytes_transferred')}>
              {$t('network.peerMetrics.dataTransfer')} {getSortIcon('total_bytes_transferred')}
            </th>
            <th on:click={() => toggleSort('encryption_support')}>
              {$t('network.peerMetrics.encryption')} {getSortIcon('encryption_support')}
            </th>
            <th>{$t('network.peerMetrics.health')}</th>
          </tr>
        </thead>
        <tbody>
          {#each sortedPeers as peer (peer.peer_id)}
            {@const healthScore = PeerSelectionService.getPeerHealthScore(peer)}
            <tr class="peer-row">
              <td class="peer-id" title={peer.peer_id}>
                {peer.peer_id.slice(0, 12)}...
              </td>
              <td class="address" title={peer.address}>
                {peer.address}
              </td>
              <td class="latency">
                {peer.latency_ms ? `${peer.latency_ms}ms` : 'N/A'}
              </td>
              <td class="bandwidth">
                {peer.bandwidth_kbps 
                  ? `${Math.round(peer.bandwidth_kbps / 1024 * 100) / 100} MB/s`
                  : 'N/A'}
              </td>
              <td class="reliability">
                <span class={getHealthColor(peer.reliability_score * 100)}>
                  {Math.round(peer.reliability_score * 100)}%
                </span>
              </td>
              <td class="success-rate">
                <span class={getHealthColor(peer.success_rate * 100)}>
                  {Math.round(peer.success_rate * 100)}%
                </span>
              </td>
              <td class="transfers">
                {peer.successful_transfers}/{peer.transfer_count}
              </td>
              <td class="data-transferred">
                {PeerSelectionService.formatBytes(peer.total_bytes_transferred)}
              </td>
              <td class="encryption">
                <span class={peer.encryption_support ? 'text-green-600' : 'text-gray-400'}>
                  {peer.encryption_support ? '[OK]' : '[X]'}
                </span>
              </td>
              <td class="health">
                <div class="health-indicator">
                  <span class={getHealthColor(healthScore)}>
                    {healthScore}%
                  </span>
                  <div class="health-bar">
                    <div 
                      class="health-fill" 
                      style="width: {healthScore}%; background-color: {
                        healthScore >= 80 ? '#10b981' :
                        healthScore >= 60 ? '#f59e0b' :
                        healthScore >= 40 ? '#f97316' : '#ef4444'
                      }"
                    ></div>
                  </div>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .peer-metrics-panel {
    padding: 1rem;
    background: white;
    border-radius: 8px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid #e5e7eb;
  }

  .header h2 {
    margin: 0;
    color: #1f2937;
    font-size: 1.5rem;
    font-weight: 600;
  }

  .actions {
    display: flex;
    gap: 0.5rem;
  }

  .btn {
    padding: 0.5rem 1rem;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
    font-weight: 500;
    transition: all 0.2s;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-secondary {
    background-color: #6b7280;
    color: white;
  }

  .btn-secondary:hover:not(:disabled) {
    background-color: #4b5563;
  }

  .btn-warning {
    background-color: #f59e0b;
    color: white;
  }

  .btn-warning:hover {
    background-color: #d97706;
  }

  .error {
    padding: 1rem;
    background-color: #fee2e2;
    color: #dc2626;
    border-radius: 6px;
    margin-bottom: 1rem;
  }

  .loading, .empty-state {
    text-align: center;
    padding: 2rem;
    color: #6b7280;
  }

  .peer-stats {
    display: flex;
    gap: 2rem;
    margin-bottom: 1rem;
    padding: 1rem;
    background-color: #f9fafb;
    border-radius: 6px;
  }

  .stat {
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .stat .label {
    font-size: 0.75rem;
    color: #6b7280;
    text-transform: uppercase;
    font-weight: 500;
  }

  .stat .value {
    font-size: 1.25rem;
    font-weight: 600;
    color: #1f2937;
  }

  .table-container {
    overflow-x: auto;
    border: 1px solid #e5e7eb;
    border-radius: 6px;
  }

  .peer-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.875rem;
  }

  .peer-table th {
    background-color: #f9fafb;
    padding: 0.75rem;
    text-align: left;
    font-weight: 600;
    color: #374151;
    cursor: pointer;
    user-select: none;
    transition: background-color 0.2s;
  }

  .peer-table th:hover {
    background-color: #f3f4f6;
  }

  .peer-table td {
    padding: 0.75rem;
    border-top: 1px solid #e5e7eb;
    vertical-align: middle;
  }

  .peer-row:hover {
    background-color: #f9fafb;
  }

  .peer-id, .address {
    font-family: monospace;
    font-size: 0.75rem;
  }

  .health-indicator {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .health-bar {
    width: 40px;
    height: 8px;
    background-color: #e5e7eb;
    border-radius: 4px;
    overflow: hidden;
  }

  .health-fill {
    height: 100%;
    transition: width 0.3s ease;
  }

  /* Responsive design */
  @media (max-width: 1024px) {
    .peer-stats {
      flex-wrap: wrap;
      gap: 1rem;
    }

    .peer-table {
      font-size: 0.75rem;
    }

    .peer-table th,
    .peer-table td {
      padding: 0.5rem;
    }
  }

  @media (max-width: 768px) {
    .header {
      flex-direction: column;
      gap: 1rem;
      align-items: stretch;
    }

    .actions {
      justify-content: center;
    }

    .table-container {
      font-size: 0.75rem;
    }
  }

  /* Utility classes */
  .text-green-600 { color: #10b981; }
  .text-yellow-600 { color: #f59e0b; }
  .text-orange-600 { color: #f97316; }
  .text-red-600 { color: #ef4444; }
  .text-gray-400 { color: #9ca3af; }
</style>