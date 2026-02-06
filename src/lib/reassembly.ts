import { invoke } from "@tauri-apps/api/core";

export interface ChunkInfo {
  index: number;
  encryptedSize: number;
  checksum?: string; // hex sha256
}

export interface ManifestForReassembly {
  fileSize: number;
  chunks: ChunkInfo[];
  merkleRoot?: string; // optional
}

export enum ChunkState {
  UNREQUESTED = "UNREQUESTED",
  REQUESTED = "REQUESTED",
  RECEIVED = "RECEIVED",
  CORRUPTED = "CORRUPTED",
}

interface TransferState {
  transferId: string;
  manifest: ManifestForReassembly;
  tmpPath: string;
  receivedChunks: Set<number>;
  corruptedChunks: Set<number>;
  offsets: number[]; // byte offset for each chunk
  // Promise chain used to serialize concurrent operations for this transfer (compat)
  pending?: Promise<any>;
  // explicit per-chunk states
  chunkStates: ChunkState[];
  // bounded write queue to limit memory usage
  writeQueue: Array<{
    run: () => Promise<boolean>;
    chunkIndex?: number;
  }>;
  writeInFlight: number;
  maxConcurrentWrites: number;
  // hard cap on queued write jobs (to limit memory used by queued chunk buffers)
  maxQueueLength: number;
  // single-writer drain flag
  isDraining?: boolean;
  // track which indices are currently queued to avoid duplicate writes
  queuedIndices?: Set<number>;
}

export type ReassemblyEventName = "chunkState" | "progress" | "finalized" | "queue";

export class ReassemblyManager {
  private transfers = new Map<string, TransferState>();
  private listeners = new Map<ReassemblyEventName, Set<(payload: any) => void>>();

  initReassembly(
    transferId: string,
    manifest: ManifestForReassembly,
    tmpPath: string,
    maxConcurrentWrites = 1,
    maxQueueLength = 1000
  ): void {
    if (this.transfers.has(transferId)) {
      throw new Error(`Transfer ${transferId} already initialized`);
    }

    // Basic manifest validation to avoid malformed inputs
    if (!manifest || !Array.isArray(manifest.chunks) || manifest.chunks.length === 0) {
      throw new Error("Invalid manifest: missing chunks");
    }

    let total = 0;
    for (const ch of manifest.chunks) {
      if (!ch || typeof ch.encryptedSize !== 'number' || ch.encryptedSize <= 0) {
        throw new Error("Invalid manifest: chunk sizes must be positive numbers");
      }
      total += ch.encryptedSize;
    }

    if (total < manifest.fileSize) {
      // tolerate metadata mismatch but log a warning via throwing would be strict; choose tolerance
      // ensure fileSize is at most sum of chunk sizes
      manifest.fileSize = total;
    }

    // Precompute offsets (supports variable chunk sizes)
    const offsets: number[] = [];
    let cursor = 0;
    for (const ch of manifest.chunks) {
      offsets.push(cursor);
      cursor += ch.encryptedSize;
    }

    const chunkStates = manifest.chunks.map(() => ChunkState.UNREQUESTED);

    const state: TransferState = {
      transferId,
      manifest,
      tmpPath,
      receivedChunks: new Set(),
      corruptedChunks: new Set(),
      offsets,
      pending: Promise.resolve(null),
      chunkStates,
      writeQueue: [],
      writeInFlight: 0,
      maxConcurrentWrites,
      maxQueueLength,
      isDraining: false,
      queuedIndices: new Set(),
    };

    this.transfers.set(transferId, state);
  }

  // Event API
  on(eventName: ReassemblyEventName, cb: (payload: any) => void): void {
    if (!this.listeners.has(eventName)) this.listeners.set(eventName, new Set());
    this.listeners.get(eventName)!.add(cb);
  }
  off(eventName: ReassemblyEventName, cb: (payload: any) => void): void {
    this.listeners.get(eventName)?.delete(cb);
  }
  private emit(eventName: ReassemblyEventName, payload: any): void {
    this.listeners.get(eventName)?.forEach((cb) => {
      try {
        cb(payload);
      } catch (e) {
        // swallow listener errors
      }
    });
  }

  // Return a safe snapshot of internal state for test/debug; do not rely on this API for production.
  getState(transferId: string): any {
    const state = this.transfers.get(transferId);
    if (!state) return null;

    // Return copies only (no references to internal mutable structures)
    return {
      transferId: state.transferId,
      manifest: state.manifest,
      tmpPath: state.tmpPath,
      offsets: state.offsets.slice(),
      receivedChunks: Array.from(state.receivedChunks.values()),
      corruptedChunks: Array.from(state.corruptedChunks.values()),
      chunkStates: state.chunkStates.slice(),
      writeInFlight: state.writeInFlight,
      writeQueueLength: state.writeQueue.length,
      maxConcurrentWrites: state.maxConcurrentWrites,
      maxQueueLength: state.maxQueueLength,
      isDraining: !!state.isDraining,
      queuedIndices: Array.from(state.queuedIndices ?? []),
    };
  }

  // Return current queue depth and in-flight count for a transfer
  getQueueDepth(transferId: string): { queueDepth: number; inFlight: number } | null {
    const state = this.transfers.get(transferId);
    if (!state) return null;
    return { queueDepth: state.writeQueue.length, inFlight: state.writeInFlight };
  }

  // Cancel and cleanup a transfer: clears queue and optionally asks backend to cleanup temp files
  async cancelReassembly(transferId: string): Promise<void> {
    const state = this.transfers.get(transferId);
    if (!state) return;

    // Clear queued jobs
    state.writeQueue.length = 0;
    state.queuedIndices?.clear();

    try {
      await invoke('cleanup_transfer_temp', { transferId });
    } catch (e) {
      // ignore cleanup errors
    }

    this.transfers.delete(transferId);
    this.emit('finalized', { transferId, cancelled: true });
  }

  private async startDrain(state: TransferState): Promise<void> {
    if (state.isDraining) return;
    state.isDraining = true;

    try {
      while (state.writeQueue.length > 0) {
        // respect maxConcurrentWrites though most callers use 1 for single-writer semantics
        if (state.writeInFlight >= state.maxConcurrentWrites) {
          // yield and retry
          await new Promise((r) => setTimeout(r, 0));
          continue;
        }

        const job = state.writeQueue.shift()!;
        const jobIndex = job.chunkIndex;
        if (jobIndex !== undefined) state.queuedIndices?.delete(jobIndex);
        state.writeInFlight += 1;
        try {
          await job.run();
        } catch (e) {
          // run() already updates chunk state on errors; swallow to keep drain alive
        } finally {
          state.writeInFlight -= 1;
          // emit progress including queue depth so callers can observe backpressure/resume
          this.emit("progress", {
            transferId: state.transferId,
            received: state.receivedChunks.size,
            total: state.manifest.chunks.length,
            queueDepth: state.writeQueue.length,
            inFlight: state.writeInFlight,
          });
          // also emit a queue-specific event for consumers that prefer it
          this.emit('queue', {
            transferId: state.transferId,
            queueDepth: state.writeQueue.length,
            inFlight: state.writeInFlight,
          });
        }
      }
    } finally {
      state.isDraining = false;
    }
  }

  async acceptChunk(
    transferId: string,
    chunkIndex: number,
    chunkData: Uint8Array
  ): Promise<boolean> {
    const state = this.transfers.get(transferId);
    if (!state) throw new Error(`Unknown transfer ${transferId}`);

    // Quick index validation
    if (chunkIndex < 0 || chunkIndex >= state.manifest.chunks.length) {
      throw new Error(`Invalid chunk index ${chunkIndex}`);
    }

    // Ignore already received chunks
    if (state.chunkStates[chunkIndex] === ChunkState.RECEIVED) {
      return true;
    }

    // If this chunk is already queued, treat as accepted (idempotent)
    if (state.queuedIndices?.has(chunkIndex)) {
      return true;
    }

    // Mark as requested only if previously UNREQUESTED
    if (state.chunkStates[chunkIndex] === ChunkState.UNREQUESTED) {
      state.chunkStates[chunkIndex] = ChunkState.REQUESTED;
      this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.REQUESTED });
    }

    const info = state.manifest.chunks[chunkIndex];

    // Validate checksum when available
    if (info.checksum) {
      const calculated = await calculateSHA256Hex(chunkData);
      if (calculated !== info.checksum) {
        state.chunkStates[chunkIndex] = ChunkState.CORRUPTED;
        state.corruptedChunks.add(chunkIndex);
        this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.CORRUPTED });
        return false;
      }
    }

    // Size and bounds checks to avoid malicious/accidental oversize writes
    const expectedSize = info.encryptedSize;
    if (chunkData.length > expectedSize) {
      state.chunkStates[chunkIndex] = ChunkState.CORRUPTED;
      state.corruptedChunks.add(chunkIndex);
      this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.CORRUPTED });
      return false;
    }
    const offset = state.offsets[chunkIndex] || 0;
    if (offset + chunkData.length > state.manifest.fileSize) {
      state.chunkStates[chunkIndex] = ChunkState.CORRUPTED;
      state.corruptedChunks.add(chunkIndex);
      this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.CORRUPTED });
      return false;
    }

    // Mark logical receipt BEFORE enqueueing to reflect progress semantics (idempotent)
    state.chunkStates[chunkIndex] = ChunkState.RECEIVED;
    state.receivedChunks.add(chunkIndex);
    state.corruptedChunks.delete(chunkIndex);
    this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.RECEIVED });
    this.emit("progress", {
      transferId,
      received: state.receivedChunks.size,
      total: state.manifest.chunks.length,
      queueDepth: state.writeQueue.length,
      inFlight: state.writeInFlight,
    });

    // Enforce hard queue length cap to bound memory
    // Count both queued and in-flight writes against the cap
    if (state.writeQueue.length + state.writeInFlight >= state.maxQueueLength) {
      // Signal backpressure to caller by returning false
      return false;
    }

    const run = async (): Promise<boolean> => {
      const maxRetries = 2;
      let attempt = 0;
      while (attempt <= maxRetries) {
        try {
          // Pass ArrayBuffer to avoid constructing a large Array from the bytes (zero-copy where possible)
          const buffer = chunkData.buffer;
          await invoke("write_chunk_temp", {
            transferId,
            chunkIndex,
            offset,
            bytes: buffer,
            chunkChecksum: info.checksum ?? undefined,
          });

          // Durability succeeded — nothing else to do since logical receipt was already marked
          return true;
        } catch (err) {
          attempt += 1;
          // small backoff for transient errors
          if (attempt <= maxRetries) {
            await new Promise((r) => setTimeout(r, 10 * attempt));
            continue;
          }

          // Persistence failed → rollback logical receipt
          state.chunkStates[chunkIndex] = ChunkState.CORRUPTED;
          state.corruptedChunks.add(chunkIndex);
          state.receivedChunks.delete(chunkIndex);

          this.emit("chunkState", {
            transferId,
            chunkIndex,
            state: ChunkState.CORRUPTED,
          });

          this.emit("progress", {
            transferId,
            received: state.receivedChunks.size,
            total: state.manifest.chunks.length,
            queueDepth: state.writeQueue.length,
            inFlight: state.writeInFlight,
          });

          return false;
        }
      }
      return false;
    };

    // Enqueue job and start single-writer drain loop. Do not await completion here (enqueue-only).
    state.writeQueue.push({ run, chunkIndex });
    state.queuedIndices?.add(chunkIndex);
    // Emit queue depth so callers can observe backpressure
    this.emit("progress", {
      transferId,
      received: state.receivedChunks.size,
      total: state.manifest.chunks.length,
      queueDepth: state.writeQueue.length,
      inFlight: state.writeInFlight,
    });
    this.emit('queue', { transferId, queueDepth: state.writeQueue.length, inFlight: state.writeInFlight });

    // Start drain loop asynchronously (fire-and-forget)
    void this.startDrain(state);

    // Immediately return true to indicate the chunk was accepted for write.
    // Caller should listen to progress/chunkState events to observe completion.
    return true;
  }

  markChunkCorrupt(transferId: string, chunkIndex: number): void {
    const state = this.transfers.get(transferId);
    if (!state) return;
    state.chunkStates[chunkIndex] = ChunkState.CORRUPTED;
    state.corruptedChunks.add(chunkIndex);
    state.receivedChunks.delete(chunkIndex);
    this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.CORRUPTED });
  }

  // Public helper to mark a chunk as received without going through acceptChunk (useful for resume/fake tests)
  markChunkReceived(transferId: string, chunkIndex: number): void {
    const state = this.transfers.get(transferId);
    if (!state) return;
    state.receivedChunks.add(chunkIndex);
    state.corruptedChunks.delete(chunkIndex);
    state.chunkStates[chunkIndex] = ChunkState.RECEIVED;
    this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.RECEIVED });
    this.emit("progress", {
      transferId,
      received: state.receivedChunks.size,
      total: state.manifest.chunks.length,
      queueDepth: state.writeQueue.length,
      inFlight: state.writeInFlight,
    });
  }

  isComplete(transferId: string): boolean {
    const state = this.transfers.get(transferId);
    if (!state) return false;
    const allReceived = state.chunkStates.every((s) => s === ChunkState.RECEIVED);
    return allReceived && state.corruptedChunks.size === 0;
  }

  async finalize(
    transferId: string,
    finalPath: string,
    expectedRoot?: string | null
  ): Promise<boolean> {
    const state = this.transfers.get(transferId);
    if (!state) throw new Error(`Unknown transfer ${transferId}`);

    if (!this.isComplete(transferId)) {
      throw new Error(`Transfer ${transferId} not complete`);
    }

    try {
      const res = await invoke("verify_and_finalize", {
        transferId,
        expectedSha256: expectedRoot ?? null,
        finalPath,
      });

      const ok = (res as any) === true || (res && (res as any).ok === true);

      if (ok) {
        this.transfers.delete(transferId);
        this.emit("finalized", { transferId, finalPath });
        return true;
      }

      return false;
    } catch (err) {
      return false;
    }
  }
}

export const reassemblyManager = new ReassemblyManager();

// Helper: calculate SHA256 hex string. Works in browsers/node.
async function calculateSHA256Hex(data: Uint8Array): Promise<string> {
  // Prefer Web Crypto
  try {
    if (typeof (globalThis as any).crypto !== "undefined" &&
      (globalThis as any).crypto.subtle &&
      typeof (globalThis as any).crypto.subtle.digest === "function") {
      const hash = await (globalThis as any).crypto.subtle.digest("SHA-256", data);
      return bufferToHex(new Uint8Array(hash));
    }
  } catch (e) {
    // fallthrough to node crypto
  }

  // Node fallback
  try {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const { createHash } = require("crypto");
    const h = createHash("sha256");
    h.update(Buffer.from(data));
    return h.digest("hex");
  } catch (e) {
    throw new Error("No available crypto to calculate SHA-256");
  }
}

function bufferToHex(buf: Uint8Array): string {
  return Array.from(buf)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}
