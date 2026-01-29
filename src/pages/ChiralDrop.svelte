<script lang="ts">
  import { t } from 'svelte-i18n';
  import { Droplet, RefreshCw, Upload as UploadIcon, Users, DollarSign, X, Check, FileText, Zap } from 'lucide-svelte';
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import Button from '$lib/components/ui/button.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import { showToast } from '$lib/toast';

  interface NearbyUser {
    peerId: string;
    alias: string;
    distance: number; // For animation positioning
    angle: number; // For circular layout
  }

  interface TransferRequest {
    from: string;
    fromAlias: string;
    fileName: string;
    fileSize: number;
    price: number;
    requestId: string;
    fileHash: string;
  }

  interface ChiralDropNotification {
    notification_type: string;
    sender_peer_id: string;
    sender_alias: string;
    file_name: string;
    file_size: number;
    price: number;
    file_hash: string | null;
  }

  let currentAlias = '';
  let nearbyUsers: NearbyUser[] = [];
  let selectedFile: { name: string; path: string; size: number } | null = null;
  let selectedRecipient: NearbyUser | null = null;
  let transferPrice = 0;
  let isFreeTransfer = true;
  let isDiscovering = false;
  let isSending = false;
  let discoveryInterval: number | null = null;
  let notificationInterval: number | null = null;
  let incomingRequest: TransferRequest | null = null;
  let showPriceInput = false;

  // Generate a random alias for the user
  async function generateAlias() {
    try {
      const alias = await invoke<string>('generate_alias');
      currentAlias = alias;
      showToast($t('chiraldrop.aliasGenerated', { values: { alias } }), 'success');
    } catch (error) {
      console.error('Failed to generate alias:', error);
      showToast($t('chiraldrop.aliasError'), 'error');
    }
  }

  // Discover nearby users on the network
  async function discoverUsers() {
    try {
      isDiscovering = true;
      const users = await invoke<Array<{ peer_id: string; alias: string }>>('discover_nearby_users');
      
      // Position users in a wave/circular pattern
      nearbyUsers = users.map((user, index) => {
        const totalUsers = users.length;
        const angle = (index / totalUsers) * 2 * Math.PI;
        const distance = 0.6 + Math.random() * 0.3; // Random distance between 0.6 and 0.9
        
        return {
          peerId: user.peer_id,
          alias: user.alias,
          distance,
          angle,
        };
      });
    } catch (error) {
      console.error('Failed to discover users:', error);
    } finally {
      isDiscovering = false;
    }
  }

  // Select a file to send
  async function selectFile() {
    try {
      const selected = await open({
        multiple: false,
        title: 'Select File to Send',
      });

      if (selected && typeof selected === 'string') {
        const fileName = selected.split(/[\\/]/).pop() || 'Unknown';
        // Get file size
        try {
          const size = await invoke<number>('get_file_size', { filePath: selected });
          selectedFile = {
            name: fileName,
            path: selected,
            size,
          };
        } catch (error) {
          console.error('Failed to get file size:', error);
          selectedFile = {
            name: fileName,
            path: selected,
            size: 0,
          };
        }
      }
    } catch (error) {
      console.error('Failed to select file:', error);
    }
  }

  // Send file to selected recipient
  async function sendFile() {
    if (!selectedFile || !selectedRecipient) return;

    try {
      isSending = true;
      const price = isFreeTransfer ? 0 : transferPrice;

      await invoke('send_chiraldrop_file', {
        recipientPeerId: selectedRecipient.peerId,
        filePath: selectedFile.path,
        fileName: selectedFile.name,
        price,
      });

      showToast(
        $t('chiraldrop.sendSuccess', { 
          values: { 
            file: selectedFile.name, 
            recipient: selectedRecipient.alias 
          } 
        }),
        'success'
      );

      // Reset
      selectedFile = null;
      selectedRecipient = null;
      transferPrice = 0;
      isFreeTransfer = true;
      showPriceInput = false;
    } catch (error) {
      console.error('Failed to send file:', error);
      showToast($t('chiraldrop.sendError'), 'error');
    } finally {
      isSending = false;
    }
  }

  // Check for incoming transfer notifications
  async function checkNotifications() {
    try {
      const notifications = await invoke<ChiralDropNotification[]>('check_chiraldrop_notifications');
      
      if (notifications.length > 0) {
        const notification = notifications[0]; // Handle first notification
        
        if (notification.notification_type === 'transfer_request') {
          incomingRequest = {
            from: notification.sender_peer_id,
            fromAlias: notification.sender_alias,
            fileName: notification.file_name,
            fileSize: notification.file_size,
            price: notification.price,
            requestId: notification.sender_peer_id + '_' + Date.now(),
            fileHash: notification.file_hash || '',
          };
        }
      }
    } catch (error) {
      console.error('Failed to check notifications:', error);
    }
  }

  // Accept incoming transfer
  async function acceptTransfer() {
    if (!incomingRequest) return;

    try {
      const savePath = await open({
        directory: true,
        title: 'Select Download Location',
      });

      if (savePath && typeof savePath === 'string') {
        const fullPath = `${savePath}/${incomingRequest.fileName}`;
        
        await invoke('accept_chiraldrop_transfer', {
          requestId: incomingRequest.requestId,
          savePath: fullPath,
          fileHash: incomingRequest.fileHash,
        });

        showToast($t('chiraldrop.acceptSuccess'), 'success');
        incomingRequest = null;
      }
    } catch (error) {
      console.error('Failed to accept transfer:', error);
      showToast($t('chiraldrop.acceptError'), 'error');
    }
  }

  // Decline incoming transfer
  async function declineTransfer() {
    if (!incomingRequest) return;

    try {
      await invoke('decline_chiraldrop_transfer', {
        requestId: incomingRequest.requestId,
      });

      showToast($t('chiraldrop.declined'), 'info');
      incomingRequest = null;
    } catch (error) {
      console.error('Failed to decline transfer:', error);
    }
  }

  // Format file size
  function formatFileSize(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return Math.round((bytes / Math.pow(k, i)) * 100) / 100 + ' ' + sizes[i];
  }

  // Start discovery interval
  function startDiscovery() {
    discoverUsers();
    discoveryInterval = window.setInterval(discoverUsers, 5000);
  }

  // Stop discovery interval
  function stopDiscovery() {
    if (discoveryInterval !== null) {
      clearInterval(discoveryInterval);
      discoveryInterval = null;
    }
  }

  onMount(async () => {
    await generateAlias();
    startDiscovery();
    
    // Start polling for incoming transfer notifications
    checkNotifications();
    notificationInterval = window.setInterval(checkNotifications, 3000);
  });

  onDestroy(() => {
    stopDiscovery();
    if (notificationInterval !== null) {
      clearInterval(notificationInterval);
      notificationInterval = null;
    }
  });
</script>

<div class="space-y-6">
  <!-- Page Header -->
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold flex items-center gap-2">
        <Droplet class="h-8 w-8 text-blue-500" />
        ChiralDrop
      </h1>
      <p class="text-muted-foreground mt-1">
        {$t('chiraldrop.subtitle')}
      </p>
    </div>
  </div>

  <!-- User Alias Card -->
  <div class="rounded-lg border border-border bg-card p-6">
    <div class="flex items-center justify-between">
      <div>
        <Label class="text-sm text-muted-foreground">{$t('chiraldrop.yourAlias')}</Label>
        <div class="flex items-center gap-3 mt-2">
          <Badge variant="outline" class="text-lg px-4 py-2 font-mono">
            {currentAlias || $t('chiraldrop.generatingAlias')}
          </Badge>
          <Badge variant="secondary" class="flex items-center gap-1">
            <Users class="h-3 w-3" />
            {nearbyUsers.length} {$t('chiraldrop.nearby')}
          </Badge>
        </div>
      </div>
      <Button
        variant="outline"
        size="sm"
        on:click={generateAlias}
        class="flex items-center gap-2"
      >
        <RefreshCw class="h-4 w-4" />
        {$t('chiraldrop.newAlias')}
      </Button>
    </div>
  </div>

  <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
    <!-- Discovery Radar -->
    <div class="rounded-lg border border-border bg-card p-6">
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-xl font-semibold flex items-center gap-2">
          <Zap class="h-5 w-5" />
          {$t('chiraldrop.nearbyUsers')}
        </h2>
        <Button
          variant={isDiscovering ? 'secondary' : 'outline'}
          size="sm"
          on:click={discoverUsers}
          disabled={isDiscovering}
        >
          <RefreshCw class="h-4 w-4 {isDiscovering ? 'animate-spin' : ''}" />
        </Button>
      </div>

      <!-- Radar/Wave Visualization -->
      <div class="relative w-full aspect-square bg-gradient-to-br from-blue-50 to-purple-50 dark:from-gray-800 dark:to-gray-900 rounded-lg overflow-hidden">
        <!-- Center (You) -->
        <div class="absolute top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2 z-10">
          <div class="w-16 h-16 rounded-full bg-blue-500 flex items-center justify-center text-white font-bold shadow-lg">
            {$t('chiraldrop.you')}
          </div>
        </div>

        <!-- Ripple waves -->
        <div class="absolute top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2 w-32 h-32 rounded-full border-2 border-blue-300 opacity-50 animate-ping"></div>
        <div class="absolute top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2 w-48 h-48 rounded-full border-2 border-blue-200 opacity-30"></div>
        <div class="absolute top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2 w-64 h-64 rounded-full border-2 border-blue-100 opacity-20"></div>

        <!-- Nearby Users -->
        {#each nearbyUsers as user}
          {@const x = 50 + user.distance * 45 * Math.cos(user.angle)}
          {@const y = 50 + user.distance * 45 * Math.sin(user.angle)}
          <button
            class="absolute w-12 h-12 rounded-full bg-gradient-to-br from-purple-400 to-purple-600 flex items-center justify-center text-white font-semibold shadow-lg hover:scale-110 transition-transform cursor-pointer z-20 {selectedRecipient?.peerId === user.peerId ? 'ring-4 ring-green-400' : ''}"
            style="left: {x}%; top: {y}%; transform: translate(-50%, -50%);"
            on:click={() => selectedRecipient = user}
            title={user.alias}
          >
            {user.alias.substring(0, 2).toUpperCase()}
          </button>
        {/each}

        {#if nearbyUsers.length === 0 && !isDiscovering}
          <div class="absolute inset-0 flex items-center justify-center">
            <p class="text-muted-foreground text-center px-4">
              {$t('chiraldrop.noUsers')}
            </p>
          </div>
        {/if}
      </div>

      {#if selectedRecipient}
        <div class="mt-4 p-3 rounded-lg bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800">
          <p class="text-sm font-medium text-green-900 dark:text-green-100">
            {$t('chiraldrop.selectedRecipient')}: <span class="font-mono">{selectedRecipient.alias}</span>
          </p>
        </div>
      {/if}
    </div>

    <!-- File Transfer Panel -->
    <div class="rounded-lg border border-border bg-card p-6">
      <h2 class="text-xl font-semibold mb-4 flex items-center gap-2">
        <UploadIcon class="h-5 w-5" />
        {$t('chiraldrop.sendFile')}
      </h2>

      <div class="space-y-4">
        <!-- File Selection -->
        <div>
          <Label>{$t('chiraldrop.selectFile')}</Label>
          <div class="mt-2">
            {#if selectedFile}
              <div class="flex items-center justify-between p-3 rounded-lg bg-gray-50 dark:bg-gray-800 border">
                <div class="flex items-center gap-2">
                  <FileText class="h-4 w-4 text-muted-foreground" />
                  <div>
                    <p class="font-medium text-sm">{selectedFile.name}</p>
                    <p class="text-xs text-muted-foreground">{formatFileSize(selectedFile.size)}</p>
                  </div>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  on:click={() => selectedFile = null}
                >
                  <X class="h-4 w-4" />
                </Button>
              </div>
            {:else}
              <Button
                variant="outline"
                class="w-full"
                on:click={selectFile}
              >
                <UploadIcon class="h-4 w-4 mr-2" />
                {$t('chiraldrop.chooseFile')}
              </Button>
            {/if}
          </div>
        </div>

        <!-- Pricing Options -->
        <div>
          <Label>{$t('chiraldrop.transferType')}</Label>
          <div class="mt-2 space-y-2">
            <button
              class="w-full p-3 rounded-lg border-2 transition-colors {isFreeTransfer ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20' : 'border-gray-200 dark:border-gray-700'}"
              on:click={() => {
                isFreeTransfer = true;
                showPriceInput = false;
                transferPrice = 0;
              }}
            >
              <div class="flex items-center gap-2">
                <div class="w-5 h-5 rounded-full border-2 {isFreeTransfer ? 'border-blue-500 bg-blue-500' : 'border-gray-300'} flex items-center justify-center">
                  {#if isFreeTransfer}
                    <Check class="h-3 w-3 text-white" />
                  {/if}
                </div>
                <span class="font-medium">{$t('chiraldrop.free')}</span>
              </div>
            </button>

            <button
              class="w-full p-3 rounded-lg border-2 transition-colors {!isFreeTransfer ? 'border-green-500 bg-green-50 dark:bg-green-900/20' : 'border-gray-200 dark:border-gray-700'}"
              on:click={() => {
                isFreeTransfer = false;
                showPriceInput = true;
              }}
            >
              <div class="flex items-center gap-2">
                <div class="w-5 h-5 rounded-full border-2 {!isFreeTransfer ? 'border-green-500 bg-green-500' : 'border-gray-300'} flex items-center justify-center">
                  {#if !isFreeTransfer}
                    <Check class="h-3 w-3 text-white" />
                  {/if}
                </div>
                <span class="font-medium">{$t('chiraldrop.paid')}</span>
              </div>
            </button>

            {#if showPriceInput}
              <div class="pl-7">
                <Label class="text-sm">{$t('chiraldrop.setPrice')}</Label>
                <div class="flex items-center gap-2 mt-1">
                  <DollarSign class="h-4 w-4 text-muted-foreground" />
                  <Input
                    type="number"
                    min="0"
                    step="0.01"
                    bind:value={transferPrice}
                    placeholder="0.00"
                    class="flex-1"
                  />
                  <span class="text-sm text-muted-foreground">ETC</span>
                </div>
              </div>
            {/if}
          </div>
        </div>

        <!-- Send Button -->
        <Button
          class="w-full"
          disabled={!selectedFile || !selectedRecipient || isSending || (!isFreeTransfer && transferPrice <= 0)}
          on:click={sendFile}
        >
          {#if isSending}
            <RefreshCw class="h-4 w-4 mr-2 animate-spin" />
            {$t('chiraldrop.sending')}
          {:else}
            <Droplet class="h-4 w-4 mr-2" />
            {$t('chiraldrop.send')}
          {/if}
        </Button>

        {#if !selectedRecipient && selectedFile}
          <p class="text-sm text-amber-600 dark:text-amber-400 text-center">
            {$t('chiraldrop.selectRecipientHint')}
          </p>
        {/if}
      </div>
    </div>
  </div>

  <!-- Incoming Transfer Request Modal -->
  {#if incomingRequest}
    <div class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
      <div class="bg-white dark:bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
        <h3 class="text-xl font-bold mb-4">{$t('chiraldrop.incomingTransfer')}</h3>
        
        <div class="space-y-3 mb-6">
          <div class="flex items-center justify-between">
            <span class="text-sm text-muted-foreground">{$t('chiraldrop.from')}:</span>
            <span class="font-mono font-medium">{incomingRequest.fromAlias}</span>
          </div>
          
          <div class="flex items-center justify-between">
            <span class="text-sm text-muted-foreground">{$t('chiraldrop.file')}:</span>
            <span class="font-medium">{incomingRequest.fileName}</span>
          </div>
          
          <div class="flex items-center justify-between">
            <span class="text-sm text-muted-foreground">{$t('chiraldrop.size')}:</span>
            <span>{formatFileSize(incomingRequest.fileSize)}</span>
          </div>
          
          {#if incomingRequest.price > 0}
            <div class="flex items-center justify-between p-3 rounded-lg bg-green-50 dark:bg-green-900/20">
              <span class="text-sm font-medium">{$t('chiraldrop.price')}:</span>
              <span class="font-bold text-green-600 dark:text-green-400">{incomingRequest.price} ETC</span>
            </div>
          {:else}
            <div class="p-3 rounded-lg bg-blue-50 dark:bg-blue-900/20">
              <span class="text-sm font-medium text-blue-600 dark:text-blue-400">{$t('chiraldrop.freeTransfer')}</span>
            </div>
          {/if}
        </div>

        <div class="flex gap-3">
          <Button
            variant="outline"
            class="flex-1"
            on:click={declineTransfer}
          >
            <X class="h-4 w-4 mr-2" />
            {$t('chiraldrop.decline')}
          </Button>
          <Button
            class="flex-1 bg-green-600 hover:bg-green-700"
            on:click={acceptTransfer}
          >
            <Check class="h-4 w-4 mr-2" />
            {$t('chiraldrop.accept')}
          </Button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  @keyframes ping {
    75%, 100% {
      transform: translate(-50%, -50%) scale(2);
      opacity: 0;
    }
  }

  .animate-ping {
    animation: ping 2s cubic-bezier(0, 0, 0.2, 1) infinite;
  }
</style>
