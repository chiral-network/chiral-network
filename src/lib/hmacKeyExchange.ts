import { invoke } from '@tauri-apps/api/core';

/**
 * HMAC key exchange request structure
 */
export interface HmacKeyExchangeRequest {
  exchange_id: string;
  initiator_peer_id: string;
  target_peer_id: string;
  initiator_public_key: string;
  session_id: string;
  timestamp: number;
  nonce: string;
}

/**
 * HMAC key exchange response structure
 */
export interface HmacKeyExchangeResponse {
  exchange_id: string;
  responder_peer_id: string;
  responder_public_key: string;
  hmac_key_confirmation: string;
  timestamp: number;
  nonce: string;
}

/**
 * HMAC key exchange confirmation structure
 */
export interface HmacKeyExchangeConfirmation {
  exchange_id: string;
  initiator_confirmation: string;
  timestamp: number;
}

/**
 * HMAC key exchange service for frontend
 * Provides secure key exchange protocol between peers
 */
export class HmacKeyExchangeService {
  /**
   * Initiate HMAC key exchange with a peer
   * @param initiatorPeerId - Initiator's peer ID
   * @param targetPeerId - Target peer ID
   * @param sessionId - Session ID for the transfer
   * @returns Promise<HmacKeyExchangeRequest> - Key exchange request
   */
  static async initiateKeyExchange(
    initiatorPeerId: string,
    targetPeerId: string,
    sessionId: string
  ): Promise<HmacKeyExchangeRequest> {
    try {
      const request = await invoke<HmacKeyExchangeRequest>('initiate_hmac_key_exchange', {
        initiatorPeerId,
        targetPeerId,
        sessionId,
      });
      console.log('Initiated HMAC key exchange:', request.exchange_id);
      return request;
    } catch (error) {
      console.error('Failed to initiate key exchange:', error);
      throw error;
    }
  }

  /**
   * Respond to HMAC key exchange request
   * @param request - Key exchange request from initiator
   * @param responderPeerId - Responder's peer ID
   * @returns Promise<HmacKeyExchangeResponse> - Key exchange response
   */
  static async respondToKeyExchange(
    request: HmacKeyExchangeRequest,
    responderPeerId: string
  ): Promise<HmacKeyExchangeResponse> {
    try {
      const response = await invoke<HmacKeyExchangeResponse>('respond_to_hmac_key_exchange', {
        request,
        responderPeerId,
      });
      console.log('Responded to HMAC key exchange:', response.exchange_id);
      return response;
    } catch (error) {
      console.error('Failed to respond to key exchange:', error);
      throw error;
    }
  }

  /**
   * Confirm HMAC key exchange completion
   * @param response - Key exchange response from responder
   * @param initiatorPeerId - Initiator's peer ID
   * @returns Promise<HmacKeyExchangeConfirmation> - Key exchange confirmation
   */
  static async confirmKeyExchange(
    response: HmacKeyExchangeResponse,
    initiatorPeerId: string
  ): Promise<HmacKeyExchangeConfirmation> {
    try {
      const confirmation = await invoke<HmacKeyExchangeConfirmation>('confirm_hmac_key_exchange', {
        response,
        initiatorPeerId,
      });
      console.log('Confirmed HMAC key exchange:', confirmation.exchange_id);
      return confirmation;
    } catch (error) {
      console.error('Failed to confirm key exchange:', error);
      throw error;
    }
  }

  /**
   * Finalize key exchange on responder side
   * @param confirmation - Key exchange confirmation from initiator
   * @param responderPeerId - Responder's peer ID
   * @returns Promise<void>
   */
  static async finalizeKeyExchange(
    confirmation: HmacKeyExchangeConfirmation,
    responderPeerId: string
  ): Promise<void> {
    try {
      await invoke('finalize_hmac_key_exchange', {
        confirmation,
        responderPeerId,
      });
      console.log('Finalized HMAC key exchange:', confirmation.exchange_id);
    } catch (error) {
      console.error('Failed to finalize key exchange:', error);
      throw error;
    }
  }

  /**
   * Get the status of a key exchange
   * @param exchangeId - Exchange ID
   * @returns Promise<string | null> - Exchange status or null if not found
   */
  static async getExchangeStatus(exchangeId: string): Promise<string | null> {
    try {
      return await invoke<string | null>('get_hmac_exchange_status', {
        exchangeId,
      });
    } catch (error) {
      console.error('Failed to get exchange status:', error);
      throw error;
    }
  }

  /**
   * Get all active key exchanges
   * @returns Promise<string[]> - List of active exchange IDs
   */
  static async getActiveExchanges(): Promise<string[]> {
    try {
      return await invoke<string[]>('get_active_hmac_exchanges');
    } catch (error) {
      console.error('Failed to get active exchanges:', error);
      throw error;
    }
  }

  /**
   * Complete key exchange flow (initiator side)
   * @param initiatorPeerId - Initiator's peer ID
   * @param targetPeerId - Target peer ID
   * @param sessionId - Session ID for the transfer
   * @param onRequest - Callback to send request to peer
   * @param waitForResponse - Callback to wait for response from peer
   * @returns Promise<void>
   */
  static async completeKeyExchangeAsInitiator(
    initiatorPeerId: string,
    targetPeerId: string,
    sessionId: string,
    onRequest: (request: HmacKeyExchangeRequest) => Promise<void>,
    waitForResponse: () => Promise<HmacKeyExchangeResponse>
  ): Promise<void> {
    try {
      // Step 1: Initiate key exchange
      const request = await this.initiateKeyExchange(
        initiatorPeerId,
        targetPeerId,
        sessionId
      );

      // Step 2: Send request to peer
      await onRequest(request);

      // Step 3: Wait for response from peer
      const response = await waitForResponse();

      // Step 4: Confirm key exchange
      const confirmation = await this.confirmKeyExchange(response, initiatorPeerId);

      console.log('Key exchange completed as initiator:', confirmation.exchange_id);
    } catch (error) {
      console.error('Failed to complete key exchange as initiator:', error);
      throw error;
    }
  }

  /**
   * Complete key exchange flow (responder side)
   * @param request - Key exchange request from initiator
   * @param responderPeerId - Responder's peer ID
   * @param onResponse - Callback to send response to peer
   * @param waitForConfirmation - Callback to wait for confirmation from peer
   * @returns Promise<void>
   */
  static async completeKeyExchangeAsResponder(
    request: HmacKeyExchangeRequest,
    responderPeerId: string,
    onResponse: (response: HmacKeyExchangeResponse) => Promise<void>,
    waitForConfirmation: () => Promise<HmacKeyExchangeConfirmation>
  ): Promise<void> {
    try {
      // Step 1: Respond to key exchange
      const response = await this.respondToKeyExchange(request, responderPeerId);

      // Step 2: Send response to peer
      await onResponse(response);

      // Step 3: Wait for confirmation from peer
      const confirmation = await waitForConfirmation();

      // Step 4: Finalize key exchange
      await this.finalizeKeyExchange(confirmation, responderPeerId);

      console.log('Key exchange completed as responder:', confirmation.exchange_id);
    } catch (error) {
      console.error('Failed to complete key exchange as responder:', error);
      throw error;
    }
  }

  /**
   * Generate a unique session ID
   * @param initiatorPeerId - Initiator's peer ID
   * @param targetPeerId - Target peer ID
   * @param fileHash - File hash (optional)
   * @returns string - Session ID
   */
  static generateSessionId(
    initiatorPeerId: string,
    targetPeerId: string,
    fileHash?: string
  ): string {
    const timestamp = Date.now();
    const base = `${initiatorPeerId}-${targetPeerId}-${timestamp}`;
    return fileHash ? `${base}-${fileHash}` : base;
  }

  /**
   * Clean up expired key exchanges and sessions
   * @returns Promise<void>
   */
  static async cleanupExpiredExchanges(): Promise<void> {
    try {
      await invoke('cleanup_auth_sessions');
      console.log('Cleaned up expired key exchanges');
    } catch (error) {
      console.error('Failed to cleanup expired exchanges:', error);
      throw error;
    }
  }

  /**
   * Create an authenticated session
   * @param sessionId - Session ID
   * @param hmacKey - HMAC key (as byte array)
   * @returns Promise<void>
   */
  static async createSession(sessionId: string, hmacKey: number[]): Promise<void> {
    try {
      await invoke('create_auth_session', {
        sessionId,
        hmacKey,
      });
      console.log('Created authenticated session:', sessionId);
    } catch (error) {
      console.error('Failed to create session:', error);
      throw error;
    }
  }

  /**
   * Generate a random HMAC key
   * @returns Promise<number[]> - HMAC key as byte array
   */
  static async generateHmacKey(): Promise<number[]> {
    try {
      return await invoke<number[]>('generate_hmac_key');
    } catch (error) {
      console.error('Failed to generate HMAC key:', error);
      throw error;
    }
  }
}



