/**
 * Peer Health Manager for transfer operations
 * 
 * Wraps ReputationStore APIs and provides per-transfer peer health decisions,
 * backoff strategies, and peer selection logic.
 */

// Thin frontend shim for authoritative peer health implemented in Rust (src-tauri/src/peer_health.rs)
// The shim forwards calls to the backend via Tauri `invoke` and exposes the same surface used by UI/tests.

import { invoke } from '@tauri-apps/api/tauri';

export interface PeerMetrics {
  peerId: string;
  successCount: number;
  failureCount: number;
  avgResponseTime: number;
  lastResponseTime: number;
  consecutiveFailures: number;
  backoffUntil: number;
  bandwidth: number; // bytes/sec estimate
  lastSeen: number;
}

export interface HealthDecision {
  shouldUse: boolean;
  reason: 'healthy' | 'backoff' | 'too-slow' | 'unreliable' | 'offline' | string;
  weight: number; // 0-1
  maxConcurrent: number;
}

// Shim functions
export async function initPeer(peerId: string): Promise<void> {
  await invoke('ph_init_peer', { peerId });
}

export async function recordSuccess(peerId: string, responseTimeMs: number, bytesTransferred: number): Promise<void> {
  await invoke('ph_record_success', { peerId, responseTimeMs, bytesTransferred });
}

export async function recordFailure(peerId: string, reason: string): Promise<void> {
  await invoke('ph_record_failure', { peerId, reason });
}

export async function getHealthDecision(peerId: string): Promise<HealthDecision> {
  return (await invoke('ph_get_health_decision', { peerId })) as HealthDecision;
}

export async function getPeerMetrics(peerId: string): Promise<PeerMetrics | null> {
  return (await invoke('ph_get_peer_metrics', { peerId })) as PeerMetrics | null;
}

export async function getAllHealthyPeers(): Promise<[string, HealthDecision][]> {
  return (await invoke('ph_get_all_healthy_peers')) as [string, HealthDecision][];
}

export async function selectPeer(excludePeers?: string[]): Promise<string | null> {
  return (await invoke('ph_select_peer', { exclude: excludePeers ?? null })) as string | null;
}

export async function cleanup(maxAgeMs: number): Promise<number> {
  return (await invoke('ph_cleanup', { maxAgeMs })) as number;
}

export async function getStats(): Promise<any> {
  return await invoke('ph_get_stats');
}

// Backwards compatibility: provide a minimal proxy class that mirrors previous API
export class PeerHealthProxy {
  async initPeer(peerId: string) { return initPeer(peerId); }
  async recordSuccess(peerId: string, responseTimeMs: number, bytesTransferred: number) { return recordSuccess(peerId, responseTimeMs, bytesTransferred); }
  async recordFailure(peerId: string, reason: string) { return recordFailure(peerId, reason); }
  async getHealthDecision(peerId: string) { return getHealthDecision(peerId); }
  async getPeerMetrics(peerId: string) { return getPeerMetrics(peerId); }
  async getAllHealthyPeers() { return getAllHealthyPeers(); }
  async selectPeer(excludePeers?: string[]) { return selectPeer(excludePeers); }
  async cleanup(maxAgeMs: number) { return cleanup(maxAgeMs); }
  async getStats() { return getStats(); }
}
