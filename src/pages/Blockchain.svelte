<script lang="ts">
  import { onMount, getContext } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { t } from 'svelte-i18n';
  import { fade } from 'svelte/transition';
  import { goto } from '@mateothegreat/svelte5-router';
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Progress from '$lib/components/ui/progress.svelte';
  import {
    Search,
    Database,
    Receipt,
    Wallet,
    RefreshCw,
    Clock,
    Coins,
    Activity,
    ChevronRight,
    Copy,
    AlertCircle
  } from 'lucide-svelte';
  import { showToast } from '$lib/toast';
  import { gethStatus, gethSyncStatus } from '$lib/services/gethService';
  import { diagnosticLogger } from '$lib/diagnostics/logger';

  const tr = (k: string, params?: Record<string, any>): string => $t(k, params);
  const navigation = getContext('navigation') as { setCurrentPage: (page: string) => void };

  // Tab state
  let activeTab: 'blocks' | 'pending' | 'search' | 'stats' = 'blocks';

  // Block data
  interface BlockInfo {
    hash: string;
    number: number;
    timestamp: number;
    nonce?: string;
    difficulty?: string;
    reward?: number;
    miner?: string;
    transactionCount?: number;
  }

  let latestBlocks: BlockInfo[] = [];
  let currentBlockNumber = 0;
  let isLoadingBlocks = false;

  // Search state
  type SearchResult =
    | { type: 'address'; address: string; balance: string; transactionCount?: number }
    | { type: 'transaction'; status?: string; block_number?: number; from_address?: string; to_address?: string; value?: string; [key: string]: unknown }
    | { type: 'block'; number?: number; hash?: string; timestamp?: number; [key: string]: unknown }
    | { type: 'error'; error: string };

  let searchQuery = '';
  let searchType: 'address' | 'transaction' | 'block' = 'address';
  let searchResult: SearchResult | null = null;
  let isSearching = false;

  // Balance checker
  let balanceAddress = '';
  let balanceResult: string | null = null;
  let isCheckingBalance = false;

  // Stats
  let networkStats = {
    totalBlocks: 0,
    difficulty: '0',
    networkHashrate: '0',
    peerCount: 0
  };

  // Txpool (off-chain pending/queued transactions)
  type TxpoolState = 'pending' | 'queued';

  interface TxpoolItem {
    state: TxpoolState;
    from: string;
    nonce: number;
    hash: string;
    to?: string | null;
    valueWeiHex?: string;
    gasWeiHex?: string;
    gasPriceWeiHex?: string;
  }

  let txpoolPending: TxpoolItem[] = [];
  let txpoolQueued: TxpoolItem[] = [];
  let txpoolCounts = { pending: 0, queued: 0 };
  let isLoadingTxpool = false;
  let txpoolError: string | null = null;

  // Fetch latest blocks
  async function fetchLatestBlocks() {
    isLoadingBlocks = true;
    try {
      // Check if Geth is running before making blockchain calls
      const gethRunning = await invoke<boolean>('is_geth_running');
      if (!gethRunning) {
        console.log('Geth is not running, skipping blockchain queries');
        latestBlocks = [];
        return;
      }

      // Get current block number
      console.log('Fetching current block number...');
      currentBlockNumber = await invoke<number>('get_current_block');
      console.log('Current block number:', currentBlockNumber);
      networkStats.totalBlocks = currentBlockNumber;

      if (currentBlockNumber === 0) {
        console.log('No blocks mined yet. Is Geth running? Is mining active?');
        showToast(tr('toasts.blockchain.noBlocks'), 'info');
        latestBlocks = [];
        return;
      }

      // Fetch last 10 blocks
      const blocks: BlockInfo[] = [];
      const startBlock = Math.max(0, currentBlockNumber - 9);

      console.log(`Fetching blocks ${startBlock} to ${currentBlockNumber}...`);
      for (let i = currentBlockNumber; i >= startBlock && i >= 0; i--) {
        try {
          const blockDetails = await invoke<any>('get_block_details_by_number', {
            blockNumber: i
          });

          console.log(`Block ${i} details:`, blockDetails);
          if (blockDetails) {
            blocks.push({
              hash: blockDetails.hash || `0x${i.toString(16)}`,
              number: i,
              timestamp: blockDetails.timestamp || Date.now() / 1000,
              nonce: blockDetails.nonce,
              difficulty: blockDetails.difficulty,
              miner: blockDetails.miner,
              transactionCount: blockDetails.transactions?.length || 0
            });
          }
        } catch (err) {
          const errorMsg = err instanceof Error ? err.message : String(err);
          diagnosticLogger.error('BLOCKCHAIN', `Failed to fetch block ${i}`, { error: errorMsg });
        }
      }

      diagnosticLogger.debug('BLOCKCHAIN', 'Fetched blocks', { count: blocks.length });
      latestBlocks = blocks;
    } catch (error: unknown) {
      const errorMsg = error instanceof Error ? error.message : String(error);
      diagnosticLogger.error('BLOCKCHAIN', 'Failed to fetch blocks', { error: errorMsg });
      showToast(
        tr('toasts.blockchain.fetchError', { values: { error: errorMsg } }),
        'error'
      );
    } finally {
      isLoadingBlocks = false;
    }
  }

  // Fetch network stats
  async function fetchNetworkStats() {
    try {
      const [difficulty, hashrate] = await invoke<[string, string]>('get_network_stats');
      networkStats.difficulty = difficulty;
      networkStats.networkHashrate = hashrate;

      const peerCount = await invoke<number>('get_network_peer_count');
      networkStats.peerCount = peerCount;
    } catch (error) {
      console.error('Failed to fetch network stats:', error);
    }
  }

  // Search functionality
  async function performSearch() {
    if (!searchQuery.trim()) {
      showToast(tr('blockchain.search.emptyQuery'), 'warning');
      return;
    }

    isSearching = true;
    searchResult = null;

    try {
      if (searchType === 'address') {
        // Check balance for address
        const balance = await invoke<string>('get_account_balance', {
          address: searchQuery.trim()
        });
        searchResult = {
          type: 'address',
          address: searchQuery.trim(),
          balance: balance
        };
      } else if (searchType === 'transaction') {
        // Get transaction receipt
        const receipt = await invoke<any>('get_transaction_receipt', {
          txHash: searchQuery.trim()
        });
        searchResult = {
          type: 'transaction',
          ...receipt
        };
      } else if (searchType === 'block') {
        // Get block by number
        const blockNumber = parseInt(searchQuery.trim());
        if (isNaN(blockNumber)) {
          // throw new Error('Invalid block number');
          throw new Error(tr('blockchain.search.invalidBlock'));
        }
        const blockDetails = await invoke<any>('get_block_details_by_number', {
          blockNumber
        });
        searchResult = {
          type: 'block',
          ...blockDetails
        };
      }
    } catch (error: unknown) {
      const errorMessage =
        error instanceof Error && error.message
          ? error.message
          : tr('blockchain.search.unknownError');
      diagnosticLogger.error('BLOCKCHAIN', 'Search error', { error: errorMessage });
      const displayMessage = tr('blockchain.search.error', {
        values: { error: errorMessage }
      });
      showToast(displayMessage, 'error');
      searchResult = { type: 'error', error: displayMessage };
    } finally {
      isSearching = false;
    }
  }

  // Check balance
  async function checkBalance() {
    if (!balanceAddress.trim()) {
      showToast(tr('blockchain.balance.emptyAddress'), 'warning');
      return;
    }

    isCheckingBalance = true;
    balanceResult = null;

    try {
      const balance = await invoke<string>('get_account_balance', {
        address: balanceAddress.trim()
      });
      balanceResult = balance;
    } catch (error: unknown) {
      const errorMessage =
        error instanceof Error && error.message
          ? error.message
          : tr('blockchain.search.unknownError');
      diagnosticLogger.error('BLOCKCHAIN', 'Balance check error', { error: errorMessage });
      showToast(
        tr('blockchain.balance.error', { values: { error: errorMessage } }),
        'error'
      );
      balanceResult = tr('blockchain.balance.errorLabel');
    } finally {
      isCheckingBalance = false;
    }
  }

  // Format timestamp
  function formatTimestamp(timestamp: number): string {
    return new Date(timestamp * 1000).toLocaleString();
  }

  // Format hash (truncate)
  function formatHash(hash: string): string {
    if (!hash) return 'N/A';
    return `${hash.substring(0, 10)}...${hash.substring(hash.length - 8)}`;
  }

  function hexToNumber(hex: string | undefined | null): number {
    if (!hex) return 0;
    const clean = hex.startsWith('0x') || hex.startsWith('0X') ? hex.slice(2) : hex;
    if (!clean) return 0;
    return Number.parseInt(clean, 16);
  }

  function weiHexToCN(weiHex: string | undefined | null): string {
    if (!weiHex) return '0';
    try {
      const n = BigInt(weiHex);
      const whole = n / 1000000000000000000n;
      const frac = n % 1000000000000000000n;
      const fracStr = frac.toString().padStart(18, '0').replace(/0+$/, '');
      return fracStr ? `${whole.toString()}.${fracStr.slice(0, 6)}` : whole.toString();
    } catch {
      return '0';
    }
  }

  async function fetchTxpool() {
    isLoadingTxpool = true;
    txpoolError = null;
    try {
      const gethRunning = await invoke<boolean>('is_geth_running');
      if (!gethRunning) {
        txpoolPending = [];
        txpoolQueued = [];
        txpoolCounts = { pending: 0, queued: 0 };
        return;
      }

      const [status, content] = await Promise.all([
        invoke<any>('get_txpool_status'),
        invoke<any>('get_txpool_content')
      ]);

      txpoolCounts = {
        pending: hexToNumber(status?.pending),
        queued: hexToNumber(status?.queued)
      };

      const flatten = (state: TxpoolState, obj: any): TxpoolItem[] => {
        const out: TxpoolItem[] = [];
        if (!obj || typeof obj !== 'object') return out;
        for (const [from, byNonce] of Object.entries<any>(obj)) {
          if (!byNonce || typeof byNonce !== 'object') continue;
          for (const [nonceStr, tx] of Object.entries<any>(byNonce)) {
            const nonce = Number.parseInt(nonceStr, 10);
            if (!Number.isFinite(nonce)) continue;
            out.push({
              state,
              from,
              nonce,
              hash: tx?.hash || '',
              to: tx?.to ?? null,
              valueWeiHex: tx?.value,
              gasWeiHex: tx?.gas,
              gasPriceWeiHex: tx?.gasPrice
            });
          }
        }
        out.sort((a, b) => a.nonce - b.nonce);
        return out;
      };

      txpoolPending = flatten('pending', content?.pending);
      txpoolQueued = flatten('queued', content?.queued);
    } catch (error: unknown) {
      const errorMsg = error instanceof Error ? error.message : String(error);
      txpoolError = errorMsg;
      diagnosticLogger.error('BLOCKCHAIN', 'Failed to fetch txpool', { error: errorMsg });
    } finally {
      isLoadingTxpool = false;
    }
  }

  // Copy to clipboard
  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    showToast(tr('blockchain.copied'), 'success');
  }

  // Format time remaining
  function formatTimeRemaining(seconds: number | null): string {
    if (seconds === null || seconds === 0) return 'Complete';
    if (seconds < 60) return `${Math.round(seconds)}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`;
    return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  }

  // Refresh data
  async function refreshAll() {
    await Promise.all([
      fetchLatestBlocks(),
      fetchTxpool(),
      fetchNetworkStats()
    ]);
    showToast(tr('blockchain.refreshed'), 'success');
  }

  onMount(() => {
    fetchLatestBlocks();
    fetchTxpool();
    fetchNetworkStats();

    // Auto-refresh every 30 seconds
    const interval = setInterval(() => {
      if (activeTab === 'blocks') {
        fetchLatestBlocks();
      }
      if (activeTab === 'pending') {
        fetchTxpool();
      }
      fetchNetworkStats();
    }, 30000);

    return () => clearInterval(interval);
  });
</script>

<div class="space-y-6">
  <!-- Header -->
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">
        {tr('blockchain.title')}
      </h1>
      <p class="text-muted-foreground mt-2">
        {tr('blockchain.subtitle')}
      </p>
    </div>
    <Button on:click={refreshAll} class="gap-2">
      <RefreshCw class="w-4 h-4" />
      {tr('blockchain.refresh')}
    </Button>
  </div>

  <!-- Warning Banner: Geth Not Running -->
  {#if $gethStatus !== 'running'}
    <div class="bg-yellow-500/10 border border-yellow-500/20 rounded-lg p-4">
      <div class="flex items-center gap-3">
        <AlertCircle class="h-5 w-5 text-yellow-500 flex-shrink-0" />
        <p class="text-sm text-yellow-600">
          {$t('nav.blockchainUnavailable')} <button on:click={() => { navigation.setCurrentPage('network'); goto('/network'); }} class="underline font-medium">{$t('nav.networkPageLink')}</button>.
        </p>
      </div>
    </div>
  {/if}

  <!-- Blockchain Sync Status -->
  {#if $gethSyncStatus?.syncing}
    <div class="bg-blue-500/10 border border-blue-500/20 rounded-lg p-4">
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <RefreshCw class="h-4 w-4 text-blue-500 animate-spin" />
            <span class="text-sm font-medium text-blue-600">{tr('blockchain.sync.syncing')}</span>
          </div>
          <span class="text-xs text-blue-600">{$gethSyncStatus.progress_percent.toFixed(1)}%</span>
        </div>
        <Progress value={$gethSyncStatus.progress_percent} max={100} class="h-2 [&>div]:bg-blue-500" />
        <div class="grid grid-cols-2 gap-4 text-xs text-blue-600">
          <div>
            <span class="text-muted-foreground">{tr('blockchain.sync.current')}:</span> #{$gethSyncStatus.current_block.toLocaleString()}
          </div>
          <div>
            <span class="text-muted-foreground">{tr('blockchain.sync.highest')}:</span> #{$gethSyncStatus.highest_block.toLocaleString()}
          </div>
          <div>
            <span class="text-muted-foreground">{tr('blockchain.sync.remaining')}:</span> {$gethSyncStatus.blocks_remaining.toLocaleString()} blocks
          </div>
          <div>
            <span class="text-muted-foreground">{tr('blockchain.sync.eta')}:</span> {formatTimeRemaining($gethSyncStatus.estimated_seconds_remaining)}
          </div>
        </div>
        <p class="text-xs text-blue-600 mt-2">
          ⏳ {tr('blockchain.sync.complete')}
        </p>
      </div>
    </div>
  {/if}

  <!-- Network Stats Cards -->
  <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-3 bg-blue-100 rounded-lg flex-shrink-0">
          <Database class="w-6 h-6 text-blue-600" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="text-sm text-gray-700 truncate">
            {tr('blockchain.stats.totalBlocks')}
          </p>
          <p class="text-2xl font-bold text-black break-words">
            {networkStats.totalBlocks.toLocaleString()}
          </p>
        </div>
      </div>
    </Card>

    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-3 bg-green-100 rounded-lg flex-shrink-0">
          <Activity class="w-6 h-6 text-green-600" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="text-sm text-gray-700 truncate">
            {tr('blockchain.stats.hashrate')}
          </p>
          <p class="text-2xl font-bold text-black break-words">
            {networkStats.networkHashrate}
          </p>
        </div>
      </div>
    </Card>

    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-3 bg-purple-100 rounded-lg flex-shrink-0">
          <Coins class="w-6 h-6 text-purple-600" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="text-sm text-gray-700 truncate">
            {tr('blockchain.stats.difficulty')}
          </p>
          <p class="text-2xl font-bold text-black break-words">
            {networkStats.difficulty}
          </p>
        </div>
      </div>
    </Card>

    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-3 bg-orange-100 rounded-lg flex-shrink-0">
          <Activity class="w-6 h-6 text-orange-600" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="text-sm text-gray-700 truncate">
            {tr('blockchain.stats.peers')}
          </p>
          <p class="text-2xl font-bold text-black break-words">
            {networkStats.peerCount}
          </p>
        </div>
      </div>
    </Card>
  </div>

  <!-- Tabs -->
  <div class="flex gap-2 border-b border-gray-200">
    <button
      class="px-4 py-2 font-medium transition-colors {activeTab === 'blocks'
        ? 'text-blue-600 border-b-2 border-blue-600'
        : 'text-gray-700 hover:text-blue-500 hover:bg-gray-100'}"
      on:click={() => activeTab = 'blocks'}
    >
      <div class="flex items-center gap-2">
        <Database class="w-4 h-4" />
        {tr('blockchain.tabs.blocks')}
      </div>
    </button>
    <button
      class="px-4 py-2 font-medium transition-colors {activeTab === 'pending'
        ? 'text-blue-600 border-b-2 border-blue-600'
        : 'text-gray-700 hover:text-blue-500 hover:bg-gray-100'}"
      on:click={() => {
        activeTab = 'pending';
        fetchTxpool();
      }}
    >
      <div class="flex items-center gap-2">
        <Receipt class="w-4 h-4" />
        {tr('blockchain.tabs.pending')}
        <Badge class="ml-1">{txpoolCounts.pending}</Badge>
      </div>
    </button>
    <button
      class="px-4 py-2 font-medium transition-colors {activeTab === 'search'
        ? 'text-blue-600 border-b-2 border-blue-600'
        : 'text-gray-700 hover:text-blue-500 hover:bg-gray-100'}"
      on:click={() => activeTab = 'search'}
    >
      <div class="flex items-center gap-2">
        <Search class="w-4 h-4" />
        {tr('blockchain.tabs.search')}
      </div>
    </button>
    <button
      class="px-4 py-2 font-medium transition-colors {activeTab === 'stats'
        ? 'text-blue-600 border-b-2 border-blue-600'
        : 'text-gray-700 hover:text-blue-500 hover:bg-gray-100'}"
      on:click={() => activeTab = 'stats'}
    >
      <div class="flex items-center gap-2">
        <Wallet class="w-4 h-4" />
        {tr('blockchain.tabs.stats')}
      </div>
    </button>
  </div>

  <!-- Tab Content -->
  {#if activeTab === 'blocks'}
    <div transition:fade={{ duration: 200 }}>
      <Card class="p-6">
        <h2 class="text-xl font-bold mb-4 text-black">
          {tr('blockchain.blocks.latest')}
        </h2>

        {#if isLoadingBlocks}
          <div class="flex items-center justify-center py-8">
            <RefreshCw class="w-6 h-6 animate-spin text-blue-600" />
          </div>
        {:else if latestBlocks.length === 0}
          <div class="text-center py-8">
            <p class="text-gray-900 mb-4 font-medium">
              {tr('blockchain.blocks.noBlocks')}
            </p>
            <p class="text-gray-700 text-sm mb-4">
              No blocks have been mined yet. To create blocks:
            </p>
            <ol class="text-left text-gray-700 text-sm max-w-md mx-auto space-y-2 mb-4">
              <li>1. Start the Chiral node (Network page)</li>
              <li>2. Start mining (Mining page)</li>
              <li>3. Wait for blocks to be mined</li>
            </ol>
          </div>
        {:else}
          <div class="space-y-3">
            {#each latestBlocks as block}
              <div class="flex items-center justify-between p-4 bg-gray-50 rounded-lg hover:bg-gray-100 transition-colors">
                <div class="flex items-center gap-4 flex-1">
                  <div class="p-2 bg-blue-100 rounded">
                    <Database class="w-5 h-5 text-blue-600" />
                  </div>
                  <div class="flex-1">
                    <div class="flex items-center gap-2">
                      <span class="font-bold text-gray-900">
                        Block #{block.number}
                      </span>
                      <Badge>{block.transactionCount || 0} txs</Badge>
                    </div>
                    <div class="flex items-center gap-4 mt-1 text-sm text-gray-600">
                      <span class="flex items-center gap-1">
                        <Clock class="w-3 h-3" />
                        {formatTimestamp(block.timestamp)}
                      </span>
                      <span class="font-mono">{formatHash(block.hash)}</span>
                      <button
                        on:click={() => copyToClipboard(block.hash)}
                        class="hover:text-blue-600"
                      >
                        <Copy class="w-3 h-3" />
                      </button>
                    </div>
                  </div>
                </div>
                <ChevronRight class="w-5 h-5 text-gray-400" />
              </div>
            {/each}
          </div>
        {/if}
      </Card>
    </div>
  {/if}

  {#if activeTab === 'pending'}
    <div transition:fade={{ duration: 200 }} class="space-y-6">
      <Card class="p-6">
        <div class="flex items-start justify-between gap-4">
          <div>
            <h2 class="text-xl font-bold text-black">
              {tr('blockchain.pending.title')}
            </h2>
            <p class="text-sm text-muted-foreground mt-1">
              {tr('blockchain.pending.subtitle')}
            </p>
          </div>
          <Button on:click={fetchTxpool} disabled={isLoadingTxpool} class="gap-2">
            <RefreshCw class="w-4 h-4 {isLoadingTxpool ? 'animate-spin' : ''}" />
            {tr('blockchain.refresh')}
          </Button>
        </div>

        {#if txpoolError}
          <div class="mt-4 bg-red-500/10 border border-red-500/20 rounded-lg p-4 text-sm text-red-700">
            {txpoolError}
          </div>
        {/if}

        <div class="mt-6 grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div class="p-4 bg-gray-50 rounded-lg">
            <p class="text-sm text-gray-700">{tr('blockchain.pending.pending')}</p>
            <p class="text-2xl font-bold text-black">{txpoolCounts.pending.toLocaleString()}</p>
          </div>
          <div class="p-4 bg-gray-50 rounded-lg">
            <p class="text-sm text-gray-700">{tr('blockchain.pending.queued')}</p>
            <p class="text-2xl font-bold text-black">{txpoolCounts.queued.toLocaleString()}</p>
          </div>
        </div>

        <div class="mt-6 space-y-6">
          <div>
            <h3 class="font-bold text-gray-900 mb-3">{tr('blockchain.pending.pending')}</h3>
            {#if isLoadingTxpool}
              <div class="flex items-center justify-center py-8">
                <RefreshCw class="w-6 h-6 animate-spin text-blue-600" />
              </div>
            {:else if txpoolPending.length === 0}
              <p class="text-sm text-gray-700">{tr('blockchain.pending.empty')}</p>
            {:else}
              <div class="overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="text-left text-gray-600 border-b">
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.hash')}</th>
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.from')}</th>
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.to')}</th>
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.value')}</th>
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.nonce')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each txpoolPending as tx (tx.hash + ':' + tx.nonce)}
                      <tr class="border-b last:border-b-0">
                        <td class="py-2 pr-4">
                          <div class="flex items-center gap-2">
                            <span class="font-mono text-gray-900">{formatHash(tx.hash)}</span>
                            <button on:click={() => copyToClipboard(tx.hash)} class="hover:text-blue-600">
                              <Copy class="w-3 h-3" />
                            </button>
                          </div>
                        </td>
                        <td class="py-2 pr-4">
                          <div class="flex items-center gap-2">
                            <span class="font-mono text-xs text-gray-900 break-all">{formatHash(tx.from)}</span>
                            <button on:click={() => copyToClipboard(tx.from)} class="hover:text-blue-600">
                              <Copy class="w-3 h-3" />
                            </button>
                          </div>
                        </td>
                        <td class="py-2 pr-4">
                          {#if tx.to}
                            <div class="flex items-center gap-2">
                              <span class="font-mono text-xs text-gray-900 break-all">{formatHash(tx.to)}</span>
                              <button on:click={() => copyToClipboard(tx.to || '')} class="hover:text-blue-600">
                                <Copy class="w-3 h-3" />
                              </button>
                            </div>
                          {:else}
                            <span class="text-gray-600">Contract Creation</span>
                          {/if}
                        </td>
                        <td class="py-2 pr-4 text-gray-900">{weiHexToCN(tx.valueWeiHex)} CN</td>
                        <td class="py-2 pr-4 text-gray-900">{tx.nonce}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {/if}
          </div>

          <div>
            <h3 class="font-bold text-gray-900 mb-3">{tr('blockchain.pending.queued')}</h3>
            {#if isLoadingTxpool}
              <div class="flex items-center justify-center py-8">
                <RefreshCw class="w-6 h-6 animate-spin text-blue-600" />
              </div>
            {:else if txpoolQueued.length === 0}
              <p class="text-sm text-gray-700">{tr('blockchain.pending.emptyQueued')}</p>
            {:else}
              <div class="overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="text-left text-gray-600 border-b">
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.hash')}</th>
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.from')}</th>
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.to')}</th>
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.value')}</th>
                      <th class="py-2 pr-4">{tr('blockchain.pending.fields.nonce')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each txpoolQueued as tx (tx.hash + ':' + tx.nonce)}
                      <tr class="border-b last:border-b-0">
                        <td class="py-2 pr-4">
                          <div class="flex items-center gap-2">
                            <span class="font-mono text-gray-900">{formatHash(tx.hash)}</span>
                            <button on:click={() => copyToClipboard(tx.hash)} class="hover:text-blue-600">
                              <Copy class="w-3 h-3" />
                            </button>
                          </div>
                        </td>
                        <td class="py-2 pr-4">
                          <div class="flex items-center gap-2">
                            <span class="font-mono text-xs text-gray-900 break-all">{formatHash(tx.from)}</span>
                            <button on:click={() => copyToClipboard(tx.from)} class="hover:text-blue-600">
                              <Copy class="w-3 h-3" />
                            </button>
                          </div>
                        </td>
                        <td class="py-2 pr-4">
                          {#if tx.to}
                            <div class="flex items-center gap-2">
                              <span class="font-mono text-xs text-gray-900 break-all">{formatHash(tx.to)}</span>
                              <button on:click={() => copyToClipboard(tx.to || '')} class="hover:text-blue-600">
                                <Copy class="w-3 h-3" />
                              </button>
                            </div>
                          {:else}
                            <span class="text-gray-600">Contract Creation</span>
                          {/if}
                        </td>
                        <td class="py-2 pr-4 text-gray-900">{weiHexToCN(tx.valueWeiHex)} CN</td>
                        <td class="py-2 pr-4 text-gray-900">{tx.nonce}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {/if}
          </div>
        </div>
      </Card>
    </div>
  {/if}

  {#if activeTab === 'search'}
    <div transition:fade={{ duration: 200 }} class="space-y-6">
      <Card class="p-6">
        <h2 class="text-xl font-bold mb-4 text-black">
          {tr('blockchain.search.title')}
        </h2>

        <div class="space-y-4">
          <!-- Search Type Selection -->
          <div class="flex gap-2">
            <Button
              variant={searchType === 'address' ? 'default' : 'outline'}
              on:click={() => searchType = 'address'}
              class="flex-1"
            >
              <Wallet class="w-4 h-4 mr-2" />
              {tr('blockchain.search.address')}
            </Button>
            <Button
              variant={searchType === 'transaction' ? 'default' : 'outline'}
              on:click={() => searchType = 'transaction'}
              class="flex-1"
            >
              <Receipt class="w-4 h-4 mr-2" />
              {tr('blockchain.search.transaction')}
            </Button>
            <Button
              variant={searchType === 'block' ? 'default' : 'outline'}
              on:click={() => searchType = 'block'}
              class="flex-1"
            >
              <Database class="w-4 h-4 mr-2" />
              {tr('blockchain.search.block')}
            </Button>
          </div>

          <!-- Search Input -->
          <div class="flex gap-2">
            <Input
              bind:value={searchQuery}
              placeholder={searchType === 'address'
                ? '0x...'
                : searchType === 'transaction'
                  ? '0x...'
                  : 'Block number'}
              class="flex-1"
              on:keydown={(e) => { const ev = (e as unknown as KeyboardEvent); if (ev.key === 'Enter') performSearch(); }}
            />
            <Button on:click={performSearch} disabled={isSearching}>
              {#if isSearching}
                <RefreshCw class="w-4 h-4 animate-spin" />
              {:else}
                <Search class="w-4 h-4" />
              {/if}
            </Button>
          </div>

          <!-- Search Results -->
          {#if searchResult}
            <div class="mt-4 p-4 bg-gray-50 rounded-lg">
              {#if searchResult.type === 'error'}
                <p class="text-red-600">
                  {tr('blockchain.search.notFound')}: {searchResult.error}
                </p>
              {:else if searchResult.type === 'address'}
                <div class="space-y-2">
                  <h3 class="font-bold text-gray-900">
                    {tr('blockchain.search.addressDetails')}
                  </h3>
                  <div class="grid grid-cols-2 gap-2 text-sm">
                    <span class="text-gray-600">
                      {tr('blockchain.search.addressLabel')}:
                    </span>
                    <span class="font-mono text-gray-900 break-all">
                      {searchResult.address}
                    </span>
                    <span class="text-gray-600">
                      {tr('blockchain.search.balance')}:
                    </span>
                    <span class="font-bold text-green-600">
                      {searchResult.balance} CN
                    </span>
                  </div>
                </div>
              {:else if searchResult.type === 'transaction'}
                <div class="space-y-2">
                  <h3 class="font-bold text-gray-900">
                    {tr('blockchain.search.txDetails')}
                  </h3>
                  <div class="grid grid-cols-2 gap-2 text-sm">
                    <span class="text-gray-600">
                      {tr('blockchain.search.status')}:
                    </span>
                    <Badge class={searchResult.status === 'success' ? 'bg-green-100 text-green-800' : 'bg-yellow-100 text-yellow-800'}>
                      {searchResult.status}
                    </Badge>
                    <span class="text-gray-600">
                      {tr('blockchain.search.blockNumber')}:
                    </span>
                    <span class="text-gray-900">
                      {searchResult.block_number || 'Pending'}
                    </span>
                    <span class="text-gray-600">
                      {tr('blockchain.search.from')}:
                    </span>
                    <span class="font-mono text-gray-900 text-xs break-all">
                      {searchResult.from_address}
                    </span>
                    <span class="text-gray-600">
                      {tr('blockchain.search.to')}:
                    </span>
                    <span class="font-mono text-gray-900 text-xs break-all">
                      {searchResult.to_address || 'Contract Creation'}
                    </span>
                    <span class="text-gray-600">
                      {tr('blockchain.search.value')}:
                    </span>
                    <span class="text-gray-900">
                      {searchResult.value} Wei
                    </span>
                  </div>
                </div>
              {:else if searchResult.type === 'block'}
                <div class="space-y-2">
                  <h3 class="font-bold text-gray-900">
                    {tr('blockchain.search.blockDetails')}
                  </h3>
                  <div class="grid grid-cols-2 gap-2 text-sm">
                    <span class="text-gray-600">
                      {tr('blockchain.search.blockNumber')}:
                    </span>
                    <span class="text-gray-900">
                      {searchResult.number}
                    </span>
                    <span class="text-gray-600">
                      {tr('blockchain.search.hash')}:
                    </span>
                    <span class="font-mono text-gray-900 text-xs break-all">
                      {searchResult.hash}
                    </span>
                    <span class="text-gray-600">
                      {tr('blockchain.search.timestamp')}:
                    </span>
                    <span class="text-gray-900">
                      {searchResult.timestamp ? formatTimestamp(searchResult.timestamp) : 'N/A'}
                    </span>
                  </div>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </Card>
    </div>
  {/if}

  {#if activeTab === 'stats'}
    <div transition:fade={{ duration: 200 }}>
      <Card class="p-6">
        <h2 class="text-xl font-bold mb-4 text-black">
          {tr('blockchain.balance.checker')}
        </h2>

        <div class="space-y-4">
          <div>
            <Label for="balanceAddress">
              {tr('blockchain.balance.address')}
            </Label>
            <div class="flex gap-2 mt-2">
              <Input
                id="balanceAddress"
                bind:value={balanceAddress}
                placeholder="0x..."
                class="flex-1"
                on:keydown={(e) => { const ev = (e as unknown as KeyboardEvent); if (ev.key === 'Enter') checkBalance(); }}
              />
              <Button on:click={checkBalance} disabled={isCheckingBalance}>
                {#if isCheckingBalance}
                  <RefreshCw class="w-4 h-4 animate-spin" />
                {:else}
                  <Wallet class="w-4 h-4" />
                {/if}
                {tr('blockchain.balance.check')}
              </Button>
            </div>
          </div>

          {#if balanceResult !== null}
            <div class="p-4 bg-green-50 rounded-lg">
              <p class="text-sm text-gray-600 mb-1">
                {tr('blockchain.balance.result')}
              </p>
              <p class="text-2xl font-bold text-green-600">
                {balanceResult} CN
              </p>
            </div>
          {/if}
        </div>
      </Card>
    </div>
  {/if}
</div>

<style>
  /* Add any custom styles here */
</style>
