const RELAY_BASE = 'http://130.245.173.73:8080';

/** Current owner wallet address */
let currentOwner = '';

/** Set the owner wallet for rating API requests */
export function setRatingOwner(address: string) {
  currentOwner = address;
}

export interface Rating {
  id: string;
  seederWallet: string;
  raterWallet: string;
  fileHash: string;
  score: number;
  comment?: string;
  createdAt: number;
}

export interface RatingResponse {
  ratings: Rating[];
  average: number;
  count: number;
}

export interface BatchRatingEntry {
  average: number;
  count: number;
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
  /** Submit a rating for a seeder after downloading */
  async submitRating(
    seederWallet: string,
    fileHash: string,
    score: number,
    comment?: string,
  ): Promise<Rating> {
    return request<Rating>('/api/ratings', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        seederWallet,
        fileHash,
        score,
        comment: comment || null,
      }),
    });
  },

  /** Get all ratings for a wallet address */
  async getRatings(wallet: string): Promise<RatingResponse> {
    return request<RatingResponse>(`/api/ratings/${encodeURIComponent(wallet)}`);
  },

  /** Batch fetch average ratings for multiple wallets */
  async getBatchRatings(
    wallets: string[],
  ): Promise<Record<string, BatchRatingEntry>> {
    const resp = await request<{ ratings: Record<string, BatchRatingEntry> }>(
      '/api/ratings/batch',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ wallets }),
      },
    );
    return resp.ratings;
  },
};
