<script lang="ts">
  import { Download, Upload, Wallet, Globe, Settings, LogOut, Send } from 'lucide-svelte';
  import { goto } from '@mateothegreat/svelte5-router';
  import { isAuthenticated, walletAccount, networkConnected } from '$lib/stores';

  export let currentPage: string = 'download';

  function handleLogout() {
    isAuthenticated.set(false);
    walletAccount.set(null);
    goto('/wallet');
  }

  const navItems = [
    { path: '/download', label: 'Download', icon: Download },
    { path: '/upload', label: 'Upload', icon: Upload },
    { path: '/chiraldrop', label: 'ChiralDrop', icon: Send },
    { path: '/account', label: 'Account', icon: Wallet },
    { path: '/network', label: 'Network', icon: Globe },
    { path: '/settings', label: 'Settings', icon: Settings }
  ];
</script>

<nav class="bg-white shadow-md border-b border-gray-200">
  <div class="max-w-7xl mx-auto px-4">
    <div class="flex items-center justify-between h-16">
      <div class="flex items-center gap-8">
        <div class="flex items-center gap-2">
          <div class="w-8 h-8 bg-gradient-to-br from-blue-500 to-purple-600 rounded-lg"></div>
          <span class="text-xl font-bold">Chiral Network</span>
        </div>
        
        <div class="flex gap-1">
          {#each navItems as item}
            <a
              href={item.path}
              class="flex items-center gap-2 px-4 py-2 rounded-lg transition {currentPage === item.path ? 'bg-blue-50 text-blue-600' : 'text-gray-700 hover:bg-gray-100'}"
            >
              <svelte:component this={item.icon} class="w-4 h-4" />
              <span class="font-medium">{item.label}</span>
            </a>
          {/each}
        </div>
      </div>
      
      <div class="flex items-center gap-4">
        <div class="flex items-center gap-2 px-3 py-1.5 rounded-full {$networkConnected ? 'bg-green-50' : 'bg-red-50'}">
          <div class="w-2 h-2 rounded-full {$networkConnected ? 'bg-green-500' : 'bg-red-500'}"></div>
          <span class="text-sm font-medium {$networkConnected ? 'text-green-700' : 'text-red-700'}">
            {$networkConnected ? 'Connected' : 'Disconnected'}
          </span>
        </div>
        
        <button
          on:click={handleLogout}
          class="flex items-center gap-2 px-4 py-2 text-gray-700 hover:bg-gray-100 rounded-lg transition"
        >
          <LogOut class="w-4 h-4" />
          <span>Logout</span>
        </button>
      </div>
    </div>
  </div>
</nav>
