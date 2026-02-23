/**
 * In-memory reputation cache — NOT persisted to localStorage.
 *
 * Scores are derived from cryptographically signed verdicts stored in the DHT,
 * so local persistence would only create a stale tamper-able copy. All reads
 * go through the Tauri backend which fetches from the DHT and verifies signatures.
 */

export interface VerifiedReputation {
  score: number;           // 0.0 – 1.0
  trustLevel: string;      // 'trusted' | 'high' | 'medium' | 'low' | 'unknown'
  totalVerdicts: number;
  goodCount: number;
  disputedCount: number;
  badCount: number;
  signatureVerifiedCount: number;
  confidence: number;      // 0.0 – 1.0
}

export interface TransactionVerdict {
  targetId: string;
  issuerId: string;
  outcome: 'good' | 'disputed' | 'bad';
  details: string | null;
  issuedAt: number;        // unix seconds
  issuerSig: string;       // hex-encoded ed25519 signature
}

export function unknownReputation(): VerifiedReputation {
  return {
    score: 0.5,
    trustLevel: 'unknown',
    totalVerdicts: 0,
    goodCount: 0,
    disputedCount: 0,
    badCount: 0,
    signatureVerifiedCount: 0,
    confidence: 0,
  };
}

export function trustLevelColor(level: string): string {
  switch (level) {
    case 'trusted': return 'text-green-600 dark:text-green-400';
    case 'high':    return 'text-blue-600 dark:text-blue-400';
    case 'medium':  return 'text-yellow-600 dark:text-yellow-400';
    case 'low':     return 'text-orange-600 dark:text-orange-400';
    default:        return 'text-gray-500 dark:text-gray-400';
  }
}

export function trustLevelBg(level: string): string {
  switch (level) {
    case 'trusted': return 'bg-green-100 dark:bg-green-900/30 text-green-800 dark:text-green-200';
    case 'high':    return 'bg-blue-100 dark:bg-blue-900/30 text-blue-800 dark:text-blue-200';
    case 'medium':  return 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-800 dark:text-yellow-200';
    case 'low':     return 'bg-orange-100 dark:bg-orange-900/30 text-orange-800 dark:text-orange-200';
    default:        return 'bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400';
  }
}

export function outcomeLabel(outcome: string): string {
  switch (outcome) {
    case 'good':     return '✅ Positive';
    case 'disputed': return '⚠️ Disputed';
    case 'bad':      return '❌ Negative';
    default:         return outcome;
  }
}

export function scoreToStars(score: number): string {
  const stars = Math.round(score * 5 * 10) / 10;
  return `${stars.toFixed(1)} / 5.0`;
}

/** In-memory cache: peerId → { rep, fetchedAt } */
const cache = new Map<string, { rep: VerifiedReputation; fetchedAt: number }>();
const CACHE_TTL_MS = 5 * 60 * 1000; // 5 minutes

export function getCached(peerId: string): VerifiedReputation | null {
  const entry = cache.get(peerId);
  if (!entry) return null;
  if (Date.now() - entry.fetchedAt > CACHE_TTL_MS) {
    cache.delete(peerId);
    return null;
  }
  return entry.rep;
}

export function setCached(peerId: string, rep: VerifiedReputation): void {
  cache.set(peerId, { rep, fetchedAt: Date.now() });
}

export function clearCache(): void {
  cache.clear();
}
