// src/lib/reputationStore.ts
export type PeerId = string;

export type PeerReputation = {
  alpha: number;      // successes (decayed)
  beta: number;       // failures (decayed)
  rttMsEMA: number;   // exponential moving average
  lastSeenMs: number; // epoch ms
  lastUpdatedMs: number;
};

const STORAGE_KEY = 'chiral.reputation.store';

export class ReputationStore {
  private static _instance: ReputationStore | null = null;
  static getInstance() {
    if (!this._instance) this._instance = new ReputationStore();
    return this._instance;
  }

  private store = new Map<PeerId, PeerReputation>();
  private readonly a0 = 1;        // Beta prior
  private readonly b0 = 1;
  private readonly rttAlpha = 0.3; // EMA weight
  private readonly halfLifeDays = 14;

  private constructor() {
    this.loadFromStorage();
  }

  private loadFromStorage() {
    if (typeof window === 'undefined') return;
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored) {
        const data = JSON.parse(stored) as Array<[PeerId, PeerReputation]>;
        this.store = new Map(data);
        console.log(`[OK] Loaded ${this.store.size} peer reputations from storage`);
      }
    } catch (e) {
      console.warn('Failed to load reputation store from localStorage:', e);
    }
  }

  private saveToStorage() {
    if (typeof window === 'undefined') return;
    try {
      const data = Array.from(this.store.entries());
      localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
    } catch (e) {
      console.warn('Failed to save reputation store to localStorage:', e);
    }
  }

  private decay(rep: PeerReputation) {
    const now = Date.now();
    const days = (now - rep.lastUpdatedMs) / (1000 * 60 * 60 * 24);
    if (days <= 0) return;
    const k = Math.pow(0.5, days / this.halfLifeDays);
    rep.alpha *= k;
    rep.beta  *= k;
    rep.lastUpdatedMs = now;
  }

  private ensure(id: PeerId): PeerReputation {
    if (!this.store.has(id)) {
      this.store.set(id, {
        alpha: 0,
        beta: 0,
        rttMsEMA: 300,
        lastSeenMs: 0,
        lastUpdatedMs: Date.now(),
      });
    }
    const rep = this.store.get(id)!;
    this.decay(rep);
    return rep;
  }

  noteSeen(id: PeerId) {
    const rep = this.ensure(id);
    rep.lastSeenMs = Date.now();
    this.saveToStorage();
  }

  success(id: PeerId, rttMs?: number) {
    const rep = this.ensure(id);
    rep.alpha += 1;
    if (typeof rttMs === "number") {
      rep.rttMsEMA = rep.rttMsEMA * (1 - this.rttAlpha) + rttMs * this.rttAlpha;
    }
    this.saveToStorage();
  }

  failure(id: PeerId) {
    const rep = this.ensure(id);
    rep.beta += 1;
    this.saveToStorage();
  }

  // Core components in [0,1]
  repScore(id: PeerId): number {
    const rep = this.ensure(id);
    return (rep.alpha + this.a0) / (rep.alpha + rep.beta + this.a0 + this.b0);
  }

  freshScore(id: PeerId): number {
    const rep = this.ensure(id);
    if (!rep.lastSeenMs) return 0;
    const ageSec = (Date.now() - rep.lastSeenMs) / 1000;
    if (ageSec <= 60) return 1;
    if (ageSec >= 86400) return 0; // > 24h
    return 1 - (ageSec - 60) / (86400 - 60);
  }

  perfScore(id: PeerId): number {
    const rep = this.ensure(id);
    const clamped = Math.max(100, Math.min(2000, rep.rttMsEMA));
    return 1 - (clamped - 100) / (2000 - 100);
  }

  composite(id: PeerId): number {
    const wRep = 0.6, wFresh = 0.25, wPerf = 0.15;
    return wRep * this.repScore(id) + wFresh * this.freshScore(id) + wPerf * this.perfScore(id);
  }

  // Load reputation data from backend peer metrics
  loadFromBackendMetrics(metrics: Array<{ peer_id: string; successful_transfers: number; failed_transfers: number; latency_ms?: number; last_seen: number }>) {
    for (const m of metrics) {
      const rep = this.ensure(m.peer_id);
      // Sync backend data with frontend store
      rep.alpha = Math.max(rep.alpha, m.successful_transfers);
      rep.beta = Math.max(rep.beta, m.failed_transfers);
      if (typeof m.latency_ms === 'number' && m.latency_ms > 0) {
        rep.rttMsEMA = rep.rttMsEMA * 0.7 + m.latency_ms * 0.3; // Blend with existing
      }
      rep.lastSeenMs = Math.max(rep.lastSeenMs, m.last_seen * 1000);
      rep.lastUpdatedMs = Date.now();
    }
    this.saveToStorage();
    console.log(`[OK] Synced ${metrics.length} peer reputations from backend`);
  }

  // Get all peers with their data
  getAllPeers(): Map<PeerId, PeerReputation> {
    return new Map(this.store);
  }

  // Clear all data (for testing/debugging)
  clear() {
    this.store.clear();
    this.saveToStorage();
  }
}

export default ReputationStore;
