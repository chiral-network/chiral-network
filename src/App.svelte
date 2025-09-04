<script lang="ts">
  import { onMount } from 'svelte';
  import './styles/globals.css'
  import { Upload, Download, Shield, Wallet, Globe, BarChart3, Settings, Cpu, Sun, Moon } from 'lucide-svelte'
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
  let theme = 'dark'
  let mounted = false;

  onMount(() => {
    const savedTheme = localStorage.getItem('theme')
    if (savedTheme) {
      theme = savedTheme
      document.documentElement.classList.toggle('dark', theme === 'dark')
    }
    mounted = true;
  });

  function toggleTheme() {
    theme = theme === 'dark' ? 'light' : 'dark'
    document.documentElement.classList.toggle('dark', theme === 'dark')
    localStorage.setItem('theme', theme)
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
</script>

<div class="flex h-screen bg-background">
  <!-- Sidebar -->
  <div class="w-64 bg-card border-r transition-all">
    <nav class="p-4 space-y-2">
      <!-- Network Status and Light/Dark mode toggle at top of sidebar -->
      <div class="flex items-center justify-between gap-2 px-3 py-2 mb-4 text-xs">
        <div class="flex items-center gap-2">
          <div class="w-2 h-2 rounded-full {$networkStatus === 'connected' ? 'bg-green-500' : 'bg-red-500'}"></div>
          <span class="text-muted-foreground">{$networkStatus}</span>
        </div>
        <div class="flex items-center gap-2">
          {#if mounted}
            <button on:click={toggleTheme} class="text-muted-foreground hover:text-foreground">
              {#if theme === 'dark'}
                <Sun class="h-4 w-4" />
              {:else}
                <Moon class="h-4 w-4" />
              {/if}
            </button>
          {/if}
        </div>
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