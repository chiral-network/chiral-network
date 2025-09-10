<script lang="ts">
  import Card from "$lib/components/ui/card.svelte";
  import Button from "$lib/components/ui/button.svelte";
  import Input from "$lib/components/ui/input.svelte";
  import Label from "$lib/components/ui/label.svelte";
  import Badge from "$lib/components/ui/badge.svelte";
  import {
    Save,
    FolderOpen,
    HardDrive,
    Wifi,
    Shield,
    Bell,
    RefreshCw,
    Database,
    ChevronsUpDown,
  } from "lucide-svelte";
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { homeDir } from "@tauri-apps/api/path";
  import { getVersion } from "@tauri-apps/api/app";
  import { userLocation } from "$lib/stores";

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

  // Check for changes
  $: hasChanges = JSON.stringify(settings) !== JSON.stringify(savedSettings);

  function saveSettings() {
    // Save to local storage
    localStorage.setItem("chiralSettings", JSON.stringify(settings));
    savedSettings = { ...settings };
    userLocation.set(settings.userLocation);
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
      title: "Select Storage Location",
    });

    if (typeof result === "string") {
      settings.storagePath = result.replace(home, "~");
    }
  } catch {
    // Fallback for browser environment
    if (window.showDirectoryPicker) {
      // Use File System Access API (Chrome/Edge)
      try {
        const directoryHandle = await window.showDirectoryPicker();
        settings.storagePath = directoryHandle.name;
      } catch (err) {
        if (err.name !== 'AbortError') {
          console.error('Directory picker error:', err);
        }
      }
    } else {
      // Fallback: let user type path manually
      const newPath = prompt(
        "Enter storage path (Tauri file picker not available in browser):", 
        settings.storagePath
      );
      if (newPath) {
        settings.storagePath = newPath;
      }
    }
  }
}

  function clearCache() {
    // Clear application cache
    console.log("Clearing cache...");
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
  }

  function importSettings(event: Event) {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = (e) => {
      try {
        const imported = JSON.parse(e.target?.result as string);
        settings = { ...settings, ...imported };
        saveSettings();
        localStorage.setItem("chiralSettings", JSON.stringify(settings));
        hasChanges = false;
      } catch (err) {
        console.error("Failed to import settings:", err);
        alert("Invalid JSON file. Please select a valid export.");
      }
    };
    reader.readAsText(file);

    // allow re-uploading the same file later
    (event.target as HTMLInputElement).value = "";
  }

  onMount(() => {
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
  });

  const limits = {
    maxStorageSize:     { min: 10,   max: 10000, label: "Max Storage Size (GB)" },
    cleanupThreshold:   { min: 50,   max: 100,   label: "Auto-Cleanup Threshold (%)" },
    maxConnections:     { min: 10,   max: 200,   label: "Max Connections" },
    port:               { min: 1024, max: 65535, label: "Port" },
    uploadBandwidth:    { min: 0,    max: Infinity, label: "Upload Limit (MB/s)" },
    downloadBandwidth:  { min: 0,    max: Infinity, label: "Download Limit (MB/s)" },
    chunkSize:          { min: 64,   max: 1024,  label: "Chunk Size (KB)" },
    cacheSize:          { min: 256,  max: 8192,  label: "Cache Size (MB)" },
  } as const;

  let errors: Record<string, string | null> = {};

  function rangeMessage(label: string, min: number, max: number) {
    if (max === Infinity) return `${label} must be ≥ ${min}.`;
    return `${label} must be between ${min} and ${max}.`;
  }

  function validate(settings : any) {
    const next: Record<string, string | null> = {};
    for (const [key, cfg] of Object.entries(limits)) {
      const val = Number((settings as any)[key]);
      if (val < cfg.min || val > cfg.max) {
        next[key] = rangeMessage(cfg.label, cfg.min, cfg.max);
        continue;
      }
      next[key] = null;
    }
    console.log(next)
    errors = next;
  }

  // Revalidate whenever settings change
  $: validate(settings);

  // Valid when no error messages remain
  $: isValid = Object.values(errors).every((e) => !e);

</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">Settings</h1>
      <p class="text-muted-foreground mt-2">
        Configure your Chiral Network preferences
      </p>
    </div>
    {#if hasChanges}
      <Badge variant="outline" class="text-orange-500">Unsaved changes</Badge>
    {/if}
  </div>

  <!-- Storage Settings -->
  <Card class="p-6">
    <div class="flex items-center gap-2 mb-4">
      <HardDrive class="h-5 w-5" />
      <h2 class="text-lg font-semibold">Storage</h2>
    </div>

    <div class="space-y-4">
      <div>
        <Label for="storage-path">Storage Location</Label>
        <div class="flex gap-2 mt-2">
          <Input
            id="storage-path"
            bind:value={settings.storagePath}
            placeholder="~/ChiralNetwork/Storage"
            class="flex-1"
          />
          <Button variant="outline" on:click={selectStoragePath}>
            <FolderOpen class="h-4 w-4" />
          </Button>
        </div>
      </div>

      <div class="grid grid-cols-2 gap-4">
        <div>
          <Label for="max-storage">Max Storage Size (GB)</Label>
          <Input
            id="max-storage"
            type="number"
            bind:value={settings.maxStorageSize}
            min="10"
            max="10000"
            class="mt-2"
          />
          {#if errors.maxStorageSize}
            <p class="mt-1 text-sm text-red-500">{errors.maxStorageSize}</p>
          {/if}
        </div>

        <div>
          <Label for="cleanup-threshold">Auto-Cleanup Threshold (%)</Label>
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
          Enable automatic cleanup when storage is full
        </Label>
      </div>
    </div>
  </Card>

  <!-- Network Settings -->
  <Card class="p-6">
    <div class="flex items-center gap-2 mb-4">
      <Wifi class="h-5 w-5" />
      <h2 class="text-lg font-semibold">Network</h2>
    </div>

    <div class="space-y-4">
      <div class="grid grid-cols-2 gap-4">
        <div>
          <Label for="max-connections">Max Connections</Label>
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
          <Label for="port">Port</Label>
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
          <Label for="upload-bandwidth">Upload Limit (MB/s, 0=unlimited)</Label>
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
          <Label for="download-bandwidth"
            >Download Limit (MB/s, 0=unlimited)</Label
          >
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
        <Label for="user-location">Your Location</Label>
        <select
          id="user-location"
          bind:value={settings.userLocation}
          class="w-full px-3 py-2 mt-2 border rounded-md bg-white"
        >
          <option value="US-East">US East</option>
          <option value="US-West">US West</option>
          <option value="EU-West">Europe West</option>
          <option value="Asia-Pacific">Asia Pacific</option>
        </select>
        <p class="text-xs text-muted-foreground mt-1">
          Used to prioritize geographically closer peers for better performance
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
            Enable UPnP port mapping
          </Label>
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="enable-nat"
            bind:checked={settings.enableNAT}
          />
          <Label for="enable-nat" class="cursor-pointer">
            Enable NAT traversal
          </Label>
        </div>

        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="enable-dht"
            bind:checked={settings.enableDHT}
          />
          <Label for="enable-dht" class="cursor-pointer">
            Enable DHT (Distributed Hash Table)
          </Label>
        </div>
      </div>
    </div>
  </Card>

  <!-- Privacy Settings -->
  <Card class="p-6">
    <div class="flex items-center gap-2 mb-4">
      <Shield class="h-5 w-5" />
      <h2 class="text-lg font-semibold">Privacy & Security</h2>
    </div>

    <div class="space-y-2">
      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          id="enable-proxy"
          bind:checked={settings.enableProxy}
        />
        <Label for="enable-proxy" class="cursor-pointer">
          Enable proxy routing for anonymity
        </Label>
      </div>

      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          id="enable-encryption"
          bind:checked={settings.enableEncryption}
        />
        <Label for="enable-encryption" class="cursor-pointer">
          Enable end-to-end encryption
        </Label>
      </div>

      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          id="anonymous-mode"
          bind:checked={settings.anonymousMode}
        />
        <Label for="anonymous-mode" class="cursor-pointer">
          Anonymous mode (hide all identifying information)
        </Label>
      </div>

      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          id="share-analytics"
          bind:checked={settings.shareAnalytics}
        />
        <Label for="share-analytics" class="cursor-pointer">
          Share anonymous usage analytics
        </Label>
      </div>
    </div>
  </Card>

  <!-- Notifications -->
  <Card class="p-6">
    <div class="flex items-center gap-2 mb-4">
      <Bell class="h-5 w-5" />
      <h2 class="text-lg font-semibold">Notifications</h2>
    </div>

    <div class="space-y-2">
      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          id="enable-notifications"
          bind:checked={settings.enableNotifications}
        />
        <Label for="enable-notifications" class="cursor-pointer">
          Enable desktop notifications
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
              Notify when downloads complete
            </Label>
          </div>

          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="notify-error"
              bind:checked={settings.notifyOnError}
            />
            <Label for="notify-error" class="cursor-pointer">
              Notify on errors
            </Label>
          </div>

          <div class="flex items-center gap-2">
            <input
              type="checkbox"
              id="sound-alerts"
              bind:checked={settings.soundAlerts}
            />
            <Label for="sound-alerts" class="cursor-pointer">
              Play sound alerts
            </Label>
          </div>
        </div>
      {/if}
    </div>
  </Card>

  <!-- Advanced Settings -->
  <Card class="p-6">
    <div class="flex items-center gap-2 mb-4">
      <Database class="h-5 w-5" />
      <h2 class="text-lg font-semibold">Advanced</h2>
    </div>

    <div class="space-y-4">
      <div class="grid grid-cols-2 gap-4">
        <div>
          <Label for="chunk-size">Chunk Size (KB)</Label>
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
          <Label for="cache-size">Cache Size (MB)</Label>
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
        <Label for="log-level">Log Level</Label>
        <select
          id="log-level"
          bind:value={settings.logLevel}
          class="w-full mt-2 px-3 py-2 border rounded-lg bg-background appearance-none"
        >
          <option value="error">Error</option>
          <option value="warn">Warning</option>
          <option value="info">Info</option>
          <option value="debug">Debug</option>
        </select>
        <ChevronsUpDown
          class="pointer-events-none absolute right-2 mt-4 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
        />
      </div>

      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          id="auto-update"
          bind:checked={settings.autoUpdate}
        />
        <Label for="auto-update" class="cursor-pointer">
          Automatically install updates
        </Label>
      </div>

      <div class="flex flex-wrap gap-2">
        <Button variant="outline" size="xs" on:click={clearCache}>
          <RefreshCw class="h-4 w-4 mr-2" />
          Clear Cache
        </Button>

        <Button variant="outline" size="xs" on:click={exportSettings}>
          Export Settings
        </Button>

        <label for="import-settings">
          <Button
            variant="outline"
            size="xs"
            on:click={() => fileInputEl?.click()}
          >
            Import Settings
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
    </div>
  </Card>

  <!-- Action Buttons -->
  <div class="flex flex-wrap items-center justify-between gap-2">
    <Button variant="outline" size="xs" on:click={openResetConfirm}>
      Reset to Defaults
    </Button>

    <div class="flex gap-2">
      <Button
        variant="outline"
        size="xs"
        on:click={() => (settings = { ...savedSettings })}
        disabled={!hasChanges}
      >
        Cancel
      </Button>
      <Button size="xs" on:click={saveSettings} disabled={!hasChanges || !isValid}>
        <Save class="h-4 w-4 mr-2" />
        Save Settings
      </Button>
    </div>
  </div>
</div>
{#if showResetConfirmModal}
  <div
    class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4"
    role="button"
    tabindex="0"
    on:click={() => showResetConfirmModal = false}
    on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') showResetConfirmModal = false; }}
  >
    <div
      class="bg-white p-6 rounded-lg shadow-xl w-full max-w-sm"
      role="dialog"
      tabindex="0"
      aria-modal="true"
      on:click|stopPropagation
      on:keydown={(e) => {
        if (e.key === 'Escape') showResetConfirmModal = false;
      }}
    >
      <h3 class="text-lg font-bold mb-2">Are you sure?</h3>
      <p class="text-sm text-gray-600 mb-6">
        All settings will be reset to their default values. This action will be saved immediately.
      </p>
      <div class="flex justify-end gap-3">
        <Button
          variant="outline"
          on:click={() => showResetConfirmModal = false}
        >
          Cancel
        </Button>
        <Button
          variant="destructive"
          on:click={handleConfirmReset}
        >
          Confirm Reset
        </Button>
      </div>
    </div>
  </div>
{/if}