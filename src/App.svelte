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
  import { AlertTriangle } from 'lucide-svelte';
  import { cancelLogout, confirmLogout, logoutModalOpen, loggingOut } from '$lib/logout';
  import WalletPage from './pages/Wallet.svelte';
  import DownloadPage from './pages/Download.svelte';
  import ChiralDropPage from './pages/ChiralDrop.svelte';
  import AccountPage from './pages/Account.svelte';
  import NetworkPage from './pages/Network.svelte';
  import MiningPage from './pages/Mining.svelte';
  import DiagnosticsPage from './pages/Diagnostics.svelte';
  import SettingsPage from './pages/Settings.svelte';
  import HostsPage from './pages/Hosts.svelte';
  import DrivePage from './pages/Drive.svelte';


  let currentPath = $state('/wallet');
  let sidebarCollapsed = $state(false);
  let hostingAutoPublishedKey = $state<string | null>(null);
  let hostingAutoPublishing = $state(false);
  let hostingAutoPublishRetryTimer = $state<number | null>(null);
  let autoReseedInFlight = $state(false);
  let autoReseedCompletedWallet = $state<string | null>(null);
  let unlistenBootstrapComplete: (() => void) | null = null;
  let showCloseConfirm = $state(false);
  let closeConfirmResolve: ((confirmed: boolean) => void) | null = null;

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
      const currentWindow = getCurrentWindow();

      // --- Graceful close: confirm with user, cleanup DHT, then force-close ---
      let isClosing = false;
      currentWindow.onCloseRequested(async (event) => {
        if (isClosing) return;
        event.preventDefault();

        // Ask the user to confirm
        const confirmed = await new Promise<boolean>((resolve) => {
          closeConfirmResolve = resolve;
          showCloseConfirm = true;
        });
        showCloseConfirm = false;
        closeConfirmResolve = null;

        if (!confirmed) return;

        isClosing = true;
        // Kick off DHT cleanup, but don't hold the UI open for long.
        const shutdownCleanup = Promise.allSettled([
          invoke('unpublish_all_shared_files'),
          invoke('unpublish_host_advertisement'),
        ]);
        await Promise.race([
          shutdownCleanup,
          new Promise((resolve) => setTimeout(resolve, 150)),
        ]);
        // Force-close the native window without emitting another close request.
        void currentWindow.destroy();
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
                priceChi: null, walletAddress: $walletAccount?.address ?? null,
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
    const walletAddress = $walletAccount?.address ?? null;

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
              walletAddress,
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
              walletAddress,
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
      path: '/hosts',
      component: HostsPage
    },
    {
      path: '/drive',
      component: DrivePage
    },
    {
      path: '/settings',
      component: SettingsPage
    },
    {
      path: '/',
      component: AccountPage
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
  
  // Redirect to account page when authenticated
  $effect(() => {
    if ($isAuthenticated) {
      const path = window.location.pathname;
      if (path === '/wallet' || path === '/') {
        goto('/account');
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

  // Auto-connect DHT whenever the user is authenticated.
  // On logout dhtService.stop() is called, so on next login we need to restart.
  let dhtStartedForSession = $state(false);
  $effect(() => {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
    if ($isAuthenticated && !dhtStartedForSession) {
      dhtStartedForSession = true;
      dhtService.start().catch((err) => {
        console.warn('DHT auto-start failed:', err);
      });
    }
    if (!$isAuthenticated) {
      dhtStartedForSession = false;
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
    description={toast.description}
    type={toast.type}
    duration={toast.duration}
    {index}
    onClose={() => toasts.remove(toast.id)}
  />
{/each}

<!-- Logout confirmation modal -->
{#if $logoutModalOpen}
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="fixed inset-0 bg-black/50 flex items-center justify-center z-[9998]"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={cancelLogout}
  onkeydown={(e: KeyboardEvent) => { if (e.key === 'Escape') cancelLogout(); }}
>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="bg-white dark:bg-gray-800 rounded-xl shadow-xl p-6 max-w-md mx-4"
    role="document"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <div class="flex items-center gap-3 mb-4">
      <div class="p-2 bg-red-100 dark:bg-red-900/30 rounded-lg">
        <AlertTriangle class="w-6 h-6 text-red-600 dark:text-red-400" />
      </div>
      <h3 class="text-lg font-semibold dark:text-white">Logout</h3>
    </div>
    <p class="text-sm text-gray-600 dark:text-gray-400 mb-6">
      Are you sure you want to logout? Make sure you have saved your recovery phrase or exported your wallet before logging out.
    </p>
    <div class="flex gap-3">
      <button
        onclick={cancelLogout}
        disabled={$loggingOut}
        class="flex-1 px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors dark:text-gray-300 disabled:opacity-50 disabled:cursor-not-allowed"
      >
        Cancel
      </button>
      <button
        onclick={confirmLogout}
        disabled={$loggingOut}
        class="flex-1 px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {#if $loggingOut}
          Logging out...
        {:else}
          Logout
        {/if}
      </button>
    </div>
  </div>
</div>
{/if}

<!-- Close confirmation modal -->
{#if showCloseConfirm}
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 bg-black/50 flex items-center justify-center z-[9999]"
  onkeydown={(e: KeyboardEvent) => { if (e.key === 'Escape') closeConfirmResolve?.(false); }}
>
  <div class="bg-white dark:bg-gray-800 rounded-xl shadow-2xl border border-gray-200 dark:border-gray-700 p-6 max-w-sm w-full mx-4">
    <div class="flex items-center gap-3 mb-3">
      <div class="p-2 bg-amber-100 dark:bg-amber-900/40 rounded-lg">
        <AlertTriangle class="w-5 h-5 text-amber-600 dark:text-amber-400" />
      </div>
      <h3 class="font-semibold text-lg text-gray-900 dark:text-white">Quit Chiral Network?</h3>
    </div>
    <p class="text-sm text-gray-600 dark:text-gray-400 mb-5 ml-[52px]">
      Active downloads, seeding, and mining will be stopped. Any in-progress file transfers will be cancelled.
    </p>
    <div class="flex gap-3 justify-end">
      <button
        onclick={() => closeConfirmResolve?.(false)}
        class="px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg text-sm font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
      >
        Cancel
      </button>
      <button
        onclick={() => closeConfirmResolve?.(true)}
        class="px-4 py-2 bg-red-600 text-white rounded-lg text-sm font-medium hover:bg-red-700 transition-colors"
      >
        Quit
      </button>
    </div>
  </div>
</div>
{/if}

<style>
</style>
