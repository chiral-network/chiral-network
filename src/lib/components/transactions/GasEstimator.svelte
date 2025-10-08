<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from 'svelte';
  import { Zap, Clock, TrendingUp, RefreshCw, AlertCircle } from 'lucide-svelte';
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import {
    getNetworkGasPrice,
    estimateTransaction,
    type NetworkGasPrice,
    type TransactionEstimate,
    TransactionServiceError
  } from '$lib/services/transactionService';

  // Props
  export let from: string;
  export let to: string;
  export let value: string; // Amount in ETH/CHR

  // State
  let gasPrice: NetworkGasPrice | null = null;
  let estimate: TransactionEstimate | null = null;
  let selectedSpeed: 'slow' | 'standard' | 'fast' = 'standard';
  let isLoadingGas = false;
  let isEstimating = false;
  let error: string | null = null;
  let debounceTimer: number | null = null;
  let refreshInterval: number | null = null;

  const dispatch = createEventDispatcher<{
    gasSelected: {
      gasPriceWei: string;
      estimatedGas: number;
      speed: 'slow' | 'standard' | 'fast';
      totalCostWei: string;
      totalCostEth: string;
    };
  }>();

  /**
   * Fetch current gas prices
   */
  async function fetchGasPrices() {
    isLoadingGas = true;
    error = null;

    try {
      gasPrice = await getNetworkGasPrice();

      // Auto-select standard speed
      if (gasPrice && selectedSpeed) {
        emitGasSelection();
      }
    } catch (err) {
      console.error('Failed to fetch gas prices:', err);
      if (err instanceof TransactionServiceError) {
        error = err.getUserMessage();
      } else {
        error = 'Failed to fetch current gas prices';
      }
    } finally {
      isLoadingGas = false;
    }
  }

  /**
   * Estimate gas for the transaction
   */
  async function performEstimation() {
    // Validate inputs
    if (!from || !to || !value || parseFloat(value) <= 0) {
      return;
    }

    isEstimating = true;
    error = null;

    try {
      estimate = await estimateTransaction(from, to, value);
      gasPrice = estimate.gas_prices;

      // Emit the current selection with new estimate
      emitGasSelection();
    } catch (err) {
      console.error('Failed to estimate transaction:', err);
      if (err instanceof TransactionServiceError) {
        error = err.getUserMessage();

        // Provide specific guidance for common errors
        if (err.code === 'INSUFFICIENT_FUNDS' && err.details) {
          error = `Insufficient balance: You have ${err.details.account_balance}, but need ${err.details.total_required}`;
        }
      } else {
        error = 'Failed to estimate gas';
      }

      // Use default gas limit on error
      if (!estimate && gasPrice) {
        estimate = {
          gas_estimate: 21000,
          gas_prices: gasPrice,
          total_cost_wei: '0',
          validation: {
            sufficient_balance: false,
            valid_recipient: true,
            account_balance: '0'
          },
          recommended_nonce: 0
        };
      }
    } finally {
      isEstimating = false;
    }
  }

  /**
   * Debounced estimation trigger
   */
  function triggerEstimation() {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }
    debounceTimer = window.setTimeout(() => {
      performEstimation();
    }, 500);
  }

  /**
   * Select gas speed and emit event
   */
  function selectSpeed(speed: 'slow' | 'standard' | 'fast') {
    selectedSpeed = speed;
    emitGasSelection();
  }

  /**
   * Emit the current gas selection
   */
  function emitGasSelection() {
    if (!gasPrice || !estimate) return;

    const speedData = gasPrice[selectedSpeed];
    const gasPriceWei = speedData.gas_price;
    const estimatedGas = estimate.gas_estimate;
    const totalCostWei = (BigInt(gasPriceWei) * BigInt(estimatedGas)).toString();
    const totalCostEth = (Number(totalCostWei) / 1e18).toFixed(6);

    dispatch('gasSelected', {
      gasPriceWei,
      estimatedGas,
      speed: selectedSpeed,
      totalCostWei,
      totalCostEth
    });
  }

  /**
   * Format Wei to Gwei for display
   */
  function weiToGwei(wei: string): string {
    return (Number(wei) / 1e9).toFixed(2);
  }

  /**
   * Manual refresh
   */
  async function handleRefresh() {
    await fetchGasPrices();
    if (from && to && value) {
      await performEstimation();
    }
  }

  // React to prop changes
  $: if (from && to && value && parseFloat(value) > 0) {
    triggerEstimation();
  }

  onMount(async () => {
    await fetchGasPrices();

    // Auto-refresh every 15 seconds
    refreshInterval = window.setInterval(fetchGasPrices, 15000);
  });

  onDestroy(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
    if (refreshInterval) clearInterval(refreshInterval);
  });
</script>

<Card class="p-4">
  <div class="space-y-4">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <div class="flex items-center space-x-2">
        <Zap class="w-5 h-5 text-yellow-600" />
        <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
          Gas Settings
        </h3>
      </div>
      <div class="flex items-center space-x-2">
        {#if gasPrice && !error}
          <Badge class="bg-green-100 text-green-800 dark:bg-green-900/20 dark:text-green-400">
            Live
          </Badge>
        {/if}
        <Button
          variant="ghost"
          size="sm"
          on:click={handleRefresh}
          disabled={isLoadingGas || isEstimating}
          class="p-2"
        >
          <RefreshCw class="w-4 h-4 {isLoadingGas ? 'animate-spin' : ''}" />
        </Button>
      </div>
    </div>

    {#if error}
      <!-- Error Display -->
      <div class="p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg flex items-start space-x-2">
        <AlertCircle class="w-4 h-4 text-red-600 dark:text-red-400 mt-0.5" />
        <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
      </div>
    {/if}

    {#if gasPrice}
      <!-- Gas Speed Options -->
      <div class="grid grid-cols-3 gap-3">
        <!-- Slow Option -->
        <button
          on:click={() => selectSpeed('slow')}
          disabled={isEstimating}
          class="p-4 rounded-xl border-2 transition-all cursor-pointer disabled:opacity-50 {
            selectedSpeed === 'slow'
              ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
              : 'border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600'
          }"
        >
          <div class="text-center space-y-2">
            <Clock class="w-5 h-5 mx-auto text-gray-600 dark:text-gray-400" />
            <div class="text-xs font-medium text-gray-600 dark:text-gray-400">Slow</div>
            <div class="text-lg font-bold text-gray-900 dark:text-gray-100">
              {weiToGwei(gasPrice.slow.gas_price)} Gwei
            </div>
            <div class="text-xs text-gray-500 dark:text-gray-400">
              {gasPrice.slow.estimated_time}
            </div>
          </div>
        </button>

        <!-- Standard Option -->
        <button
          on:click={() => selectSpeed('standard')}
          disabled={isEstimating}
          class="p-4 rounded-xl border-2 transition-all cursor-pointer disabled:opacity-50 {
            selectedSpeed === 'standard'
              ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
              : 'border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600'
          }"
        >
          <div class="text-center space-y-2">
            <Zap class="w-5 h-5 mx-auto text-yellow-600" />
            <div class="text-xs font-medium text-gray-600 dark:text-gray-400">Standard</div>
            <div class="text-lg font-bold text-gray-900 dark:text-gray-100">
              {weiToGwei(gasPrice.standard.gas_price)} Gwei
            </div>
            <div class="text-xs text-gray-500 dark:text-gray-400">
              {gasPrice.standard.estimated_time}
            </div>
          </div>
        </button>

        <!-- Fast Option -->
        <button
          on:click={() => selectSpeed('fast')}
          disabled={isEstimating}
          class="p-4 rounded-xl border-2 transition-all cursor-pointer disabled:opacity-50 {
            selectedSpeed === 'fast'
              ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
              : 'border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600'
          }"
        >
          <div class="text-center space-y-2">
            <TrendingUp class="w-5 h-5 mx-auto text-green-600" />
            <div class="text-xs font-medium text-gray-600 dark:text-gray-400">Fast</div>
            <div class="text-lg font-bold text-gray-900 dark:text-gray-100">
              {weiToGwei(gasPrice.fast.gas_price)} Gwei
            </div>
            <div class="text-xs text-gray-500 dark:text-gray-400">
              {gasPrice.fast.estimated_time}
            </div>
          </div>
        </button>
      </div>

      <!-- Gas Estimate and Validation Info -->
      {#if estimate}
        <div class="space-y-2">
          <div class="p-3 bg-gray-50 dark:bg-gray-800 rounded-lg">
            <div class="flex justify-between items-center text-sm">
              <span class="text-gray-600 dark:text-gray-400">Estimated Gas Units:</span>
              <span class="font-medium text-gray-900 dark:text-gray-100">
                {estimate.gas_estimate.toLocaleString()}
              </span>
            </div>
          </div>

          {#if !estimate.validation.sufficient_balance}
            <div class="p-2 bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800 rounded text-sm text-yellow-700 dark:text-yellow-300">
              Warning: Insufficient balance for this transaction
            </div>
          {/if}
        </div>
      {/if}

      <!-- Network Congestion Indicator -->
      {#if gasPrice.network_congestion}
        <div class="text-xs text-center text-gray-500 dark:text-gray-400">
          Network: {gasPrice.network_congestion}
        </div>
      {/if}

    {:else if isLoadingGas}
      <!-- Loading State -->
      <div class="text-center py-8">
        <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600 mx-auto"></div>
        <p class="text-sm text-gray-500 mt-2">Fetching gas prices...</p>
      </div>
    {/if}
  </div>
</Card>
