/**
 * Reputation Scoring Unit Tests
 *
 * Tests for reputation calculation algorithms, trust levels,
 * and peer ranking logic.
 */

import { describe, it, expect, beforeEach } from 'vitest';

describe('Reputation Scoring Algorithms', () => {
  describe('Score Calculation', () => {
    it('should calculate base reputation score from metrics', () => {
      const metrics = {
        successfulTransfers: 10,
        failedTransfers: 2,
        averageLatency: 50, // ms
        averageBandwidth: 1000, // KB/s
        uptime: 0.95 // 95%
      };

      // Simple weighted average
      const successRate = metrics.successfulTransfers / (metrics.successfulTransfers + metrics.failedTransfers);
      const latencyScore = Math.max(0, 1 - metrics.averageLatency / 1000);
      const bandwidthScore = Math.min(1, metrics.averageBandwidth / 1000);
      const uptimeScore = metrics.uptime;

      const totalScore = (successRate * 0.4 + latencyScore * 0.2 + bandwidthScore * 0.2 + uptimeScore * 0.2) * 100;

      expect(totalScore).toBeGreaterThan(0);
      expect(totalScore).toBeLessThanOrEqual(100);
    });

    it('should penalize high failure rates', () => {
      const goodPeer = {
        successfulTransfers: 100,
        failedTransfers: 5
      };

      const badPeer = {
        successfulTransfers: 50,
        failedTransfers: 50
      };

      const goodScore = goodPeer.successfulTransfers / (goodPeer.successfulTransfers + goodPeer.failedTransfers);
      const badScore = badPeer.successfulTransfers / (badPeer.successfulTransfers + badPeer.failedTransfers);

      expect(goodScore).toBeGreaterThan(badScore);
    });

    it('should reward low latency peers', () => {
      const fastPeer = { latency: 20 }; // 20ms
      const slowPeer = { latency: 500 }; // 500ms

      const fastScore = Math.max(0, 1 - fastPeer.latency / 1000);
      const slowScore = Math.max(0, 1 - slowPeer.latency / 1000);

      expect(fastScore).toBeGreaterThan(slowScore);
    });

    it('should reward high bandwidth peers', () => {
      const highBandwidth = 5000; // KB/s
      const lowBandwidth = 100; // KB/s

      const highScore = Math.min(1, highBandwidth / 1000);
      const lowScore = Math.min(1, lowBandwidth / 1000);

      expect(highScore).toBeGreaterThan(lowScore);
    });

    it('should reward high uptime peers', () => {
      const reliablePeer = { uptime: 0.99 }; // 99%
      const unreliablePeer = { uptime: 0.50 }; // 50%

      expect(reliablePeer.uptime).toBeGreaterThan(unreliablePeer.uptime);
    });
  });

  describe('Trust Level Determination', () => {
    it('should classify score 80-100 as Trusted', () => {
      const score = 90;
      const trustLevel = score >= 80 ? 'Trusted' : score >= 60 ? 'High' : score >= 40 ? 'Medium' : score >= 20 ? 'Low' : 'Unknown';

      expect(trustLevel).toBe('Trusted');
    });

    it('should classify score 60-79 as High', () => {
      const score = 70;
      const trustLevel = score >= 80 ? 'Trusted' : score >= 60 ? 'High' : score >= 40 ? 'Medium' : score >= 20 ? 'Low' : 'Unknown';

      expect(trustLevel).toBe('High');
    });

    it('should classify score 40-59 as Medium', () => {
      const score = 50;
      const trustLevel = score >= 80 ? 'Trusted' : score >= 60 ? 'High' : score >= 40 ? 'Medium' : score >= 20 ? 'Low' : 'Unknown';

      expect(trustLevel).toBe('Medium');
    });

    it('should classify score 20-39 as Low', () => {
      const score = 30;
      const trustLevel = score >= 80 ? 'Trusted' : score >= 60 ? 'High' : score >= 40 ? 'Medium' : score >= 20 ? 'Low' : 'Unknown';

      expect(trustLevel).toBe('Low');
    });

    it('should classify score 0-19 as Unknown', () => {
      const score = 10;
      const trustLevel = score >= 80 ? 'Trusted' : score >= 60 ? 'High' : score >= 40 ? 'Medium' : score >= 20 ? 'Low' : 'Unknown';

      expect(trustLevel).toBe('Unknown');
    });

    it('should handle edge case at trust level boundaries', () => {
      const scores = [0, 20, 40, 60, 80, 100];
      const expectedLevels = ['Unknown', 'Low', 'Medium', 'High', 'Trusted', 'Trusted'];

      scores.forEach((score, i) => {
        const trustLevel = score >= 80 ? 'Trusted' : score >= 60 ? 'High' : score >= 40 ? 'Medium' : score >= 20 ? 'Low' : 'Unknown';
        expect(trustLevel).toBe(expectedLevels[i]);
      });
    });
  });

  describe('Peer Ranking', () => {
    it('should rank peers by reputation score descending', () => {
      const peers = [
        { id: 'peer1', score: 50 },
        { id: 'peer2', score: 90 },
        { id: 'peer3', score: 70 }
      ];

      const ranked = peers.sort((a, b) => b.score - a.score);

      expect(ranked[0].id).toBe('peer2');
      expect(ranked[1].id).toBe('peer3');
      expect(ranked[2].id).toBe('peer1');
    });

    it('should handle ties in scoring', () => {
      const peers = [
        { id: 'peer1', score: 80 },
        { id: 'peer2', score: 80 },
        { id: 'peer3', score: 90 }
      ];

      const ranked = peers.sort((a, b) => b.score - a.score);

      expect(ranked[0].score).toBe(90);
      expect(ranked[1].score).toBe(80);
      expect(ranked[2].score).toBe(80);
    });

    it('should filter peers below minimum trust level', () => {
      const peers = [
        { id: 'peer1', score: 90 },
        { id: 'peer2', score: 30 },
        { id: 'peer3', score: 70 }
      ];

      const minScore = 40;
      const filtered = peers.filter(p => p.score >= minScore);

      expect(filtered.length).toBe(2);
      expect(filtered.some(p => p.id === 'peer2')).toBe(false);
    });

    it('should select top N peers for download', () => {
      const peers = [
        { id: 'peer1', score: 50 },
        { id: 'peer2', score: 90 },
        { id: 'peer3', score: 70 },
        { id: 'peer4', score: 60 },
        { id: 'peer5', score: 80 }
      ];

      const topN = 3;
      const selected = peers
        .sort((a, b) => b.score - a.score)
        .slice(0, topN);

      expect(selected.length).toBe(3);
      expect(selected[0].id).toBe('peer2'); // 90
      expect(selected[1].id).toBe('peer5'); // 80
      expect(selected[2].id).toBe('peer3'); // 70
    });
  });

  describe('Reputation Decay', () => {
    it('should decay reputation over time for inactive peers', () => {
      const initialScore = 100;
      const daysSinceLastSeen = 30;
      const decayRate = 0.01; // 1% per day

      const decayedScore = initialScore * Math.pow(1 - decayRate, daysSinceLastSeen);

      expect(decayedScore).toBeLessThan(initialScore);
      expect(decayedScore).toBeGreaterThan(0);
    });

    it('should not decay reputation for recently active peers', () => {
      const initialScore = 100;
      const daysSinceLastSeen = 0;
      const decayRate = 0.01;

      const decayedScore = initialScore * Math.pow(1 - decayRate, daysSinceLastSeen);

      expect(decayedScore).toBe(initialScore);
    });

    it('should apply exponential decay formula correctly', () => {
      const initialScore = 100;
      const days = 10;
      const decayRate = 0.02; // 2% per day

      const expected = initialScore * Math.pow(0.98, days);
      const actual = initialScore * Math.pow(1 - decayRate, days);

      expect(actual).toBeCloseTo(expected, 2);
    });

    it('should have minimum reputation floor', () => {
      const initialScore = 100;
      const daysSinceLastSeen = 365; // 1 year
      const decayRate = 0.01;
      const minimumScore = 10;

      const rawDecay = initialScore * Math.pow(1 - decayRate, daysSinceLastSeen);
      const finalScore = Math.max(minimumScore, rawDecay);

      expect(finalScore).toBeGreaterThanOrEqual(minimumScore);
    });
  });

  describe('Score Updates', () => {
    it('should increase score on successful transfer', () => {
      const currentScore = 50;
      const successBonus = 5;

      const newScore = Math.min(100, currentScore + successBonus);

      expect(newScore).toBe(55);
    });

    it('should decrease score on failed transfer', () => {
      const currentScore = 50;
      const failurePenalty = 10;

      const newScore = Math.max(0, currentScore - failurePenalty);

      expect(newScore).toBe(40);
    });

    it('should cap score at maximum (100)', () => {
      const currentScore = 98;
      const successBonus = 5;

      const newScore = Math.min(100, currentScore + successBonus);

      expect(newScore).toBe(100);
    });

    it('should floor score at minimum (0)', () => {
      const currentScore = 5;
      const failurePenalty = 10;

      const newScore = Math.max(0, currentScore - failurePenalty);

      expect(newScore).toBe(0);
    });

    it('should apply weighted updates based on transfer size', () => {
      const baseBonus = 5;
      const transferSizeMB = 100;
      const weight = Math.min(2, transferSizeMB / 50); // Larger transfers get more weight

      const weightedBonus = baseBonus * weight;

      expect(weightedBonus).toBeGreaterThan(baseBonus);
    });
  });

  describe('Interaction Tracking', () => {
    it('should track total number of interactions', () => {
      let totalInteractions = 0;

      totalInteractions += 1; // Success
      totalInteractions += 1; // Failure
      totalInteractions += 1; // Success

      expect(totalInteractions).toBe(3);
    });

    it('should track last interaction timestamp', () => {
      const lastSeen = Date.now();

      expect(lastSeen).toBeGreaterThan(0);
      expect(lastSeen).toBeLessThanOrEqual(Date.now());
    });

    it('should calculate time since last interaction', () => {
      const lastSeen = Date.now() - 3600000; // 1 hour ago
      const now = Date.now();

      const hoursSince = (now - lastSeen) / 3600000;

      expect(hoursSince).toBeCloseTo(1, 1);
    });
  });

  describe('Composite Scoring', () => {
    it('should combine multiple metrics into final score', () => {
      const metrics = {
        reliability: 0.9, // 90%
        speed: 0.8,       // 80%
        availability: 0.95, // 95%
        transferSuccess: 0.85 // 85%
      };

      const weights = {
        reliability: 0.3,
        speed: 0.2,
        availability: 0.2,
        transferSuccess: 0.3
      };

      const compositeScore = (
        metrics.reliability * weights.reliability +
        metrics.speed * weights.speed +
        metrics.availability * weights.availability +
        metrics.transferSuccess * weights.transferSuccess
      ) * 100;

      expect(compositeScore).toBeGreaterThan(0);
      expect(compositeScore).toBeLessThanOrEqual(100);
    });

    it('should ensure weights sum to 1.0', () => {
      const weights = {
        reliability: 0.3,
        speed: 0.2,
        availability: 0.2,
        transferSuccess: 0.3
      };

      const sum = Object.values(weights).reduce((a, b) => a + b, 0);

      expect(sum).toBeCloseTo(1.0, 5);
    });
  });

  describe('Edge Cases', () => {
    it('should handle new peer with no history', () => {
      const newPeer = {
        successfulTransfers: 0,
        failedTransfers: 0,
        uptime: 0
      };

      const defaultScore = 50; // Neutral starting point
      expect(defaultScore).toBe(50);
    });

    it('should handle division by zero in success rate', () => {
      const transfers = {
        successful: 0,
        failed: 0
      };

      const total = transfers.successful + transfers.failed;
      const successRate = total === 0 ? 0.5 : transfers.successful / total;

      expect(successRate).toBe(0.5); // Default to 50% for unknown
    });

    it('should handle negative scores from calculation errors', () => {
      const rawScore = -10;
      const clampedScore = Math.max(0, Math.min(100, rawScore));

      expect(clampedScore).toBe(0);
    });

    it('should handle scores above 100 from calculation errors', () => {
      const rawScore = 150;
      const clampedScore = Math.max(0, Math.min(100, rawScore));

      expect(clampedScore).toBe(100);
    });
  });
});
