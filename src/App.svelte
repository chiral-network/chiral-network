<script lang="ts">
  import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
  import { isAuthenticated, isDarkMode, networkConnected, settings, walletAccount } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import { dhtService } from '$lib/dhtService';
  import { gethService } from '$lib/services/gethService';
  import { applyColorTheme } from '$lib/services/colorThemeService';
  import { onDestroy, onMount } from 'svelte';
  import Navbar from '$lib/components/Navbar.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import Toast from '$lib/components/Toast.svelte';
  import WalletPage from './pages/Wallet.svelte';
  import DownloadPage from './pages/Download.svelte';
  import ChiralDropPage from './pages/ChiralDrop.svelte';
  import AccountPage from './pages/Account.svelte';
  import NetworkPage from './pages/Network.svelte';
  import MiningPage from './pages/Mining.svelte';
  import DiagnosticsPage from './pages/Diagnostics.svelte';
  import SettingsPage from './pages/Settings.svelte';
  import HostingPage from './pages/Hosting.svelte';
  import DrivePage from './pages/Drive.svelte';
  import HostsPage from './pages/Hosts.svelte';

  let currentPath = $state('/wallet');
  let sidebarCollapsed = $state(false);
  let hostingAutoPublishedKey = $state<string | null>(null);
  let hostingAutoPublishing = $state(false);
  let hostingAutoPublishRetryTimer = $state<number | null>(null);
  let autoReseedInFlight = $state(false);
  let autoReseedCompletedWallet = $state<string | null>(null);
  let unlistenBootstrapComplete: (() => void) | null = null;

  // Apply dark mode class to document
  $effect(() => {
    if (typeof document !== 'undefined') {
      if ($isDarkMode) {
        document.documentElement.classList.add('dark');
      } else {
        document.documentElement.classList.remove('dark');
      }
    }
  });

  // Apply color theme
  $effect(() => {
    if (typeof document !== 'undefined') {
      applyColorTheme($settings.colorTheme);
    }
  });

  // Unpublish shared files from DHT when the window is closing
  // + Auto-register hosted files as seeded when downloads complete
  // + Auto-reseed legacy uploads and hosted files on startup
  onMount(async () => {
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const { invoke } = await import('@tauri-apps/api/core');
      const { listen } = await import('@tauri-apps/api/event');

      // --- Graceful close: cleanup DHT, then force-close the window ---
      let isClosing = false;
      getCurrentWindow().onCloseRequested(async (event) => {
        if (isClosing) return; // guard against re-entrant calls
        isClosing = true;
        event.preventDefault(); // take control of the close sequence
        // Keep close responsive: run cleanup best-effort with a short cap.
        await Promise.race([
          Promise.allSettled([
            invoke('unpublish_all_shared_files'),
            invoke('unpublish_host_advertisement'),
          ]),
          new Promise((resolve) => setTimeout(resolve, 800)),
        ]);
        // Use backend exit_app command which calls AppHandle::exit(0)
        // This triggers RunEvent::Exit for Geth cleanup and fully terminates the process.
        await invoke('exit_app');
      });

      // Global listener: when a file download completes, check if it belongs
      // to a hosting agreement and auto-register as seeder + publish to DHT.
      // This runs regardless of which page is mounted.
      await listen<{
        fileHash: string; fileName: string; filePath: string; fileSize: number;
      }>('file-download-complete', async (event) => {
        const { fileHash, fileName, filePath, fileSize } = event.payload;
        try {
          // Check if this file belongs to any hosting agreement we're hosting
          const myPeerId = await invoke<string | null>('get_peer_id');
          if (!myPeerId) return;

          const agreementIds = await invoke<string[]>('list_hosting_agreements');
          for (const id of agreementIds) {
            const json = await invoke<string | null>('get_hosting_agreement', { agreementId: id });
            if (!json) continue;
            const agreement = JSON.parse(json);
            if (
              agreement.hostPeerId === myPeerId &&
              (agreement.status === 'accepted' || agreement.status === 'active') &&
              agreement.fileHashes?.includes(fileHash)
            ) {
              // Register as seeder and publish to DHT
              await invoke('republish_shared_file', {
                fileHash, filePath, fileName, fileSize,
                priceChi: null, walletAddress: null,
              });
              console.log(`✅ Auto-registered hosted file ${fileHash} as seeder`);

              // Update agreement status to active
              agreement.status = 'active';
              await invoke('store_hosting_agreement', {
                agreementId: id,
                agreementJson: JSON.stringify(agreement),
              });

              // Notify proposer that hosting is active (best-effort)
              const message = JSON.stringify({
                type: 'hosting_response',
                agreementId: id,
                status: 'active',
              });
              try {
                await invoke('echo_peer', {
                  peerId: agreement.clientPeerId,
                  payload: Array.from(new TextEncoder().encode(message)),
                });
              } catch {
                // Peer offline — they'll see the status change when they load agreements
              }

              // Add hosted file to Drive "Shared" folder
              try {
                const addr = $walletAccount?.address;
                if (addr) {
                  const rootItems = await invoke<{ id: string; name: string; itemType: string }[]>(
                    'drive_list_items', { owner: addr, parentId: null },
                  );
                  let sharedFolder = rootItems.find(
                    (i) => i.name === 'Shared' && i.itemType === 'folder',
                  );
                  if (!sharedFolder) {
                    sharedFolder = await invoke<{ id: string; name: string; itemType: string }>(
                      'drive_create_folder', { owner: addr, name: 'Shared', parentId: null },
                    );
                  }
                  await invoke('drive_upload_file', {
                    owner: addr,
                    filePath: filePath,
                    parentId: sharedFolder.id,
                    merkleRoot: fileHash,
                  });
                  console.log(`✅ Added hosted file ${fileHash} to Drive/Shared`);
                }
              } catch (driveErr) {
                console.warn('Failed to add hosted file to Drive:', driveErr);
              }

              break;
            }
          }
        } catch {
          // Non-hosting download or error — ignore silently
        }
      });

    }
  });

  onDestroy(() => {
    if (hostingAutoPublishRetryTimer !== null && typeof window !== 'undefined') {
      window.clearTimeout(hostingAutoPublishRetryTimer);
      hostingAutoPublishRetryTimer = null;
    }
    unlistenBootstrapComplete?.();
    unlistenBootstrapComplete = null;
  });

  // Auto-reseed AFTER Kademlia bootstrap completes, so DHT puts propagate.
  // The Rust backend emits "dht-bootstrap-complete" once bootstrap finishes.
  onMount(async () => {
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      const { listen } = await import('@tauri-apps/api/event');
      unlistenBootstrapComplete = await listen('dht-bootstrap-complete', () => {
        void maybeAutoReseed(true);
      });
    }
  });

  async function maybeAutoReseed(force = false) {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
    if (!$isAuthenticated || !$networkConnected) return;
    const addr = $walletAccount?.address ?? null;
    if (!addr) return;
    if (autoReseedInFlight) return;
    if (!force && autoReseedCompletedWallet === addr) return;

    autoReseedInFlight = true;
    try {
      await autoReseedOnStartup();
      autoReseedCompletedWallet = addr;
    } finally {
      autoReseedInFlight = false;
    }
  }

  async function autoReseedOnStartup() {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
    const { invoke } = await import('@tauri-apps/api/core');

    // 1. Re-register uploaded files from localStorage
    try {
      const stored = localStorage.getItem('chiral_upload_history');
      if (stored) {
        const files = JSON.parse(stored) as Array<{
          hash: string; filePath: string; name: string; size: number; priceChi?: string;
        }>;
        let count = 0;
        for (const file of files) {
          try {
            await invoke('republish_shared_file', {
              fileHash: file.hash,
              filePath: file.filePath,
              fileName: file.name,
              fileSize: file.size,
              priceChi: file.priceChi && file.priceChi !== '0' ? file.priceChi : null,
              walletAddress: file.priceChi && file.priceChi !== '0' ? $walletAccount?.address : null,
            });
            count++;
          } catch {
            // File may no longer exist on disk — skip
          }
        }
        if (count > 0) console.log(`✅ Auto-reseeded ${count} uploaded file(s)`);
      }
    } catch {
      // localStorage parse error — skip
    }

    // 2. Re-register hosted files from active agreements
    try {
      const hostedEntries = await invoke<{ fileHash: string; agreementId: string; clientPeerId: string }[]>(
        'get_active_hosted_files'
      );
      if (hostedEntries.length > 0) {
        const downloadDir = await invoke<string>('get_download_directory');
        let count = 0;
        for (const entry of hostedEntries) {
          try {
            await invoke('republish_shared_file', {
              fileHash: entry.fileHash,
              filePath: `${downloadDir}/${entry.fileHash}`,
              fileName: entry.fileHash,
              fileSize: 0,
              priceChi: null,
              walletAddress: null,
            });
            count++;
          } catch {
            // File may not exist on disk — skip
          }
        }
        if (count > 0) console.log(`✅ Auto-reseeded ${count} hosted file(s)`);
      }
    } catch {
      // Agreements dir may not exist yet — skip
    }
  }

  function clearHostingAutoPublishRetry() {
    if (hostingAutoPublishRetryTimer !== null && typeof window !== 'undefined') {
      window.clearTimeout(hostingAutoPublishRetryTimer);
      hostingAutoPublishRetryTimer = null;
    }
  }

  function scheduleHostingAutoPublishRetry() {
    if (typeof window === 'undefined' || hostingAutoPublishRetryTimer !== null) return;
    hostingAutoPublishRetryTimer = window.setTimeout(() => {
      hostingAutoPublishRetryTimer = null;
      void maybeAutoPublishHosting();
    }, 3000);
  }

  async function maybeAutoPublishHosting(): Promise<void> {
    if (
      typeof window === 'undefined'
      || !('__TAURI_INTERNALS__' in window)
      || !$isAuthenticated
      || !$networkConnected
      || !$settings.hostingConfig?.enabled
    ) {
      return;
    }

    const addr = $walletAccount?.address;
    if (!addr) return;

    const publishKey = `${addr}:${JSON.stringify($settings.hostingConfig)}`;
    if (hostingAutoPublishing || hostingAutoPublishedKey === publishKey) {
      return;
    }

    hostingAutoPublishing = true;
    try {
      const { hostingService } = await import('$lib/services/hostingService');
      await hostingService.publishHostAdvertisement($settings.hostingConfig, addr);
      hostingAutoPublishedKey = publishKey;
      clearHostingAutoPublishRetry();
      console.log('✅ Auto-published hosting marketplace advertisement');
    } catch (err) {
      hostingAutoPublishedKey = null;
      scheduleHostingAutoPublishRetry();
      console.warn('Hosting auto-publish failed, retrying shortly:', err);
    } finally {
      hostingAutoPublishing = false;
    }
  }

  const authenticatedRoutes: RouteConfig[] = [
    {
      path: '/download',
      component: DownloadPage
    },
    {
      path: '/chiraldrop',
      component: ChiralDropPage
    },
    {
      path: '/account',
      component: AccountPage
    },
    {
      path: '/network',
      component: NetworkPage
    },
    {
      path: '/mining',
      component: MiningPage
    },
    {
      path: '/diagnostics',
      component: DiagnosticsPage
    },
    {
      path: '/hosting',
      component: HostingPage
    },
    {
      path: '/drive',
      component: DrivePage
    },
    {
      path: '/hosts',
      component: HostsPage
    },
    {
      path: '/settings',
      component: SettingsPage
    },
    {
      path: '/',
      component: NetworkPage
    }
  ];
  
  const unauthenticatedRoutes: RouteConfig[] = [
    {
      path: '/wallet',
      component: WalletPage
    },
    {
      path: '/',
      component: WalletPage
    },
    {
      path: '*',
      component: WalletPage
    }
  ];
  
  // Track current path for navbar highlighting
  $effect(() => {
    currentPath = window.location.pathname || '/';
  });
  
  // Redirect to network page when authenticated
  $effect(() => {
    if ($isAuthenticated) {
      const path = window.location.pathname;
      if (path === '/wallet' || path === '/') {
        goto('/network');
      }
    }
  });

  // Sync download directory setting to backend on startup
  let downloadDirSynced = false;
  $effect(() => {
    if (!downloadDirSynced && typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window)) {
      downloadDirSynced = true;
      const dir = $settings.downloadDirectory;
      if (dir) {
        import('@tauri-apps/api/core').then(({ invoke }) => {
          invoke('set_download_directory', { path: dir }).catch(() => {});
        });
      }
    }
  });

  // Auto-connect DHT once on app launch.
  let dhtAutoConnected = false;
  $effect(() => {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
    if (!dhtAutoConnected) {
      dhtAutoConnected = true;
      dhtService.start().catch((err) => {
        console.warn('DHT auto-start failed:', err);
      });
    }
  });

  // Fallback auto-reseed trigger when the app is authenticated and the network is up.
  // This covers sessions where bootstrap-complete may have fired before our listener
  // or when DHT is already running on app startup.
  $effect(() => {
    const canReseed =
      typeof window !== 'undefined'
      && '__TAURI_INTERNALS__' in window
      && $isAuthenticated
      && $networkConnected
      && !!$walletAccount?.address;

    if (!canReseed) {
      autoReseedCompletedWallet = null;
      return;
    }

    void maybeAutoReseed(false);
  });

  // Auto-publish hosting marketplace on login while enabled.
  // This path is independent of bootstrap events so it still works when DHT is
  // already running from a previous session.
  $effect(() => {
    const enabled = $settings.hostingConfig?.enabled ?? false;
    const addr = $walletAccount?.address ?? null;
    const canPublish = $isAuthenticated && enabled && !!addr && $networkConnected;

    if (!canPublish) {
      hostingAutoPublishedKey = null;
      clearHostingAutoPublishRetry();
      return;
    }

    void maybeAutoPublishHosting();
  });

  // Auto-start Geth node once when user logs in
  let gethAutoStarted = false;
  $effect(() => {
    if ($isAuthenticated && !gethAutoStarted) {
      gethAutoStarted = true;
      (async () => {
        try {
          const installed = await gethService.isInstalled();
          if (!installed) return;
          const status = await gethService.getStatus();
          if (status.running) {
            // Already running — just start polling
            gethService.startStatusPolling();
            return;
          }
          // Start Geth with wallet address as miner
          const addr = $walletAccount?.address;
          await gethService.start(addr || undefined);
        } catch (err) {
          // Silently handle — user can start manually from Network page
          console.warn('Geth auto-start failed:', err);
        }
      })();
    }
    if (!$isAuthenticated) {
      gethAutoStarted = false;
    }
  });
</script>

{#if $isAuthenticated}
  {#if $settings.navStyle === 'sidebar'}
    <div class="min-h-screen bg-gray-50 dark:bg-gray-900 transition-colors">
      <Sidebar currentPage={currentPath} bind:collapsed={sidebarCollapsed} />
      <div class="transition-[margin] duration-200 hidden md:block {sidebarCollapsed ? 'md:ml-16' : 'md:ml-48'}">
        <Router routes={authenticatedRoutes} />
      </div>
      <div class="md:hidden">
        <Router routes={authenticatedRoutes} />
      </div>
    </div>
  {:else}
    <div class="min-h-screen bg-gray-50 dark:bg-gray-900 transition-colors">
      <Navbar currentPage={currentPath} />
      <Router routes={authenticatedRoutes} />
    </div>
  {/if}
{:else}
  <div class="dark:bg-gray-900 min-h-screen transition-colors">
    <Router routes={unauthenticatedRoutes} />
  </div>
{/if}

<!-- Toast notifications -->
{#each $toasts as toast, index (toast.id)}
  <Toast
    message={toast.message}
    type={toast.type}
    {index}
    onClose={() => toasts.remove(toast.id)}
  />
{/each}

<style>
</style>
