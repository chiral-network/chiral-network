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
    import { networkStatus } from '$lib/stores'
    import { Router, type RouteConfig, goto } from '@mateothegreat/svelte5-router';
    import {onMount} from 'svelte';
    import { tick } from 'svelte'

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
    
    onMount(()=>{
      // set the currentPage var
      syncFromUrl();

      // popstate - event that tracks history of current tab 
      // (i.e. clicking on new url or going back)
      const onPop = () => syncFromUrl();

      // triggers onPop to make sure currentPage variable is up to date
      window.addEventListener('popstate', onPop);

      //  cleanup when component unmounts for removing duplicate event listeners. 
      return () => window.removeEventListener('popstate', onPop);
    })
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

    const menuItems = [
      { id: 'download', label: 'Download', icon: Download },
      { id: 'upload', label: 'Upload', icon: Upload },
      { id: 'network', label: 'Network', icon: Globe },
      { id: 'mining', label: 'Mining', icon: Cpu },
      { id: 'proxy', label: 'Proxy', icon: Shield },
      { id: 'analytics', label: 'Analytics', icon: BarChart3 },
      { id: 'account', label: 'Account', icon: Wallet },
      { id: 'settings', label: 'Settings', icon: Settings },
    ]

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
    ]

    
  </script>
  
  <div class="flex h-screen bg-background">
    <!-- Desktop Sidebar -->
    <div class="hidden md:block {sidebarCollapsed ? 'w-16' : 'w-64'} bg-card border-r transition-all">
      <nav class="p-2 space-y-2">
        <!-- Sidebar Header (desktop only) -->
        <div class="flex items-center justify-between px-2 py-2 mb-2">
          <div class="flex items-center">
            <button
              aria-label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
              class="p-2 rounded transition-colors hover:bg-gray-100"
              on:click={() => sidebarCollapsed = !sidebarCollapsed}
            >
              <Menu class="h-5 w-5" />
            </button>
            {#if !sidebarCollapsed}
              <span class="ml-2 font-bold text-base">Menu</span>
            {/if}
          </div>
  
          {#if !sidebarCollapsed}
            <div class="flex items-center gap-2 text-xs">
              <div class="w-2 h-2 rounded-full {$networkStatus === 'connected' ? 'bg-green-500' : 'bg-red-500'}"></div>
              <span class="text-muted-foreground">{$networkStatus}</span>
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
              <span class="flex items-center justify-center rounded w-10 h-10">
                <svelte:component this={item.icon} class="h-5 w-5" />
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
    aria-label="Close mobile menu"
    on:click={() => mobileMenuOpen = false}
    on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { mobileMenuOpen = false } }}
  ></div>

  <!-- Sidebar -->
  <div class="fixed top-0 right-0 h-full w-64 bg-white z-50 flex flex-col md:hidden">
    <!-- Mobile Header -->
    <div class="flex justify-between items-center p-4 border-b">
      <!-- Left side -->
      <span class="font-bold text-base">Menu</span>

      <!-- Right side -->
      <div class="flex items-center gap-3">
        <div class="flex items-center gap-2">
          <div class="w-2 h-2 rounded-full {$networkStatus === 'connected' ? 'bg-green-500' : 'bg-red-500'}"></div>
          <span class="text-muted-foreground text-sm">{$networkStatus}</span>
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

    <!-- Main Content -->
    <div class="flex-1 overflow-auto">
      <div class="p-6">
        <!-- <Router {routes} /> -->
         
        <Router
          {routes}
          statuses={{
            // visiting non-path default to NotFound page
            404: () => ({
              component: NotFound
            })
          }}
        />
      </div>
    </div>
  </div>
