/**
 * Upload Service
 *
 * Centralized service for handling file uploads across different protocols.
 * Follows DRY principle and provides clean separation of concerns.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Protocol } from "./contentProtocols/types";

// ============================================================================
// Types
// ============================================================================
export interface FtpConfig {
  url: string;
  username?: string;
  password?: string;
  useFtps: boolean;
  passiveMode: boolean;
}

export interface FtpUploadResult {
  fileHash: string;
  ftpUrl: string;
  ftpSource: {
    url: string;
    username?: string;
    encryptedPassword?: string;
    passiveMode: boolean;
    useFtps: boolean;
    timeoutSecs?: number;
    supportsResume: boolean;
    fileSize: number;
    lastChecked?: number;
    isAvailable: boolean;
  };
}

export interface WebRTCUploadResult {
  fileHash: string;
  filePath: string;
}

export interface FileHashingProgress {
  filePath: string;
  bytesHashed: number;
  totalBytes: number;
  percent: number;
}

export interface UploadOptions {
  protocol: Protocol;
  filePath: string;
  pricePerMb: number;
  ftpConfig?: FtpConfig;
  onHashingProgress?: (progress: FileHashingProgress) => void;
}

export interface UploadResult {
  success: boolean;
  fileHash?: string;
  protocolHash?: string;
  error?: string;
  protocol: Protocol;
}

// ============================================================================
// Upload Handlers
// ============================================================================

/**
 * Upload file via FTP using the new ProtocolManager architecture
 */
export async function uploadViaFtp(
  filePath: string,
  pricePerMb: number,
  ftpConfig: FtpConfig,
  onHashingProgress?: (progress: FileHashingProgress) => void,
): Promise<FtpUploadResult> {
  // Listen for hashing progress events
  let unlisten: UnlistenFn | null = null;

  if (onHashingProgress) {
    unlisten = await listen<FileHashingProgress>(
      "file_hashing_progress",
      (event) => {
        if (event.payload.filePath === filePath) {
          onHashingProgress(event.payload);
        }
      },
    );
  }

  try {
    const result = await invoke<FtpUploadResult>("upload_via_ftp", {
      filePath,
      ftpUrl: ftpConfig.url,
      username: ftpConfig.username || null,
      password: ftpConfig.password || null,
      useFtps: ftpConfig.useFtps,
      passiveMode: ftpConfig.passiveMode,
      pricePerMb,
    });

    return result;
  } finally {
    // Clean up event listener
    if (unlisten) {
      unlisten();
    }
  }
}

/**
 * Upload file via WebRTC using the new ProtocolManager architecture
 */
export async function uploadViaWebRTC(
  filePath: string,
  pricePerMb: number,
  onHashingProgress?: (progress: FileHashingProgress) => void,
): Promise<WebRTCUploadResult> {
  // Listen for hashing progress events
  let unlisten: UnlistenFn | null = null;

  if (onHashingProgress) {
    unlisten = await listen<FileHashingProgress>(
      "file_hashing_progress",
      (event) => {
        if (event.payload.filePath === filePath) {
          onHashingProgress(event.payload);
        }
      },
    );
  }

  try {
    const result = await invoke<WebRTCUploadResult>("upload_via_webrtc", {
      filePath,
      pricePerMb,
    });

    return result;
  } finally {
    // Clean up event listener
    if (unlisten) {
      unlisten();
    }
  }
}

/**
 * Upload file via BitTorrent (legacy - uses existing command)
 */
export async function uploadViaBitTorrent(
  filePath: string,
): Promise<{ magnetLink: string }> {
  const magnetLink = await invoke<string>("create_and_seed_torrent", {
    filePath,
  });

  return { magnetLink };
}

/**
 * Upload file via legacy protocols (HTTP, Bitswap)
 * Uses the old upload_file_to_network command for backward compatibility
 */
export async function uploadViaLegacyProtocol(
  filePath: string,
  price: number,
  protocol: Protocol,
): Promise<{ merkleRoot: string }> {
  // Copy file to temp location to prevent original from being moved
  const tempFilePath = await invoke<string>("copy_file_to_temp", {
    filePath,
  });

  // Extract original filename
  const originalFileName = filePath.split(/[/\\]/).pop() || "unknown";

  // Use legacy DHT service
  const { dhtService } = await import("$lib/dht");
  const metadata = await dhtService.publishFileToNetwork(
    tempFilePath,
    price,
    protocol,
    originalFileName,
  );

  return { merkleRoot: metadata.merkleRoot || "" };
}

// ============================================================================
// Unified Upload Function
// ============================================================================

/**
 * Upload a file using the specified protocol
 *
 * This is the main entry point for file uploads. It handles:
 * - Protocol-specific upload logic
 * - Progress tracking
 * - Error handling
 * - Toast notifications
 *
 * @param options - Upload configuration options
 * @returns UploadResult with success status and file hash
 */
export async function uploadFile(
  options: UploadOptions,
): Promise<UploadResult> {
  const { protocol, filePath, pricePerMb, ftpConfig, onHashingProgress } =
    options;

  try {
    switch (protocol) {
      case Protocol.FTP: {
        if (!ftpConfig) {
          throw new Error("FTP configuration is required for FTP uploads");
        }

        const result = await uploadViaFtp(
          filePath,
          pricePerMb,
          ftpConfig,
          onHashingProgress,
        );

        return {
          success: true,
          fileHash: result.fileHash,
          protocolHash: result.ftpUrl,
          protocol,
        };
      }

      case Protocol.WebRTC: {
        const result = await uploadViaWebRTC(
          filePath,
          pricePerMb,
          onHashingProgress,
        );

        return {
          success: true,
          fileHash: result.fileHash,
          protocolHash: result.fileHash,
          protocol,
        };
      }

      case Protocol.BitTorrent: {
        const result = await uploadViaBitTorrent(filePath);

        return {
          success: true,
          fileHash: result.magnetLink,
          protocolHash: result.magnetLink,
          protocol,
        };
      }

      case Protocol.HTTP: {
        // Get file size to calculate total price
        const fileSize = await invoke<number>("get_file_size", { filePath });
        const totalPrice = (fileSize / (1024 * 1024)) * pricePerMb;

        const result = await uploadViaLegacyProtocol(
          filePath,
          totalPrice,
          protocol,
        );

        return {
          success: true,
          fileHash: result.merkleRoot,
          protocolHash: result.merkleRoot,
          protocol,
        };
      }
      // to be implemented
      case Protocol.ED2K: {
        throw new Error(`Unsupported protocol: ${protocol}`);
      }

      default: {
        throw new Error(`Unsupported protocol: ${protocol}`);
      }
    }
  } catch (error) {
    console.error(`[UploadService] Upload failed for ${protocol}:`, error);

    return {
      success: false,
      error: error instanceof Error ? error.message : String(error),
      protocol,
    };
  }
}

// ============================================================================
// Validation Helpers
// ============================================================================

/**
 * Validate upload prerequisites
 */
export async function validateUploadPrerequisites(): Promise<{
  valid: boolean;
  error?: string;
}> {
  try {
    // Check if running in Tauri
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return { valid: false, error: "Not running in Tauri environment" };
    }

    // Check if user has an active account
    const hasAccount = await invoke<boolean>("has_active_account");
    if (!hasAccount) {
      return {
        valid: false,
        error: "Please log in to your account before uploading files",
      };
    }

    // Check if DHT is connected
    const isDhtRunning = await invoke<boolean>("is_dht_running");
    if (!isDhtRunning) {
      return {
        valid: false,
        error:
          "DHT network is not connected. Please start the DHT network before uploading files.",
      };
    }

    return { valid: true };
  } catch (error) {
    console.error("[UploadService] Validation failed:", error);
    return {
      valid: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

/**
 * Calculate file price based on size and price per MB
 */
export async function calculateFilePrice(
  filePath: string,
  pricePerMb: number,
): Promise<number> {
  const fileSize = await invoke<number>("get_file_size", { filePath });
  const fileSizeMB = fileSize / (1024 * 1024);
  return fileSizeMB * pricePerMb;
}
