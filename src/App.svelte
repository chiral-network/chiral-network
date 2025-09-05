<script lang="ts">
  import './styles/globals.css'
  import { Upload, Download, Shield, Wallet, Globe, BarChart3, Settings, Cpu, Menu } from 'lucide-svelte'
  import UploadPage from './pages/Upload.svelte'
  import DownloadPage from './pages/Download.svelte'
  import ProxyPage from './pages/Proxy.svelte'
  import AccountPage from './pages/Account.svelte'
  import NetworkPage from './pages/Network.svelte'
  import AnalyticsPage from './pages/Analytics.svelte'
  import SettingsPage from './pages/Settings.svelte'
  import MiningPage from './pages/Mining.svelte'
  import { networkStatus } from '$lib/stores'
  
  let currentPage = 'download'
  let sidebarCollapsed = false
  
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
</script>

<div class="flex h-screen bg-background">
  <!-- Sidebar -->
  <div class="{sidebarCollapsed ? 'w-16' : 'w-64'} bg-card border-r transition-all">
    <nav class="p-2 space-y-2">
      <!-- Collapse control + Network Status -->
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

      {#each menuItems as item}
        <button
          on:click={() => currentPage = item.id}
          class="w-full group"
          aria-current={currentPage === item.id ? 'page' : undefined}
        >
          <div class="flex items-center {sidebarCollapsed ? 'justify-center' : ''} rounded {currentPage === item.id ? 'bg-gray-200' : 'group-hover:bg-gray-100'}" style="width: 100%;">
            <span class="flex items-center justify-center transition-colors rounded" style="width: 2.5rem; min-width: 2.5rem; height: 2.5rem;">
              <svelte:component this={item.icon} class="h-5 w-5" style="height: 1.25rem; width: 1.25rem; min-width: 1.25rem; min-height: 1.25rem;" />
            </span>
            {#if !sidebarCollapsed}
              <span class="flex-1 px-2 py-1 rounded transition-colors text-left">{item.label}</span>
            {/if}
          </div>
        </button>
      {/each}
    </nav>
  </div>
  
  <!-- Main Content -->
  <div class="flex-1 overflow-auto">
    <div class="p-6">
      {#if currentPage === 'upload'}
        <UploadPage />
      {:else if currentPage === 'download'}
        <DownloadPage />
      {:else if currentPage === 'network'}
        <NetworkPage />
      {:else if currentPage === 'mining'}
        <MiningPage />
      {:else if currentPage === 'proxy'}
        <ProxyPage />
      {:else if currentPage === 'analytics'}
        <AnalyticsPage />
      {:else if currentPage === 'account'}
        <AccountPage />
      {:else if currentPage === 'settings'}
        <SettingsPage />
      {/if}
    </div>
  </div>
</div>