// TypeScript shim for the Rust chunk scheduler (frontend-only)
// Purpose: keep the frontend as a thin API layer. 
// All core scheduling logic runs in`src-tauri/src/chunk_scheduler.rs` . 
// The shim provides typed wrappers around
// Tauri commands so UI code can interact with the scheduler.

import { invoke } from '@tauri-apps/api/tauri';

export interface ChunkRequest {
  chunkIndex: number;
  peerId: string;
  requestedAt: number;
  timeoutMs: number;
}

export interface PeerInfo {
  peerId: string;
  available: boolean;
  lastSeen: number;
  pendingRequests: number;
  maxConcurrent: number;
  avgResponseTime: number;
  failureCount: number;
}

export interface SchedulerConfig {
  maxConcurrentPerPeer: number;
  chunkTimeoutMs: number;
  maxRetries: number;
  peerSelectionStrategy: 'round-robin' | 'fastest-first' | 'load-balanced';
}

export interface ChunkManifest {
  chunks: Array<{ index: number; size: number; checksum?: string }>;
}

export enum ChunkState {
  UNREQUESTED = 'UNREQUESTED',
  REQUESTED = 'REQUESTED',
  RECEIVED = 'RECEIVED',
  CORRUPTED = 'CORRUPTED',
}

export async function initScheduler(manifest: ChunkManifest): Promise<void> {
  await invoke('init_scheduler', { manifest });
}

export async function addPeer(peerId: string, maxConcurrent?: number): Promise<void> {
  await invoke('add_peer', { peerId, maxConcurrent });
}

export async function removePeer(peerId: string): Promise<void> {
  await invoke('remove_peer', { peerId });
}

export async function updatePeerHealth(peerId: string, available: boolean, responseTimeMs?: number): Promise<void> {
  await invoke('update_peer_health', { peerId, available, responseTimeMs });
}

export async function onChunkReceived(chunkIndex: number): Promise<void> {
  await invoke('on_chunk_received', { chunkIndex });
}

export async function onChunkFailed(chunkIndex: number, markCorrupted = false): Promise<void> {
  await invoke('on_chunk_failed', { chunkIndex, markCorrupted });
}

export async function getNextRequests(maxRequests = 10): Promise<ChunkRequest[]> {
  return (await invoke('get_next_requests', { maxRequests })) as ChunkRequest[];
}

export async function getSchedulerState(): Promise<any> {
  return await invoke('get_scheduler_state');
}

export async function isComplete(): Promise<boolean> {
  return (await invoke('is_complete')) as boolean;
}

export async function getActiveRequests(): Promise<ChunkRequest[]> {
  return (await invoke('get_active_requests')) as ChunkRequest[];
}

export async function getPeers(): Promise<PeerInfo[]> {
  return (await invoke('get_peers')) as PeerInfo[];
}
