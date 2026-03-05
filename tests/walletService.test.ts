import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { ethers } from 'ethers';

const mockInvoke = vi.mocked(invoke);

describe('walletService', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    delete (window as any).__TAURI_INTERNALS__;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('getChainId', () => {
    it('returns default chain ID outside Tauri', async () => {
      const { getChainId } = await import('$lib/services/walletService');
      await expect(getChainId()).resolves.toBe(98765);
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it('fetches and caches chain ID in Tauri', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      mockInvoke.mockResolvedValueOnce(123456);

      const { getChainId } = await import('$lib/services/walletService');
      await expect(getChainId()).resolves.toBe(123456);
      await expect(getChainId()).resolves.toBe(123456);

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('get_chain_id');
    });

    it('falls back to default chain ID when backend fails', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      mockInvoke.mockRejectedValueOnce(new Error('backend down'));

      const { getChainId } = await import('$lib/services/walletService');
      await expect(getChainId()).resolves.toBe(98765);
    });
  });

  describe('walletService.getBalance', () => {
    it('returns zero for empty address', async () => {
      const { walletService } = await import('$lib/services/walletService');
      await expect(walletService.getBalance('')).resolves.toBe('0.00');
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it('uses cache for repeated calls with the same address', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      mockInvoke.mockResolvedValueOnce({ balance: '10.50', balanceWei: '10500000000000000000' });

      const { walletService } = await import('$lib/services/walletService');
      const first = await walletService.getBalance('0xAbC');
      const second = await walletService.getBalance('0xabc');

      expect(first).toBe('10.50');
      expect(second).toBe('10.50');
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('get_wallet_balance', { address: '0xAbC' });
    });

    it('refreshBalance bypasses cache', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      mockInvoke
        .mockResolvedValueOnce({ balance: '1.00', balanceWei: '1000000000000000000' })
        .mockResolvedValueOnce({ balance: '2.00', balanceWei: '2000000000000000000' });

      const { walletService } = await import('$lib/services/walletService');
      await expect(walletService.getBalance('0xme')).resolves.toBe('1.00');
      await expect(walletService.refreshBalance('0xme')).resolves.toBe('2.00');
      expect(mockInvoke).toHaveBeenCalledTimes(2);
    });

    it('returns cached value when backend refresh fails after cache expires', async () => {
      (window as any).__TAURI_INTERNALS__ = {};
      const nowSpy = vi.spyOn(Date, 'now');
      let now = 0;
      nowSpy.mockImplementation(() => now);

      mockInvoke
        .mockResolvedValueOnce({ balance: '3.14', balanceWei: '3140000000000000000' })
        .mockRejectedValueOnce(new Error('RPC timeout'));

      const { walletService } = await import('$lib/services/walletService');
      await expect(walletService.getBalance('0xme')).resolves.toBe('3.14');
      now = 31000; // 31s > cache TTL
      await expect(walletService.getBalance('0xme')).resolves.toBe('3.14');
      expect(mockInvoke).toHaveBeenCalledTimes(2);
    });
  });

  describe('signTransaction', () => {
    it('throws when no wallet is available', async () => {
      const { walletAccount } = await import('$lib/stores');
      walletAccount.set(null);

      const { signTransaction } = await import('$lib/services/walletService');
      await expect(
        signTransaction({
          from: '0x0',
          to: '0x1111111111111111111111111111111111111111',
          value: '0.01',
          gasLimit: 21000,
          gasPrice: 1_000_000_000,
        }),
      ).rejects.toThrow('No wallet available for signing');
    });

    it('signs a transaction with expected values', async () => {
      const { walletAccount } = await import('$lib/stores');
      walletAccount.set({
        address: '0x70997970C51812dc3A010C7d01b50e0d17dc79C8',
        privateKey: '0x59c6995e998f97a5a0044966f0945388cf9f6f5b7f4cbce8f7d2a7bc01db57ee',
      });

      const { signTransaction } = await import('$lib/services/walletService');
      const signed = await signTransaction({
        from: '0x70997970C51812dc3A010C7d01b50e0d17dc79C8',
        to: '0x1111111111111111111111111111111111111111',
        value: '0.01',
        gasLimit: 21000,
        gasPrice: 1_000_000_000,
        nonce: 7,
      });

      const tx = ethers.Transaction.from(signed);
      expect(tx.to?.toLowerCase()).toBe('0x1111111111111111111111111111111111111111');
      expect(tx.value).toBe(ethers.parseEther('0.01'));
      expect(tx.gasLimit).toBe(21000n);
      expect(tx.gasPrice).toBe(1_000_000_000n);
      expect(tx.nonce).toBe(7);
      expect(tx.chainId).toBe(98765n);
    });
  });

  describe('address and ether helpers', () => {
    it('validates addresses correctly', async () => {
      const { isValidAddress } = await import('$lib/services/walletService');
      expect(isValidAddress('0x1111111111111111111111111111111111111111')).toBe(true);
      expect(isValidAddress('0x1234')).toBe(false);
    });

    it('parses and formats ether values', async () => {
      const { parseEther, formatEther } = await import('$lib/services/walletService');
      const wei = parseEther('1.5');
      expect(wei).toBe('1500000000000000000');
      expect(formatEther(wei)).toBe('1.5');
    });
  });
});
