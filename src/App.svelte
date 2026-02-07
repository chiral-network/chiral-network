<script lang="ts">
    import './styles/globals.css'
    import { Upload, Download, Wallet, Globe, BarChart3, Settings, Cpu, Menu, X, Star, Server, Database, LogOut, ChevronDown, Droplet } from 'lucide-svelte'
    import UploadPage from './pages/Upload.svelte'
    import DownloadPage from './pages/Download.svelte'
    import AccountPage from './pages/Account.svelte'
    import NetworkPage from './pages/Network.svelte'
    import AnalyticsPage from './pages/Analytics.svelte'
    import SettingsPage from './pages/Settings.svelte'
    import MiningPage from './pages/Mining.svelte'
    import ReputationPage from './pages/Reputation.svelte'
    import Blockchain from './pages/Blockchain.svelte'
    import ChiralDropPage from './pages/ChiralDrop.svelte'
    import NotFound from './pages/NotFound.svelte'
import { networkStatus, settings, userLocation, wallet, activeBandwidthLimits, etcAccount, showAuthWizard } from './lib/stores'
import type { AppSettings, ActiveBandwidthLimits } from './lib/stores'
    import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
    import {onMount, onDestroy, setContext} from 'svelte';
    import { tick } from 'svelte';
    import { get } from 'svelte/store';
    import { setupI18n } from './i18n/i18n';
    import { t } from 'svelte-i18n';
    import SimpleToast from './lib/components/SimpleToast.svelte';
    import FirstRunWizard from './lib/components/wallet/FirstRunWizard.svelte';
    import KeyboardShortcutsPanel from './lib/components/KeyboardShortcutsPanel.svelte';
import CommandPalette from './lib/components/CommandPalette.svelte';
import ExitPrompt from './lib/components/ExitPrompt.svelte';
import { startNetworkMonitoring } from './lib/services/networkService';
import { startGethMonitoring, gethStatus } from './lib/services/gethService';
    import { bandwidthScheduler } from '$lib/services/bandwidthScheduler';
    import { detectUserRegion } from '$lib/services/geolocation';
import { lockAccount } from '$lib/services/accountLock';
import { paymentService } from '$lib/services/paymentService';
import { subscribeToTransferEvents, transferStore, unsubscribeFromTransferEvents } from '$lib/stores/transferEventsStore';
    import { showToast } from '$lib/toast';
import { walletService } from '$lib/wallet';
import { listen, type Event } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { diagnosticLogger } from './lib/diagnostics/logger';
    // gets path name not entire url:
    // ex: http://locatlhost:1420/download → /download
    
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
let initError: string | null = null;
let schedulerRunning = false;
let unsubscribeScheduler: (() => void) | null = null;
let unsubscribeBandwidth: (() => void) | null = null;
let lastAppliedBandwidthSignature: string | null = null;
let showFirstRunWizard = false;
let showShortcutsPanel = false;
let showCommandPalette = false;
let showExitPrompt = false;
let isExiting = false;
let isLockingAccount = false;
let exitError: string | null = null;
let canShowLockAction = true;
let transferStoreUnsubscribe: (() => void) | null = null;
let unlistenExitPrompt: (() => void) | null = null;
let unsubscribeAuthWizard: (() => void) | null = null;
const notifiedCompletedTransfers = new Set<string>();
const scrollPositions: Record<string, number> = {};

// Event payload types
interface TorrentSeederPaymentPayload {
  seeder_wallet_address: string;
  info_hash: string;
  file_name: string;
  file_size: number;
  downloader_address: string;
  transaction_hash: string;
}

interface SeederPaymentPayload {
  seeder_wallet_address: string;
  file_hash: string;
  file_name: string;
  file_size: number;
  downloader_address: string;
  downloader_peer_id?: string;
  transaction_hash: string;
}

interface GethDownloadProgressPayload {
  percentage: number;
}

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
    const errorMsg = error instanceof Error ? error.message : String(error);
    diagnosticLogger.error('BANDWIDTH', 'Failed to apply bandwidth limits', { error: errorMsg });
  });
};

// First-run wizard handlers
function handleFirstRunComplete() {
  showFirstRunWizard = false;
  showAuthWizard.set(false);
  // Navigate to account page after completing wizard
  navigateTo('account', '/account');
}

function handleStayInApp() {
  exitError = null;
  isExiting = false;
  showExitPrompt = false;
}

async function handleConfirmExit() {
  if (isExiting) return;
  isExiting = true;
  exitError = null;

  try {
    await invoke('confirm_exit');
  } catch (error) {
    const errorMsg = error instanceof Error ? error.message : String(error);
    diagnosticLogger.error('APP', 'Failed to exit app', { error: errorMsg });
    exitError = 'Could not close the app. Please try again.';
    isExiting = false;
  }
}

async function handleLockFromExitPrompt() {
  if (isLockingAccount || isExiting) return;
  isLockingAccount = true;
  exitError = null;

  try {
    await lockAccount();
    showExitPrompt = false;
    isExiting = false;
  } catch (error) {
    const errorMsg = error instanceof Error ? error.message : String(error);
    diagnosticLogger.error('ACCOUNT', 'Failed to lock account', { error: errorMsg });
    exitError = 'Could not lock the account. Please try again.';
  } finally {
    isLockingAccount = false;
  }
}

$: canShowLockAction = !showFirstRunWizard;


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
      // If any init step hangs (e.g., backend invoke), don't leave the UI stuck on the loading screen.
      const loadingSafetyTimer = window.setTimeout(() => {
        if (loading) {
          diagnosticLogger.warn('APP', 'Initialization is taking too long; leaving loading screen to avoid a stuck UI');
          loading = false;
        }
      }, 2500);

      try {
      // Subscribe to transfer events from backend (non-blocking)
      subscribeToTransferEvents()
        .then((unsub) => {
          transferEventsUnsubscribe = unsub;
          transferStoreUnsubscribe = transferStore.subscribe(($store) => {

          if (!$store || !$store.transfers) {
            return;
          }

          for (const [transferId, transfer] of $store.transfers.entries()) {
            if (transfer.status === 'completed') {
              // First time we see this transfer as completed -> fire toast
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
        })
        .catch((error) => {
          const errorMsg = error instanceof Error ? error.message : String(error);
          diagnosticLogger.warn('TRANSFERS', 'Failed to subscribe to transfer events', { error: errorMsg });
        });

      // Initialize services (non-blocking; do not block UI)
      try {
        paymentService.initialize();
      } catch (error) {
        const errorMsg = error instanceof Error ? error.message : String(error);
        diagnosticLogger.warn('PAYMENT', 'Payment service init failed', { error: errorMsg });
      }
      walletService.initialize().catch((error) => {
        const errorMsg = error instanceof Error ? error.message : String(error);
        diagnosticLogger.warn('WALLET', 'Wallet service init failed', { error: errorMsg });
      });

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
            const errorMsg = err instanceof Error ? err.message : String(err);
            diagnosticLogger.warn('WALLET', 'Failed to sync wallet after geth start', { error: errorMsg });
          }
        }
      });

      // Listen for payment notifications from backend (non-blocking)
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        try {
          // Listener for BitTorrent protocol payments
          listen<TorrentSeederPaymentPayload>("torrent_seeder_payment_received", async (event: Event<TorrentSeederPaymentPayload>) => {
              const payload = event.payload;
              diagnosticLogger.info('PAYMENT', 'Torrent seeder payment notification received', { payload });

              const currentWalletAddress = get(wallet).address;
              const seederAddress = payload.seeder_wallet_address;

              if (
                !seederAddress ||
                !currentWalletAddress ||
                currentWalletAddress.toLowerCase() !==
                  seederAddress.toLowerCase()
              ) {
                diagnosticLogger.debug('PAYMENT', 'Skipping torrent payment credit - not for us');
                return;
              }

              diagnosticLogger.info('PAYMENT', 'This torrent payment is for us! Crediting...');

              const result = await paymentService.creditSeederPayment(
                payload.info_hash, // For torrents, this would be the info_hash
                payload.file_name,
                payload.file_size,
                payload.downloader_address,
                payload.transaction_hash,
              );

              if (!result.success) {
                diagnosticLogger.error('PAYMENT', 'Failed to credit torrent seeder payment', { error: result.error });
              }
            })
            .then((unlisten) => {
              unlistenTorrentPayment = unlisten;
            })
            .catch((error) => {
              const errorMsg = error instanceof Error ? error.message : String(error);
              diagnosticLogger.error('PAYMENT', 'Failed to setup torrent payment listener', { error: errorMsg });
            });

          listen<SeederPaymentPayload>("seeder_payment_received", async (event: Event<SeederPaymentPayload>) => {
            const payload = event.payload;
            diagnosticLogger.info('PAYMENT', 'Seeder payment notification received', { payload });

              // Only credit the payment if we are the seeder (not the downloader)
              const currentWalletAddress = get(wallet).address;
              const seederAddress = payload.seeder_wallet_address;

              if (!seederAddress || !currentWalletAddress) {
                diagnosticLogger.warn('PAYMENT', 'Missing wallet addresses, skipping payment credit');
                return;
              }

              // Check if this payment is meant for us (we are the seeder)
              if (
                currentWalletAddress.toLowerCase() !==
                seederAddress.toLowerCase()
              ) {
                diagnosticLogger.debug('PAYMENT', 'Skipping payment credit - not for us', { seederAddress, currentWalletAddress });
                return;
              }

              diagnosticLogger.info('PAYMENT', 'This payment is for us! Crediting...');

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
                diagnosticLogger.info('PAYMENT', 'Seeder payment credited successfully');
              } else {
                diagnosticLogger.error('PAYMENT', 'Failed to credit seeder payment', { error: result.error });
              }
            })
            .then((unlisten) => {
              unlistenSeederPayment = unlisten;
            })
            .catch((error) => {
              const errorMsg = error instanceof Error ? error.message : String(error);
              diagnosticLogger.error('PAYMENT', 'Failed to setup payment listener', { error: errorMsg });
            });
        } catch (error) {
          const errorMsg = error instanceof Error ? error.message : String(error);
          diagnosticLogger.error('PAYMENT', 'Failed to setup payment listener', { error: errorMsg });
        }

        try {
          unlistenExitPrompt = await listen('show_exit_prompt', () => {
            exitError = null;
            isExiting = false;
            showExitPrompt = true;
          });
        } catch (error) {
          const errorMsg = error instanceof Error ? error.message : String(error);
          diagnosticLogger.error('APP', 'Failed to set up exit prompt listener', { error: errorMsg });
        }
      }

      unsubscribeAuthWizard = showAuthWizard.subscribe((visible) => {
        showFirstRunWizard = visible;
      });

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
                    const errorMsg = error instanceof Error ? error.message : String(error);
                    diagnosticLogger.warn('ACCOUNT', 'Failed to get private key from backend', { error: errorMsg });
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
                  const errorMsg = error instanceof Error ? error.message : String(error);
                  diagnosticLogger.error('ACCOUNT', 'Failed to restore account from backend', { error: errorMsg });
                }
              }
            } catch (error) {
              const errorMsg = error instanceof Error ? error.message : String(error);
              diagnosticLogger.warn('ACCOUNT', 'Failed to check account status', { error: errorMsg });
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
              const errorMsg = error instanceof Error ? error.message : String(error);
              diagnosticLogger.warn('ACCOUNT', 'Failed to check keystore files', { error: errorMsg });
            }
          }

          // Show wizard if no account AND no keystore files exist
          // (Don't rely on first-run flag since user may have cleared data)
          if (!hasAccount && !hasKeystoreFiles) {
            showAuthWizard.set(true);
          }
        } catch (error) {
          const errorMsg = error instanceof Error ? error.message : String(error);
          diagnosticLogger.warn('APP', 'Failed to check first-run status', { error: errorMsg });
        }

        // Set loading to false AFTER wizard check to prevent race conditions
        loading = false;
      } catch (error) {
        const errorMsg = error instanceof Error ? error.message : String(error);
        diagnosticLogger.error('APP', 'App initialization failed', { error: errorMsg });
        initError = error instanceof Error ? error.message : String(error);
      } finally {
        window.clearTimeout(loadingSafetyTimer);
        // Never leave the app in a "blank screen" state.
        loading = false;
      }

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
        const errorMsg = error instanceof Error ? error.message : String(error);
        diagnosticLogger.warn('SETTINGS', 'Failed to load stored user location', { error: errorMsg });
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
                const errorMsg = storageError instanceof Error ? storageError.message : String(storageError);
                diagnosticLogger.warn('SETTINGS', 'Failed to persist detected location', { error: errorMsg });
              }
              diagnosticLogger.info('GEOLOCATION', `User region detected via ${detection.source}: ${detectedLocation}`);
              return next;
            });
          }
        }
      } catch (error) {
        const errorMsg = error instanceof Error ? error.message : String(error);
        diagnosticLogger.warn('GEOLOCATION', 'Automatic location detection failed', { error: errorMsg });
      }
      
      // Load settings from localStorage before auto-starting services
      try {
        const storedSettings = localStorage.getItem("chiralSettings");
        if (storedSettings) {
          const parsed = JSON.parse(storedSettings);
          // Ensure selectedProtocol always has a valid default
          if (!parsed.selectedProtocol) {
            parsed.selectedProtocol = "WebRTC";
          }
          settings.update(prev => ({ ...prev, ...parsed }));
        }
      } catch (error) {
        const errorMsg = error instanceof Error ? error.message : String(error);
        diagnosticLogger.warn('SETTINGS', 'Failed to load settings from localStorage', { error: errorMsg });
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
            diagnosticLogger.info('DHT', 'Auto-starting DHT node...');

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
                chunkSizeKb: currentSettings.chunkSize,
                cacheSizeMb: currentSettings.cacheSize,
                enableUpnp: currentSettings.enableUPnP,
                pureClientMode: currentSettings.pureClientMode,
                forceServerMode: currentSettings.forceServerMode,
              });

              diagnosticLogger.info('DHT', 'DHT node auto-started successfully', { peerId });

              // Update network status
              networkStatus.set('connected');
            } catch (dhtError) {
              const dhtErrorMsg = dhtError instanceof Error ? dhtError.message : String(dhtError);
              if (dhtErrorMsg.includes("already running")) {
                // Race condition - DHT was started between our check and start attempt
                diagnosticLogger.info('DHT', 'DHT started by another process (race condition)');

                // Sync the peer ID since DHT is running
                const { dhtService } = await import("$lib/dht");
                try {
                  const peerId = await invoke<string | null>("get_dht_peer_id");
                  if (peerId) {
                    dhtService.setPeerId(peerId);
                    diagnosticLogger.info('DHT', 'Synced peer ID after race condition', { peerId });
                  }
                } catch {}

                networkStatus.set('connected');
              } else {
                // Real error
                diagnosticLogger.error('DHT', 'Failed to auto-start DHT', { error: dhtErrorMsg });
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
            diagnosticLogger.warn('FILE_TRANSFER', 'File transfer service start warning', { error: ftErrorMsg });
          }
        }
        
        // Start Geth blockchain node if auto-start is enabled
        // Pure-client mode uses partial sync (limited blocks) instead of full sync
        if (currentSettings.autoStartGeth && typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
          try {
            // Check if Geth (local or shared remote RPC) is already running
            const isGethRunning = await invoke<boolean>('is_geth_running').catch(() => false);

            if (isGethRunning) {
              // Shared RPC or local Geth is reachable — mark as running so
              // wallet/transaction logic isn't gated behind gethStatus
              const { gethStatus } = await import('./lib/services/gethService');
              gethStatus.set('running');
              diagnosticLogger.info('GETH', 'Geth RPC is already reachable, skipping local Geth start');
            } else {
              // Check if Geth is installed
              const isGethInstalled = await invoke<boolean>('check_geth_binary').catch(() => false);

              if (!isGethInstalled) {
                diagnosticLogger.info('GETH', 'Geth not installed. Downloading Geth...');
                showToast("Downloading Geth blockchain node...", "info");

                // Listen for download progress
                const unlisten = await listen<GethDownloadProgressPayload>('geth-download-progress', (event: Event<GethDownloadProgressPayload>) => {
                  const progress = event.payload;
                  if (progress.percentage >= 100) {
                    diagnosticLogger.info('GETH', 'Geth download complete');
                  } else {
                    diagnosticLogger.debug('GETH', `Downloading Geth: ${progress.percentage.toFixed(1)}%`);
                  }
                });

                try {
                  await invoke('download_geth_binary');
                  unlisten();
                  diagnosticLogger.info('GETH', 'Geth downloaded successfully');
                  showToast("Geth downloaded successfully", "success");
                } catch (downloadError) {
                  unlisten();
                  const downloadErrorMsg = downloadError instanceof Error ? downloadError.message : String(downloadError);
                  diagnosticLogger.error('GETH', 'Failed to download Geth', { error: downloadErrorMsg });
                  showToast(`Failed to download Geth: ${downloadErrorMsg}`, "error");
                  throw downloadError; // Don't try to start if download failed
                }
              }

              try {
                // Check if in client mode (forced OR NAT-based)
                let isClientMode = currentSettings.pureClientMode;
                if (!isClientMode) {
                  // Check DHT reachability to detect NAT-based client mode
                  try {
                    const { dhtService } = await import('./lib/dht');
                    const health = await dhtService.getHealth();
                    if (health && health.reachability === 'private') {
                      isClientMode = true;
                    }
                  } catch (err) {
                    const errorMsg = err instanceof Error ? err.message : String(err);
                    diagnosticLogger.warn('GETH', 'Failed to check DHT reachability for client mode', { error: errorMsg });
                  }
                }

                await invoke('start_geth_node', {
                  dataDir: './bin/geth-data',
                  pureClientMode: isClientMode  // Combined: forced OR NAT-based
                });

                // Update geth status
                const { gethStatus } = await import('./lib/services/gethService');
                gethStatus.set('running');
              } catch (gethError) {
                const gethErrorMsg = gethError instanceof Error ? gethError.message : String(gethError);
                if (gethErrorMsg.includes("already running") || gethErrorMsg.includes("already started")) {
                  diagnosticLogger.info('GETH', 'Geth started by another process');
                } else if (gethErrorMsg.includes("not found") || gethErrorMsg.includes("No such file")) {
                  diagnosticLogger.info('GETH', 'Geth not downloaded yet. Download it from the Network page.');
                } else {
                  diagnosticLogger.error('GETH', 'Failed to auto-start Geth', { error: gethErrorMsg });
                }
              }
            }
          } catch (error) {
            const errorMsg = error instanceof Error ? error.message : String(error);
            diagnosticLogger.error('GETH', 'Error checking/starting Geth', { error: errorMsg });
          }
        }
      } catch (error) {
        // Unexpected error in the initialization block
        const errorMsg = error instanceof Error ? error.message : String(error);
        diagnosticLogger.error('APP', 'Unexpected error during service initialization', { error: errorMsg });
      }
    })();

    // set the currentPage var
    syncFromUrl();

    // Start network monitoring
    stopNetworkMonitoring = startNetworkMonitoring();

    // Start Geth monitoring
    stopGethMonitoring = startGethMonitoring();

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
        const appWindow = getCurrentWebviewWindow();
        appWindow.close().catch((error) => {
          const errorMsg = error instanceof Error ? error.message : String(error);
          diagnosticLogger.error('APP', 'Failed to close app window', { error: errorMsg });
        });
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


  let navDropdownOpen = false;
  let mobileMenuOpen = false;

  // Close dropdown when clicking outside
  function handleClickOutside(event: MouseEvent) {
    const target = event.target as HTMLElement;
    if (navDropdownOpen && !target.closest('.nav-dropdown-container')) {
      navDropdownOpen = false;
    }
  }

  onMount(() => {
    if (typeof window !== 'undefined') {
      window.addEventListener('click', handleClickOutside);
    }
  });

  onDestroy(() => {
    if (typeof window !== 'undefined') {
      window.removeEventListener('click', handleClickOutside);
    }
  });

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
      { id: "account", label: $t("nav.account"), icon: Wallet },
      { id: "analytics", label: $t("nav.analytics"), icon: BarChart3 },
      { id: "blockchain", label: $t("nav.blockchain"), icon: Database },
      { id: "chiraldrop", label: $t("nav.chiraldrop"), icon: Droplet },
      { id: "download", label: $t("nav.download"), icon: Download },
      { id: "mining", label: $t("nav.mining"), icon: Cpu },
      { id: "network", label: $t("nav.network"), icon: Globe },
      { id: "reputation", label: $t("nav.reputation"), icon: Star },
      { id: "settings", label: $t("nav.settings"), icon: Settings },
      { id: "upload", label: $t("nav.upload"), icon: Upload },
      // { id: 'proxy', label: $t('nav.proxy'), icon: Shield }, // DISABLED

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
      path: "mining",
      component: MiningPage,
    },
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
      path: "chiraldrop",
      component: ChiralDropPage,
    },
    {
      path: "account",
      component: AccountPage,
    },
    {
      path: "settings",
      component: SettingsPage,
    },
  ];

  onDestroy(() => {
    unlistenExitPrompt?.();
    unsubscribeScheduler?.();
    unsubscribeBandwidth?.();
    transferStoreUnsubscribe?.();
    unsubscribeAuthWizard?.();
  });
</script>

<div class="flex flex-col bg-background h-full">
  {#if loading}
    <div class="w-full h-full flex items-center justify-center">
      <div
        class="max-w-lg w-full mx-6 p-6 rounded-lg border border-border bg-card text-card-foreground"
      >
        {#if initError}
          <h2 class="text-lg font-semibold mb-2">App initialization failed</h2>
          <p class="text-sm text-muted-foreground mb-4 break-words">
            {initError}
          </p>
          <button
            class="px-4 py-2 rounded bg-primary text-primary-foreground"
            on:click={() => window.location.reload()}
          >
            Retry
          </button>
        {:else}
          <h2 class="text-lg font-semibold mb-2">Loading…</h2>
          <p class="text-sm text-muted-foreground">
            If this takes too long, check the console for errors. Please wait.
          </p>
        {/if}
      </div>
    </div>
  {:else}
    <!-- Desktop Navbar -->
    <nav class="sticky top-0 z-50 bg-card border-b">
      <div class="px-4 py-3">
        <div class="flex items-center justify-between">
          <!-- Left: Logo/Brand -->
          <div class="flex items-center gap-2">
            <span class="font-bold text-lg">Chiral Network</span>
          </div>

          <!-- Center: Primary Navigation -->
          <div class="hidden md:flex items-center gap-1">
            <!-- Download -->
            <button
              on:click={() => navigateTo('download', '/download')}
              class="px-4 py-2 rounded transition-colors {currentPage === 'download' ? 'bg-primary text-primary-foreground' : 'hover:bg-gray-100'}"
            >
              <div class="flex items-center gap-2">
                <Download class="h-4 w-4" />
                <span>{menuItems.find(i => i.id === 'download')?.label || 'Download'}</span>
              </div>
            </button>

            <!-- Upload -->
            <button
              on:click={() => navigateTo('upload', '/upload')}
              class="px-4 py-2 rounded transition-colors {currentPage === 'upload' ? 'bg-primary text-primary-foreground' : 'hover:bg-gray-100'}"
            >
              <div class="flex items-center gap-2">
                <Upload class="h-4 w-4" />
                <span>{menuItems.find(i => i.id === 'upload')?.label || 'Upload'}</span>
              </div>
            </button>

            <!-- Account -->
            <button
              on:click={() => navigateTo('account', '/account')}
              class="px-4 py-2 rounded transition-colors {currentPage === 'account' ? 'bg-primary text-primary-foreground' : 'hover:bg-gray-100'}"
            >
              <div class="flex items-center gap-2">
                <Wallet class="h-4 w-4" />
                <span>{menuItems.find(i => i.id === 'account')?.label || 'Account'}</span>
              </div>
            </button>

            <!-- Network -->
            <button
              on:click={() => navigateTo('network', '/network')}
              class="px-4 py-2 rounded transition-colors {currentPage === 'network' ? 'bg-primary text-primary-foreground' : 'hover:bg-gray-100'}"
            >
              <div class="flex items-center gap-2">
                <Globe class="h-4 w-4" />
                <span>{menuItems.find(i => i.id === 'network')?.label || 'Network'}</span>
              </div>
            </button>

            <!-- Settings -->
            <button
              on:click={() => navigateTo('settings', '/settings')}
              class="px-4 py-2 rounded transition-colors {currentPage === 'settings' ? 'bg-primary text-primary-foreground' : 'hover:bg-gray-100'}"
            >
              <div class="flex items-center gap-2">
                <Settings class="h-4 w-4" />
                <span>{menuItems.find(i => i.id === 'settings')?.label || 'Settings'}</span>
              </div>
            </button>

            <!-- More Dropdown -->
            <div class="relative nav-dropdown-container">
              <button
                on:click={() => navDropdownOpen = !navDropdownOpen}
                class="px-4 py-2 rounded transition-colors hover:bg-gray-100 flex items-center gap-1"
              >
                <span>{$t('nav.more')}</span>
                <ChevronDown class="h-4 w-4" />
              </button>

              {#if navDropdownOpen}
                <div 
                  class="absolute right-0 mt-1 w-48 rounded-md border bg-white shadow-lg z-50"
                  role="button"
                  tabindex="0"
                  on:click|stopPropagation
                  on:keydown={(e) => e.key === 'Escape' && (navDropdownOpen = false)}
                >
                  {#each menuItems.filter(item => !['download', 'upload', 'account', 'network', 'settings'].includes(item.id)) as item}
                    {@const requiresGeth = item.id === 'blockchain' || item.id === 'mining'}
                    {@const isBlocked = requiresGeth && $gethStatus !== 'running'}
                    <button
                      on:click={() => {
                        if (isBlocked) return;
                        navigateTo(item.id, `/${item.id}`);
                        navDropdownOpen = false;
                      }}
                      class="w-full text-left px-4 py-2 text-sm hover:bg-gray-100 flex items-center gap-2 {isBlocked ? 'cursor-not-allowed opacity-60' : ''}"
                      disabled={isBlocked}
                      title={isBlocked ? $t('nav.blockchainUnavailable') + ' ' + $t('nav.networkPageLink') : ''}
                    >
                      <svelte:component this={item.icon} class="h-4 w-4" />
                      <span>{item.label}</span>
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          </div>

          <!-- Right: Status and Logout -->
          <div class="hidden md:flex items-center gap-4">
            <!-- Connection Status -->
            <div class="flex items-center gap-2 text-sm">
              <div
                class="w-2 h-2 rounded-full {$networkStatus === 'connected'
                  ? 'bg-green-500'
                  : 'bg-red-500'}"
              ></div>
              <span class="text-muted-foreground">
                {$networkStatus === "connected"
                  ? $t("nav.connected")
                  : $t("nav.disconnected")}
              </span>
            </div>

            <!-- Logout Button -->
            <button
              on:click={async () => await lockAccount()}
              class="px-3 py-2 rounded transition-colors hover:bg-red-100 text-red-600 flex items-center gap-2"
              title={$t('actions.lockWallet')}
            >
              <LogOut class="h-4 w-4" />
              <span class="text-sm">{$t('actions.logout')}</span>
            </button>
          </div>

          <!-- Mobile Menu Button -->
          <button
            class="md:hidden p-2 rounded hover:bg-gray-100"
            on:click={() => mobileMenuOpen = true}
          >
            <Menu class="h-6 w-6" />
          </button>
        </div>
      </div>
    </nav>

    <!-- Mobile Menu Overlay -->
    {#if mobileMenuOpen}
      <!-- Backdrop -->
      <div
        class="fixed inset-0 bg-black bg-opacity-50 z-40 md:hidden"
        role="button"
        tabindex="0"
        aria-label={$t("nav.closeMenu")}
        on:click={() => mobileMenuOpen = false}
        on:keydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            mobileMenuOpen = false;
          }
        }}
      ></div>

      <!-- Mobile Menu -->
      <div class="fixed top-0 right-0 h-full w-64 bg-white z-50 flex flex-col md:hidden">
        <!-- Header -->
        <div class="flex justify-between items-center p-4 border-b">
          <span class="font-bold text-base">Chiral Network</span>
          <button on:click={() => mobileMenuOpen = false}>
            <X class="h-6 w-6" />
          </button>
        </div>

        <!-- Status -->
        <div class="px-4 py-3 border-b">
          <div class="flex items-center gap-2">
            <div
              class="w-2 h-2 rounded-full {$networkStatus === 'connected'
                ? 'bg-green-500'
                : 'bg-red-500'}"
            ></div>
            <span class="text-muted-foreground text-sm">
              {$networkStatus === "connected"
                ? $t("nav.connected")
                : $t("nav.disconnected")}
            </span>
          </div>
        </div>

        <!-- Menu Items -->
        <nav class="flex-1 p-4 space-y-2 overflow-y-auto">
          {#each menuItems as item}
            {@const requiresGeth = item.id === 'blockchain' || item.id === 'mining'}
            {@const isBlocked = requiresGeth && $gethStatus !== 'running'}
            <button
              on:click={() => {
                if (isBlocked) return;
                navigateTo(item.id, `/${item.id}`);
                mobileMenuOpen = false;
              }}
              class="w-full flex items-center rounded px-4 py-3 text-base {currentPage === item.id ? 'bg-primary text-primary-foreground' : isBlocked ? 'cursor-not-allowed opacity-60' : 'hover:bg-gray-100'}"
              disabled={isBlocked}
              title={isBlocked ? $t('nav.blockchainUnavailable') + ' ' + $t('nav.networkPageLink') : ''}
            >
              <svelte:component this={item.icon} class="h-5 w-5 mr-3" />
              {item.label}
            </button>
          {/each}
        </nav>

        <!-- Logout Button -->
        <div class="p-4 border-t">
          <button
            on:click={async () => {
              await lockAccount();
              mobileMenuOpen = false;
            }}
            class="w-full px-4 py-3 rounded bg-red-50 text-red-600 hover:bg-red-100 flex items-center justify-center gap-2"
          >
            <LogOut class="h-5 w-5" />
            <span>{$t('actions.logout')}</span>
          </button>
        </div>
      </div>
    {/if}
  {/if}

  <!-- Main Content -->
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

{#if showExitPrompt}
  <ExitPrompt
    isExiting={isExiting}
    isLocking={isLockingAccount}
    canLock={canShowLockAction}
    error={exitError}
    onStay={handleStayInApp}
    onLockAccount={handleLockFromExitPrompt}
    onExit={handleConfirmExit}
  />
{/if}

  <!-- add Toast  -->
<SimpleToast />
