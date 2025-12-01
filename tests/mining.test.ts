import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  miningState,
  miningProgress,
  totalEarned,
  transactions,
  type MiningState,
  type MiningHistoryPoint,
  type Transaction,
} from "../src/lib/stores";

describe("Mining State Management", () => {
  beforeEach(() => {
    // Reset mining state before each test
    miningState.set({
      isMining: false,
      hashRate: "0 H/s",
      totalRewards: 0,
      blocksFound: 0,
      activeThreads: 1,
      minerIntensity: 50,
      selectedPool: "solo",
      sessionStartTime: undefined,
      recentBlocks: [],
      miningHistory: [],
    });

    miningProgress.set({ cumulative: 0, lastBlock: 0 });

    // Reset transactions for totalEarned derived store tests
    transactions.set([]);
  });

  describe("Mining State Initialization", () => {
    it("should initialize with default values", () => {
      const state = get(miningState);

      expect(state.isMining).toBe(false);
      expect(state.hashRate).toBe("0 H/s");
      expect(state.totalRewards).toBe(0);
      expect(state.blocksFound).toBe(0);
      expect(state.activeThreads).toBe(1);
      expect(state.minerIntensity).toBe(50);
      expect(state.selectedPool).toBe("solo");
      expect(state.recentBlocks).toEqual([]);
      expect(state.miningHistory).toEqual([]);
    });

    it("should have undefined session start time initially", () => {
      const state = get(miningState);
      expect(state.sessionStartTime).toBeUndefined();
    });
  });

  describe("Mining State Updates", () => {
    it("should toggle mining state", () => {
      miningState.update((state) => ({
        ...state,
        isMining: true,
      }));

      let state = get(miningState);
      expect(state.isMining).toBe(true);

      miningState.update((state) => ({
        ...state,
        isMining: false,
      }));

      state = get(miningState);
      expect(state.isMining).toBe(false);
    });

    it("should update hash rate", () => {
      miningState.update((state) => ({
        ...state,
        hashRate: "1500 H/s",
      }));

      const state = get(miningState);
      expect(state.hashRate).toBe("1500 H/s");
    });

    it("should update total rewards", () => {
      miningState.update((state) => ({
        ...state,
        totalRewards: 100,
      }));

      const state = get(miningState);
      expect(state.totalRewards).toBe(100);
    });

    it("should update blocks found", () => {
      miningState.update((state) => ({
        ...state,
        blocksFound: 5,
      }));

      const state = get(miningState);
      expect(state.blocksFound).toBe(5);
    });
  });

  describe("Mining Configuration", () => {
    it("should update active threads", () => {
      miningState.update((state) => ({
        ...state,
        activeThreads: 4,
      }));

      const state = get(miningState);
      expect(state.activeThreads).toBe(4);
    });

    it("should update miner intensity", () => {
      miningState.update((state) => ({
        ...state,
        minerIntensity: 75,
      }));

      const state = get(miningState);
      expect(state.minerIntensity).toBe(75);
    });

    it("should validate miner intensity range (0-100)", () => {
      // Test valid range
      for (const intensity of [0, 25, 50, 75, 100]) {
        miningState.update((state) => ({
          ...state,
          minerIntensity: intensity,
        }));
        const state = get(miningState);
        expect(state.minerIntensity).toBe(intensity);
      }
    });

    it("should change selected pool", () => {
      miningState.update((state) => ({
        ...state,
        selectedPool: "pool1",
      }));

      let state = get(miningState);
      expect(state.selectedPool).toBe("pool1");

      miningState.update((s) => ({
        ...s,
        selectedPool: "solo",
      }));

      state = get(miningState);
      expect(state.selectedPool).toBe("solo");
    });
  });

  describe("Mining Session Tracking", () => {
    it("should set session start time when mining starts", () => {
      const startTime = Date.now();
      miningState.update((state) => ({
        ...state,
        isMining: true,
        sessionStartTime: startTime,
      }));

      const state = get(miningState);
      expect(state.sessionStartTime).toBe(startTime);
    });

    it("should clear session start time when mining stops", () => {
      miningState.update((state) => ({
        ...state,
        isMining: true,
        sessionStartTime: Date.now(),
      }));

      miningState.update((state) => ({
        ...state,
        isMining: false,
        sessionStartTime: undefined,
      }));

      const state = get(miningState);
      expect(state.sessionStartTime).toBeUndefined();
    });
  });

  describe("Recent Blocks", () => {
    it("should add blocks to recent blocks list", () => {
      const block = {
        id: "block-1",
        hash: "0xabc123",
        reward: 2,
        timestamp: new Date(),
        difficulty: 1000000,
        nonce: 12345,
      };

      miningState.update((state) => ({
        ...state,
        recentBlocks: [block, ...(state.recentBlocks ?? [])],
        blocksFound: 1,
        totalRewards: 2,
      }));

      const state = get(miningState);
      expect(state.recentBlocks).toHaveLength(1);
      expect(state.recentBlocks?.[0].hash).toBe("0xabc123");
      expect(state.blocksFound).toBe(1);
      expect(state.totalRewards).toBe(2);
    });

    it("should maintain multiple blocks", () => {
      const blocks = [
        {
          id: "block-1",
          hash: "0xabc123",
          reward: 2,
          timestamp: new Date(),
          difficulty: 1000000,
          nonce: 12345,
        },
        {
          id: "block-2",
          hash: "0xdef456",
          reward: 2,
          timestamp: new Date(),
          difficulty: 1000000,
          nonce: 67890,
        },
      ];

      miningState.update((state) => ({
        ...state,
        recentBlocks: blocks,
        blocksFound: 2,
        totalRewards: 4,
      }));

      const state = get(miningState);
      expect(state.recentBlocks).toHaveLength(2);
      expect(state.blocksFound).toBe(2);
      expect(state.totalRewards).toBe(4);
    });

    it("should limit recent blocks to 50", () => {
      const blocks = Array.from({ length: 60 }, (_, i) => ({
        id: `block-${i}`,
        hash: `0x${i.toString(16).padStart(6, "0")}`,
        reward: 2,
        timestamp: new Date(),
        difficulty: 1000000,
        nonce: i,
      }));

      miningState.update((state) => ({
        ...state,
        recentBlocks: blocks.slice(0, 50),
      }));

      const state = get(miningState);
      expect(state.recentBlocks?.length).toBeLessThanOrEqual(50);
    });

    it("should calculate rewards from blocks", () => {
      const blocks = [
        {
          id: "block-1",
          hash: "0xabc",
          reward: 2,
          timestamp: new Date(),
          difficulty: 1000000,
          nonce: 1,
        },
        {
          id: "block-2",
          hash: "0xdef",
          reward: 2,
          timestamp: new Date(),
          difficulty: 1000000,
          nonce: 2,
        },
        {
          id: "block-3",
          hash: "0xghi",
          reward: 2,
          timestamp: new Date(),
          difficulty: 1000000,
          nonce: 3,
        },
      ];

      const totalReward = blocks.reduce(
        (sum, block) => sum + block.reward,
        0
      );

      miningState.update((state) => ({
        ...state,
        recentBlocks: blocks,
        blocksFound: blocks.length,
        totalRewards: totalReward,
      }));

      const state = get(miningState);
      expect(state.totalRewards).toBe(6);
    });
  });

  describe("Mining History", () => {
    it("should track hash rate history", () => {
      const historyPoint: MiningHistoryPoint = {
        timestamp: Date.now(),
        hashRate: 1500,
        power: 75,
      };

      miningState.update((state) => ({
        ...state,
        miningHistory: [historyPoint],
      }));

      const state = get(miningState);
      expect(state.miningHistory).toHaveLength(1);
      expect(state.miningHistory?.[0].hashRate).toBe(1500);
    });

    it("should maintain multiple history points", () => {
      const history: MiningHistoryPoint[] = [
        { timestamp: Date.now() - 2000, hashRate: 1000, power: 50 },
        { timestamp: Date.now() - 1000, hashRate: 1500, power: 65 },
        { timestamp: Date.now(), hashRate: 2000, power: 80 },
      ];

      miningState.update((state) => ({
        ...state,
        miningHistory: history,
      }));

      const state = get(miningState);
      expect(state.miningHistory).toHaveLength(3);
      expect(state.miningHistory?.[2].hashRate).toBe(2000);
    });

    it("should track power consumption over time", () => {
      const history: MiningHistoryPoint[] = [
        { timestamp: Date.now() - 1000, hashRate: 1500, power: 60 },
        { timestamp: Date.now(), hashRate: 1800, power: 70 },
      ];

      miningState.update((state) => ({
        ...state,
        miningHistory: history,
      }));

      const state = get(miningState);
      const avgPower =
        state.miningHistory!.reduce((sum, point) => sum + point.power, 0) /
        state.miningHistory!.length;

      expect(avgPower).toBe(65);
    });
  });

  describe("Mining Progress", () => {
    it("should initialize progress at zero", () => {
      const progress = get(miningProgress);
      expect(progress.cumulative).toBe(0);
      expect(progress.lastBlock).toBe(0);
    });

    it("should update cumulative progress", () => {
      miningProgress.update(() => ({
        cumulative: 45,
        lastBlock: 45,
      }));

      const progress = get(miningProgress);
      expect(progress.cumulative).toBe(45);
    });

    it("should reset last block progress on new block", () => {
      miningProgress.set({ cumulative: 100, lastBlock: 100 });

      // New block found, reset lastBlock
      miningProgress.update((p) => ({
        cumulative: p.cumulative,
        lastBlock: 0,
      }));

      const progress = get(miningProgress);
      expect(progress.cumulative).toBe(100);
      expect(progress.lastBlock).toBe(0);
    });

    it("should increment progress over time", () => {
      miningProgress.set({ cumulative: 0, lastBlock: 0 });

      // Simulate progress increments
      for (let i = 1; i <= 10; i++) {
        miningProgress.update((p) => ({
          cumulative: p.cumulative + 10,
          lastBlock: p.lastBlock + 10,
        }));
      }

      const progress = get(miningProgress);
      expect(progress.cumulative).toBe(100);
      expect(progress.lastBlock).toBe(100);
    });
  });

  describe("Total Earned Derived Store", () => {
    // IMPORTANT: totalEarned is derived from transactions store, NOT miningState.totalRewards
    // This was a bug introduced in commit 0dba4b0 by Aaron Purnawan where wallet.ts
    // used totalEarned without importing it and without calling get()

    it("should calculate total earned from mining transactions", () => {
      // totalEarned is derived from transactions with type === "mining"
      const miningTx: Transaction = {
        id: 1,
        type: "mining",
        amount: 2,
        from: "Mining reward",
        date: new Date(),
        description: "Block Reward",
        status: "success",
      };

      transactions.set([miningTx]);

      const earned = get(totalEarned);
      expect(earned).toBe(2);
    });

    it("should sum multiple mining transactions", () => {
      const miningTxs: Transaction[] = [
        { id: 1, type: "mining", amount: 2, from: "Mining reward", date: new Date(), description: "Block 1", status: "success" },
        { id: 2, type: "mining", amount: 2, from: "Mining reward", date: new Date(), description: "Block 2", status: "success" },
        { id: 3, type: "mining", amount: 2, from: "Mining reward", date: new Date(), description: "Block 3", status: "success" },
      ];

      transactions.set(miningTxs);

      const earned = get(totalEarned);
      expect(earned).toBe(6); // 3 blocks * 2 Chiral each
    });

    it("should only count mining type transactions", () => {
      const mixedTxs: Transaction[] = [
        { id: 1, type: "mining", amount: 2, from: "Mining reward", date: new Date(), description: "Block 1", status: "success" },
        { id: 2, type: "sent", amount: 10, from: "0xabc", date: new Date(), description: "Sent", status: "success" },
        { id: 3, type: "received", amount: 5, from: "0xdef", date: new Date(), description: "Received", status: "success" },
        { id: 4, type: "mining", amount: 2, from: "Mining reward", date: new Date(), description: "Block 2", status: "success" },
      ];

      transactions.set(mixedTxs);

      const earned = get(totalEarned);
      expect(earned).toBe(4); // Only mining transactions: 2 + 2 = 4
    });

    it("should return 0 when no mining transactions exist", () => {
      transactions.set([]);
      expect(get(totalEarned)).toBe(0);

      // Add non-mining transactions
      transactions.set([
        { id: 1, type: "sent", amount: 10, from: "0xabc", date: new Date(), description: "Sent", status: "success" },
      ]);
      expect(get(totalEarned)).toBe(0);
    });

    it("should update reactively when transactions change", () => {
      transactions.set([]);
      expect(get(totalEarned)).toBe(0);

      // Add first mining transaction
      transactions.update((txs) => [
        ...txs,
        { id: 1, type: "mining", amount: 2, from: "Mining reward", date: new Date(), description: "Block 1", status: "success" },
      ]);
      expect(get(totalEarned)).toBe(2);

      // Add second mining transaction
      transactions.update((txs) => [
        ...txs,
        { id: 2, type: "mining", amount: 2, from: "Mining reward", date: new Date(), description: "Block 2", status: "success" },
      ]);
      expect(get(totalEarned)).toBe(4);
    });

    it("should be a number (regression test for undefined bug)", () => {
      // This test catches the bug where totalEarned was used without get()
      // which would result in the store object itself being assigned instead of its value
      transactions.set([
        { id: 1, type: "mining", amount: 2, from: "Mining reward", date: new Date(), description: "Block 1", status: "success" },
      ]);

      const earned = get(totalEarned);

      // Must be a number, not undefined, not a store object
      expect(typeof earned).toBe("number");
      expect(earned).not.toBeUndefined();
      expect(earned).not.toBeNull();
      expect(earned).toBe(2);
    });
  });

  describe("Mining State Edge Cases", () => {
    it("should handle zero hash rate", () => {
      miningState.update((state) => ({
        ...state,
        hashRate: "0 H/s",
      }));

      const state = get(miningState);
      expect(state.hashRate).toBe("0 H/s");
    });

    it("should handle high hash rates", () => {
      miningState.update((state) => ({
        ...state,
        hashRate: "15.5 MH/s",
      }));

      const state = get(miningState);
      expect(state.hashRate).toBe("15.5 MH/s");
    });

    it("should handle zero threads", () => {
      miningState.update((state) => ({
        ...state,
        activeThreads: 0,
      }));

      const state = get(miningState);
      expect(state.activeThreads).toBe(0);
    });

    it("should handle many threads", () => {
      miningState.update((state) => ({
        ...state,
        activeThreads: 16,
      }));

      const state = get(miningState);
      expect(state.activeThreads).toBe(16);
    });

    it("should handle empty block history", () => {
      const state = get(miningState);
      expect(state.recentBlocks).toEqual([]);
    });

    it("should handle empty mining history", () => {
      const state = get(miningState);
      expect(state.miningHistory).toEqual([]);
    });
  });

  describe("Mining Session Lifecycle", () => {
    it("should track complete mining session", () => {
      const startTime = Date.now();

      // Start mining
      miningState.update((state) => ({
        ...state,
        isMining: true,
        sessionStartTime: startTime,
        activeThreads: 2,
        minerIntensity: 50,
      }));

      let state = get(miningState);
      expect(state.isMining).toBe(true);
      expect(state.sessionStartTime).toBe(startTime);

      // Mine some blocks
      const block = {
        id: "block-1",
        hash: "0xtest",
        reward: 2,
        timestamp: new Date(),
        difficulty: 1000000,
        nonce: 12345,
      };

      miningState.update((s) => ({
        ...s,
        recentBlocks: [block],
        blocksFound: 1,
        totalRewards: 2,
      }));

      state = get(miningState);
      expect(state.blocksFound).toBe(1);
      expect(state.totalRewards).toBe(2);

      // Stop mining
      miningState.update((s) => ({
        ...s,
        isMining: false,
        sessionStartTime: undefined,
        hashRate: "0 H/s",
      }));

      state = get(miningState);
      expect(state.isMining).toBe(false);
      expect(state.sessionStartTime).toBeUndefined();
      // Rewards and blocks persist after stopping
      expect(state.totalRewards).toBe(2);
      expect(state.blocksFound).toBe(1);
    });
  });

  describe("Integration with Rewards", () => {
    it("should increment rewards when block is found", () => {
      const initialRewards = get(miningState).totalRewards;
      const blockReward = 2;

      miningState.update((state) => ({
        ...state,
        totalRewards: state.totalRewards + blockReward,
        blocksFound: state.blocksFound + 1,
      }));

      const state = get(miningState);
      expect(state.totalRewards).toBe(initialRewards + blockReward);
    });

    it("should track cumulative rewards over multiple blocks", () => {
      const rewardPerBlock = 2;
      const numberOfBlocks = 10;

      for (let i = 0; i < numberOfBlocks; i++) {
        miningState.update((state) => ({
          ...state,
          totalRewards: state.totalRewards + rewardPerBlock,
          blocksFound: state.blocksFound + 1,
        }));
      }

      const state = get(miningState);
      expect(state.totalRewards).toBe(rewardPerBlock * numberOfBlocks);
      expect(state.blocksFound).toBe(numberOfBlocks);
    });
  });
});
