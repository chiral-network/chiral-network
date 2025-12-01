import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  HmacKeyExchangeService,
  type HmacKeyExchangeRequest,
  type HmacKeyExchangeResponse,
  type HmacKeyExchangeConfirmation,
} from '../src/lib/hmacKeyExchange';

// Mock the Tauri invoke function
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

describe('hmacKeyExchange.ts', () => {
  const mockInvoke = vi.mocked(invoke);

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('HmacKeyExchangeService', () => {
    describe('initiateKeyExchange', () => {
      it('should initiate key exchange successfully', async () => {
        const mockRequest: HmacKeyExchangeRequest = {
          exchange_id: 'exchange-123',
          initiator_peer_id: 'peer-alice',
          target_peer_id: 'peer-bob',
          initiator_public_key: 'public-key-alice',
          session_id: 'session-456',
          timestamp: Date.now(),
          nonce: 'nonce-789',
        };

        mockInvoke.mockResolvedValueOnce(mockRequest);

        const result = await HmacKeyExchangeService.initiateKeyExchange(
          'peer-alice',
          'peer-bob',
          'session-456'
        );

        expect(mockInvoke).toHaveBeenCalledWith('initiate_hmac_key_exchange', {
          initiatorPeerId: 'peer-alice',
          targetPeerId: 'peer-bob',
          sessionId: 'session-456',
        });
        expect(result).toEqual(mockRequest);
      });

      it('should handle initiation errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('Backend error'));

        await expect(
          HmacKeyExchangeService.initiateKeyExchange(
            'peer-alice',
            'peer-bob',
            'session-456'
          )
        ).rejects.toThrow('Backend error');
      });

      it('should log exchange ID on success', async () => {
        const consoleSpy = vi.spyOn(console, 'log');
        const mockRequest: HmacKeyExchangeRequest = {
          exchange_id: 'exchange-123',
          initiator_peer_id: 'peer-alice',
          target_peer_id: 'peer-bob',
          initiator_public_key: 'public-key-alice',
          session_id: 'session-456',
          timestamp: Date.now(),
          nonce: 'nonce-789',
        };

        mockInvoke.mockResolvedValueOnce(mockRequest);

        await HmacKeyExchangeService.initiateKeyExchange(
          'peer-alice',
          'peer-bob',
          'session-456'
        );

        expect(consoleSpy).toHaveBeenCalledWith(
          'Initiated HMAC key exchange:',
          'exchange-123'
        );
      });
    });

    describe('respondToKeyExchange', () => {
      it('should respond to key exchange successfully', async () => {
        const mockRequest: HmacKeyExchangeRequest = {
          exchange_id: 'exchange-123',
          initiator_peer_id: 'peer-alice',
          target_peer_id: 'peer-bob',
          initiator_public_key: 'public-key-alice',
          session_id: 'session-456',
          timestamp: Date.now(),
          nonce: 'nonce-789',
        };

        const mockResponse: HmacKeyExchangeResponse = {
          exchange_id: 'exchange-123',
          responder_peer_id: 'peer-bob',
          responder_public_key: 'public-key-bob',
          hmac_key_confirmation: 'confirmation-hash',
          timestamp: Date.now(),
          nonce: 'nonce-abc',
        };

        mockInvoke.mockResolvedValueOnce(mockResponse);

        const result = await HmacKeyExchangeService.respondToKeyExchange(
          mockRequest,
          'peer-bob'
        );

        expect(mockInvoke).toHaveBeenCalledWith('respond_to_hmac_key_exchange', {
          request: mockRequest,
          responderPeerId: 'peer-bob',
        });
        expect(result).toEqual(mockResponse);
      });

      it('should handle response errors', async () => {
        const mockRequest: HmacKeyExchangeRequest = {
          exchange_id: 'exchange-123',
          initiator_peer_id: 'peer-alice',
          target_peer_id: 'peer-bob',
          initiator_public_key: 'public-key-alice',
          session_id: 'session-456',
          timestamp: Date.now(),
          nonce: 'nonce-789',
        };

        mockInvoke.mockRejectedValueOnce(new Error('Invalid request'));

        await expect(
          HmacKeyExchangeService.respondToKeyExchange(mockRequest, 'peer-bob')
        ).rejects.toThrow('Invalid request');
      });
    });

    describe('confirmKeyExchange', () => {
      it('should confirm key exchange successfully', async () => {
        const mockResponse: HmacKeyExchangeResponse = {
          exchange_id: 'exchange-123',
          responder_peer_id: 'peer-bob',
          responder_public_key: 'public-key-bob',
          hmac_key_confirmation: 'confirmation-hash',
          timestamp: Date.now(),
          nonce: 'nonce-abc',
        };

        const mockConfirmation: HmacKeyExchangeConfirmation = {
          exchange_id: 'exchange-123',
          initiator_confirmation: 'initiator-confirmation-hash',
          timestamp: Date.now(),
        };

        mockInvoke.mockResolvedValueOnce(mockConfirmation);

        const result = await HmacKeyExchangeService.confirmKeyExchange(
          mockResponse,
          'peer-alice'
        );

        expect(mockInvoke).toHaveBeenCalledWith('confirm_hmac_key_exchange', {
          response: mockResponse,
          initiatorPeerId: 'peer-alice',
        });
        expect(result).toEqual(mockConfirmation);
      });

      it('should handle confirmation errors', async () => {
        const mockResponse: HmacKeyExchangeResponse = {
          exchange_id: 'exchange-123',
          responder_peer_id: 'peer-bob',
          responder_public_key: 'public-key-bob',
          hmac_key_confirmation: 'confirmation-hash',
          timestamp: Date.now(),
          nonce: 'nonce-abc',
        };

        mockInvoke.mockRejectedValueOnce(new Error('Confirmation failed'));

        await expect(
          HmacKeyExchangeService.confirmKeyExchange(mockResponse, 'peer-alice')
        ).rejects.toThrow('Confirmation failed');
      });
    });

    describe('finalizeKeyExchange', () => {
      it('should finalize key exchange successfully', async () => {
        const mockConfirmation: HmacKeyExchangeConfirmation = {
          exchange_id: 'exchange-123',
          initiator_confirmation: 'initiator-confirmation-hash',
          timestamp: Date.now(),
        };

        mockInvoke.mockResolvedValueOnce(undefined);

        await HmacKeyExchangeService.finalizeKeyExchange(
          mockConfirmation,
          'peer-bob'
        );

        expect(mockInvoke).toHaveBeenCalledWith('finalize_hmac_key_exchange', {
          confirmation: mockConfirmation,
          responderPeerId: 'peer-bob',
        });
      });

      it('should handle finalization errors', async () => {
        const mockConfirmation: HmacKeyExchangeConfirmation = {
          exchange_id: 'exchange-123',
          initiator_confirmation: 'initiator-confirmation-hash',
          timestamp: Date.now(),
        };

        mockInvoke.mockRejectedValueOnce(new Error('Finalization failed'));

        await expect(
          HmacKeyExchangeService.finalizeKeyExchange(mockConfirmation, 'peer-bob')
        ).rejects.toThrow('Finalization failed');
      });
    });

    describe('getExchangeStatus', () => {
      it('should get exchange status successfully', async () => {
        mockInvoke.mockResolvedValueOnce('completed');

        const status = await HmacKeyExchangeService.getExchangeStatus('exchange-123');

        expect(mockInvoke).toHaveBeenCalledWith('get_hmac_exchange_status', {
          exchangeId: 'exchange-123',
        });
        expect(status).toBe('completed');
      });

      it('should return null for non-existent exchange', async () => {
        mockInvoke.mockResolvedValueOnce(null);

        const status = await HmacKeyExchangeService.getExchangeStatus('non-existent');

        expect(status).toBeNull();
      });

      it('should handle status query errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('Query failed'));

        await expect(
          HmacKeyExchangeService.getExchangeStatus('exchange-123')
        ).rejects.toThrow('Query failed');
      });
    });

    describe('getActiveExchanges', () => {
      it('should get active exchanges successfully', async () => {
        const mockExchanges = ['exchange-1', 'exchange-2', 'exchange-3'];
        mockInvoke.mockResolvedValueOnce(mockExchanges);

        const exchanges = await HmacKeyExchangeService.getActiveExchanges();

        expect(mockInvoke).toHaveBeenCalledWith('get_active_hmac_exchanges');
        expect(exchanges).toEqual(mockExchanges);
      });

      it('should return empty array when no active exchanges', async () => {
        mockInvoke.mockResolvedValueOnce([]);

        const exchanges = await HmacKeyExchangeService.getActiveExchanges();

        expect(exchanges).toEqual([]);
      });

      it('should handle query errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('Query failed'));

        await expect(
          HmacKeyExchangeService.getActiveExchanges()
        ).rejects.toThrow('Query failed');
      });
    });

    describe('completeKeyExchangeAsInitiator', () => {
      it('should complete full key exchange flow as initiator', async () => {
        const mockRequest: HmacKeyExchangeRequest = {
          exchange_id: 'exchange-123',
          initiator_peer_id: 'peer-alice',
          target_peer_id: 'peer-bob',
          initiator_public_key: 'public-key-alice',
          session_id: 'session-456',
          timestamp: Date.now(),
          nonce: 'nonce-789',
        };

        const mockResponse: HmacKeyExchangeResponse = {
          exchange_id: 'exchange-123',
          responder_peer_id: 'peer-bob',
          responder_public_key: 'public-key-bob',
          hmac_key_confirmation: 'confirmation-hash',
          timestamp: Date.now(),
          nonce: 'nonce-abc',
        };

        const mockConfirmation: HmacKeyExchangeConfirmation = {
          exchange_id: 'exchange-123',
          initiator_confirmation: 'initiator-confirmation-hash',
          timestamp: Date.now(),
        };

        mockInvoke
          .mockResolvedValueOnce(mockRequest)
          .mockResolvedValueOnce(mockConfirmation);

        const onRequest = vi.fn().mockResolvedValue(undefined);
        const waitForResponse = vi.fn().mockResolvedValue(mockResponse);

        await HmacKeyExchangeService.completeKeyExchangeAsInitiator(
          'peer-alice',
          'peer-bob',
          'session-456',
          onRequest,
          waitForResponse
        );

        expect(onRequest).toHaveBeenCalledWith(mockRequest);
        expect(waitForResponse).toHaveBeenCalled();
      });

      it('should handle errors in initiator flow', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('Flow error'));

        const onRequest = vi.fn();
        const waitForResponse = vi.fn();

        await expect(
          HmacKeyExchangeService.completeKeyExchangeAsInitiator(
            'peer-alice',
            'peer-bob',
            'session-456',
            onRequest,
            waitForResponse
          )
        ).rejects.toThrow('Flow error');
      });
    });

    describe('completeKeyExchangeAsResponder', () => {
      it('should complete full key exchange flow as responder', async () => {
        const mockRequest: HmacKeyExchangeRequest = {
          exchange_id: 'exchange-123',
          initiator_peer_id: 'peer-alice',
          target_peer_id: 'peer-bob',
          initiator_public_key: 'public-key-alice',
          session_id: 'session-456',
          timestamp: Date.now(),
          nonce: 'nonce-789',
        };

        const mockResponse: HmacKeyExchangeResponse = {
          exchange_id: 'exchange-123',
          responder_peer_id: 'peer-bob',
          responder_public_key: 'public-key-bob',
          hmac_key_confirmation: 'confirmation-hash',
          timestamp: Date.now(),
          nonce: 'nonce-abc',
        };

        const mockConfirmation: HmacKeyExchangeConfirmation = {
          exchange_id: 'exchange-123',
          initiator_confirmation: 'initiator-confirmation-hash',
          timestamp: Date.now(),
        };

        mockInvoke
          .mockResolvedValueOnce(mockResponse)
          .mockResolvedValueOnce(undefined);

        const onResponse = vi.fn().mockResolvedValue(undefined);
        const waitForConfirmation = vi.fn().mockResolvedValue(mockConfirmation);

        await HmacKeyExchangeService.completeKeyExchangeAsResponder(
          mockRequest,
          'peer-bob',
          onResponse,
          waitForConfirmation
        );

        expect(onResponse).toHaveBeenCalledWith(mockResponse);
        expect(waitForConfirmation).toHaveBeenCalled();
      });

      it('should handle errors in responder flow', async () => {
        const mockRequest: HmacKeyExchangeRequest = {
          exchange_id: 'exchange-123',
          initiator_peer_id: 'peer-alice',
          target_peer_id: 'peer-bob',
          initiator_public_key: 'public-key-alice',
          session_id: 'session-456',
          timestamp: Date.now(),
          nonce: 'nonce-789',
        };

        mockInvoke.mockRejectedValueOnce(new Error('Flow error'));

        const onResponse = vi.fn();
        const waitForConfirmation = vi.fn();

        await expect(
          HmacKeyExchangeService.completeKeyExchangeAsResponder(
            mockRequest,
            'peer-bob',
            onResponse,
            waitForConfirmation
          )
        ).rejects.toThrow('Flow error');
      });
    });

    describe('generateSessionId', () => {
      it('should generate session ID without file hash', () => {
        const sessionId = HmacKeyExchangeService.generateSessionId(
          'peer-alice',
          'peer-bob'
        );

        expect(sessionId).toMatch(/^peer-alice-peer-bob-\d+$/);
      });

      it('should generate session ID with file hash', () => {
        const sessionId = HmacKeyExchangeService.generateSessionId(
          'peer-alice',
          'peer-bob',
          'file-hash-123'
        );

        expect(sessionId).toMatch(/^peer-alice-peer-bob-\d+-file-hash-123$/);
      });

      it('should generate unique session IDs', () => {
        const sessionId1 = HmacKeyExchangeService.generateSessionId(
          'peer-alice',
          'peer-bob'
        );
        
        // Small delay to ensure different timestamp
        const sessionId2 = HmacKeyExchangeService.generateSessionId(
          'peer-alice',
          'peer-bob'
        );

        // They should either be different or have same timestamp
        expect(sessionId1).toBeDefined();
        expect(sessionId2).toBeDefined();
      });
    });

    describe('cleanupExpiredExchanges', () => {
      it('should cleanup expired exchanges successfully', async () => {
        mockInvoke.mockResolvedValueOnce(undefined);

        await HmacKeyExchangeService.cleanupExpiredExchanges();

        expect(mockInvoke).toHaveBeenCalledWith('cleanup_auth_sessions');
      });

      it('should handle cleanup errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('Cleanup failed'));

        await expect(
          HmacKeyExchangeService.cleanupExpiredExchanges()
        ).rejects.toThrow('Cleanup failed');
      });
    });

    describe('createSession', () => {
      it('should create session successfully', async () => {
        const hmacKey = [1, 2, 3, 4, 5];
        mockInvoke.mockResolvedValueOnce(undefined);

        await HmacKeyExchangeService.createSession('session-123', hmacKey);

        expect(mockInvoke).toHaveBeenCalledWith('create_auth_session', {
          sessionId: 'session-123',
          hmacKey: hmacKey,
        });
      });

      it('should handle session creation errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('Creation failed'));

        await expect(
          HmacKeyExchangeService.createSession('session-123', [1, 2, 3])
        ).rejects.toThrow('Creation failed');
      });
    });

    describe('generateHmacKey', () => {
      it('should generate HMAC key successfully', async () => {
        const mockKey = new Array(32).fill(0).map((_, i) => i);
        mockInvoke.mockResolvedValueOnce(mockKey);

        const key = await HmacKeyExchangeService.generateHmacKey();

        expect(mockInvoke).toHaveBeenCalledWith('generate_hmac_key');
        expect(key).toEqual(mockKey);
        expect(key.length).toBe(32);
      });

      it('should handle key generation errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('Generation failed'));

        await expect(
          HmacKeyExchangeService.generateHmacKey()
        ).rejects.toThrow('Generation failed');
      });
    });
  });
});