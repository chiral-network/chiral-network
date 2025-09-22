<script lang="ts">
    import './styles/globals.css'
    import { Upload, Download, Shield, Wallet, Globe, BarChart3, Settings, Cpu, Menu, X } from 'lucide-svelte'
    import UploadPage from './pages/Upload.svelte'
    import DownloadPage from './pages/Download.svelte'
    import ProxyPage from './pages/Proxy.svelte'
    import AccountPage from './pages/Account.svelte'
    import NetworkPage from './pages/Network.svelte'
    import AnalyticsPage from './pages/Analytics.svelte'
    import SettingsPage from './pages/Settings.svelte'
    import MiningPage from './pages/Mining.svelte'
    import NotFound from './pages/NotFound.svelte'
    import ProxySelfTest from './routes/proxy-self-test.svelte'
    import { networkStatus } from './lib/stores'
    import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
    import {onMount, setContext} from 'svelte';
    import { tick } from 'svelte';
    import { setupI18n } from './i18n/i18n';
    import { t } from 'svelte-i18n';
    import SimpleToast from './lib/components/SimpleToast.svelte';
    import { startNetworkMonitoring } from './lib/services/networkService';
    import { fileService } from '$lib/services/fileService';
    // gets path name not entire url:
    // ex: http://locatlhost:1420/download -> /download

    const DEV = import.meta.env.DEV;
    
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

      (async () => {
        // setup i18n
        await setupI18n();
        loading = false;

        // Initialize backend services (File Transfer, DHT)
        try {
          await fileService.initializeServices();
          console.log('Backend services (File Transfer, DHT) initialized successfully.');
        } catch (error) {
          console.error('Failed to initialize backend services:', error);
        }

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
      };
    })

    setContext('navigation', {
      setCurrentPage: (page: string) => {
        currentPage = page;
      }
    });

    let sidebarCollapsed = false
    let mobileMenuOpen = false

    // Scroll to top when page changes
    $: if (currentPage) {
        tick().then(() => {
            const mainContent = document.querySelector('.flex-1.overflow-auto')
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
        { id: 'network', label: $t('nav.network'), icon: Globe },
        { id: 'mining', label: $t('nav.mining'), icon: Cpu },
        { id: 'proxy', label: $t('nav.proxy'), icon: Shield },
        { id: 'analytics', label: $t('nav.analytics'), icon: BarChart3 },
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
        path: "network",
        component: NetworkPage
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
  
  <div class="flex h-screen bg-background">
    {#if !loading}
    <!-- Desktop Sidebar -->
    <div class="hidden md:block {sidebarCollapsed ? 'w-16' : 'w-64'} bg-card border-r transition-all">
      <nav class="p-2 space-y-2">
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
  
    <!-- Mobile Menu Button -->
    <div class="absolute top-2 right-2 md:hidden">
      <button
        class="p-2 rounded bg-card shadow"
        on:click={() => mobileMenuOpen = true}
      >
        <Menu class="h-6 w-6" />
      </button>
    </div>
  
<!-- Mobile Menu Overlay -->
{#if mobileMenuOpen}
  <!-- Backdrop -->
  <div
    class="fixed inset-0 bg-black bg-opacity-50 z-40 md:hidden"
    role="button"
    tabindex="0"
    aria-label={$t('nav.closeMobileMenu')}
    on:click={() => mobileMenuOpen = false}
    on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { mobileMenuOpen = false } }}
  ></div>

  <!-- Sidebar -->
  <div class="fixed top-0 right-0 h-full w-64 bg-white z-50 flex flex-col md:hidden">
    <!-- Mobile Header -->
    <div class="flex justify-between items-center p-4 border-b">
      <!-- Left side -->
      <span class="font-bold text-base">{$t('nav.menu')}</span>

      <!-- Right side -->
      <div class="flex items-center gap-3">
        <div class="flex items-center gap-2">
          <div class="w-2 h-2 rounded-full {$networkStatus === 'connected' ? 'bg-green-500' : 'bg-red-500'}"></div>
          <span class="text-muted-foreground text-sm">{$networkStatus === 'connected' ? $t('nav.connected') : $t('nav.disconnected')}</span>
        </div>
        <button on:click={() => mobileMenuOpen = false}>
          <X class="h-6 w-6" />
        </button>
      </div>
    </div>

    <!-- Mobile Nav Items -->
    <nav class="flex-1 p-4 space-y-2">
      {#each menuItems as item}
        <button
          on:click={() => {
            currentPage = item.id
            goto(`/${item.id}`)
            mobileMenuOpen = false
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
    <div class="flex-1 overflow-auto">
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
  