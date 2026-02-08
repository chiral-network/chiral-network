<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Send, X, Check, History, User, FileIcon, Upload, Coins } from 'lucide-svelte';
  import {
    userAlias,
    nearbyPeers,
    pendingTransfers,
    transferHistory,
    selectedPeer,
    incomingPendingTransfers,
    localPeerId,
    setLocalPeerId,
    addNearbyPeer,
    removeNearbyPeer,
    selectPeer,
    addPendingTransfer,
    acceptTransfer,
    declineTransfer,
    updateTransferStatus,
    generateTransferId,
    formatFileSize,
    formatPriceWei,
    type NearbyPeer,
    type FileTransfer
  } from '$lib/chiralDropStore';
  import { aliasFromPeerId } from '$lib/aliasService';
  import { peers, walletAccount } from '$lib/stores';
  import { get } from 'svelte/store';
  import { toasts } from '$lib/toastStore';
  import { dhtService } from '$lib/dhtService';
  import { logger } from '$lib/logger';
  const log = logger('ChiralDrop');

  // Check if running in Tauri environment (reactive)
  let isTauri = $state(false);
  
  // Check Tauri availability
  function checkTauriAvailability(): boolean {
    return typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);
  }

  let showHistory = $state(false);
  let fileInput = $state<HTMLInputElement>();
  let animationFrame: number;
  let time = $state(0);
  let sendPrice = $state('');
  let unlistenPeerDiscovered: (() => void) | null = null;
  let unlistenFileReceived: (() => void) | null = null;
  let unlistenFileComplete: (() => void) | null = null;
  let unlistenFileReceivedComplete: (() => void) | null = null;
  let unlistenPaidRequest: (() => void) | null = null;
  let unlistenConnectionEstablished: (() => void) | null = null;

  // Wave animation
  function animate() {
    time = Date.now() / 1000;
    animationFrame = requestAnimationFrame(animate);
  }

  // Calculate wave offset for a peer
  function getWaveOffset(peer: NearbyPeer): number {
    return Math.sin(time * 2 + peer.wavePhase) * 5;
  }

  onMount(async () => {
    animate();
    
    // Check Tauri availability
    isTauri = checkTauriAvailability();

    // Get our local peer ID for consistent alias
    if (isTauri) {
      try {
        const peerId = await dhtService.getPeerId();
        if (peerId) {
          setLocalPeerId(peerId);
        }
      } catch (error) {
        log.warn('Failed to get local peer ID:', error);
      }
    }

    // Only set up Tauri listeners if in Tauri environment
    if (isTauri) {
      try {
        const { listen } = await import('@tauri-apps/api/event');

        // Listen for peer discovery events
        unlistenPeerDiscovered = await listen<any[]>('peer-discovered', (event) => {
          const discoveredPeers = event.payload;
          discoveredPeers.forEach((peer: { id: string }) => {
            addNearbyPeer(peer.id);
          });
        });

        // Listen for incoming file transfer requests
        unlistenFileReceived = await listen<any>('file-transfer-request', (event) => {
          const { fromPeerId, fileName, fileSize, transferId } = event.payload;
          const fromAlias = aliasFromPeerId(fromPeerId);
          const toAlias = $userAlias;

          addPendingTransfer({
            id: transferId,
            fileName,
            fileSize,
            fromPeerId,
            fromAlias,
            toPeerId: $localPeerId || '',
            toAlias,
            status: 'pending',
            direction: 'incoming',
            timestamp: Date.now()
          });

          toasts.show(`${fromAlias.displayName} wants to send you a file: ${fileName}`, 'info');
        });

        // Listen for file transfer completion (outgoing)
        unlistenFileComplete = await listen<any>('file-transfer-complete', (event) => {
          const { transferId, status } = event.payload;
          if (status === 'completed') {
            updateTransferStatus(transferId, 'completed');
          } else if (status === 'declined') {
            updateTransferStatus(transferId, 'declined');
          }
        });

        // Listen for file received (incoming complete)
        unlistenFileReceivedComplete = await listen<any>('file-received', (event) => {
          const { transferId, fileName, fromPeerId, filePath } = event.payload;
          const fromAlias = aliasFromPeerId(fromPeerId);
          toasts.show(`File "${fileName}" saved to ${filePath}`, 'success', 8000);
          updateTransferStatus(transferId, 'completed');
        });

        // Listen for connection-established events (catches non-mDNS peers too)
        unlistenConnectionEstablished = await listen<string>('connection-established', (event) => {
          const peerId = event.payload;
          addNearbyPeer(peerId);
        });

        // Listen for paid file transfer requests (sent via chiraldrop with price > 0)
        unlistenPaidRequest = await listen<any>('chiraldrop-paid-request', (event) => {
          const { transferId, fromPeerId, fileName, fileHash, fileSize, priceWei, senderWallet } = event.payload;
          const fromAlias = aliasFromPeerId(fromPeerId);
          const toAlias = $userAlias;

          addPendingTransfer({
            id: transferId,
            fileName,
            fileSize: fileSize || 0,
            fromPeerId,
            fromAlias,
            toPeerId: $localPeerId || '',
            toAlias,
            status: 'pending',
            direction: 'incoming',
            timestamp: Date.now(),
            priceWei,
            senderWallet,
            fileHash
          });

          const priceDisplay = formatPriceWei(priceWei);
          toasts.show(`${fromAlias.displayName} wants to send you "${fileName}" for ${priceDisplay}`, 'info');
        });
      } catch (error) {
        log.warn('Failed to set up Tauri event listeners:', error);
      }
    }

    // Sync with existing peers from the network
    const currentPeers = $peers;
    currentPeers.forEach((peer) => {
      addNearbyPeer(peer.id);
    });
  });

  onDestroy(() => {
    if (animationFrame) {
      cancelAnimationFrame(animationFrame);
    }
    if (unlistenPeerDiscovered) {
      unlistenPeerDiscovered();
    }
    if (unlistenFileReceived) {
      unlistenFileReceived();
    }
    if (unlistenFileComplete) {
      unlistenFileComplete();
    }
    if (unlistenFileReceivedComplete) {
      unlistenFileReceivedComplete();
    }
    if (unlistenPaidRequest) {
      unlistenPaidRequest();
    }
    if (unlistenConnectionEstablished) {
      unlistenConnectionEstablished();
    }
  });

  // Subscribe to peers store updates — sync adds AND removals
  // Use get() for nearbyPeers to avoid creating a reactive dependency
  // (writing to nearbyPeers inside an effect that reads it would cause an infinite loop)
  $effect(() => {
    const currentPeerIds = new Set($peers.map((p) => p.id));

    // Add any new peers
    $peers.forEach((peer) => {
      addNearbyPeer(peer.id);
    });

    // Remove peers no longer in the peers store
    const nearby = get(nearbyPeers);
    nearby.forEach((np) => {
      if (!currentPeerIds.has(np.peerId)) {
        removeNearbyPeer(np.peerId);
      }
    });
  });

  function handlePeerClick(peer: NearbyPeer) {
    selectPeer(peer);
  }

  function handleFileSelect(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file || !$selectedPeer) return;

    sendFile(file, $selectedPeer);
    input.value = '';
  }

  async function sendFile(file: File, peer: NearbyPeer) {
    const transferId = generateTransferId();
    const fromAlias = $userAlias;
    const toAlias = peer.alias;
    const price = sendPrice.trim();
    const isPaid = price !== '' && parseFloat(price) > 0;

    // Validate wallet for paid transfers
    if (isPaid && !$walletAccount) {
      toasts.show('Connect your wallet to set a price on files', 'error');
      return;
    }

    // Add to pending transfers
    addPendingTransfer({
      id: transferId,
      fileName: file.name,
      fileSize: file.size,
      fromPeerId: $localPeerId || '',
      fromAlias,
      toPeerId: peer.peerId,
      toAlias,
      status: 'pending',
      direction: 'outgoing',
      timestamp: Date.now()
    });

    const tauriAvailable = checkTauriAvailability();
    if (!tauriAvailable) {
      toasts.show('File transfer requires the desktop app', 'error');
      updateTransferStatus(transferId, 'failed');
      selectPeer(null);
      return;
    }

    try {
      const { invoke } = await import('@tauri-apps/api/core');

      if (isPaid) {
        // Paid transfer flow:
        // 1. Read file bytes and publish via publish_file_data (hashes, stores in memory, registers for chunked serving)
        // 2. Send metadata-only request via file_transfer protocol with pricing info
        //    (receiver will download via chunked protocol with payment handshake)

        const buffer = await file.arrayBuffer();
        const bytes = Array.from(new Uint8Array(buffer));

        // Convert CHR to wei
        const priceParts = price.split('.');
        const whole = BigInt(priceParts[0] || '0');
        const fracStr = (priceParts[1] || '').padEnd(18, '0').slice(0, 18);
        const frac = BigInt(fracStr);
        const priceWei = (whole * BigInt(1e18) + frac).toString();

        // Publish file data to get hash and register for chunked serving
        const publishResult = await invoke<{ merkleRoot: string }>('publish_file_data', {
          fileName: file.name,
          fileData: bytes,
          priceChr: price,
          walletAddress: $walletAccount!.address
        });

        const fileHash = publishResult.merkleRoot;

        // Send metadata-only transfer request (empty file data) with pricing
        await invoke('send_file', {
          peerId: peer.peerId,
          fileName: file.name,
          fileData: [],
          transferId,
          priceWei,
          senderWallet: $walletAccount!.address,
          fileHash,
          fileSize: file.size
        });

        updateTransferStatus(transferId, 'completed');
        toasts.show(`Paid file offer sent to ${toAlias.displayName} (${price} CHR)`, 'success');
      } else {
        // Free transfer: send file data directly (existing behavior)
        const buffer = await file.arrayBuffer();
        const bytes = Array.from(new Uint8Array(buffer));

        await invoke('send_file', {
          peerId: peer.peerId,
          fileName: file.name,
          fileData: bytes,
          transferId
        });

        updateTransferStatus(transferId, 'completed');
        toasts.show(`File sent to ${toAlias.displayName}`, 'success');
      }
    } catch (error) {
      log.error('Failed to send file:', error);
      updateTransferStatus(transferId, 'failed');
      toasts.show(`Failed to send file: ${error}`, 'error');
    }

    sendPrice = '';
    selectPeer(null);
  }

  async function handleAccept(transfer: FileTransfer) {
    const tauriAvailable = checkTauriAvailability();
    if (!tauriAvailable) {
      toasts.show('File transfer requires the desktop app', 'error');
      return;
    }

    try {
      const { invoke } = await import('@tauri-apps/api/core');

      const isPaid = transfer.priceWei && transfer.priceWei !== '0' && BigInt(transfer.priceWei) > 0;

      if (isPaid) {
        // Paid transfer: download via chunked protocol with payment handshake
        if (!$walletAccount) {
          toasts.show('Connect your wallet to accept paid file transfers', 'error');
          return;
        }

        acceptTransfer(transfer.id);
        toasts.show(`Starting paid download of "${transfer.fileName}" (${formatPriceWei(transfer.priceWei!)})...`, 'info');

        await invoke('start_download', {
          fileHash: transfer.fileHash,
          fileName: transfer.fileName,
          seeders: [transfer.fromPeerId],
          speedTier: 'free',
          fileSize: transfer.fileSize,
          walletAddress: $walletAccount.address,
          privateKey: $walletAccount.privateKey,
          seederPriceWei: transfer.priceWei,
          seederWalletAddress: transfer.senderWallet
        });
      } else {
        // Free transfer: accept via direct file transfer (existing behavior)
        await invoke<string>('accept_file_transfer', { transferId: transfer.id });
        acceptTransfer(transfer.id);
        toasts.show(`Accepting file from ${transfer.fromAlias.displayName}...`, 'info');
      }
    } catch (error) {
      log.error('Failed to accept transfer:', error);
      toasts.show(`Failed to accept transfer: ${error}`, 'error');
    }
  }

  async function handleDecline(transfer: FileTransfer) {
    const tauriAvailable = checkTauriAvailability();
    if (!tauriAvailable) {
      declineTransfer(transfer.id);
      return;
    }

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('decline_file_transfer', { transferId: transfer.id });
      declineTransfer(transfer.id);
      toasts.show(`Declined file from ${transfer.fromAlias.displayName}`, 'info');
    } catch (error) {
      log.error('Failed to decline transfer:', error);
    }
  }

  function formatTimestamp(ts: number): string {
    const date = new Date(ts);
    return date.toLocaleString();
  }

  function getStatusColor(status: FileTransfer['status']): string {
    switch (status) {
      case 'completed': return 'text-green-600';
      case 'failed': return 'text-red-600';
      case 'declined': return 'text-gray-500';
      case 'pending': return 'text-yellow-600';
      case 'accepted': return 'text-blue-600';
      default: return 'text-gray-600';
    }
  }
</script>

<div class="p-6 h-[calc(100vh-64px)] flex flex-col">
  <!-- Header -->
  <div class="flex items-center justify-between mb-6">
    <div>
      <h1 class="text-2xl font-bold text-gray-900 dark:text-white">ChiralDrop</h1>
      <p class="text-gray-600 dark:text-gray-400 mt-1">
        Your alias: <span class="font-semibold" style="color: {$userAlias.colorHex}">{$userAlias.displayName}</span>
      </p>
    </div>
    <button
      onclick={() => showHistory = !showHistory}
      class="flex items-center gap-2 px-4 py-2 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition dark:text-gray-300"
    >
      <History class="w-4 h-4" />
      <span>{showHistory ? 'Hide History' : 'Show History'}</span>
    </button>
  </div>

  <div class="flex-1 flex gap-6 min-h-0">
    <!-- Map Area -->
    <div class="flex-1 bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 shadow-sm relative overflow-hidden">
      <!-- Wave background -->
      <div class="absolute inset-0 bg-gradient-to-br from-blue-50 to-purple-50 dark:from-blue-900/20 dark:to-purple-900/20"></div>

      <!-- Wave pattern overlay -->
      <svg class="absolute inset-0 w-full h-full opacity-20" preserveAspectRatio="none">
        <defs>
          <pattern id="wave-pattern" x="0" y="0" width="100" height="20" patternUnits="userSpaceOnUse">
            <path
              d="M0 10 Q 25 0, 50 10 T 100 10"
              fill="none"
              stroke="currentColor"
              stroke-width="1"
              class="text-blue-400"
            />
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill="url(#wave-pattern)" />
      </svg>

      <!-- Pulsing waves from user -->
      <div class="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 pointer-events-none">
        <div class="pulse-wave pulse-wave-1"></div>
        <div class="pulse-wave pulse-wave-2"></div>
        <div class="pulse-wave pulse-wave-3"></div>
      </div>

      <!-- User (center) -->
      <div
        class="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 flex flex-col items-center z-10"
      >
        <div
          class="w-16 h-16 rounded-full flex items-center justify-center shadow-lg border-4 border-white dark:border-gray-700"
          style="background-color: {$userAlias.colorHex}"
        >
          <User class="w-8 h-8 text-white" />
        </div>
        <span class="mt-2 text-sm font-medium text-gray-700 dark:text-gray-200 bg-white dark:bg-gray-700 px-2 py-1 rounded shadow">
          You
        </span>
      </div>

      <!-- Nearby Peers -->
      {#each $nearbyPeers as peer (peer.peerId)}
        {@const waveOffset = getWaveOffset(peer)}
        <button
          onclick={() => handlePeerClick(peer)}
          class="absolute flex flex-col items-center transition-transform hover:scale-110 focus:outline-none focus:ring-2 focus:ring-blue-400 focus:ring-offset-2 rounded-full"
          style="left: {peer.position.x}%; top: calc({peer.position.y}% + {waveOffset}px); transform: translate(-50%, -50%);"
        >
          <div
            class="w-12 h-12 rounded-full flex items-center justify-center shadow-md border-2 border-white dark:border-gray-700"
            style="background-color: {peer.alias.colorHex}"
          >
            <User class="w-6 h-6 text-white" />
          </div>
          <span class="mt-1 text-xs font-medium text-gray-700 dark:text-gray-200 bg-white dark:bg-gray-700 px-2 py-0.5 rounded shadow whitespace-nowrap">
            {peer.alias.displayName}
          </span>
        </button>
      {/each}

      <!-- Empty state -->
      {#if $nearbyPeers.length === 0}
        <div class="absolute inset-0 flex items-center justify-center">
          <div class="text-center text-gray-500 dark:text-gray-400">
            <p class="text-lg font-medium">No nearby users found</p>
            <p class="text-sm mt-1">Connect to the network to discover peers</p>
          </div>
        </div>
      {/if}
    </div>

    <!-- Side Panel -->
    <div class="w-80 flex flex-col gap-4">
      <!-- Incoming Transfer Requests -->
      {#if $incomingPendingTransfers.length > 0}
        <div class="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 shadow-sm p-4">
          <h3 class="font-semibold text-gray-900 dark:text-white mb-3">Incoming Transfers</h3>
          <div class="space-y-3 max-h-48 overflow-y-auto">
            {#each $incomingPendingTransfers as transfer (transfer.id)}
              <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
                <div class="flex items-start gap-3">
                  <div
                    class="w-8 h-8 rounded-full flex items-center justify-center flex-shrink-0"
                    style="background-color: {transfer.fromAlias.colorHex}"
                  >
                    <User class="w-4 h-4 text-white" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <p class="text-sm font-medium text-gray-900 dark:text-white truncate">{transfer.fileName}</p>
                    <p class="text-xs text-gray-500 dark:text-gray-400">
                      From {transfer.fromAlias.displayName} - {formatFileSize(transfer.fileSize)}
                    </p>
                    {#if transfer.priceWei && transfer.priceWei !== '0' && BigInt(transfer.priceWei) > 0}
                      <span class="inline-flex items-center gap-1 mt-1 px-2 py-0.5 bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400 text-xs rounded-full">
                        <Coins class="w-3 h-3" />
                        {formatPriceWei(transfer.priceWei)}
                      </span>
                    {/if}
                  </div>
                </div>
                <div class="flex gap-2 mt-3">
                  <button
                    onclick={() => handleAccept(transfer)}
                    class="flex-1 flex items-center justify-center gap-1 px-3 py-1.5 bg-green-500 text-white rounded-lg hover:bg-green-600 transition text-sm"
                  >
                    <Check class="w-4 h-4" />
                    {transfer.priceWei && transfer.priceWei !== '0' && BigInt(transfer.priceWei) > 0
                      ? `Pay & Accept`
                      : 'Accept'}
                  </button>
                  <button
                    onclick={() => handleDecline(transfer)}
                    class="flex-1 flex items-center justify-center gap-1 px-3 py-1.5 bg-red-500 text-white rounded-lg hover:bg-red-600 transition text-sm"
                  >
                    <X class="w-4 h-4" />
                    Decline
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Selected Peer Panel -->
      {#if $selectedPeer}
        <div class="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 shadow-sm p-4">
          <div class="flex items-center justify-between mb-4">
            <h3 class="font-semibold text-gray-900 dark:text-white">Send to Peer</h3>
            <button
              onclick={() => selectPeer(null)}
              class="p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition"
            >
              <X class="w-4 h-4 text-gray-500 dark:text-gray-400" />
            </button>
          </div>
          <div class="flex items-center gap-3 mb-4">
            <div
              class="w-12 h-12 rounded-full flex items-center justify-center"
              style="background-color: {$selectedPeer.alias.colorHex}"
            >
              <User class="w-6 h-6 text-white" />
            </div>
            <div>
              <p class="font-medium text-gray-900 dark:text-white">{$selectedPeer.alias.displayName}</p>
              <p class="text-xs text-gray-500 dark:text-gray-400 truncate max-w-[180px]">{$selectedPeer.peerId}</p>
            </div>
          </div>
          <!-- Price Input -->
          <div class="mb-3">
            <label for="chiraldrop-price" class="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
              Price (CHR) — leave empty for free
            </label>
            <div class="flex items-center gap-2">
              <Coins class="w-4 h-4 text-amber-500 flex-shrink-0" />
              <input
                id="chiraldrop-price"
                type="number"
                step="0.001"
                min="0"
                placeholder="0 (free)"
                bind:value={sendPrice}
                class="flex-1 px-3 py-2 bg-gray-50 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg text-sm text-gray-900 dark:text-white"
              />
            </div>
            {#if sendPrice && parseFloat(sendPrice) > 0 && !$walletAccount}
              <p class="text-xs text-amber-500 mt-1">Connect wallet to set a price</p>
            {/if}
          </div>
          <button
            onclick={() => fileInput?.click()}
            class="w-full flex items-center justify-center gap-2 px-4 py-3 bg-blue-500 text-white rounded-lg hover:bg-blue-600 transition"
          >
            <Send class="w-4 h-4" />
            {sendPrice && parseFloat(sendPrice) > 0 ? `Send for ${sendPrice} CHR` : 'Select File to Send'}
          </button>
          <input
            bind:this={fileInput}
            type="file"
            class="hidden"
            onchange={handleFileSelect}
          />
        </div>
      {:else}
        <div class="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 shadow-sm p-4 text-center">
          <div class="w-12 h-12 bg-gray-100 dark:bg-gray-700 rounded-full flex items-center justify-center mx-auto mb-3">
            <Upload class="w-6 h-6 text-gray-400" />
          </div>
          <p class="text-gray-600 dark:text-gray-400 text-sm">Click on a nearby user to send them a file</p>
        </div>
      {/if}

      <!-- Transaction History -->
      {#if showHistory}
        <div class="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 shadow-sm p-4 flex-1 min-h-0 flex flex-col">
          <h3 class="font-semibold text-gray-900 dark:text-white mb-3">Transaction History</h3>
          <div class="flex-1 overflow-y-auto space-y-2">
            {#if $transferHistory.length === 0}
              <p class="text-gray-500 dark:text-gray-400 text-sm text-center py-4">No transfers yet</p>
            {:else}
              {#each $transferHistory as transfer (transfer.id)}
                <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
                  <div class="flex items-start gap-2">
                    <FileIcon class="w-4 h-4 text-gray-400 flex-shrink-0 mt-0.5" />
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium text-gray-900 dark:text-white truncate">{transfer.fileName}</p>
                      <p class="text-xs text-gray-500 dark:text-gray-400">
                        {transfer.direction === 'incoming' ? 'From' : 'To'} {transfer.direction === 'incoming' ? transfer.fromAlias.displayName : transfer.toAlias.displayName}
                      </p>
                      <div class="flex items-center gap-2 mt-1">
                        <span class="text-xs {getStatusColor(transfer.status)} capitalize">{transfer.status}</span>
                        <span class="text-xs text-gray-400">{formatFileSize(transfer.fileSize)}</span>
                      </div>
                      <p class="text-xs text-gray-400 mt-1">{formatTimestamp(transfer.timestamp)}</p>
                    </div>
                  </div>
                </div>
              {/each}
            {/if}
          </div>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .pulse-wave {
    position: absolute;
    border-radius: 50%;
    border: 2px solid rgba(59, 130, 246, 0.5);
    animation: pulse-expand 3s ease-out infinite;
    transform: translate(-50%, -50%);
  }

  .pulse-wave-1 {
    animation-delay: 0s;
  }

  .pulse-wave-2 {
    animation-delay: 1s;
  }

  .pulse-wave-3 {
    animation-delay: 2s;
  }

  @keyframes pulse-expand {
    0% {
      width: 64px;
      height: 64px;
      opacity: 0.6;
      border-width: 3px;
    }
    100% {
      width: 400px;
      height: 400px;
      opacity: 0;
      border-width: 1px;
    }
  }
</style>
