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

function asNumber(value: unknown, fallback = 0): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function asString(value: unknown, fallback = ''): string {
  return typeof value === 'string' ? value : fallback;
}

function normalizeEvent(event: any): ReputationEvent {
  return {
    id: asString(event?.id),
    transferId: asString(event?.transferId ?? event?.transfer_id),
    seederWallet: asString(event?.seederWallet ?? event?.seeder_wallet),
    downloaderWallet: asString(event?.downloaderWallet ?? event?.downloader_wallet),
    fileHash: asString(event?.fileHash ?? event?.file_hash),
    amountWei: asString(event?.amountWei ?? event?.amount_wei, '0'),
    outcome: event?.outcome === 'failed' ? 'failed' : 'completed',
    txHash: typeof (event?.txHash ?? event?.tx_hash) === 'string'
      ? (event?.txHash ?? event?.tx_hash)
      : undefined,
    ratingScore: typeof event?.ratingScore === 'number'
      ? event.ratingScore
      : (typeof event?.rating_score === 'number' ? event.rating_score : undefined),
    ratingComment: typeof (event?.ratingComment ?? event?.rating_comment) === 'string'
      ? (event?.ratingComment ?? event?.rating_comment)
      : undefined,
    createdAt: asNumber(event?.createdAt ?? event?.created_at, 0),
    updatedAt: asNumber(event?.updatedAt ?? event?.updated_at, 0),
  };
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
    const raw = await request<any>(`/api/ratings/${encodeURIComponent(wallet)}`);
    const rawEvents = Array.isArray(raw?.events) ? raw.events : [];
    return {
      wallet: asString(raw?.wallet, wallet),
      elo: asNumber(raw?.elo, 50),
      baseElo: asNumber(raw?.baseElo ?? raw?.base_elo, 50),
      completedCount: asNumber(raw?.completedCount ?? raw?.completed_count, 0),
      failedCount: asNumber(raw?.failedCount ?? raw?.failed_count, 0),
      transactionCount: asNumber(raw?.transactionCount ?? raw?.transaction_count, 0),
      ratingCount: asNumber(raw?.ratingCount ?? raw?.rating_count, 0),
      totalEarnedWei: asString(raw?.totalEarnedWei ?? raw?.total_earned_wei, '0'),
      events: rawEvents.map(normalizeEvent),
    };
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
