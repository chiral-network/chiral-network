import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { paymentCheckpointService } from '../src/lib/services/paymentCheckpointService';

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
