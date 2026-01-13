import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { paymentCheckpointService } from '../src/lib/services/paymentCheckpointService';
import type { PaymentCheckpointInfo } from '../src/lib/services/paymentCheckpointService';

// PaymentCheckpointService is a thin Tauri RPC wrapper; in unit tests we emulate the backend
// state machine in-memory so the suite can run in plain Node.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);

const MB = 1024 * 1024;
const initialCheckpointMb = 10;

type SessionState = 'active' | 'waiting_for_payment' | 'payment_received' | 'payment_failed' | 'completed';

type Session = {
  sessionId: string;
  fileHash: string;
  fileSize: number;
  bytesTransferred: number;
  nextCheckpointBytes: number;
  intervalMb: number;
  totalPaid: number;
  seederAddress: string;
  seederPeerId: string;
  pricePerMb: number;
  paymentMode: 'exponential' | 'upfront';
  state: SessionState;
  checkpointHistory: PaymentCheckpointInfo['checkpoint_history'];
};

function toInfo(s: Session): PaymentCheckpointInfo {
  const stateObj: PaymentCheckpointInfo['state'] =
    s.state === 'active'
      ? { Active: null }
      : s.state === 'waiting_for_payment'
        ? { WaitingForPayment: { checkpoint_mb: s.nextCheckpointBytes / MB, amount_chiral: 0 } }
        : s.state === 'payment_received'
          ? { PaymentReceived: { transaction_hash: '' } }
          : s.state === 'payment_failed'
            ? { PaymentFailed: { reason: 'failed' } }
            : { Completed: null };

  return {
    session_id: s.sessionId,
    file_hash: s.fileHash,
    file_size: s.fileSize,
    bytes_transferred: s.bytesTransferred,
    next_checkpoint_bytes: s.nextCheckpointBytes,
    state: stateObj,
    last_checkpoint_mb: s.intervalMb,
    total_paid_chiral: s.totalPaid,
    seeder_address: s.seederAddress,
    seeder_peer_id: s.seederPeerId,
    price_per_mb: s.pricePerMb,
    payment_mode: s.paymentMode,
    checkpoint_history: s.checkpointHistory,
  };
}

/**
 * Payment Checkpoint Service Tests
 *
 * Tests the complete payment checkpoint flow:
 * 1. Initialize checkpoint session
 * 2. Update progress (triggers checkpoints at 10 MB, 30 MB, etc.)
 * 3. Record payments
 * 4. Verify exponential scaling
 */
describe('Payment Checkpoint Service', () => {
  const testSessionId = 'test-session-123';
  const testFileHash = 'test-file-hash-abc';
  const testFileSize = 100 * 1024 * 1024; // 100 MB
  const testSeederAddress = '0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb';
  const testSeederPeerId = 'QmTestPeer123';
  const pricePerMb = 0.001;

  let sessions: Map<string, Session>;

  beforeEach(() => {
    sessions = new Map();

    mockInvoke.mockImplementation(async (cmd: string, args?: any) => {
      switch (cmd) {
        case 'init_payment_checkpoint': {
          const {
            sessionId,
            fileHash,
            fileSize,
            seederAddress,
            seederPeerId,
            pricePerMb,
            paymentMode,
          } = args as any;

          const mode = (paymentMode ?? 'exponential') as 'exponential' | 'upfront';
          const nextBytes = mode === 'upfront' ? fileSize : initialCheckpointMb * MB;

          sessions.set(sessionId, {
            sessionId,
            fileHash,
            fileSize,
            bytesTransferred: 0,
            nextCheckpointBytes: nextBytes,
            intervalMb: initialCheckpointMb,
            totalPaid: 0,
            seederAddress,
            seederPeerId,
            pricePerMb,
            paymentMode: mode,
            state: 'active',
            checkpointHistory: [],
          });
          return undefined;
        }

        case 'update_payment_checkpoint_progress': {
          const { sessionId, bytesTransferred } = args as any;
          const s = sessions.get(sessionId);
          if (!s) throw new Error('Session not found');

          s.bytesTransferred = bytesTransferred;

          if (s.paymentMode === 'upfront') {
            // Upfront: pause once we reach the full file threshold (tests only inspect next_checkpoint_bytes).
            if (bytesTransferred >= s.nextCheckpointBytes && s.totalPaid === 0) {
              s.state = 'waiting_for_payment';
              return 'waiting_for_payment';
            }
            return 'active';
          }

          if (bytesTransferred >= s.nextCheckpointBytes && s.state === 'active') {
            s.state = 'waiting_for_payment';
            return 'waiting_for_payment';
          }
          return 'active';
        }

        case 'check_should_pause_serving': {
          const { sessionId } = args as any;
          const s = sessions.get(sessionId);
          if (!s) throw new Error('Session not found');
          return s.state === 'waiting_for_payment';
        }

        case 'record_checkpoint_payment': {
          const { sessionId, transactionHash, amountPaid } = args as any;
          const s = sessions.get(sessionId);
          if (!s) throw new Error('Session not found');

          s.totalPaid += amountPaid;
          s.checkpointHistory.push({
            checkpoint_mb: Math.round(s.bytesTransferred / MB),
            bytes_at_checkpoint: s.bytesTransferred,
            amount_paid: amountPaid,
            transaction_hash: transactionHash,
            timestamp: Date.now(),
          });

          s.state = 'active';

          // Exponential scaling: 10 -> 20 -> 40 ... applied AFTER payment
          if (s.paymentMode === 'exponential') {
            s.intervalMb *= 2;
            s.nextCheckpointBytes = s.bytesTransferred + s.intervalMb * MB;
          }
          return undefined;
        }

        case 'get_payment_checkpoint_info': {
          const { sessionId } = args as any;
          const s = sessions.get(sessionId);
          if (!s) throw new Error('Session not found');
          return toInfo(s);
        }

        case 'mark_checkpoint_payment_failed': {
          const { sessionId } = args as any;
          const s = sessions.get(sessionId);
          if (!s) throw new Error('Session not found');
          s.state = 'payment_failed';
          return undefined;
        }

        case 'mark_checkpoint_completed': {
          const { sessionId } = args as any;
          const s = sessions.get(sessionId);
          if (!s) throw new Error('Session not found');
          s.state = 'completed';
          return undefined;
        }

        case 'remove_payment_checkpoint_session': {
          const { sessionId } = args as any;
          sessions.delete(sessionId);
          return undefined;
        }

        default:
          throw new Error(`Unhandled invoke command in test: ${cmd}`);
      }
    });
  });

  afterEach(async () => {
    // Cleanup after each test
    try {
      await paymentCheckpointService.removeSession(testSessionId);
    } catch (e) {
      // Session might not exist, that's okay
    }
  });

  describe('Session Initialization', () => {
    it('should initialize a checkpoint session successfully', async () => {
      await expect(
        paymentCheckpointService.initCheckpointSession(
          testSessionId,
          testFileHash,
          testFileSize,
          testSeederAddress,
          testSeederPeerId,
          pricePerMb,
          'exponential'
        )
      ).resolves.not.toThrow();
    });

    it('should create session with correct initial state', async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        testFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'exponential'
      );

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);

      expect(info.session_id).toBe(testSessionId);
      expect(info.file_hash).toBe(testFileHash);
      expect(info.file_size).toBe(testFileSize);
      expect(info.bytes_transferred).toBe(0);
      expect(info.total_paid_chiral).toBe(0);
      expect(info.payment_mode).toBe('exponential');
    });

    it('should initialize with upfront payment mode', async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        testFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'upfront'
      );

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);
      expect(info.payment_mode).toBe('upfront');
    });
  });

  describe('Progress Updates', () => {
    beforeEach(async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        testFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'exponential'
      );
    });

    it('should remain active before first checkpoint (< 10 MB)', async () => {
      const state = await paymentCheckpointService.updateProgress(
        testSessionId,
        5 * 1024 * 1024 // 5 MB
      );

      expect(state).toBe('active');
    });

    it('should trigger checkpoint at exactly 10 MB', async () => {
      const state = await paymentCheckpointService.updateProgress(
        testSessionId,
        10 * 1024 * 1024 // 10 MB
      );

      expect(state).toBe('waiting_for_payment');
    });

    it('should not trigger checkpoint at 9.9 MB', async () => {
      const state = await paymentCheckpointService.updateProgress(
        testSessionId,
        9.9 * 1024 * 1024
      );

      expect(state).toBe('active');
    });

    it('should trigger checkpoint immediately when threshold exceeded', async () => {
      const state = await paymentCheckpointService.updateProgress(
        testSessionId,
        15 * 1024 * 1024 // Jump to 15 MB
      );

      expect(state).toBe('waiting_for_payment');
    });
  });

  describe('Checkpoint Pausing', () => {
    beforeEach(async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        testFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'exponential'
      );
    });

    it('should not pause before checkpoint', async () => {
      await paymentCheckpointService.updateProgress(testSessionId, 5 * 1024 * 1024);

      const shouldPause = await paymentCheckpointService.shouldPauseServing(testSessionId);
      expect(shouldPause).toBe(false);
    });

    it('should pause at checkpoint', async () => {
      await paymentCheckpointService.updateProgress(testSessionId, 10 * 1024 * 1024);

      const shouldPause = await paymentCheckpointService.shouldPauseServing(testSessionId);
      expect(shouldPause).toBe(true);
    });

    it('should not pause after payment', async () => {
      // Reach checkpoint
      await paymentCheckpointService.updateProgress(testSessionId, 10 * 1024 * 1024);

      // Pay
      await paymentCheckpointService.recordPayment(
        testSessionId,
        '0x123abc...',
        0.01
      );

      // Should not pause anymore
      const shouldPause = await paymentCheckpointService.shouldPauseServing(testSessionId);
      expect(shouldPause).toBe(false);
    });
  });

  describe('Payment Recording', () => {
    beforeEach(async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        testFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'exponential'
      );

      // Reach first checkpoint
      await paymentCheckpointService.updateProgress(testSessionId, 10 * 1024 * 1024);
    });

    it('should record payment successfully', async () => {
      await expect(
        paymentCheckpointService.recordPayment(
          testSessionId,
          '0x123abc...',
          0.01
        )
      ).resolves.not.toThrow();
    });

    it('should update total paid amount', async () => {
      await paymentCheckpointService.recordPayment(testSessionId, '0x123', 0.01);

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);
      expect(info.total_paid_chiral).toBe(0.01);
    });

    it('should accumulate multiple payments', async () => {
      // First payment
      await paymentCheckpointService.recordPayment(testSessionId, '0x123', 0.01);

      // Reach second checkpoint (30 MB)
      await paymentCheckpointService.updateProgress(testSessionId, 30 * 1024 * 1024);

      // Second payment
      await paymentCheckpointService.recordPayment(testSessionId, '0x456', 0.02);

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);
      expect(info.total_paid_chiral).toBe(0.03); // 0.01 + 0.02
    });

    it('should track payment history', async () => {
      await paymentCheckpointService.recordPayment(testSessionId, '0x123', 0.01);

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);
      expect(info.checkpoint_history).toHaveLength(1);
      expect(info.checkpoint_history[0].amount_paid).toBe(0.01);
      expect(info.checkpoint_history[0].transaction_hash).toBe('0x123');
    });
  });

  describe('Exponential Scaling', () => {
    beforeEach(async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        testFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'exponential'
      );
    });

    it('should scale from 10 MB to 20 MB', async () => {
      // First checkpoint at 10 MB
      await paymentCheckpointService.updateProgress(testSessionId, 10 * 1024 * 1024);
      await paymentCheckpointService.recordPayment(testSessionId, '0x1', 0.01);

      // Get info to check next checkpoint
      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);

      // Next checkpoint should be at 30 MB (10 + 20)
      expect(info.next_checkpoint_bytes).toBe(30 * 1024 * 1024);
      expect(info.last_checkpoint_mb).toBe(20);
    });

    it('should scale from 20 MB to 40 MB', async () => {
      // Checkpoint 1: 10 MB
      await paymentCheckpointService.updateProgress(testSessionId, 10 * 1024 * 1024);
      await paymentCheckpointService.recordPayment(testSessionId, '0x1', 0.01);

      // Checkpoint 2: 30 MB (10 + 20)
      await paymentCheckpointService.updateProgress(testSessionId, 30 * 1024 * 1024);
      await paymentCheckpointService.recordPayment(testSessionId, '0x2', 0.02);

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);

      // Next checkpoint should be at 70 MB (30 + 40)
      expect(info.next_checkpoint_bytes).toBe(70 * 1024 * 1024);
      expect(info.last_checkpoint_mb).toBe(40);
    });

    it('should follow complete exponential sequence', async () => {
      const sequence = [
        { bytes: 10 * 1024 * 1024, payment: 0.01, nextCheckpoint: 30 * 1024 * 1024 },
        { bytes: 30 * 1024 * 1024, payment: 0.02, nextCheckpoint: 70 * 1024 * 1024 },
        { bytes: 70 * 1024 * 1024, payment: 0.04, nextCheckpoint: 150 * 1024 * 1024 },
      ];

      for (let i = 0; i < sequence.length; i++) {
        const step = sequence[i];

        await paymentCheckpointService.updateProgress(testSessionId, step.bytes);
        await paymentCheckpointService.recordPayment(testSessionId, `0x${i}`, step.payment);

        const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);
        expect(info.next_checkpoint_bytes).toBe(step.nextCheckpoint);
      }
    });
  });

  describe('Upfront Payment Mode', () => {
    it('should require payment for full file size', async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        testFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'upfront'
      );

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);

      // In upfront mode, checkpoint is set to full file size
      expect(info.next_checkpoint_bytes).toBe(testFileSize);
    });
  });

  describe('Cost Calculations', () => {
    beforeEach(async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        testFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'exponential'
      );
    });

    it('should calculate correct remaining cost', async () => {
      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);
      const remaining = paymentCheckpointService.calculateRemainingCost(info);

      // 100 MB file * 0.001 Chiral/MB = 0.1 Chiral
      expect(remaining).toBeCloseTo(0.1, 4);
    });

    it('should calculate remaining cost after partial download', async () => {
      // Download 20 MB
      await paymentCheckpointService.updateProgress(testSessionId, 20 * 1024 * 1024);

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);
      const remaining = paymentCheckpointService.calculateRemainingCost(info);

      // Remaining: 80 MB * 0.001 = 0.08 Chiral
      expect(remaining).toBeCloseTo(0.08, 4);
    });
  });

  describe('State Management', () => {
    beforeEach(async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        testFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'exponential'
      );
    });

    it('should mark payment as failed', async () => {
      await paymentCheckpointService.updateProgress(testSessionId, 10 * 1024 * 1024);

      await expect(
        paymentCheckpointService.markPaymentFailed(testSessionId, 'Insufficient balance')
      ).resolves.not.toThrow();
    });

    it('should mark session as completed', async () => {
      await expect(
        paymentCheckpointService.markCompleted(testSessionId)
      ).resolves.not.toThrow();
    });

    it('should remove session', async () => {
      await paymentCheckpointService.removeSession(testSessionId);

      // Should throw when trying to get info of removed session
      await expect(
        paymentCheckpointService.getCheckpointInfo(testSessionId)
      ).rejects.toThrow();
    });
  });

  describe('Edge Cases', () => {
    it('should handle non-existent session gracefully', async () => {
      await expect(
        paymentCheckpointService.updateProgress('non-existent', 1024)
      ).rejects.toThrow();
    });

    it('should handle zero file size', async () => {
      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        0,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'exponential'
      );

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);
      expect(info.file_size).toBe(0);
    });

    it('should handle very large file sizes', async () => {
      const largeFileSize = 10 * 1024 * 1024 * 1024; // 10 GB

      await paymentCheckpointService.initCheckpointSession(
        testSessionId,
        testFileHash,
        largeFileSize,
        testSeederAddress,
        testSeederPeerId,
        pricePerMb,
        'exponential'
      );

      const info = await paymentCheckpointService.getCheckpointInfo(testSessionId);
      expect(info.file_size).toBe(largeFileSize);
    });
  });
});
