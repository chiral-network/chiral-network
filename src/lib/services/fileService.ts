import { invoke } from "@tauri-apps/api/core";
import { downloadDir, join } from "@tauri-apps/api/path";

/**
 * A service class to interact with the file transfer and DHT commands
 * on the Rust backend. This is adapted from the implementation guide to match
 * the current backend commands in `main.rs`.
 */
export class FileService {
  /**
   * Initializes the file transfer and DHT services on the backend.
   * This should be called once when the application starts.
   */
  async initializeServices(): Promise<void> {
    await invoke("start_file_transfer_service");
    // Also start the DHT node, as it's closely related to file sharing.
    // The port and bootstrap nodes could be made configurable in the future.
    // Get bootstrap nodes from the backend instead of hardcoding
    const bootstrapNodes = await invoke<string[]>(
      "get_bootstrap_nodes_command"
    );
    await invoke("start_dht_node", {
      port: 4001,
      bootstrapNodes,
    });

    // Get the peer ID and set it on the DHT service singleton
    const { dhtService } = await import("$lib/dht");
    const peerId = await invoke<string | null>("get_dht_peer_id");
    if (peerId) {
      dhtService.setPeerId(peerId);
    }
  }

  /**
   * Uploads a file to the network using streaming upload for unlimited file sizes.
   * This reads the file in chunks and streams them to the backend without temp files.
   * @param file The file object to upload.
   * @returns The metadata of the uploaded file.
   */
  async uploadFile(file: File): Promise<any> {
    const chunkSize = 64 * 1024; // 64KB chunks for efficient streaming
    const totalChunks = Math.ceil(file.size / chunkSize);

    try {
      // Start the streaming upload session
      const uploadId = await invoke<string>("start_streaming_upload", {
        fileName: file.name,
        fileSize: file.size,
      });

      let fileHash: string | null = null;

      // Stream the file in chunks
      for (let chunkIndex = 0; chunkIndex < totalChunks; chunkIndex++) {
        const start = chunkIndex * chunkSize;
        const end = Math.min(start + chunkSize, file.size);
        const chunk = file.slice(start, end);
        const buffer = await chunk.arrayBuffer();
        const chunkData = Array.from(new Uint8Array(buffer));

        const isLastChunk = chunkIndex === totalChunks - 1;

        // Upload this chunk
        const result = await invoke<string | null>("upload_file_chunk", {
          uploadId,
          chunkData,
          chunkIndex,
          isLastChunk,
        });

        // If this was the last chunk, we get the file hash back
        if (isLastChunk && result) {
          fileHash = result;
        }
      }

      if (!fileHash) {
        throw new Error("Upload completed but no file hash received");
      }

      // Return metadata similar to what the backend would provide
      return {
        fileHash,
        fileName: file.name,
        fileSize: file.size,
        seeders: [],
        createdAt: Date.now(),
        mimeType: file.type || undefined,
        isEncrypted: false,
        version: 1,
      };
    } catch (error: any) {
      console.error("Streaming upload failed:", error);
      throw new Error(`Upload failed: ${error}`);
    }
  }

  /**
   * Uploads a file to the network from a given file path.
   * This is suitable for files already on the user's disk, referenced by path.
   * @param filePath The absolute path to the file.
   * @param fileName The name of the file.
   * @returns The hash of the uploaded file.
   */
  async uploadFileFromPath(
    filePath: string,
    fileName: string
  ): Promise<string> {
    // Calls 'upload_file_to_network' on the backend.
    const hash = await invoke<string>("upload_file_to_network", {
      filePath,
      fileName,
    });
    return hash;
  }

  /**
   * Downloads a file from the network given its hash.
   * The backend saves it to the user's default download directory.
   * @param hash The hash of the file to download.
   * @param fileName The name to save the file as.
   * @returns The full path to the downloaded file.
   */
  async downloadFile(hash: string, fileName: string): Promise<string> {
    const downloadPath = await downloadDir();
    const outputPath = await join(downloadPath, fileName);

    // Calls 'download_file_from_network' on the backend.
    // Note: The current backend implementation only retrieves from its local
    // storage, not from other peers yet. This is a starting point.
    await invoke("download_file_from_network", {
      fileHash: hash,
      outputPath: outputPath,
    });

    return outputPath;
  }

  /**
   * Opens the folder containing the specified file in the native file explorer.
   * @param path The full path to the file.
   */
  async showInFolder(path: string): Promise<void> {
    await invoke("show_in_folder", { path });
  }

  /**
   * Queries the backend for the amount of free disk space (in GB) the node can use.
   * Returns null when the call fails so the UI can surface a retry affordance.
   */
  async getAvailableStorage(): Promise<number | null> {
    try {
      const storage = await invoke<number>("get_available_storage");
      return Number.isFinite(storage) ? storage : null;
    } catch (error) {
      console.error("Failed to load available storage:", error);
      return null;
    }
  }
}

// It's often useful to export a singleton instance of the service.
export const fileService = new FileService();
