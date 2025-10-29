<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Button } from '$lib/components/ui/button.svelte';
  import { Card } from '$lib/components/ui/card.svelte';
  import { Badge } from '$lib/components/ui/badge.svelte';
  import { Dropdown } from '$lib/components/ui/dropDown.svelte';
  import { Globe, Shield, Zap, Home, Wifi, AlertCircle, CheckCircle, Loader } from 'lucide-svelte';

  interface TunnelInfo {
    is_active: boolean;
    public_url?: string;
    local_port: number;
    tunnel_type: string;
    provider: string;
    status: string;
    error_message?: string;
  }

  let tunnelInfo: TunnelInfo = {
    is_active: false,
    local_port: 8080,
    tunnel_type: 'ngrok',
    provider: 'ngrok',
    status: 'stopped'
  };

  let availableProviders: string[] = [];
  let selectedProvider = 'auto';
  let isLoading = false;
  let error = '';

  const providerIcons = {
    ngrok: Globe,
    cloudflared: Zap,
    bore: Wifi,
    localtunnel: Globe,
    self_hosted: Home
  };

  const providerNames = {
    ngrok: 'Ngrok',
    cloudflared: 'Cloudflare Tunnel',
    bore: 'Bore',
    localtunnel: 'Localtunnel',
    self_hosted: 'Self-Hosted'
  };

  onMount(async () => {
    await loadTunnelInfo();
    await loadAvailableProviders();
  });

  async function loadTunnelInfo() {
    try {
      tunnelInfo = await invoke('get_tunnel_info');
    } catch (err) {
      console.error('Failed to load tunnel info:', err);
    }
  }

  async function loadAvailableProviders() {
    try {
      availableProviders = await invoke('get_available_providers');
    } catch (err) {
      console.error('Failed to load providers:', err);
    }
  }

  async function startTunnel() {
    isLoading = true;
    error = '';
    
    try {
      let url: string;
      
      if (selectedProvider === 'auto') {
        url = await invoke('start_tunnel_auto', { port: 8080 });
      } else {
        url = await invoke('start_tunnel', { port: 8080, provider: selectedProvider });
      }
      
      await loadTunnelInfo();
    } catch (err) {
      error = err as string;
      console.error('Failed to start tunnel:', err);
    } finally {
      isLoading = false;
    }
  }

  async function stopTunnel() {
    isLoading = true;
    error = '';
    
    try {
      await invoke('stop_tunnel');
      await loadTunnelInfo();
    } catch (err) {
      error = err as string;
      console.error('Failed to stop tunnel:', err);
    } finally {
      isLoading = false;
    }
  }

  function getStatusIcon() {
    switch (tunnelInfo.status) {
      case 'connected':
        return CheckCircle;
      case 'connecting':
        return Loader;
      case 'failed':
        return AlertCircle;
      default:
        return Wifi;
    }
  }

  function getStatusColor() {
    switch (tunnelInfo.status) {
      case 'connected':
        return 'text-green-500';
      case 'connecting':
        return 'text-blue-500 animate-spin';
      case 'failed':
        return 'text-red-500';
      default:
        return 'text-gray-500';
    }
  }
</script>

<Card class="p-6">
  <div class="flex items-center justify-between mb-4">
    <h3 class="text-lg font-semibold flex items-center gap-2">
      <Globe class="w-5 h-5" />
      HTTP Tunnel Manager
    </h3>
    <Badge variant={tunnelInfo.is_active ? 'default' : 'secondary'}>
      {tunnelInfo.status}
    </Badge>
  </div>

  {#if tunnelInfo.is_active}
    <div class="space-y-4">
      <div class="flex items-center gap-2">
        <svelte:component this={getStatusIcon()} class="w-4 h-4 {getStatusColor()}" />
        <span class="font-medium">Tunnel Active</span>
      </div>
      
      <div class="bg-gray-50 dark:bg-gray-800 p-3 rounded-lg">
        <div class="text-sm text-gray-600 dark:text-gray-400 mb-1">Public URL:</div>
        <div class="font-mono text-sm break-all">
          {tunnelInfo.public_url || 'Loading...'}
        </div>
      </div>
      
      <div class="text-sm text-gray-600 dark:text-gray-400">
        Provider: {providerNames[tunnelInfo.provider] || tunnelInfo.provider}
        • Port: {tunnelInfo.local_port}
      </div>

      {#if tunnelInfo.error_message}
        <div class="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-3">
          <div class="text-sm text-red-600 dark:text-red-400">
            {tunnelInfo.error_message}
          </div>
        </div>
      {/if}

      <Button on:click={stopTunnel} disabled={isLoading} variant="destructive" class="w-full">
        {#if isLoading}
          <Loader class="w-4 h-4 mr-2 animate-spin" />
        {/if}
        Stop Tunnel
      </Button>
    </div>
  {:else}
    <div class="space-y-4">
      <div class="flex items-center gap-2">
        <Wifi class="w-4 h-4 text-gray-500" />
        <span class="text-gray-600 dark:text-gray-400">No active tunnel</span>
      </div>

      <div class="space-y-2">
        <label class="text-sm font-medium">Tunnel Provider:</label>
        <Dropdown
          options={[
            { value: 'auto', label: 'Auto (Best Available)' },
            ...availableProviders.map(provider => ({
              value: provider,
              label: providerNames[provider] || provider
            }))
          ]}
          bind:selected={selectedProvider}
          placeholder="Select provider"
        />
      </div>

      {#if error}
        <div class="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-3">
          <div class="text-sm text-red-600 dark:text-red-400">
            {error}
          </div>
        </div>
      {/if}

      <Button on:click={startTunnel} disabled={isLoading} class="w-full">
        {#if isLoading}
          <Loader class="w-4 h-4 mr-2 animate-spin" />
        {/if}
        Start Tunnel
      </Button>
    </div>
  {/if}

  <div class="mt-4 pt-4 border-t border-gray-200 dark:border-gray-700">
    <div class="text-xs text-gray-500 dark:text-gray-400 space-y-1">
      <div>💡 <strong>Ngrok:</strong> Most reliable, requires account</div>
      <div>⚡ <strong>Cloudflare:</strong> Fast and free</div>
      <div>🔧 <strong>Bore:</strong> Simple and lightweight</div>
      <div>🏠 <strong>Self-Hosted:</strong> Most private, requires port forwarding</div>
    </div>
  </div>
</Card>