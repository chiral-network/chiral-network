<script lang="ts">
    import './styles/globals.css'
    import { Upload, Download, Wallet, Globe, BarChart3, Settings, Cpu, Menu, X, Star, Server, Database } from 'lucide-svelte'
    import UploadPage from './pages/Upload.svelte'
    import DownloadPage from './pages/Download.svelte'
    // import ProxyPage from './pages/Proxy.svelte' // DISABLED
    import AccountPage from './pages/Account.svelte'
    import NetworkPage from './pages/Network.svelte'
    import AnalyticsPage from './pages/Analytics.svelte'
    // import TorrentDownloadPage from './pages/TorrentDownload.svelte' // INTEGRATED INTO DOWNLOAD/UPLOAD PAGES
    import SettingsPage from './pages/Settings.svelte'
    import MiningPage from './pages/Mining.svelte'
    import ReputationPage from './pages/Reputation.svelte'
    import RelayPage from './pages/Relay.svelte'
    import BlockchainDashboard from './pages/BlockchainDashboard.svelte'
    import NotFound from './pages/NotFound.svelte'
    // import ProxySelfTest from './routes/proxy-self-test.svelte' // DISABLED
import { networkStatus, settings, userLocation, wallet, activeBandwidthLimits, etcAccount } from './lib/stores'
import type { AppSettings, ActiveBandwidthLimits } from './lib/stores'
    import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
    import {onMount, setContext} from 'svelte';
    import { tick } from 'svelte';
    import { get } from 'svelte/store';
    import { setupI18n } from './i18n/i18n';
    import { t } from 'svelte-i18n';
    import SimpleToast from './lib/components/SimpleToast.svelte';
    import FirstRunWizard from './lib/components/wallet/FirstRunWizard.svelte';
    import { startNetworkMonitoring } from './lib/services/networkService';
    import { startGethMonitoring, gethStatus } from './lib/services/gethService';
    import { fileService } from '$lib/services/fileService';
    import { bandwidthScheduler } from '$lib/services/bandwidthScheduler';
    import { detectUserRegion } from '$lib/services/geolocation';
    import { paymentService } from '$lib/services/paymentService';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { exit } from '@tauri-apps/plugin-process';
    // gets path name not entire url:
    // ex: http://locatlhost:1420/download -> /download
    
    // get path name based on current url
    // if no path name, default to 'download'
    const getPathName = (pathname: string) => {
      const p = pathname.replace(/^\/+/, ''); // remove leading '/'
      return p ? p.split('/')[0] : 'download'; // get first path name
    };
    
    // makes currentPage var to be up-to-date to current page
    function syncFromUrl() {
      currentPage = getPathName(window.location.pathname);
    }
    
let currentPage = getPathName(window.location.pathname);
let loading = true;
let schedulerRunning = false;
let unsubscribeScheduler: (() => void) | null = null;
let unsubscribeBandwidth: (() => void) | null = null;
let lastAppliedBandwidthSignature: string | null = null;
let showFirstRunWizard = false;

  const syncBandwidthScheduler = (config: AppSettings) => {
    const enabledSchedules =
      config.bandwidthSchedules?.filter((entry) => entry.enabled) ?? [];
    const shouldRun =
      config.enableBandwidthScheduling && enabledSchedules.length > 0;

    if (shouldRun) {
      if (!schedulerRunning) {
        bandwidthScheduler.start();
        schedulerRunning = true;
      }
      bandwidthScheduler.forceUpdate();
      return;
    }

    if (schedulerRunning) {
      bandwidthScheduler.stop();
      schedulerRunning = false;
    } else {
      // Ensure limits reflect the defaults when scheduler is idle.
      bandwidthScheduler.forceUpdate();
    }
  };

  const pushBandwidthLimits = (limits: ActiveBandwidthLimits) => {
    const uploadKbps = Math.max(0, Math.floor(limits.uploadLimitKbps || 0));
    const downloadKbps = Math.max(0, Math.floor(limits.downloadLimitKbps || 0));
    const signature = `${uploadKbps}:${downloadKbps}`;

    if (signature === lastAppliedBandwidthSignature) {
      return;
    }

    lastAppliedBandwidthSignature = signature;

    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }

  invoke("set_bandwidth_limits", {
    uploadKbps,
    downloadKbps,
  }).catch((error) => {
    console.error("Failed to apply bandwidth limits:", error);
  });
};

// First-run wizard handlers
function handleFirstRunComplete() {
  showFirstRunWizard = false;
  // Navigate to account page after completing wizard
  currentPage = 'account';
  goto('/account');
}

  onMount(() => {
    let stopNetworkMonitoring: () => void = () => {};
    let stopGethMonitoring: () => void = () => {};
    let unlistenSeederPayment: (() => void) | null = null;

    unsubscribeScheduler = settings.subscribe(syncBandwidthScheduler);
    syncBandwidthScheduler(get(settings));
    unsubscribeBandwidth = activeBandwidthLimits.subscribe(pushBandwidthLimits);
    pushBandwidthLimits(get(activeBandwidthLimits));

    (async () => {
      // Initialize payment service to load wallet and transactions
      await paymentService.initialize();

      // Listen for payment notifications from backend
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        try {
          const unlisten = await listen(
            "seeder_payment_received",
            async (event: any) => {
              const payload = event.payload;
              console.log("💰 Seeder payment notification received:", payload);

              // Only credit the payment if we are the seeder (not the downloader)
              const currentWalletAddress = get(wallet).address;
              const seederAddress = payload.seeder_wallet_address;

              if (!seederAddress || !currentWalletAddress) {
                console.warn(
                  "⚠️ Missing wallet addresses, skipping payment credit",
                );
                return;
              }

              // Check if this payment is meant for us (we are the seeder)
              if (
                currentWalletAddress.toLowerCase() !==
                seederAddress.toLowerCase()
              ) {
                console.log(
                  `⏭️ Skipping payment credit - not for us. Seeder: ${seederAddress}, Us: ${currentWalletAddress}`,
                );
                return;
              }

              console.log("✅ This payment is for us! Crediting...");

              // Credit the seeder's wallet
              const result = await paymentService.creditSeederPayment(
                payload.file_hash,
                payload.file_name,
                payload.file_size,
                payload.downloader_address,
                payload.transaction_hash,
              );

              if (result.success) {
                console.log("✅ Seeder payment credited successfully");
              } else {
                console.error(
                  "❌ Failed to credit seeder payment:",
                  result.error,
                );
              }
            },
          );
          unlistenSeederPayment = unlisten;
        } catch (error) {
          console.error("Failed to setup payment listener:", error);
        }
      }

        // setup i18n
        await setupI18n();

        // Check for first-run and show wizard if no account exists
        // DO THIS BEFORE setting loading = false to prevent race conditions
        try {
          // Check backend for active account
          let hasAccount = false;
          if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
            try {
              hasAccount = await invoke<boolean>('has_active_account');
              
              // If backend has account, restore it to frontend
              if (hasAccount) {
                try {
                  const address = await invoke<string>('get_active_account_address');
                  
                  // Import wallet service to prevent sync during restoration
                  const { walletService } = await import('./lib/wallet');
                  walletService.setRestoringAccount(true);
                  
                  // Fetch private key from backend to restore it to the frontend store
                  let privateKey = '';
                  try {
                    privateKey = await invoke<string>('get_active_account_private_key');
                  } catch (error) {
                    console.warn('Failed to get private key from backend:', error);
                  }
                  
                  // Restore account with private key
                  etcAccount.set({ address, private_key: privateKey });
                  
                  // Update wallet with address
                  wallet.update(w => ({ 
                    ...w, 
                    address
                  }));
                  
                  // Re-enable syncing and trigger a sync
                  walletService.setRestoringAccount(false);
                  
                  // Now sync from blockchain
                  await walletService.refreshTransactions();
                  await walletService.refreshBalance();
                } catch (error) {
                  console.error('Failed to restore account from backend:', error);
                }
              }
            } catch (error) {
              console.warn('Failed to check account status:', error);
            }
          } else {
            // For web/demo mode, check frontend store
            hasAccount = get(etcAccount) !== null;
          }

          // Check if there are any keystore files (Tauri only)
          let hasKeystoreFiles = false;
          if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
            try {
              const keystoreFiles = await invoke<string[]>('list_keystore_accounts');
              hasKeystoreFiles = keystoreFiles && keystoreFiles.length > 0;
            } catch (error) {
              console.warn('Failed to check keystore files:', error);
            }
          }

          // Show wizard if no account AND no keystore files exist
          // (Don't rely on first-run flag since user may have cleared data)
          if (!hasAccount && !hasKeystoreFiles) {
            showFirstRunWizard = true;
          }
        } catch (error) {
          console.warn('Failed to check first-run status:', error);
        }

        // Set loading to false AFTER wizard check to prevent race conditions
        loading = false;

      let storedLocation: string | null = null;
      try {
        const storedSettings = localStorage.getItem("chiralSettings");
        if (storedSettings) {
          const parsed = JSON.parse(storedSettings);
          if (typeof parsed?.userLocation === "string" && parsed.userLocation) {
            storedLocation = parsed.userLocation;
            userLocation.set(parsed.userLocation);
          }
        }
      } catch (error) {
        console.warn("Failed to load stored user location:", error);
      }
      try {
        const currentLocation = get(userLocation);
        const shouldAutoDetect =
          !storedLocation || currentLocation === "US-East";

        if (shouldAutoDetect) {
          const detection = await detectUserRegion();
          const detectedLocation = detection.region.label;
          if (detectedLocation && detectedLocation !== currentLocation) {
            userLocation.set(detectedLocation);
            settings.update((previous) => {
              const next = { ...previous, userLocation: detectedLocation };
              try {
                const storedSettings = localStorage.getItem("chiralSettings");
                if (storedSettings) {
                  const parsed = JSON.parse(storedSettings) ?? {};
                  parsed.userLocation = detectedLocation;
                  localStorage.setItem(
                    "chiralSettings",
                    JSON.stringify(parsed),
                  );
                } else {
                  localStorage.setItem("chiralSettings", JSON.stringify(next));
                }
              } catch (storageError) {
                console.warn(
                  "Failed to persist detected location:",
                  storageError,
                );
              }
              console.log(
                "User region detected via ${detection.source}: ${detectedLocation}",
              );
              return next;
            });
          }
        }
      } catch (error) {
        console.warn("Automatic location detection failed:", error);
      }
      // Initialize backend services (File Transfer, DHT - conditionally)
      try {
        const currentSettings = get(settings);
        if (currentSettings.autoStartDHT) {
          await fileService.initializeServices();
        } else {
          // Only start file transfer service, not DHT
          await invoke("start_file_transfer_service");
        }
      } catch (error) {
        // Ignore "already running" errors - this is normal during hot reload
        // Silently skip all errors since services may already be initialized
      }

      // set the currentPage var
      syncFromUrl();

      // Start network monitoring
      stopNetworkMonitoring = startNetworkMonitoring();

      // Start Geth monitoring
      stopGethMonitoring = startGethMonitoring();
    })();

      // popstate - event that tracks history of current tab
      const onPop = () => syncFromUrl();
      window.addEventListener('popstate', onPop);



    // keyboard shortcuts
    const handleKeyDown = (event: KeyboardEvent) => {
      // Ctrl/Cmd + Q - Quit application
      if ((event.ctrlKey || event.metaKey) && event.key === "q") {
        event.preventDefault();
        exit(0);
        return;
      }

      // Ctrl/Cmd + , - Open Settings
      if ((event.ctrlKey || event.metaKey) && event.key === ",") {
        event.preventDefault();
        currentPage = "settings";
        goto("/settings");
        return;
      }

      // Ctrl/Cmd + R - Refresh current page
      if ((event.ctrlKey || event.metaKey) && event.key === "r") {
        event.preventDefault();
        window.location.reload();
        return;
      }

      // F5 - Reload application
      if (event.key === "F5") {
        event.preventDefault();
        window.location.reload();
        return;
      }

      // F11 - Toggle fullscreen (desktop)
      if (event.key === "F11") {
        event.preventDefault();
        if (document.fullscreenElement) {
          document.exitFullscreen();
        } else {
          document.documentElement.requestFullscreen();
        }
        return;
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    // cleanup
    return () => {
      window.removeEventListener("popstate", onPop);
      window.removeEventListener("keydown", handleKeyDown);
      stopNetworkMonitoring();
      stopGethMonitoring();
      if (schedulerRunning) {
        bandwidthScheduler.stop();
        schedulerRunning = false;
      } else {
        bandwidthScheduler.forceUpdate();
      }
      if (unlistenSeederPayment) {
        unlistenSeederPayment();
      }
      if (unsubscribeScheduler) {
        unsubscribeScheduler();
        unsubscribeScheduler = null;
      }
      if (unsubscribeBandwidth) {
        unsubscribeBandwidth();
        unsubscribeBandwidth = null;
      }
      lastAppliedBandwidthSignature = null;
    };
  });

  setContext("navigation", {
    setCurrentPage: (page: string) => {
      currentPage = page;
    },
  });

  let sidebarCollapsed = false;
  let sidebarMenuOpen = false;

  // Scroll to top when page changes
  $: if (currentPage) {
    tick().then(() => {
      const mainContent = document.querySelector("#main-content");
      if (mainContent) {
        mainContent.scrollTop = 0;
      }
    });
  }

  type MenuItem = {
    id: string;
    label: string;
    icon: typeof Upload;
  };

  let menuItems: MenuItem[] = [];
  $: if (!loading) {
    menuItems = [
      { id: "download", label: $t("nav.download"), icon: Download },
      { id: "upload", label: $t("nav.upload"), icon: Upload },
      { id: "network", label: $t("nav.network"), icon: Globe },
      { id: "mining", label: $t("nav.mining"), icon: Cpu },
      { id: "relay", label: $t("nav.relay"), icon: Server },
      // { id: 'proxy', label: $t('nav.proxy'), icon: Shield }, // DISABLED
      { id: "analytics", label: $t("nav.analytics"), icon: BarChart3 },
      { id: "reputation", label: $t("nav.reputation"), icon: Star },
      { id: "blockchain", label: $t("nav.blockchain"), icon: Database },
      { id: "account", label: $t("nav.account"), icon: Wallet },
      { id: "settings", label: $t("nav.settings"), icon: Settings },

      // DISABLED: Proxy self-test page
      // ...(import.meta.env.DEV ? [{ id: 'proxy-self-test', label: 'Proxy Self-Test', icon: Shield }] : [])
    ];
  }

  // routes to be used:
  const routes: RouteConfig[] = [
    {
      component: DownloadPage, // root path: '/'
    },
    {
      path: "download",
      component: DownloadPage,
    },
    {
      path: "upload",
      component: UploadPage,
    },
    {
      path: "network",
      component: NetworkPage,
    },
    {
      path: "relay",
      component: RelayPage,
    },
    {
      path: "mining",
      component: MiningPage,
    },
    // DISABLED: Proxy page
    // {
    //   path: "proxy",
    //   component: ProxyPage
    // },
    {
      path: "analytics",
      component: AnalyticsPage,
    },
    {
      path: "reputation",
      component: ReputationPage,
    },
    {
      path: "blockchain",
      component: BlockchainDashboard,
    },
    {
      path: "account",
      component: AccountPage,
    },
    {
      path: "settings",
      component: SettingsPage,
    },
    // DISABLED: Proxy self-test page
    // {
    //   path: "proxy-self-test",
    //   component: ProxySelfTest
    // },
  ];
</script>

<div class="flex bg-background h-full">
  {#if !loading}
    <!-- Desktop Sidebar -->
    <!-- Make the sidebar sticky so it stays visible while the main content scrolls -->
    <div
      class="hidden md:block {sidebarCollapsed
        ? 'w-16'
        : 'w-64'} bg-card border-r transition-all sticky top-0 h-screen"
    >
      <nav class="p-2 space-y-2 h-full overflow-y-auto">
        <!-- Sidebar Header (desktop only) -->
        <div class="flex items-center justify-between px-2 py-2 mb-2">
          <div class="flex items-center">
            <button
              aria-label={$t(
                sidebarCollapsed ? "nav.expandSidebar" : "nav.collapseSidebar",
              )}
              class="p-2 rounded transition-colors hover:bg-gray-100"
              on:click={() => (sidebarCollapsed = !sidebarCollapsed)}
            >
              <Menu class="h-5 w-5" />
            </button>
            {#if !sidebarCollapsed}
              <span class="ml-2 font-bold text-base">{$t("nav.menu")}</span>
            {/if}
          </div>

          {#if !sidebarCollapsed}
            <div class="flex items-center gap-2 text-xs">
              <div
                class="w-2 h-2 rounded-full {$networkStatus === 'connected'
                  ? 'bg-green-500'
                  : 'bg-red-500'}"
              ></div>
              <span class="text-muted-foreground"
                >{$networkStatus === "connected"
                  ? $t("nav.connected")
                  : $t("nav.disconnected")}</span
              >
            </div>
          {:else}
            <div
              class="w-2 h-2 rounded-full {$networkStatus === 'connected'
                ? 'bg-green-500'
                : 'bg-red-500'}"
            ></div>
          {/if}
        </div>

        <!-- Sidebar Nav Items -->
        {#each menuItems as item}
          {@const isBlockchainDisabled = item.id === 'blockchain' && $gethStatus !== 'running'}
          <button
            on:click={() => {
              if (isBlockchainDisabled) return;
              currentPage = item.id;
              goto(`/${item.id}`);
            }}
            class="w-full group {isBlockchainDisabled ? 'cursor-not-allowed opacity-50' : ''}"
            aria-current={currentPage === item.id ? "page" : undefined}
            disabled={isBlockchainDisabled}
            title={isBlockchainDisabled ? $t('nav.blockchainUnavailable') + ' ' + $t('nav.networkPageLink') : ''}
          >
            <div
              class="flex items-center {sidebarCollapsed
                ? 'justify-center'
                : ''} rounded {currentPage === item.id
                ? 'bg-gray-200'
                : isBlockchainDisabled ? '' : 'group-hover:bg-gray-100'}"
            >
              <span
                class="flex items-center justify-center rounded w-10 h-10 relative"
              >
                <svelte:component this={item.icon} class="h-5 w-5" />
                {#if sidebarCollapsed}
                  <span
                    class="tooltip absolute left-full ml-2 top-1/2 -translate-y-1/2 hidden whitespace-nowrap rounded bg-black text-white text-xs px-2 py-1 z-50"
                    >{item.label}</span
                  >
                {/if}
              </span>
              {#if !sidebarCollapsed}
                <span class="flex-1 px-2 py-1 text-left">{item.label}</span>
              {/if}
            </div>
          </button>
        {/each}
      </nav>
    </div>

    <!-- Sidebar Menu Button -->
    <div class="absolute top-2 right-2 md:hidden">
      <button
        class="p-2 rounded bg-card shadow"
        on:click={() => (sidebarMenuOpen = true)}
      >
        <Menu class="h-6 w-6" />
      </button>
    </div>

    <!-- Sidebar Menu Overlay -->
    {#if sidebarMenuOpen}
      <!-- Backdrop -->
      <div
        class="fixed inset-0 bg-black bg-opacity-50 z-40 md:hidden"
        role="button"
        tabindex="0"
        aria-label={$t("nav.closeSidebarMenu")}
        on:click={() => (sidebarMenuOpen = false)}
        on:keydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            sidebarMenuOpen = false;
          }
        }}
      ></div>

      <!-- Sidebar -->
      <div
        class="fixed top-0 right-0 h-full w-64 bg-white z-50 flex flex-col md:hidden"
      >
        <!-- Sidebar Header -->
        <div class="flex justify-between items-center p-4 border-b">
          <!-- Left side -->
          <span class="font-bold text-base">{$t("nav.menu")}</span>

          <!-- Right side -->
          <div class="flex items-center gap-3">
            <div class="flex items-center gap-2">
              <div
                class="w-2 h-2 rounded-full {$networkStatus === 'connected'
                  ? 'bg-green-500'
                  : 'bg-red-500'}"
              ></div>
              <span class="text-muted-foreground text-sm"
                >{$networkStatus === "connected"
                  ? $t("nav.connected")
                  : $t("nav.disconnected")}</span
              >
            </div>
            <button on:click={() => (sidebarMenuOpen = false)}>
              <X class="h-6 w-6" />
            </button>
          </div>
        </div>

        <!-- Sidebar Nav Items -->
        <nav class="flex-1 p-4 space-y-2">
          {#each menuItems as item}
            {@const isBlockchainDisabled = item.id === 'blockchain' && $gethStatus !== 'running'}
            <button
              on:click={() => {
                if (isBlockchainDisabled) return;
                currentPage = item.id;
                goto(`/${item.id}`);
                sidebarMenuOpen = false;
              }}
              class="w-full flex items-center rounded px-4 py-3 text-lg {isBlockchainDisabled ? 'cursor-not-allowed opacity-50' : 'hover:bg-gray-100'}"
              aria-current={currentPage === item.id ? "page" : undefined}
              disabled={isBlockchainDisabled}
              title={isBlockchainDisabled ? $t('nav.blockchainUnavailable') + ' ' + $t('nav.networkPageLink') : ''}
            >
              <svelte:component this={item.icon} class="h-5 w-5 mr-3" />
              {item.label}
            </button>
          {/each}
        </nav>
      </div>
    {/if}
  {/if}

  <!-- Main Content -->
  <!-- Ensure main content doesn't go under the sticky sidebar -->
  <div id="main-content" class="flex-1 overflow-y-auto">
    <div class="p-6">
      <!-- <Router {routes} /> -->

      {#if !loading}
        <Router
          {routes}
          statuses={{
            // visiting non-path default to NotFound page
            404: () => ({
              component: NotFound,
            }),
          }}
        />
        {/if}
      </div>
    </div>
  </div>

<!-- First Run Wizard -->
{#if showFirstRunWizard}
  <FirstRunWizard
    onComplete={handleFirstRunComplete}
  />
{/if}

  <!-- add Toast  -->
<SimpleToast />
