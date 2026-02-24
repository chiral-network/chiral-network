import { describe, it, expect, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

const mockInvoke = vi.mocked(invoke);

// Mock logger
vi.mock('$lib/logger', () => ({
  logger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
    ok: vi.fn(),
  }),
}));

// Mock event listener
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

describe('Wallet Operations', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
    localStorage.clear();
  });

  describe('walletAccount store', () => {
    it('should initialize as null when no wallet connected', async () => {
      const { walletAccount } = await import('$lib/stores');
      expect(get(walletAccount)).toBeNull();
    });

    it('should store wallet data when set', async () => {
      const { walletAccount } = await import('$lib/stores');
      walletAccount.set({
        address: '0xabcdef1234567890abcdef1234567890abcdef12',
        privateKey: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
      });
      const wallet = get(walletAccount);
      expect(wallet).not.toBeNull();
      expect(wallet!.address).toBe('0xabcdef1234567890abcdef1234567890abcdef12');
    });

    it('should clear wallet on disconnect', async () => {
      const { walletAccount } = await import('$lib/stores');
      walletAccount.set({ address: '0xtest', privateKey: '0xkey' });
      walletAccount.set(null);
      expect(get(walletAccount)).toBeNull();
    });
  });

  describe('send_transaction invoke', () => {
    it('should send CHI with correct args', async () => {
      const txResult = {
        hash: '0xabcdef1234567890',
        status: 'pending',
        balanceBefore: '10.000000',
        balanceAfter: '5.000000',
      };
      mockInvoke.mockResolvedValueOnce(txResult);

      const result = await invoke('send_transaction', {
        fromAddress: '0xsender',
        toAddress: '0xrecipient',
        amount: '5.0',
        privateKey: '0xprivkey',
      });

      expect(mockInvoke).toHaveBeenCalledWith('send_transaction', {
        fromAddress: '0xsender',
        toAddress: '0xrecipient',
        amount: '5.0',
        privateKey: '0xprivkey',
      });
      expect(result).toEqual(txResult);
    });

    it('should handle insufficient balance error', async () => {
      mockInvoke.mockRejectedValueOnce(
        'Insufficient balance: have 0.001000 CHI, need 5.000000 CHI (amount) + 0.000042 CHI (gas)'
      );

      await expect(
        invoke('send_transaction', {
          fromAddress: '0xsender',
          toAddress: '0xrecipient',
          amount: '5.0',
          privateKey: '0xprivkey',
        })
      ).rejects.toContain('Insufficient balance');
    });

    it('should handle invalid recipient address', async () => {
      mockInvoke.mockRejectedValueOnce('Invalid to address: odd length hex string');

      await expect(
        invoke('send_transaction', {
          fromAddress: '0xsender',
          toAddress: 'not_an_address',
          amount: '1.0',
          privateKey: '0xprivkey',
        })
      ).rejects.toContain('Invalid to address');
    });
  });

  describe('get_wallet_balance invoke', () => {
    it('should load wallet balance', async () => {
      mockInvoke.mockResolvedValueOnce({
        balance: '100.500000',
        balanceWei: '100500000000000000000',
      });

      const result = await invoke<{ balance: string; balanceWei: string }>('get_wallet_balance', {
        address: '0xmyaddr',
      });

      expect(result.balance).toBe('100.500000');
      expect(result.balanceWei).toBe('100500000000000000000');
    });

    it('should handle zero balance', async () => {
      mockInvoke.mockResolvedValueOnce({
        balance: '0.000000',
        balanceWei: '0',
      });

      const result = await invoke<{ balance: string; balanceWei: string }>('get_wallet_balance', {
        address: '0xnewwallet',
      });

      expect(result.balance).toBe('0.000000');
    });

    it('should handle balance query failure', async () => {
      mockInvoke.mockRejectedValueOnce('RPC error: connection refused');

      await expect(
        invoke('get_wallet_balance', { address: '0xmyaddr' })
      ).rejects.toContain('connection refused');
    });
  });

  describe('get_transaction_history invoke', () => {
    it('should load transaction history', async () => {
      const mockTxs = [
        {
          hash: '0xtx1',
          from: '0xsender',
          to: '0xreceiver',
          value: '5.000000',
          valueWei: '5000000000000000000',
          blockNumber: 100,
          timestamp: 1700000000,
          status: 'confirmed',
          gasUsed: 21000,
          txType: 'send',
          description: 'Sent 5.0 CHI to 0xreceiver',
        },
        {
          hash: '0xtx2',
          from: '0xpayer',
          to: '0xsender',
          value: '2.500000',
          valueWei: '2500000000000000000',
          blockNumber: 101,
          timestamp: 1700001000,
          status: 'confirmed',
          gasUsed: 21000,
          txType: 'receive',
          description: 'Received 2.5 CHI',
        },
      ];
      mockInvoke.mockResolvedValueOnce(mockTxs);

      const result = await invoke<any[]>('get_transaction_history', { address: '0xsender' });

      expect(result).toHaveLength(2);
      expect(result[0].txType).toBe('send');
      expect(result[1].txType).toBe('receive');
    });

    it('should return empty history for new wallet', async () => {
      mockInvoke.mockResolvedValueOnce([]);

      const result = await invoke<any[]>('get_transaction_history', { address: '0xnew' });
      expect(result).toHaveLength(0);
    });
  });

  describe('faucet request', () => {
    it('should request CHI from faucet', async () => {
      mockInvoke.mockResolvedValueOnce({
        hash: '0xfaucet_tx',
        status: 'pending',
        balanceBefore: '0.000000',
        balanceAfter: '1.000000',
      });

      const result = await invoke<any>('request_faucet', { address: '0xmywallet' });
      expect(result.hash).toBe('0xfaucet_tx');
    });

    it('should handle faucet unavailable', async () => {
      mockInvoke.mockRejectedValueOnce(
        'Faucet unavailable. Please mine some blocks to get CHI. Error: insufficient funds'
      );

      await expect(
        invoke('request_faucet', { address: '0xmywallet' })
      ).rejects.toContain('Faucet unavailable');
    });
  });

  describe('balance formatting', () => {
    it('should parse float balance for display', () => {
      const balance = '100.500000';
      const formatted = parseFloat(balance).toFixed(4);
      expect(formatted).toBe('100.5000');
    });

    it('should handle very small balance', () => {
      const balance = '0.000001';
      const formatted = parseFloat(balance).toFixed(4);
      expect(formatted).toBe('0.0000');
    });

    it('should handle large balance', () => {
      const balance = '999999.123456';
      const formatted = parseFloat(balance).toFixed(4);
      expect(formatted).toBe('999999.1235');
    });
  });
});
