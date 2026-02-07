import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/**
 * Payment checkpoint states
 */
export type CheckpointState =
  | 'active'
  | 'waiting_for_payment'
  | 'payment_received'
  | 'payment_failed'
  | 'completed';

/**
 * Payment checkpoint event data
 */
export interface PaymentCheckpointEvent {
  sessionId: string;
  fileHash: string;
  fileName: string;
  checkpointMb: number;
  amountChiral: number;
  bytesTransferred: number;
  seederAddress: string;
  seederPeerId: string;
}

/**
 * Payment checkpoint paid event data
 */
export interface PaymentCheckpointPaidEvent {
  sessionId: string;
  transactionHash: string;
  amountPaid: number;
}

/**
 * Checkpoint record for tracking payment history
 */
export interface CheckpointRecord {
  checkpoint_mb: number;
  bytes_at_checkpoint: number;
  amount_paid: number;
  transaction_hash: string | null;
  timestamp: number;
}

/**
 * Full payment checkpoint information
 */
export interface PaymentCheckpointInfo {
  session_id: string;
  file_hash: string;
  file_size: number;
  bytes_transferred: number;
  next_checkpoint_bytes: number;
  state: {
    Active?: null;
    WaitingForPayment?: { checkpoint_mb: number; amount_chiral: number };
    PaymentReceived?: { transaction_hash: string };
    PaymentFailed?: { reason: string };
    Completed?: null;
  };
  last_checkpoint_mb: number;
  total_paid_chiral: number;
  seeder_address: string;
  seeder_peer_id: string;
  price_per_mb: number;
  payment_mode: string;
  checkpoint_history: CheckpointRecord[];
}

/**
 * Payment Checkpoint Service
 *
 * Manages incremental payment checkpoints during file downloads.
 *
 * Flow:
 * 1. Initialize checkpoint session when download starts
 * 2. Update progress as bytes are transferred
 * 3. When checkpoint is reached, pause and wait for payment
 * 4. Process payment and resume download
 * 5. Repeat until download completes
 */
class PaymentCheckpointService {
  private checkpointListeners: Map<string, UnlistenFn> = new Map();
  private paidListeners: Map<string, UnlistenFn> = new Map();

  /**
   * Initialize a payment checkpoint session
   *
   * @param sessionId Unique session identifier (use file hash or transfer ID)
   * @param fileHash File hash being downloaded
   * @param fileSize Total file size in bytes
   * @param seederAddress Seeder's wallet address
   * @param seederPeerId Seeder's peer ID
   * @param pricePerMb Price per megabyte in Chiral
   * @param paymentMode "exponential" for 10→20→40 MB checkpoints, "upfront" for full payment
   */
  async initCheckpointSession(
    sessionId: string,
    fileHash: string,
    fileSize: number,
    seederAddress: string,
    seederPeerId: string,
    pricePerMb: number,
    paymentMode: 'exponential' | 'upfront' = 'exponential'
  ): Promise<void> {
    try {
      await invoke('init_payment_checkpoint', {
        sessionId,
        fileHash,
        fileSize,
        seederAddress,
        seederPeerId,
        pricePerMb,
        paymentMode,
      });

      console.log(`✅ Payment checkpoint session initialized: ${sessionId} (mode: ${paymentMode})`);
    } catch (error) {
      console.error('Failed to initialize payment checkpoint:', error);
      throw error;
    }
  }

  /**
   * Update download progress and check for payment checkpoints
   *
   * Call this method periodically as bytes are transferred.
   * Returns the current checkpoint state.
   *
   * @param sessionId Session identifier
   * @param bytesTransferred Total bytes transferred so far
   * @returns Current checkpoint state
   */
  async updateProgress(
    sessionId: string,
    bytesTransferred: number
  ): Promise<CheckpointState> {
    try {
      const state = await invoke<CheckpointState>('update_payment_checkpoint_progress', {
        sessionId,
        bytesTransferred,
      });

      return state;
    } catch (error) {
      console.error('Failed to update payment checkpoint progress:', error);
      throw error;
    }
  }

  /**
   * Check if download should be paused (waiting for payment)
   *
   * @param sessionId Session identifier
   * @returns True if download should pause
   */
  async shouldPauseServing(sessionId: string): Promise<boolean> {
    try {
      return await invoke<boolean>('check_should_pause_serving', { sessionId });
    } catch (error) {
      console.error('Failed to check pause status:', error);
      return false;
    }
  }

  /**
   * Record a payment for the current checkpoint
   *
   * @param sessionId Session identifier
   * @param transactionHash Blockchain transaction hash
   * @param amountPaid Amount paid in Chiral
   */
  async recordPayment(
    sessionId: string,
    transactionHash: string,
    amountPaid: number
  ): Promise<void> {
    try {
      await invoke('record_checkpoint_payment', {
        sessionId,
        transactionHash,
        amountPaid,
      });

      console.log(`✅ Checkpoint payment recorded: ${sessionId} (${amountPaid} Chiral, tx: ${transactionHash})`);
    } catch (error) {
      console.error('Failed to record checkpoint payment:', error);
      throw error;
    }
  }

  /**
   * Get full checkpoint information for a session
   *
   * @param sessionId Session identifier
   * @returns Checkpoint information
   */
  async getCheckpointInfo(sessionId: string): Promise<PaymentCheckpointInfo> {
    try {
      return await invoke<PaymentCheckpointInfo>('get_payment_checkpoint_info', { sessionId });
    } catch (error) {
      console.error('Failed to get checkpoint info:', error);
      throw error;
    }
  }

  /**
   * Mark payment as failed
   *
   * @param sessionId Session identifier
   * @param reason Failure reason
   */
  async markPaymentFailed(sessionId: string, reason: string): Promise<void> {
    try {
      await invoke('mark_checkpoint_payment_failed', { sessionId, reason });
      console.warn(`⚠️ Payment marked as failed: ${sessionId} - ${reason}`);
    } catch (error) {
      console.error('Failed to mark payment failed:', error);
      throw error;
    }
  }

  /**
   * Mark download as completed
   *
   * @param sessionId Session identifier
   */
  async markCompleted(sessionId: string): Promise<void> {
    try {
      await invoke('mark_checkpoint_completed', { sessionId });
      console.log(`✅ Download completed: ${sessionId}`);
    } catch (error) {
      console.error('Failed to mark checkpoint completed:', error);
      throw error;
    }
  }

  /**
   * Remove checkpoint session (cleanup)
   *
   * @param sessionId Session identifier
   */
  async removeSession(sessionId: string): Promise<void> {
    try {
      // Clean up listeners
      await this.stopListeningToCheckpoints(sessionId);

      // Remove backend session
      await invoke('remove_payment_checkpoint_session', { sessionId });
      console.log(`🗑️ Checkpoint session removed: ${sessionId}`);
    } catch (error) {
      console.error('Failed to remove checkpoint session:', error);
      throw error;
    }
  }

  /**
   * Listen for payment checkpoint events
   *
   * @param sessionId Session identifier (optional, to filter events)
   * @param callback Callback function when checkpoint is reached
   * @returns Unlisten function
   */
  async listenToCheckpoints(
    callback: (event: PaymentCheckpointEvent) => void | Promise<void>,
    sessionId?: string
  ): Promise<UnlistenFn> {
    const unlisten = await listen<PaymentCheckpointEvent>('payment_checkpoint_reached', async (event) => {
      // Filter by session ID if provided
      if (sessionId && event.payload.sessionId !== sessionId) {
        return;
      }

      await callback(event.payload);
    });

    if (sessionId) {
      this.checkpointListeners.set(sessionId, unlisten);
    }

    return unlisten;
  }

  /**
   * Listen for payment checkpoint paid events
   *
   * @param sessionId Session identifier (optional, to filter events)
   * @param callback Callback function when payment is completed
   * @returns Unlisten function
   */
  async listenToPayments(
    callback: (event: PaymentCheckpointPaidEvent) => void | Promise<void>,
    sessionId?: string
  ): Promise<UnlistenFn> {
    const unlisten = await listen<PaymentCheckpointPaidEvent>('payment_checkpoint_paid', async (event) => {
      // Filter by session ID if provided
      if (sessionId && event.payload.sessionId !== sessionId) {
        return;
      }

      await callback(event.payload);
    });

    if (sessionId) {
      this.paidListeners.set(sessionId, unlisten);
    }

    return unlisten;
  }

  /**
   * Stop listening to checkpoint events for a specific session
   *
   * @param sessionId Session identifier
   */
  async stopListeningToCheckpoints(sessionId: string): Promise<void> {
    const checkpointUnlisten = this.checkpointListeners.get(sessionId);
    if (checkpointUnlisten) {
      checkpointUnlisten();
      this.checkpointListeners.delete(sessionId);
    }

    const paidUnlisten = this.paidListeners.get(sessionId);
    if (paidUnlisten) {
      paidUnlisten();
      this.paidListeners.delete(sessionId);
    }
  }

  /**
   * Calculate next checkpoint size using exponential scaling
   *
   * @param currentCheckpointMb Current checkpoint size in MB
   * @returns Next checkpoint size in MB
   */
  calculateNextCheckpoint(currentCheckpointMb: number): number {
    // Exponential scaling: 10 → 20 -> 40 -> 80 MB
    return currentCheckpointMb * 2;
  }

  /**
   * Calculate total amount paid so far
   *
   * @param checkpointInfo Checkpoint information
   * @returns Total paid in Chiral
   */
  getTotalPaid(checkpointInfo: PaymentCheckpointInfo): number {
    return checkpointInfo.total_paid_chiral;
  }

  /**
   * Calculate remaining payment required for full file
   *
   * @param checkpointInfo Checkpoint information
   * @returns Remaining cost in Chiral
   */
  calculateRemainingCost(checkpointInfo: PaymentCheckpointInfo): number {
    const remainingBytes = checkpointInfo.file_size - checkpointInfo.bytes_transferred;
    const remainingMb = remainingBytes / (1024 * 1024);
    return remainingMb * checkpointInfo.price_per_mb;
  }

  /**
   * Format checkpoint state for display
   *
   * @param state Checkpoint state
   * @returns Human-readable state string
   */
  formatState(state: CheckpointState): string {
    switch (state) {
      case 'active':
        return 'Downloading';
      case 'waiting_for_payment':
        return 'Payment Required';
      case 'payment_received':
        return 'Payment Confirmed';
      case 'payment_failed':
        return 'Payment Failed';
      case 'completed':
        return 'Completed';
      default:
        return 'Unknown';
    }
  }
}

// Export singleton instance
export const paymentCheckpointService = new PaymentCheckpointService();
