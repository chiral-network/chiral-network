/**
 * Reputation Service - Simple wrapper for signed transaction message reputation
 * See docs/SIGNED_TRANSACTION_MESSAGES.md for full documentation
 */

import { invoke } from '@tauri-apps/api/core';

export interface TransactionVerdict {
  target_id: string;
  tx_hash: string | null;
  outcome: 'good' | 'bad' | 'disputed';
  details?: string;
  metric?: string;
  issued_at: number;
  issuer_id: string;
  issuer_seq_no: number;
  issuer_sig: string;
  tx_receipt?: string;
  evidence_blobs?: string[];
}

class ReputationService {
  async publishVerdict(verdict: Partial<TransactionVerdict>): Promise<void> {
    try {
      // Get DHT peer ID - fallback to get_peer_id if get_dht_peer_id returns null
      let issuerId = verdict.issuer_id;
      if (!issuerId) {
        try {
          const dhtPeerId = await invoke<string | null>('get_dht_peer_id');
          if (dhtPeerId) {
            issuerId = dhtPeerId;
          } else {
            // Fallback to get_peer_id if DHT peer ID is not available
            issuerId = await invoke<string>('get_peer_id');
          }
        } catch (err) {
          console.warn('Failed to get DHT peer ID, trying get_peer_id:', err);
          issuerId = await invoke<string>('get_peer_id');
        }
      }
      
      const completeVerdict: TransactionVerdict = {
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
      
      console.log('📊 Publishing reputation verdict to DHT:', completeVerdict);
      await invoke('publish_reputation_verdict', { verdict: completeVerdict });
      console.log('✅ Published reputation verdict to DHT for peer:', completeVerdict.target_id);
    } catch (error) {
      console.error('❌ Failed to publish reputation verdict:', error);
      throw error;
    }
  }

  async getReputationVerdicts(peerId: string): Promise<TransactionVerdict[]> {
    try {
      const verdicts = await invoke<TransactionVerdict[]>('get_reputation_verdicts', { peerId });
      return verdicts;
    } catch (error) {
      console.error('❌ Failed to get reputation verdicts:', error);
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
      const value = verdict.outcome === 'good' ? 1.0 
                  : verdict.outcome === 'disputed' ? 0.5 
                  : 0.0;
      
      totalWeight += weight;
      totalValue += weight * value;
    }

    return totalWeight > 0 ? totalValue / totalWeight : 0;
  }
}

export const reputationService = new ReputationService();
