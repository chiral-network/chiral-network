const RELAY_BASE = 'http://130.245.173.73:8080';

/** Current owner wallet address */
let currentOwner = '';
/** Current owner private key — used to sign the X-Owner-Sig header
 *  on POST /api/ratings/transfer (FM-A03). Never sent over the wire. */
let currentOwnerPrivateKey = '';

let _isTauri: boolean | null = null;
function isTauri(): boolean {
  if (_isTauri === null) {
    _isTauri = typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__;
  }
  return _isTauri;
}

let _invoke: ((cmd: string, args?: Record<string, unknown>) => Promise<any>) | null = null;
async function getInvoke() {
  if (!_invoke) {
    const mod = await import('@tauri-apps/api/core');
    _invoke = mod.invoke;
  }
  return _invoke;
}

/** Set the owner wallet (and signing key) for rating API requests */
export function setRatingOwner(address: string, privateKey: string = '') {
  currentOwner = address;
  currentOwnerPrivateKey = privateKey;
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
  totalEarnedWei: string;
  events: ReputationEvent[];
}

export interface BatchReputationEntry {
  elo: number;
  completedCount: number;
  failedCount: number;
  transactionCount: number;
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
    createdAt: asNumber(event?.createdAt ?? event?.created_at, 0),
    updatedAt: asNumber(event?.updatedAt ?? event?.updated_at, 0),
  };
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const method = (init?.method || 'GET').toUpperCase();
  // Authenticated routes (e.g. POST /api/ratings/transfer) require the
  // X-Owner-Sig header (FM-A03 + FM-A12). Compute it via the Tauri
  // command — the private key never leaves this process.
  const ownerHeaders: Record<string, string> = {};
  if (currentOwner) {
    ownerHeaders['X-Owner'] = currentOwner;
    if (currentOwnerPrivateKey && isTauri() && method !== 'GET') {
      try {
        const invoke = await getInvoke();
        const proof = await invoke('compute_owner_proof', {
          method,
          path,
          walletAddress: currentOwner,
          privateKey: currentOwnerPrivateKey,
        });
        ownerHeaders['X-Owner-Sig'] = proof.header;
      } catch {
        // Server will 401 on a missing sig — let the response surface.
      }
    }
  }
  const res = await fetch(`${RELAY_BASE}${path}`, {
    ...init,
    headers: {
      ...(init?.headers || {}),
      ...ownerHeaders,
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
