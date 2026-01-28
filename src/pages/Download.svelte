<script lang="ts">
  import Button from "$lib/components/ui/button.svelte";
  import Card from "$lib/components/ui/card.svelte";
  import Input from "$lib/components/ui/input.svelte";
  import Badge from "$lib/components/ui/badge.svelte";
  import Progress from "$lib/components/ui/progress.svelte";
  import {
    Search,
    Pause,
    Play,
    X,
    FolderOpen,
    File as FileIcon,
    FileText,
    FileImage,
    FileVideo,
    FileAudio,
    Archive,
    Code,
    FileSpreadsheet,
    Presentation,
    History,
    Download as DownloadIcon,
    Upload as UploadIcon,
    Trash2,
    RefreshCw,
    Eye,
    ChevronUp,
    ChevronDown,
  } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";

  import DownloadSearchSection from "$lib/components/download/DownloadSearchSection.svelte";
  import FilePreviewModal from "$lib/components/FilePreviewModal.svelte";
  import { canPreviewFile } from "$lib/utils/fileTypeDetector";
  import { protocolManager, type FileIdentification } from "$lib/services/contentProtocols";
  import { Protocol } from "$lib/services/contentProtocols/types";
  import { transferStore, type Transfer } from "$lib/stores/transferEventsStore";
  import {
    downloadHistoryService,
    downloadHistoryVersion,
    type DownloadHistoryEntry,
    type DownloadHistoryInput,
    type DownloadPaymentStatus,
  } from "$lib/services/downloadHistoryService";
  import { paymentService } from "$lib/services/paymentService";
  import type { CompleteFileMetadata } from "$lib/dht";
  import type { ProtocolDetails } from "$lib/types/protocols";
  import { toHumanReadableSize, formatSpeed } from "$lib/utils";
  import { bytesToMb } from "$lib/utils/pricing";
  import { showToast } from "$lib/toast";

  const tr = (k: string, params?: Record<string, any>) => $t(k, params);

  type DownloadStatus =
    | "queued"
    | "downloading"
    | "paused"
    | "completed"
    | "failed"
    | "canceled";

  type DownloadFilterStatus =
    | "all"
    | "active"
    | "paused"
    | "queued"
    | "completed"
    | "failed"
    | "canceled";

  type DownloadRow = {
    id: string;
    name: string;
    hash: string;
    size: number;
    status: DownloadStatus;
    progress: number;
    speed: string;
    eta: string;
    outputPath?: string;
    protocol?: Protocol;
    source: "transfer" | "pending";
  };

  type DownloadRequest = {
    requestId: string;
    transferId?: string;
    fileHash: string;
    fileName: string;
    fileSize: number;
    protocol: Protocol;
    peerId: string;
    protocolDetails?: ProtocolDetails;
    price: number;
    seederAddresses: string;
    status: DownloadStatus;
    createdAt: number;
  };

  type TransferContext = {
    fileHash: string;
    fileName: string;
    fileSize: number;
    protocol: Protocol;
    peerId: string;
    protocolDetails?: ProtocolDetails;
    price: number;
    seederAddresses: string;
  };

  type DownloadStartInfo = {
    fileHash: string;
    fileName: string;
    fileSize: number;
    protocol: Protocol;
    peerId: string;
    protocolDetails?: ProtocolDetails;
    price: number;
    seederAddresses: string;
  };

  const DEFAULT_SPEED = "0 B/s";
  const DEFAULT_ETA = "N/A";

  const STATUS_ORDER: Record<DownloadStatus, number> = {
    downloading: 0,
    paused: 1,
    queued: 2,
    completed: 3,
    failed: 4,
    canceled: 5,
  };

  const FILE_ICONS: Record<string, any> = {
    pdf: FileText,
    doc: FileText,
    docx: FileText,
    txt: FileText,
    rtf: FileText,
    jpg: FileImage,
    jpeg: FileImage,
    png: FileImage,
    gif: FileImage,
    bmp: FileImage,
    svg: FileImage,
    webp: FileImage,
    mp4: FileVideo,
    avi: FileVideo,
    mov: FileVideo,
    wmv: FileVideo,
    flv: FileVideo,
    webm: FileVideo,
    mkv: FileVideo,
    mp3: FileAudio,
    wav: FileAudio,
    flac: FileAudio,
    aac: FileAudio,
    ogg: FileAudio,
    zip: Archive,
    rar: Archive,
    "7z": Archive,
    tar: Archive,
    gz: Archive,
    js: Code,
    ts: Code,
    html: Code,
    css: Code,
    py: Code,
    java: Code,
    cpp: Code,
    c: Code,
    php: Code,
    xls: FileSpreadsheet,
    xlsx: FileSpreadsheet,
    csv: FileSpreadsheet,
    ppt: Presentation,
    pptx: Presentation,
  };

  let searchFilter = "";
  let filterStatus: DownloadFilterStatus = "all";

  let showHistory = false;
  let downloadHistory: DownloadHistoryEntry[] = [];
  let historySearchQuery = "";
  let historyFilter: "all" | "completed" | "failed" | "canceled" = "all";
  let statistics: ReturnType<typeof downloadHistoryService.getStatistics>;

  let showPreviewModal = false;
  let previewFileName = "";
  let previewFilePath = "";
  let previewFileSize = 0;

  let pendingRequests: DownloadRequest[] = [];
  let transferContexts = new Map<string, TransferContext>();
  let recordedHistoryTransfers = new Set<string>();

  let allDownloads: DownloadRow[] = [];
  let filteredDownloads: DownloadRow[] = [];
  let activeCount = 0;
  let pausedCount = 0;
  let queuedCount = 0;
  let completedCount = 0;
  let failedCount = 0;
  let canceledCount = 0;

  function refreshHistory() {
    downloadHistory = downloadHistoryService.getFilteredHistory(
      historyFilter === "all" ? undefined : historyFilter,
      historySearchQuery,
    );
    statistics = downloadHistoryService.getStatistics();
  }

  $: {
    historyFilter;
    historySearchQuery;
    $downloadHistoryVersion;
    refreshHistory();
  }

  function getFileIcon(fileName: string) {
    const extension = fileName.split(".").pop()?.toLowerCase() || "";
    return FILE_ICONS[extension] || FileIcon;
  }

  function isHistoryStatus(status: DownloadStatus): status is DownloadHistoryInput["status"] {
    return status === "completed" || status === "failed" || status === "canceled";
  }

  function getInitialPaymentStatus(
    status: DownloadStatus,
    context?: TransferContext,
  ): DownloadPaymentStatus | undefined {
    if (status !== "completed") return undefined;
    if (typeof context?.price === "number" && context.price <= 0) return "completed";
    if (!getSeederWalletAddress(context)) return undefined;
    return "not_sent";
  }

  function getSeederWalletAddress(context?: TransferContext): string | undefined {
    if (!context?.seederAddresses) return undefined;
    return paymentService.isValidWalletAddress(context.seederAddresses)
      ? context.seederAddresses
      : undefined;
  }

  function formatPaymentStatus(status?: DownloadPaymentStatus): string {
    if (!status) return "";
    if (status === "not_sent") return "Not sent";
    if (status === "pending") return "Pending";
    return "Completed";
  }

  function formatEta(seconds?: number): string {
    if (!seconds || seconds <= 0) return DEFAULT_ETA;
    if (seconds < 60) return `${Math.round(seconds)}s`;
    if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.round((seconds % 3600) / 60);
    return `${hours}h ${minutes}m`;
  }

  function toTransferRow(transfer: Transfer): DownloadRow {
    const status = transfer.status;
    const progress = Number.isFinite(transfer.progressPercentage)
      ? Math.min(100, Math.max(0, transfer.progressPercentage))
      : 0;
    const speed =
      status === "paused"
        ? DEFAULT_SPEED
        : transfer.downloadSpeedBps > 0
          ? formatSpeed(transfer.downloadSpeedBps)
          : DEFAULT_SPEED;
    const eta = status === "paused" ? DEFAULT_ETA : formatEta(transfer.etaSeconds);

    return {
      id: transfer.transferId,
      name: transfer.fileName,
      hash: transfer.fileHash,
      size: transfer.fileSize,
      status,
      progress,
      speed,
      eta,
      outputPath: transfer.outputPath,
      protocol: transfer.protocol,
      source: "transfer",
    };
  }

  function toRequestRow(request: DownloadRequest): DownloadRow {
    return {
      id: request.requestId,
      name: request.fileName,
      hash: request.fileHash,
      size: request.fileSize,
      status: request.status,
      progress: 0,
      speed: DEFAULT_SPEED,
      eta: DEFAULT_ETA,
      protocol: request.protocol,
      source: "pending",
    };
  }

  function matchesTransfer(request: DownloadRequest, transfer: Transfer): boolean {
    if (request.transferId) {
      return request.transferId === transfer.transferId;
    }
    if (request.fileHash && transfer.fileHash) {
      return request.fileHash === transfer.fileHash;
    }
    return (
      request.fileName === transfer.fileName && request.fileSize === transfer.fileSize
    );
  }

  function buildIdentification(info: {
    fileHash: string;
    fileName: string;
    fileSize: number;
    protocol: Protocol;
    protocolDetails?: ProtocolDetails;
  }): FileIdentification {
    return {
      protocol: info.protocol,
      fileHash: info.fileHash,
      fileName: info.fileName,
      fileSize: info.fileSize,
      protocolDetails: info.protocolDetails ?? {},
    };
  }

  function getContextForRow(row: DownloadRow): TransferContext | undefined {
    if (row.source === "transfer") {
      return transferContexts.get(row.id);
    }

    const request = pendingRequests.find((item) => item.requestId === row.id);
    if (!request) return undefined;

    return {
      fileHash: request.fileHash,
      fileName: request.fileName,
      fileSize: request.fileSize,
      protocol: request.protocol,
      peerId: request.peerId,
      protocolDetails: request.protocolDetails,
      price: request.price,
      seederAddresses: request.seederAddresses,
    };
  }

  function updatePendingStatus(requestId: string, status: DownloadStatus) {
    pendingRequests = pendingRequests.map((request) =>
      request.requestId === requestId ? { ...request, status } : request,
    );
  }

  async function startDownload(info: DownloadStartInfo) {
    const requestId = `download-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const request: DownloadRequest = {
      requestId,
      fileHash: info.fileHash,
      fileName: info.fileName,
      fileSize: info.fileSize,
      protocol: info.protocol,
      peerId: info.peerId,
      protocolDetails: info.protocolDetails,
      price: info.price,
      seederAddresses: info.seederAddresses,
      status: "queued",
      createdAt: Date.now(),
    };

    pendingRequests = [request, ...pendingRequests];

    const identification = buildIdentification({
      fileHash: info.fileHash,
      fileName: info.fileName,
      fileSize: info.fileSize,
      protocol: info.protocol,
      protocolDetails: info.protocolDetails,
    });

    showToast(`Download started: ${info.fileName}`, "success");

    try {
      const result = await protocolManager.downloadFile(
        info.peerId,
        identification,
        () => {},
      );

      if (result?.transferId) {
        pendingRequests = pendingRequests.map((request) =>
          request.requestId === requestId
            ? { ...request, transferId: result.transferId }
            : request,
        );
      }
    } catch (error) {
      updatePendingStatus(requestId, "failed");
      showToast(
        `Download failed: ${error instanceof Error ? error.message : String(error)}`,
        "error",
      );
    }
  }

  async function processSeederPayment(
    entry: DownloadHistoryInput,
    context?: TransferContext,
  ) {
    if (entry.status !== "completed") return;
    if (entry.paymentStatus !== "not_sent") return;
    const seederWalletAddress = getSeederWalletAddress(context);
    if (!seederWalletAddress) return;

    const entryHash = entry.hash;
    try {
      await paymentService.processDownloadPayment(
        entryHash,
        entry.name,
        entry.size,
        seederWalletAddress,
        context?.peerId,
        context?.price,
      );
    } catch (error) {
      console.error("Failed to process seeder payment:", error);
    }
  }

  async function handleSearchDownload(
    fullMetadata: CompleteFileMetadata,
    selectedPeer: string,
    selectedProtocol: Protocol,
    price: number,
  ) {
    const metadata = fullMetadata.dhtRecord;
    const seederInfo = fullMetadata.seederInfo[selectedPeer];
    if (!seederInfo) {
      showToast("Selected peer info is unavailable", "error");
      return;
    }
    const seederAddress = seederInfo.general.walletAddress;

    startDownload({
      fileHash: metadata.fileHash,
      fileName: metadata.fileName,
      fileSize: metadata.fileSize,
      protocol: selectedProtocol,
      peerId: selectedPeer,
      protocolDetails: seederInfo.fileSpecific.protocolDetails,
      price,
      seederAddresses: seederAddress,
    });
  }

  async function togglePause(row: DownloadRow) {
    if (row.status === "downloading") {
      await pauseDownload(row);
    } else if (row.status === "paused") {
      await resumeDownload(row);
    }
  }

  async function pauseDownload(row: DownloadRow) {
    if (!row.protocol) {
      showToast("Cannot pause: protocol unknown", "error");
      return;
    }

    const context = getContextForRow(row);
    const identification = buildIdentification({
      fileHash: row.hash,
      fileName: row.name,
      fileSize: row.size,
      protocol: row.protocol,
      protocolDetails: context?.protocolDetails,
    });

    try {
      const result = await protocolManager.pauseDownload(identification);
      if (!result) {
        showToast(`Pause not supported for ${row.protocol}`, "warning");
      }
    } catch (error) {
      showToast(
        `Failed to pause: ${error instanceof Error ? error.message : String(error)}`,
        "error",
      );
    }
  }

  async function resumeDownload(row: DownloadRow) {
    if (!row.protocol) {
      showToast("Cannot resume: protocol unknown", "error");
      return;
    }

    const context = getContextForRow(row);
    const identification = buildIdentification({
      fileHash: row.hash,
      fileName: row.name,
      fileSize: row.size,
      protocol: row.protocol,
      protocolDetails: context?.protocolDetails,
    });

    try {
      const result = await protocolManager.resumeDownload(identification);
      if (!result) {
        showToast(`Resume not supported for ${row.protocol}`, "warning");
      }
    } catch (error) {
      showToast(
        `Failed to resume: ${error instanceof Error ? error.message : String(error)}`,
        "error",
      );
    }
  }

  async function cancelDownload(row: DownloadRow) {
    const context = getContextForRow(row);
    if (row.protocol) {
      const identification = buildIdentification({
        fileHash: row.hash,
        fileName: row.name,
        fileSize: row.size,
        protocol: row.protocol,
        protocolDetails: context?.protocolDetails,
      });

      try {
        await protocolManager.cancelDownload(identification);
      } catch (error) {
        showToast(
          `Failed to cancel: ${error instanceof Error ? error.message : String(error)}`,
          "error",
        );
      }
    }

    if (row.source === "pending") {
      updatePendingStatus(row.id, "canceled");
    }
  }

  function clearDownload(row: DownloadRow) {
    if (row.source === "pending") {
      pendingRequests = pendingRequests.filter((request) => request.requestId !== row.id);
      return;
    }

    transferStore.removeTransfer(row.id);
    if (transferContexts.has(row.id)) {
      const nextContexts = new Map(transferContexts);
      nextContexts.delete(row.id);
      transferContexts = nextContexts;
    }
  }

  function clearAllFinished() {
    transferStore.clearFinished();
    pendingRequests = pendingRequests.filter(
      (request) =>
        request.status !== "completed" &&
        request.status !== "failed" &&
        request.status !== "canceled",
    );
  }

  function retryDownload(row: DownloadRow) {
    const context = getContextForRow(row);
    if (!context) {
      showToast("Missing metadata for retry. Please re-search the file.", "error");
      return;
    }
    if (!context.peerId) {
      showToast("Missing peer ID for retry. Please re-search the file.", "error");
      return;
    }

    startDownload({
      fileHash: context.fileHash,
      fileName: context.fileName,
      fileSize: context.fileSize,
      protocol: context.protocol,
      peerId: context.peerId,
      protocolDetails: context.protocolDetails,
      price: context.price,
      seederAddresses: context.seederAddresses,
    });
  }

  async function openPreview(row: DownloadRow) {
    if (row.status !== "completed" || !row.outputPath) {
      showToast("File must be completed to preview", "warning");
      return;
    }

    previewFilePath = row.outputPath;
    previewFileName = row.name;
    previewFileSize = row.size;
    showPreviewModal = true;
  }

  async function showInFolder() {
    try {
      const storagePath = await invoke("get_download_directory");
      await invoke("show_in_folder", { path: storagePath });
    } catch (error) {
      showToast(
        `Failed to open storage folder: ${error instanceof Error ? error.message : String(error)}`,
        "error",
      );
    }
  }

  $: {
    const transfers = Array.from($transferStore.transfers.values());
    let nextPending = [...pendingRequests];
    let nextContexts = new Map(transferContexts);
    let pendingChanged = false;
    let contextsChanged = false;

    for (const transfer of transfers) {
      if (nextContexts.has(transfer.transferId)) continue;
      const matchIndex = nextPending.findIndex((request) =>
        matchesTransfer(request, transfer),
      );
      if (matchIndex === -1) continue;

      const request = nextPending[matchIndex];
      nextPending.splice(matchIndex, 1);
      nextContexts.set(transfer.transferId, {
        fileHash: request.fileHash,
        fileName: request.fileName,
        fileSize: request.fileSize,
        protocol: request.protocol,
        peerId: request.peerId,
        protocolDetails: request.protocolDetails,
        price: request.price,
        seederAddresses: request.seederAddresses,
      });
      pendingChanged = true;
      contextsChanged = true;
    }

    if (pendingChanged) pendingRequests = nextPending;
    if (contextsChanged) transferContexts = nextContexts;
  }

  $: {
    const transfers = Array.from($transferStore.transfers.values());
    let nextRecorded = recordedHistoryTransfers;
    let recordedChanged = false;

    for (const transfer of transfers) {
      const status = transfer.status;
      if (!isHistoryStatus(status)) {
        continue;
      }

      if (nextRecorded.has(transfer.transferId)) continue;

      const context = transferContexts.get(transfer.transferId);
      const entryPrice = context?.price ?? 0;
      const entrySeederAddress = context?.seederAddresses ?? "";
      const entryPeerId = context?.peerId ?? "";
      const entryProtocol = transfer.protocol ?? context?.protocol ?? Protocol.UNKNOWN;
      const paymentStatus = getInitialPaymentStatus(status, context);
      const entry: DownloadHistoryInput = {
        id: transfer.transferId,
        hash: transfer.fileHash,
        name: transfer.fileName,
        size: transfer.fileSize,
        status,
        downloadPath: transfer.outputPath,
        price: entryPrice,
        seederAddresses: entrySeederAddress,
        paymentStatus,
        protocol: entryProtocol,
        protocolDetails: context?.protocolDetails,
        peerId: entryPeerId,
      };

      downloadHistoryService.addToHistory(entry);

      if (status === "completed") {
        void processSeederPayment(entry, context);
      }

      if (nextRecorded === recordedHistoryTransfers) {
        nextRecorded = new Set(recordedHistoryTransfers);
      }
      nextRecorded.add(transfer.transferId);
      recordedChanged = true;
    }

    if (recordedChanged) {
      recordedHistoryTransfers = nextRecorded;
      refreshHistory();
    }
  }

  $: {
    const transferRows = Array.from($transferStore.transfers.values()).map(toTransferRow);
    const pendingRows = pendingRequests.map(toRequestRow);

    allDownloads = [...transferRows, ...pendingRows].sort((a, b) => {
      const statusDiff = (STATUS_ORDER[a.status] ?? 999) - (STATUS_ORDER[b.status] ?? 999);
      if (statusDiff !== 0) return statusDiff;
      return a.name.localeCompare(b.name);
    });
  }

  $: {
    let filtered = allDownloads;

    if (searchFilter.trim()) {
      const query = searchFilter.toLowerCase();
      filtered = filtered.filter(
        (file) =>
          file.hash.toLowerCase().includes(query) ||
          file.name.toLowerCase().includes(query),
      );
    }

    if (filterStatus === "active") {
      filtered = filtered.filter((file) => file.status === "downloading");
    } else if (filterStatus !== "all") {
      filtered = filtered.filter((file) => file.status === filterStatus);
    }

    filteredDownloads = filtered;
  }

  $: activeCount = allDownloads.filter((file) => file.status === "downloading").length;
  $: pausedCount = allDownloads.filter((file) => file.status === "paused").length;
  $: queuedCount = allDownloads.filter((file) => file.status === "queued").length;
  $: completedCount = allDownloads.filter((file) => file.status === "completed").length;
  $: failedCount = allDownloads.filter((file) => file.status === "failed").length;
  $: canceledCount = allDownloads.filter((file) => file.status === "canceled").length;

  function exportHistory() {
    const data = downloadHistoryService.exportHistory();
    const blob = new Blob([data], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `chiral-download-history-${new Date().toISOString().split("T")[0]}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    showToast(tr("downloadHistory.messages.exportSuccess"), "success");
  }

  function importHistory() {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json";
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;

      try {
        const text = await file.text();
        const result = downloadHistoryService.importHistory(text);

        if (result.success) {
          showToast(
            tr("downloadHistory.messages.importSuccess", {
              count: result.imported,
            }),
            "success",
          );
          refreshHistory();
        } else {
          showToast(
            tr("downloadHistory.messages.importError", { error: result.error }),
            "error",
          );
        }
      } catch (error) {
        showToast(
          tr("downloadHistory.messages.importError", {
            error: error instanceof Error ? error.message : "Unknown error",
          }),
          "error",
        );
      }
    };
    input.click();
  }

  async function clearAllHistory() {
    if (confirm(tr("downloadHistory.confirmClear"))) {
      downloadHistoryService.clearHistory();
      refreshHistory();
      showToast(tr("downloadHistory.messages.historyCleared"), "success");
    }
  }

  async function clearFailedHistory() {
    if (confirm(tr("downloadHistory.confirmClearFailed"))) {
      downloadHistoryService.clearFailedDownloads();
      refreshHistory();
      showToast(tr("downloadHistory.messages.failedCleared"), "success");
    }
  }

  async function clearCanceledHistory() {
    if (confirm(tr("downloadHistory.confirmClearCanceled"))) {
      downloadHistoryService.clearCanceledDownloads();
      refreshHistory();
      showToast(tr("downloadHistory.messages.canceledCleared"), "success");
    }
  }

  function removeHistoryEntry(hash: string) {
    downloadHistoryService.removeFromHistory(hash);
    refreshHistory();
    showToast(tr("downloadHistory.messages.entryRemoved"), "success");
  }

  function redownloadFile(entry: DownloadHistoryEntry) {
    const protocol = entry.protocol;
    if (!entry.peerId) {
      showToast("Missing peer ID for re-download. Please re-search the file.", "error");
      return;
    }
    startDownload({
      fileHash: entry.hash,
      fileName: entry.name,
      fileSize: entry.size,
      protocol,
      peerId: entry.peerId,
      protocolDetails: entry.protocolDetails,
      price: entry.price,
      seederAddresses: entry.seederAddresses,
    });
  }

  const formatFileSize = toHumanReadableSize;

  function formatPricePerMb(totalPrice: number, sizeBytes: number): string {
    if (!Number.isFinite(totalPrice) || totalPrice <= 0) return "";
    if (!Number.isFinite(sizeBytes) || sizeBytes <= 0) return "";
    const mb = bytesToMb(sizeBytes);
    if (!Number.isFinite(mb) || mb <= 0) return "";
    return (totalPrice / mb).toFixed(6);
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t("download.title")}</h1>
    <p class="text-muted-foreground mt-2">{$t("download.subtitle")}</p>
  </div>

  <Card>
    <div class="border-b">
      <DownloadSearchSection
        on:download={(event) =>
          handleSearchDownload(
            event.detail.fullMetadata,
            event.detail.selectedPeer,
            event.detail.selectedProtocol,
            event.detail.price,
          )}
      />
    </div>
  </Card>

  <Card class="p-6">
    <div class="space-y-4 mb-6">
      <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
        <h2 class="text-xl font-semibold">{$t("download.downloads")}</h2>

        <div class="relative w-full sm:w-80">
          <Input
            bind:value={searchFilter}
            placeholder={$t("download.searchPlaceholder")}
            class="pr-8"
          />
          {#if searchFilter}
            <button
              on:click={() => (searchFilter = "")}
              class="absolute right-2 top-1/2 transform -translate-y-1/2 text-muted-foreground hover:text-foreground"
              type="button"
              title={$t("download.clearSearch")}
            >
              ×
            </button>
          {:else}
            <Search
              class="absolute right-2 top-1/2 transform -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none"
            />
          {/if}
        </div>
      </div>

      <div class="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-4">
        <div class="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant={filterStatus === "all" ? "default" : "outline"}
            on:click={() => (filterStatus = "all")}
            class="text-xs"
          >
            {$t("download.filters.all")} ({allDownloads.length})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === "active" ? "default" : "outline"}
            on:click={() => (filterStatus = "active")}
            class="text-xs"
          >
            {$t("download.filters.active")} ({activeCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === "paused" ? "default" : "outline"}
            on:click={() => (filterStatus = "paused")}
            class="text-xs"
          >
            {$t("download.filters.paused")} ({pausedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === "queued" ? "default" : "outline"}
            on:click={() => (filterStatus = "queued")}
            class="text-xs"
          >
            {$t("download.filters.queued")} ({queuedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === "completed" ? "default" : "outline"}
            on:click={() => (filterStatus = "completed")}
            class="text-xs"
          >
            {$t("download.filters.completed")} ({completedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === "canceled" ? "default" : "outline"}
            on:click={() => (filterStatus = "canceled")}
            class="text-xs"
          >
            {$t("download.filters.canceled")} ({canceledCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === "failed" ? "default" : "outline"}
            on:click={() => (filterStatus = "failed")}
            class="text-xs"
          >
            {$t("download.filters.failed")} ({failedCount})
          </Button>

          {#if completedCount > 0 || failedCount > 0 || canceledCount > 0}
            <Button
              size="sm"
              variant="outline"
              on:click={clearAllFinished}
              class="text-xs text-destructive border-destructive hover:bg-destructive/10 hover:text-destructive"
            >
              <X class="h-3 w-3 mr-1" />
              {$t("download.clearFinished")}
            </Button>
          {/if}
        </div>
      </div>
    </div>

    {#if filteredDownloads.length === 0}
      <p class="text-sm text-muted-foreground text-center py-8">
        {#if filterStatus === "all"}
          {$t("download.status.noDownloads")}
        {:else if filterStatus === "active"}
          {$t("download.status.noActive")}
        {:else if filterStatus === "paused"}
          {$t("download.status.noPaused")}
        {:else if filterStatus === "queued"}
          {$t("download.status.noQueued")}
        {:else if filterStatus === "completed"}
          {$t("download.status.noCompleted")}
        {:else}
          {$t("download.status.noFailed")}
        {/if}
      </p>
    {:else}
      <div class="space-y-3" role="list">
        {#each filteredDownloads as file}
          <div
            role="listitem"
            class="p-3 bg-muted/60 rounded-lg hover:bg-muted/80 transition-colors"
          >
            <div class="pb-2">
              <div class="flex items-start justify-between gap-4">
                <div class="flex items-start gap-3 flex-1 min-w-0">
                  <div class="flex items-start gap-3 flex-1 min-w-0">
                    <svelte:component
                      this={getFileIcon(file.name)}
                      class="h-4 w-4 text-muted-foreground mt-0.5"
                    />
                    <div class="flex-1 min-w-0">
                      <div class="flex items-center gap-3 mb-1">
                        <h3 class="font-semibold text-sm truncate">{file.name}</h3>
                        <Badge
                          class="text-xs font-semibold bg-muted-foreground/20 text-foreground border-0 px-2 py-0.5"
                        >
                          {formatFileSize(file.size)}
                        </Badge>
                      </div>
                      <div class="flex items-center gap-x-3 gap-y-1 mt-1">
                        <p class="text-xs text-muted-foreground truncate">
                          {$t("download.file.hash")}: {file.hash}
                        </p>
                      </div>
                    </div>
                  </div>
                </div>

                <Badge
                  class={
                    file.status === "downloading"
                      ? "bg-blue-500 text-white border-blue-500"
                      : file.status === "completed"
                        ? "bg-green-500 text-white border-green-500"
                        : file.status === "paused"
                          ? "bg-yellow-400 text-white border-yellow-400"
                          : file.status === "queued"
                            ? "bg-gray-500 text-white border-gray-500"
                            : file.status === "canceled"
                              ? "bg-red-600 text-white border-red-600"
                              : "bg-red-500 text-white border-red-500"
                  }
                >
                  {file.status}
                </Badge>
              </div>
            </div>

            {#if file.status === "downloading" || file.status === "paused"}
              <div class="pb-2 ml-7">
                <div class="flex items-center justify-between text-sm mb-1">
                  <div class="flex items-center gap-4 text-muted-foreground">
                    <span>
                      Speed: {file.status === "paused" ? DEFAULT_SPEED : file.speed}
                    </span>
                    <span>ETA: {file.status === "paused" ? DEFAULT_ETA : file.eta}</span>
                  </div>
                  <span class="text-foreground">{file.progress.toFixed(2)}%</span>
                </div>
                <Progress
                  value={file.progress}
                  max={100}
                  class="h-2 bg-border [&>div]:bg-green-500 w-full"
                />
              </div>
            {/if}

            <div class="pt-2 ml-7">
              <div class="flex flex-wrap gap-2">
                {#if file.status === "downloading" || file.status === "paused"}
                  <Button
                    size="sm"
                    variant="outline"
                    on:click={() => togglePause(file)}
                    class="h-7 px-3 text-sm"
                  >
                    {#if file.status === "downloading"}
                      <Pause class="h-3 w-3 mr-1" />
                      {$t("download.actions.pause")}
                    {:else}
                      <Play class="h-3 w-3 mr-1" />
                      {$t("download.actions.resume")}
                    {/if}
                  </Button>
                  <Button
                    size="sm"
                    variant="destructive"
                    on:click={() => cancelDownload(file)}
                    class="h-7 px-3 text-sm"
                  >
                    <X class="h-3 w-3 mr-1" />
                    {$t("download.actions.cancel")}
                  </Button>
                {:else if file.status === "queued"}
                  <Button
                    size="sm"
                    variant="destructive"
                    on:click={() => cancelDownload(file)}
                    class="h-7 px-3 text-sm"
                  >
                    <X class="h-3 w-3 mr-1" />
                    {$t("download.actions.remove")}
                  </Button>
                {:else if file.status === "completed"}
                  {#if canPreviewFile(file.name)}
                    <Button
                      size="sm"
                      variant="outline"
                      on:click={() => openPreview(file)}
                      class="h-7 px-3 text-sm"
                    >
                      <Eye class="h-3 w-3 mr-1" />
                      {$t("download.actions.preview", { default: "Preview" })}
                    </Button>
                  {/if}
                  <Button
                    size="sm"
                    variant="outline"
                    on:click={showInFolder}
                    class="h-7 px-3 text-sm"
                  >
                    <FolderOpen class="h-3 w-3 mr-1" />
                    {$t("download.actions.showInFolder")}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    on:click={() => clearDownload(file)}
                    class="h-7 px-3 text-sm text-muted-foreground hover:text-destructive"
                    title={$t("download.actions.remove", { default: "Remove" })}
                  >
                    <X class="h-3 w-3" />
                  </Button>
                {:else if file.status === "failed" || file.status === "canceled"}
                  <Button
                    size="sm"
                    variant="outline"
                    on:click={() => retryDownload(file)}
                    class="h-7 px-3 text-sm"
                  >
                    <Play class="h-3 w-3 mr-1" />
                    {$t("download.actions.retry", { default: "Retry" })}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    on:click={() => clearDownload(file)}
                    class="h-7 px-3 text-sm text-muted-foreground hover:text-destructive"
                    title={$t("download.actions.remove", { default: "Remove" })}
                  >
                    <X class="h-3 w-3" />
                  </Button>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </Card>

  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <History class="h-5 w-5" />
        <h2 class="text-lg font-semibold">{$t("downloadHistory.title")}</h2>
        <Badge variant="secondary">{statistics?.total ?? 0}</Badge>
      </div>
      <Button size="sm" variant="outline" on:click={() => (showHistory = !showHistory)}>
        {showHistory ? $t("downloadHistory.hideHistory") : $t("downloadHistory.showHistory")}
        {#if showHistory}
          <ChevronUp class="h-4 w-4 ml-1" />
        {:else}
          <ChevronDown class="h-4 w-4 ml-1" />
        {/if}
      </Button>
    </div>

    {#if showHistory}
      <div class="mb-4 space-y-3">
        <div class="flex flex-wrap gap-2">
          <div class="relative flex-1 min-w-[200px]">
            <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              type="text"
              bind:value={historySearchQuery}
              placeholder={$t("downloadHistory.search")}
              class="pl-10"
            />
          </div>
          <div class="flex gap-2">
            <Button
              size="sm"
              variant={historyFilter === "all" ? "default" : "outline"}
              on:click={() => (historyFilter = "all")}
            >
              {$t("downloadHistory.filterAll")} ({statistics?.total ?? 0})
            </Button>
            <Button
              size="sm"
              variant={historyFilter === "completed" ? "default" : "outline"}
              on:click={() => (historyFilter = "completed")}
            >
              {$t("downloadHistory.filterCompleted")} ({statistics?.completed ?? 0})
            </Button>
            <Button
              size="sm"
              variant={historyFilter === "failed" ? "default" : "outline"}
              on:click={() => (historyFilter = "failed")}
            >
              {$t("downloadHistory.filterFailed")} ({statistics?.failed ?? 0})
            </Button>
            <Button
              size="sm"
              variant={historyFilter === "canceled" ? "default" : "outline"}
              on:click={() => (historyFilter = "canceled")}
            >
              {$t("downloadHistory.filterCanceled")} ({statistics?.canceled ?? 0})
            </Button>
          </div>
        </div>

        <div class="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" on:click={exportHistory}>
            <UploadIcon class="h-3 w-3 mr-1" />
            {$t("downloadHistory.exportHistory")}
          </Button>
          <Button size="sm" variant="outline" on:click={importHistory}>
            <DownloadIcon class="h-3 w-3 mr-1" />
            {$t("downloadHistory.importHistory")}
          </Button>
          {#if (statistics?.failed ?? 0) > 0}
            <Button
              size="sm"
              variant="outline"
              on:click={clearFailedHistory}
              class="text-orange-600 border-orange-600 hover:bg-orange-50"
            >
              <Trash2 class="h-3 w-3 mr-1" />
              {$t("downloadHistory.clearFailed")}
            </Button>
          {/if}
          {#if (statistics?.canceled ?? 0) > 0}
            <Button
              size="sm"
              variant="outline"
              on:click={clearCanceledHistory}
              class="text-orange-600 border-orange-600 hover:bg-orange-50"
            >
              <Trash2 class="h-3 w-3 mr-1" />
              {$t("downloadHistory.clearCanceled")}
            </Button>
          {/if}
          {#if downloadHistory.length > 0}
            <Button
              size="sm"
              variant="outline"
              on:click={clearAllHistory}
              class="text-destructive border-destructive hover:bg-destructive/10"
            >
              <Trash2 class="h-3 w-3 mr-1" />
              {$t("downloadHistory.clearHistory")}
            </Button>
          {/if}
        </div>
      </div>

      {#if downloadHistory.length === 0}
        <div class="text-center py-12 text-muted-foreground">
          <History class="h-12 w-12 mx-auto mb-3 opacity-50" />
          <p class="font-medium">{$t("downloadHistory.empty")}</p>
          <p class="text-sm">{$t("downloadHistory.emptyDescription")}</p>
        </div>
      {:else}
        <div class="space-y-2">
          {#each downloadHistory as entry (entry.id + entry.downloadDate)}
            <div class="flex items-center gap-3 p-3 rounded-lg border bg-card hover:bg-muted/50 transition-colors">
              <div class="flex-shrink-0">
                <svelte:component
                  this={getFileIcon(entry.name)}
                  class="h-5 w-5 text-muted-foreground"
                />
              </div>

              <div class="flex-1 min-w-0">
                <p class="font-medium truncate">{entry.name}</p>
                <p class="text-xs text-muted-foreground">
                  {toHumanReadableSize(entry.size)}
                  {#if entry.price}
                    · Payment total: {entry.price.toFixed(4)} Chiral
                    {#if formatPricePerMb(entry.price, entry.size)}
                      · Rate: {formatPricePerMb(entry.price, entry.size)} Chiral/MB
                    {/if}
                  {/if}
                  {#if entry.paymentStatus}
                    · Payment: {formatPaymentStatus(entry.paymentStatus)}
                  {/if}
                  · {new Date(entry.downloadDate).toLocaleString()}
                </p>
              </div>

              <Badge
                variant={
                  entry.status === "completed"
                    ? "default"
                    : entry.status === "failed"
                      ? "destructive"
                      : "secondary"
                }
              >
                {entry.status}
              </Badge>

              <div class="flex gap-1">
                <Button
                  size="sm"
                  variant="ghost"
                  on:click={() => redownloadFile(entry)}
                  title={$t("downloadHistory.redownload")}
                >
                  <RefreshCw class="h-4 w-4" />
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  on:click={() => removeHistoryEntry(entry.hash)}
                  title={$t("downloadHistory.remove")}
                  class="text-muted-foreground hover:text-destructive"
                >
                  <X class="h-4 w-4" />
                </Button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </Card>
</div>

<FilePreviewModal
  bind:isOpen={showPreviewModal}
  fileName={previewFileName}
  filePath={previewFilePath}
  fileSize={previewFileSize}
/>
