import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import { signedFetch, signedJsonFetch } from '../src/lib/services/apiClient';

// Mock walletService used by apiClient
vi.mock('../src/lib/wallet', () => ({
  walletService: {
    isDesktopEnvironment: () => true,
    signApiRequest: async (_method: string, _path: string, _bytes: Uint8Array, opts?: any) => ({
      address: '0xabc',
      signature: 'sig',
      timestamp: opts?.timestamp || Math.floor(Date.now() / 1000)
    })
  }
}));

describe('apiClient', () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it('signedFetch attaches signature headers and forwards body', async () => {
    const mockFetch = vi.fn(async (_input: string, init?: any) => {
      return new Response(JSON.stringify({ ok: true }), { status: 200 });
    });
    (globalThis as any).fetch = mockFetch;

    const res = await signedFetch('https://example.com/api/test', { method: 'POST', body: { a: 1 } });
    expect(mockFetch).toHaveBeenCalled();
    const calledInit = mockFetch.mock.calls[0][1];
    expect(calledInit.headers.get('X-Wallet-Address')).toBe('0xabc');
    expect(calledInit.headers.get('X-Signature')).toBe('sig');
  });

  it('signedJsonFetch throws on non-2xx response with body text', async () => {
    const mockFetch = vi.fn(async () => {
      return new Response('error occurred', { status: 500 });
    });
    (globalThis as any).fetch = mockFetch;

    await expect(signedJsonFetch('https://example.com/bad', {})).rejects.toThrow('Request failed with status 500: error occurred');
  });
});
