<script lang="ts">
  import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
  import { isAuthenticated, isDarkMode, networkConnected, settings, walletAccount } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import { dhtService } from '$lib/dhtService';
  import { gethService } from '$lib/services/gethService';
  import { applyColorTheme } from '$lib/services/colorThemeService';
  import { onMount } from 'svelte';
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
  // + Auto-reseed uploaded files and hosted files on startup
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
        try {
          await Promise.race([
            Promise.all([
              invoke('unpublish_all_shared_files'),
              invoke('unpublish_host_advertisement').catch(() => {}),
            ]),
            new Promise((_, reject) => setTimeout(() => reject(new Error('timeout')), 5000)),
          ]);
        } catch {
          // DHT may already be stopped or timed out — proceed with close
        }
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

  // Auto-reseed AFTER Kademlia bootstrap completes, so DHT puts propagate.
  // The Rust backend emits "dht-bootstrap-complete" once bootstrap finishes.
  onMount(async () => {
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      const { listen } = await import('@tauri-apps/api/event');
      let hasAutoReseeded = false;
      await listen('dht-bootstrap-complete', () => {
        if (!hasAutoReseeded) {
          hasAutoReseeded = true;
          autoReseedOnStartup();
        }
      });
    }
  });

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

    // 2. Re-register Drive files that were being seeded
    // Note: Rust struct uses #[serde(rename_all = "camelCase")] so fields are camelCase
    try {
      const addr = $walletAccount?.address;
      if (addr) {
        const driveItems = await invoke<any[]>('drive_list_items', { owner: addr, parentId: null });
        let count = 0;
        for (const item of driveItems) {
          if (!item.seeding || !item.merkleRoot) continue;
          try {
            await invoke('publish_drive_file', {
              owner: addr,
              itemId: item.id,
              protocol: item.protocol || null,
              priceChi: item.priceChi || null,
              walletAddress: item.priceChi && item.priceChi !== '0' ? addr : null,
            });
            count++;
          } catch {
            // File may no longer exist — skip
          }
        }
        if (count > 0) console.log(`✅ Auto-reseeded ${count} Drive file(s)`);
      }
    } catch {
      // Drive manifest may not exist yet — skip
    }

    // 3. Re-register hosted files from active agreements
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

    // 4. Auto-publish hosting marketplace if enabled in settings
    try {
      const currentSettings = $settings;
      if (currentSettings.hostingConfig?.enabled) {
        const addr = $walletAccount?.address;
        if (addr) {
          const { hostingService } = await import('$lib/services/hostingService');
          await hostingService.publishHostAdvertisement(currentSettings.hostingConfig, addr);
          console.log('✅ Auto-published hosting marketplace advertisement');
        }
      }
    } catch {
      // Hosting publish may fail if DHT not ready — skip
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

  // Auto-connect DHT once when user logs in
  let dhtAutoConnected = false;
  $effect(() => {
    if ($isAuthenticated && !dhtAutoConnected) {
      dhtAutoConnected = true;
      dhtService.start().catch((err) => {
        const msg = err instanceof Error ? err.message : String(err);
        if (msg.includes('already running')) {
          networkConnected.set(true);
        }
      });
    }
    if (!$isAuthenticated) {
      dhtAutoConnected = false;
    }
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
