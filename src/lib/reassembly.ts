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
    resolve: (v: boolean) => void;
    reject: (e: any) => void;
  }>;
  writeInFlight: number;
  maxConcurrentWrites: number;
  // hard cap on queued write jobs (to limit memory used by queued chunk buffers)
  maxQueueLength: number;
}

export type ReassemblyEventName = "chunkState" | "progress" | "finalized";

export class ReassemblyManager {
  private transfers = new Map<string, TransferState>();
  private listeners = new Map<ReassemblyEventName, Set<(payload: any) => void>>();

  initReassembly(
    transferId: string,
    manifest: ManifestForReassembly,
    tmpPath: string,
    maxConcurrentWrites = 2,
    maxQueueLength = 1000
  ): void {
    if (this.transfers.has(transferId)) {
      throw new Error(`Transfer ${transferId} already initialized`);
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
    };
  }

  private processWriteQueue(state: TransferState): void {
    while (
      state.writeInFlight < state.maxConcurrentWrites &&
      state.writeQueue.length > 0
    ) {
      const job = state.writeQueue.shift()!;
      state.writeInFlight += 1;
      job
        .run()
        .then((res) => {
          try {
            job.resolve(res);
          } finally {
            state.writeInFlight -= 1;
            this.processWriteQueue(state);
          }
        })
        .catch((err) => {
          try {
            job.reject(err);
          } finally {
            state.writeInFlight -= 1;
            this.processWriteQueue(state);
          }
        });
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

    // Mark requested
    state.chunkStates[chunkIndex] = ChunkState.REQUESTED;
    this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.REQUESTED });

    // Enforce hard queue length cap to bound memory
    // Count both queued and in-flight writes against the cap
    if (state.writeQueue.length + state.writeInFlight >= state.maxQueueLength) {
      throw new Error("Write queue full");
    }

    // Create write task and enqueue
    let resolveFn: (v: boolean) => void;
    let rejectFn: (e: any) => void;
    const p = new Promise<boolean>((resolve, reject) => {
      resolveFn = resolve;
      rejectFn = reject;
    });

    const run = async (): Promise<boolean> => {
      const offset = state.offsets[chunkIndex] || 0;
      try {
        const bytes = Array.from(chunkData as Uint8Array);
        await invoke("write_chunk_temp", {
          transferId,
          chunkIndex,
          offset,
          bytes,
        });

        state.receivedChunks.add(chunkIndex);
        state.chunkStates[chunkIndex] = ChunkState.RECEIVED;
        state.corruptedChunks.delete(chunkIndex);
        this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.RECEIVED });
        this.emit("progress", {
          transferId,
          received: state.receivedChunks.size,
          total: state.manifest.chunks.length,
        });
        return true;
      } catch (err) {
        state.chunkStates[chunkIndex] = ChunkState.CORRUPTED;
        state.corruptedChunks.add(chunkIndex);
        this.emit("chunkState", { transferId, chunkIndex, state: ChunkState.CORRUPTED });
        return false;
      }
    };

    state.writeQueue.push({ run, resolve: resolveFn!, reject: rejectFn! });
    this.processWriteQueue(state);

    const result = await p;
    return result;
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
        expectedRoot: expectedRoot ?? null,
        finalPath,
        tmpPath: state.tmpPath,
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
