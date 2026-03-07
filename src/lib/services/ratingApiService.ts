const RELAY_BASE = 'http://130.245.173.73:8080';

/** Current owner wallet address */
let currentOwner = '';

/** Set the owner wallet for rating API requests */
export function setRatingOwner(address: string) {
  currentOwner = address;
}

export type TransferOutcome = 'completed' | 'failed';

export interface ReputationEvent {
  id: string;
  transferId: string;
  seederWallet: string;
  downloaderWallet: string;
  fileHash: string;
  amountWei: string;
  outcome: TransferOutcome;
  txHash?: string;
  ratingScore?: number;
  ratingComment?: string;
  createdAt: number;
  updatedAt: number;
}

export interface ReputationResponse {
  wallet: string;
  elo: number;
  baseElo: number;
  completedCount: number;
  failedCount: number;
  transactionCount: number;
  ratingCount: number;
  totalEarnedWei: string;
  events: ReputationEvent[];
}

export interface BatchReputationEntry {
  elo: number;
  completedCount: number;
  failedCount: number;
  transactionCount: number;
  ratingCount: number;
  totalEarnedWei: string;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${RELAY_BASE}${path}`, {
    ...init,
    headers: {
      ...(init?.headers || {}),
      ...(currentOwner ? { 'X-Owner': currentOwner } : {}),
    },
  });
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(text || `HTTP ${res.status}`);
  }
  const contentType = res.headers.get('content-type') || '';
  if (contentType.includes('application/json')) {
    return res.json();
  }
  return (await res.text()) as unknown as T;
}

export const ratingApi = {
  /** Record transfer outcome (completed/failed) for reputation scoring. */
  async recordTransferOutcome(
    transferId: string,
    seederWallet: string,
    fileHash: string,
    outcome: TransferOutcome,
    amountWei: string = '0',
    txHash?: string,
  ): Promise<ReputationEvent> {
    return request<ReputationEvent>('/api/ratings/transfer', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        transferId,
        seederWallet,
        fileHash,
        outcome,
        amountWei,
        txHash: txHash || null,
      }),
    });
  },

  /** Submit a 1-5 rating for a completed transfer event. */
  async submitRating(
    transferId: string,
    seederWallet: string,
    fileHash: string,
    score: number,
    comment?: string,
  ): Promise<ReputationEvent> {
    return request<ReputationEvent>('/api/ratings', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        transferId,
        seederWallet,
        fileHash,
        score,
        comment: comment || null,
      }),
    });
  },

  /** Get Elo reputation summary for a wallet. */
  async getReputation(wallet: string): Promise<ReputationResponse> {
    return request<ReputationResponse>(`/api/ratings/${encodeURIComponent(wallet)}`);
  },

  /** Batch fetch Elo reputations for multiple wallets. */
  async getBatchReputation(
    wallets: string[],
  ): Promise<Record<string, BatchReputationEntry>> {
    const resp = await request<{ reputations: Record<string, BatchReputationEntry> }>(
      '/api/ratings/batch',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ wallets }),
      },
    );
    return resp?.reputations && typeof resp.reputations === 'object' ? resp.reputations : {};
  },
};
