
<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import { Search } from 'lucide-svelte';
  import { t } from 'svelte-i18n';

  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  let searchTerm = '';
  let blocks: any[] = [];
  let transactions: any[] = [];
  let topReceivers: any[] = [];
  let topMiners: any[] = [];
  let activeTab = 'explorer';
  let loading = true;

  onMount(async () => {
    await getLatestBlocks(12);
    loading = false;
  });

  async function getLatestBlocks(count: number) {
    try {
      blocks = await invoke('get_latest_blocks_command', { count });
    } catch (e) {
      console.error('Failed to fetch latest blocks:', e);
    }
  }

  async function handleSearch() {
    if (searchTerm.length === 0) {
        transactions = [];
        await getLatestBlocks(12);
        return;
    }
    if (searchTerm.startsWith('0x')) {
        if (searchTerm.length === 66) { // Transaction hash
            try {
                const tx = await invoke('get_transaction_by_hash_command', { txHash: searchTerm });
                if (tx) {
                    transactions = [tx];
                    blocks = []; // Clear blocks when searching for a transaction
                } else {
                    transactions = [];
                }
            } catch (e) {
                console.error('Failed to fetch transaction:', e);
                transactions = [];
            }
        } else if (searchTerm.length === 42) { // Address
            try {
                transactions = await invoke('get_transactions_by_address_command', { address: searchTerm });
                blocks = []; // Clear blocks when searching for transactions by address
            } catch (e) {
                console.error('Failed to fetch transactions for address:', e);
                transactions = [];
            }
        }
    } else { // Block number
        try {
            const block = await invoke('get_block_details_command', { blockNumber: parseInt(searchTerm) });
            if (block) {
                blocks = [block];
                transactions = block.transactions.map((tx: any) => ({ ...tx, blockNumber: parseInt(block.number, 16) })); // Add block number to transactions
            } else {
                blocks = [];
                transactions = [];
            }
        } catch (e) {
            console.error('Failed to fetch block:', e);
            blocks = [];
            transactions = [];
        }
    }
  }
  
  $: if (activeTab !== 'explorer') {
    transactions = []; // Clear transactions when switching tabs
  }

  async function getTopReceivers() {
    try {
      topReceivers = await invoke('get_top_receivers_command', { blockCount: 1000 });
    } catch (e) {
      console.error('Failed to fetch top receivers:', e);
    }
  }

  async function getTopMiners() {
    try {
      topMiners = await invoke('get_top_miners_command', { blockCount: 1000 });
    } catch (e) {
      console.error('Failed to fetch top miners:', e);
    }
  }

  $: if (activeTab === 'topReceivers' && topReceivers.length === 0) {
    getTopReceivers();
  }

  $: if (activeTab === 'topMiners' && topMiners.length === 0) {
    getTopMiners();
  }

</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('nav.blockExplorer')}</h1>
    <p class="text-muted-foreground mt-2">{$t('blockExplorer.subtitle')}</p>
  </div>

  <Card class="p-6">
    <div class="flex justify-between items-center mb-4">
      <div class="flex space-x-2">
        <Button variant={activeTab === 'explorer' ? 'default' : 'outline'} on:click={() => activeTab = 'explorer'}>{$t('blockExplorer.tabs.explorer')}</Button>
        <Button variant={activeTab === 'topReceivers' ? 'default' : 'outline'} on:click={() => activeTab = 'topReceivers'}>{$t('blockExplorer.tabs.topReceivers')}</Button>
        <Button variant={activeTab === 'topMiners' ? 'default' : 'outline'} on:click={() => activeTab = 'topMiners'}>{$t('blockExplorer.tabs.topMiners')}</Button>
      </div>
      <div class="flex items-center space-x-2">
        <Input bind:value={searchTerm} placeholder={$t('blockExplorer.searchPlaceholder')} class="w-64" on:keydown={(e) => e.key === 'Enter' && handleSearch()} />
        <Button on:click={handleSearch}><Search class="h-4 w-4" /></Button>
      </div>
    </div>

    {#if loading}
      <div class="text-center p-8">
        <p>Loading...</p>
      </div>
    {:else if activeTab === 'explorer'}
      <div>
        <h2 class="text-xl font-semibold mb-4">{$t('blockExplorer.latestBlocks')}</h2>
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {#each blocks as block}
            <Card class="p-4">
              <div class="flex justify-between items-center">
                <h3 class="font-bold">{$t('blockExplorer.block')} #{parseInt(block.number, 16)}</h3>
                <span class="text-xs text-muted-foreground">{new Date(parseInt(block.timestamp, 16) * 1000).toLocaleString()}</span>
              </div>
              <p class="text-sm mt-2">{$t('blockExplorer.transactions')}: {block.transactions.length}</p>
              <p class="text-sm">{$t('blockExplorer.miner')}: <span class="font-mono">{block.miner}</span></p>
            </Card>
          {/each}
        </div>
      </div>
      {#if transactions.length > 0}
        <div class="mt-8">
            <h2 class="text-xl font-semibold mb-4">Search Results</h2>
            <ul class="space-y-2">
                {#each transactions as tx}
                <li class="flex justify-between items-center p-2 rounded-lg hover:bg-muted">
                    <span class="font-mono">{tx.hash}</span>
                    <span>{parseInt(tx.value, 16) / 1e18} Coins</span>
                </li>
                {/each}
            </ul>
        </div>
      {/if}
    {:else if activeTab === 'topReceivers'}
      <div>
        <h2 class="text-xl font-semibold mb-4">{$t('blockExplorer.topReceivers')}</h2>
        <ul class="space-y-2">
          {#each topReceivers as receiver}
            <li class="flex justify-between items-center p-2 rounded-lg hover:bg-muted">
              <span class="font-mono">{receiver.address}</span>
              <span>{receiver.amount.toFixed(6)} Coins</span>
            </li>
          {/each}
        </ul>
      </div>
    {:else if activeTab === 'topMiners'}
      <div>
        <h2 class="text-xl font-semibold mb-4">{$t('blockExplorer.topMiners')}</h2>
        <ul class="space-y-2">
          {#each topMiners as miner}
            <li class="flex justify-between items-center p-2 rounded-lg hover:bg-muted">
              <span class="font-mono">{miner.address}</span>
              <span>{miner.amount.toFixed(6)} Coins</span>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  </Card>
</div>
