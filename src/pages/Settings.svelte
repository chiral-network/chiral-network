<script lang="ts">
  import Button from "$lib/components/ui/button.svelte";
  import Input from "$lib/components/ui/input.svelte";
  import Label from "$lib/components/ui/label.svelte";
  import Badge from "$lib/components/ui/badge.svelte";
  import DropDown from "$lib/components/ui/dropDown.svelte";
  import {
    Save,
    FolderOpen,
    HardDrive,
    Wifi,
    Shield,
    Bell,
    RefreshCw,
    Database,
    Languages,
    FileText,
    Activity,
    CheckCircle,
    AlertTriangle,
    Copy,
    Download as DownloadIcon,
    Upload as UploadIcon,
    Code,
  } from "lucide-svelte";
  import { onMount } from "svelte";
  import {open} from "@tauri-apps/plugin-dialog";
  import { homeDir } from "@tauri-apps/api/path";
  import { getVersion } from "@tauri-apps/api/app";
  import { userLocation } from "$lib/stores";
  import { GEO_REGIONS, UNKNOWN_REGION_ID } from '$lib/geo';
  import { changeLocale, loadLocale } from "../i18n/i18n";
  import { t } from "svelte-i18n";
  import { get } from "svelte/store";
  import { showToast } from "$lib/toast";
  import { invoke } from "@tauri-apps/api/core";
  import Expandable from "$lib/components/ui/Expandable.svelte";
  import { settings, activeBandwidthLimits, type AppSettings } from "$lib/stores";
  import { bandwidthScheduler } from "$lib/services/bandwidthScheduler";
  import { settingsBackupService } from "$lib/services/settingsBackupService";
  import { diagnosticLogger, errorLogger } from '$lib/diagnostics/logger';
  import { diagnosticsService, type DiagReport } from "$lib/services/diagnosticsService";
  import {
    validateStoragePath,
    validatePort,
    validateProxyAddress,
    validateBandwidth,
    validatePercentage,
    validateMultiaddr
  } from "$lib/utils/validation";
  import ProtocolTestPanel from '$lib/components/ProtocolTestPanel.svelte';
  import DownloadRestartControls from '$lib/components/download/DownloadRestartControls.svelte';

  const tr = (key: string, params?: Record<string, any>) => $t(key, params);

  let showResetConfirmModal = false;
  let storageSectionOpen = false;
  let networkSectionOpen = false;
  let advancedSectionOpen = false;
  let bandwidthSectionOpen = false;
  let languageSectionOpen = false;
  let privacySectionOpen = false;
  let notificationsSectionOpen = false;
  let diagnosticsSectionOpen = false;
  let developersSectionOpen = false;

  // Restartable HTTP Download variables
  let showRestartSection = false;
  let restartDownloadId = '';
  let restartSourceUrl = '';
  let restartDestinationPath = '';
  let restartSha256 = '';

  // Dynamic placeholder for storage path
  let storagePathPlaceholder = "Select download directory";

  const ACCORDION_STORAGE_KEY = "settingsAccordionState";

  type AccordionState = {
    storage: boolean;
    network: boolean;
    bandwidthScheduling: boolean;
    language: boolean;
    privacy: boolean;
    notifications: boolean;
    advanced: boolean;
    diagnostics: boolean;
    backupRestore: boolean;
    developers: boolean;
  };

  let accordionStateInitialized = false;

  // Settings state
  let defaultSettings: AppSettings = {
    // Storage settings
    storagePath: "", // Will be set to platform-specific default at runtime
    maxStorageSize: 100, // GB
    autoCleanup: true,
    cleanupThreshold: 90, // %

    // Network settings
    maxConnections: 50,
    uploadBandwidth: 0, // 0 = unlimited
    downloadBandwidth: 0, // 0 = unlimited
    monthlyUploadCapGb: 0, // 0 = unlimited
    monthlyDownloadCapGb: 0, // 0 = unlimited
    capWarningThresholds: [75, 90],
    port: 30303,
    enableUPnP: true,
    enableNAT: true,
    userLocation: "US-East", // Geographic region for peer sorting

    // Privacy settings
    enableProxy: true,
    proxyAddress: "127.0.0.1:9050", // Default Tor SOCKS address
    ipPrivacyMode: "off",
    trustedProxyRelays: [],
    disableDirectNatTraversal: false,
    enableAutonat: true,
    autonatProbeInterval: 30,
    autonatServers: [],
    anonymousMode: false,
    shareAnalytics: true,
    customBootstrapNodes: [],
    autoStartDHT: true, // Auto-start DHT by default
    autoStartGeth: true, // Auto-start Geth by default

    // Notifications
    enableNotifications: true,
    notifyOnComplete: true,
    notifyOnError: true,
    soundAlerts: false,
    notifyOnBandwidthCap: true,
    notifyOnBandwidthCapDesktop: false,

    // Advanced
    enableIPFS: false,
    chunkSize: 256, // KB
    cacheSize: 1024, // MB
    logLevel: "info",
    autoUpdate: true,
    pureClientMode: false,
    forceServerMode: false,
    enableWalletAutoLock: false,
    pricePerMb: 0.001, // Default price: 0.001 Chiral per MB
    enableBandwidthScheduling: false,
    bandwidthSchedules: [],
    enableFileLogging: false, // Logging to disk
    maxLogSizeMB: 10, // MB per log file

    // Upload Protocol
    selectedProtocol: "WebRTC", // Default to WebRTC
  };
  let localSettings: AppSettings = JSON.parse(JSON.stringify(get(settings)));
  let savedSettings: AppSettings = JSON.parse(JSON.stringify(localSettings));
  savedSettings = {...defaultSettings,...savedSettings}
  let hasChanges = false;
  let fileInputEl: HTMLInputElement | null = null;
  let selectedLanguage: string | undefined = undefined;
  let clearingCache = false;
  let cacheCleared = false;
  let importExportFeedback: {
    message: string;
    type: "success" | "error";
  } | null = null;

  // Diagnostics state
  let diagnosticsRunning = false;
  let diagnosticsReport: DiagReport | null = null;

  // Backup/Restore state
  let backupRestoreSectionOpen = false;
  let isExporting = false;
  let isImporting = false;
  let backupMessage: { text: string; type: 'success' | 'error' | 'warning' } | null = null;

  // NAT & privacy configuration text bindings
  let autonatServersText = '';
  let trustedProxyText = '';

  // Logs directory (loaded from backend)
  let logsDirectory: string | null = null;
  let newBootstrapNode = '';
  
  // Storage path validation state
  let storagePathWarning: string | null = null;

  const locationOptions = GEO_REGIONS
    .filter((region) => region.id !== UNKNOWN_REGION_ID)
    .map((region) => ({ value: region.label, label: region.label }))
    .sort((a, b) => a.label.localeCompare(b.label));

  let languages = [];
  // $: languages = [
  //   { value: "en", label: (get(t) as any)("language.english") },
  //   { value: "es", label: (get(t) as any)("language.spanish") },
  //   { value: "zh", label: (get(t) as any)("language.chinese") },
  //   { value: "ko", label: (get(t) as any)("language.korean") },
  //   { value: "ru", label: (get(t) as any)("language.russian") },
  // ];
  $: languages = [
    { value: "en", label: tr("language.english") },
    { value: "es", label: tr("language.spanish") },
    { value: "zh", label: tr("language.chinese") },
    { value: "ko", label: tr("language.korean") },
    { value: "ru", label: tr("language.russian") },
    { value: "pt", label: tr("language.portuguese") },
    { value: "hi", label: tr("language.hindi") },
    { value: "fr", label: tr("language.french") },
    { value: "bn", label: tr("language.bengali") },
    { value: "ar", label: tr("language.arabic") },
  ];

  // Initialize configuration text from arrays
  $: autonatServersText = localSettings.autonatServers?.join('\n') || '';
  $: trustedProxyText = localSettings.trustedProxyRelays?.join('\n') || '';

  const privacyModeOptions = [
    {
      value: "off",
      label: "Off (direct connections allowed)",
    },
    {
      value: "prefer",
      label: "Prefer Proxy (fall back to direct if needed)",
    },
    {
      value: "strict",
      label: "Strict Proxy-only (never expose your IP)",
    },
  ];

  type PrivacySnapshot = Pick<
    AppSettings,
    "ipPrivacyMode" | "enableProxy" | "proxyAddress" | "disableDirectNatTraversal"
  >;

  let anonymousModeRestore: PrivacySnapshot | null = null;
  let prevNetworkError = false;
  let prevAdvancedError = false;

  onMount(() => {
    if (typeof window === "undefined") {
      accordionStateInitialized = true;
      return;
    }

    try {
      const storedAccordion = window.localStorage.getItem(ACCORDION_STORAGE_KEY);
      if (storedAccordion) {
        const parsed = JSON.parse(storedAccordion) as Partial<AccordionState>;
        if (typeof parsed.storage === "boolean") storageSectionOpen = parsed.storage;
        if (typeof parsed.network === "boolean") networkSectionOpen = parsed.network;
        if (typeof parsed.bandwidthScheduling === "boolean") bandwidthSectionOpen = parsed.bandwidthScheduling;
        if (typeof parsed.language === "boolean") languageSectionOpen = parsed.language;
        if (typeof parsed.privacy === "boolean") privacySectionOpen = parsed.privacy;
        if (typeof parsed.notifications === "boolean") notificationsSectionOpen = parsed.notifications;
        if (typeof parsed.advanced === "boolean") advancedSectionOpen = parsed.advanced;
        if (typeof parsed.diagnostics === "boolean") diagnosticsSectionOpen = parsed.diagnostics;
        if (typeof parsed.backupRestore === "boolean") backupRestoreSectionOpen = parsed.backupRestore;
        if (typeof parsed.developers === "boolean") developersSectionOpen = parsed.developers;
      }
    } catch (error) {
      diagnosticLogger.warn('Settings', 'Failed to restore settings accordion state', { error: error instanceof Error ? error.message : String(error) });
    } finally {
      accordionStateInitialized = true;
    }
  });

  $: if (accordionStateInitialized && typeof window !== "undefined") {
    try {
      const accordionState: AccordionState = {
        storage: storageSectionOpen,
        network: networkSectionOpen,
        bandwidthScheduling: bandwidthSectionOpen,
        language: languageSectionOpen,
        privacy: privacySectionOpen,
        notifications: notificationsSectionOpen,
        advanced: advancedSectionOpen,
        diagnostics: diagnosticsSectionOpen,
        backupRestore: backupRestoreSectionOpen,
        developers: developersSectionOpen,
      };
      window.localStorage.setItem(ACCORDION_STORAGE_KEY, JSON.stringify(accordionState));
    } catch (error) {
      diagnosticLogger.warn('Settings', 'Failed to persist settings accordion state', { error: error instanceof Error ? error.message : String(error) });
    }
  }

  const formatBandwidthLimit = (limitKbps: number): string => {
    if (!Number.isFinite(limitKbps) || limitKbps <= 0) {
      return "Unlimited";
    }
    return `${limitKbps} KB/s`;
  };

  function formatNextChange(timestamp?: number): string {
    if (!timestamp || !Number.isFinite(timestamp)) {
      return "Not scheduled";
    }

    const deltaMs = timestamp - Date.now();
    if (deltaMs <= 0) {
      return new Date(timestamp).toLocaleString();
    }

    const deltaMinutes = Math.round(deltaMs / 60000);
    if (deltaMinutes < 60) {
      return `in ${deltaMinutes} min`;
    }

    const deltaHours = Math.round(deltaMs / 3600000);
    if (deltaHours < 24) {
      return `in ${deltaHours} hr`;
    }

    return new Date(timestamp).toLocaleString();
  }

  function capturePrivacySnapshot(): void {
    if (anonymousModeRestore !== null) {
      return;
    }
    anonymousModeRestore = {
      ipPrivacyMode: localSettings.ipPrivacyMode,
      enableProxy: localSettings.enableProxy,
      proxyAddress: localSettings.proxyAddress,
      disableDirectNatTraversal: localSettings.disableDirectNatTraversal,
    };
  }

  function applyAnonymousDefaults(): void {
    capturePrivacySnapshot();

    const needsUpdate =
      localSettings.ipPrivacyMode !== "strict" ||
      !localSettings.enableProxy ||
      !localSettings.disableDirectNatTraversal;

    if (!needsUpdate) {
      return;
    }

    localSettings = {
      ...localSettings,
      ipPrivacyMode: "strict",
      enableProxy: true,
      disableDirectNatTraversal: true,
    };
  }

  function restorePrivacySnapshot(): void {
    if (!anonymousModeRestore) {
      return;
    }

    const snapshot = anonymousModeRestore;
    anonymousModeRestore = null;

    localSettings = {
      ...localSettings,
      ipPrivacyMode: snapshot.ipPrivacyMode,
      enableProxy: snapshot.enableProxy,
      proxyAddress: snapshot.proxyAddress,
      disableDirectNatTraversal: snapshot.disableDirectNatTraversal,
    };
  }

  $: if (localSettings.anonymousMode) {
    applyAnonymousDefaults();
  } else {
    restorePrivacySnapshot();
  }

  $: privacyStatus = (() => {
    switch (localSettings.ipPrivacyMode) {
      case "prefer":
        return "Proxy routing will be attempted first. If no proxy is available, the app falls back to a direct connection and your IP may be exposed.";
      case "strict":
        return "All traffic must tunnel through a trusted proxy. Direct connections and IP exposure are blocked.";
      default:
        return "Direct connections are allowed. Use a SOCKS5 proxy if you still want to mask your IP.";
    }
  })();

  // Check for changes
  $: hasChanges = JSON.stringify(localSettings) !== JSON.stringify(savedSettings);

  // Check for validation errors
  $: hasValidationErrors = Object.values(errors).some(error => error !== null) || !!maxStorageError;

  async function saveSettings() {
    // Prevent saving if there are validation errors
    if (hasValidationErrors) {
      showToast("Please fix validation errors before saving", "error");
      return;
    }

    const sanitizedThresholds = Array.isArray(localSettings.capWarningThresholds)
      ? Array.from(
          new Set(
            localSettings.capWarningThresholds
              .map((value) => Math.round(Number(value)))
              .filter(
                (value) => Number.isFinite(value) && value > 0 && value <= 100
              )
          )
        ).sort((a, b) => a - b)
      : [];

    localSettings = {
      ...localSettings,
      capWarningThresholds: sanitizedThresholds,
    };

    // Save local changes to the Svelte store
    settings.set(localSettings);

    // Save to local storage (for web compatibility)
    localStorage.setItem("chiralSettings", JSON.stringify(localSettings));

    // Save to Tauri app data directory (for backend access)

    let isTauri = false;
    try {
      await getVersion();
      isTauri = true;
    } catch {

    }
  if (isTauri) {
      try {
        await invoke("save_app_settings", {
          settingsJson: JSON.stringify(localSettings),
        });
      } catch (error) {
        errorLogger.fileOperationError('Save settings', error instanceof Error ? error.message : String(error));
      }
    }

    savedSettings = JSON.parse(JSON.stringify(localSettings));
    userLocation.set(localSettings.userLocation);

    // Force bandwidth scheduler to update with new settings
    bandwidthScheduler.forceUpdate();

    importExportFeedback = null;

    try {
      await applyPrivacyRoutingSettings();
      await restartDhtWithProxy();
      await updateLogConfiguration();
      showToast(tr('toasts.settings.updated'));
    } catch (error) {
      errorLogger.networkError(`Failed to apply networking settings: ${error instanceof Error ? error.message : String(error)}`);
      showToast(tr('toasts.settings.networkingError'), "error");
    }
  }

// Logging improvements
  async function updateLogConfiguration() {
    if (typeof window === "undefined" || !window.navigator.userAgent.includes("tauri")) {
      return;
    }

    try {
      await invoke("update_log_config", {
        maxLogSizeMb: localSettings.maxLogSizeMB,
        enabled: localSettings.enableFileLogging,
      });
    } catch (error) {
      diagnosticLogger.warn('Settings', 'Failed to update log configuration', { error: error instanceof Error ? error.message : String(error) });
    }
  }


  // Backup/Restore Functions
  async function exportSettings() {
    isExporting = true;
    backupMessage = null;

    try {
      const result = await settingsBackupService.exportSettings(true);

      if (result.success && result.data) {
        // Download as file
        settingsBackupService.downloadBackupFile(result.data);
        backupMessage = {
          text: $t('settingsBackup.messages.exportSuccess'),
          type: 'success'
        };
        showToast($t('settingsBackup.messages.exportSuccess'), 'success');
      } else {
        throw new Error(result.error || 'Export failed');
      }
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : 'Unknown error';
      backupMessage = {
        text: $t('settingsBackup.messages.exportError', { values: { error: errorMsg } }),
        type: 'error'
      };
      showToast(backupMessage.text, 'error');
    } finally {
      isExporting = false;

      // Clear message after 5 seconds
      setTimeout(() => {
        backupMessage = null;
      }, 5000);
    }
  }

  async function importSettings() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.json';

    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;

      isImporting = true;
      backupMessage = null;

      try {
        const text = await file.text();
        const result = await settingsBackupService.importSettings(text, { merge: false });

        if (result.success && result.imported) {
          // Update local settings from imported data
          localSettings = { ...result.imported };
          savedSettings = JSON.parse(JSON.stringify(localSettings));
          settings.set(localSettings);

          if (result.warnings && result.warnings.length > 0) {
            backupMessage = {
              text: $t('settingsBackup.messages.importWarnings', { values: { warnings: result.warnings.join(', ') } }),
              type: 'warning'
            };
            showToast(backupMessage.text, 'warning');
          } else {
            backupMessage = {
              text: $t('settingsBackup.messages.importSuccess'),
              type: 'success'
            };
            showToast(backupMessage.text, 'success');
          }

          // Apply new settings
          await saveSettings();
        } else {
          throw new Error(result.error || 'Import failed');
        }
      } catch (error) {
        const errorMsg = error instanceof Error ? error.message : 'Unknown error';
        backupMessage = {
          text: $t('settingsBackup.messages.importError', { values: { error: errorMsg } }),
          type: 'error'
        };
        showToast(backupMessage.text, 'error');
      } finally {
        isImporting = false;

        // Clear message after 5 seconds
        setTimeout(() => {
          backupMessage = null;
        }, 5000);
      }
    };

    input.click();
  }


  async function applyPrivacyRoutingSettings() {
    if (typeof window === "undefined" || !("__TAURI__" in window)) {
      return;
    }

    if (localSettings.ipPrivacyMode !== "off" && (!localSettings.trustedProxyRelays || localSettings.trustedProxyRelays.length === 0)) {
      showToast(tr('toasts.settings.proxyRelayWarning'), "warning");
      try {
        await invoke("disable_privacy_routing");
      } catch (error) {
        diagnosticLogger.warn('Settings', 'disable_privacy_routing failed while updating privacy settings', { error: error instanceof Error ? error.message : String(error) });
      }
      return;
    }

    if (localSettings.ipPrivacyMode === "off") {
      try {
        await invoke("disable_privacy_routing");
      } catch (error) {
        diagnosticLogger.warn('Settings', 'disable_privacy_routing failed while turning privacy off', { error: error instanceof Error ? error.message : String(error) });
      }
      return;
    }

    await invoke("enable_privacy_routing", {
      proxyAddresses: localSettings.trustedProxyRelays,
      mode: localSettings.ipPrivacyMode,
    });
  }

  async function restartDhtWithProxy() {
    if (typeof window === "undefined" || !("__TAURI__" in window)) {
      return;
    }

    try {
      await invoke("stop_dht_node");
    } catch (error) {
      diagnosticLogger.debug('Settings', 'stop_dht_node failed (probably already stopped)', { error: error instanceof Error ? error.message : String(error) });
    }

    // Use custom bootstrap nodes if configured, otherwise use defaults
    let bootstrapNodes: string[] = [];
    if (localSettings.customBootstrapNodes && localSettings.customBootstrapNodes.length > 0) {
      bootstrapNodes = localSettings.customBootstrapNodes;
    } else {
      try {
        bootstrapNodes = await invoke<string[]>("get_bootstrap_nodes_command");
      } catch (error) {
        errorLogger.networkError(`Failed to fetch bootstrap nodes: ${error instanceof Error ? error.message : String(error)}`);
        throw error;
      }

      if (!Array.isArray(bootstrapNodes) || bootstrapNodes.length === 0) {
        throw new Error("No bootstrap nodes available to restart DHT");
      }
    }

    const payload: Record<string, unknown> = {
      port: localSettings.port,
      bootstrapNodes,
      enableAutonat: !localSettings.disableDirectNatTraversal,
      autonatProbeIntervalSecs: localSettings.autonatProbeInterval,
      chunkSizeKb: localSettings.chunkSize,
      cacheSizeMb: localSettings.cacheSize,
      enableUpnp: localSettings.enableUPnP,
    };

    if (localSettings.autonatServers?.length) {
      payload.autonatServers = localSettings.autonatServers;
    }
    if (localSettings.enableProxy && localSettings.proxyAddress?.trim()) {
      payload.proxyAddress = localSettings.proxyAddress.trim();
    }

    await invoke("start_dht_node", payload);
  }

  $: {
    // Open Storage section if it has any errors (but don't close it if already open)
    const hasStorageError = !!maxStorageError || !!errors.maxStorageSize || !!errors.cleanupThreshold || !!errors.storagePath;
    if (hasStorageError) storageSectionOpen = true;

    // Open Network section if it has any errors (but don't close it if already open)
    const hasNetworkError =
      !!errors.maxConnections ||
      !!errors.port ||
      !!errors.uploadBandwidth ||
      !!errors.downloadBandwidth ||
      !!errors.monthlyUploadCapGb ||
      !!errors.monthlyDownloadCapGb ||
      !!errors.capWarningThresholds;
    if (hasNetworkError && (!accordionStateInitialized || !prevNetworkError)) networkSectionOpen = true;

    // Open Advanced section if it has any errors (but don't close it if already open)
    const hasAdvancedError = !!errors.chunkSize || !!errors.cacheSize;
    if (hasAdvancedError && (!accordionStateInitialized || !prevAdvancedError)) advancedSectionOpen = true;

    prevNetworkError = hasNetworkError;
    prevAdvancedError = hasAdvancedError;
  }

  async function handleConfirmReset() {
    // Set platform-specific default storage path
    // Get platform-specific default storage path from backend
    let defaultPath = "~/Downloads/Chiral-Network-Storage";
    try {
      defaultPath = await invoke("get_default_storage_directory");
    } catch (e) {
      errorLogger.fileOperationError('Get default storage directory (reset)', e instanceof Error ? e.message : String(e));
    }
    localSettings = { ...defaultSettings, storagePath: defaultPath };
    settings.set(localSettings); // Reset the store
    await saveSettings(); // Save the reset state
    showResetConfirmModal = false;
  }

  function openResetConfirm() {
    showResetConfirmModal = true;
  }

  async function selectStoragePath() {
    // const tr = (k: string, params?: Record<string, any>) => (get(t) as any)(k, params);
    try {
      // Try Tauri first
      await getVersion(); // only works in Tauri
      const home = await homeDir();
      const result = await open({
        directory: true,
        multiple: false,
        defaultPath: localSettings.storagePath.startsWith("~/")
          ? localSettings.storagePath.replace("~", home)
          : localSettings.storagePath,
        title: tr("storage.selectLocationTitle"),
      });

      if (typeof result === "string") {
        // Reassign the entire object to trigger reactivity
        localSettings = { ...localSettings, storagePath: result };
      }
    } catch {
      // Fallback for browser environment
      if ("showDirectoryPicker" in window) {
        // Use File System Access API (Chrome/Edge)
        try {
          const directoryHandle = await (window as any).showDirectoryPicker();
          // Reassign the entire object to trigger reactivity
          localSettings = { ...localSettings, storagePath: directoryHandle.name };
        } catch (err: any) {
          if (err.name !== "AbortError") {
            errorLogger.fileOperationError('Directory picker', err instanceof Error ? err.message : String(err));
          }
        }
      } else {
        // Fallback: let user type path manually
        const newPath = prompt(
          `${tr("storage.enterPathPrompt")} ( ${tr("storage.browserNoPicker")} )`,
          localSettings.storagePath
        );
        if (newPath) {
          // Reassign the entire object to trigger reactivity
          localSettings = { ...localSettings, storagePath: newPath };
        }
      }
    }
  }

  async function clearCache() {
    clearingCache = true;
    // Simulate cache clearing work
    await new Promise((resolve) => setTimeout(resolve, 1500));
    clearingCache = false;
    cacheCleared = true;
    // Reset success state after 2 seconds
    setTimeout(() => {
      cacheCleared = false;
    }, 2000);
  }

  function exportSettingsOld() {
    const blob = new Blob([JSON.stringify(localSettings, null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "chiral-settings.json";
    a.click();
    URL.revokeObjectURL(url);
    importExportFeedback = {
      message: tr("advanced.exportSuccess", {
        default: "Settings exported to your browser's download folder.",
      }),
      type: "success",
    };
  }

  function importSettingsOld(event: Event) {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = async (e) => {
      try {
        const imported = JSON.parse(e.target?.result as string);
        localSettings = { ...localSettings, ...imported };
        await saveSettings(); // This saves, updates savedSettings, and clears any old feedback.
        // Now we set the new feedback for the import action.
        importExportFeedback = {
          message: tr("advanced.importSuccess", {
            default: "Settings imported successfully.",
          }),
          type: "success",
        };
      } catch (err) {
        errorLogger.fileOperationError('Import settings', err instanceof Error ? err.message : String(err));
        importExportFeedback = {
          message: tr("advanced.importError", {
            default: "Invalid JSON file. Please select a valid export.",
          }),
          type: "error",
        };
      }
    };
    reader.readAsText(file);

    // allow re-uploading the same file later
    (event.target as HTMLInputElement).value = "";
  }

  function updateAutonatServers() {
    localSettings.autonatServers = autonatServersText
      .split('\n')
      .map(s => s.trim())
      .filter(s => s.length > 0);
  }

  function updateTrustedProxyRelays() {
    localSettings.trustedProxyRelays = trustedProxyText
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
  }

  function addBootstrapNode() {
    const trimmed = newBootstrapNode.trim();
    if (!trimmed) {
      showToast("Bootstrap node address cannot be empty", "error");
      return;
    }

    // Validate multiaddress format
    const validation = validateMultiaddr(trimmed);
    if (!validation.isValid) {
      showToast(validation.error || "Invalid multiaddress format", "error");
      return;
    }

    // Check for duplicates
    if (localSettings.customBootstrapNodes.includes(trimmed)) {
      showToast("This bootstrap node is already added", "warning");
      return;
    }

    localSettings.customBootstrapNodes = [...localSettings.customBootstrapNodes, trimmed];
    newBootstrapNode = '';
    showToast("Bootstrap node added successfully", "success");
  }

  function removeBootstrapNode(index: number) {
    localSettings.customBootstrapNodes = localSettings.customBootstrapNodes.filter((_, i) => i !== index);
  }

  async function runDiagnostics() {
    diagnosticsRunning = true;
    diagnosticsReport = null;

    try {
      // Run all diagnostic checks
      const report = await diagnosticsService.runAll();
      diagnosticsReport = report;
    } catch (e) {
      console.error("Diagnostics failed:", e);
      showToast(tr("settings.diagnostics.copyFailed"), "error");
    } finally {
      diagnosticsRunning = false;
    }
  }

  async function copyDiagnostics() {
    if (!diagnosticsReport) return;

    try {
      const reportText = diagnosticsService.formatReport(diagnosticsReport);
      await navigator.clipboard.writeText(reportText);
      showToast(tr("settings.diagnostics.copied"));
    } catch (e) {
      showToast(tr("settings.diagnostics.copyFailed"), "error");
    }
  }

  async function chooseRestartDestination() {
    try {
      const defaultDir = await homeDir();
      const suggestedPath =
        restartDestinationPath || `${defaultDir.replace(/\/$/, '')}/Downloads/restart-download.bin`;
      const { save } = await import('@tauri-apps/plugin-dialog');
      const selection = await save({
        defaultPath: suggestedPath,
        filters: [
          {
            name: 'All Files',
            extensions: ['*']
          }
        ]
      });
      if (selection) {
        restartDestinationPath = selection;
      }
    } catch (error) {
      console.error('Failed to choose destination path', error);
    }
  }

    onMount(async () => {
    // Get the canonical download directory from backend (single source of truth)
    try {
      // Pass null/undefined to get the default when no frontend settings exist
      const defaultPath = await invoke<string>("get_download_directory");
      defaultSettings.storagePath = defaultPath;
      storagePathPlaceholder = defaultPath; // Update placeholder to show actual default
    } catch (e) {
      errorLogger.fileOperationError('Get download directory', e instanceof Error ? e.message : String(e));
      // Fallback if backend fails
      defaultSettings.storagePath = "~/Downloads/Chiral";
      storagePathPlaceholder = "~/Downloads/Chiral";
    }

    // Load settings from local storage
    const stored = localStorage.getItem("chiralSettings");
    if (stored) {
  try {
    const loadedSettings: AppSettings = JSON.parse(stored);
    // Ensure storagePath is set to default if missing or empty
    if (!loadedSettings.storagePath || loadedSettings.storagePath.trim() === "") {
      loadedSettings.storagePath = defaultSettings.storagePath;
    }
    // Set the store, which ensures it is available globally
    settings.set({ ...defaultSettings, ...loadedSettings });
    // Update local state from the store after loading
    localSettings = JSON.parse(JSON.stringify(get(settings)));
    savedSettings = JSON.parse(JSON.stringify(localSettings));
  } catch (e) {
    errorLogger.fileOperationError('Load settings', e instanceof Error ? e.message : String(e));
  }
}

const saved = await loadLocale(); // 'en' | 'ko' | null
const initial = saved || "en";
selectedLanguage = initial; // Synchronize dropdown display value
// (From root, setupI18n() has already been called, so only once here)

// Fetch logs directory from backend (Tauri only)
  try {
    await getVersion();
    logsDirectory = await invoke("get_logs_directory");
  } catch (error) {
    errorLogger.fileOperationError('Get logs directory', error instanceof Error ? error.message : String(error));
  }

});

  async function onLanguageChange(lang: string) {
    selectedLanguage = lang;
    changeLocale(lang); // Save + update global state (yes, i18n.ts takes care of saving)
    (settings as any).language = lang;
    await saveSettings(); // If you want to reflect in settings as well
  }

  const limits = {
    maxStorageSize: { min: 10, max: 10000, label: "Max Storage Size (GB)" },
    cleanupThreshold: {
      min: 50,
      max: 100,
      label: "Auto-Cleanup Threshold (%)",
    },
    maxConnections: { min: 10, max: 200, label: "Max Connections" },
    port: { min: 1024, max: 65535, label: "Port" },
    uploadBandwidth: { min: 0, max: Infinity, label: "Upload Limit (MB/s)" },
    downloadBandwidth: {
      min: 0,
      max: Infinity,
      label: "Download Limit (MB/s)",
    },
    monthlyUploadCapGb: { min: 0, max: 100000, label: "Monthly Upload Cap (GB)" },
    monthlyDownloadCapGb: { min: 0, max: 100000, label: "Monthly Download Cap (GB)" },
    chunkSize: { min: 64, max: 1024, label: "Chunk Size (KB)" },
    cacheSize: { min: 256, max: 8192, label: "Cache Size (MB)" },
  } as const;

  let errors: Record<string, string | null> = {};

  let capThresholdInput = (localSettings.capWarningThresholds ?? []).join(", ");
  let editingCapThresholds = false;

  function parseCapThresholds(value: string): number[] {
    if (!value) {
      return [];
    }

    const tokens = value
      .split(/[,\\s]+/)
      .map((token) => token.trim())
      .filter(Boolean);

    if (tokens.length === 0) {
      return [];
    }

    const numbers = tokens
      .map((token) => Number.parseFloat(token))
      .filter((num) => Number.isFinite(num));

    const sanitized = Array.from(new Set(numbers.map((num) => Math.round(num))))
      .filter((num) => num > 0 && num <= 100)
      .sort((a, b) => a - b);

    return sanitized;
  }

  function handleCapThresholdInput(event: Event) {
    const target = event.target as HTMLInputElement;
    capThresholdInput = target.value;
  }

  function handleCapThresholdFocus() {
    editingCapThresholds = true;
  }

  function handleCapThresholdBlur() {
    const parsed = parseCapThresholds(capThresholdInput);
    capThresholdInput = parsed.join(", ");
    localSettings.capWarningThresholds = parsed;
    localSettings = { ...localSettings };
    editingCapThresholds = false;
  }

  $: if (!editingCapThresholds) {
    capThresholdInput = (localSettings.capWarningThresholds ?? []).join(", ");
  }

  function rangeMessage(label: string, min: number, max: number) {
    if (max === Infinity) return `${label} must be >= ${min}.`;
    return `${label} must be between ${min} and ${max}.`;
  }

  function validate(localSettings: any) {
    const next: Record<string, string | null> = {};
    for (const [key, cfg] of Object.entries(limits)) {
        const val = Number((localSettings as any)[key]);
        if (val < cfg.min || val > cfg.max) {
            next[key] = rangeMessage(cfg.label, cfg.min, cfg.max);
        }
    }

    // Validate port number
    if (localSettings.port) {
      const portValidation = validatePort(localSettings.port);
      if (!portValidation.isValid) {
        next.port = portValidation.error || "Invalid port";
      }
    }

    // Validate bandwidth limits
    if (localSettings.uploadBandwidth !== undefined && localSettings.uploadBandwidth !== 0) {
      const bwValidation = validateBandwidth(localSettings.uploadBandwidth, 1000000); // Max 1 GB/s
      if (!bwValidation.isValid) {
        next.uploadBandwidth = bwValidation.error || "Invalid upload bandwidth";
      }
    }

    if (localSettings.downloadBandwidth !== undefined && localSettings.downloadBandwidth !== 0) {
      const bwValidation = validateBandwidth(localSettings.downloadBandwidth, 1000000); // Max 1 GB/s
      if (!bwValidation.isValid) {
        next.downloadBandwidth = bwValidation.error || "Invalid download bandwidth";
      }
    }

    // Validate proxy address if proxy is enabled
    if (localSettings.enableProxy && localSettings.proxyAddress && localSettings.proxyAddress.trim()) {
      const proxyValidation = validateProxyAddress(localSettings.proxyAddress);
      if (!proxyValidation.isValid) {
        next.proxyAddress = proxyValidation.error || "Invalid proxy address";
      }
    }

    // Validate cleanup threshold percentage
    if (localSettings.cleanupThreshold !== undefined) {
      const percentValidation = validatePercentage(localSettings.cleanupThreshold, 50, 99);
      if (!percentValidation.isValid) {
        next.cleanupThreshold = percentValidation.error || "Invalid cleanup threshold";
      }
    }

    const thresholds = Array.isArray(localSettings.capWarningThresholds)
      ? localSettings.capWarningThresholds
      : [];

    const hasInvalidThreshold = thresholds.some(
      (value: number) => !Number.isFinite(value) || value <= 0 || value > 100
    );

    if (hasInvalidThreshold) {
      next.capWarningThresholds = "Thresholds must be between 1 and 100.";
    } else if (thresholds.length > 6) {
      next.capWarningThresholds = "Keep warning thresholds to six entries or fewer.";
    } else {
      next.capWarningThresholds = null;
    }

    // Validate storage path
    if (localSettings.storagePath && localSettings.storagePath.trim()) {
      // First do basic frontend validation
      const pathValidation = validateStoragePath(localSettings.storagePath);
      if (!pathValidation.isValid) {
        next.storagePath = pathValidation.error || "Invalid storage path";
      } else {
        // Don't clear the error yet if we have a backend error
        // This preserves platform-specific validation errors from backend
        if (!errors.storagePath) {
          next.storagePath = null;
        } else {
          next.storagePath = errors.storagePath;
        }
      }
      
      // Schedule backend validation (debounced)
      scheduleBackendValidation(localSettings.storagePath);
    } else {
      next.storagePath = null; // Empty is allowed (will use default)
      storagePathWarning = null; // Clear any warnings when path is empty
    }

    errors = next;
}

  // Backend validation for storage path with platform-specific checks
  async function validateStoragePathBackend(path: string) {
    if (!path || !path.trim()) {
      storagePathWarning = null;
      // Also clear any backend errors
      if (errors.storagePath) {
        errors = { ...errors, storagePath: null };
      }
      return;
    }
    
    // Clear previous warning
    storagePathWarning = null;
    
    try {
      await invoke("validate_storage_path", { path });
      // If successful, clear any previous backend errors
      if (errors.storagePath) {
        errors = { ...errors, storagePath: null };
      }
    } catch (error) {
      const errorMsg = String(error);
      // Check if it's a warning (directory doesn't exist)
      if (errorMsg.startsWith("WARNING:")) {
        storagePathWarning = errorMsg.substring(9); // Remove "WARNING: " prefix
        // Warning doesn't block saving, clear error
        if (errors.storagePath) {
          errors = { ...errors, storagePath: null };
        }
      } else {
        // It's an actual error, update the errors object
        errors = { ...errors, storagePath: errorMsg };
        // Clear warning when there's an error
        storagePathWarning = null;
      }
    }
  }


  // Debounced backend validation to avoid too many calls while typing
  let backendValidationTimeout: NodeJS.Timeout | null = null;
  let isTauri = false;
  async function detectTauri() {
    try {
      await getVersion();
      isTauri = true;
    } catch {
      isTauri = false;
    }
  }
  onMount(() => {
    detectTauri();
  });

  function scheduleBackendValidation(path: string) {
    if (backendValidationTimeout) {
      clearTimeout(backendValidationTimeout);
    }
    backendValidationTimeout = setTimeout(() => {
      console.log('Scheduling backend validation for path:', path);
      console.log('Is Tauri:', isTauri);
      if (isTauri) {
        console.log('Passed check:', path);
        validateStoragePathBackend(path).catch((err) => {
          diagnosticLogger.debug('Settings', 'Storage path backend validation error', { error: String(err) });
        });
      }
    }, 500); // Wait 500ms after user stops typing
  }

  // Revalidate whenever settings change
  $: validate(localSettings);

  // Computed CSS class for storage path input
  $: storagePathInputClass = `flex-1 ${
    errors.storagePath ? 'border-red-500 focus:border-red-500 ring-red-500' : 
    storagePathWarning ? 'border-yellow-500 focus:border-yellow-500 ring-yellow-500' : ''
  }`;

  let freeSpaceGB: number | null = null;
  let maxStorageError: string | null = null;

  onMount(async () => {
    freeSpaceGB = await invoke('get_available_storage');
  });

  $: {
    if (freeSpaceGB !== null && localSettings.maxStorageSize > freeSpaceGB) {
      maxStorageError = `Insufficient disk space. Only ${freeSpaceGB} GB available.`;
    } else {
      maxStorageError = null;
    }
  }

  let search = '';

// const sectionLabels: Record<string, string[]> = {
//   storage: [
//     (get(t) as any)("storage.title"),
//     (get(t) as any)("storage.location"),
//     (get(t) as any)("storage.maxSize"),
//     (get(t) as any)("storage.cleanupThreshold"),
//     (get(t) as any)("storage.enableCleanup"),
//   ],
//   network: [
//     (get(t) as any)("network.title"),
//     (get(t) as any)("network.maxConnections"),
//     (get(t) as any)("network.port"),
//     (get(t) as any)("network.uploadLimit"),
//     (get(t) as any)("network.downloadLimit"),
//     (get(t) as any)("network.userLocation"),
//     (get(t) as any)("network.enableUpnp"),
//     (get(t) as any)("network.enableNat"),
//     (get(t) as any)("network.enableDht"),
//     "Bootstrap Nodes",
//     "Custom Bootstrap Nodes",
//   ],
//   bandwidthScheduling: [
//     "Bandwidth Scheduling",
//     "Enable Bandwidth Scheduling",
//     "Schedule different bandwidth limits",
//   ],
//   language: [
//     (get(t) as any)("language.title"),
//     (get(t) as any)("language.select"),
//   ],
//   privacy: [
//     (get(t) as any)("privacy.title"),
//     (get(t) as any)("privacy.enableProxy"),
//     (get(t) as any)("privacy.anonymousMode"),
//     (get(t) as any)("privacy.shareAnalytics"),
//   ],
//   notifications: [
//     (get(t) as any)("notifications.title"),
//     (get(t) as any)("notifications.enable"),
//     (get(t) as any)("notifications.notifyComplete"),
//     (get(t) as any)("notifications.notifyError"),
//     (get(t) as any)("notifications.soundAlerts"),
//   ],
//   advanced: [
//     (get(t) as any)("advanced.title"),
//     (get(t) as any)("advanced.chunkSize"),
//     (get(t) as any)("advanced.cacheSize"),
//     (get(t) as any)("advanced.logLevel"),
//     (get(t) as any)("advanced.autoUpdate"),
//     (get(t) as any)("advanced.exportSettings"),
//     (get(t) as any)("advanced.importSettings"),
//   ],
// };

// function sectionMatches(section: string, query: string) {
//   if (!query) return true;
//   const labels = sectionLabels[section] || [];
//   return labels.some((label) =>
//     label.toLowerCase().includes(query.toLowerCase())
//   );
// }
let sectionLabels: Record<string, string[]> = {};
$: sectionLabels = {
  storage: [
    tr("storage.title"),
    tr("storage.location"),
    tr("storage.maxSize"),
    tr("storage.cleanupThreshold"),
    tr("storage.enableCleanup"),
  ],
  network: [
    tr("network.title"),
    tr("network.maxConnections"),
    tr("network.port"),
    tr("network.uploadLimit"),
    tr("network.downloadLimit"),
    tr("network.userLocation"),
    tr("network.enableUpnp"),
    tr("network.enableNat"),
    tr("network.enableDht"),
    "Bootstrap Nodes",
    "Custom Bootstrap Nodes",
  ],
  bandwidthScheduling: [
    "Bandwidth Scheduling",
    "Enable Bandwidth Scheduling",
    "Schedule different bandwidth limits",
  ],
  language: [
    tr("language.title"),
    tr("language.select"),
  ],
  privacy: [
    tr("privacy.title"),
    tr("privacy.enableProxy"),
    tr("privacy.anonymousMode"),
    tr("privacy.shareAnalytics"),
    tr("privacy.autoLockWallet"),
  ],
  notifications: [
    tr("notifications.title"),
    tr("notifications.enable"),
    tr("notifications.notifyComplete"),
    tr("notifications.notifyError"),
    tr("notifications.soundAlerts"),
  ],
  logs: [
    "Logs",
    "Enable File Logging",
    "Maximum Log File Size",
    "Disk logging",
    "Debug logs",
  ],
  advanced: [
    tr("advanced.title"),
    tr("advanced.chunkSize"),
    tr("advanced.cacheSize"),
    tr("advanced.logLevel"),
    tr("advanced.autoUpdate"),
    tr("advanced.exportSettings"),
    tr("advanced.importSettings"),
  ],
};

function sectionMatches(section: string, query: string) {
  if (!query) return true;
  const labels = sectionLabels[section] || [];
  return labels.some((label) =>
    label.toLowerCase().includes(query.toLowerCase())
  );
}

</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">{$t("settings.title")}</h1>
      <p class="text-muted-foreground mt-2">
        {$t("settings.subtitle")}
      </p>
    </div>
    {#if hasChanges}
      <Badge variant="outline" class="text-orange-500"
        >{$t("badges.unsaved")}</Badge
      >
    {/if}
  </div>

  <!-- Search bar for filtering settings -->
  <div class="mb-4 flex items-center gap-2">
    <Input
      type="text"
      placeholder={$t('settings.searchPlaceholder')}
      bind:value={search}
      class="w-full"
    />
  </div>

  <!-- Storage Settings -->
  {#if sectionMatches("storage", search)}
    <Expandable bind:isOpen={storageSectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <HardDrive class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">{$t("storage.title")}</h2>
      </div>
      <div class="space-y-4">
        <div>
          <Label for="storage-path">{$t("storage.location")}</Label>
          <div class="flex gap-2 mt-2">
            <Input
              id="storage-path"
              bind:value={localSettings.storagePath}
              placeholder={storagePathPlaceholder}
              class={storagePathInputClass}
            />
            <Button
              variant="outline"
              on:click={selectStoragePath}
              aria-label={$t("storage.locationPick")}
            >
              <FolderOpen class="h-4 w-4" />
            </Button>
          </div>
          {#if errors.storagePath}
            <p class="mt-1 text-sm text-red-500">{errors.storagePath}</p>
          {:else if storagePathWarning}
            <p class="mt-1 text-sm text-yellow-600 flex items-center gap-1">
              <AlertTriangle class="h-4 w-4" />
              {storagePathWarning}
            </p>
          {/if}
        </div>

        <div class="grid grid-cols-2 gap-4">
          <div>
            <div class="flex items-center">
              <div class="flex-1">
                <Label for="max-storage">{$t("storage.maxSize")}</Label>
                {#if freeSpaceGB !== null}
                  <span class="ml-2 text-xs text-muted-foreground whitespace-nowrap mt-6">
                    {freeSpaceGB} GB available
                  </span>
                {/if}
                <Input
                  id="max-storage"
                  type="number"
                  bind:value={localSettings.maxStorageSize}
                  min="10"
                  max={freeSpaceGB ?? 10000}
                  class={`mt-2 ${maxStorageError ? 'border-red-500 focus:border-red-500 ring-red-500' : ''}`}
                />
                {#if maxStorageError}
                  <p class="mt-1 text-sm text-red-500">{maxStorageError}</p>
                {/if}
              </div>
            </div>
          </div>

          <div>
            <Label for="cleanup-threshold">{$t("storage.cleanupThreshold")}</Label
            >
            <Input
              id="cleanup-threshold"
              type="number"
              bind:value={localSettings.cleanupThreshold}
              min="50"
              max="100"
              disabled={!localSettings.autoCleanup}
              class="mt-2"
            />
            {#if errors.cleanupThreshold}
              <p class="mt-1 text-sm text-red-500">{errors.cleanupThreshold}</p>
            {/if}
          </div>
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="auto-cleanup"
            bind:checked={localSettings.autoCleanup}
          />
          <Label for="auto-cleanup" class="cursor-pointer">
            {$t("storage.enableCleanup")}
          </Label>
        </div>
      </div>
    </Expandable>
  {/if}

  <!-- Network Settings -->
  {#if sectionMatches("network", search)}
    <Expandable bind:isOpen={networkSectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <Wifi class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">{$t("network.title")}</h2>
      </div>
      <div class="space-y-4">
        <div class="grid grid-cols-2 gap-4">
          <div>
            <Label for="max-connections">{$t("network.maxConnections")}</Label>
            <Input
              id="max-connections"
              type="number"
              bind:value={localSettings.maxConnections}
              min="10"
              max="200"
              class="mt-2 {errors.maxConnections ? 'border-red-500 focus:border-red-500' : ''}"
            />
            {#if errors.maxConnections}
              <p class="mt-1 text-sm text-red-500">{errors.maxConnections}</p>
            {/if}
          </div>

          <div>
            <Label for="port">{$t("network.port")}</Label>
            <Input
              id="port"
              type="number"
              bind:value={localSettings.port}
              min="1024"
              max="65535"
              class="mt-2 {errors.port ? 'border-red-500 focus:border-red-500' : ''}"
            />
            {#if errors.port}
              <p class="mt-1 text-sm text-red-500">{errors.port}</p>
            {/if}
          </div>
        </div>

        <div class="grid grid-cols-2 gap-4">
          <div>
            <Label for="upload-bandwidth">{$t("network.uploadLimit")}</Label>
            <Input
              id="upload-bandwidth"
              type="number"
              bind:value={localSettings.uploadBandwidth}
              min="0"
              class="mt-2 {errors.uploadBandwidth ? 'border-red-500 focus:border-red-500' : ''}"
            />
            {#if errors.uploadBandwidth}
              <p class="mt-1 text-sm text-red-500">{errors.uploadBandwidth}</p>
            {/if}
          </div>

          <div>
            <Label for="download-bandwidth">{$t("network.downloadLimit")}</Label>
            <Input
              id="download-bandwidth"
              type="number"
              bind:value={localSettings.downloadBandwidth}
              min="0"
              class="mt-2 {errors.downloadBandwidth ? 'border-red-500 focus:border-red-500' : ''}"
            />
            {#if errors.downloadBandwidth}
              <p class="mt-1 text-sm text-red-500">{errors.downloadBandwidth}</p>
            {/if}
          </div>
        </div>

        <div class="grid grid-cols-2 gap-4">
          <div>
            <Label for="monthly-upload-cap">Monthly Upload Cap (GB)</Label>
            <Input
              id="monthly-upload-cap"
              type="number"
              bind:value={localSettings.monthlyUploadCapGb}
              min="0"
              step="1"
              class="mt-2"
            />
            <p class="mt-1 text-xs text-muted-foreground">
              Set to 0 to keep uploads uncapped. Caps reset each calendar month.
            </p>
            {#if errors.monthlyUploadCapGb}
              <p class="mt-1 text-sm text-red-500">{errors.monthlyUploadCapGb}</p>
            {/if}
          </div>

          <div>
            <Label for="monthly-download-cap">Monthly Download Cap (GB)</Label>
            <Input
              id="monthly-download-cap"
              type="number"
              bind:value={localSettings.monthlyDownloadCapGb}
              min="0"
              step="1"
              class="mt-2"
            />
            <p class="mt-1 text-xs text-muted-foreground">
              Set to 0 to keep downloads uncapped. Caps reset each calendar month.
            </p>
            {#if errors.monthlyDownloadCapGb}
              <p class="mt-1 text-sm text-red-500">{errors.monthlyDownloadCapGb}</p>
            {/if}
          </div>
        </div>

        <div>
          <Label for="cap-thresholds">Usage Warning Thresholds (%)</Label>
          <Input
            id="cap-thresholds"
            type="text"
            bind:value={capThresholdInput}
            on:focus={handleCapThresholdFocus}
            on:input={handleCapThresholdInput}
            on:blur={handleCapThresholdBlur}
            placeholder="e.g. 75, 90"
            class="mt-2"
          />
          <p class="mt-1 text-xs text-muted-foreground">
            Enter comma-separated percentages (1-100). Leave blank to skip warnings.
          </p>
          {#if errors.capWarningThresholds}
            <p class="mt-1 text-sm text-red-500">{errors.capWarningThresholds}</p>
          {/if}
        </div>

        <!-- User Location -->
        <div>
          <Label for="user-location">{$t("network.userLocation")}</Label>
          <DropDown
            id="user-location"
            options={locationOptions}
            bind:value={localSettings.userLocation}
          />
          <p class="text-xs text-muted-foreground mt-1">
            {$t("network.locationHint")}
          </p>
        </div>

        <div class="space-y-2">
          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="enable-upnp"
              bind:checked={localSettings.enableUPnP}
            />
            <Label for="enable-upnp" class="cursor-pointer">
              {$t("network.enableUpnp")}
            </Label>
          </div>

          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="enable-nat"
              bind:checked={localSettings.enableNAT}
            />
            <Label for="enable-nat" class="cursor-pointer">
              {$t("network.enableNat")}
            </Label>
          </div>

          <div class="space-y-2">
            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="auto-start-dht"
                bind:checked={localSettings.autoStartDHT}
              />
              <Label for="auto-start-dht" class="cursor-pointer">
                {$t("network.autoStartNode")}
              </Label>
            </div>
            <p class="text-xs text-muted-foreground ml-6">
              {$t("network.autoStartNodeDescription")}
            </p>
          </div>

          <div class="space-y-2">
            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="auto-start-geth"
                bind:checked={localSettings.autoStartGeth}
              />
              <Label for="auto-start-geth" class="cursor-pointer">
                {$t("network.autoStartGeth")}
              </Label>
            </div>
            <p class="text-xs text-muted-foreground ml-6">
              {$t("network.autoStartGethDescription")}
            </p>
          </div>

        </div>

        <!-- Custom Bootstrap Nodes -->
        <div class="space-y-3 border-t pt-3">
          <h4 class="font-medium">Custom Bootstrap Nodes</h4>
          <p class="text-xs text-muted-foreground">
            Configure custom bootstrap nodes for the DHT network. Leave empty to use the default hardcoded bootstrap nodes.
          </p>

          <div>
            <Label for="new-bootstrap-node">Add Bootstrap Node</Label>
            <div class="flex gap-2 mt-1">
              <Input
                id="new-bootstrap-node"
                bind:value={newBootstrapNode}
                on:keydown={(e) => {
                  const ev = (e as unknown as KeyboardEvent);
                  if (ev.key === 'Enter') {
                    ev.preventDefault();
                    addBootstrapNode();
                  }
                }}
                placeholder="/ip4/54.198.145.146/tcp/4001/p2p/12D3KooW..."
                class="flex-1 font-mono text-sm"
              />
              <Button
                size="sm"
                variant="outline"
                on:click={addBootstrapNode}
                disabled={!newBootstrapNode.trim()}
              >
                Add
              </Button>
            </div>
            <p class="text-xs text-muted-foreground mt-1">
              Enter bootstrap node multiaddress and press Enter or click Add
            </p>
          </div>

          {#if localSettings.customBootstrapNodes && localSettings.customBootstrapNodes.length > 0}
            <div class="space-y-2">
              <Label>Configured Bootstrap Nodes ({localSettings.customBootstrapNodes.length})</Label>
              <div class="space-y-2">
                {#each localSettings.customBootstrapNodes as node, index}
                  <div class="flex items-center gap-2 p-2 bg-slate-50 rounded border">
                    <span class="flex-1 text-xs font-mono break-all">{node}</span>
                    <Button
                      size="xs"
                      variant="destructive"
                      on:click={() => removeBootstrapNode(index)}
                    >
                      Remove
                    </Button>
                  </div>
                {/each}
              </div>
            </div>
          {/if}

          <div class="rounded-md border border-dashed border-slate-200 p-3 text-xs text-muted-foreground space-y-1">
            <p>Custom bootstrap nodes: {localSettings.customBootstrapNodes?.length || 0}</p>
            {#if localSettings.customBootstrapNodes && localSettings.customBootstrapNodes.length > 0}
              <p class="text-green-600">✓ Using custom bootstrap nodes</p>
            {:else}
              <p class="text-blue-600">Using default hardcoded bootstrap nodes</p>
            {/if}
          </div>
        </div>

      </div>
    </Expandable>
  {/if}

  <!-- Bandwidth Scheduling -->
  {#if sectionMatches("bandwidthScheduling", search)}
    <Expandable bind:isOpen={bandwidthSectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <RefreshCw class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">{$t('bandwidthScheduling.title')}</h2>
      </div>
      <div class="space-y-4">
        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="enable-bandwidth-scheduling"
            bind:checked={localSettings.enableBandwidthScheduling}
          />
          <Label for="enable-bandwidth-scheduling" class="cursor-pointer">
            {$t('bandwidthScheduling.enable')}
          </Label>
        </div>
        <p class="text-xs text-muted-foreground">
          {$t('bandwidthScheduling.description')}
        </p>

        {#if localSettings.enableBandwidthScheduling}
          <div class="mt-3 rounded-md border border-blue-200 bg-blue-50 p-3 text-xs text-blue-900">
            <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <div class="text-sm font-semibold text-blue-800">Current limits</div>
              <div class="text-[0.75rem] uppercase tracking-wide">
                {#if $activeBandwidthLimits.source === "schedule"}
                  Active schedule: {$activeBandwidthLimits.scheduleName ?? "Unnamed schedule"}
                {:else}
                  Default limits in effect
                {/if}
              </div>
            </div>
            <div class="mt-2 grid gap-2 sm:grid-cols-2">
              <div>Upload: {formatBandwidthLimit($activeBandwidthLimits.uploadLimitKbps)}</div>
              <div>Download: {formatBandwidthLimit($activeBandwidthLimits.downloadLimitKbps)}</div>
            </div>
            <div class="mt-2">
              Next change: {formatNextChange($activeBandwidthLimits.nextChangeAt)}
            </div>
          </div>

          <div class="space-y-3 mt-4">
            {#each localSettings.bandwidthSchedules as schedule, index}
              <div class="p-4 border rounded-lg bg-muted/30">
                <div class="flex items-start justify-between gap-4 mb-3">
                  <div class="flex items-center gap-2 flex-1">
                    <input
                      type="checkbox"
                      id="schedule-enabled-{index}"
                      bind:checked={schedule.enabled}
                      on:change={() => {
                        localSettings.bandwidthSchedules = [...localSettings.bandwidthSchedules];
                      }}
                    />
                    <Input
                      type="text"
                      bind:value={schedule.name}
                      placeholder={$t('bandwidthScheduling.scheduleName')}
                      class="flex-1"
                      on:input={() => {
                        localSettings.bandwidthSchedules = [...localSettings.bandwidthSchedules];
                      }}
                    />
                  </div>
                  <Button
                    size="sm"
                    variant="destructive"
                    on:click={() => {
                      localSettings.bandwidthSchedules = localSettings.bandwidthSchedules.filter((_, i) => i !== index);
                    }}
                  >
                    {$t('bandwidthScheduling.remove')}
                  </Button>
                </div>

                <div class="grid grid-cols-2 gap-3 mb-3">
                  <div>
                    <Label for="schedule-start-{index}" class="text-xs">{$t('bandwidthScheduling.startTime')}</Label>
                    <Input
                      id="schedule-start-{index}"
                      type="time"
                      bind:value={schedule.startTime}
                      class="mt-1"
                      on:input={() => {
                        localSettings.bandwidthSchedules = [...localSettings.bandwidthSchedules];
                      }}
                    />
                  </div>
                  <div>
                    <Label for="schedule-end-{index}" class="text-xs">{$t('bandwidthScheduling.endTime')}</Label>
                    <Input
                      id="schedule-end-{index}"
                      type="time"
                      bind:value={schedule.endTime}
                      class="mt-1"
                      on:input={() => {
                        localSettings.bandwidthSchedules = [...localSettings.bandwidthSchedules];
                      }}
                    />
                  </div>
                </div>

                <div class="mb-3">
                  <Label class="text-xs">{$t('bandwidthScheduling.daysOfWeek')}</Label>
                  <div class="flex gap-2 mt-1">
                    {#each [{value: 0, label: $t('bandwidthScheduling.days.sun')}, {value: 1, label: $t('bandwidthScheduling.days.mon')}, {value: 2, label: $t('bandwidthScheduling.days.tue')}, {value: 3, label: $t('bandwidthScheduling.days.wed')}, {value: 4, label: $t('bandwidthScheduling.days.thu')}, {value: 5, label: $t('bandwidthScheduling.days.fri')}, {value: 6, label: $t('bandwidthScheduling.days.sat')}] as day}
                      <button
                        type="button"
                        class="px-2 py-1 text-xs rounded border {schedule.daysOfWeek.includes(day.value) ? 'bg-primary text-primary-foreground' : 'bg-background'}"
                        on:click={() => {
                          // Update the schedule's days
                          if (schedule.daysOfWeek.includes(day.value)) {
                            schedule.daysOfWeek = schedule.daysOfWeek.filter(d => d !== day.value);
                          } else {
                            schedule.daysOfWeek = [...schedule.daysOfWeek, day.value].sort();
                          }
                          // Trigger reactivity by reassigning the entire array
                          localSettings.bandwidthSchedules = [...localSettings.bandwidthSchedules];
                        }}
                      >
                        {day.label}
                      </button>
                    {/each}
                  </div>
                </div>

                <div class="grid grid-cols-2 gap-3">
                  <div>
                    <Label for="schedule-upload-{index}" class="text-xs">{$t('bandwidthScheduling.uploadLimit')}</Label>
                    <Input
                      id="schedule-upload-{index}"
                      type="number"
                      bind:value={schedule.uploadLimit}
                      min="0"
                      class="mt-1"
                      on:input={() => {
                        localSettings.bandwidthSchedules = [...localSettings.bandwidthSchedules];
                      }}
                    />
                  </div>
                  <div>
                    <Label for="schedule-download-{index}" class="text-xs">{$t('bandwidthScheduling.downloadLimit')}</Label>
                    <Input
                      id="schedule-download-{index}"
                      type="number"
                      bind:value={schedule.downloadLimit}
                      min="0"
                      class="mt-1"
                      on:input={() => {
                        localSettings.bandwidthSchedules = [...localSettings.bandwidthSchedules];
                      }}
                    />
                  </div>
                </div>
              </div>
            {/each}

            <Button
              size="sm"
              variant="outline"
              on:click={() => {
                localSettings.bandwidthSchedules = [
                  ...localSettings.bandwidthSchedules,
                  {
                    id: `schedule-${Date.now()}`,
                    name: $t('bandwidthScheduling.scheduleDefault', { values: { number: localSettings.bandwidthSchedules.length + 1 } }),
                    startTime: '00:00',
                    endTime: '23:59',
                    daysOfWeek: [0, 1, 2, 3, 4, 5, 6],
                    uploadLimit: 0,
                    downloadLimit: 0,
                    enabled: true,
                  },
                ];
              }}
            >
              {$t('bandwidthScheduling.addSchedule')}
            </Button>
          </div>
        {/if}
      </div>
    </Expandable>
  {/if}

  <!-- Language Settings -->
  {#if sectionMatches("language", search)}
    <Expandable bind:isOpen={languageSectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <Languages class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">{$t("language.title")}</h2>
      </div>
      <div class="space-y-4">
        <div>
          <Label for="language-select">{$t("language.select")}</Label>
          <DropDown
            id="language-select"
            options={languages}
            bind:value={selectedLanguage}
            on:change={(e) => onLanguageChange(e.detail.value)}
          />
        </div>
      </div>
    </Expandable>
  {/if}


  <!-- Privacy Settings -->
  {#if sectionMatches("privacy", search)}
    <Expandable bind:isOpen={privacySectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <Shield class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">{$t("privacy.title")}</h2>
      </div>
      <div class="space-y-4">
        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="enable-proxy"
            bind:checked={localSettings.enableProxy}
          />
          <Label for="enable-proxy" class="cursor-pointer">
            {$t("privacy.enableProxy")}
          </Label>
        </div>

        {#if localSettings.enableProxy}
          <div>
            <Label for="proxy-address">{$t("privacy.proxyAddress")}</Label>
            <Input
              id="proxy-address"
              bind:value={localSettings.proxyAddress}
              placeholder="127.0.0.1:9050 (SOCKS5)"
              class="mt-1 {errors.proxyAddress ? 'border-red-500 focus:border-red-500' : ''}"
            />
            <p class="text-xs text-muted-foreground mt-1">{$t("privacy.proxyHint")}</p>
            {#if errors.proxyAddress}
              <p class="mt-1 text-sm text-red-500">{errors.proxyAddress}</p>
            {/if}
          </div>
        {/if}

        <div class="space-y-3 border-t pt-3">
          <h4 class="font-medium flex items-center gap-2">
            <Shield class="h-4 w-4 text-blue-600" />
            Hide My IP
          </h4>

          <div>
            <Label for="privacy-mode-select">Routing preference</Label>
            <DropDown
              id="privacy-mode-select"
              options={privacyModeOptions}
              bind:value={localSettings.ipPrivacyMode}
            />
          </div>

          <div class="rounded-md border border-slate-200 bg-slate-50 p-3 text-sm text-slate-700">
            {privacyStatus}
          </div>

          {#if localSettings.ipPrivacyMode !== "off"}
            <div>
              <Label for="trusted-proxies">Trusted proxy addresses</Label>
              <textarea
                id="trusted-proxies"
                bind:value={trustedProxyText}
                on:blur={updateTrustedProxyRelays}
                placeholder="/dns4/proxy.example.com/tcp/4001/p2p/12D3KooW...\nOne multiaddress per line."
                rows="4"
                class="w-full px-3 py-2 border rounded-md text-sm"
              ></textarea>
              <p class="text-xs text-muted-foreground mt-1">
                These peers will be used when auto-routing traffic to hide your IP. Make sure they are operated by someone you trust.
              </p>
            </div>

            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="disable-nat-privacy"
                bind:checked={localSettings.disableDirectNatTraversal}
              />
              <Label for="disable-nat-privacy" class="cursor-pointer">
                Disable STUN while hiding IP
              </Label>
            </div>

            <div class="rounded-md border border-dashed border-slate-200 p-3 text-xs text-muted-foreground space-y-1">
              <p>Trusted proxies configured: {localSettings.trustedProxyRelays.length}</p>
              <p>SOCKS5 proxy: {localSettings.enableProxy ? (localSettings.proxyAddress || "Not set") : "Disabled"}</p>
              <p>Direct NAT traversal: {localSettings.disableDirectNatTraversal ? "Disabled (safer)" : "Enabled (may reveal IP)"}</p>
            </div>
          {/if}
        </div>

        <!-- AutoNAT Configuration -->
        <div class="space-y-3 border-t pt-3">
          <h4 class="font-medium">NAT Traversal & Reachability</h4>

          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="enable-autonat"
              bind:checked={localSettings.enableAutonat}
            />
            <Label for="enable-autonat" class="cursor-pointer">
              Enable AutoNAT Reachability Detection
            </Label>
          </div>

          {#if localSettings.enableAutonat}
            <div>
              <Label for="autonat-interval">Probe Interval (seconds)</Label>
              <Input
                id="autonat-interval"
                type="number"
                bind:value={localSettings.autonatProbeInterval}
                min="10"
                max="300"
                placeholder="30"
                class="mt-1"
              />
              <p class="text-xs text-muted-foreground mt-1">
                How often to check network reachability (default: 30s)
              </p>
            </div>

            <div>
              <Label for="autonat-servers">Custom AutoNAT Servers (optional)</Label>
              <textarea
                id="autonat-servers"
                bind:value={autonatServersText}
                on:input={updateAutonatServers}
                on:blur={updateAutonatServers}
                placeholder="/ip4/1.2.3.4/tcp/4001/p2p/QmPeerId&#10;One multiaddr per line"
                rows="3"
                class="w-full px-3 py-2 border rounded-md text-sm"
              ></textarea>
              <p class="text-xs text-muted-foreground mt-1">
                Leave empty to use bootstrap nodes
              </p>
            </div>
          {/if}
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="anonymous-mode"
            bind:checked={localSettings.anonymousMode}
          />
          <Label for="anonymous-mode" class="cursor-pointer">
            {$t("privacy.anonymousMode")}
          </Label>
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="share-analytics"
            bind:checked={localSettings.shareAnalytics}
          />
          <Label for="share-analytics" class="cursor-pointer">
            {$t("privacy.shareAnalytics")}
          </Label>
        </div>
      </div>
    </Expandable>
  {/if}

  <!-- Notifications -->
  {#if sectionMatches("notifications", search)}
    <Expandable bind:isOpen={notificationsSectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <Bell class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">{$t("notifications.title")}</h2>
      </div>
      <div class="space-y-2">
        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="enable-notifications"
            bind:checked={localSettings.enableNotifications}
          />
          <Label for="enable-notifications" class="cursor-pointer">
            {$t("notifications.enable")}
          </Label>
        </div>

        {#if localSettings.enableNotifications}
          <div class="ml-6 space-y-2">
            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="notify-complete"
                bind:checked={localSettings.notifyOnComplete}
              />
              <Label for="notify-complete" class="cursor-pointer">
                {$t("notifications.notifyComplete")}
              </Label>
            </div>

            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="notify-error"
                bind:checked={localSettings.notifyOnError}
              />
              <Label for="notify-error" class="cursor-pointer">
                {$t("notifications.notifyError")}
              </Label>
            </div>

            <div>
              <div class="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="notify-cap"
                  bind:checked={localSettings.notifyOnBandwidthCap}
                />
                <Label for="notify-cap" class="cursor-pointer">
                  Warn when monthly caps reach thresholds (toast)
                </Label>
              </div>
              <p class="ml-7 text-xs text-muted-foreground">
                Triggers an in-app toast as soon as usage crosses your percentages.
              </p>
            </div>

            <div>
              <div class="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="notify-cap-desktop"
                  bind:checked={localSettings.notifyOnBandwidthCapDesktop}
                />
                <Label for="notify-cap-desktop" class="cursor-pointer">
                  Send desktop notification for cap warnings
                </Label>
              </div>
              <p class="ml-7 text-xs text-muted-foreground">
                Useful when the app is minimized; respects your OS notification settings.
              </p>
            </div>

            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="sound-alerts"
                bind:checked={localSettings.soundAlerts}
              />
              <Label for="sound-alerts" class="cursor-pointer">
                {$t("notifications.soundAlerts")}
              </Label>
            </div>
          </div>
        {/if}
      </div>
    </Expandable>
  {/if}

  <!-- Logs Settings -->
  {#if sectionMatches("logs", search)}
    <Expandable>
      <div slot="title" class="flex items-center gap-3">
        <FileText class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">Logs</h2>
      </div>
      <div class="space-y-4">
        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="enable-file-logging"
            bind:checked={localSettings.enableFileLogging}
          />
          <Label for="enable-file-logging" class="cursor-pointer">
            Enable File Logging
          </Label>
        </div>

        {#if localSettings.enableFileLogging}
          <div class="ml-6 space-y-4">
            <div class="p-3 bg-blue-50 rounded-md border border-blue-200">
              <p class="text-xs text-blue-900">
                Logs will be stored in:<br/>
                <code class="bg-blue-100 px-1 rounded break-all">{logsDirectory || 'Loading...'}</code>
                <br/><br/>
                Old logs are automatically deleted when storage limits are reached.
              </p>
            </div>

            <div>
              <Label for="max-log-size">Maximum Log File Size (MB)</Label>
              <Input
                id="max-log-size"
                type="number"
                bind:value={localSettings.maxLogSizeMB}
                min="1"
                max="100"
                class="mt-2"
              />
              <p class="text-xs text-muted-foreground mt-1">
                When a log file reaches this size, a new log file will be created.
                The system will keep up to 10x this size in total logs.
              </p>
            </div>
          </div>
        {:else}
          <div class="ml-6 p-3 bg-gray-50 rounded-md border border-gray-200">
            <p class="text-xs text-gray-700">
              Console logging is always enabled. Enable file logging to also save logs to disk for debugging.
            </p>
          </div>
        {/if}
      </div>
    </Expandable>
  {/if}

  <!-- Advanced Settings -->
  {#if sectionMatches("advanced", search)}
    <Expandable bind:isOpen={advancedSectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <Database class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">{$t("advanced.title")}</h2>
      </div>
      <div class="space-y-4">
        <div class="grid grid-cols-2 gap-4">
          <div>
            <Label for="chunk-size">{$t("advanced.chunkSize")}</Label>
            <Input
              id="chunk-size"
              type="number"
              bind:value={localSettings.chunkSize}
              min="64"
              max="1024"
              class="mt-2"
            />
            {#if errors.chunkSize}
              <p class="mt-1 text-sm text-red-500">{errors.chunkSize}</p>
            {/if}
          </div>

          <div>
            <Label for="cache-size">{$t("advanced.cacheSize")}</Label>
            <Input
              id="cache-size"
              type="number"
              bind:value={localSettings.cacheSize}
              min="256"
              max="8192"
              class="mt-2"
            />
            {#if errors.cacheSize}
              <p class="mt-1 text-sm text-red-500">{errors.cacheSize}</p>
            {/if}
          </div>
        </div>

        <div class="relative">
          <Label for="log-level">{$t("advanced.logLevel")}</Label>
          <DropDown
            id="log-level"
            options={[
              { value: "error", label: $t("advanced.logError") },
              { value: "warn", label: $t("advanced.logWarn") },
              { value: "info", label: $t("advanced.logInfo") },
              { value: "debug", label: $t("advanced.logDebug") },
            ]}
            bind:value={localSettings.logLevel}
          />
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="auto-update"
            bind:checked={localSettings.autoUpdate}
          />
          <Label for="auto-update" class="cursor-pointer">
            {$t("advanced.autoUpdate")}
          </Label>
        </div>


        <div class="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="xs"
            on:click={clearCache}
            disabled={clearingCache || cacheCleared}
          >
            <RefreshCw
              class="h-4 w-4 mr-2 {clearingCache ? 'animate-spin' : ''}"
            />
            {clearingCache
              ? $t("button.clearing")
              : cacheCleared
                ? $t("button.cleared")
                : $t("button.clearCache")}
          </Button>
          <Button variant="outline" size="xs" on:click={exportSettingsOld}>
            {$t("advanced.exportSettings")}
          </Button>

          <label for="import-settings">
            <Button
              variant="outline"
              size="xs"
              on:click={() => fileInputEl?.click()}
            >
              {$t("advanced.importSettings")}
            </Button>
            <input
              bind:this={fileInputEl}
              id="import-settings"
              type="file"
              accept=".json"
              on:change={importSettingsOld}
              class="hidden"
            />
          </label>
        </div>

        {#if importExportFeedback}
          <div
            class="mt-4 p-3 rounded-md text-sm {importExportFeedback.type ===
            'success'
              ? 'bg-green-100 text-green-800'
              : 'bg-red-100 text-red-800'}"
          >
            {importExportFeedback.message}
          </div>
        {/if}
      </div>
    </Expandable>
  {/if}



  <!-- Diagnostics -->
  {#if sectionMatches("diagnostics", search)}
    <Expandable bind:isOpen={diagnosticsSectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <Activity class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">{$t("settings.diagnostics.title")}</h2>
      </div>
      <div class="space-y-4">
        <p class="text-sm text-muted-foreground">{$t("settings.diagnostics.description")}</p>

        <div class="flex gap-2 items-center">
          <Button size="xs" on:click={runDiagnostics} disabled={diagnosticsRunning}>
            <RefreshCw class="h-4 w-4 mr-2 {diagnosticsRunning ? 'animate-spin' : ''}" />
            {diagnosticsRunning ? $t("settings.diagnostics.running") : $t("settings.diagnostics.run")}
          </Button>
          {#if diagnosticsReport}
            <Button variant="outline" size="xs" on:click={copyDiagnostics}>
              <Copy class="h-4 w-4 mr-2" />{$t("settings.diagnostics.copyReport")}
            </Button>
          {/if}
        </div>

        {#if diagnosticsReport}
          <div>
            <!-- Summary -->
            <div class="mb-4 p-3 bg-gray-50 rounded-lg">
              <h3 class="font-medium mb-2">{$t("settings.diagnostics.resultsTitle")}</h3>
              <div class="flex gap-4 text-sm">
                <span class="text-green-600">✓ {diagnosticsReport.summary.passed} passed</span>
                {#if diagnosticsReport.summary.warnings > 0}
                  <span class="text-amber-600">⚠ {diagnosticsReport.summary.warnings} warnings</span>
                {/if}
                {#if diagnosticsReport.summary.failed > 0}
                  <span class="text-red-600">✗ {diagnosticsReport.summary.failed} failed</span>
                {/if}
              </div>
            </div>

            <!-- Results by Category -->
            {#each ['environment', 'network', 'storage', 'security', 'system'] as category}
              {@const categoryResults = diagnosticsReport.results.filter(r => r.category === category)}
              {#if categoryResults.length > 0}
                <div class="mb-4">
                  <h4 class="text-sm font-semibold text-gray-700 uppercase mb-2">{category}</h4>
                  <ul class="space-y-2">
                    {#each categoryResults as d}
                      <li class="flex items-start gap-2">
                        {#if d.status === 'pass'}
                          <CheckCircle class="h-4 w-4 text-green-600 mt-0.5 flex-shrink-0" />
                        {:else if d.status === 'warn'}
                          <AlertTriangle class="h-4 w-4 text-amber-600 mt-0.5 flex-shrink-0" />
                        {:else if d.status === 'fail'}
                          <AlertTriangle class="h-4 w-4 text-red-600 mt-0.5 flex-shrink-0" />
                        {:else}
                          <Activity class="h-4 w-4 text-blue-600 mt-0.5 flex-shrink-0" />
                        {/if}
                        <div class="flex-1 min-w-0">
                          <div class="text-sm font-medium">{d.label}</div>
                          {#if d.details}
                            <div class="text-xs text-muted-foreground break-words">{d.details}</div>
                          {/if}
                          {#if d.error}
                            <div class="text-xs text-red-600 break-words">Error: {d.error}</div>
                          {/if}
                        </div>
                      </li>
                    {/each}
                  </ul>
                </div>
              {/if}
            {/each}
          </div>
        {/if}
      </div>
    </Expandable>
  {/if}

  <!-- Backup & Restore -->
  {#if sectionMatches("backup", search) || sectionMatches("restore", search) || sectionMatches("export", search) || sectionMatches("import", search)}
    <Expandable bind:isOpen={backupRestoreSectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <Database class="h-6 w-6 text-blue-600" />
        <h2 class="text-xl font-semibold text-black">{$t("settingsBackup.title")}</h2>
      </div>
      <div class="space-y-4">
        <p class="text-sm text-muted-foreground">{$t("settingsBackup.description")}</p>

        <!-- Export/Import Buttons -->
        <div class="flex flex-wrap gap-3">
          <Button
            size="sm"
            on:click={exportSettings}
            disabled={isExporting}
            class="min-w-[140px]"
          >
            {#if isExporting}
              <RefreshCw class="h-4 w-4 mr-2 animate-spin" />
              {$t("settingsBackup.export")}...
            {:else}
              <UploadIcon class="h-4 w-4 mr-2" />
              {$t("settingsBackup.exportButton")}
            {/if}
          </Button>

          <Button
            size="sm"
            variant="outline"
            on:click={importSettings}
            disabled={isImporting}
            class="min-w-[140px]"
          >
            {#if isImporting}
              <RefreshCw class="h-4 w-4 mr-2 animate-spin" />
              {$t("settingsBackup.import")}...
            {:else}
              <DownloadIcon class="h-4 w-4 mr-2" />
              {$t("settingsBackup.importButton")}
            {/if}
          </Button>
        </div>

        <!-- Feedback Message -->
        {#if backupMessage}
          <div
            class="p-3 rounded-lg text-sm transition-all"
            class:bg-green-100={backupMessage.type === 'success'}
            class:text-green-800={backupMessage.type === 'success'}
            class:bg-red-100={backupMessage.type === 'error'}
            class:text-red-800={backupMessage.type === 'error'}
            class:bg-yellow-100={backupMessage.type === 'warning'}
            class:text-yellow-800={backupMessage.type === 'warning'}
          >
            <div class="flex items-start gap-2">
              {#if backupMessage.type === 'success'}
                <CheckCircle class="h-4 w-4 mt-0.5 flex-shrink-0" />
              {:else}
                <AlertTriangle class="h-4 w-4 mt-0.5 flex-shrink-0" />
              {/if}
              <p>{backupMessage.text}</p>
            </div>
          </div>
        {/if}

        <!-- Info Box -->
        <div class="p-4 bg-muted/50 rounded-lg border border-dashed">
          <h3 class="font-medium text-sm mb-2">{$t("settingsBackup.autoBackup")}</h3>
          <p class="text-xs text-muted-foreground mb-3">{$t("settingsBackup.autoBackupDescription")}</p>

          <!-- Auto-backups list -->
          {#if settingsBackupService.getAutoBackups().length > 0}
            <div class="space-y-2">
              {#each settingsBackupService.getAutoBackups().slice(0, 3) as backup}
                <div class="flex items-center justify-between p-2 bg-background rounded border text-xs">
                  <span class="text-muted-foreground">
                    {backup.date.toLocaleString()}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    on:click={async () => {
                      const result = await settingsBackupService.restoreAutoBackup(backup.key);
                      if (result.success) {
                        showToast($t("settingsBackup.messages.autoBackupRestored"), 'success');
                        // Reload settings
                        const stored = localStorage.getItem('chiralSettings');
                        if (stored) {
                          localSettings = JSON.parse(stored);
                          savedSettings = JSON.parse(JSON.stringify(localSettings));
                        }
                      } else {
                        showToast($t("settingsBackup.messages.importError", { values: { error: result.error } }), 'error');
                      }
                    }}
                    class="h-6 text-xs"
                  >
                    {$t("settingsBackup.restoreAutoBackup")}
                  </Button>
                </div>
              {/each}
            </div>
          {:else}
            <p class="text-xs text-muted-foreground italic">No automatic backups available</p>
          {/if}
        </div>
      </div>
    </Expandable>
  {/if}

  <!-- Developers -->
  {#if sectionMatches("developers", search) || sectionMatches("developer", search)}
    <Expandable bind:isOpen={developersSectionOpen}>
      <div slot="title" class="flex items-center gap-3">
        <Code class="h-6 w-6 text-purple-600" />
        <h2 class="text-xl font-semibold text-black">Developers</h2>
      </div>
      <div class="space-y-4">
        <p class="text-sm text-muted-foreground">
          Advanced developer options for testing and debugging. Most users don't need to change these settings.
        </p>

        <!-- DHT Client Mode -->
        <div class="space-y-3 border-t pt-3">
          <h4 class="font-medium">{$t('pureClientMode.settings.dht.clientMode.title')}</h4>

          <div class="space-y-2">
            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="pure-client-mode"
                bind:checked={localSettings.pureClientMode}
              />
              <Label for="pure-client-mode" class="cursor-pointer">
                {$t('pureClientMode.settings.dht.clientMode.label')}
              </Label>
            </div>
            <p class="text-xs text-muted-foreground ml-6">
              <strong>{$t('pureClientMode.settings.dht.clientMode.descriptionOverride')}</strong> {$t('pureClientMode.settings.dht.clientMode.descriptionText')}
              <br/>
              <strong>{$t('pureClientMode.settings.dht.clientMode.noteLabel')}</strong> {$t('pureClientMode.settings.dht.clientMode.noteText')}
            </p>
            {#if localSettings.pureClientMode}
              <div class="ml-6 p-2 bg-yellow-50 border border-yellow-200 rounded text-xs text-yellow-800 dark:bg-yellow-950/30 dark:border-yellow-800 dark:text-yellow-200">
                {$t('pureClientMode.warnings.settingsForced')}
              </div>
            {/if}
            <div class="ml-6 p-2 bg-blue-50 border border-blue-200 rounded text-xs text-blue-800">
              ℹ️ <strong>{$t('pureClientMode.settings.dht.clientMode.autoBehaviorLabel')}</strong> {$t('pureClientMode.settings.dht.clientMode.autoBehaviorText')}
            </div>
          </div>
        </div>

        <!-- DHT Server Mode -->
        <div class="space-y-3 border-t pt-3">
          <h4 class="font-medium">{$t('pureClientMode.settings.dht.serverMode.title')}</h4>

          <div class="space-y-2">
            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="force-server-mode"
                bind:checked={localSettings.forceServerMode}
                disabled={localSettings.pureClientMode}
              />
              <Label for="force-server-mode" class="cursor-pointer">
                {$t('pureClientMode.settings.dht.serverMode.label')}
              </Label>
            </div>
            <p class="text-xs text-muted-foreground ml-6">
              <strong>{$t('pureClientMode.settings.dht.serverMode.descriptionOverride')}</strong> {$t('pureClientMode.settings.dht.serverMode.descriptionText')}
              <br/>
              <strong>{$t('pureClientMode.settings.dht.serverMode.noteLabel')}</strong> {$t('pureClientMode.settings.dht.serverMode.noteText')}
            </p>
            {#if localSettings.forceServerMode}
              <div class="ml-6 p-2 bg-yellow-50 border border-yellow-200 rounded text-xs text-yellow-800">
                ⚠️ <strong>{$t('pureClientMode.settings.dht.serverMode.warningLabel')}</strong> {$t('pureClientMode.settings.dht.serverMode.warningText')}
              </div>
            {/if}
            {#if localSettings.pureClientMode}
              <div class="ml-6 p-2 bg-gray-50 border border-gray-200 rounded text-xs text-gray-600">
                {$t('pureClientMode.warnings.serverModeDisabled')}
              </div>
            {/if}
            <div class="ml-6 p-2 bg-blue-50 border border-blue-200 rounded text-xs text-blue-800">
              ℹ️ <strong>{$t('pureClientMode.settings.dht.serverMode.autoBehaviorLabel')}</strong> {$t('pureClientMode.settings.dht.serverMode.autoBehaviorText')}
            </div>
          </div>
        </div>

        <!-- Protocol Test Panel -->
        <div class="space-y-3 border-t pt-3">
          <h4 class="font-medium">Protocol Testing</h4>
          <p class="text-xs text-muted-foreground">
            Test and verify protocol functionality for development and debugging.
          </p>
          <ProtocolTestPanel />
        </div>

        <!-- Restartable HTTP Download -->
        <div class="space-y-3 border-t pt-3">
          <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h4 class="font-medium">Restartable HTTP Download (Beta)</h4>
              <p class="text-xs text-muted-foreground mt-1">
                Download any HTTP resource with pause/resume support. Not P2P - uses centralized HTTP servers.
              </p>
            </div>
            <Button size="xs" variant="outline" on:click={() => (showRestartSection = !showRestartSection)}>
              {showRestartSection ? 'Hide' : 'Show'}
            </Button>
          </div>

          {#if showRestartSection}
            <div class="mt-4 space-y-4">
              <div class="grid gap-3 md:grid-cols-2">
                <div class="space-y-2">
                  <Label for="restart-url">HTTP Source URL</Label>
                  <Input
                    id="restart-url"
                    type="url"
                    placeholder="https://example.com/file.bin"
                    bind:value={restartSourceUrl}
                  />
                </div>
                <div class="space-y-2">
                  <Label for="restart-hash">Expected SHA-256 (optional)</Label>
                  <Input
                    id="restart-hash"
                    placeholder="64-character hex"
                    bind:value={restartSha256}
                  />
                </div>
                <div class="space-y-2">
                  <Label for="restart-id">Download ID (optional)</Label>
                  <Input
                    id="restart-id"
                    placeholder="Leave blank to auto-generate"
                    bind:value={restartDownloadId}
                  />
                </div>
              </div>
              <div class="space-y-2">
                <Label for="restart-dest">Destination Path</Label>
                <div class="flex flex-col gap-2 md:flex-row">
                  <Input
                    id="restart-dest"
                    placeholder="/home/user/Downloads/file.bin"
                    bind:value={restartDestinationPath}
                    class="flex-1"
                  />
                  <Button type="button" variant="outline" size="xs" on:click={chooseRestartDestination}>
                    Choose Path
                  </Button>
                </div>
              </div>

              <div class="rounded-md border border-dashed p-3 text-xs text-muted-foreground">
                <p>
                  Enter a direct HTTP URL and destination path, then use the controls below to start,
                  pause, or resume the transfer. Metadata is stored next to the destination as
                  <code>.filename.chiral.meta.json</code> so progress survives restarts.
                </p>
              </div>

              <DownloadRestartControls
                bind:downloadId={restartDownloadId}
                sourceUrl={restartSourceUrl}
                destinationPath={restartDestinationPath}
                expectedSha256={restartSha256 ? restartSha256 : null}
              />
            </div>
          {/if}
        </div>
      </div>
    </Expandable>
  {/if}

  <!-- Action Buttons -->
  <div class="flex flex-wrap items-center justify-between gap-2">
    <Button variant="destructive" size="xs" on:click={openResetConfirm}>
      {$t("actions.resetDefaults")}
    </Button>

    <div class="flex gap-2">
      <Button
        variant="outline"
        size="xs"
        disabled={!hasChanges}
        on:click={() => (localSettings = JSON.parse(JSON.stringify(savedSettings)))}
        class={`transition-colors duration-200 ${!hasChanges ? "cursor-not-allowed opacity-50" : ""}`}
      >
        {$t("actions.cancel")}
      </Button>

      <Button
        size="xs"
        on:click={saveSettings}
        disabled={!hasChanges || hasValidationErrors}

        class={`transition-colors duration-200 ${!hasChanges || hasValidationErrors ? "cursor-not-allowed opacity-50" : ""}`}
      >
        <Save class="h-4 w-4 mr-2" />
        {$t("actions.save")}
      </Button>
    </div>
  </div>
</div>

{#if showResetConfirmModal}
  <div
    class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4"
    role="button"
    tabindex="0"
    on:click={() => (showResetConfirmModal = false)}
    on:keydown={(e) => {
      if (e.key === "Enter" || e.key === " ") showResetConfirmModal = false;
    }}
  >
    <div
      class="bg-white p-6 rounded-lg shadow-xl w-full max-w-sm"
      role="dialog"
      tabindex="0"
      aria-modal="true"
      on:click|stopPropagation
      on:keydown={(e) => {
        if (e.key === "Escape") showResetConfirmModal = false;
      }}
    >
      <h3 class="text-lg font-bold mb-2">{$t("confirm.title")}</h3>
      <p class="text-sm text-gray-600 mb-6">
        {$t("confirm.resetBody")}
      </p>
      <div class="flex justify-end gap-3">
        <Button
          variant="outline"
          on:click={() => (showResetConfirmModal = false)}
        >
          {$t("actions.cancel")}
        </Button>
        <Button variant="destructive" on:click={handleConfirmReset}>
          {$t("actions.confirm")}
        </Button>
      </div>
    </div>
  </div>
{/if}
