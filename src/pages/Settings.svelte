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
    enableAutoRelay: true, // Enabled by default
    preferredRelays: "", // Empty by default
    userLocation: "US-East", // Geographic region for peer sorting

    // Privacy settings
    enableProxy: true,
    proxyAddress: "127.0.0.1:9050", // Default Tor SOCKS address
    enableAutonat: true,
    autonatProbeInterval: 30,
    autonatServers: [],
    enableAutorelay: true,
    preferredRelays: [],
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
  let localSettings: AppSettings = get(settings); 
  let savedSettings: AppSettings = get(settings);
  let hasChanges = false;
  let fileInputEl: HTMLInputElement | null = null;
  let selectedLanguage: string | undefined = undefined;
  let clearingCache = false;
  let cacheCleared = false;
  let importExportFeedback: {
    message: string;
    type: "success" | "error";
  } | null = null;

  // NAT configuration text bindings
  let autonatServersText = '';
  let preferredRelaysText = '';

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

  // Initialize NAT configuration text from arrays
  $: autonatServersText = localSettings.autonatServers?.join('\n') || '';
  $: preferredRelaysText = localSettings.preferredRelays?.join('\n') || '';

  $: if (localSettings.anonymousMode) {
    localSettings.enableProxy = true;
    localSettings.enableAutonat = false;
    localSettings.enableAutorelay = true;
  }

  // Check for changes
  $: hasChanges = JSON.stringify(localSettings) !== JSON.stringify(savedSettings);

  function saveSettings() {
    if (!isValid || maxStorageError) {
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
    showToast("Settings Updated!");
  }

  $: {
    // Expand Storage section if it has any errors
    const hasStorageError = !!maxStorageError || !!errors.maxStorageSize || !!errors.cleanupThreshold;
    storageSectionOpen = hasStorageError;

    // Expand Network section if it has any errors
    const hasNetworkError = !!errors.maxConnections || !!errors.port || !!errors.uploadBandwidth || !!errors.downloadBandwidth;
    networkSectionOpen = hasNetworkError;

    // Expand Advanced section if it has any errors
    const hasAdvancedError = !!errors.chunkSize || !!errors.cacheSize;
    advancedSectionOpen = hasAdvancedError;
  }

  function handleConfirmReset() {
    localSettings = { ...defaultSettings }; // Reset local changes
    settings.set(defaultSettings); // Reset the store
    saveSettings(); // Save the reset state
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
        localSettings.storagePath = result.replace(home, "~");
      }
    } catch {
      // Fallback for browser environment
      if ("showDirectoryPicker" in window) {
        // Use File System Access API (Chrome/Edge)
        try {
          const directoryHandle = await (window as any).showDirectoryPicker();
          localSettings.storagePath = directoryHandle.name;
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
          localSettings.storagePath = newPath;
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
    reader.onload = (e) => {
      try {
        const imported = JSON.parse(e.target?.result as string);
        localSettings = { ...localSettings, ...imported };
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

  function updateAutonatServers() {
    localSettings.autonatServers = autonatServersText
      .split('\n')
      .map(s => s.trim())
      .filter(s => s.length > 0);
  }

  function updatePreferredRelays() {
    localSettings.preferredRelays = preferredRelaysText
      .split('\n')
      .map(s => s.trim())
      .filter(s => s.length > 0);
  }

    onMount(async () => {
    // Load settings from local storage
    const stored = localStorage.getItem("chiralSettings");
    if (stored) {
      try {
        const loadedSettings: AppSettings = JSON.parse(stored);
        // Set the store, which ensures it is available globally
        settings.set({ ...defaultSettings, ...loadedSettings }); 
        // Update local state from the store after loading
        localSettings = get(settings); 
        savedSettings = get(settings); 
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
            options={locations}
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
              id="enable-autorelay"
              bind:checked={localSettings.enableAutoRelay}
            />
            <Label for="enable-autorelay" class="cursor-pointer">
              {$t("settings.autorelay.enable")}
            </Label>
          </div>

          {#if localSettings.enableAutoRelay}
            <div class="space-y-2 ml-6">
              <Label for="preferred-relays">{$t("settings.autorelay.relays")}</Label>
              <textarea
                id="preferred-relays"
                bind:value={localSettings.preferredRelays}
                placeholder="/ip4/147.75.80.110/tcp/4001/p2p/QmNnooDu..."
                rows="3"
                class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm font-mono"
              />
              <p class="text-xs text-muted-foreground">
                {$t("settings.autorelay.description")}
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
      <div class="space-y-2">
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

        <!-- AutoRelay Configuration -->
        <div class="space-y-3 border-t pt-3">
          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="enable-autorelay"
              bind:checked={localSettings.enableAutorelay}
            />
            <Label for="enable-autorelay" class="cursor-pointer">
              Enable Circuit Relay v2 (AutoRelay)
            </Label>
          </div>

          {#if localSettings.enableAutorelay}
            <div>
              <Label for="preferred-relays">Preferred Relay Nodes (optional)</Label>
              <textarea
                id="preferred-relays"
                bind:value={preferredRelaysText}
                on:blur={updatePreferredRelays}
                placeholder="/ip4/relay.example.com/tcp/4001/p2p/QmRelayId&#10;One multiaddr per line"
                rows="3"
                class="w-full px-3 py-2 border rounded-md text-sm"
              ></textarea>
              <p class="text-xs text-muted-foreground mt-1">
                Leave empty to use bootstrap nodes as relays
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
        on:click={() => (localSettings = { ...savedSettings })}
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