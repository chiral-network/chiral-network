<script lang="ts">
  import Card from "$lib/components/ui/card.svelte";
  import Badge from "$lib/components/ui/badge.svelte";
  import DropDown from "$lib/components/ui/dropDown.svelte";
  import {
    File as FileIcon,
    X,
    Plus,
    FolderOpen,
    FileText,
    Image,
    Music,
    Video,
    Archive,
    Code,
    FileSpreadsheet,
    Upload,
    Download,
    RefreshCw,
    Lock,
    Key,
     Copy,
     Share2,
    Globe,
    Network,
    Server,
  } from "lucide-svelte";
  import { files, coalescedFiles, type FileItem } from "$lib/stores";
  import {
    loadSeedList,
    saveSeedList,
    clearSeedList,
    type SeedRecord,
  } from "$lib/services/seedPersistence";
  import { t } from "svelte-i18n";
  import { get } from "svelte/store";
  import { onMount, onDestroy } from "svelte";
  import { showToast } from "$lib/toast";
  import { getStorageStatus } from "$lib/uploadHelpers";
  import { fileService } from "$lib/services/fileService";
  import { extractInfoHashFromMagnet } from "$lib/utils/magnetInfoHash";
  import { toHumanReadableSize } from "$lib/utils";
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { dhtService } from "$lib/dht";
  import Label from "$lib/components/ui/label.svelte";
  import Input from "$lib/components/ui/input.svelte";
  import FTPUploadConfig from "$lib/components/upload/FTPUploadConfig.svelte";
  import { settings } from "$lib/stores";
  import { paymentService } from '$lib/services/paymentService';
  import { getCurrentWindow } from "@tauri-apps/api/window";
  const tr = (k: string, params?: Record<string, any>): string =>
    $t(k, params);

  // Check if running in Tauri environment
  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  // Enhanced file type detection with icons
  function getFileIcon(fileName: string) {
    const ext = fileName.split(".").pop()?.toLowerCase() || "";

    // Imageso
    if (
      ["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico"].includes(ext)
    )
      return Image;
    // Videos
    if (["mp4", "avi", "mkv", "mov", "wmv", "webm", "flv", "m4v"].includes(ext))
      return Video;
    // Audio
    if (["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma"].includes(ext))
      return Music;
    // Archives
    if (["zip", "rar", "7z", "tar", "gz", "bz2", "xz"].includes(ext))
      return Archive;
    // Code files
    if (
      [
        "js",
        "ts",
        "html",
        "css",
        "py",
        "java",
        "cpp",
        "c",
        "php",
        "rb",
        "go",
        "rs",
      ].includes(ext)
    )
      return Code;
    // Documents
    if (["txt", "md", "pdf", "doc", "docx", "rtf"].includes(ext))
      return FileText;
    // Spreadsheets
    if (["xls", "xlsx", "csv", "ods"].includes(ext)) return FileSpreadsheet;

    return FileIcon;
  }

  function getFileColor(fileName: string) {
    const ext = fileName.split(".").pop()?.toLowerCase() || "";

    if (
      ["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico"].includes(ext)
    )
      return "text-blue-500";
    if (["mp4", "avi", "mkv", "mov", "wmv", "webm", "flv", "m4v"].includes(ext))
      return "text-purple-500";
    if (["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma"].includes(ext))
      return "text-green-500";
    if (["zip", "rar", "7z", "tar", "gz", "bz2", "xz"].includes(ext))
      return "text-orange-500";
    if (
      [
        "js",
        "ts",
        "html",
        "css",
        "py",
        "java",
        "cpp",
        "c",
        "php",
        "rb",
        "go",
        "rs",
      ].includes(ext)
    )
      return "text-red-500";
    if (["txt", "md", "pdf", "doc", "docx", "rtf"].includes(ext))
      return "text-gray-600";
    if (["xls", "xlsx", "csv", "ods"].includes(ext)) return "text-emerald-500";

    return "text-muted-foreground";
  }

  // Helper function to check if DHT is connected (consistent with Network.svelte)
  async function isDhtConnected(): Promise<boolean> {
    if (!isTauri) return false;

    try {
      const isRunning = await invoke<boolean>("is_dht_running").catch(
        () => false,
      );
      return isRunning;
    } catch {
      return false;
    }
  }

  let isDragging = false;
  const LOW_STORAGE_THRESHOLD = 5;
  let availableStorage: number | null = null;
  let storageStatus: "unknown" | "ok" | "low" = "unknown";
  let isRefreshingStorage = false;
  let storageError: string | null = null;
  let lastChecked: Date | null = null;
  let isUploading = false;

  // Client mode detection
  let isClientMode = false;
  let clientModeReason: "forced" | "nat" | null = null;

  // Protocol selection state - initialize with WebRTC as default
  let selectedProtocol = $settings.selectedProtocol || "WebRTC";

  // Ensure settings store always has a valid protocol (defensive fix)
  $: if (!$settings.selectedProtocol) {
    settings.update(s => ({ ...s, selectedProtocol: "WebRTC" }));
    selectedProtocol = "WebRTC";
  }

  // Sync selectedProtocol changes back to settings
  $: if (selectedProtocol && selectedProtocol !== $settings.selectedProtocol) {
    settings.update(s => ({ ...s, selectedProtocol }));
  }

  // Encrypted sharing state
  let useEncryptedSharing = false;
  let recipientPublicKeys: Record<string, string>[] = [];
  let recipientPublicKeyInput = "";
  let editingRecipientIndex = -1;
  let showEncryptionOptions = false;

  // FTP upload configuration state
  let ftpUrl = '';
  let ftpUsername = '';
  let ftpPassword = '';
  let ftpUseFTPS = false;
  let ftpPassiveMode = true;

  // Calculate price using dynamic network metrics with safe fallbacks
  async function uploadFileStreamingToDisk(file: File) {
    const CHUNK_SIZE = 1024 * 1024; // 1MB chunks
    const totalChunks = Math.ceil(file.size / CHUNK_SIZE);

    try {
      // Create a temporary file path for streaming upload to disk
      const tempFilePath = await invoke<string>(
        "create_temp_file_for_streaming",
        {
          fileName: file.name,
        },
      );

      // Stream file to disk in chunks
      for (let chunkIndex = 0; chunkIndex < totalChunks; chunkIndex++) {
        const start = chunkIndex * CHUNK_SIZE;
        const end = Math.min(start + CHUNK_SIZE, file.size);
        const chunk = file.slice(start, end);

        const buffer = await chunk.arrayBuffer();
        const chunkData = Array.from(new Uint8Array(buffer));

        // Append chunk to temp file
        await invoke("append_chunk_to_temp_file", {
          tempFilePath,
          chunkData,
        });
      }

      return tempFilePath;
    } catch (error) {
      console.error("Streaming upload to disk failed:", error);
      throw error;
    }
  }

  async function calculateFilePrice(sizeInBytes: number): Promise<number> {
    const sizeInMB = sizeInBytes / 1_048_576; // Convert bytes to MB

    try {
      const dynamicPrice =
        await paymentService.calculateDownloadCost(sizeInBytes);
      if (Number.isFinite(dynamicPrice) && dynamicPrice > 0) {
        return Number(dynamicPrice.toFixed(8));
      }
    } catch (error) {
      console.warn(
        "Dynamic price calculation failed, falling back to static rate:",
        error,
      );
    }

    try {
      const pricePerMb = await paymentService.getDynamicPricePerMB(1.2);
      if (Number.isFinite(pricePerMb) && pricePerMb > 0) {
        return Number((sizeInMB * pricePerMb).toFixed(8));
      }
    } catch (secondaryError) {
      console.warn("Secondary dynamic price lookup failed:", secondaryError);
    }

    const fallbackPricePerMb = 0.001;
    return Number((sizeInMB * fallbackPricePerMb).toFixed(8));
  }

  $: storageLabel = isRefreshingStorage
    ? tr("upload.storage.checking")
    : availableStorage !== null
      ? tr("upload.storage.available", {
          values: { space: availableStorage.toLocaleString() },
        })
      : tr("upload.storage.unknown");

  $: storageBadgeClass =
    storageStatus === "low"
      ? "bg-red-500 text-white border-red-500"
      : storageStatus === "ok"
        ? "bg-green-500 text-white border-green-500"
        : "bg-gray-500 text-white border-gray-500";

  $: storageBadgeText =
    storageStatus === "low"
      ? tr("upload.storage.lowBadge")
      : storageStatus === "ok"
        ? tr("upload.storage.okBadge")
        : tr("upload.storage.unknownBadge");

  $: lastCheckedLabel = lastChecked
    ? tr("upload.storage.lastChecked", {
        values: {
          time: new Intl.DateTimeFormat(undefined, {
            hour: "2-digit",
            minute: "2-digit",
            timeZoneName: "short",
          }).format(lastChecked),
        },
      })
    : null;

  $: showLowStorageDescription =
    storageStatus === "low" && !isRefreshingStorage;

  async function refreshAvailableStorage() {
    if (isRefreshingStorage) return;
    isRefreshingStorage = true;
    storageError = null;

    const startTime = Date.now();

    try {
      const timeoutPromise = new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error("Storage check timeout")), 3000),
      );

      const storagePromise = fileService
        .getAvailableStorage()
        .catch((error) => {
          console.warn("Storage service error:", error);
          return null;
        });

      const result = (await Promise.race([storagePromise, timeoutPromise])) as
        | number
        | null;

      storageStatus = getStorageStatus(result, LOW_STORAGE_THRESHOLD);

      if (result === null || result === undefined || !Number.isFinite(result)) {
        storageError = "Unable to check disk space";
        availableStorage = null;
        lastChecked = null;
        storageStatus = "unknown";
      } else {
        availableStorage = Math.max(0, Math.floor(result));
        storageError = null;
        lastChecked = new Date();
      }
    } catch (error) {
      console.error("Storage check failed:", error);
      storageError =
        error instanceof Error && error.message.includes("timeout")
          ? "Storage check timed out"
          : "Unable to check disk space";
      availableStorage = null;
      lastChecked = null;
      storageStatus = "unknown";
    } finally {
      const elapsed = Date.now() - startTime;
      const minDelay = 600;
      if (elapsed < minDelay) {
        await new Promise((resolve) => setTimeout(resolve, minDelay - elapsed));
      }
      isRefreshingStorage = false;
    }
  }

  // Check if node is in client mode (cannot seed)
  async function checkClientMode() {
    try {
      // Check if pure client mode is forced
      if ($settings.pureClientMode) {
        isClientMode = true;
        clientModeReason = "forced";
        return;
      }

      // Check DHT health to see if behind NAT
      const health = await dhtService.getHealth();
      if (health) {
        if (health.reachability === "private") {
          isClientMode = true;
          clientModeReason = "nat";
        } else if (health.reachability === "public") {
          isClientMode = false;
          clientModeReason = null;
        }
      }
    } catch (error) {
      console.error("Failed to check client mode:", error);
    }
  }

  // Map to track active WebRTC sessions with downloaders
  let activeSeederSessions = new Map<string, any>();
  let signalingService: any = null;
  let unlisten: (() => void) | null = null;
  onMount(async () => {
    // Check if in client mode
    await checkClientMode();


    // Initialize WebRTC seeder to accept download requests
    try {
      const { SignalingService } = await import(
        "$lib/services/signalingService"
      );

      signalingService = new SignalingService({
        preferDht: true,  // Prefer DHT for signaling in desktop app
        persistPeers: false  // Don't persist peers to avoid stale peer IDs
      });

      // Connect to signaling server
      await signalingService.connect();

      // Expose for debugging
      if (typeof window !== "undefined") {
        (window as any).uploadSignalingService = signalingService;
      }

      // Listen for incoming WebRTC connection requests
      // Don't use webrtcService - handle WebRTC directly to avoid handler conflicts
      signalingService.setOnMessage(async (message: any) => {
        console.log("[Upload] Received signaling message:", message);

        if (message.type === "offer") {
          console.log("[Upload] Received download request from:", message.from);

          // Check if we already have a session with this peer
          if (activeSeederSessions.has(message.from)) {
            console.log(
              "[Upload] Already have session with peer:",
              message.from,
            );
            // Still handle the new offer - create new peer connection
            const oldSession = activeSeederSessions.get(message.from);
            try {
              oldSession.pc?.close();
            } catch (e) {
              console.error("[Upload] Error closing old session:", e);
            }
            activeSeederSessions.delete(message.from);
          }

          try {
            // Create RTCPeerConnection directly (not using webrtcService to avoid handler conflicts)
            const pc = new RTCPeerConnection({
              iceServers: [
                { urls: "stun:stun.l.google.com:19302" },
                { urls: "stun:global.stun.twilio.com:3478" },
              ],
            });

            let dataChannel: RTCDataChannel | null = null;

            // Handle incoming data channel (created by initiator)
            pc.ondatachannel = (event) => {
              console.log("[Upload] Data channel received");
              dataChannel = event.channel;

              dataChannel.onopen = () => {
                console.log(
                  "[Upload] Data channel opened with downloader:",
                  message.from,
                );
              };

              dataChannel.onclose = () => {
                console.log(
                  "[Upload] Data channel closed with downloader:",
                  message.from,
                );
                activeSeederSessions.delete(message.from);
              };

              dataChannel.onerror = (e) => {
                console.error("[Upload] Data channel error:", e);
              };

              dataChannel.onmessage = async (event) => {
                const data = event.data;
                console.log("[Upload] Received message from downloader:", data);

                // Handle file chunk requests
                if (typeof data === "string") {
                  try {
                    const request = JSON.parse(data);

                    // Handle chunk_request
                    if (request.type === "chunk_request") {
                      const fileHash = request.fileHash;
                      const chunkIndex = request.chunkIndex;

                      const currentFiles = get(files);
                      const requestedFile = currentFiles.find(
                        (f) => f.hash === fileHash,
                      );

                      if (requestedFile && requestedFile.path) {
                        console.log(
                          `[Upload] Sending chunk ${chunkIndex} for file ${requestedFile.name}`,
                        );

                        try {
                          const { readFile } = await import(
                            "@tauri-apps/plugin-fs"
                          );
                          const fileData = await readFile(requestedFile.path);

                          const CHUNK_SIZE = 16 * 1024;
                          const start = chunkIndex * CHUNK_SIZE;
                          const end = Math.min(
                            start + CHUNK_SIZE,
                            fileData.length,
                          );
                          const chunk = fileData.slice(start, end);

                          dataChannel?.send(chunk.buffer);
                          console.log(
                            `[Upload] Sent chunk ${chunkIndex} (${chunk.length} bytes)`,
                          );
                        } catch (error) {
                          console.error(
                            "[Upload] Error reading file chunk:",
                            error,
                          );
                        }
                      } else {
                        console.error(
                          "[Upload] Requested file not found or no path:",
                          fileHash,
                        );
                      }
                    }
                  } catch (e) {
                    console.error("[Upload] Error handling message:", e);
                  }
                }
              };
            };

            // Handle ICE candidates
            pc.onicecandidate = (event) => {
              if (event.candidate) {
                signalingService.send({
                  type: "candidate",
                  candidate: event.candidate.toJSON(),
                  to: message.from,
                });
              }
            };

            pc.onconnectionstatechange = () => {
              console.log("[Upload] Connection state:", pc.connectionState);
              if (
                pc.connectionState === "failed" ||
                pc.connectionState === "disconnected" ||
                pc.connectionState === "closed"
              ) {
                activeSeederSessions.delete(message.from);
              }
            };

            // Accept the offer and create answer
            await pc.setRemoteDescription(message.sdp);
            const answer = await pc.createAnswer();
            await pc.setLocalDescription(answer);

            // Send answer
            signalingService.send({
              type: "answer",
              sdp: answer,
              to: message.from,
            });
            console.log("[Upload] Sent answer to:", message.from);

            // Store session
            activeSeederSessions.set(message.from, { pc, dataChannel });
          } catch (error) {
            console.error("[Upload] Failed to create WebRTC session:", error);
          }
        } else if (message.type === "candidate") {
          // Handle incoming ICE candidates
          const session = activeSeederSessions.get(message.from);
          if (session && session.pc) {
            try {
              await session.pc.addIceCandidate(message.candidate);
              console.log("[Upload] Added ICE candidate from:", message.from);
            } catch (e) {
              console.error("[Upload] Error adding ICE candidate:", e);
            }
          }
        }
      });
    } catch (error) {
      console.error("[Upload] Failed to initialize WebRTC seeder:", error);
    }

    // Make storage refresh non-blocking on startup to prevent UI hanging
    setTimeout(() => refreshAvailableStorage(), 100);

    // Clear persisted seed list on startup to prevent ghost files from other nodes
    try {
      await clearSeedList();
    } catch (e) {
      console.warn("Failed to clear persisted seed list", e);
    }

    // Restore persisted seeding list (if any)
    try {
      const persisted: SeedRecord[] = await loadSeedList();
      if (persisted && persisted.length > 0) {
        const existing = get(files);
        const toAdd: FileItem[] = [];
        for (const s of persisted) {
          if (!existing.some((f) => f.hash === s.hash)) {
            toAdd.push({
              id: s.id,
              name: s.name || s.path.split(/[\\/]/).pop() || s.hash,
              path: s.path,
              hash: s.hash,
              size: s.size || 0,
              status: "seeding",
              seeders: 1,
              leechers: 0,
              uploadDate: s.addedAt ? new Date(s.addedAt) : new Date(),
              isEncrypted: false,
              manifest: s.manifest ?? null,
              price: s.price ?? 0,
            });
          }
        }
        if (toAdd.length > 0) {
          files.update((curr) => [...curr, ...toAdd]);
        }
      }
    } catch (e) {
      console.warn("Failed to restore persisted seed list", e);
    }

    // HTML5 Drag and Drop functionality
    const dropZone = document.querySelector(".drop-zone") as HTMLElement;

    if (dropZone) {
      const handleDragOver = (e: DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        if (isClientMode) {
          e.dataTransfer!.dropEffect = "none";
        } else {
          e.dataTransfer!.dropEffect = "copy";
          isDragging = true;
        }
      };

      const handleDragEnter = (e: DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        if (isClientMode) {
          e.dataTransfer!.dropEffect = "none";
        } else {
          e.dataTransfer!.dropEffect = "copy";
          isDragging = true;
        }
      };

      const handleDragLeave = (e: DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        if (e.currentTarget && !dropZone.contains(e.relatedTarget as Node)) {
          isDragging = false;
        }
      };

      const handleDragEnd = (_e: DragEvent) => {
        isDragging = false;
      };

      const handleDrop = async (e: DragEvent) => {
        isDragging = false;

        // Check if in client mode
        if (isClientMode) {
          showToast(
            "File sharing is disabled in client-only mode",
            "warning",
          );
          return;
        }

        // IMPORTANT: Extract files immediately before any async operations
        // dataTransfer.files becomes empty after the event completes
        const droppedFiles = Array.from(e.dataTransfer?.files || []);

        // STEP 1: Verify backend has active account before proceeding
        if (isTauri) {
          try {
            const hasAccount = await invoke<boolean>("has_active_account");
            if (!hasAccount) {
              showToast(
                // "Please log in to your account before uploading files",
                tr("toasts.upload.loginRequired"),
                "error",
              );
              return;
            }
          } catch (error) {
            console.error("Failed to verify account status:", error);
            showToast(
              // "Failed to verify account status. Please try logging in again.",
              tr("toasts.upload.verifyAccountFailed"),
              "error",
            );
            return;
          }
        }

        if (isUploading) {
          showToast(tr("upload.uploadInProgress"), "warning");
          return;
        }

        // STEP 2: Ensure DHT is connected before attempting upload
        const dhtConnected = await isDhtConnected();
        if (!dhtConnected) {
          showToast(
            // "DHT network is not connected. Please start the DHT network before uploading files.",
            tr("toasts.upload.dhtDisconnected"),
            "error",
          );
          return;
        }

        if (droppedFiles.length > 0) {
          if (!isTauri) {
            showToast(tr("upload.desktopOnly"), "error");
            return;
          }

          try {
            isUploading = true;
            let duplicateCount = 0;
            let addedCount = 0;
            let blockedCount = 0;

            // Process files sequentially (unified flow for all protocols)
            for (const file of droppedFiles) {
              const blockedExtensions = [
                ".exe",
                ".bat",
                ".cmd",
                ".com",
                ".msi",
                ".scr",
                ".vbs",
              ];
              const fileName = file.name.toLowerCase();
              if (blockedExtensions.some((ext) => fileName.endsWith(ext))) {
                showToast(
                  tr("upload.executableBlocked", {
                    values: { name: file.name },
                  }),
                  "error",
                );
                blockedCount++;
                continue;
              }

              if (file.size === 0) {
                showToast(
                  tr("upload.emptyFile", { values: { name: file.name } }),
                  "error",
                );
                blockedCount++;
                continue;
              }

              // Check if node is in pure-client mode (cannot seed files)
              if ($settings.pureClientMode) {
                showToast(
                  tr("toasts.upload.pureClientMode"),
                  "error",
                );
                blockedCount++;
                continue;
              }

              try {
                let metadata;
                const filePrice = await calculateFilePrice(file.size);

                // Use streaming upload for all protocols to avoid memory issues with large files
                // All protocol handlers read from file paths on disk
                const tempFilePath = await uploadFileStreamingToDisk(file);
                metadata = await dhtService.publishFileToNetwork(
                  tempFilePath,
                  filePrice,
                  selectedProtocol,
                  file.name,
                );

                // Check for same content + same protocol (true duplicate)
                if (
                  get(files).some(
                    (f) =>
                      f.hash === metadata.merkleRoot &&
                      f.protocol === selectedProtocol,
                  )
                ) {
                  duplicateCount++;
                  showToast(
                    tr("upload.duplicateSkipped", { values: { count: 1 } }),
                    "warning",
                  );
                  continue;
                }

                // Construct protocol-specific hash for display
                let protocolHash = metadata.merkleRoot || "";
                if (selectedProtocol === "BitTorrent" && metadata.infoHash) {
                  // Construct magnet link for BitTorrent
                  const trackers = metadata.trackers
                    ? metadata.trackers.join("&tr=")
                    : "udp://tracker.openbittorrent.com:80";
                  protocolHash = `magnet:?xt=urn:btih:${metadata.infoHash}&tr=${trackers}`;
                }

                const newFile = {
                  id: `file-${Date.now()}-${Math.random()}`,
                  name: metadata.fileName,
                  path: file.name,
                  hash: metadata.merkleRoot || "",
                  protocolHash,
                  size: metadata.fileSize,
                  status: "seeding" as const,
                  seeders: metadata.seeders?.length ?? 0,
                  seederAddresses: metadata.seeders ?? [],
                  leechers: 0,
                  uploadDate: new Date(metadata.createdAt),
                  price: filePrice,
                  cids: metadata.cids,
                  protocol: selectedProtocol,
                };

                files.update((currentFiles) => [...currentFiles, newFile]);
                addedCount++;
                
                // Show success with propagation timing info
                const baseMessage = tr("toasts.upload.fileSuccess", {
                  values: { name: file.name },
                });
                const propagationHint = " File will be searchable in 60-90 seconds after DHT propagation.";
                showToast(
                  baseMessage + propagationHint,
                  "success",
                  6000
                );
              } catch (error) {
                console.error(
                  "Error uploading dropped file:",
                  file.name,
                  error,
                );
                showToast(
                  tr("upload.fileFailed", {
                    values: { name: file.name, error: String(error) },
                  }),
                  "error",
                );
              }
            }

            if (duplicateCount > 0) {
              showToast(
                tr("upload.duplicateSkipped", {
                  values: { count: duplicateCount },
                }),
                "warning",
              );
            }

            // Refresh storage after uploads
            if (addedCount > 0) {
              setTimeout(() => refreshAvailableStorage(), 100);
            }
          } catch (error) {
            console.error("Error handling dropped files:", error);
            showToast(tr("upload.uploadError"), "error");
          } finally {
            isUploading = false;
          }
        }
      };

      dropZone.addEventListener("dragenter", handleDragEnter);
      dropZone.addEventListener("dragover", handleDragOver);
      dropZone.addEventListener("dragleave", handleDragLeave);
      dropZone.addEventListener("drop", handleDrop);

      const preventDefaults = (e: Event) => {
        e.preventDefault();
        e.stopPropagation();
      };

      window.addEventListener("dragover", preventDefaults);
      window.addEventListener("drop", preventDefaults);

      document.addEventListener("dragend", handleDragEnd);
      document.addEventListener("drop", handleDragEnd);

      (window as any).dragDropCleanup = () => {
        dropZone.removeEventListener("dragenter", handleDragEnter);
        dropZone.removeEventListener("dragover", handleDragOver);
        dropZone.removeEventListener("dragleave", handleDragLeave);
        dropZone.removeEventListener("drop", handleDrop);
        window.removeEventListener("dragover", preventDefaults);
        window.removeEventListener("drop", preventDefaults);
        document.removeEventListener("dragend", handleDragEnd);
        document.removeEventListener("drop", handleDragEnd);
      };
    }
    if (isTauri) {
      try {
        unlisten = await getCurrentWindow().onDragDropEvent((event) => {
          if (event.payload.type === 'over') {
             // User is dragging files over the window
            isDragging = true;
          } else if (event.payload.type === 'drop') {
             // User dropped the files
            isDragging = false;
            
             // event.payload.paths is an array of strings (absolute paths)
            const paths = event.payload.paths;
            if (paths && paths.length > 0) {
               // No need to check other conditions. addFilesFromPaths checks those conditions.
              addFilesFromPaths(paths);
            }
          } else {
             // 'leave' or cancelled
            isDragging = false;
          }
        });
      } catch (err) {
        console.error("Failed to setup Tauri drag drop listener:", err);
      }
    }
    else{
        showToast(
            tr("upload.desktopOnly"),
            "error",
          );
      }

  });

  onDestroy(() => {
    if (unlisten) {
      unlisten();
    }
  });

  let persistTimeout: ReturnType<typeof setTimeout> | null = null;
  const unsubscribeFiles = files.subscribe(($files) => {
    const seeds: SeedRecord[] = $files
      .filter((f) => f.status === "seeding" && f.path)
      .map((f) => ({
        id: f.id,
        path: f.path!,
        hash: f.hash,
        name: f.name,
        size: f.size,
        addedAt: f.uploadDate
          ? f.uploadDate.toISOString()
          : new Date().toISOString(),
        manifest: f.manifest,
        price: f.price ?? 0,
      }));

    if (persistTimeout) clearTimeout(persistTimeout);
    persistTimeout = setTimeout(() => {
      saveSeedList(seeds).catch((e) =>
        console.warn("Failed to persist seed list", e),
      );
    }, 400);
  });

  onDestroy(() => {
    unsubscribeFiles();
    if (persistTimeout) clearTimeout(persistTimeout);
  });

  async function openFileDialog() {
    // Verify backend has active account before proceeding
    if (isTauri) {
      try {
        const hasAccount = await invoke<boolean>("has_active_account");
        if (!hasAccount) {
          showToast(
            // "Please log in to your account before uploading files",
            tr("toasts.upload.loginRequired"),
            "error",
          );
          return;
        }
      } catch (error) {
        console.error("Failed to verify account status:", error);
        showToast(
          // "Failed to verify account status. Please try logging in again.",
          tr("toasts.upload.verifyAccountFailed"),
          "error",
        );
        return;
      }
    }

    if (isUploading) return;

    try {
      const selectedPaths = (await open({
        multiple: true,
      })) as string[] | null;

      if (selectedPaths && selectedPaths.length > 0) {
        isUploading = true;
        await addFilesFromPaths(selectedPaths);
      }
    } catch (e) {
      showToast(tr("upload.fileDialogError"), "error");
    } finally {
      isUploading = false;
    }
  }

  async function removeFile(contentHash: string) {
    if (!isTauri) {
      showToast(tr("upload.fileManagementDesktopOnly"), "error");
      return;
    }

    try {
      // Find all files with this content hash and stop publishing each one
      const filesToRemove = get(files).filter(
        (file) => file.hash === contentHash,
      );

      for (const file of filesToRemove) {
        try {
          await invoke("stop_publishing_file", { fileHash: file.hash });
        } catch (unpublishError) {
          console.warn("Failed to unpublish file from DHT:", unpublishError);
        }
      }

      // Remove all files with this content hash from the store
      files.update((f) => f.filter((file) => file.hash !== contentHash));

      const protocolCount = filesToRemove.length;
      showToast(
        `Stopped sharing file on ${protocolCount} protocol${protocolCount > 1 ? "s" : ""}`,
        "success",
      );
    } catch (error) {
      console.error(error);
      showToast(
        tr("upload.fileFailed", {
          values: {
            name: contentHash.slice(0, 8) + "...",
            error: String(error),
          },
        }),
        "error",
      );
    }
  }

  async function addFilesFromPaths(paths: string[]) {
    // Fallback: Force reset isUploading after 30 seconds to prevent UI from being stuck
    const forceResetTimeout = setTimeout(() => {
      console.log(`[UPLOAD] Force resetting isUploading due to timeout`);
      isUploading = false;
      showToast("Upload timed out - please try again", "error");
    }, 30000);

    // STEP 1: Verify backend has active account before proceeding
    if (isTauri) {
      try {
        const hasAccount = await invoke<boolean>("has_active_account");
        if (!hasAccount) {
          showToast(
            // "Please log in to your account before uploading files",
            tr("toasts.upload.loginRequired"),
            "error",
          );
          clearTimeout(forceResetTimeout);
          isUploading = false;
          return;
        }
      } catch (error) {
        console.error("Failed to verify account status:", error);
        showToast(
          // "Failed to verify account status. Please try logging in again.",
          tr("toasts.upload.verifyAccountFailed"),
          "error",
        );
        clearTimeout(forceResetTimeout);
        isUploading = false;
        return;
      }
    }

    // STEP 2: Ensure DHT is connected before attempting upload
    const dhtConnected = await isDhtConnected();
    if (!dhtConnected) {
      showToast(
        // "DHT network is not connected. Please start the DHT network before uploading files.",
        tr("toasts.upload.dhtDisconnected"),
        "error",
      );
      clearTimeout(forceResetTimeout);
      isUploading = false;
      return;
    }

    // STEP 3: Check if node is in pure-client mode (cannot seed files)
    if ($settings.pureClientMode) {
      showToast(
        tr("toasts.upload.pureClientMode"),
        "error",
      );
      clearTimeout(forceResetTimeout);
      isUploading = false;
      return;
    }

    let addedCount = 0;

    // Unified upload flow for all protocols
    for (const filePath of paths) {
      try {
        const fileName = filePath.replace(/^.*[\\/]/, "") || "";

        // Get file size to calculate price
        const fileSize = await invoke<number>("get_file_size", { filePath });
        const price = await calculateFilePrice(fileSize);

        // Handle BitTorrent differently - create and seed torrent
        if (selectedProtocol === "BitTorrent") {
          const magnetLink = await invoke<string>('create_and_seed_torrent', { filePath });

          const torrentFile = {
            id: `torrent-${Date.now()}-${Math.random()}`,
            name: fileName,
            hash: magnetLink, // Use magnet link as hash for torrents
            size: fileSize,
            path: filePath,
            seederAddresses: [],
            uploadDate: new Date(),
            seeders: 1,
            status: "seeding" as const,
            price: 0, // BitTorrent is free
          };

          files.update(f => [...f, torrentFile]);
          // showToast(`${fileName} is now seeding as a torrent`, "success");
          showToast(
            tr('toasts.upload.torrentSeeding', { values: { name: fileName } }),
            "success"
          );
          // continue; // Skip the normal Chiral upload flow
        }

        // Handle FTP upload to external server
        if (selectedProtocol === "FTP" && ftpUrl) {
          try {
            // Show upload in progress toast
            showToast(
              `Uploading ${fileName} to FTP server...`,
              "info"
            );

            // Upload file to external FTP server
            const uploadedUrl = await invoke<string>('upload_to_external_ftp', {
              filePath,
              ftpUrl,
              username: ftpUsername || null,
              password: ftpPassword || null,
              useFtps: ftpUseFTPS,
              passiveMode: ftpPassiveMode,
            });

            // Create file entry with FTP URL
            const ftpFile = {
              id: `ftp-${Date.now()}-${Math.random()}`,
              name: fileName,
              hash: uploadedUrl, // Use FTP URL as hash
              protocolHash: uploadedUrl,
              size: fileSize,
              path: filePath,
              seederAddresses: [],
              uploadDate: new Date(),
              seeders: 1,
              status: "seeding" as const,
              price: 0, // FTP is free
              protocol: "FTP" as const,
            };

            files.update(f => [...f, ftpFile]);
            showToast(
              `${fileName} uploaded to FTP server successfully`,
              "success"
            );
            addedCount++;
            continue; // Skip the normal Chiral upload flow
          } catch (error) {
            console.error("FTP upload failed:", error);
            showToast(
              `FTP upload failed: ${error}`,
              "error"
            );
            continue;
          }
        }

        // Copy file to temp location to prevent original file from being moved
        const tempFilePath = await invoke<string>("copy_file_to_temp", {
          filePath,
        });

        // Extract original filename from the file path
        const originalFileName = filePath.split(/[/\\]/).pop() || filePath;

        const metadata = await dhtService.publishFileToNetwork(
          tempFilePath,
          price,
          selectedProtocol,
          originalFileName,
        );

        // Use seeders from metadata (backend already adds local peer ID via heartbeat system)
        // Only add WebSocket client ID if no seeders exist (shouldn't happen in normal flow)
        const allSeederAddresses = metadata.seeders && metadata.seeders.length > 0
          ? metadata.seeders
          : (signalingService?.clientId ? [signalingService.clientId] : []);

        // Construct protocol-specific hash for display
        let protocolHash = metadata.merkleRoot || "";
        if ((selectedProtocol as "WebRTC" | "BitTorrent" | "ED2K" | "FTP") === "BitTorrent" && metadata.infoHash) {
          // Construct magnet link for BitTorrent
          const trackers = metadata.trackers
            ? metadata.trackers.join("&tr=")
            : "udp://tracker.openbittorrent.com:80";
          protocolHash = `magnet:?xt=urn:btih:${metadata.infoHash}&tr=${trackers}`;
        } else if (
          selectedProtocol === "ED2K" &&
          metadata.ed2kSources &&
          metadata.ed2kSources.length > 0
        ) {
          // Use the first ED2K source
          const ed2kSource = metadata.ed2kSources[0];
          protocolHash = `ed2k://|file|${metadata.fileName}|${metadata.fileSize}|${ed2kSource.file_hash}|/`;
        } else if (
          selectedProtocol === "FTP" &&
          metadata.ftpSources &&
          metadata.ftpSources.length > 0
        ) {
          // Use the first FTP source
          protocolHash = metadata.ftpSources[0].url;
        }

        const newFile = {
          id: `file-${Date.now()}-${Math.random()}`,
          name: metadata.fileName,
          path: filePath,
          hash: metadata.merkleRoot || "",
          protocolHash,
          size: metadata.fileSize,
          status: "seeding" as const,
          seeders: metadata.seeders?.length ?? 0,
          seederAddresses: allSeederAddresses,
          leechers: 0,
          uploadDate: new Date(metadata.createdAt),
          price: price,
          cids: metadata.cids,
          protocol: selectedProtocol, // Track which protocol was used
        };

        let existed = false;
        files.update((f) => {
          const matchIndex = f.findIndex(
            (item) =>
              metadata.merkleRoot &&
              item.hash === metadata.merkleRoot &&
              item.protocol === selectedProtocol,
          );

          if (matchIndex !== -1) {
            const existing = f[matchIndex];
            // Use seeders from metadata (backend already adds local peer ID via heartbeat system)
            // Only add WebSocket client ID if no seeders exist (shouldn't happen in normal flow)
            const mergedSeederAddresses = (metadata.seeders && metadata.seeders.length > 0)
              ? metadata.seeders
              : (existing.seederAddresses && existing.seederAddresses.length > 0)
                ? existing.seederAddresses
                : (signalingService?.clientId ? [signalingService.clientId] : []);
            const updated = {
              ...existing,
              name: metadata.fileName || existing.name,
              hash: metadata.merkleRoot || existing.hash,
              size: metadata.fileSize ?? existing.size,
              seeders: metadata.seeders?.length ?? existing.seeders,
              seederAddresses: mergedSeederAddresses,
              uploadDate: new Date(
                (metadata.createdAt ??
                  existing.uploadDate?.getTime() ??
                  Date.now()) * 1000,
              ),
              status: "seeding" as const,
              price: price,
            };
            f = f.slice();
            f[matchIndex] = updated;
            existed = true;
          } else {
            f = [...f, newFile];
          }

          return f;
        });

        if (existed) {
          // File was updated, not skipped - don't count as duplicate
          showToast(
            tr("upload.fileUpdated", { values: { name: fileName } }),
            "info",
          );
        } else {
          addedCount++;
          const baseMessage = tr("toasts.upload.fileSuccess", { values: { name: fileName } });
          const propagationHint = " File will be searchable in 60-90 seconds after DHT propagation.";
          showToast(
            baseMessage + propagationHint,
            "success",
            6000
          );
        }
      } catch (error) {
        console.error(`[UPLOAD] Error uploading ${filePath}:`, error);
        showToast(
          tr("upload.fileFailed", {
            values: {
              name: filePath.replace(/^.*[\\/]/, ""),
              error: String(error),
            },
          }),
          "error",
        );
      }
    }

    if (addedCount > 0) {
      setTimeout(() => refreshAvailableStorage(), 100);
    }
    clearTimeout(forceResetTimeout);
    isUploading = false;
  }

  // Use centralized file size formatting for consistency
  const formatFileSize = toHumanReadableSize;

  // Protocol options for dropdown
  const protocolOptions = [
    { value: "WebRTC", label: "WebRTC" },
    { value: "BitTorrent", label: "BitTorrent" },
    { value: "ED2K", label: "ED2K" },
    { value: "FTP", label: "FTP" },
  ];

  async function handleCopy(hash: string) {
    await navigator.clipboard.writeText(hash);
    showToast(tr("upload.hashCopiedClipboard"), "success");
  }

  // Extract info hash from magnet link
  function extractInfoHash(magnetLink: string): string {
    // Supports both hex and base32 btih.
    // Keep this local helper returning a string for UI display.
    return extractInfoHashFromMagnet(magnetLink) ?? "unknown";
  }

  // Extract MD4 hash from ed2k link
  function extractEd2kHash(ed2kLink: string): string {
    const parts = ed2kLink.split("|");
    // ed2k://|file|name|size|hash|/
    return parts.length >= 5 ? parts[4] : "unknown";
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t("upload.title")}</h1>
    <p class="text-muted-foreground mt-2">{$t("upload.subtitle")}</p>
  </div>

  <!-- Client Mode Warning -->
  {#if isClientMode}
    <Card class="p-4 bg-yellow-50 border-yellow-200">
      <div class="flex items-start gap-3">
        <div class="text-yellow-600 mt-0.5">
          [WARN]
        </div>
        <div class="flex-1 space-y-2">
          <p class="text-sm font-semibold text-yellow-800">
            {#if clientModeReason === "forced"}
              {$t('pureClientMode.banner.title.forced')}
            {:else if clientModeReason === "nat"}
              {$t('pureClientMode.banner.title.nat')}
            {:else}
              {$t('pureClientMode.banner.title.default')}
            {/if}
          </p>
          <p class="text-sm text-yellow-700">
            {#if clientModeReason === "forced"}
              {$t('pureClientMode.warnings.forcedMode')}
            {:else if clientModeReason === "nat"}
              {$t('pureClientMode.warnings.natDetected')}
            {:else}
              {$t('pureClientMode.banner.description.default')}
            {/if}
          </p>
          <p class="text-xs text-yellow-600">
            {#if clientModeReason === "forced"}
              {$t('pureClientMode.banner.help.forced')}
            {:else if clientModeReason === "nat"}
              {$t('pureClientMode.banner.help.nat')}
            {:else}
              {$t('pureClientMode.banner.help.default')}
            {/if}
          </p>
        </div>
      </div>
    </Card>
  {/if}

  {#if isTauri}
    <Card class="p-4 flex flex-wrap items-start justify-between gap-4">
      <div class="space-y-1">
        <p class="text-sm font-semibold text-foreground">
          {$t("upload.storage.title")}
        </p>
        <p class="text-sm text-muted-foreground">{storageLabel}</p>
        {#if lastCheckedLabel}
          <p class="text-xs text-muted-foreground">{lastCheckedLabel}</p>
        {/if}
        {#if showLowStorageDescription}
          <p class="text-xs text-amber-600 dark:text-amber-400">
            {$t("upload.storage.lowDescription")}
          </p>
        {/if}
        {#if storageError}
          <p class="text-xs text-destructive">{storageError}</p>
        {/if}
      </div>
      <div class="flex items-center gap-3">
        <Badge class={`text-xs font-medium ${storageBadgeClass}`}
          >{storageBadgeText}</Badge
        >
        <button
          class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium border border-input bg-background hover:bg-muted disabled:opacity-60 disabled:cursor-not-allowed"
          on:click={() => refreshAvailableStorage()}
          disabled={isRefreshingStorage}
          aria-label={$t("upload.storage.refresh")}
        >
          <RefreshCw
            class={`h-4 w-4 mr-2 ${isRefreshingStorage ? "animate-spin" : ""}`}
          />
          {$t("upload.storage.refresh")}
        </button>
      </div>
    </Card>
  {:else}
    <Card class="p-4">
      <div class="text-center">
        <p class="text-sm font-semibold text-foreground mb-2">
          {$t("upload.desktopAppRequired")}
        </p>
        <p class="text-sm text-muted-foreground">
          {$t("upload.storageMonitoringDesktopOnly")}
        </p>
      </div>
    </Card>
  {/if}

  <!-- Upload Protocol Selection -->
  {#if isTauri}
    <Card class="p-4">
      <div class="flex items-center justify-between gap-4">
        <div class="flex items-center gap-3">
          <div
            class="flex items-center justify-center w-10 h-10 bg-gradient-to-br from-blue-500/10 to-blue-500/5 rounded-lg border border-blue-500/20"
          >
            <Upload class="h-5 w-5 text-blue-600" />
          </div>
          <div class="text-left">
            <h3 class="text-sm font-semibold text-foreground">
              Upload Protocol
            </h3>
            <p class="text-xs text-muted-foreground">
              Choose which protocol to use for uploading files
            </p>
          </div>
        </div>

        <div class="w-fit min-w-32">
          <DropDown
            id="upload-protocol"
            options={protocolOptions}
            bind:value={selectedProtocol}
            placeholder="Select Protocol"
          />
        </div>
      </div>
    </Card>
  {/if}

  <!-- FTP Server Configuration (only shown when FTP is selected) -->
  {#if isTauri && selectedProtocol === "FTP"}
    <Card class="p-4">
      <div class="flex items-center gap-3 mb-4">
        <div
          class="flex items-center justify-center w-10 h-10 bg-gradient-to-br from-gray-500/10 to-gray-500/5 rounded-lg border border-gray-500/20"
        >
          <Server class="h-5 w-5 text-gray-600" />
        </div>
        <div class="text-left">
          <h3 class="text-sm font-semibold text-foreground">
            FTP Server Configuration
          </h3>
          <p class="text-xs text-muted-foreground">
            Configure your FTP server to upload files
          </p>
        </div>
      </div>

      <FTPUploadConfig
        bind:ftpUrl
        bind:ftpUsername
        bind:ftpPassword
        bind:ftpUseFTPS
        bind:ftpPassiveMode
      />
    </Card>
  {/if}

  <!-- Encrypted Sharing Options -->
  {#if isTauri}
    <Card class="p-4">
      <button
        class="w-full flex items-center justify-between cursor-pointer hover:opacity-80 transition-opacity"
        on:click={() => (showEncryptionOptions = !showEncryptionOptions)}
      >
        <div class="flex items-center gap-3">
          <div
            class="flex items-center justify-center w-10 h-10 bg-gradient-to-br from-purple-500/10 to-purple-500/5 rounded-lg border border-purple-500/20"
          >
            <Lock class="h-5 w-5 text-purple-600" />
          </div>
          <div class="text-left">
            <h3 class="text-sm font-semibold text-foreground">
              {$t("upload.encryption.title")}
            </h3>
            <p class="text-xs text-muted-foreground">
              {$t("upload.encryption.subtitle")}
            </p>
          </div>
        </div>
        <svg
          class="h-5 w-5 text-muted-foreground transition-transform duration-200 {showEncryptionOptions
            ? 'rotate-180'
            : ''}"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M19 9l-7 7-7-7"
          />
        </svg>
      </button>

      {#if showEncryptionOptions}
        <div class="mt-4 space-y-4 pt-4 border-t border-border">
          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="use-encrypted-sharing"
              bind:checked={useEncryptedSharing}
              class="cursor-pointer"
            />
            <Label for="use-encrypted-sharing" class="cursor-pointer text-sm">
              {$t("upload.encryption.enableForRecipient")}
            </Label>
          </div>

          {#if useEncryptedSharing}
            <div class="space-y-2 pl-6">
              <div class="flex items-center gap-2">
                <Key class="h-4 w-4 text-muted-foreground" />
                <Label for="recipient-public-key" class="text-sm font-medium">
                  {$t("upload.encryption.recipientPublicKey")}
                </Label>
              </div>
              <form
                on:submit|preventDefault={() => {
                  if (recipientPublicKeyInput.trim()) {
                    const trimmedKey = recipientPublicKeyInput.trim();
                    if (/^[0-9a-fA-F]{64}$/.test(trimmedKey)) {
                      recipientPublicKeys = [
                        ...recipientPublicKeys,
                        {
                          key: trimmedKey,
                          name: `Recipient ${recipientPublicKeys.length + 1}`,
                        },
                      ];
                      recipientPublicKeyInput = "";
                    }
                  }
                }}
              >
                <Input
                  id="recipient-public-key"
                  bind:value={recipientPublicKeyInput}
                  placeholder={$t("upload.encryption.publicKeyPlaceholder")}
                  class="font-mono text-sm"
                  disabled={isUploading}
                />
              </form>
              <p class="text-xs text-muted-foreground">
                {$t("upload.encryption.publicKeyHint")}
              </p>
              {#if recipientPublicKeyInput && !/^[0-9a-fA-F]{64}$/.test(recipientPublicKeyInput.trim())}
                <p class="text-xs text-destructive">
                  {$t("upload.encryption.invalidPublicKey")}
                </p>
              {/if}
              <div class="space-y-2">
                <h4 class="text-sm font-semibold">
                  {$t("upload.encryption.recipientsList")}
                </h4>
                {#if recipientPublicKeys.length > 0}
                  <ul class="space-y-2">
                    {#each recipientPublicKeys as item, index}
                      <li class="flex items-center gap-2">
                        <span class="flex flex-col gap-1">
                            {#if editingRecipientIndex === index}
                            <input
                              bind:value={item.name}
                              class="text-sm font-medium text-gray-800 border-none outline-none text-left px-2 py-1 rounded"
                              on:blur={() => (editingRecipientIndex = -1)}
                              on:keydown={(e) => {
                              if (e.key === "Enter")
                                editingRecipientIndex = -1;
                              }}
                            />
                            {:else}
                            <button
                              type="button"
                              class="text-sm font-medium text-gray-800 border-none outline-none text-left px-2 py-1 rounded underline"
                              on:click={() => (editingRecipientIndex = index)}
                              title="Click to edit name"
                            >
                              {item.name}
                            </button>
                            {/if}
                          <span class="flex items-center gap-2">
                            <code
                              class="font-mono text-sm bg-muted/50 px-2 py-1 rounded w-[36rem]"
                              placeholder="Public key"
                            >
                              {item.key}
                            </code>
                            <button
                              class="group/btn p-1 hover:bg-destructive/10 rounded transition-colors"
                              on:click={() =>
                                (recipientPublicKeys =
                                  recipientPublicKeys.filter(
                                    (_, i) => i !== index,
                                  ))}
                              title={$t("upload.encryption.removeRecipient")}
                              aria-label={$t(
                                "upload.encryption.removeRecipient",
                              )}
                            >
                              <X
                                class="h-4 w-4 text-muted-foreground group-hover/btn:text-destructive transition-colors"
                              />
                            </button>
                            <button
                              class="group/btn p-1 hover:bg-primary/10 rounded transition-colors"
                              on:click={() =>
                                navigator.clipboard.writeText(item.key)}
                              title="Copy public key"
                              aria-label="Copy public key"
                            >
                              <Copy
                                class="h-4 w-4 text-muted-foreground group-hover/btn:text-primary transition-colors"
                              />
                            </button>
                          </span>
                        </span>
                      </li>
                    {/each}
                  </ul>
                {/if}
                <p class="text-xs text-muted-foreground">
                  {recipientPublicKeys.length +
                    " " +
                    $t("upload.encryption.numberOfRecipientsAdded")}
                </p>
              </div>
            </div>
          {/if}
        </div>
      {/if}
    </Card>
  {/if}

  <!-- BitTorrent Seeding Section (Collapsible) - REMOVED: Now integrated as protocol option -->

  <Card
    class="drop-zone relative p-6 transition-all duration-200 border-dashed {$settings.pureClientMode
      ? 'border-muted-foreground/15 bg-muted/30 opacity-60'
      : isDragging
      ? 'border-primary bg-primary/5'
      : isUploading
        ? 'border-orange-500 bg-orange-500/5'
        : 'border-muted-foreground/25 hover:border-muted-foreground/50'}"
  >
    <!-- Drag & Drop Indicator -->
    {#if $files.filter((f) => f.status === "seeding" || f.status === "uploaded").length === 0}
      <div
        class="text-center py-12 transition-all duration-300 relative overflow-hidden"
      >
        <div class="relative z-10">
          <div class="relative mb-6">
            {#if isDragging}
              <Upload class="h-16 w-16 mx-auto text-primary" />
            {:else}
              <FolderOpen
                class="h-16 w-16 mx-auto text-muted-foreground/70 hover:text-primary transition-colors duration-300"
              />
            {/if}
          </div>

          <h3
            class="text-2xl font-bold mb-3 transition-all duration-300 {isDragging
              ? 'text-primary'
              : isUploading
                ? 'text-orange-500'
                : isClientMode
                  ? 'text-muted-foreground'
                  : 'text-foreground'}"
          >
            {isClientMode
              ? "File Sharing Unavailable"
              : isDragging
                ? $t("upload.dropFilesHere")
                : isUploading
                  ? $t("upload.uploadingFiles")
                  : $t("upload.dropFiles")}
          </h3>

          <p
            class="text-muted-foreground mb-8 text-lg transition-colors duration-300"
          >
            {isClientMode
              ? "You are in client-only mode and cannot share files with the network."
              : isDragging
                ? isTauri
                  ? $t("upload.releaseToUpload")
                  : $t("upload.dragDropWebNotAvailable")
                : isUploading
                  ? $t("upload.pleaseWaitProcessing")
                  : isTauri
                    ? $t("upload.dropFilesHint")
                    : $t("upload.dragDropRequiresDesktop")}
          </p>

          <div
            class="flex justify-center gap-4 mb-8 opacity-60 {isDragging
              ? 'invisible'
              : 'visible'}"
          >
            <Image class="h-8 w-8 text-blue-500 animate-pulse" />
            <Video class="h-8 w-8 text-purple-500 animate-pulse" />
            <Music class="h-8 w-8 text-green-500 animate-pulse" />
            <Archive class="h-8 w-8 text-orange-500 animate-pulse" />
            <Code class="h-8 w-8 text-red-500 animate-pulse" />
          </div>

          <div
            class="flex justify-center gap-3 {isDragging
              ? 'invisible'
              : 'visible'}"
          >
            {#if isTauri}
              <button
                class="group inline-flex items-center justify-center h-12 rounded-xl px-6 text-sm font-medium bg-gradient-to-r from-primary to-primary/90 text-primary-foreground hover:from-primary/90 hover:to-primary shadow-lg hover:shadow-xl transition-all duration-300 hover:scale-105 disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:scale-100"
                disabled={isUploading || $settings.pureClientMode}
                on:click={openFileDialog}
                title={$settings.pureClientMode ? "File sharing disabled - pure client mode is enabled in Settings" : ""}
              >
                <Plus
                  class="h-5 w-5 mr-2 group-hover:rotate-90 transition-transform duration-300"
                />
                {$settings.pureClientMode ? "Sharing Disabled" : isUploading ? $t("upload.uploading") : $t("upload.addFiles")}
              </button>
            {:else}
              <div class="text-center">
                <p class="text-sm text-muted-foreground mb-3">
                  {$t("upload.fileUploadDesktopApp")}
                </p>
                <p class="text-xs text-muted-foreground">
                  {$t("upload.downloadDesktopApp")}
                </p>
              </div>
            {/if}
          </div>

          <p
            class="text-xs text-muted-foreground/75 mt-4 {isDragging
              ? 'invisible'
              : 'visible'}"
          >
            {#if isTauri}
              {$t("upload.supportedFormats")}
            {:else}
              {$t("upload.supportedFormatsDesktop")}
            {/if}
          </p>
        </div>
      </div>
    {:else}
      <!-- Shared Files Header -->
      <div class="flex flex-wrap items-center justify-between gap-4 mb-4 px-4">
        <div>
          <h2 class="text-lg font-semibold">
            {$t("upload.sharedFiles")}
          </h2>
          <p class="text-sm text-muted-foreground mt-1">
            {$coalescedFiles.length}
            {$coalescedFiles.length === 1
              ? $t("upload.file")
              : $t("upload.files")} •
            {formatFileSize(
              $coalescedFiles.reduce((sum, f) => sum + f.size, 0),
            )}
            {$t("upload.total")}
            <span class="text-green-600 font-medium">
              ({$coalescedFiles.reduce((sum, f) => sum + f.totalSeeders, 0)}
              {$coalescedFiles.reduce((sum, f) => sum + f.totalSeeders, 0) === 1
                ? "seeder"
                : "seeders"})
            </span>
          </p>
          <p class="text-xs text-muted-foreground mt-1">
            {$t("upload.tip")}
          </p>
        </div>

        <div class="flex gap-2">
          {#if isTauri}
            <button
              class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
              disabled={isUploading || $settings.pureClientMode}
              on:click={openFileDialog}
              title={$settings.pureClientMode ? "File sharing disabled - pure client mode enabled in Settings" : ""}
            >
              <Plus class="h-4 w-4 mr-2" />
              {$settings.pureClientMode ? "Sharing Disabled" : isUploading ? $t("upload.uploading") : $t("upload.addMoreFiles")}
            </button>
          {:else}
            <div class="text-center">
              <p class="text-xs text-muted-foreground">
                {$t("upload.desktopManagementRequired")}
              </p>
            </div>
          {/if}
        </div>
      </div>

      <!-- Client Mode Warning for Shared Files - Only show when FORCED -->
      {#if clientModeReason === "forced" && $coalescedFiles.length > 0}
        <div class="mx-4 mb-4 p-3 bg-amber-50 border border-amber-200 rounded-lg">
          <div class="flex items-start gap-2">
            <div class="text-amber-600 text-sm mt-0.5">[WARN]</div>
            <div class="flex-1">
              <p class="text-sm font-medium text-amber-800">
                Seeding Disabled - Pure Client Mode Active
              </p>
              <p class="text-xs text-amber-700 mt-1">
                Your files are saved locally but cannot be shared with the network because Pure Client Mode is enabled in Settings. Disable it to enable file sharing.
              </p>
            </div>
          </div>
        </div>
      {/if}
      
      <!-- File List -->
      {#if $coalescedFiles.length > 0}
        <div class="space-y-3 relative px-4">
          {#each $coalescedFiles as coalescedFile}
            <div
              class="group relative bg-gradient-to-r from-card to-card/80 border border-border/50 rounded-xl p-4 hover:shadow-lg hover:border-border transition-all duration-300 overflow-hidden mb-3"
            >
              <div
                class="absolute inset-0 bg-gradient-to-r from-primary/5 via-transparent to-secondary/5 opacity-0 group-hover:opacity-100 transition-opacity duration-300"
              ></div>

              <div class="relative flex items-center justify-between gap-4">
                <div class="flex items-center gap-4 min-w-0 flex-1">
                  <!-- File Icon -->
                  <div class="relative">
                    <div
                      class="absolute inset-0 bg-primary/20 rounded-lg blur-lg opacity-0 group-hover:opacity-100 transition-opacity duration-300"
                    ></div>
                    <div
                      class="relative flex items-center justify-center w-12 h-12 bg-gradient-to-br from-primary/10 to-primary/5 rounded-lg border border-primary/20"
                    >
                      <svelte:component
                        this={getFileIcon(coalescedFile.name)}
                        class="h-6 w-6 {getFileColor(coalescedFile.name)}"
                      />
                    </div>
                  </div>

                  <!-- File Info -->
                  <div class="flex-1 min-w-0 space-y-2">
                    <div class="flex items-center gap-2">
                      <p class="text-sm font-semibold truncate text-foreground">
                        {coalescedFile.name || "Unnamed File"}
                      </p>

                      {#if coalescedFile.primaryProtocol?.fileItem.isEncrypted}
                        <Badge
                          class="bg-purple-100 text-purple-800 text-xs px-2 py-0.5 flex items-center gap-1"
                          title={$t("upload.encryptedEndToEnd")}
                        >
                          <Lock class="h-3 w-3" />
                          {$t("upload.encryption.encryptedBadge")}
                        </Badge>
                      {/if}
                    </div>

                    <div class="space-y-2 text-xs text-muted-foreground">
                      <!-- Protocol Badges -->
                      <div class="flex items-center gap-2 flex-wrap">
                        {#each coalescedFile.protocols as protocolEntry}
                          <Badge
                            class={`text-xs px-2 py-0.5 ${
                              protocolEntry.protocol === "WebRTC"
                                ? "bg-blue-100 text-blue-800"
                                : protocolEntry.protocol === "BitTorrent"
                                  ? "bg-green-100 text-green-800"
                                  : protocolEntry.protocol === "ED2K"
                                    ? "bg-orange-100 text-orange-800"
                                    : "bg-gray-100 text-gray-800"
                            }`}
                          >
                            {#if protocolEntry.protocol === "WebRTC"}
                              <Globe class="h-3 w-3 mr-1" />
                            {:else if protocolEntry.protocol === "BitTorrent"}
                              <Share2 class="h-3 w-3 mr-1" />
                            {:else if protocolEntry.protocol === "ED2K"}
                              <Network class="h-3 w-3 mr-1" />
                            {:else if protocolEntry.protocol === "FTP"}
                              <Server class="h-3 w-3 mr-1" />
                            {/if}
                            {protocolEntry.protocol}
                          </Badge>
                        {/each}
                      </div>

                      <!-- All Identifiers/Links at the top -->
                      <div class="space-y-1 mb-3">
                        <!-- Merkle Hash -->
                        <div class="flex items-center gap-1">
                          <span class="text-xs opacity-70">Merkle Hash:</span>
                          <code
                            class="bg-muted/50 px-1.5 py-0.5 rounded text-xs font-mono"
                          >
                            {coalescedFile.contentHash.slice(
                              0,
                              8,
                            )}...{coalescedFile.contentHash.slice(-6)}
                          </code>
                          <button
                            on:click={() =>
                              handleCopy(coalescedFile.contentHash)}
                            class="group/btn p-1 hover:bg-primary/10 rounded transition-colors"
                            title="Copy Merkle Hash (use this to search and download)"
                            aria-label="Copy Merkle hash"
                          >
                            <Copy
                              class="h-3 w-3 text-muted-foreground group-hover/btn:text-primary transition-colors"
                            />
                          </button>
                        </div>

                        <!-- Protocol-Specific Links -->
                        {#each coalescedFile.protocols as protocolEntry}
                          {#if protocolEntry.protocol === "BitTorrent" && protocolEntry.hash.startsWith("magnet:")}
                            <div class="flex items-center gap-1">
                              <span class="text-xs opacity-70"
                                >Magnet Link:</span
                              >
                              <code
                                class="bg-muted/50 px-1.5 py-0.5 rounded text-xs font-mono truncate max-w-32"
                              >
                                magnet:?xt=urn:btih:{extractInfoHash(
                                  protocolEntry.hash,
                                )}
                              </code>
                              <button
                                on:click={() => handleCopy(protocolEntry.hash)}
                                class="group/btn p-1 hover:bg-primary/10 rounded transition-colors"
                                title="Copy Magnet Link"
                                aria-label="Copy magnet link"
                              >
                                <Copy
                                  class="h-3 w-3 text-muted-foreground group-hover/btn:text-primary transition-colors"
                                />
                              </button>
                            </div>
                          {:else if protocolEntry.protocol === "ED2K" && protocolEntry.hash.startsWith("ed2k://")}
                            <div class="flex items-center gap-1">
                              <span class="text-xs opacity-70">eD2k Link:</span>
                              <code
                                class="bg-muted/50 px-1.5 py-0.5 rounded text-xs font-mono truncate max-w-32"
                              >
                                ed2k://|file|{coalescedFile.name}|{coalescedFile.size}|{extractEd2kHash(
                                  protocolEntry.hash,
                                )}|/
                              </code>
                              <button
                                on:click={() => handleCopy(protocolEntry.hash)}
                                class="group/btn p-1 hover:bg-primary/10 rounded transition-colors"
                                title="Copy eD2k Link"
                                aria-label="Copy eD2k link"
                              >
                                <Copy
                                  class="h-3 w-3 text-muted-foreground group-hover/btn:text-primary transition-colors"
                                />
                              </button>
                            </div>
                          {:else if protocolEntry.protocol === "FTP" && protocolEntry.hash.startsWith("ftp://")}
                            <div class="flex items-center gap-1">
                              <span class="text-xs opacity-70">FTP URL:</span>
                              <code
                                class="bg-muted/50 px-1.5 py-0.5 rounded text-xs font-mono truncate max-w-32"
                              >
                                {protocolEntry.hash}
                              </code>
                              <button
                                on:click={() => handleCopy(protocolEntry.hash)}
                                class="group/btn p-1 hover:bg-primary/10 rounded transition-colors"
                                title="Copy FTP URL"
                                aria-label="Copy FTP URL"
                              >
                                <Copy
                                  class="h-3 w-3 text-muted-foreground group-hover/btn:text-primary transition-colors"
                                />
                              </button>
                            </div>
                          {/if}

                        <!-- Price and Actions -->
                        {/each}
                      </div>

                      <!-- Protocol Seeder Status -->
                      <div class="space-y-1">
                        {#each coalescedFile.protocols as protocolEntry}
                          <div class="text-xs opacity-70">
                            <span
                              >{coalescedFile.protocols.length > 1
                                ? protocolEntry.protocol + " "
                                : ""}Seeders: {protocolEntry.technicalInfo
                                .seederCount || 0}</span
                            >
                          </div>
                        {/each}
                      </div>

                      <div class="flex items-center gap-3">
                        <span class="font-medium"
                          >{formatFileSize(coalescedFile.size)}</span
                        >

                        {#if coalescedFile.totalSeeders > 0}
                          <div class="flex items-center gap-1">
                            <Upload class="h-3 w-3 text-green-500" />
                            <span class="text-green-600 font-medium"
                              >{coalescedFile.totalSeeders}</span
                            >
                          </div>
                        {/if}

                        {#if coalescedFile.totalLeechers > 0}
                          <div class="flex items-center gap-1">
                            <Download class="h-3 w-3 text-orange-500" />
                            <span class="text-orange-600 font-medium"
                              >{coalescedFile.totalLeechers}</span
                            >
                          </div>
                        {/if}
                      </div>
                    </div>
                  </div>
                </div>

                <!-- Price and Actions -->
                <div class="flex items-center gap-2">
                  {#if isTauri}
                    <button
                      on:click={() => removeFile(coalescedFile.contentHash)}
                      class="group/btn p-2 hover:bg-destructive/10 rounded-lg transition-all duration-200 hover:scale-110"
                      title={`Stop sharing this file on all ${coalescedFile.protocols.length} protocol${coalescedFile.protocols.length > 1 ? "s" : ""}`}
                      aria-label="Stop sharing file"
                    >
                      <X
                        class="h-4 w-4 text-muted-foreground group-hover/btn:text-destructive transition-colors"
                      />
                    </button>
                  {:else}
                    <div
                      class="p-2 text-muted-foreground/50 cursor-not-allowed"
                      title={$t("upload.fileManagementTooltip")}
                      aria-label={$t("upload.fileManagementWebNotAvailable")}
                    >
                      <X class="h-4 w-4" />
                    </div>
                  {/if}
                </div>
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="text-center py-8">
          <FolderOpen class="h-12 w-12 mx-auto text-muted-foreground mb-3" />
          <p class="text-sm text-muted-foreground">
            {$t("upload.noFilesShared")}
          </p>
          <p class="text-xs text-muted-foreground mt-1">
            {$t("upload.addFilesHint2")}
          </p>
        </div>
      {/if}
    {/if}
  </Card>
</div>
