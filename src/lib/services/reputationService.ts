/**
 * Reputation Service - Simple wrapper for signed transaction message reputation
 * See docs/SIGNED_TRANSACTION_MESSAGES.md for full documentation
 */

import { invoke } from '@tauri-apps/api/core';
import type { TransactionVerdict } from '$lib/types/reputation';
import {
  reputationRateLimiter,
  RateLimitError,
  type RateLimitDecision,
} from './reputationRateLimiter';

class ReputationService {
  async publishVerdict(verdict: Partial<TransactionVerdict>): Promise<RateLimitDecision> {
    let decision: RateLimitDecision | null = null;
    let completeVerdict: TransactionVerdict | null = null;

    try {
      // Get DHT peer ID - fallback to get_peer_id if get_dht_peer_id returns null
      let issuerId = verdict.issuer_id;
      if (!issuerId) {
        try {
          const dhtPeerId = await invoke<string | null>('get_dht_peer_id');
          issuerId = dhtPeerId || (await invoke<string>('get_peer_id'));
        } catch (err) {
          console.warn('Failed to get DHT peer ID, trying get_peer_id:', err);
          issuerId = await invoke<string>('get_peer_id');
        }
      }

      completeVerdict = {
        target_id: verdict.target_id!,
        tx_hash: verdict.tx_hash || null,
        outcome: verdict.outcome || 'good',
        details: verdict.details,
        metric: verdict.metric || 'transaction',
        issued_at: verdict.issued_at || Math.floor(Date.now() / 1000),
        issuer_id: issuerId,
        issuer_seq_no: verdict.issuer_seq_no || Date.now(),
        issuer_sig: verdict.issuer_sig || '',
        tx_receipt: verdict.tx_receipt,
        evidence_blobs: verdict.evidence_blobs,
      };

      decision = reputationRateLimiter.evaluate(completeVerdict);
      if (!decision.allowed) {
        reputationRateLimiter.recordDecision(completeVerdict, decision, { sent: false });
        const message = `Reputation verdict blocked by rate limiter (${decision.reason ?? 'limit'})`;
        console.warn(message, decision);
        throw new RateLimitError(message, decision);
      }

      console.log('[STATS] Publishing reputation verdict to DHT:', completeVerdict);
      await invoke('publish_reputation_verdict', { verdict: completeVerdict });
      reputationRateLimiter.recordDecision(completeVerdict, decision, { sent: true });
      console.log('[OK] Published reputation verdict to DHT for peer:', completeVerdict.target_id);
      return decision;
    } catch (error) {
      if (completeVerdict && decision && !(error instanceof RateLimitError)) {
        reputationRateLimiter.recordDecision(completeVerdict, decision, { sent: false });
      }
      console.error('[X] Failed to publish reputation verdict:', error);
      throw error;
    }
  }

  async getReputationVerdicts(peerId: string): Promise<TransactionVerdict[]> {
    try {
      const verdicts = await invoke<TransactionVerdict[]>('get_reputation_verdicts', { peerId });
      return verdicts;
    } catch (error) {
      console.error('[X] Failed to get reputation verdicts:', error);
      return [];
    }
  }

  async getPeerScore(peerId: string): Promise<number> {
    const verdicts = await this.getReputationVerdicts(peerId);
    if (verdicts.length === 0) return 0;

    let totalWeight = 0;
    let totalValue = 0;

    for (const verdict of verdicts) {
      const weight = 1.0;
      const value =
        verdict.outcome === 'good' ? 1.0 : verdict.outcome === 'disputed' ? 0.5 : 0.0;

      totalWeight += weight;
      totalValue += weight * value;
    }

    return totalWeight > 0 ? totalValue / totalWeight : 0;
  }
}

export const reputationService = new ReputationService();

