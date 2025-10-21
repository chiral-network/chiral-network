import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  transactions,
  wallet,
  totalSpent,
  type Transaction,
  type WalletInfo,
} from "../src/lib/stores";

describe("Transaction Management", () => {
  beforeEach(() => {
    // Reset stores before each test
    transactions.set([]);
    wallet.set({
      address: "0x1234567890123456789012345678901234567890",
      balance: 1000,
      pendingTransactions: 0,
    });
  });

  describe("Transaction Store", () => {
    it("should initialize with empty array", () => {
      transactions.set([]);
      const txList = get(transactions);
      expect(txList).toEqual([]);
    });

    it("should add new transaction", () => {
      const newTx: Transaction = {
        id: 1,
        type: "received",
        amount: 50,
        from: "0x9876543210987654321098765432109876543210",
        date: new Date(),
        description: "Test reward",
        status: "completed",
      };

      transactions.set([newTx]);
      const txList = get(transactions);

      expect(txList).toHaveLength(1);
      expect(txList[0]).toEqual(newTx);
    });

    it("should handle multiple transactions", () => {
      const txs: Transaction[] = [
        {
          id: 1,
          type: "received",
          amount: 50,
          from: "0xabc",
          date: new Date(),
          description: "Mining reward",
          status: "completed",
        },
        {
          id: 2,
          type: "sent",
          amount: 10,
          to: "0xdef",
          date: new Date(),
          description: "Payment",
          status: "completed",
        },
        {
          id: 3,
          type: "received",
          amount: 25,
          from: "0xghi",
          date: new Date(),
          description: "Storage reward",
          status: "completed",
        },
      ];

      transactions.set(txs);
      const txList = get(transactions);

      expect(txList).toHaveLength(3);
      expect(txList[0].type).toBe("received");
      expect(txList[1].type).toBe("sent");
    });
  });

  describe("Transaction Types", () => {
    it("should handle received transactions", () => {
      const receivedTx: Transaction = {
        id: 1,
        type: "received",
        amount: 100,
        from: "0xsender",
        date: new Date(),
        description: "Mining reward",
        status: "completed",
      };

      transactions.set([receivedTx]);
      const txList = get(transactions);

      expect(txList[0].type).toBe("received");
      expect(txList[0].from).toBeDefined();
      expect(txList[0].to).toBeUndefined();
    });

    it("should handle sent transactions", () => {
      const sentTx: Transaction = {
        id: 2,
        type: "sent",
        amount: 50,
        to: "0xrecipient",
        date: new Date(),
        description: "Payment",
        status: "completed",
      };

      transactions.set([sentTx]);
      const txList = get(transactions);

      expect(txList[0].type).toBe("sent");
      expect(txList[0].to).toBeDefined();
      expect(txList[0].from).toBeUndefined();
    });
  });

  describe("Transaction Status", () => {
    it("should handle pending transactions", () => {
      const pendingTx: Transaction = {
        id: 1,
        type: "sent",
        amount: 10,
        to: "0xrecipient",
        date: new Date(),
        description: "Pending payment",
        status: "pending",
      };

      transactions.set([pendingTx]);
      const txList = get(transactions);

      expect(txList[0].status).toBe("pending");
    });

    it("should handle completed transactions", () => {
      const completedTx: Transaction = {
        id: 1,
        type: "received",
        amount: 100,
        from: "0xsender",
        date: new Date(),
        description: "Completed reward",
        status: "completed",
      };

      transactions.set([completedTx]);
      const txList = get(transactions);

      expect(txList[0].status).toBe("completed");
    });

    it("should update transaction status from pending to completed", () => {
      transactions.set([
        {
          id: 1,
          type: "sent",
          amount: 10,
          to: "0xrecipient",
          date: new Date(),
          description: "Payment",
          status: "pending",
        },
      ]);

      // Update status
      transactions.update((txs) =>
        txs.map((tx) =>
          tx.id === 1 ? { ...tx, status: "completed" as const } : tx
        )
      );

      const txList = get(transactions);
      expect(txList[0].status).toBe("completed");
    });
  });

  describe("Transaction Filtering", () => {
    beforeEach(() => {
      const mixedTxs: Transaction[] = [
        {
          id: 1,
          type: "received",
          amount: 100,
          from: "0xabc",
          date: new Date("2024-01-01"),
          description: "Mining reward",
          status: "completed",
        },
        {
          id: 2,
          type: "sent",
          amount: 25,
          to: "0xdef",
          date: new Date("2024-01-02"),
          description: "Payment",
          status: "completed",
        },
        {
          id: 3,
          type: "received",
          amount: 50,
          from: "0xghi",
          date: new Date("2024-01-03"),
          description: "Storage reward",
          status: "completed",
        },
        {
          id: 4,
          type: "sent",
          amount: 10,
          to: "0xjkl",
          date: new Date("2024-01-04"),
          description: "Proxy payment",
          status: "pending",
        },
      ];
      transactions.set(mixedTxs);
    });

    it("should filter received transactions", () => {
      const txList = get(transactions);
      const received = txList.filter((tx) => tx.type === "received");

      expect(received).toHaveLength(2);
      expect(received.every((tx) => tx.type === "received")).toBe(true);
    });

    it("should filter sent transactions", () => {
      const txList = get(transactions);
      const sent = txList.filter((tx) => tx.type === "sent");

      expect(sent).toHaveLength(2);
      expect(sent.every((tx) => tx.type === "sent")).toBe(true);
    });

    it("should filter pending transactions", () => {
      const txList = get(transactions);
      const pending = txList.filter((tx) => tx.status === "pending");

      expect(pending).toHaveLength(1);
      expect(pending[0].id).toBe(4);
    });

    it("should filter completed transactions", () => {
      const txList = get(transactions);
      const completed = txList.filter((tx) => tx.status === "completed");

      expect(completed).toHaveLength(3);
    });
  });

  describe("Transaction Amounts", () => {
    it("should calculate total received amount", () => {
      const txs: Transaction[] = [
        {
          id: 1,
          type: "received",
          amount: 100,
          from: "0xabc",
          date: new Date(),
          description: "Reward 1",
          status: "completed",
        },
        {
          id: 2,
          type: "received",
          amount: 50,
          from: "0xdef",
          date: new Date(),
          description: "Reward 2",
          status: "completed",
        },
        {
          id: 3,
          type: "sent",
          amount: 25,
          to: "0xghi",
          date: new Date(),
          description: "Payment",
          status: "completed",
        },
      ];

      transactions.set(txs);
      const txList = get(transactions);
      const totalReceived = txList
        .filter((tx) => tx.type === "received")
        .reduce((sum, tx) => sum + tx.amount, 0);

      expect(totalReceived).toBe(150);
    });

    it("should calculate total sent amount using derived store", () => {
      const txs: Transaction[] = [
        {
          id: 1,
          type: "sent",
          amount: 10,
          to: "0xabc",
          date: new Date(),
          description: "Payment 1",
          status: "completed",
        },
        {
          id: 2,
          type: "sent",
          amount: 25,
          to: "0xdef",
          date: new Date(),
          description: "Payment 2",
          status: "completed",
        },
        {
          id: 3,
          type: "received",
          amount: 100,
          from: "0xghi",
          date: new Date(),
          description: "Reward",
          status: "completed",
        },
      ];

      transactions.set(txs);
      const spent = get(totalSpent);

      expect(spent).toBe(35);
    });
  });

  describe("Transaction Dates", () => {
    it("should maintain transaction date", () => {
      const testDate = new Date("2024-01-15T10:30:00");
      const tx: Transaction = {
        id: 1,
        type: "received",
        amount: 100,
        from: "0xabc",
        date: testDate,
        description: "Test",
        status: "completed",
      };

      transactions.set([tx]);
      const txList = get(transactions);

      expect(txList[0].date).toEqual(testDate);
    });

    it("should sort transactions by date", () => {
      const txs: Transaction[] = [
        {
          id: 1,
          type: "received",
          amount: 10,
          from: "0xabc",
          date: new Date("2024-01-03"),
          description: "Third",
          status: "completed",
        },
        {
          id: 2,
          type: "received",
          amount: 20,
          from: "0xdef",
          date: new Date("2024-01-01"),
          description: "First",
          status: "completed",
        },
        {
          id: 3,
          type: "received",
          amount: 30,
          from: "0xghi",
          date: new Date("2024-01-02"),
          description: "Second",
          status: "completed",
        },
      ];

      transactions.set(txs);
      const txList = get(transactions);
      const sorted = [...txList].sort(
        (a, b) => b.date.getTime() - a.date.getTime()
      );

      expect(sorted[0].description).toBe("Third");
      expect(sorted[1].description).toBe("Second");
      expect(sorted[2].description).toBe("First");
    });
  });

  describe("Wallet Integration", () => {
    it("should update wallet balance after transaction", () => {
      wallet.update((w) => ({
        ...w,
        balance: w.balance + 50,
      }));

      const walletState = get(wallet);
      expect(walletState.balance).toBe(1050);
    });

    it("should decrement balance for sent transactions", () => {
      const sendAmount = 100;
      wallet.update((w) => ({
        ...w,
        balance: w.balance - sendAmount,
      }));

      const walletState = get(wallet);
      expect(walletState.balance).toBe(900);
    });

    it("should track pending transactions count", () => {
      wallet.update((w) => ({
        ...w,
        pendingTransactions: 3,
      }));

      const walletState = get(wallet);
      expect(walletState.pendingTransactions).toBe(3);
    });
  });

  describe("Edge Cases", () => {
    it("should handle zero amount transactions", () => {
      const tx: Transaction = {
        id: 1,
        type: "received",
        amount: 0,
        from: "0xabc",
        date: new Date(),
        description: "Zero amount",
        status: "completed",
      };

      transactions.set([tx]);
      const txList = get(transactions);

      expect(txList[0].amount).toBe(0);
    });

    it("should handle large amounts", () => {
      const tx: Transaction = {
        id: 1,
        type: "received",
        amount: 999999999.99,
        from: "0xabc",
        date: new Date(),
        description: "Large amount",
        status: "completed",
      };

      transactions.set([tx]);
      const txList = get(transactions);

      expect(txList[0].amount).toBe(999999999.99);
    });

    it("should handle empty description", () => {
      const tx: Transaction = {
        id: 1,
        type: "received",
        amount: 10,
        from: "0xabc",
        date: new Date(),
        description: "",
        status: "completed",
      };

      transactions.set([tx]);
      const txList = get(transactions);

      expect(txList[0].description).toBe("");
    });

    it("should maintain transaction order when adding to beginning", () => {
      transactions.set([
        {
          id: 1,
          type: "received",
          amount: 10,
          from: "0xabc",
          date: new Date(),
          description: "First",
          status: "completed",
        },
      ]);

      transactions.update((txs) => [
        {
          id: 2,
          type: "received",
          amount: 20,
          from: "0xdef",
          date: new Date(),
          description: "Second",
          status: "completed",
        },
        ...txs,
      ]);

      const txList = get(transactions);
      expect(txList[0].id).toBe(2);
      expect(txList[1].id).toBe(1);
    });
  });
});
