<script lang="ts">
    import './styles/globals.css'
    import { Upload, Download, Shield, Wallet, Globe, BarChart3, Settings, Cpu, Menu, X, Star, Mail, Server } from 'lucide-svelte'
    import UploadPage from './pages/Upload.svelte'
    import DownloadPage from './pages/Download.svelte'
    import ProxyPage from './pages/Proxy.svelte'
    import AccountPage from './pages/Account.svelte'
    import NetworkPage from './pages/Network.svelte'
    import AnalyticsPage from './pages/Analytics.svelte'
    import SettingsPage from './pages/Settings.svelte'
    import MiningPage from './pages/Mining.svelte'
    import ReputationPage from './pages/Reputation.svelte'
    import MessagesPage from './pages/Messages.svelte'
    import RelayPage from './pages/Relay.svelte'
    import NotFound from './pages/NotFound.svelte'
    import ProxySelfTest from './routes/proxy-self-test.svelte'
    import { networkStatus, settings, userLocation, wallet } from './lib/stores'
    import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
    import {onMount, setContext} from 'svelte';
    import { tick } from 'svelte';
    import { get } from 'svelte/store';
    import { setupI18n } from './i18n/i18n';
    import { t } from 'svelte-i18n';
    import SimpleToast from './lib/components/SimpleToast.svelte';
    import { startNetworkMonitoring } from './lib/services/networkService';
    import { fileService } from '$lib/services/fileService';
    import { bandwidthScheduler } from '$lib/services/bandwidthScheduler';
    import { detectUserRegion } from '$lib/services/geolocation';
    import { paymentService } from '$lib/services/paymentService';
    import { listen } from '@tauri-apps/api/event';
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
    
    onMount(() => {
      let stopNetworkMonitoring: () => void = () => {};
      let unlistenSeederPayment: (() => void) | null = null;

      (async () => {
        // Initialize payment service to load wallet and transactions
        paymentService.initialize();

        // Listen for payment notifications from backend
        if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
          try {
            const unlisten = await listen('seeder_payment_received', async (event: any) => {
              const payload = event.payload;
              console.log('💰 Seeder payment notification received:', payload);

              // Only credit the payment if we are the seeder (not the downloader)
              const currentWalletAddress = get(wallet).address;
              const seederAddress = payload.seeder_wallet_address;

              if (!seederAddress || !currentWalletAddress) {
                console.warn('⚠️ Missing wallet addresses, skipping payment credit');
                return;
              }

              // Check if this payment is meant for us (we are the seeder)
              if (currentWalletAddress.toLowerCase() !== seederAddress.toLowerCase()) {
                console.log(`⏭️ Skipping payment credit - not for us. Seeder: ${seederAddress}, Us: ${currentWalletAddress}`);
                return;
              }

              console.log('✅ This payment is for us! Crediting...');

              // Credit the seeder's wallet
              const result = await paymentService.creditSeederPayment(
                payload.file_hash,
                payload.file_name,
                payload.file_size,
                payload.downloader_address,
                payload.transaction_hash
              );

              if (result.success) {
                console.log('✅ Seeder payment credited successfully');
              } else {
                console.error('❌ Failed to credit seeder payment:', result.error);
              }
            });
            unlistenSeederPayment = unlisten;
          } catch (error) {
            console.error('Failed to setup payment listener:', error);
          }
        }

        // setup i18n
        await setupI18n();
        loading = false;

        let storedLocation: string | null = null;
        try {
          const storedSettings = localStorage.getItem('chiralSettings');
          if (storedSettings) {
            const parsed = JSON.parse(storedSettings);
            if (typeof parsed?.userLocation === 'string' && parsed.userLocation) {
              storedLocation = parsed.userLocation;
              userLocation.set(parsed.userLocation);
            }
          }
        } catch (error) {
          console.warn('Failed to load stored user location:', error);
        }
        try {
          const currentLocation = get(userLocation);
          const shouldAutoDetect = !storedLocation || currentLocation === 'US-East';

          if (shouldAutoDetect) {
            const detection = await detectUserRegion();
            const detectedLocation = detection.region.label;
            if (detectedLocation && detectedLocation !== currentLocation) {
              userLocation.set(detectedLocation);
              settings.update((previous) => {
                const next = { ...previous, userLocation: detectedLocation };
                try {
                  const storedSettings = localStorage.getItem('chiralSettings');
                  if (storedSettings) {
                    const parsed = JSON.parse(storedSettings) ?? {};
                    parsed.userLocation = detectedLocation;
                    localStorage.setItem ('chiralSettings', JSON.stringify(parsed));
                  } else {
                    localStorage.setItem('chiralSettings', JSON.stringify(next));
                  }
                } catch (storageError) {
                  console.warn('Failed to persist detected location:', storageError);
                }
                console.log('User region detected via ${detection.source}: ${detectedLocation}');
                return next;
              });
            }
          }
        } catch (error) {
          console.warn('Automatic location detection failed:', error);
        }
        // Initialize backend services (File Transfer, DHT)
        try {
          await fileService.initializeServices();
          console.log('Backend services (File Transfer, DHT) initialized successfully.');
        } catch (error) {
          console.error('Failed to initialize backend services:', error);
        }

        // Start bandwidth scheduler
        bandwidthScheduler.start();
        console.log('Bandwidth scheduler started.');

        // set the currentPage var
        syncFromUrl();

        // Start network monitoring
        stopNetworkMonitoring = startNetworkMonitoring();
      })();

      // popstate - event that tracks history of current tab
      const onPop = () => syncFromUrl();
      window.addEventListener('popstate', onPop);

      // cleanup
      return () => {
        window.removeEventListener('popstate', onPop);
        stopNetworkMonitoring();
        bandwidthScheduler.stop();
        if (unlistenSeederPayment) {
          unlistenSeederPayment();
        }
      };
    })

    setContext('navigation', {
      setCurrentPage: (page: string) => {
        currentPage = page;
      }
    });

    let sidebarCollapsed = false
    let sidebarMenuOpen = false

    // Scroll to top when page changes
    $: if (currentPage) {
        tick().then(() => {
            const mainContent = document.querySelector('#main-content')
            if (mainContent) {
                mainContent.scrollTop = 0
            }
        })
    }

    type MenuItem = {
      id: string;
      label: string;
      icon: typeof Upload;
    };

    let menuItems: MenuItem[] = [];
    $: if (!loading) {
      menuItems = [
        { id: 'download', label: $t('nav.download'), icon: Download },
        { id: 'upload', label: $t('nav.upload'), icon: Upload },
        { id: 'messages', label: 'Messages', icon: Mail },
        { id: 'network', label: $t('nav.network'), icon: Globe },
        { id: 'relay', label: $t('nav.relay'), icon: Server },
        { id: 'mining', label: $t('nav.mining'), icon: Cpu },
        { id: 'proxy', label: $t('nav.proxy'), icon: Shield },
        { id: 'analytics', label: $t('nav.analytics'), icon: BarChart3 },
        { id: 'reputation', label: $t('nav.reputation'), icon: Star },
        { id: 'account', label: $t('nav.account'), icon: Wallet },
        { id: 'settings', label: $t('nav.settings'), icon: Settings },

        ...(import.meta.env.DEV ? [{ id: 'proxy-self-test', label: 'Proxy Self-Test', icon: Shield }] : [])

      ]
    }

    // routes to be used:
    const routes: RouteConfig[] = [
      {
        component: DownloadPage, // root path: '/'
      },
      {
        path: "download",
        component: DownloadPage
      },
      {
        path: "upload",
        component: UploadPage
      },
      {
        path: "messages",
        component: MessagesPage
      },
      {
        path: "network",
        component: NetworkPage
      },
      {
        path: "relay",
        component: RelayPage
      },
      {
        path: "mining",
        component: MiningPage
      },
      {
        path: "proxy",
        component: ProxyPage
      },
      {
        path: "analytics",
        component: AnalyticsPage
      },
      {
        path: "reputation",
        component: ReputationPage
      },
      {
        path: "account",
        component: AccountPage,
      },
      {
        path: "settings",
        component: SettingsPage
      },
      {
        path: "proxy-self-test",
        component: ProxySelfTest
      },
    ]

    
  </script>
  
  <div class="flex bg-background h-full">
    {#if !loading}
    <!-- Desktop Sidebar -->
    <!-- Make the sidebar sticky so it stays visible while the main content scrolls -->
    <div class="hidden md:block {sidebarCollapsed ? 'w-16' : 'w-64'} bg-card border-r transition-all sticky top-0 h-screen">
      <nav class="p-2 space-y-2 h-full overflow-y-auto">
        <!-- Sidebar Header (desktop only) -->
        <div class="flex items-center justify-between px-2 py-2 mb-2">
          <div class="flex items-center">
            <button
              aria-label={$t(sidebarCollapsed ? 'nav.expandSidebar' : 'nav.collapseSidebar')}
              class="p-2 rounded transition-colors hover:bg-gray-100"
              on:click={() => sidebarCollapsed = !sidebarCollapsed}
            >
              <Menu class="h-5 w-5" />
            </button>
            {#if !sidebarCollapsed}
              <span class="ml-2 font-bold text-base">{$t('nav.menu')}</span>
            {/if}
          </div>
  
          {#if !sidebarCollapsed}
            <div class="flex items-center gap-2 text-xs">
              <div class="w-2 h-2 rounded-full {$networkStatus === 'connected' ? 'bg-green-500' : 'bg-red-500'}"></div>
              <span class="text-muted-foreground">{$networkStatus === 'connected' ? $t('nav.connected') : $t('nav.disconnected')}</span>
            </div>
          {:else}
            <div class="w-2 h-2 rounded-full {$networkStatus === 'connected' ? 'bg-green-500' : 'bg-red-500'}"></div>
          {/if}
        </div>
  
        <!-- Sidebar Nav Items -->
        {#each menuItems as item}
          <button
            on:click={() => {
              currentPage = item.id
              goto(`/${item.id}`)
            }}
            class="w-full group"
            aria-current={currentPage === item.id ? 'page' : undefined}
          >
            <div class="flex items-center {sidebarCollapsed ? 'justify-center' : ''} rounded {currentPage === item.id ? 'bg-gray-200' : 'group-hover:bg-gray-100'}">
              <span class="flex items-center justify-center rounded w-10 h-10 relative">
                <svelte:component this={item.icon} class="h-5 w-5" />
                {#if sidebarCollapsed}
                  <span class="tooltip absolute left-full ml-2 top-1/2 -translate-y-1/2 hidden whitespace-nowrap rounded bg-black text-white text-xs px-2 py-1 z-50">{item.label}</span>
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
        on:click={() => sidebarMenuOpen = true}
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
    aria-label={$t('nav.closeSidebarMenu')}
    on:click={() => sidebarMenuOpen = false}
    on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { sidebarMenuOpen = false } }}
  ></div>

  <!-- Sidebar -->
  <div class="fixed top-0 right-0 h-full w-64 bg-white z-50 flex flex-col md:hidden">
    <!-- Sidebar Header -->
    <div class="flex justify-between items-center p-4 border-b">
      <!-- Left side -->
      <span class="font-bold text-base">{$t('nav.menu')}</span>

      <!-- Right side -->
      <div class="flex items-center gap-3">
        <div class="flex items-center gap-2">
          <div class="w-2 h-2 rounded-full {$networkStatus === 'connected' ? 'bg-green-500' : 'bg-red-500'}"></div>
          <span class="text-muted-foreground text-sm">{$networkStatus === 'connected' ? $t('nav.connected') : $t('nav.disconnected')}</span>
        </div>
        <button on:click={() => sidebarMenuOpen = false}>
          <X class="h-6 w-6" />
        </button>
      </div>
    </div>

    <!-- Sidebar Nav Items -->
    <nav class="flex-1 p-4 space-y-2">
      {#each menuItems as item}
        <button
          on:click={() => {
            currentPage = item.id
            goto(`/${item.id}`)
            sidebarMenuOpen = false
          }}
          class="w-full flex items-center rounded px-4 py-3 text-lg hover:bg-gray-100"
          aria-current={currentPage === item.id ? 'page' : undefined}
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
              component: NotFound
            })
          }}
        />
        {/if}
      </div>
    </div>
  </div>
  <!-- add Toast  -->
<SimpleToast />
