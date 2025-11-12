<script lang="ts">
  import Card from "$lib/components/ui/card.svelte";
  import Badge from "$lib/components/ui/badge.svelte";
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
    Blocks,
    Globe,
    DollarSign,
    Copy,
    Share2
  } from "lucide-svelte";
  import { files, type FileItem } from "$lib/stores";
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
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { dhtService } from "$lib/dht";
  import Label from "$lib/components/ui/label.svelte";
  import Input from "$lib/components/ui/input.svelte";
  import { selectedProtocol as protocolStore } from "$lib/stores/protocolStore";
  import { paymentService } from '$lib/services/paymentService';


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
      const isRunning = await invoke<boolean>('is_dht_running').catch(() => false);
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

  // Protocol selection state
  $: selectedProtocol = $protocolStore;
  $: hasSelectedProtocol = selectedProtocol !== null;

  function handleProtocolSelect(protocol: "WebRTC" | "Bitswap" | "BitTorrent") {
    protocolStore.set(protocol);
  }

  function changeProtocol() {
    protocolStore.reset();
  }

  // Encrypted sharing state
  let useEncryptedSharing = false;
  let recipientPublicKey = "";
  let showEncryptionOptions = false;

  // Calculate price using dynamic network metrics with safe fallbacks
  async function calculateFilePrice(sizeInBytes: number): Promise<number> {
    const sizeInMB = sizeInBytes / 1_048_576; // Convert bytes to MB

    try {
      const dynamicPrice = await paymentService.calculateDownloadCost(sizeInBytes);
      if (Number.isFinite(dynamicPrice) && dynamicPrice > 0) {
        return Number(dynamicPrice.toFixed(8));
      }
    } catch (error) {
      console.warn("Dynamic price calculation failed, falling back to static rate:", error);
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

  onMount(async () => {

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
        e.dataTransfer!.dropEffect = "copy";
        isDragging = true;
      };

      const handleDragEnter = (e: DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        e.dataTransfer!.dropEffect = "copy";
        isDragging = true;
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

        // IMPORTANT: Extract files immediately before any async operations
        // dataTransfer.files becomes empty after the event completes
        const droppedFiles = Array.from(e.dataTransfer?.files || []);

        // STEP 1: Verify backend has active account before proceeding
        if (isTauri) {
          try {
            const hasAccount = await invoke<boolean>("has_active_account");
            if (!hasAccount) {
              showToast(
                "Please log in to your account before uploading files",
                "error",
              );
              return;
            }
          } catch (error) {
            console.error("Failed to verify account status:", error);
            showToast(
              "Failed to verify account status. Please try logging in again.",
              "error",
            );
            return;
          }
        }

        if (isUploading) {
          showToast(
            tr("upload.uploadInProgress"),
            "warning",
          );
          return;
        }

        // STEP 2: Ensure DHT is connected before attempting upload
        const dhtConnected = await isDhtConnected();
        if (!dhtConnected) {
          showToast(
            "DHT network is not connected. Please start the DHT network before uploading files.",
            "error",
          );
          return;
        }

        if (droppedFiles.length > 0) {
          if (!isTauri) {
            showToast(
              tr("upload.desktopOnly"),
              "error",
            );
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
                  tr("upload.executableBlocked", { values: { name: file.name } }),
                  "error",
                );
                blockedCount++;
                continue;
              }

              if (file.size === 0) {
                showToast(tr("upload.emptyFile", { values: { name: file.name } }), "error");
                blockedCount++;
                continue;
              }

              try {
                const buffer = await file.arrayBuffer();
                const fileData = Array.from(new Uint8Array(buffer));
                const tempFilePath = await invoke<string>(
                  "save_temp_file_for_upload",
                  {
                    fileName: file.name,
                    fileData,
                  },
                );
                const filePrice = await calculateFilePrice(file.size);

                const metadata = await dhtService.publishFileToNetwork(tempFilePath, filePrice);

                if (get(files).some((f) => f.hash === metadata.merkleRoot)) {
                  duplicateCount++;
                  continue;
                }

                const newFile = {
                  id: `file-${Date.now()}-${Math.random()}`,
                  name: metadata.fileName,
                  path: file.name,
                  hash: metadata.merkleRoot || "",
                  size: metadata.fileSize,
                  status: "seeding" as const,
                  seeders: metadata.seeders?.length ?? 0,
                  seederAddresses: metadata.seeders ?? [],
                  leechers: 0,
                  uploadDate: new Date(metadata.createdAt),
                  price: filePrice,
                  cids: metadata.cids,
                };

                files.update((currentFiles) => [...currentFiles, newFile]);
                addedCount++;
                showToast(`${file.name} uploaded successfully`, "success");
              } catch (error) {
                console.error("Error uploading dropped file:", file.name, error);
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
            showToast(
              tr("upload.uploadError"),
              "error",
            );
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
  });

  onDestroy(() => {
    if ((window as any).dragDropCleanup) {
      (window as any).dragDropCleanup();
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
            "Please log in to your account before uploading files",
            "error",
          );
          return;
        }
      } catch (error) {
        console.error("Failed to verify account status:", error);
        showToast(
          "Failed to verify account status. Please try logging in again.",
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

  async function removeFile(fileHash: string) {
    if (!isTauri) {
      showToast(
        tr("upload.fileManagementDesktopOnly"),
        "error",
      );
      return;
    }

    try {
      try {
        await invoke("stop_publishing_file", { fileHash });
        console.log("File unpublished from DHT:", fileHash);
      } catch (unpublishError) {
        console.warn("Failed to unpublish file from DHT:", unpublishError);
      }

      files.update((f) => f.filter((file) => file.hash !== fileHash));
    } catch (error) {
      console.error(error);
      showToast(
        tr("upload.fileFailed", {
          values: { name: fileHash, error: String(error) },
        }),
        "error",
      );
    }
  }

  async function addFilesFromPaths(paths: string[]) {
    // STEP 1: Verify backend has active account before proceeding
    if (isTauri) {
      try {
        const hasAccount = await invoke<boolean>("has_active_account");
        if (!hasAccount) {
          showToast(
            "Please log in to your account before uploading files",
            "error",
          );
          return;
        }
      } catch (error) {
        console.error("Failed to verify account status:", error);
        showToast(
          "Failed to verify account status. Please try logging in again.",
          "error",
        );
        return;
      }
    }

    // STEP 2: Ensure DHT is connected before attempting upload
    const dhtConnected = await isDhtConnected();
    if (!dhtConnected) {
      showToast(
        "DHT network is not connected. Please start the DHT network before uploading files.",
        "error",
      );
      return;
    }

    let duplicateCount = 0;
    let addedCount = 0;

    // Unified upload flow for all protocols
    for (const filePath of paths) {
      try {
        const fileName = filePath.replace(/^.*[\\/]/, "") || "";

        // Get file size to calculate price
        const fileSize = await invoke<number>('get_file_size', { filePath });
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
            status: "seeding" as const,
            price: 0, // BitTorrent is free
          };

          files.update(f => [...f, torrentFile]);
          showToast(`${fileName} is now seeding as a torrent`, "success");
          continue; // Skip the normal Chiral upload flow
        }

        const metadata = await dhtService.publishFileToNetwork(filePath, price);

        const newFile = {
          id: `file-${Date.now()}-${Math.random()}`,
          name: metadata.fileName,
          path: filePath,
          hash: metadata.merkleRoot || "",
          size: metadata.fileSize,
          status: "seeding" as const,
          seeders: metadata.seeders?.length ?? 0,
          seederAddresses: metadata.seeders ?? [],
          leechers: 0,
          uploadDate: new Date(metadata.createdAt),
          price: price,
          cids: metadata.cids,
        };

        let existed = false;
        files.update((f) => {
          const matchIndex = f.findIndex(
            (item) =>
              (metadata.merkleRoot && item.hash === metadata.merkleRoot) ||
              (item.name === metadata.fileName &&
                item.size === metadata.fileSize),
          );

          if (matchIndex !== -1) {
            const existing = f[matchIndex];
            const updated = {
              ...existing,
              name: metadata.fileName || existing.name,
              hash: metadata.merkleRoot || existing.hash,
              size: metadata.fileSize ?? existing.size,
              seeders: metadata.seeders?.length ?? existing.seeders,
              seederAddresses: metadata.seeders ?? existing.seederAddresses,
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
          duplicateCount++;
          showToast(
            tr("upload.fileUpdated", { values: { name: fileName } }),
            "info",
          );
        } else {
          addedCount++;
          showToast(`${fileName} uploaded successfully`, "success");
        }
      } catch (error) {
        console.error(error);
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

    if (duplicateCount > 0) {
      showToast(
        tr("upload.duplicateSkipped", { values: { count: duplicateCount } }),
        "warning",
      );
    }

    if (addedCount > 0) {
      setTimeout(() => refreshAvailableStorage(), 100);
    }
  }

  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return bytes + " B";
    if (bytes < 1048576) return (bytes / 1024).toFixed(2) + " KB";
    return (bytes / 1048576).toFixed(2) + " MB";
  }

  async function handleCopy(hash: string) {
    await navigator.clipboard.writeText(hash);
    showToast(tr("upload.hashCopiedClipboard"), "success");
  }

  // BitTorrent seeding functions - REMOVED: Now integrated into main upload flow

</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t("upload.title")}</h1>
    <p class="text-muted-foreground mt-2">{$t("upload.subtitle")}</p>
  </div>

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
              <Input
                id="recipient-public-key"
                bind:value={recipientPublicKey}
                placeholder={$t("upload.encryption.publicKeyPlaceholder")}
                class="font-mono text-sm"
                disabled={isUploading}
              />
              <p class="text-xs text-muted-foreground">
                {$t("upload.encryption.publicKeyHint")}
              </p>
              {#if recipientPublicKey && !/^[0-9a-fA-F]{64}$/.test(recipientPublicKey.trim())}
                <p class="text-xs text-destructive">
                  {$t("upload.encryption.invalidPublicKey")}
                </p>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </Card>
  {/if}

  <!-- Protocol Selection/Indicator Card -->
  {#if hasSelectedProtocol}
    <Card>
      <div class="p-4">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <div
              class="flex items-center justify-center w-10 h-10 bg-gradient-to-br from-blue-500/10 to-blue-500/5 rounded-lg border border-blue-500/20"
            >
              {#if selectedProtocol === "WebRTC"}
                <Globe class="h-5 w-5 text-blue-600" />
              {:else if selectedProtocol === "Bitswap"}
                <Blocks class="h-5 w-5 text-blue-600" />
              {:else}
                <Share2 class="h-5 w-5 text-green-600" />
              {/if}
            </div>
            <div>
              <p class="text-sm font-semibold">
                {$t("upload.currentProtocol")}: {selectedProtocol}
              </p>
              <p class="text-xs text-muted-foreground">
                {selectedProtocol === "WebRTC"
                  ? $t("upload.webrtcDescription")
                  : selectedProtocol === "Bitswap"
                  ? $t("upload.bitswapDescription")
                  : $t("torrent.seed.description")}
              </p>
            </div>
          </div>
          <button
            on:click={changeProtocol}
            class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium border border-input bg-background hover:bg-muted transition-colors"
          >
            <RefreshCw class="h-4 w-4 mr-2" />
            {$t("upload.changeProtocol")}
          </button>
        </div>
      </div>
    </Card>
  {/if}

  <!-- BitTorrent Seeding Section (Collapsible) - REMOVED: Now integrated as protocol option -->

  <Card
    class="drop-zone relative p-6 transition-all duration-200 border-dashed {isDragging
      ? 'border-primary bg-primary/5'
      : isUploading
        ? 'border-orange-500 bg-orange-500/5'
        : 'border-muted-foreground/25 hover:border-muted-foreground/50'}"
  >
    {#if !hasSelectedProtocol}
      <Card>
        <div class="p-6">
          <h2 class="text-2xl font-bold mb-6 text-center">
            {$t("upload.selectProtocol")}
          </h2>
          <div class="grid grid-cols-1 md:grid-cols-3 gap-4 max-w-4xl mx-auto">
            <!-- WebRTC Option -->
            <button
              class="p-6 border-2 rounded-lg hover:border-blue-500 transition-colors duration-200 flex flex-col items-center gap-4 {selectedProtocol ===
              'WebRTC'
                ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
                : 'border-gray-200 dark:border-gray-700'}"
              on:click={() => handleProtocolSelect("WebRTC")}
            >
              <div
                class="w-16 h-16 flex items-center justify-center bg-blue-100 rounded-full"
              >
                <Globe class="w-8 h-8 text-blue-600" />
              </div>
              <div class="text-center">
                <h3 class="text-lg font-semibold mb-2">WebRTC</h3>
                <p class="text-sm text-gray-600 dark:text-gray-400">
                  {$t("upload.webrtcDescription")}
                </p>
              </div>
            </button>

            <!-- Bitswap Option -->
            <button
              class="p-6 border-2 rounded-lg hover:border-blue-500 transition-colors duration-200 flex flex-col items-center gap-4 {selectedProtocol ===
              'Bitswap'
                ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
                : 'border-gray-200 dark:border-gray-700'}"
              on:click={() => handleProtocolSelect("Bitswap")}
            >
              <div
                class="w-16 h-16 flex items-center justify-center bg-blue-100 rounded-full"
              >
                <Blocks class="w-8 h-8 text-blue-600" />
              </div>
              <div class="text-center">
                <h3 class="text-lg font-semibold mb-2">Bitswap</h3>
                <p class="text-sm text-gray-600 dark:text-gray-400">
                  {$t("upload.bitswapDescription")}
                </p>
              </div>
            </button>

            <!-- BitTorrent Option -->
            <button
              class="p-6 border-2 rounded-lg hover:border-green-500 transition-colors duration-200 flex flex-col items-center gap-4 {selectedProtocol ===
              'BitTorrent'
                ? 'border-green-500 bg-green-50 dark:bg-green-900/20'
                : 'border-gray-200 dark:border-gray-700'}"
              on:click={() => handleProtocolSelect("BitTorrent")}
            >
              <div
                class="w-16 h-16 flex items-center justify-center bg-green-100 rounded-full"
              >
                <Share2 class="w-8 h-8 text-green-600" />
              </div>
              <div class="text-center">
                <h3 class="text-lg font-semibold mb-2">BitTorrent</h3>
                <p class="text-sm text-gray-600 dark:text-gray-400">
                  {$t("torrent.seed.description")}
                </p>
              </div>
            </button>
          </div>
        </div>
      </Card>
    {:else}
      <!-- Drag & Drop Indicator -->
            {#if $files.filter((f) => f.status === "seeding" || f.status === "uploaded").length === 0}
              <div
                class="text-center py-12 transition-all duration-300 relative overflow-hidden"
              >
                <div class="relative z-10">
                  <div class="relative mb-6">
                    {#if isDragging}
                      <Upload
                        class="h-16 w-16 mx-auto text-primary"
                      />
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
                        : 'text-foreground'}"
                  >
                    {isDragging
                      ? $t("upload.dropFilesHere")
                      : isUploading
                        ? $t("upload.uploadingFiles")
                        : $t("upload.dropFiles")}
                  </h3>

                  <p
                    class="text-muted-foreground mb-8 text-lg transition-colors duration-300"
                  >
                    {isDragging
                      ? isTauri
                        ? $t("upload.releaseToUpload")
                        : $t("upload.dragDropWebNotAvailable")
                      : isUploading
                        ? $t("upload.pleaseWaitProcessing")
                        : isTauri
                          ? $t("upload.dropFilesHint")
                          : $t("upload.dragDropRequiresDesktop")}
                  </p>

                  <div class="flex justify-center gap-4 mb-8 opacity-60 {isDragging ? 'invisible' : 'visible'}">
                    <Image class="h-8 w-8 text-blue-500 animate-pulse" />
                    <Video class="h-8 w-8 text-purple-500 animate-pulse" />
                    <Music class="h-8 w-8 text-green-500 animate-pulse" />
                    <Archive class="h-8 w-8 text-orange-500 animate-pulse" />
                    <Code class="h-8 w-8 text-red-500 animate-pulse" />
                  </div>

                  <div class="flex justify-center gap-3 {isDragging ? 'invisible' : 'visible'}">
                    {#if isTauri}
                        <button
                          class="group inline-flex items-center justify-center h-12 rounded-xl px-6 text-sm font-medium bg-gradient-to-r from-primary to-primary/90 text-primary-foreground hover:from-primary/90 hover:to-primary shadow-lg hover:shadow-xl transition-all duration-300 hover:scale-105 disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:scale-100"
                          disabled={isUploading}
                          on:click={openFileDialog}
                        >
                          <Plus
                            class="h-5 w-5 mr-2 group-hover:rotate-90 transition-transform duration-300"
                          />
                          {isUploading ? $t("upload.uploading") : $t("upload.addFiles")}
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

                  <p class="text-xs text-muted-foreground/75 mt-4 {isDragging ? 'invisible' : 'visible'}">
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
              <div
                class="flex flex-wrap items-center justify-between gap-4 mb-4 px-4"
              >
                <div>
                  <h2 class="text-lg font-semibold">
                    {$t("upload.sharedFiles")}
                  </h2>
                  <p class="text-sm text-muted-foreground mt-1">
                    {$files.filter(
                      (f) => f.status === "seeding" || f.status === "uploaded",
                    ).length}
                    {$files.filter(
                      (f) => f.status === "seeding" || f.status === "uploaded",
                    ).length === 1 ? $t("upload.file") : $t("upload.files")} •
                    {formatFileSize(
                      $files
                        .filter(
                          (f) =>
                            f.status === "seeding" || f.status === "uploaded",
                        )
                        .reduce((sum, f) => sum + f.size, 0),
                    )}
                    {$t("upload.total")}
                    {#if $files.filter((f) => f.status === "seeding").length > 0}
                      <span class="text-green-600 font-medium">
                        ({$files.filter((f) => f.status === "seeding").length} {$files.filter((f) => f.status === "seeding").length === 1 ? "seeding" : "seeding"})
                      </span>
                    {/if}
                  </p>
                  <p class="text-xs text-muted-foreground mt-1">
                    {$t("upload.tip")}
                  </p>
                </div>

                <div class="flex gap-2">
                  {#if isTauri}
                    <button
                      class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
                      disabled={isUploading}
                      on:click={openFileDialog}
                    >
                      <Plus class="h-4 w-4 mr-2" />
                      {isUploading ? $t("upload.uploading") : $t("upload.addMoreFiles")}
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

              <!-- File List -->
              {#if $files.filter((f) => f.status === "seeding" || f.status === "uploaded").length > 0}
                <div class="space-y-3 relative px-4">
                  {#each $files.filter((f) => f.status === "seeding" || f.status === "uploaded") as file}
                    <div
                      class="group relative bg-gradient-to-r from-card to-card/80 border border-border/50 rounded-xl p-4 hover:shadow-lg hover:border-border transition-all duration-300 overflow-hidden mb-3"
                    >
                      <div
                        class="absolute inset-0 bg-gradient-to-r from-primary/5 via-transparent to-secondary/5 opacity-0 group-hover:opacity-100 transition-opacity duration-300"
                      ></div>

                      <div
                        class="relative flex items-center justify-between gap-4"
                      >
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
                                this={getFileIcon(file.name)}
                                class="h-6 w-6 {getFileColor(file.name)}"
                              />
                            </div>
                          </div>

                          <!-- File Info -->
                          <div class="flex-1 min-w-0 space-y-2">
                            <div class="flex items-center gap-2">
                              <p
                                class="text-sm font-semibold truncate text-foreground"
                              >
                                {file.name}
                              </p>

                              {#if file.isEncrypted}
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
                              <div class="flex items-center gap-1">
                                <span class="opacity-60">{$t("upload.hashLabel")}</span>
                                <code
                                  class="bg-muted/50 px-1.5 py-0.5 rounded text-xs font-mono"
                                >
                                  {file.hash.slice(0, 8)}...{file.hash.slice(
                                    -6,
                                  )}
                                </code>
                                <button
                                  on:click={() => handleCopy(file.hash)}
                                  class="group/btn p-1 hover:bg-primary/10 rounded transition-colors"
                                  title={$t("upload.copyHash")}
                                  aria-label="Copy file hash"
                                >
                                  <Copy class="h-3 w-3 text-muted-foreground group-hover/btn:text-primary transition-colors" />
                                </button>
                              </div>

                              {#if file.cids && file.cids.length > 0}
                                <div class="flex items-center gap-1">
                                  <span class="opacity-60">CID:</span>
                                  <code
                                    class="bg-muted/50 px-1.5 py-0.5 rounded text-xs font-mono"
                                  >
                                    {file.cids[0].slice(0, 8)}...{file.cids[0].slice(-6)}
                                  </code>
                                  <button
                                    on:click={() => handleCopy(file.cids![0])}
                                    class="group/btn p-1 hover:bg-primary/10 rounded transition-colors"
                                    title="Copy CID"
                                    aria-label="Copy file CID"
                                  >
                                    <Copy class="h-3 w-3 text-muted-foreground group-hover/btn:text-primary transition-colors" />
                                  </button>
                                </div>
                              {/if}

                              {#if file.hash.startsWith('magnet:')}
                                <div class="flex items-center gap-1">
                                  <Badge class="bg-green-100 text-green-800 text-xs px-2 py-0.5">
                                    <Share2 class="h-3 w-3 mr-1" />
                                    BitTorrent
                                  </Badge>
                                  <button
                                    on:click={() => handleCopy(file.hash)}
                                    class="text-xs text-muted-foreground hover:text-primary"
                                    title="Copy magnet link"
                                  >
                                    Copy Magnet Link
                                  </button>
                                </div>
                              {/if}

                              <div class="flex items-center gap-3">
                                <span class="font-medium"
                                  >{formatFileSize(file.size)}</span
                                >

                                {#if file.seeders !== undefined}
                                  <div class="flex items-center gap-1">
                                    <Upload class="h-3 w-3 text-green-500" />
                                    <span class="text-green-600 font-medium"
                                      >{file.seeders || 1}</span
                                    >
                                  </div>
                                {/if}

                                {#if file.leechers && file.leechers > 0}
                                  <div class="flex items-center gap-1">
                                    <Download class="h-3 w-3 text-orange-500" />
                                    <span class="text-orange-600 font-medium"
                                      >{file.leechers}</span
                                    >
                                  </div>
                                {/if}
                              </div>
                            </div>
                          </div>
                        </div>

                        <!-- Price and Actions -->
                        <div class="flex items-center gap-2">
                          <!-- Price Badge -->
                          {#if file.price !== undefined && file.price !== null}
                            <div
                              class="flex items-center gap-1.5 bg-green-500/10 text-green-600 border border-green-500/20 font-medium px-2.5 py-1 rounded-md"
                              title={$t("upload.priceTooltip")}
                            >
                              <DollarSign class="h-3.5 w-3.5" />
                              <span class="text-sm"
                                >{file.price.toFixed(8)} Chiral</span
                              >
                            </div>
                          {/if}

                          {#if isTauri}
                            <button
                              on:click={() => removeFile(file.hash)}
                              class="group/btn p-2 hover:bg-destructive/10 rounded-lg transition-all duration-200 hover:scale-110"
                              title={$t("upload.stopSharing")}
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
                  <FolderOpen
                    class="h-12 w-12 mx-auto text-muted-foreground mb-3"
                  />
                  <p class="text-sm text-muted-foreground">
                    {$t("upload.noFilesShared")}
                  </p>
                  <p class="text-xs text-muted-foreground mt-1">
                    {$t("upload.addFilesHint2")}
                  </p>
                </div>
              {/if}
            {/if}
    {/if}
  </Card>
</div>
