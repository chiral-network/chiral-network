<script lang="ts">
  import './styles/globals.css'
  import { Upload, Download, Shield, Wallet, Globe, BarChart3, Settings, Cpu } from 'lucide-svelte'
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
  <div class="w-64 bg-card border-r transition-all">
    <nav class="p-4 space-y-2">
      <!-- Network Status at top of sidebar -->
      <div class="flex items-center gap-2 px-3 py-2 mb-4 text-xs">
        <div class="w-2 h-2 rounded-full {$networkStatus === 'connected' ? 'bg-green-500' : 'bg-red-500'}"></div>
        <span class="text-muted-foreground">{$networkStatus}</span>
      </div>
      
      {#each menuItems as item}
        <button
          on:click={() => currentPage = item.id}
          class="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm transition-colors {currentPage === item.id ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/50'}"
        >
          <svelte:component this={item.icon} class="h-4 w-4" />
          {item.label}
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