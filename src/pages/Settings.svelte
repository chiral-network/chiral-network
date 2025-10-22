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
    Activity,
    CheckCircle,
    AlertTriangle,
    Copy
  } from "lucide-svelte";
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { homeDir } from "@tauri-apps/api/path";
  import { getVersion } from "@tauri-apps/api/app";
  import { userLocation } from "$lib/stores";
  import { GEO_REGIONS, UNKNOWN_REGION_ID } from '$lib/geo';
  import { changeLocale, loadLocale, saveLocale } from "../i18n/i18n";
  import { t } from "svelte-i18n";
  import { get } from "svelte/store";
  import { showToast } from "$lib/toast";
  import { invoke } from "@tauri-apps/api/core";
  import Expandable from "$lib/components/ui/Expandable.svelte";
  import { settings, type AppSettings } from "$lib/stores";
  import { bandwidthScheduler } from "$lib/services/bandwidthScheduler";

  let showResetConfirmModal = false;
  let storageSectionOpen = false;
  let networkSectionOpen = false;
  let advancedSectionOpen = false;

  // Settings state
  let defaultSettings: AppSettings = {
    // Storage settings
    storagePath: "~/ChiralNetwork/Storage",
    maxStorageSize: 100, // GB
    autoCleanup: true,
    cleanupThreshold: 90, // %

    // Network settings
    maxConnections: 50,
    uploadBandwidth: 0, // 0 = unlimited
    downloadBandwidth: 0, // 0 = unlimited
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
    enableAutorelay: true,
    preferredRelays: [],
    enableRelayServer: false,
    autoStartDht: false,
    anonymousMode: false,
    shareAnalytics: true,

    // Notifications
    enableNotifications: true,
    notifyOnComplete: true,
    notifyOnError: true,
    soundAlerts: false,

    // Advanced
    enableDHT: true,
    enableIPFS: false,
    chunkSize: 256, // KB
    cacheSize: 1024, // MB
    logLevel: "info",
    autoUpdate: true,
    enableBandwidthScheduling: false,
    bandwidthSchedules: [],
  };
  let localSettings: AppSettings = JSON.parse(JSON.stringify(get(settings)));
  let savedSettings: AppSettings = JSON.parse(JSON.stringify(localSettings));
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
  type DiagStatus = "pass" | "fail" | "warn";
  type DiagItem = { id: string; label: string; status: DiagStatus; details?: string };
  let diagnosticsRunning = false;
  let diagnostics: DiagItem[] = [];
  let diagnosticsReport = "";

  // NAT & privacy configuration text bindings
  let autonatServersText = '';
  let trustedProxyText = '';

  const locationOptions = GEO_REGIONS
    .filter((region) => region.id !== UNKNOWN_REGION_ID)
    .map((region) => ({ value: region.label, label: region.label }))
    .sort((a, b) => a.label.localeCompare(b.label));

  let languages = [];
  $: languages = [
    { value: "en", label: $t("language.english") },
    { value: "es", label: $t("language.spanish") },
    { value: "zh", label: $t("language.chinese") },
    { value: "ko", label: $t("language.korean") },
    { value: "ru", label: $t("language.russian") },
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
      label: "Prefer Relay (fall back to direct if needed)",
    },
    {
      value: "strict",
      label: "Strict Relay-only (never expose your IP)",
    },
  ];

  type PrivacySnapshot = Pick<
    AppSettings,
    "ipPrivacyMode" | "enableProxy" | "proxyAddress" | "disableDirectNatTraversal" | "enableAutorelay"
  >;

  let anonymousModeRestore: PrivacySnapshot | null = null;

  function capturePrivacySnapshot(): void {
    if (anonymousModeRestore !== null) {
      return;
    }
    anonymousModeRestore = {
      ipPrivacyMode: localSettings.ipPrivacyMode,
      enableProxy: localSettings.enableProxy,
      proxyAddress: localSettings.proxyAddress,
      disableDirectNatTraversal: localSettings.disableDirectNatTraversal,
      enableAutorelay: localSettings.enableAutorelay,
    };
  }

  function applyAnonymousDefaults(): void {
    capturePrivacySnapshot();

    const needsUpdate =
      localSettings.ipPrivacyMode !== "strict" ||
      !localSettings.enableProxy ||
      !localSettings.disableDirectNatTraversal ||
      !localSettings.enableAutorelay;

    if (!needsUpdate) {
      return;
    }

    localSettings = {
      ...localSettings,
      ipPrivacyMode: "strict",
      enableProxy: true,
      enableAutorelay: true,
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
      enableAutorelay: snapshot.enableAutorelay,
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
        return "Relay routing will be attempted first. If no relay is available, the app falls back to a direct connection and your IP may be exposed.";
      case "strict":
        return "All traffic must tunnel through a trusted relay or proxy. Direct connections and IP exposure are blocked.";
      default:
        return "Direct connections are allowed. Use a SOCKS5 proxy if you still want to mask your IP.";
    }
  })();

  // Check for changes
  $: hasChanges = JSON.stringify(localSettings) !== JSON.stringify(savedSettings);

  async function saveSettings() {
    if (!isValid || maxStorageError || storagePathError) {
      return;
    }

    // Save local changes to the Svelte store
    settings.set(localSettings);

    // Save to local storage
    localStorage.setItem("chiralSettings", JSON.stringify(localSettings));
    
    savedSettings = JSON.parse(JSON.stringify(localSettings));
    userLocation.set(localSettings.userLocation);
    
    // Force bandwidth scheduler to update with new settings
    bandwidthScheduler.forceUpdate();
    
    importExportFeedback = null;

    try {
      await applyPrivacyRoutingSettings();
      await restartDhtWithProxy();
      showToast("Settings Updated!");
    } catch (error) {
      console.error("Failed to apply networking settings:", error);
      showToast("Settings saved, but networking update failed", "error");
    }
  }

  async function applyPrivacyRoutingSettings() {
    if (typeof window === "undefined" || !("__TAURI__" in window)) {
      return;
    }

    if (localSettings.ipPrivacyMode !== "off" && (!localSettings.trustedProxyRelays || localSettings.trustedProxyRelays.length === 0)) {
      showToast("Add at least one trusted proxy relay before enabling Hide My IP.", "warning");
      try {
        await invoke("disable_privacy_routing");
      } catch (error) {
        console.warn("disable_privacy_routing failed while updating privacy settings:", error);
      }
      return;
    }

    if (localSettings.ipPrivacyMode === "off") {
      try {
        await invoke("disable_privacy_routing");
      } catch (error) {
        console.warn("disable_privacy_routing failed while turning privacy off:", error);
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
      console.debug("stop_dht_node failed (probably already stopped):", error);
    }

    let bootstrapNodes: string[] = [];
    try {
      bootstrapNodes = await invoke<string[]>("get_bootstrap_nodes_command");
    } catch (error) {
      console.error("Failed to fetch bootstrap nodes:", error);
      throw error;
    }

    if (!Array.isArray(bootstrapNodes) || bootstrapNodes.length === 0) {
      throw new Error("No bootstrap nodes available to restart DHT");
    }

    const payload: Record<string, unknown> = {
      port: localSettings.port,
      bootstrapNodes,
      enableAutonat: !localSettings.disableDirectNatTraversal,
      autonatProbeIntervalSecs: localSettings.autonatProbeInterval,
      chunkSizeKb: localSettings.chunkSize,
      cacheSizeMb: localSettings.cacheSize,
      enableAutorelay: localSettings.ipPrivacyMode !== "off" ? true : localSettings.enableAutorelay,
      enableRelayServer: localSettings.enableRelayServer,
    };

    if (localSettings.autonatServers?.length) {
      payload.autonatServers = localSettings.autonatServers;
    }
    if (localSettings.trustedProxyRelays?.length) {
      payload.preferredRelays = localSettings.trustedProxyRelays;
    } else if (localSettings.preferredRelays?.length) {
      payload.preferredRelays = localSettings.preferredRelays;
    }
    if (localSettings.enableProxy && localSettings.proxyAddress?.trim()) {
      payload.proxyAddress = localSettings.proxyAddress.trim();
    }

    await invoke("start_dht_node", payload);
  }

  $: {
    // Open Storage section if it has any errors (but don't close it if already open)
    const hasStorageError = !!maxStorageError || !!storagePathError || !!errors.maxStorageSize || !!errors.cleanupThreshold;
    if (hasStorageError) storageSectionOpen = true;

    // Open Network section if it has any errors (but don't close it if already open)
    const hasNetworkError = !!errors.maxConnections || !!errors.port || !!errors.uploadBandwidth || !!errors.downloadBandwidth;
    if (hasNetworkError) networkSectionOpen = true;

    // Open Advanced section if it has any errors (but don't close it if already open)
    const hasAdvancedError = !!errors.chunkSize || !!errors.cacheSize;
    if (hasAdvancedError) advancedSectionOpen = true;
  }

  async function handleConfirmReset() {
    localSettings = { ...defaultSettings }; // Reset local changes
    settings.set(defaultSettings); // Reset the store
    await saveSettings(); // Save the reset state
    showResetConfirmModal = false;
  }

  function openResetConfirm() {
    showResetConfirmModal = true;
  }

  async function selectStoragePath() {
    const tr = get(t) as (key: string, params?: any) => string;
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
            console.error("Directory picker error:", err);
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

  function exportSettings() {
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
      message: $t("advanced.exportSuccess", {
        default: "Settings exported to your browser's download folder.",
      }),
      type: "success",
    };
  }

  function importSettings(event: Event) {
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
          message: $t("advanced.importSuccess", {
            default: "Settings imported successfully.",
          }),
          type: "success",
        };
      } catch (err) {
        console.error("Failed to import settings:", err);
        importExportFeedback = {
          message: $t("advanced.importError", {
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

  async function runDiagnostics() {
    diagnosticsRunning = true;
    diagnostics = [];
    diagnosticsReport = "";

    const add = (item: DiagItem) => {
      diagnostics = [...diagnostics, item];
    };

    const tr = get(t) as (key: string, params?: any) => string;

    // 1) Environment (Web vs Tauri)
    const isTauri = typeof window !== "undefined" && "__TAURI__" in window;
    try {
      if (isTauri) {
        const ver = await getVersion();
        add({ id: "env", label: tr("settings.diagnostics.environment"), status: "pass", details: `Tauri ${ver}` });
      } else {
        add({ id: "env", label: tr("settings.diagnostics.environment"), status: "warn", details: "Web build: some checks skipped" });
      }
    } catch (e:any) {
      add({ id: "env", label: tr("settings.diagnostics.environment"), status: "fail", details: String(e) });
    }

    // 2) i18n storage read/write
    try {
      const before = await loadLocale();
      await saveLocale(before || "en");
      const after = await loadLocale();
      const ok = (before || "en") === (after || "en");
      add({ id: "i18n", label: tr("settings.diagnostics.i18nStorage"), status: ok ? "pass" : "warn", details: `value=${after ?? "null"}` });
    } catch (e:any) {
      add({ id: "i18n", label: tr("settings.diagnostics.i18nStorage"), status: "fail", details: String(e) });
    }

    // 3) Bootstrap nodes availability (DHT)
    try {
      // Only in Tauri builds
      if (isTauri) {
        const nodes = await invoke<string[]>("get_bootstrap_nodes_command");
        const count = Array.isArray(nodes) ? nodes.length : 0;
        add({ id: "dht", label: tr("settings.diagnostics.bootstrapNodes"), status: count > 0 ? "pass" : "fail", details: `count=${count}` });
      } else {
        add({ id: "dht", label: tr("settings.diagnostics.bootstrapNodes"), status: "warn", details: "Skipped in web build" });
      }
    } catch (e:any) {
      add({ id: "dht", label: tr("settings.diagnostics.bootstrapNodes"), status: "fail", details: String(e) });
    }

    // 4) Privacy routing configuration sanity
    try {
      const ipMode = localSettings.ipPrivacyMode;
      const trusted = localSettings.trustedProxyRelays?.length ?? 0;
      if (ipMode !== "off" && trusted === 0) {
        add({ id: "privacy", label: tr("settings.diagnostics.privacyConfig"), status: "warn", details: tr("settings.diagnostics.privacyNeedsTrusted") });
      } else {
        add({ id: "privacy", label: tr("settings.diagnostics.privacyConfig"), status: "pass", details: `mode=${ipMode}, trusted=${trusted}` });
      }
    } catch (e:any) {
      add({ id: "privacy", label: tr("settings.diagnostics.privacyConfig"), status: "fail", details: String(e) });
    }

    // Build report text
    diagnosticsReport = diagnostics
      .map((d) => `${d.status.toUpperCase()} - ${d.label}: ${d.details ?? ""}`)
      .join("\n");
    diagnosticsRunning = false;
  }

  async function copyDiagnostics() {
    try {
      await navigator.clipboard.writeText(diagnosticsReport);
      showToast(tr("settings.diagnostics.copied"));
    } catch (e) {
      showToast(tr("settings.diagnostics.copyFailed"), "error");
    }
  }

    onMount(async () => {
    // Get platform-specific default storage path from Tauri
    try {
      const platformDefaultPath = await invoke<string>("get_default_storage_path");
      defaultSettings.storagePath = platformDefaultPath;
    } catch (e) {
      console.error("Failed to get default storage path:", e);
      // Fallback to the hardcoded default if the command fails
      defaultSettings.storagePath = "~/ChiralNetwork/Storage";
    }

    // Load settings from local storage
    const stored = localStorage.getItem("chiralSettings");
    if (stored) {
  try {
    const loadedSettings: AppSettings = JSON.parse(stored);
    // Set the store, which ensures it is available globally
    settings.set({ ...defaultSettings, ...loadedSettings });
    // Update local state from the store after loading
    localSettings = JSON.parse(JSON.stringify(get(settings)));
    savedSettings = JSON.parse(JSON.stringify(localSettings)); 
  } catch (e) {
    console.error("Failed to load settings:", e);
  }
}

const saved = await loadLocale(); // 'en' | 'ko' | null
const initial = saved || "en";
selectedLanguage = initial; // Synchronize dropdown display value
// (From root, setupI18n() has already been called, so only once here)
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
    chunkSize: { min: 64, max: 1024, label: "Chunk Size (KB)" },
    cacheSize: { min: 256, max: 8192, label: "Cache Size (MB)" },
  } as const;

  let errors: Record<string, string | null> = {};

  function rangeMessage(label: string, min: number, max: number) {
    if (max === Infinity) return `${label} must be ≥ ${min}.`;
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
    errors = next;
}

  // Revalidate whenever settings change
  $: validate(localSettings);

  // Valid when no error messages remain
  let isValid = true;
  $: isValid = Object.values(errors).every((e) => !e);

  let freeSpaceGB: number | null = null;
  let maxStorageError: string | null = null;
  let storagePathError: string | null = null;

  onMount(async () => {
    freeSpaceGB = await invoke('get_available_storage');
  });

  // Check if storage path exists
  async function checkStoragePathExists(path: string) {
    if (!path || path.trim() === '') {
      storagePathError = null;
      return;
    }

    try {
      // Expand ~ to actual home directory for checking
      let pathToCheck = path;
      if (path.startsWith("~/") || path.startsWith("~\\")) {
        const home = await homeDir();
        pathToCheck = path.replace(/^~/, home);
      } else if (path === "~") {
        pathToCheck = await homeDir();
      }

      const exists = await invoke<boolean>('check_directory_exists', { path: pathToCheck });
      if (!exists) {
        storagePathError = 'Directory does not exist';
      } else {
        storagePathError = null;
      }
    } catch (error) {
      console.error('Failed to check directory:', error);
      storagePathError = null;
    }
  }

  // Check storage path whenever it changes
  $: if (localSettings.storagePath) {
    checkStoragePathExists(localSettings.storagePath);
  }

  $: {
    if (freeSpaceGB !== null && localSettings.maxStorageSize > freeSpaceGB) {
      maxStorageError = `Insufficient disk space. Only ${freeSpaceGB} GB available.`;
    } else {
      maxStorageError = null;
    }
  }

  let search = '';

const sectionLabels: Record<string, string[]> = {
  storage: [
    $t("storage.title"),
    $t("storage.location"),
    $t("storage.maxSize"),
    $t("storage.cleanupThreshold"),
    $t("storage.enableCleanup"),
  ],
  network: [
    $t("network.title"),
    $t("network.maxConnections"),
    $t("network.port"),
    $t("network.uploadLimit"),
    $t("network.downloadLimit"),
    $t("network.userLocation"),
    $t("network.enableUpnp"),
    $t("network.enableNat"),
    $t("network.enableDht"),
  ],
  bandwidthScheduling: [
    "Bandwidth Scheduling",
    "Enable Bandwidth Scheduling",
    "Schedule different bandwidth limits",
  ],
  language: [
    $t("language.title"),
    $t("language.select"),
  ],
  privacy: [
    $t("privacy.title"),
    $t("privacy.enableProxy"),
    $t("privacy.anonymousMode"),
    $t("privacy.shareAnalytics"),
  ],
  notifications: [
    $t("notifications.title"),
    $t("notifications.enable"),
    $t("notifications.notifyComplete"),
    $t("notifications.notifyError"),
    $t("notifications.soundAlerts"),
  ],
  advanced: [
    $t("advanced.title"),
    $t("advanced.chunkSize"),
    $t("advanced.cacheSize"),
    $t("advanced.logLevel"),
    $t("advanced.autoUpdate"),
    $t("advanced.exportSettings"),
    $t("advanced.importSettings"),
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
              placeholder="~/ChiralNetwork/Storage"
              class={`flex-1 ${storagePathError ? 'border-red-500 focus:border-red-500 ring-red-500' : ''}`}
            />
            <Button
              variant="outline"
              on:click={selectStoragePath}
              aria-label={$t("storage.locationPick")}
            >
              <FolderOpen class="h-4 w-4" />
            </Button>
          </div>
          {#if storagePathError}
            <p class="mt-1 text-sm text-red-500">{storagePathError}</p>
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
              class="mt-2"
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
              class="mt-2"
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
              class="mt-2"
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
              class="mt-2"
            />
            {#if errors.downloadBandwidth}
              <p class="mt-1 text-sm text-red-500">{errors.downloadBandwidth}</p>
            {/if}
          </div>
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

          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="enable-dht"
              bind:checked={localSettings.enableDHT}
            />
            <Label for="enable-dht" class="cursor-pointer">
              {$t("network.enableDht")}
            </Label>
          </div>

          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="auto-start-dht"
              bind:checked={localSettings.autoStartDht}
            />
            <Label for="auto-start-dht" class="cursor-pointer">
              Auto-start Network on App Launch
            </Label>
          </div>

          {#if localSettings.autoStartDht}
            <div class="ml-6 p-3 bg-blue-50 rounded-md border border-blue-200">
              <p class="text-xs text-blue-900">
                The DHT network will automatically start when you open the application, so you don't have to manually start it each time.
              </p>
            </div>
          {/if}

          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="enable-relay-server"
              bind:checked={localSettings.enableRelayServer}
            />
            <Label for="enable-relay-server" class="cursor-pointer">
              Enable Relay Server <span class="text-xs text-green-600 font-semibold">(Recommended - Enabled by Default)</span>
            </Label>
          </div>

          {#if localSettings.enableRelayServer}
            <div class="ml-6 p-4 bg-green-50 rounded-md border border-green-200">
              <p class="text-sm text-green-900 mb-2">
                <strong>✅ Relay Server Enabled</strong>
              </p>
              <p class="text-xs text-green-700 mb-2">
                Your node helps peers behind NAT connect. This strengthens the decentralized network without requiring central infrastructure.
              </p>
              <ul class="text-xs text-green-600 space-y-1">
                <li>• Enables cross-network peer connections</li>
                <li>• Strengthens network decentralization</li>
                <li>• Minimal resource usage when idle</li>
                <li>• Only uses bandwidth when actively relaying</li>
              </ul>
            </div>
          {:else}
            <div class="ml-6 p-4 bg-yellow-50 rounded-md border border-yellow-200">
              <p class="text-sm text-yellow-900 mb-2">
                <strong>⚠️ Relay Server Disabled</strong>
              </p>
              <p class="text-xs text-yellow-700">
                Your node cannot help others connect across networks. Enable this to strengthen the network.
              </p>
            </div>
          {/if}
        </div>
      </div>
    </Expandable>
  {/if}

  <!-- Bandwidth Scheduling -->
  {#if sectionMatches("bandwidthScheduling", search)}
    <Expandable>
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
    <Expandable>
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
    <Expandable>
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
              class="mt-1"
            />
            <p class="text-xs text-muted-foreground mt-1">{$t("privacy.proxyHint")}</p>
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
              <Label for="trusted-proxies">Trusted proxy relays</Label>
              <textarea
                id="trusted-proxies"
                bind:value={trustedProxyText}
                on:blur={updateTrustedProxyRelays}
                placeholder="/dns4/relay.example.com/tcp/4001/p2p/12D3KooW...\nOne multiaddress per line."
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
                Disable STUN/DCUtR while hiding IP
              </Label>
            </div>

            <div class="rounded-md border border-dashed border-slate-200 p-3 text-xs text-muted-foreground space-y-1">
              <p>Trusted relays configured: {localSettings.trustedProxyRelays.length}</p>
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
    <Expandable>
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
          <Button variant="outline" size="xs" on:click={exportSettings}>
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
              on:change={importSettings}
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
    <Expandable>
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
          {#if diagnostics.length > 0}
            <Button variant="outline" size="xs" on:click={copyDiagnostics}>
              <Copy class="h-4 w-4 mr-2" />{$t("settings.diagnostics.copyReport")}
            </Button>
          {/if}
        </div>

        {#if diagnostics.length > 0}
          <div>
            <h3 class="font-medium mb-2">{$t("settings.diagnostics.resultsTitle")}</h3>
            <ul class="space-y-2">
              {#each diagnostics as d}
                <li class="flex items-start gap-2">
                  {#if d.status === 'pass'}
                    <CheckCircle class="h-4 w-4 text-green-600 mt-0.5" />
                  {:else if d.status === 'warn'}
                    <AlertTriangle class="h-4 w-4 text-amber-600 mt-0.5" />
                  {:else}
                    <AlertTriangle class="h-4 w-4 text-red-600 mt-0.5" />
                  {/if}
                  <div>
                    <div class="text-sm font-medium">{d.label}</div>
                    {#if d.details}
                      <div class="text-xs text-muted-foreground">{d.details}</div>
                    {/if}
                  </div>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
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
        disabled={!hasChanges || maxStorageError || storagePathError || !isValid}
      
        class={`transition-colors duration-200 ${!hasChanges || maxStorageError || storagePathError || !isValid ? "cursor-not-allowed opacity-50" : ""}`}
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
