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
    Languages
  } from "lucide-svelte";
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { homeDir } from "@tauri-apps/api/path";
  import { getVersion } from "@tauri-apps/api/app";
  import { userLocation } from "$lib/stores";
  import { changeLocale, loadLocale } from "../i18n/i18n";
  import { t } from "svelte-i18n";
  import { get } from "svelte/store";
  import { showToast } from "$lib/toast";
  import { invoke } from "@tauri-apps/api/core";
  import Expandable from "$lib/components/ui/Expandable.svelte";

  let showResetConfirmModal = false;
  // Settings state
  let defaultSettings = {
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
    enableEncryption: true,
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
  };
  let settings = { ...defaultSettings };
  let savedSettings = { ...defaultSettings };
  let hasChanges = false;
  let fileInputEl: HTMLInputElement | null = null;
  let selectedLanguage: string | undefined = undefined;
  let clearingCache = false;
  let cacheCleared = false;
  let importExportFeedback: {
    message: string;
    type: "success" | "error";
  } | null = null;

  const locations = [
    { value: "US-East", label: "US East" },
    { value: "US-West", label: "US West" },
    { value: "EU-West", label: "Europe West" },
    { value: "Asia-Pacific", label: "Asia Pacific" },
  ];

  let languages = [];
  $: languages = [
    { value: "en", label: $t("language.english") },
    { value: "es", label: $t("language.spanish") },
    { value: "zh", label: $t("language.chinese") },
    { value: "ko", label: $t("language.korean") },
  ];

  // Check for changes
  $: hasChanges = JSON.stringify(settings) !== JSON.stringify(savedSettings);

  function saveSettings() {
    if (!isValid || maxStorageError) {
      return;
    }

    // Save to local storage
    localStorage.setItem("chiralSettings", JSON.stringify(settings));
    savedSettings = { ...settings };
    userLocation.set(settings.userLocation);
    importExportFeedback = null;
    showToast("Settings Updated!");
  }

  function handleConfirmReset() {
    settings = { ...defaultSettings };
    saveSettings();
    showResetConfirmModal = false;
  }

  function openResetConfirm() {
    showResetConfirmModal = true;
  }

  async function selectStoragePath() {
    const tr = get(t);
    try {
      // Try Tauri first
      await getVersion(); // only works in Tauri
      const home = await homeDir();
      const result = await open({
        directory: true,
        multiple: false,
        defaultPath: settings.storagePath.startsWith("~/")
          ? settings.storagePath.replace("~", home)
          : settings.storagePath,
        title: tr("storage.selectLocationTitle"),
      });

      if (typeof result === "string") {
        settings.storagePath = result.replace(home, "~");
      }
    } catch {
      // Fallback for browser environment
      if ("showDirectoryPicker" in window) {
        // Use File System Access API (Chrome/Edge)
        try {
          const directoryHandle = await (window as any).showDirectoryPicker();
          settings.storagePath = directoryHandle.name;
        } catch (err: any) {
          if (err.name !== "AbortError") {
            console.error("Directory picker error:", err);
          }
        }
      } else {
        // Fallback: let user type path manually
        const newPath = prompt(
          `${tr("storage.enterPathPrompt")} ( ${tr("storage.browserNoPicker")} )`,
          settings.storagePath
        );
        if (newPath) {
          settings.storagePath = newPath;
        }
      }
    }
  }

  async function clearCache() {
    clearingCache = true;
    // Clear application cache
    console.log("Clearing cache...");
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
    const blob = new Blob([JSON.stringify(settings, null, 2)], {
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
    reader.onload = (e) => {
      try {
        const imported = JSON.parse(e.target?.result as string);
        settings = { ...settings, ...imported };
        saveSettings(); // This saves, updates savedSettings, and clears any old feedback.
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

    onMount(async () => {
    // Load settings from local storage
    const stored = localStorage.getItem("chiralSettings");
    if (stored) {
      try {
        settings = JSON.parse(stored);
        savedSettings = { ...settings };
      } catch (e) {
        console.error("Failed to load settings:", e);
      }
    }

    const saved = await loadLocale(); // 'en' | 'ko' | null
    const initial = saved || "en";
    selectedLanguage = initial; // Synchronize dropdown display value
    // (From root, setupI18n() has already been called, so only once here)
  });

  function onLanguageChange(lang: string) {
    selectedLanguage = lang;
    changeLocale(lang); // Save + update global state (yes, i18n.ts takes care of saving)
    (settings as any).language = lang;
    saveSettings(); // If you want to reflect in settings as well
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

  function validate(settings: any) {
    const next: Record<string, string | null> = {};
    for (const [key, cfg] of Object.entries(limits)) {
      const val = Number((settings as any)[key]);
      if (val < cfg.min || val > cfg.max) {
        next[key] = rangeMessage(cfg.label, cfg.min, cfg.max);
        continue;
      }
      next[key] = null;
    }
    errors = next;
  }

  // Revalidate whenever settings change
  $: validate(settings);

  // Valid when no error messages remain
  let isValid = true;
  $: isValid = Object.values(errors).every((e) => !e);

  let freeSpaceGB: number | null = null;
  let maxStorageError: string | null = null;

  onMount(async () => {
    freeSpaceGB = await invoke('get_available_storage');
  });

  $: {
    if (freeSpaceGB !== null && settings.maxStorageSize > freeSpaceGB) {
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
  language: [
    $t("language.title"),
    $t("language.select"),
  ],
  privacy: [
    $t("privacy.title"),
    $t("privacy.enableProxy"),
    $t("privacy.enableEncryption"),
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
      placeholder="Search settings..."
      bind:value={search}
      class="w-full"
    />
  </div>

  <!-- Storage Settings -->
  {#if sectionMatches("storage", search)}
    <Expandable>
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
              bind:value={settings.storagePath}
              placeholder="~/ChiralNetwork/Storage"
              class="flex-1"
            />
            <Button
              variant="outline"
              on:click={selectStoragePath}
              aria-label={$t("storage.locationPick")}
            >
              <FolderOpen class="h-4 w-4" />
            </Button>
          </div>
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
                  bind:value={settings.maxStorageSize}
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
              bind:value={settings.cleanupThreshold}
              min="50"
              max="100"
              disabled={!settings.autoCleanup}
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
            bind:checked={settings.autoCleanup}
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
    <Expandable>
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
              bind:value={settings.maxConnections}
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
              bind:value={settings.port}
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
              bind:value={settings.uploadBandwidth}
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
              bind:value={settings.downloadBandwidth}
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
            options={locations}
            bind:value={settings.userLocation}
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
              bind:checked={settings.enableUPnP}
            />
            <Label for="enable-upnp" class="cursor-pointer">
              {$t("network.enableUpnp")}
            </Label>
          </div>

          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="enable-nat"
              bind:checked={settings.enableNAT}
            />
            <Label for="enable-nat" class="cursor-pointer">
              {$t("network.enableNat")}
            </Label>
          </div>

          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="enable-dht"
              bind:checked={settings.enableDHT}
            />
            <Label for="enable-dht" class="cursor-pointer">
              {$t("network.enableDht")}
            </Label>
          </div>
        </div>
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
      <div class="space-y-2">
        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="enable-proxy"
            bind:checked={settings.enableProxy}
          />
          <Label for="enable-proxy" class="cursor-pointer">
            {$t("privacy.enableProxy")}
          </Label>
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="enable-encryption"
            bind:checked={settings.enableEncryption}
          />
          <Label for="enable-encryption" class="cursor-pointer">
            {$t("privacy.enableEncryption")}
          </Label>
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="anonymous-mode"
            bind:checked={settings.anonymousMode}
          />
          <Label for="anonymous-mode" class="cursor-pointer">
            {$t("privacy.anonymousMode")}
          </Label>
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="share-analytics"
            bind:checked={settings.shareAnalytics}
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
            bind:checked={settings.enableNotifications}
          />
          <Label for="enable-notifications" class="cursor-pointer">
            {$t("notifications.enable")}
          </Label>
        </div>

        {#if settings.enableNotifications}
          <div class="ml-6 space-y-2">
            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="notify-complete"
                bind:checked={settings.notifyOnComplete}
              />
              <Label for="notify-complete" class="cursor-pointer">
                {$t("notifications.notifyComplete")}
              </Label>
            </div>

            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="notify-error"
                bind:checked={settings.notifyOnError}
              />
              <Label for="notify-error" class="cursor-pointer">
                {$t("notifications.notifyError")}
              </Label>
            </div>

            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="sound-alerts"
                bind:checked={settings.soundAlerts}
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
    <Expandable>
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
              bind:value={settings.chunkSize}
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
              bind:value={settings.cacheSize}
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
            bind:value={settings.logLevel}
          />
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="auto-update"
            bind:checked={settings.autoUpdate}
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
        on:click={() => (settings = { ...savedSettings })}
        class={`transition-colors duration-200 ${!hasChanges ? "cursor-not-allowed opacity-50" : ""}`}
      >
        {$t("actions.cancel")}
      </Button>

      <Button
        size="xs"
        on:click={saveSettings}
        disabled={!hasChanges || !!maxStorageError || !isValid}
        class={`transition-colors duration-200 ${!hasChanges || !!maxStorageError || !isValid ? "cursor-not-allowed opacity-50" : ""}`}
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