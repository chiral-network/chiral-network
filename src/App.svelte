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
    import Blockchain from './pages/Blockchain.svelte'
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
    import KeyboardShortcutsPanel from './lib/components/KeyboardShortcutsPanel.svelte';
    import CommandPalette from './lib/components/CommandPalette.svelte';
import { startNetworkMonitoring } from './lib/services/networkService';
import { startGethMonitoring, gethStatus } from './lib/services/gethService';
    import { bandwidthScheduler } from '$lib/services/bandwidthScheduler';
    import { detectUserRegion } from '$lib/services/geolocation';
import { paymentService } from '$lib/services/paymentService';
import { subscribeToTransferEvents, transferStore, unsubscribeFromTransferEvents } from '$lib/stores/transferEventsStore';
    import { showToast } from '$lib/toast';
import { walletService } from '$lib/wallet';
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
let showShortcutsPanel = false;
let showCommandPalette = false;
let transferStoreUnsubscribe: (() => void) | null = null;
const notifiedCompletedTransfers = new Set<string>();
const scrollPositions: Record<string, number> = {};

// Helper to get the main scroll container (if present)
const getMainContent = () =>
  document.querySelector("#main-content") as HTMLElement | null;

// Save scroll position for the current page
const saveScrollPosition = (page: string) => {
  if (!page || typeof window === 'undefined') return;

  const mainContent = getMainContent();

  if (mainContent && mainContent.scrollHeight > mainContent.clientHeight) {
    // App is using the #main-content div as scroll container
    scrollPositions[page] = mainContent.scrollTop;
  } else {
    // Fallback to window scroll (body/document scrolling)
    scrollPositions[page] = window.scrollY || window.pageYOffset || 0;
  }
};

// Restore scroll position for a page
const restoreScrollPosition = async (page: string) => {
  if (!page || typeof window === 'undefined') return;

  await tick();

  const y = scrollPositions[page] ?? 0;
  const mainContent = getMainContent();

  if (mainContent && mainContent.scrollHeight > mainContent.clientHeight) {
    mainContent.scrollTop = y;
  } else {
    window.scrollTo(0, y);
  }
};


const navigateTo = (page: string, path: string) => {
  if (page !== currentPage) {
    saveScrollPosition(currentPage);
  }
  currentPage = page;
  goto(path);
};

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
  navigateTo('account', '/account');
}


  onMount(() => {
    let stopNetworkMonitoring: () => void = () => {};
    let stopGethMonitoring: () => void = () => {};
    let unlistenSeederPayment: (() => void) | null = null;
    let unlistenTorrentPayment: (() => void) | null = null;
    let transferEventsUnsubscribe: (() => void) | null = null;
    let unsubscribeGethStatus: (() => void) | null = null;

    unsubscribeScheduler = settings.subscribe(syncBandwidthScheduler);
    syncBandwidthScheduler(get(settings));
    unsubscribeBandwidth = activeBandwidthLimits.subscribe(pushBandwidthLimits);
    pushBandwidthLimits(get(activeBandwidthLimits));

    (async () => {
      // Subscribe to transfer events from backend
      try {
        transferEventsUnsubscribe = await subscribeToTransferEvents();
        transferStoreUnsubscribe = transferStore.subscribe(($store) => {

          if (!$store || !$store.transfers) {
            return;
          }

          for (const [transferId, transfer] of $store.transfers.entries()) {
            if (transfer.status === 'completed') {
              // First time we see this transfer as completed → fire toast
              if (!notifiedCompletedTransfers.has(transferId)) {
                notifiedCompletedTransfers.add(transferId);

                const fileName = transfer.fileName ?? 'file';
                const message = `Download complete: "${fileName}"`;

                showToast(message, 'success');
              }
            } else {
              // If the transfer goes back to a non-completed status (e.g. retry),
              // allow a later completion to trigger a new toast
              notifiedCompletedTransfers.delete(transferId);
            }
          }
        });
      } catch (error) {
        console.warn('Failed to subscribe to transfer events:', error);
      }

      // Initialize payment service to load wallet and transactions
      await paymentService.initialize();
      // Initialize wallet service early so imports/polls populate balance/tx history
      await walletService.initialize();

      // When geth starts running, immediately sync wallet state (balance + txs)
      unsubscribeGethStatus = gethStatus.subscribe(async (status) => {
        if (status === 'running') {
          try {
            const hasAccount = await invoke<boolean>('has_active_account');
            if (hasAccount) {
              await walletService.refreshTransactions();
              await walletService.refreshBalance();
              walletService.startProgressiveLoading();
            }
          } catch (err) {
            console.warn('Failed to sync wallet after geth start:', err);
          }
        }
      });

      // Listen for payment notifications from backend
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        try {
          // Listener for BitTorrent protocol payments
          const torrentUnlisten = await listen(
            "torrent_seeder_payment_received",
            async (event: any) => {
              const payload = event.payload;
              console.log(
                "💰 Torrent seeder payment notification received:",
                payload,
              );

              const currentWalletAddress = get(wallet).address;
              const seederAddress = payload.seeder_wallet_address;

              if (
                !seederAddress ||
                !currentWalletAddress ||
                currentWalletAddress.toLowerCase() !==
                  seederAddress.toLowerCase()
              ) {
                console.log("⏭️ Skipping torrent payment credit - not for us.");
                return;
              }

              console.log(
                "✅ This torrent payment is for us! Crediting...",
              );

              const result = await paymentService.creditSeederPayment(
                payload.info_hash, // For torrents, this would be the info_hash
                payload.file_name,
                payload.file_size,
                payload.downloader_address,
                payload.transaction_hash,
              );

              if (!result.success) {
                console.error(
                  "❌ Failed to credit torrent seeder payment:",
                  result.error,
                );
              }
            },
          );
          unlistenTorrentPayment = torrentUnlisten;

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
                payload.downloader_peer_id || payload.downloader_address, // Use peer ID or fallback to address
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
                  walletService.startProgressiveLoading();
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
      
      // Load settings from localStorage before auto-starting services
      try {
        const storedSettings = localStorage.getItem("chiralSettings");
        if (storedSettings) {
          const parsed = JSON.parse(storedSettings);
          // Ensure selectedProtocol always has a valid default
          if (!parsed.selectedProtocol) {
            parsed.selectedProtocol = "Bitswap";
          }
          settings.update(prev => ({ ...prev, ...parsed }));
        }
      } catch (error) {
        console.warn("Failed to load settings from localStorage:", error);
      }
      
      // Initialize backend services (DHT first - it initializes chunk manager, then File Transfer)
      try {
        const currentSettings = get(settings);
        
        // Check if DHT is already running first (to avoid duplicate start attempts)
        let isDhtAlreadyRunning = false;
        try {
          isDhtAlreadyRunning = await invoke<boolean>("is_dht_running");
        } catch (err) {
          // Command might not be available, assume not running
          isDhtAlreadyRunning = false;
        }
        
        // Start DHT first if auto-start is enabled (DHT initializes chunk manager needed by file transfer)
        if (currentSettings.autoStartDHT) {
          if (isDhtAlreadyRunning) {
            // Import dhtService and sync the peer ID
            const { dhtService } = await import("$lib/dht");
            try {
              const peerId = await invoke<string | null>("get_dht_peer_id");
              if (peerId) {
                dhtService.setPeerId(peerId);
              }
            } catch {}
            
            // Update network status
            networkStatus.set('connected');
          } else {
            console.log("🚀 Auto-starting DHT node...");
            
            try {
              // Import dhtService to start DHT with full settings
              const { dhtService } = await import("$lib/dht");
              
              // Get bootstrap nodes - use custom ones if specified, otherwise use defaults
              let bootstrapNodes = currentSettings.customBootstrapNodes || [];
              if (bootstrapNodes.length === 0) {
                bootstrapNodes = await invoke<string[]>("get_bootstrap_nodes_command");
              }
              
              // Start DHT with all user settings (same as Network page does)
              const peerId = await dhtService.start({
                port: currentSettings.port || 4001,
                bootstrapNodes,
                enableAutonat: currentSettings.enableAutonat,
                autonatProbeIntervalSeconds: currentSettings.autonatProbeInterval,
                autonatServers: currentSettings.autonatServers,
                proxyAddress: currentSettings.enableProxy ? currentSettings.proxyAddress : undefined,
                enableAutorelay: currentSettings.enableAutorelay,
                preferredRelays: currentSettings.preferredRelays || [],
                enableRelayServer: currentSettings.enableRelayServer,
                relayServerAlias: currentSettings.relayServerAlias || '',
                chunkSizeKb: currentSettings.chunkSize,
                cacheSizeMb: currentSettings.cacheSize,
                enableUpnp: currentSettings.enableUPnP,
              });
              
              console.log("✅ DHT node auto-started successfully with peer ID:", peerId);
              
              // Update network status
              networkStatus.set('connected');
            } catch (dhtError) {
              const dhtErrorMsg = dhtError instanceof Error ? dhtError.message : String(dhtError);
              if (dhtErrorMsg.includes("already running")) {
                // Race condition - DHT was started between our check and start attempt
                console.log("ℹ️ DHT started by another process (race condition)");
                
                // Sync the peer ID since DHT is running
                const { dhtService } = await import("$lib/dht");
                try {
                  const peerId = await invoke<string | null>("get_dht_peer_id");
                  if (peerId) {
                    dhtService.setPeerId(peerId);
                    console.log("✅ Synced peer ID after race condition:", peerId);
                  }
                } catch {}
                
                networkStatus.set('connected');
              } else {
                // Real error
                console.error("❌ Failed to auto-start DHT:", dhtErrorMsg);
              }
            }
          }
        }
        
        // Start file transfer service AFTER DHT (needs chunk manager initialized by DHT)
        try {
          await invoke("start_file_transfer_service");
        } catch (ftError) {
          const ftErrorMsg = ftError instanceof Error ? ftError.message : String(ftError);
          // Suppress known non-critical warnings
          if (!ftErrorMsg.includes("already running") && 
              !ftErrorMsg.includes("already initialized") &&
              !ftErrorMsg.includes("Chunk manager not initialized")) {
            console.warn("⚠️ File transfer service start warning:", ftErrorMsg);
          }
        }
        
        // Start Geth blockchain node if auto-start is enabled
        if (currentSettings.autoStartGeth && typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
          try {
            // Check if Geth is already running
            const isGethRunning = await invoke<boolean>('is_geth_running').catch(() => false);
            
            if (isGethRunning) {
              // Geth already running
            } else {
              // Check if Geth is installed
              const isGethInstalled = await invoke<boolean>('check_geth_binary').catch(() => false);
              
              if (!isGethInstalled) {
                console.log("📦 Geth not installed. Downloading Geth...");
                showToast("Downloading Geth blockchain node...", "info");
                
                // Listen for download progress
                const unlisten = await listen('geth-download-progress', (event: any) => {
                  const progress = event.payload;
                  if (progress.percentage >= 100) {
                    console.log("✅ Geth download complete");
                  } else {
                    console.log(`⬇️ Downloading Geth: ${progress.percentage.toFixed(1)}%`);
                  }
                });
                
                try {
                  await invoke('download_geth_binary');
                  unlisten();
                  console.log("✅ Geth downloaded successfully");
                  showToast("Geth downloaded successfully", "success");
                } catch (downloadError) {
                  unlisten();
                  const downloadErrorMsg = downloadError instanceof Error ? downloadError.message : String(downloadError);
                  console.error("❌ Failed to download Geth:", downloadErrorMsg);
                  showToast(`Failed to download Geth: ${downloadErrorMsg}`, "error");
                  throw downloadError; // Don't try to start if download failed
                }
              }

              try {
                await invoke('start_geth_node', { dataDir: './bin/geth-data' });
                
                // Update geth status
                const { gethStatus } = await import('./lib/services/gethService');
                gethStatus.set('running');
              } catch (gethError) {
                const gethErrorMsg = gethError instanceof Error ? gethError.message : String(gethError);
                if (gethErrorMsg.includes("already running") || gethErrorMsg.includes("already started")) {
                  console.log("ℹ️ Geth started by another process");
                } else if (gethErrorMsg.includes("not found") || gethErrorMsg.includes("No such file")) {
                  console.log("ℹ️ Geth not downloaded yet. Download it from the Network page.");
                } else {
                  console.error("❌ Failed to auto-start Geth:", gethErrorMsg);
                }
              }
            }
          } catch (error) {
            console.error("⚠️ Error checking/starting Geth:", error);
          }
        }
      } catch (error) {
        // Unexpected error in the initialization block
        console.error("⚠️ Unexpected error during service initialization:", error);
      }

      // set the currentPage var
      syncFromUrl();

      // Start network monitoring
      stopNetworkMonitoring = startNetworkMonitoring();

      // Start Geth monitoring
      stopGethMonitoring = startGethMonitoring();
    })();

      // popstate - event that tracks history of current tab
      // const onPop = () => syncFromUrl();
      // window.addEventListener('popstate', onPop);
      // popstate - event that tracks history of current tab
    const onPop = () => {
      // Save where we were on the page we're leaving
      saveScrollPosition(currentPage);
      // Update currentPage based on URL
      syncFromUrl();
      // restoreScrollPosition will run via the reactive currentPage block
    };
    window.addEventListener('popstate', onPop);


    // keyboard shortcuts
    const handleKeyDown = (event: KeyboardEvent) => {
      // Ignore shortcuts if user is typing in an input/textarea
      const target = event.target as HTMLElement;
      const isInputField = target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable;
      
      // ? or F1 - Show keyboard shortcuts help
      if ((event.key === '?' || event.key === 'F1') && !isInputField) {
        event.preventDefault();
        showShortcutsPanel = true;
        return;
      }
      
      // Ctrl/Cmd + K - Open command palette
      if ((event.ctrlKey || event.metaKey) && event.key === 'k' && !isInputField) {
        event.preventDefault();
        showCommandPalette = true;
        return;
      }
      
      // Ctrl/Cmd + D - Go to Download
      if ((event.ctrlKey || event.metaKey) && event.key === 'd' && !isInputField) {
        event.preventDefault();
        navigateTo('download', '/download');
        return;
      }

      
      // Ctrl/Cmd + U - Go to Upload
      if ((event.ctrlKey || event.metaKey) && event.key === 'u' && !isInputField) {
        event.preventDefault();
        navigateTo('upload', '/upload');
        return;
      }

      
      // Ctrl/Cmd + N - Go to Network
      if ((event.ctrlKey || event.metaKey) && event.key === 'n' && !isInputField) {
        event.preventDefault();
        navigateTo('network', '/network');
        return;
      }

      
      // Ctrl/Cmd + M - Go to Mining
      if ((event.ctrlKey || event.metaKey) && event.key === 'm' && !isInputField) {
        event.preventDefault();
        navigateTo('mining', '/mining');
        return;
      }

      
      // Ctrl/Cmd + A - Go to Account (only if not in input field)
      if ((event.ctrlKey || event.metaKey) && event.key === 'a' && !isInputField) {
        event.preventDefault();
        navigateTo('account', '/account');
        return;
      }


      // Ctrl/Cmd + Q - Quit application
      if ((event.ctrlKey || event.metaKey) && event.key === "q") {
        event.preventDefault();
        exit(0);
        return;
      }

      // Ctrl/Cmd + , - Open Settings
      if ((event.ctrlKey || event.metaKey) && event.key === ",") {
        event.preventDefault();
        navigateTo('settings', '/settings');
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
      if (unsubscribeGethStatus) {
        unsubscribeGethStatus();
      }
      if (schedulerRunning) {
        bandwidthScheduler.stop();
        schedulerRunning = false;
      } else {
        bandwidthScheduler.forceUpdate();
      }
      if (unlistenSeederPayment) {
        unlistenSeederPayment();
      }
      if (unlistenTorrentPayment) {
        unlistenTorrentPayment();
      }
      if (transferEventsUnsubscribe) {
        transferEventsUnsubscribe();
      }
      if (transferStoreUnsubscribe) {
        transferStoreUnsubscribe();
        transferStoreUnsubscribe = null;
      }
      // Also ensure transfer events are fully unsubscribed
      unsubscribeFromTransferEvents();
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
      if (page !== currentPage) {
        saveScrollPosition(currentPage);
      }
      currentPage = page;
    },
    navigateTo,
  });


  let sidebarCollapsed = false;
  let sidebarMenuOpen = false;

  // Restore the previous scroll position when page changes
  $: if (currentPage) {
    restoreScrollPosition(currentPage);
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
      component: Blockchain,
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
          {@const requiresGeth = item.id === 'blockchain' || item.id === 'mining'}
          {@const isBlocked = requiresGeth && $gethStatus !== 'running'}
          <button
            on:click={() => {
              if (isBlocked) return;
              navigateTo(item.id, `/${item.id}`);
            }}
            class="w-full group {isBlocked ? 'cursor-not-allowed opacity-60' : ''}"
            aria-current={currentPage === item.id ? "page" : undefined}
            disabled={isBlocked}
            title={isBlocked ? $t('nav.blockchainUnavailable') + ' ' + $t('nav.networkPageLink') : ''}
          >
            <div
              class="flex items-center {sidebarCollapsed
                ? 'justify-center'
                : ''} rounded {currentPage === item.id
                ? 'bg-gray-200'
                : isBlocked ? '' : 'group-hover:bg-gray-100'}"
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
            {@const requiresGeth = item.id === 'blockchain' || item.id === 'mining'}
            {@const isBlocked = requiresGeth && $gethStatus !== 'running'}
            <button
              on:click={() => {
                if (isBlocked) return;
                navigateTo(item.id, `/${item.id}`);
                sidebarMenuOpen = false;
              }}
              class="w-full flex items-center rounded px-4 py-3 text-lg {isBlocked ? 'cursor-not-allowed opacity-60' : 'hover:bg-gray-100'}"
              aria-current={currentPage === item.id ? "page" : undefined}
              disabled={isBlocked}
              title={isBlocked ? $t('nav.blockchainUnavailable') + ' ' + $t('nav.networkPageLink') : ''}
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

<!-- Keyboard Shortcuts Help Panel -->
<KeyboardShortcutsPanel 
  isOpen={showShortcutsPanel}
  onClose={() => showShortcutsPanel = false}
/>

<!-- Command Palette -->
<CommandPalette 
  isOpen={showCommandPalette}
  onClose={() => showCommandPalette = false}
/>

  <!-- add Toast  -->
<SimpleToast />
