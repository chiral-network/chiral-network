import { invoke } from "@tauri-apps/api/core";
import { join } from "@tauri-apps/api/path";

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
   * Uploads a file to the network (for drag-and-drop via Web File API).
   * Saves file to temp location then uses ChunkManager for encryption/chunking.
   * @param file The file object to upload.
   * @param recipientPublicKey Optional recipient public key for encrypted sharing.
   * @returns The file manifest from ChunkManager.
   */
  async uploadFile(file: File, recipientPublicKey?: string): Promise<any> {
    try {
      // Read file into memory
      const buffer = await file.arrayBuffer();
      const fileData = Array.from(new Uint8Array(buffer));

      // Save to temp file (backend will use ChunkManager on this)
      const tempFilePath = await invoke<string>("save_temp_file_for_upload", {
        fileName: file.name,
        fileData,
      });

      console.log(`Saved drag-and-drop file to temp: ${tempFilePath}`);

      // Import encryptionService to use same flow as file path selection
      const { encryptionService } = await import("./encryption");

      // Use ChunkManager via encryptionService (same as file path upload)
      const manifest = await encryptionService.encryptFile(
        tempFilePath,
        recipientPublicKey
      );

      return manifest;
    } catch (error: any) {
      console.error("Upload failed:", error);
      throw new Error(`Upload failed: ${error}`);
    }
  }

  /**
   * Retrieves the Merkle root for a given file hash from the backend.
   * Used for Proof of Storage challenge setup.
   */
  async getMerkleRoot(fileHash: string): Promise<string | null> {
    try {
      const root = await invoke<string | null>("get_merkle_root_for_file", {
        fileHash,
      });
      return root ?? null;
    } catch (error) {
      console.error("Failed to get Merkle root:", error);
      return null;
    }
  }

  /**
   * Downloads a file from the network given its hash.
   * Uses the configured download path from settings.
   * @param hash The hash of the file to download.
   * @param fileName The name to save the file as.
   * @returns The full path to the downloaded file.
   */
  async downloadFile(hash: string, fileName: string): Promise<string> {
    // Get the canonical download directory from backend (single source of truth)
    const downloadDir = await invoke<string>("get_download_directory");

    // Ensure directory exists (create it if it doesn't)
    await invoke("ensure_directory_exists", { path: downloadDir });

    // Construct full file path using join for proper path handling
    const outputPath = await join(downloadDir, fileName);

    console.log("✅ Starting download to:", outputPath);

    // Call the backend with the validated path
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
