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

interface TransferState {
  transferId: string;
  manifest: ManifestForReassembly;
  tmpPath: string;
  receivedChunks: Set<number>;
  corruptedChunks: Set<number>;
  offsets: number[]; // byte offset for each chunk
  // Promise chain used to serialize concurrent operations for this transfer
  pending?: Promise<any>;
}

export class ReassemblyManager {
  private transfers = new Map<string, TransferState>();

  initReassembly(
    transferId: string,
    manifest: ManifestForReassembly,
    tmpPath: string
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

    const state: TransferState = {
      transferId,
      manifest,
      tmpPath,
      receivedChunks: new Set(),
      corruptedChunks: new Set(),
      offsets,
      pending: Promise.resolve(null),
    };

    this.transfers.set(transferId, state);
  }

  getState(transferId: string): TransferState | null {
    return this.transfers.get(transferId) ?? null;
  }

  async acceptChunk(
    transferId: string,
    chunkIndex: number,
    chunkData: Uint8Array
  ): Promise<boolean> {
    const state = this.transfers.get(transferId);
    if (!state) throw new Error(`Unknown transfer ${transferId}`);

    // Serialize concurrent acceptChunk calls per-transfer using pending chain
    const task = async () => {
      if (chunkIndex < 0 || chunkIndex >= state.manifest.chunks.length) {
        throw new Error(`Invalid chunk index ${chunkIndex}`);
      }

      const info = state.manifest.chunks[chunkIndex];

      // Validate checksum when available
      if (info.checksum) {
        const calculated = await calculateSHA256Hex(chunkData);
        if (calculated !== info.checksum) {
          // Mark corrupted and do not persist
          state.corruptedChunks.add(chunkIndex);
          return false;
        }
      }

      // Persist chunk to temporary file via backend
      const offset = state.offsets[chunkIndex] || 0;

      try {
        // Convert to plain number[] for Tauri IPC
        const bytes = Array.from(chunkData as Uint8Array);
        await invoke("write_chunk_temp", {
          transferId,
          chunkIndex,
          offset,
          bytes,
        });

        state.receivedChunks.add(chunkIndex);
        // If previously marked corrupted, remove
        state.corruptedChunks.delete(chunkIndex);
        return true;
      } catch (err) {
        // Mark as corrupted/failed to persist
        state.corruptedChunks.add(chunkIndex);
        return false;
      }
    };

    // Chain the task onto the pending promise so operations execute sequentially
    const prev = state.pending ?? Promise.resolve(null);
    const next = prev.then(() => task(), () => task());
    // Store next as pending, but ensure any rejection is caught later by caller
    state.pending = next;

    try {
      const result = await next;
      return Boolean(result);
    } catch (err) {
      // propagate error
      throw err;
    }
  }

  markChunkCorrupt(transferId: string, chunkIndex: number): void {
    const state = this.transfers.get(transferId);
    if (!state) return;
    state.corruptedChunks.add(chunkIndex);
    state.receivedChunks.delete(chunkIndex);
  }

  isComplete(transferId: string): boolean {
    const state = this.transfers.get(transferId);
    if (!state) return false;
    return (
      state.receivedChunks.size === state.manifest.chunks.length &&
      state.corruptedChunks.size === 0
    );
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

      // Backend should return truthy success object or boolean
      const ok = (res as any) === true || (res && (res as any).ok === true);

      if (ok) {
        this.transfers.delete(transferId);
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
