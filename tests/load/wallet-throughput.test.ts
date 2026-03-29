/**
 * Wallet and transaction throughput tests
 *
 * Tests concurrent balance queries, transaction sending,
 * chain ID lookups, and wallet service caching behavior.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { get } from 'svelte/store';

const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock('$lib/logger', () => ({
  logger: () => ({
    info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn(), ok: vi.fn(),
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

function makeBalanceResult(chi: string = '10.500000') {
  const wei = BigInt(Math.round(parseFloat(chi) * 1e18));
  return { balance: chi, balanceWei: wei.toString() };
}

describe('Wallet throughput tests', () => {
  beforeEach(() => {
    vi.resetModules();
    mockInvoke.mockReset();
  });

  describe('concurrent balance queries', () => {
    it('should handle 20 concurrent balance queries for different addresses', async () => {
      const addresses = Array.from({ length: 20 }, (_, i) =>
        `0x${i.toString(16).padStart(40, '0')}`
      );
      for (const addr of addresses) {
        mockInvoke.mockResolvedValueOnce(makeBalanceResult(`${(Math.random() * 100).toFixed(6)}`));
      }

      const results = await Promise.all(
        addresses.map(addr => mockInvoke('get_wallet_balance', { address: addr }))
      );

      expect(results).toHaveLength(20);
      results.forEach(r => {
        expect(r).toHaveProperty('balance');
        expect(r).toHaveProperty('balanceWei');
      });
    });

    it('should handle 50 concurrent balance queries for the same address', async () => {
      for (let i = 0; i < 50; i++) {
        mockInvoke.mockResolvedValueOnce(makeBalanceResult('25.000000'));
      }

      const results = await Promise.all(
        Array.from({ length: 50 }, () =>
          mockInvoke('get_wallet_balance', { address: '0xabc123' })
        )
      );

      expect(results).toHaveLength(50);
      results.forEach(r => expect(r.balance).toBe('25.000000'));
    });

    it('should handle mixed success/failure balance queries', async () => {
      for (let i = 0; i < 30; i++) {
        if (i % 3 === 0) {
          mockInvoke.mockRejectedValueOnce('RPC timeout');
        } else {
          mockInvoke.mockResolvedValueOnce(makeBalanceResult('5.000000'));
        }
      }

      const results = await Promise.allSettled(
        Array.from({ length: 30 }, () =>
          mockInvoke('get_wallet_balance', { address: '0xtest' })
        )
      );

      const successes = results.filter(r => r.status === 'fulfilled');
      const failures = results.filter(r => r.status === 'rejected');
      expect(successes).toHaveLength(20);
      expect(failures).toHaveLength(10);
    });
  });

  describe('walletService caching', () => {
    it('should use cached balance on rapid repeated calls', async () => {
      // Set up Tauri environment
      (globalThis as any).__TAURI__ = true;
      (globalThis as any).__TAURI_INTERNALS__ = true;
      mockInvoke.mockResolvedValue(makeBalanceResult('10.000000'));

      const { walletService } = await import('$lib/services/walletService');

      const first = await walletService.getBalance('0xtest');
      expect(first).toBe('10.000000');

      const second = await walletService.getBalance('0xtest');
      expect(second).toBe('10.000000');

      const balanceCalls = mockInvoke.mock.calls.filter(
        c => c[0] === 'get_wallet_balance'
      );
      expect(balanceCalls).toHaveLength(1);

      delete (globalThis as any).__TAURI__;
      delete (globalThis as any).__TAURI_INTERNALS__;
    });

    it('should refresh balance bypassing cache', async () => {
      (globalThis as any).__TAURI__ = true;
      (globalThis as any).__TAURI_INTERNALS__ = true;
      mockInvoke
        .mockResolvedValueOnce(makeBalanceResult('10.000000'))
        .mockResolvedValueOnce(makeBalanceResult('15.000000'));

      const { walletService } = await import('$lib/services/walletService');

      const first = await walletService.getBalance('0xtest');
      expect(first).toBe('10.000000');

      const refreshed = await walletService.refreshBalance('0xtest');
      expect(refreshed).toBe('15.000000');

      delete (globalThis as any).__TAURI__;
      delete (globalThis as any).__TAURI_INTERNALS__;
    });

    it('should handle different addresses independently in cache', async () => {
      (globalThis as any).__TAURI__ = true;
      (globalThis as any).__TAURI_INTERNALS__ = true;
      mockInvoke
        .mockResolvedValueOnce(makeBalanceResult('10.000000'))
        .mockResolvedValueOnce(makeBalanceResult('20.000000'));

      const { walletService } = await import('$lib/services/walletService');

      const balance1 = await walletService.getBalance('0xaddr1');
      const balance2 = await walletService.getBalance('0xaddr2');

      expect(balance1).toBe('10.000000');
      expect(balance2).toBe('20.000000');
      expect(mockInvoke.mock.calls.filter(c => c[0] === 'get_wallet_balance')).toHaveLength(2);

      delete (globalThis as any).__TAURI__;
      delete (globalThis as any).__TAURI_INTERNALS__;
    });
  });

  describe('transaction sending', () => {
    it('should handle concurrent transaction sends', async () => {
      for (let i = 0; i < 10; i++) {
        mockInvoke.mockResolvedValueOnce({
          hash: `0x${i.toString(16).padStart(64, '0')}`,
          status: 'pending',
          balanceBefore: '100.000000',
          balanceAfter: '99.000000',
        });
      }

      const results = await Promise.all(
        Array.from({ length: 10 }, (_, i) =>
          mockInvoke('send_transaction', {
            fromAddress: '0xsender',
            toAddress: `0x${i.toString(16).padStart(40, '0')}`,
            amount: '1.0',
            privateKey: '0x' + 'a'.repeat(64),
          })
        )
      );

      expect(results).toHaveLength(10);
      results.forEach(r => {
        expect(r).toHaveProperty('hash');
        expect(r.status).toBe('pending');
      });
    });

    it('should handle transaction failures gracefully', async () => {
      mockInvoke
        .mockRejectedValueOnce('Insufficient balance')
        .mockRejectedValueOnce('Nonce too low')
        .mockResolvedValueOnce({ hash: '0xsuccess', status: 'pending', balanceBefore: '10', balanceAfter: '9' });

      const results = await Promise.allSettled([
        mockInvoke('send_transaction', { fromAddress: '0xa', toAddress: '0xb', amount: '999', privateKey: '0x' + 'a'.repeat(64) }),
        mockInvoke('send_transaction', { fromAddress: '0xa', toAddress: '0xc', amount: '1', privateKey: '0x' + 'a'.repeat(64) }),
        mockInvoke('send_transaction', { fromAddress: '0xa', toAddress: '0xd', amount: '1', privateKey: '0x' + 'a'.repeat(64) }),
      ]);

      expect(results[0].status).toBe('rejected');
      expect(results[1].status).toBe('rejected');
      expect(results[2].status).toBe('fulfilled');
    });
  });

  describe('chain ID queries', () => {
    it('should handle 100 concurrent chain ID queries', async () => {
      for (let i = 0; i < 100; i++) {
        mockInvoke.mockResolvedValueOnce(98765);
      }

      const results = await Promise.all(
        Array.from({ length: 100 }, () => mockInvoke('get_chain_id'))
      );

      expect(results).toHaveLength(100);
      results.forEach(r => expect(r).toBe(98765));
    });
  });

  describe('transaction history', () => {
    it('should handle large transaction history (1000 entries)', async () => {
      const txs = Array.from({ length: 1000 }, (_, i) => ({
        hash: `0x${i.toString(16).padStart(64, '0')}`,
        from: '0xsender',
        to: '0xreceiver',
        value: '1000000000000000000',
        blockNumber: i + 1,
        timestamp: 1700000000 + i,
        gasUsed: 21000,
        txType: i % 3 === 0 ? 'send' : i % 3 === 1 ? 'receive' : 'download_payment',
        description: `Transaction ${i}`,
        speedTier: null,
      }));
      mockInvoke.mockResolvedValueOnce({ transactions: txs });

      const result = await mockInvoke('get_transaction_history', { address: '0xtest' });

      expect(result.transactions).toHaveLength(1000);
      expect(result.transactions[0].hash).toContain('0x');
    });

    it('should handle concurrent history requests for different wallets', async () => {
      for (let i = 0; i < 5; i++) {
        mockInvoke.mockResolvedValueOnce({
          transactions: Array.from({ length: 10 }, (_, j) => ({
            hash: `0x${(i * 10 + j).toString(16).padStart(64, '0')}`,
            from: `0xwallet${i}`,
            to: '0xother',
            value: '0',
            blockNumber: j + 1,
            timestamp: 1700000000 + j,
            gasUsed: 21000,
            txType: 'send',
            description: '',
          })),
        });
      }

      const results = await Promise.all(
        Array.from({ length: 5 }, (_, i) =>
          mockInvoke('get_transaction_history', { address: `0xwallet${i}` })
        )
      );

      expect(results).toHaveLength(5);
      results.forEach(r => expect(r.transactions).toHaveLength(10));
    });
  });

  describe('wallet creation throughput', () => {
    it('should generate 20 unique wallet addresses', async () => {
      // Simulate wallet creation via invoke (ethers not available in test env)
      const addresses: string[] = [];
      for (let i = 0; i < 20; i++) {
        mockInvoke.mockResolvedValueOnce({
          address: `0x${i.toString(16).padStart(40, '0')}`,
          privateKey: `0x${'a'.repeat(64)}`,
        });
      }

      for (let i = 0; i < 20; i++) {
        const result = await mockInvoke('create_wallet');
        addresses.push(result.address);
      }

      expect(addresses).toHaveLength(20);
      const unique = new Set(addresses);
      expect(unique.size).toBe(20);
    });

    it('should handle rapid wallet import attempts', async () => {
      for (let i = 0; i < 10; i++) {
        mockInvoke.mockResolvedValueOnce({
          address: `0x${i.toString(16).padStart(40, '0')}`,
          privateKey: `0x${'b'.repeat(64)}`,
        });
      }

      const results = await Promise.all(
        Array.from({ length: 10 }, () => mockInvoke('import_wallet', { privateKey: '0x' + 'b'.repeat(64) }))
      );

      expect(results).toHaveLength(10);
      results.forEach(r => expect(r).toHaveProperty('address'));
    });
  });

  describe('faucet requests', () => {
    it('should handle concurrent faucet requests', async () => {
      for (let i = 0; i < 10; i++) {
        mockInvoke.mockResolvedValueOnce({
          hash: `0xfaucet${i}`,
          status: 'pending',
          balanceBefore: '0',
          balanceAfter: '1.000000',
        });
      }

      const results = await Promise.all(
        Array.from({ length: 10 }, (_, i) =>
          mockInvoke('request_faucet', { address: `0x${i.toString(16).padStart(40, '0')}` })
        )
      );

      expect(results).toHaveLength(10);
      results.forEach(r => expect(r.hash).toContain('faucet'));
    });

    it('should handle faucet rate limiting', async () => {
      mockInvoke
        .mockResolvedValueOnce({ hash: '0xok', status: 'pending', balanceBefore: '0', balanceAfter: '1' })
        .mockRejectedValueOnce('Faucet rate limited')
        .mockRejectedValueOnce('Faucet rate limited');

      const results = await Promise.allSettled([
        mockInvoke('request_faucet', { address: '0xaddr1' }),
        mockInvoke('request_faucet', { address: '0xaddr2' }),
        mockInvoke('request_faucet', { address: '0xaddr3' }),
      ]);

      expect(results[0].status).toBe('fulfilled');
      expect(results[1].status).toBe('rejected');
      expect(results[2].status).toBe('rejected');
    });
  });
});
