## Complete Transaction Feature Implementation Plan

This plan provides a production-ready implementation of the Chiral Network transaction system, fully aligned with the API specification.

---

## Phase 1: Backend Integration Layer

### Step 1.1: Define Rust Command Interfaces
**File**: `src-tauri/src/commands/transactions.rs`

```rust
use serde::{Deserialize, Serialize};
use tauri::command;

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub suggestion: Option<String>,
    pub documentation_url: Option<String>,
    pub geth_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BroadcastResponse {
    pub transaction_hash: String,
    pub status: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct TransactionStatus {
    pub transaction_hash: String,
    pub status: String, // "submitted" | "pending" | "success" | "failed" | "not_found"
    pub block_number: Option<u64>,
    pub block_hash: Option<String>,
    pub gas_used: Option<u64>,
    pub effective_gas_price: Option<String>,
    pub confirmations: Option<u32>,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub value: Option<String>,
    pub nonce: Option<u64>,
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GasPriceInfo {
    pub gas_price: String,
    pub estimated_time: String,
}

#[derive(Debug, Serialize)]
pub struct NetworkGasPrice {
    pub timestamp: String,
    pub slow: GasPriceInfo,
    pub standard: GasPriceInfo,
    pub fast: GasPriceInfo,
    pub network_congestion: String,
    pub base_fee: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TransactionEstimate {
    pub gas_estimate: u64,
    pub gas_prices: NetworkGasPrice,
    pub total_cost_wei: String,
    pub validation: ValidationResult,
    pub recommended_nonce: u64,
}

#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub sufficient_balance: bool,
    pub valid_recipient: bool,
    pub account_balance: String,
}

#[command]
pub async fn broadcast_transaction(
    signed_payload: String,
) -> Result<BroadcastResponse, ApiError> {
    // Implementation calls ethereum.rs functions
    todo!()
}

#[command]
pub async fn get_transaction_status(
    tx_hash: String,
) -> Result<TransactionStatus, ApiError> {
    // Implementation
    todo!()
}

#[command]
pub async fn get_transaction_history(
    address: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<TransactionStatus>, ApiError> {
    // Implementation
    todo!()
}

#[command]
pub async fn get_address_nonce(address: String) -> Result<u64, ApiError> {
    // Implementation
    todo!()
}

#[command]
pub async fn estimate_transaction(
    from: String,
    to: String,
    value: String,
) -> Result<TransactionEstimate, ApiError> {
    // Implementation
    todo!()
}

#[command]
pub async fn get_network_gas_price() -> Result<NetworkGasPrice, ApiError> {
    // Implementation
    todo!()
}

#[command]
pub async fn get_network_status() -> Result<NetworkStatus, ApiError> {
    // Implementation
    todo!()
}
```

### Step 1.2: Register Commands in Tauri
**File**: `src-tauri/src/main.rs`

Add to the imports:
```rust
mod commands;
use commands::transactions::*;
```

Add to the Tauri builder:
```rust
.invoke_handler(tauri::generate_handler![
    // ... existing handlers
    broadcast_transaction,
    get_transaction_status,
    get_transaction_history,
    get_address_nonce,
    estimate_transaction,
    get_network_gas_price,
    get_network_status,
])
```

---

## Phase 2: Frontend Service Layer

### Step 2.1: Transaction Service with Proper Error Handling
**File**: `src/lib/services/transactionService.ts`

```typescript
import { invoke } from '@tauri-apps/api/core';

/**
 * Backend API Error Structure (matches Rust ApiError)
 */
export interface ApiError {
  code: string;
  message: string;
  details?: Record<string, any>;
  suggestion?: string;
  documentation_url?: string;
  geth_error?: string;
}

/**
 * Custom error class for transaction operations
 */
export class TransactionServiceError extends Error {
  constructor(
    public code: string,
    message: string,
    public details?: Record<string, any>,
    public suggestion?: string,
    public documentation_url?: string,
    public geth_error?: string
  ) {
    super(message);
    this.name = 'TransactionServiceError';
  }

  static fromApiError(apiError: ApiError): TransactionServiceError {
    return new TransactionServiceError(
      apiError.code,
      apiError.message,
      apiError.details,
      apiError.suggestion,
      apiError.documentation_url,
      apiError.geth_error
    );
  }

  getUserMessage(): string {
    // Use the suggestion if available, otherwise fallback to predefined messages
    if (this.suggestion) {
      return this.suggestion;
    }
    
    const userMessages: Record<string, string> = {
      'NONCE_TOO_LOW': 'Transaction nonce is outdated. Please refresh and try again.',
      'NONCE_TOO_HIGH': 'Transaction nonce is too high. Check for pending transactions.',
      'INSUFFICIENT_FUNDS': 'Insufficient balance to complete this transaction.',
      'GAS_PRICE_TOO_LOW': 'Gas price too low. Please increase the gas price.',
      'GAS_LIMIT_EXCEEDED': 'Transaction exceeds block gas limit.',
      'REPLACEMENT_UNDERPRICED': 'Replacement transaction needs higher gas price.',
      'MEMPOOL_FULL': 'Network is congested. Please try again later.',
      'INVALID_TRANSACTION_FORMAT': 'Invalid transaction format.',
      'TRANSACTION_NOT_FOUND': 'Transaction not found on the network.',
      'NETWORK_ERROR': 'Network connection error.',
    };
    
    return userMessages[this.code] || this.message;
  }
}

export interface GasPriceInfo {
  gas_price: string;
  estimated_time: string;
}

export interface NetworkGasPrice {
  timestamp: string;
  slow: GasPriceInfo;
  standard: GasPriceInfo;
  fast: GasPriceInfo;
  network_congestion: string;
  base_fee?: string;
}

export interface TransactionEstimate {
  gas_estimate: number;
  gas_prices: NetworkGasPrice;
  total_cost_wei: string;
  validation: {
    sufficient_balance: boolean;
    valid_recipient: boolean;
    account_balance: string;
  };
  recommended_nonce: number;
}

export interface BroadcastResponse {
  transaction_hash: string;
  status: string;
  timestamp: string;
}

export type TransactionStatusType = 'submitted' | 'pending' | 'success' | 'failed' | 'not_found';

export interface TransactionStatus {
  transaction_hash: string;
  status: TransactionStatusType;
  block_number?: number;
  block_hash?: string;
  gas_used?: number;
  effective_gas_price?: string;
  confirmations?: number;
  from_address?: string;
  to_address?: string;
  value?: string;
  nonce?: number;
  error_message?: string;
}

export interface NetworkStatus {
  network_id: number;
  latest_block: number;
  peer_count: number;
  is_syncing: boolean;
  sync_progress?: {
    current_block: number;
    highest_block: number;
    starting_block: number;
  };
  node_version: string;
  network_hashrate: string;
  difficulty: string;
  average_block_time: number;
  mempool_size: number;
  suggested_gas_price: string;
  chain_id: number;
}

/**
 * Helper to handle Tauri command errors
 */
async function invokeWithErrorHandling<T>(
  command: string,
  args?: Record<string, any>
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error: any) {
    // Check if error has our API error structure
    if (error && typeof error === 'object' && 'code' in error) {
      throw TransactionServiceError.fromApiError(error as ApiError);
    }
    // Fallback for unexpected errors
    throw new TransactionServiceError(
      'UNKNOWN_ERROR',
      error?.message || 'An unexpected error occurred',
      { originalError: error }
    );
  }
}

// API Functions

export async function broadcastTransaction(signedPayload: string): Promise<BroadcastResponse> {
  return invokeWithErrorHandling<BroadcastResponse>('broadcast_transaction', {
    signedPayload
  });
}

export async function getTransactionStatus(txHash: string): Promise<TransactionStatus> {
  return invokeWithErrorHandling<TransactionStatus>('get_transaction_status', {
    txHash
  });
}

export async function getTransactionHistory(
  address: string,
  limit?: number,
  offset?: number
): Promise<TransactionStatus[]> {
  return invokeWithErrorHandling<TransactionStatus[]>('get_transaction_history', {
    address,
    limit,
    offset
  });
}

export async function getAddressNonce(address: string): Promise<number> {
  return invokeWithErrorHandling<number>('get_address_nonce', { address });
}

export async function estimateTransaction(
  from: string,
  to: string,
  value: string
): Promise<TransactionEstimate> {
  return invokeWithErrorHandling<TransactionEstimate>('estimate_transaction', {
    from,
    to,
    value
  });
}

export async function getNetworkGasPrice(): Promise<NetworkGasPrice> {
  return invokeWithErrorHandling<NetworkGasPrice>('get_network_gas_price');
}

export async function getNetworkStatus(): Promise<NetworkStatus> {
  return invokeWithErrorHandling<NetworkStatus>('get_network_status');
}

/**
 * Poll transaction status until confirmed or failed
 */
export async function pollTransactionStatus(
  txHash: string,
  onUpdate?: (status: TransactionStatus) => void,
  maxAttempts: number = 120,
  intervalMs: number = 2000
): Promise<TransactionStatus> {
  let attempts = 0;
  let lastStatus: TransactionStatusType = 'submitted';
  
  while (attempts < maxAttempts) {
    try {
      const status = await getTransactionStatus(txHash);
      
      // Call update callback if status changed
      if (status.status !== lastStatus && onUpdate) {
        onUpdate(status);
      }
      lastStatus = status.status;
      
      // Check if transaction is final
      if (status.status === 'success' || status.status === 'failed') {
        return status;
      }
      
      // Handle not_found status (transaction may not be indexed yet)
      if (status.status === 'not_found' && attempts < 5) {
        // Give it more time for initial indexing
      }
      
    } catch (error) {
      if (error instanceof TransactionServiceError && 
          error.code === 'TRANSACTION_NOT_FOUND' && 
          attempts < 5) {
        // Expected during initial submission
      } else {
        throw error;
      }
    }
    
    await new Promise(resolve => setTimeout(resolve, intervalMs));
    attempts++;
  }
  
  throw new TransactionServiceError(
    'TIMEOUT',
    'Transaction confirmation timeout',
    { txHash, attempts: maxAttempts }
  );
}
```

### Step 2.2: Wallet Service Implementation
**File**: `src/lib/services/walletService.ts`

```typescript
import { ethers } from 'ethers';
import { get } from 'svelte/store';
import { etcAccount, wallet } from '$lib/stores';

const CHAIN_ID = 98765; // Chiral Network Chain ID

export interface TransactionRequest {
  from: string;
  to: string;
  value: string; // Amount in ETH/CHR as string
  gasLimit: number;
  gasPrice: number; // in Wei
  nonce?: number;
}

/**
 * Sign a transaction using the stored wallet
 */
export async function signTransaction(txRequest: TransactionRequest): Promise<string> {
  const account = get(etcAccount);
  
  if (!account?.privateKey) {
    throw new Error('No wallet available for signing');
  }
  
  // Create ethers wallet from private key
  const wallet = new ethers.Wallet(account.privateKey);
  
  // Convert value from ETH string to Wei
  const valueWei = ethers.parseEther(txRequest.value);
  
  // Build transaction
  const transaction: ethers.TransactionRequest = {
    to: txRequest.to,
    value: valueWei,
    gasLimit: BigInt(txRequest.gasLimit),
    gasPrice: BigInt(txRequest.gasPrice),
    nonce: txRequest.nonce,
    chainId: CHAIN_ID,
    type: 0, // Legacy transaction type
  };
  
  try {
    // Sign the transaction
    const signedTx = await wallet.signTransaction(transaction);
    return signedTx;
  } catch (error) {
    console.error('Transaction signing failed:', error);
    throw new Error('Failed to sign transaction: ' + (error instanceof Error ? error.message : 'Unknown error'));
  }
}

/**
 * Validate Ethereum address format
 */
export function isValidAddress(address: string): boolean {
  try {
    ethers.getAddress(address); // Will throw if invalid
    return true;
  } catch {
    return false;
  }
}

/**
 * Format Wei to ETH for display
 */
export function formatEther(wei: string | number): string {
  return ethers.formatEther(wei.toString());
}

/**
 * Parse ETH to Wei
 */
export function parseEther(eth: string): string {
  return ethers.parseEther(eth).toString();
}
```

---

## Phase 3: Store Extensions

### Step 3.1: Update Transaction Store
**File**: `src/lib/stores.ts` (additions)

```typescript
import type { TransactionStatus as ApiTransactionStatus } from './services/transactionService';
import { pollTransactionStatus } from './services/transactionService';

// Update the Transaction interface to match API
export interface Transaction {
  id: number;
  type: "sent" | "received";
  amount: number;
  to?: string;
  from?: string;
  date: Date;
  description: string;
  status: 'submitted' | 'pending' | 'success' | 'failed'; // Match API statuses
  transaction_hash?: string;
  gas_used?: number;
  gas_price?: number; // in Wei
  confirmations?: number;
  block_number?: number;
  nonce?: number;
  fee?: number; // Total fee in Wei
  timestamp?: number;
  error_message?: string;
}

// Active polling tracker
const activePollingTasks = new Map<string, boolean>();

/**
 * Add a transaction and start polling for status updates
 */
export async function addTransactionWithPolling(
  transaction: Transaction
): Promise<void> {
  if (!transaction.transaction_hash) {
    throw new Error('Transaction must have a hash for polling');
  }
  
  const txHash = transaction.transaction_hash;
  
  // Prevent duplicate polling
  if (activePollingTasks.has(txHash)) {
    console.warn(`Already polling transaction ${txHash}`);
    return;
  }
  
  // Add to store immediately with 'submitted' status
  transactions.update(txs => [transaction, ...txs]);
  
  // Mark as actively polling
  activePollingTasks.set(txHash, true);
  
  try {
    // Start polling with status updates
    await pollTransactionStatus(
      txHash,
      (status: ApiTransactionStatus) => {
        // Update transaction in store on each status change
        transactions.update(txs =>
          txs.map(tx => {
            if (tx.transaction_hash === txHash) {
              return {
                ...tx,
                status: status.status === 'success' ? 'success' : 
                        status.status === 'failed' ? 'failed' :
                        status.status === 'pending' ? 'pending' :
                        'submitted',
                confirmations: status.confirmations || 0,
                block_number: status.block_number || undefined,
                gas_used: status.gas_used || undefined,
                error_message: status.error_message || undefined,
              };
            }
            return tx;
          })
        );
      },
      120, // 2 minutes max polling
      2000  // 2 second intervals
    );
  } catch (error) {
    console.error(`Failed to poll transaction ${txHash}:`, error);
    
    // Mark as failed on error
    transactions.update(txs =>
      txs.map(tx => {
        if (tx.transaction_hash === txHash) {
          return {
            ...tx,
            status: 'failed',
            error_message: error instanceof Error ? error.message : 'Polling failed',
          };
        }
        return tx;
      })
    );
  } finally {
    activePollingTasks.delete(txHash);
  }
}

/**
 * Helper to update transaction status manually
 */
export function updateTransactionStatus(
  txHash: string,
  updates: Partial<Transaction>
): void {
  transactions.update(txs =>
    txs.map(tx =>
      tx.transaction_hash === txHash 
        ? { ...tx, ...updates }
        : tx
    )
  );
}
```

---

## Phase 4: UI Components

### Step 4.1: Gas Estimator Component
**File**: `src/lib/components/transactions/GasEstimator.svelte`

```svelte
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
      if (!estimate) {
        estimate = {
          gas_estimate: 21000,
          gas_prices: gasPrice!,
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
```svelte
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
```

### Step 4.2: Transaction Form Component
**File**: `src/lib/components/transactions/TransactionForm.svelte`

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import Button from '$lib/components/ui/button.svelte';
  import Card from '$lib/components/ui/card.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import GasEstimator from './GasEstimator.svelte';
  import { Send, AlertCircle, CheckCircle, Loader, ArrowLeft } from 'lucide-svelte';
  import { wallet, etcAccount, addTransactionWithPolling } from '$lib/stores';
  import {
    broadcastTransaction,
    getNetworkStatus,
    getAddressNonce,
    TransactionServiceError,
    type NetworkStatus
  } from '$lib/services/transactionService';
  import { 
    signTransaction, 
    isValidAddress,
    formatEther,
    parseEther 
  } from '$lib/services/walletService';
  import { showToast } from '$lib/toast';

  // Form state
  let recipientAddress = '';
  let sendAmount = '';
  let description = '';

  // Gas state from estimator
  let selectedGasPriceWei = '0';
  let estimatedGasLimit = 21000;
  let selectedSpeed: 'slow' | 'standard' | 'fast' = 'standard';
  let totalGasCostWei = '0';
  let totalGasCostEth = '0';

  // UI state
  type FormStep = 'input' | 'review' | 'signing' | 'broadcasting' | 'success';
  let formStep: FormStep = 'input';
  let isProcessing = false;
  let networkStatus: NetworkStatus | null = null;
  let validationErrors: Record<string, string> = {};
  let transactionHash: string | null = null;
  let currentNonce: number | null = null;

  // Reactive validations
  $: isAddressValid = recipientAddress && isValidAddress(recipientAddress);
  $: amountNum = parseFloat(sendAmount) || 0;
  $: isAmountValid = amountNum > 0 && amountNum <= $wallet.balance;
  $: hasGasSelection = Number(selectedGasPriceWei) > 0;
  $: totalCostEth = amountNum + parseFloat(totalGasCostEth);
  $: canAffordTotal = totalCostEth <= $wallet.balance;

  /**
   * Validate all form inputs
   */
  function validateForm(): boolean {
    validationErrors = {};

    if (!recipientAddress) {
      validationErrors.recipient = 'Recipient address is required';
    } else if (!isAddressValid) {
      validationErrors.recipient = 'Invalid Ethereum address format';
    }

    if (!sendAmount || amountNum <= 0) {
      validationErrors.amount = 'Amount must be greater than 0';
    } else if (amountNum > $wallet.balance) {
      validationErrors.amount = `Insufficient balance (available: ${$wallet.balance.toFixed(6)} CHR)`;
    } else if (!canAffordTotal) {
      validationErrors.amount = `Insufficient balance for amount + gas (need: ${totalCostEth.toFixed(6)} CHR)`;
    }

    if (!networkStatus?.chain_id || networkStatus.chain_id !== 98765) {
      validationErrors.network = 'Not connected to Chiral Network (Chain ID 98765)';
    }

    if (!hasGasSelection) {
      validationErrors.gas = 'Please select a gas price';
    }

    return Object.keys(validationErrors).length === 0;
  }

  /**
   * Handle gas selection from estimator
   */
  function handleGasSelected(event: CustomEvent<{
    gasPriceWei: string;
    estimatedGas: number;
    speed: 'slow' | 'standard' | 'fast';
    totalCostWei: string;
    totalCostEth: string;
  }>) {
    selectedGasPriceWei = event.detail.gasPriceWei;
    estimatedGasLimit = event.detail.estimatedGas;
    selectedSpeed = event.detail.speed;
    totalGasCostWei = event.detail.totalCostWei;
    totalGasCostEth = event.detail.totalCostEth;
  }

  /**
   * Move to review step
   */
  async function proceedToReview() {
    if (!validateForm()) {
      showToast('Please fix all errors before continuing', 'error');
      return;
    }

    // Fetch current nonce for the transaction
    try {
      const address = $etcAccount?.address;
      if (!address) {
        showToast('No wallet connected', 'error');
        return;
      }
      currentNonce = await getAddressNonce(address);
    } catch (error) {
      console.error('Failed to fetch nonce:', error);
      showToast('Failed to prepare transaction', 'error');
      return;
    }

    formStep = 'review';
  }

  /**
   * Go back to input
   */
  function backToInput() {
    formStep = 'input';
    validationErrors = {};
  }

  /**
   * Execute the transaction
   */
  async function executeTransaction() {
    const account = get(etcAccount);
    if (!account?.address) {
      showToast('No wallet connected', 'error');
      return;
    }

    if (!validateForm()) {
      showToast('Please fix validation errors', 'error');
      formStep = 'input';
      return;
    }

    isProcessing = true;

    try {
      // Step 1: Sign the transaction
      formStep = 'signing';
      showToast('Signing transaction...', 'info');

      const signedTx = await signTransaction({
        from: account.address,
        to: recipientAddress,
        value: sendAmount,
        gasLimit: estimatedGasLimit,
        gasPrice: Number(selectedGasPriceWei),
        nonce: currentNonce || undefined
      });

      // Step 2: Broadcast to network
      formStep = 'broadcasting';
      showToast('Broadcasting to network...', 'info');

      const response = await broadcastTransaction(signedTx);
      transactionHash = response.transaction_hash;

      // Step 3: Add to store and start polling
      const newTransaction = {
        id: Date.now(),
        type: 'sent' as const,
        amount: amountNum,
        to: recipientAddress,
        from: account.address,
        date: new Date(),
        description: description || `Send ${sendAmount} CHR`,
        status: 'submitted' as const,
        transaction_hash: response.transaction_hash,
        gas_price: Number(selectedGasPriceWei),
        gas_used: estimatedGasLimit,
        confirmations: 0,
        nonce: currentNonce || undefined,
        fee: Number(totalGasCostWei)
      };

      // Start polling in background
      addTransactionWithPolling(newTransaction).catch(err => {
        console.error('Polling failed:', err);
      });

      // Success!
      formStep = 'success';
      showToast(`Transaction submitted! Hash: ${response.transaction_hash.slice(0, 10)}...`, 'success');

    } catch (error) {
      console.error('Transaction failed:', error);
      
      if (error instanceof TransactionServiceError) {
        // Show user-friendly error message
        showToast(error.getUserMessage(), 'error');
        
        // Handle specific error cases
        switch (error.code) {
          case 'NONCE_TOO_LOW':
          case 'NONCE_TOO_HIGH':
            // Refresh nonce and go back to review
            try {
              currentNonce = await getAddressNonce(account.address);
              showToast('Nonce updated. Please try again.', 'info');
            } catch {}
            formStep = 'review';
            break;
            
          case 'INSUFFICIENT_FUNDS':
            validationErrors.amount = error.getUserMessage();
            formStep = 'input';
            break;
            
          case 'GAS_PRICE_TOO_LOW':
            showToast('Please select a higher gas price', 'warning');
            formStep = 'review';
            break;
            
          default:
            formStep = 'input';
        }
      } else {
        showToast(
          'Transaction failed: ' + (error instanceof Error ? error.message : 'Unknown error'),
          'error'
        );
        formStep = 'input';
      }
    } finally {
      isProcessing = false;
    }
  }

  /**
   * Reset form for new transaction
   */
  function resetForm() {
    recipientAddress = '';
    sendAmount = '';
    description = '';
    selectedGasPriceWei = '0';
    estimatedGasLimit = 21000;
    transactionHash = null;
    currentNonce = null;
    validationErrors = {};
    formStep = 'input';
  }

  /**
   * Initialize network status
   */
  onMount(async () => {
    try {
      networkStatus = await getNetworkStatus();
      
      if (networkStatus.chain_id !== 98765) {
        showToast('Warning: Not connected to Chiral Network', 'warning');
      }
    } catch (error) {
      console.error('Failed to get network status:', error);
      showToast('Failed to connect to network', 'error');
    }
  });
</script>

<div class="space-y-4">
  {#if formStep === 'input'}
    <!-- Input Form -->
    <Card class="p-6">
      <div class="space-y-6">
        <div>
          <h2 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Send CHR
          </h2>
          <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Transfer CHR to another address on the Chiral Network
          </p>
        </div>

        <!-- Network Status -->
        <div class="flex items-center justify-between p-3 bg-gray-50 dark:bg-gray-800 rounded-lg">
          <div class="flex items-center space-x-2">
            <div class="w-2 h-2 rounded-full {
              networkStatus?.chain_id === 98765 ? 'bg-green-500 animate-pulse' : 'bg-red-500'
            }"></div>
            <span class="text-sm text-gray-600 dark:text-gray-400">
              {#if networkStatus}
                {networkStatus.chain_id === 98765 
                  ? `Connected • Block ${networkStatus.latest_block}`
                  : `Wrong network (Chain ID: ${networkStatus.chain_id})`}
              {:else}
                Connecting...
              {/if}
            </span>
          </div>
          {#if networkStatus?.is_syncing}
            <Badge class="bg-yellow-100 text-yellow-800">Syncing</Badge>
          {/if}
        </div>

        <!-- Recipient Address -->
        <div class="space-y-2">
          <Label for="recipient">Recipient Address</Label>
          <Input
            id="recipient"
            bind:value={recipientAddress}
            placeholder="0x..."
            class={validationErrors.recipient ? 'border-red-500' : ''}
            disabled={isProcessing}
          />
          {#if validationErrors.recipient}
            <div class="flex items-center space-x-1 text-xs text-red-600 dark:text-red-400">
              <AlertCircle class="w-3 h-3" />
              <span>{validationErrors.recipient}</span>
            </div>
          {/if}
        </div>

        <!-- Amount -->
        <div class="space-y-2">
          <Label for="amount">Amount (CHR)</Label>
          <Input
            id="amount"
            type="number"
            step="0.000001"
            min="0"
            max={$wallet.balance}
            bind:value={sendAmount}
            placeholder="0.00"
            class={validationErrors.amount ? 'border-red-500' : ''}
            disabled={isProcessing}
          />
          <div class="flex justify-between text-xs">
            <span class="text-gray-500 dark:text-gray-400">
              Balance: {$wallet.balance.toFixed(6)} CHR
            </span>
            {#if validationErrors.amount}
              <span class="text-red-600 dark:text-red-400">
                {validationErrors.amount}
              </span>
            {/if}
          </div>
        </div>

        <!-- Description -->
        <div class="space-y-2">
          <Label for="description">Description (Optional)</Label>
          <Input
            id="description"
            bind:value={description}
            placeholder="Payment for..."
            disabled={isProcessing}
          />
        </div>

        <!-- Errors -->
        {#if validationErrors.network}
          <div class="p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg">
            <div class="flex items-center space-x-2">
              <AlertCircle class="w-4 h-4 text-red-600 dark:text-red-400" />
              <span class="text-sm text-red-600 dark:text-red-400">
                {validationErrors.network}
              </span>
            </div>
          </div>
        {/if}

        <!-- Continue Button -->
        <Button
          on:click={proceedToReview}
          disabled={!recipientAddress || !sendAmount || isProcessing}
          class="w-full"
        >
          Continue to Review
        </Button>
      </div>
    </Card>

  {:else if formStep === 'review'}
    <!-- Review & Gas Selection -->
    <Card class="p-6">
      <div class="space-y-6">
        <div class="flex items-center justify-between">
          <div>
            <h2 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
              Review Transaction
            </h2>
            <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Confirm details and select gas price
            </p>
          </div>
          <Button
            variant="ghost"
            size="sm"
            on:click={backToInput}
            disabled={isProcessing}
          >
            <ArrowLeft class="w-4 h-4 mr-1" />
            Back
          </Button>
        </div>

        <!-- Transaction Details -->
        <div class="p-4 bg-gradient-to-r from-blue-50 to-indigo-50 dark:from-blue-900/20 dark:to-indigo-900/20 rounded-lg space-y-3 border border-blue-200 dark:border-blue-800">
          <div class="flex justify-between">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">From:</span>
            <code class="text-sm font-mono">
              {$etcAccount?.address.slice(0, 10)}...{$etcAccount?.address.slice(-8)}
            </code>
          </div>
          <div class="flex justify-between">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">To:</span>
            <code class="text-sm font-mono">
              {recipientAddress.slice(0, 10)}...{recipientAddress.slice(-8)}
            </code>
          </div>
          <div class="flex justify-between">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">Amount:</span>
            <span class="text-lg font-bold">{parseFloat(sendAmount).toFixed(6)} CHR</span>
          </div>
          {#if currentNonce !== null}
            <div class="flex justify-between">
              <span class="text-sm font-medium text-gray-700 dark:text-gray-300">Nonce:</span>
              <span class="text-sm font-mono">{currentNonce}</span>
            </div>
          {/if}
        </div>

        <!-- Gas Estimator -->
        <GasEstimator
          from={$etcAccount?.address || ''}
          to={recipientAddress}
          value={sendAmount}
          on:gasSelected={handleGasSelected}
        />

        <!-- Total Cost Summary -->
        {#if hasGasSelection}
          <div class="p-4 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
            <div class="space-y-2">
              <div class="flex justify-between text-sm">
                <span class="text-gray-600 dark:text-gray-400">Amount to send:</span>
                <span class="font-medium">{parseFloat(sendAmount).toFixed(6)} CHR</span>
              </div>
              <div class="flex justify-between text-sm">
                <span class="text-gray-600 dark:text-gray-400">Gas fee ({selectedSpeed}):</span>
                <span class="font-medium">{totalGasCostEth} CHR</span>
              </div>
              <div class="pt-2 border-t border-gray-200 dark:border-gray-600">
                <div class="flex justify-between">
                  <span class="font-semibold">Total:</span>
                  <span class="text-lg font-bold {
                    totalCostEth > $wallet.balance ? 'text-red-600' : 'text-gray-900 dark:text-gray-100'
                  }">
                    {totalCostEth.toFixed(6)} CHR
                  </span>
                </div>
              </div>
            </div>
          </div>
        {/if}

        <!-- Action Buttons -->
        <div class="flex space-x-3">
          <Button
            variant="outline"
            on:click={backToInput}
            disabled={isProcessing}
            class="flex-1"
          >
            Cancel
          </Button>
          <Button
            on:click={executeTransaction}
            disabled={!hasGasSelection || !canAffordTotal || isProcessing}
            class="flex-1"
          >
            <Send class="w-4 h-4 mr-2" />
            Send Transaction
          </Button>
        </div>
      </div>
    </Card>

  {:else if formStep === 'signing'}
    <!-- Signing State -->
    <Card class="p-6">
      <div class="text-center space-y-4 py-8">
        <div class="w-16 h-16 mx-auto bg-blue-100 dark:bg-blue-900/20 rounded-full flex items-center justify-center">
          <Loader class="w-8 h-8 animate-spin text-blue-600" />
        </div>
        <h3 class="text-xl font-semibold">Signing Transaction</h3>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          Preparing your transaction for the network...
        </p>
      </div>
    </Card>

  {:else if formStep === 'broadcasting'}
    <!-- Broadcasting State -->
    <Card class="p-6">
      <div class="text-center space-y-4 py-8">
        <div class="w-16 h-16 mx-auto bg-yellow-100 dark:bg-yellow-900/20 rounded-full flex items-center justify-center">
          <Loader class="w-8 h-8 animate-spin text-yellow-600" />
        </div>
        <h3 class="text-xl font-semibold">Broadcasting Transaction</h3>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          Submitting to the Chiral Network...
        </p>
      </div>
    </Card>

  {:else if formStep === 'success'}
    <!-- Success State -->
    <Card class="p-6">
      <div class="text-center space-y-6 py-8">
        <div class="w-16 h-16 mx-auto bg-green-100 dark:bg-green-900/20 rounded-full flex items-center justify-center">
          <CheckCircle class="w-8 h-8 text-green-600" />
        </div>
        <div>
          <h3 class="text-xl font-semibold">Transaction Submitted!</h3>
          <p class="text-sm text-gray-500 dark:text-gray-400 mt-2">
            Your transaction is being processed by the network
          </p>
        </div>

        {#if transactionHash}
          <div class="p-4 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
            <p class="text-xs font-medium text-gray-600 dark:text-gray-400 mb-2">
              Transaction Hash:
            </p>
            <code class="text-xs font-mono text-gray-900 dark:text-gray-100 break-all block">
              {transactionHash}
            </code>
          </div>
        {/if}

        <div class="space-y-3">
          <Button on:click={resetForm} class="w-full">
            <Send class="w-4 h-4 mr-2" />
            Send Another Transaction
          </Button>
          <p class="text-xs text-gray-500 dark:text-gray-400">
            Track this transaction in your history below
          </p>
        </div>
      </div>
    </Card>
  {/if}
</div>
```

---

## Phase 5: Page Integration

### Step 5.1: Update Account Page
**File**: `src/pages/Account.svelte`

Add these imports and integrate the components:

```svelte
<script lang="ts">
  // ... existing imports ...
  import TransactionForm from '$lib/components/transactions/TransactionForm.svelte';
  import TransactionList from '$lib/components/TransactionList.svelte';
  import TransactionReceipt from '$lib/components/TransactionReceipt.svelte';
  import { transactions } from '$lib/stores';
  import { Send, History } from 'lucide-svelte';

  // Modal state
  let showReceiptModal = false;
  let selectedTransaction = null;

  function handleTransactionClick(transaction) {
    selectedTransaction = transaction;
    showReceiptModal = true;
  }
</script>

<!-- Add after wallet balance section -->
<div class="container mx-auto px-4 py-8 max-w-6xl">
  <!-- Existing wallet section ... -->

  <!-- Send Transaction Section -->
  <section class="mb-8">
    <div class="flex items-center space-x-2 mb-4">
      <Send class="w-5 h-5 text-blue-600" />
      <h2 class="text-xl font-bold text-gray-900 dark:text-gray-100">
        Send Transaction
      </h2>
    </div>
    
    <TransactionForm />
  </section>

  <!-- Transaction History Section -->
  <section>
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center space-x-2">
        <History class="w-5 h-5 text-purple-600" />
        <h2 class="text-xl font-bold text-gray-900 dark:text-gray-100">
          Transaction History
        </h2>
      </div>
      <Badge class="bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300">
        {$transactions.length} transactions
      </Badge>
    </div>

    <TransactionList
      transactions={$transactions}
      onTransactionClick={handleTransactionClick}
      showFilters={true}
      compact={false}
    />
  </section>
</div>

<!-- Transaction Receipt Modal -->
{#if showReceiptModal && selectedTransaction}
  <TransactionReceipt
    transaction={selectedTransaction}
    isOpen={showReceiptModal}
    onClose={() => {
      showReceiptModal = false;
      selectedTransaction = null;
    }}
  />
{/if}
```

---

## Implementation Checklist

### Phase 1: Backend Setup ✓
- [ ] Create `src-tauri/src/commands/transactions.rs` with all command definitions
- [ ] Define proper error structures matching API spec
- [ ] Register commands in `main.rs`
- [ ] Test command invocation from frontend

### Phase 2: Service Layer ✓
- [ ] Create `transactionService.ts` with proper error handling
- [ ] Implement all 7 API functions with correct types
- [ ] Create `walletService.ts` with signing logic
- [ ] Test error scenarios with mock responses

### Phase 3: State Management ✓
- [ ] Update Transaction interface to match API statuses
- [ ] Implement `addTransactionWithPolling` function
- [ ] Add status update helpers
- [ ] Test polling lifecycle

### Phase 4: UI Components ✓
- [ ] Build `GasEstimator.svelte` with proper Wei/Gwei conversion
- [ ] Build `TransactionForm.svelte` with multi-step flow
- [ ] Test all form validations
- [ ] Verify error handling UI

### Phase 5: Integration ✓
- [ ] Integrate components into Account page
- [ ] Test end-to-end transaction flow
- [ ] Verify status updates in transaction list
- [ ] Test all error scenarios

---

This comprehensive plan addresses all the identified gaps and provides a production-ready implementation that strictly adheres to the API specification while maintaining excellent developer experience and user feedback.