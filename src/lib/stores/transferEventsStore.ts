/**
 * Transfer Events Store
 * 
 * This module provides a reactive Svelte store that listens to typed transfer events
 * from the Rust backend and maintains the state of all active transfers.
 * 
 * Usage:
 * ```typescript
 * import { transferStore, subscribeToTransferEvents } from '$lib/stores/transferEventsStore';
 * 
 * // Subscribe to events when component mounts
 * onMount(async () => {
 *   const unsubscribe = await subscribeToTransferEvents();
 *   return unsubscribe;
 * });
 * 
 * // Access transfer state reactively
 * $: activeTransfers = $transferStore.transfers;
 * ```
 */

import { writable, derived, get, type Readable } from 'svelte/store';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// ============================================================================
// Type Definitions (matching Rust types)
// ============================================================================

export type TransferPriority = 'low' | 'normal' | 'high';

export type SourceType = 'http' | 'ftp' | 'p2p' | 'bittorrent' | 'webrtc' | 'relay';

export type TransferStatus = 
  | 'queued'
  | 'starting'
  | 'downloading'
  | 'paused'
  | 'completed'
  | 'failed'
  | 'canceled';

export interface SourceInfo {
  id: string;
  sourceType: SourceType;
  address: string;
  reputation?: number;
  estimatedSpeedBps?: number;
  latencyMs?: number;
  location?: string;
}

export interface SourceSummary {
  sourceId: string;
  sourceType: SourceType;
  chunksProvided: number;
  bytesProvided: number;
  averageSpeedBps: number;
  connectionDurationSeconds: number;
}

export interface ConnectedSource {
  sourceId: string;
  sourceType: SourceType;
  sourceInfo: SourceInfo;
  connectedAt: number;
  assignedChunks: number[];
  completedChunks: number;
  isActive: boolean;
}

export interface Transfer {
  transferId: string;
  fileHash: string;
  fileName: string;
  fileSize: number;
  outputPath: string;
  status: TransferStatus;
  priority: TransferPriority;
  
  // Progress tracking
  downloadedBytes: number;
  completedChunks: number;
  totalChunks: number;
  progressPercentage: number;
  
  // Speed tracking
  downloadSpeedBps: number;
  uploadSpeedBps: number;
  etaSeconds?: number;
  
  // Source tracking
  availableSources: SourceInfo[];
  connectedSources: Map<string, ConnectedSource>;
  activeSources: number;
  
  // Timing
  queuedAt?: number;
  startedAt?: number;
  completedAt?: number;
  pausedAt?: number;
  failedAt?: number;
  canceledAt?: number;
  durationSeconds?: number;
  averageSpeedBps?: number;
  
  // Error tracking
  error?: string;
  errorCategory?: string;
  retryPossible?: boolean;
  
  // Queue management
  queuePosition?: number;
  
  // Pause/resume state
  canResume?: boolean;
  pauseReason?: string;
  
  // Completion data
  sourcesUsed?: SourceSummary[];
  keepPartial?: boolean;
}

export interface TransferEventPayload {
  type: string;
  [key: string]: any;
}

// ============================================================================
// Store State
// ============================================================================

interface TransferStoreState {
  transfers: Map<string, Transfer>;
  activeCount: number;
  queuedCount: number;
  completedCount: number;
  failedCount: number;
  totalDownloadSpeed: number;
  totalUploadSpeed: number;
  lastEventTimestamp: number;
}

const initialState: TransferStoreState = {
  transfers: new Map(),
  activeCount: 0,
  queuedCount: 0,
  completedCount: 0,
  failedCount: 0,
  totalDownloadSpeed: 0,
  totalUploadSpeed: 0,
  lastEventTimestamp: 0,
};

// ============================================================================
// Writable Store
// ============================================================================

function createTransferStore() {
  const { subscribe, set, update } = writable<TransferStoreState>(initialState);

  return {
    subscribe,
    
    /**
     * Handle a transfer event from the backend
     */
    handleEvent: (event: TransferEventPayload) => {
      update(state => {
        const transfers = new Map(state.transfers);
        const timestamp = Date.now();

        switch (event.type) {
          case 'queued':
            handleQueuedEvent(transfers, event);
            break;
          case 'started':
            handleStartedEvent(transfers, event);
            break;
          case 'source_connected':
            handleSourceConnectedEvent(transfers, event);
            break;
          case 'source_disconnected':
            handleSourceDisconnectedEvent(transfers, event);
            break;
          case 'chunk_completed':
            handleChunkCompletedEvent(transfers, event);
            break;
          case 'chunk_failed':
            handleChunkFailedEvent(transfers, event);
            break;
          case 'progress':
            handleProgressEvent(transfers, event);
            break;
          case 'paused':
            handlePausedEvent(transfers, event);
            break;
          case 'resumed':
            handleResumedEvent(transfers, event);
            break;
          case 'completed':
            handleCompletedEvent(transfers, event);
            break;
          case 'failed':
            handleFailedEvent(transfers, event);
            break;
          case 'canceled':
            handleCanceledEvent(transfers, event);
            break;
          case 'speed_update':
            handleSpeedUpdateEvent(transfers, event);
            break;
          default:
            console.warn('Unknown transfer event type:', event.type);
        }

        return {
          transfers,
          ...calculateStats(transfers),
          lastEventTimestamp: timestamp,
        };
      });
    },

    /**
     * Get a specific transfer by ID
     */
    getTransfer: (transferId: string): Transfer | undefined => {
      return get({ subscribe }).transfers.get(transferId);
    },

    /**
     * Remove a transfer from the store (e.g., after user dismisses completed/failed transfer)
     */
    removeTransfer: (transferId: string) => {
      update(state => {
        const transfers = new Map(state.transfers);
        transfers.delete(transferId);
        return {
          transfers,
          ...calculateStats(transfers),
          lastEventTimestamp: Date.now(),
        };
      });
    },

    /**
     * Clear all completed and failed transfers
     */
    clearFinished: () => {
      update(state => {
        const transfers = new Map(state.transfers);
        for (const [id, transfer] of transfers.entries()) {
          if (transfer.status === 'completed' || transfer.status === 'failed' || transfer.status === 'canceled') {
            transfers.delete(id);
          }
        }
        return {
          transfers,
          ...calculateStats(transfers),
          lastEventTimestamp: Date.now(),
        };
      });
    },

    /**
     * Reset the entire store
     */
    reset: () => set(initialState),
  };
}

// ============================================================================
// Event Handlers
// ============================================================================

function handleQueuedEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer: Transfer = {
    transferId: event.transferId,
    fileHash: event.fileHash,
    fileName: event.fileName,
    fileSize: event.fileSize,
    outputPath: event.outputPath,
    status: 'queued',
    priority: event.priority,
    downloadedBytes: 0,
    completedChunks: 0,
    totalChunks: 0,
    progressPercentage: 0,
    downloadSpeedBps: 0,
    uploadSpeedBps: 0,
    availableSources: [],
    connectedSources: new Map(),
    activeSources: 0,
    queuedAt: event.queuedAt,
    queuePosition: event.queuePosition,
  };
  transfers.set(transfer.transferId, transfer);
}

function handleStartedEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  transfer.status = 'starting';
  transfer.startedAt = event.startedAt;
  transfer.totalChunks = event.totalChunks;
  transfer.availableSources = event.availableSources || [];
}

function handleSourceConnectedEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  const source: ConnectedSource = {
    sourceId: event.sourceId,
    sourceType: event.sourceType,
    sourceInfo: event.sourceInfo,
    connectedAt: event.connectedAt,
    assignedChunks: event.assignedChunks || [],
    completedChunks: 0,
    isActive: true,
  };
  
  transfer.connectedSources.set(event.sourceId, source);
  transfer.activeSources = Array.from(transfer.connectedSources.values())
    .filter(s => s.isActive).length;
  
  if (transfer.status === 'starting') {
    transfer.status = 'downloading';
  }
}

function handleSourceDisconnectedEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  const source = transfer.connectedSources.get(event.sourceId);
  if (source) {
    source.isActive = false;
  }
  
  transfer.activeSources = Array.from(transfer.connectedSources.values())
    .filter(s => s.isActive).length;
}

function handleChunkCompletedEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  transfer.completedChunks++;
  
  const source = transfer.connectedSources.get(event.sourceId);
  if (source) {
    source.completedChunks++;
  }
}

function handleChunkFailedEvent(transfers: Map<string, Transfer>, event: any) {
  // Currently just logging, could add retry tracking here
  console.warn('Chunk failed:', event);
}

function handleProgressEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  transfer.downloadedBytes = event.downloadedBytes;
  transfer.completedChunks = event.completedChunks;
  transfer.progressPercentage = event.progressPercentage;
  transfer.downloadSpeedBps = event.downloadSpeedBps;
  transfer.uploadSpeedBps = event.uploadSpeedBps;
  transfer.etaSeconds = event.etaSeconds;
  transfer.activeSources = event.activeSources;
  
  if (transfer.status !== 'downloading' && transfer.status !== 'paused') {
    transfer.status = 'downloading';
  }
}

function handlePausedEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  transfer.status = 'paused';
  transfer.pausedAt = event.pausedAt;
  transfer.canResume = event.canResume;
  transfer.pauseReason = event.reason;
  transfer.downloadedBytes = event.downloadedBytes;
}

function handleResumedEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  transfer.status = 'downloading';
  transfer.pausedAt = undefined;
  transfer.pauseReason = undefined;
  transfer.downloadedBytes = event.downloadedBytes;
  transfer.activeSources = event.activeSources;
}

function handleCompletedEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  transfer.status = 'completed';
  transfer.completedAt = event.completedAt;
  transfer.durationSeconds = event.durationSeconds;
  transfer.averageSpeedBps = event.averageSpeedBps;
  transfer.downloadedBytes = event.fileSize;
  transfer.progressPercentage = 100;
  transfer.sourcesUsed = event.sourcesUsed;
}

function handleFailedEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  transfer.status = 'failed';
  transfer.failedAt = event.failedAt;
  transfer.error = event.error;
  transfer.errorCategory = event.errorCategory;
  transfer.retryPossible = event.retryPossible;
  transfer.downloadedBytes = event.downloadedBytes;
}

function handleCanceledEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  transfer.status = 'canceled';
  transfer.canceledAt = event.canceledAt;
  transfer.downloadedBytes = event.downloadedBytes;
  transfer.keepPartial = event.keepPartial;
}

function handleSpeedUpdateEvent(transfers: Map<string, Transfer>, event: any) {
  const transfer = transfers.get(event.transferId);
  if (!transfer) return;

  transfer.downloadSpeedBps = event.downloadSpeedBps;
  transfer.uploadSpeedBps = event.uploadSpeedBps;
}

// ============================================================================
// Stats Calculation
// ============================================================================

function calculateStats(transfers: Map<string, Transfer>) {
  let activeCount = 0;
  let queuedCount = 0;
  let completedCount = 0;
  let failedCount = 0;
  let totalDownloadSpeed = 0;
  let totalUploadSpeed = 0;

  for (const transfer of transfers.values()) {
    switch (transfer.status) {
      case 'queued':
        queuedCount++;
        break;
      case 'starting':
      case 'downloading':
        activeCount++;
        totalDownloadSpeed += transfer.downloadSpeedBps;
        totalUploadSpeed += transfer.uploadSpeedBps;
        break;
      case 'completed':
        completedCount++;
        break;
      case 'failed':
        failedCount++;
        break;
    }
  }

  return {
    activeCount,
    queuedCount,
    completedCount,
    failedCount,
    totalDownloadSpeed,
    totalUploadSpeed,
  };
}

// ============================================================================
// Store Instance
// ============================================================================

export const transferStore = createTransferStore();

// ============================================================================
// Derived Stores
// ============================================================================

/**
 * Get only active transfers (downloading or starting)
 */
export const activeTransfers: Readable<Transfer[]> = derived(
  transferStore,
  $store => Array.from($store.transfers.values())
    .filter(t => t.status === 'downloading' || t.status === 'starting')
);

/**
 * Get only queued transfers
 */
export const queuedTransfers: Readable<Transfer[]> = derived(
  transferStore,
  $store => Array.from($store.transfers.values())
    .filter(t => t.status === 'queued')
    .sort((a, b) => (a.queuePosition || 0) - (b.queuePosition || 0))
);

/**
 * Get completed transfers
 */
export const completedTransfers: Readable<Transfer[]> = derived(
  transferStore,
  $store => Array.from($store.transfers.values())
    .filter(t => t.status === 'completed')
    .sort((a, b) => (b.completedAt || 0) - (a.completedAt || 0))
);

/**
 * Get failed transfers
 */
export const failedTransfers: Readable<Transfer[]> = derived(
  transferStore,
  $store => Array.from($store.transfers.values())
    .filter(t => t.status === 'failed')
);

/**
 * Get paused transfers
 */
export const pausedTransfers: Readable<Transfer[]> = derived(
  transferStore,
  $store => Array.from($store.transfers.values())
    .filter(t => t.status === 'paused')
);

// ============================================================================
// Event Subscription
// ============================================================================

let unlistenFunctions: UnlistenFn[] = [];

/**
 * Subscribe to all transfer events from the backend
 * Call this once when your app starts, typically in App.svelte's onMount
 * 
 * @returns A function to unsubscribe from all events
 */
export async function subscribeToTransferEvents(): Promise<() => void> {
  // Unsubscribe from previous listeners if any
  await unsubscribeFromTransferEvents();

  try {
    // Subscribe to the generic event channel that receives all events
    const unlisten = await listen<TransferEventPayload>('transfer:event', (event) => {
      transferStore.handleEvent(event.payload);
    });
    
    unlistenFunctions.push(unlisten);
    
    console.log('✅ Subscribed to transfer events');
    return unsubscribeFromTransferEvents;
  } catch (error) {
    console.error('Failed to subscribe to transfer events:', error);
    throw error;
  }
}

/**
 * Unsubscribe from all transfer events
 */
export async function unsubscribeFromTransferEvents(): Promise<void> {
  for (const unlisten of unlistenFunctions) {
    unlisten();
  }
  unlistenFunctions = [];
}

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Format bytes as human-readable string
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(2)} ${sizes[i]}`;
}

/**
 * Format speed as human-readable string
 */
export function formatSpeed(bps: number): string {
  return `${formatBytes(bps)}/s`;
}

/**
 * Format ETA as human-readable string
 */
export function formatETA(seconds: number | undefined): string {
  if (!seconds || seconds <= 0) return 'Unknown';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);
  return `${hours}h ${minutes}m`;
}

/**
 * Get status color for UI
 */
export function getStatusColor(status: TransferStatus): string {
  switch (status) {
    case 'queued': return 'gray';
    case 'starting': return 'blue';
    case 'downloading': return 'blue';
    case 'paused': return 'yellow';
    case 'completed': return 'green';
    case 'failed': return 'red';
    case 'canceled': return 'gray';
    default: return 'gray';
  }
}