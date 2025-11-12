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
