// src/lib/reputationStore.ts
export type PeerId = string;

export type PeerReputation = {
  alpha: number;      // successes (decayed)
  beta: number;       // failures (decayed)
  rttMsEMA: number;   // exponential moving average
  lastSeenMs: number; // epoch ms
  lastUpdatedMs: number;
};

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

  private constructor() {}

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
  }

  success(id: PeerId, rttMs?: number) {
    const rep = this.ensure(id);
    rep.alpha += 1;
    if (typeof rttMs === "number") {
      rep.rttMsEMA = rep.rttMsEMA * (1 - this.rttAlpha) + rttMs * this.rttAlpha;
    }
  }

  failure(id: PeerId) {
    const rep = this.ensure(id);
    rep.beta += 1;
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
}

export default ReputationStore;
