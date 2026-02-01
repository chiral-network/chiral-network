<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { Send, X, Check, History, User, FileIcon, Upload } from 'lucide-svelte';
  import {
    userAlias,
    nearbyPeers,
    pendingTransfers,
    transferHistory,
    selectedPeer,
    incomingPendingTransfers,
    addNearbyPeer,
    removeNearbyPeer,
    selectPeer,
    addPendingTransfer,
    acceptTransfer,
    declineTransfer,
    updateTransferStatus,
    generateTransferId,
    formatFileSize,
    type NearbyPeer,
    type FileTransfer
  } from '$lib/chiralDropStore';
  import { aliasFromPeerId } from '$lib/aliasService';
  import { peers } from '$lib/stores';
  import { toasts } from '$lib/toastStore';

  let showHistory = $state(false);
  let fileInput: HTMLInputElement;
  let animationFrame: number;
  let time = $state(0);
  let unlistenPeerDiscovered: (() => void) | null = null;
  let unlistenFileReceived: (() => void) | null = null;

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
        toPeerId: '', // Will be filled by backend
        toAlias,
        status: 'pending',
        direction: 'incoming',
        timestamp: Date.now()
      });

      toasts.add(`${fromAlias.displayName} wants to send you a file: ${fileName}`, 'info');
    });

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
  });

  // Subscribe to peers store updates
  $effect(() => {
    $peers.forEach((peer) => {
      addNearbyPeer(peer.id);
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

    // Add to pending transfers
    addPendingTransfer({
      id: transferId,
      fileName: file.name,
      fileSize: file.size,
      fromPeerId: '', // Will be filled by backend
      fromAlias,
      toPeerId: peer.peerId,
      toAlias,
      status: 'pending',
      direction: 'outgoing',
      timestamp: Date.now()
    });

    try {
      // Read file as array buffer
      const buffer = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));

      // Send via backend
      await invoke('send_file', {
        peerId: peer.peerId,
        fileName: file.name,
        fileData: bytes,
        transferId
      });

      updateTransferStatus(transferId, 'completed');
      toasts.add(`File sent to ${toAlias.displayName}`, 'success');
    } catch (error) {
      console.error('Failed to send file:', error);
      updateTransferStatus(transferId, 'failed');
      toasts.add(`Failed to send file: ${error}`, 'error');
    }

    selectPeer(null);
  }

  async function handleAccept(transfer: FileTransfer) {
    try {
      await invoke('accept_file_transfer', { transferId: transfer.id });
      acceptTransfer(transfer.id);
      toasts.add(`Accepted file from ${transfer.fromAlias.displayName}`, 'success');
    } catch (error) {
      console.error('Failed to accept transfer:', error);
      toasts.add(`Failed to accept transfer: ${error}`, 'error');
    }
  }

  async function handleDecline(transfer: FileTransfer) {
    try {
      await invoke('decline_file_transfer', { transferId: transfer.id });
      declineTransfer(transfer.id);
      toasts.add(`Declined file from ${transfer.fromAlias.displayName}`, 'info');
    } catch (error) {
      console.error('Failed to decline transfer:', error);
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
      <h1 class="text-2xl font-bold text-gray-900">ChiralDrop</h1>
      <p class="text-gray-600 mt-1">
        Your alias: <span class="font-semibold" style="color: {$userAlias.colorHex}">{$userAlias.displayName}</span>
      </p>
    </div>
    <button
      onclick={() => showHistory = !showHistory}
      class="flex items-center gap-2 px-4 py-2 bg-white border border-gray-200 rounded-lg hover:bg-gray-50 transition"
    >
      <History class="w-4 h-4" />
      <span>{showHistory ? 'Hide History' : 'Show History'}</span>
    </button>
  </div>

  <div class="flex-1 flex gap-6 min-h-0">
    <!-- Map Area -->
    <div class="flex-1 bg-white rounded-xl border border-gray-200 shadow-sm relative overflow-hidden">
      <!-- Wave background -->
      <div class="absolute inset-0 bg-gradient-to-br from-blue-50 to-purple-50"></div>

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

      <!-- User (center) -->
      <div
        class="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 flex flex-col items-center z-10"
      >
        <div
          class="w-16 h-16 rounded-full flex items-center justify-center shadow-lg border-4 border-white"
          style="background-color: {$userAlias.colorHex}"
        >
          <User class="w-8 h-8 text-white" />
        </div>
        <span class="mt-2 text-sm font-medium text-gray-700 bg-white px-2 py-1 rounded shadow">
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
            class="w-12 h-12 rounded-full flex items-center justify-center shadow-md border-2 border-white"
            style="background-color: {peer.alias.colorHex}"
          >
            <User class="w-6 h-6 text-white" />
          </div>
          <span class="mt-1 text-xs font-medium text-gray-700 bg-white px-2 py-0.5 rounded shadow whitespace-nowrap">
            {peer.alias.displayName}
          </span>
        </button>
      {/each}

      <!-- Empty state -->
      {#if $nearbyPeers.length === 0}
        <div class="absolute inset-0 flex items-center justify-center">
          <div class="text-center text-gray-500">
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
        <div class="bg-white rounded-xl border border-gray-200 shadow-sm p-4">
          <h3 class="font-semibold text-gray-900 mb-3">Incoming Transfers</h3>
          <div class="space-y-3 max-h-48 overflow-y-auto">
            {#each $incomingPendingTransfers as transfer (transfer.id)}
              <div class="bg-gray-50 rounded-lg p-3">
                <div class="flex items-start gap-3">
                  <div
                    class="w-8 h-8 rounded-full flex items-center justify-center flex-shrink-0"
                    style="background-color: {transfer.fromAlias.colorHex}"
                  >
                    <User class="w-4 h-4 text-white" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <p class="text-sm font-medium text-gray-900 truncate">{transfer.fileName}</p>
                    <p class="text-xs text-gray-500">
                      From {transfer.fromAlias.displayName} - {formatFileSize(transfer.fileSize)}
                    </p>
                  </div>
                </div>
                <div class="flex gap-2 mt-3">
                  <button
                    onclick={() => handleAccept(transfer)}
                    class="flex-1 flex items-center justify-center gap-1 px-3 py-1.5 bg-green-500 text-white rounded-lg hover:bg-green-600 transition text-sm"
                  >
                    <Check class="w-4 h-4" />
                    Accept
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
        <div class="bg-white rounded-xl border border-gray-200 shadow-sm p-4">
          <div class="flex items-center justify-between mb-4">
            <h3 class="font-semibold text-gray-900">Send to Peer</h3>
            <button
              onclick={() => selectPeer(null)}
              class="p-1 hover:bg-gray-100 rounded-lg transition"
            >
              <X class="w-4 h-4 text-gray-500" />
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
              <p class="font-medium text-gray-900">{$selectedPeer.alias.displayName}</p>
              <p class="text-xs text-gray-500 truncate max-w-[180px]">{$selectedPeer.peerId}</p>
            </div>
          </div>
          <button
            onclick={() => fileInput.click()}
            class="w-full flex items-center justify-center gap-2 px-4 py-3 bg-blue-500 text-white rounded-lg hover:bg-blue-600 transition"
          >
            <Send class="w-4 h-4" />
            Select File to Send
          </button>
          <input
            bind:this={fileInput}
            type="file"
            class="hidden"
            onchange={handleFileSelect}
          />
        </div>
      {:else}
        <div class="bg-white rounded-xl border border-gray-200 shadow-sm p-4 text-center">
          <div class="w-12 h-12 bg-gray-100 rounded-full flex items-center justify-center mx-auto mb-3">
            <Upload class="w-6 h-6 text-gray-400" />
          </div>
          <p class="text-gray-600 text-sm">Click on a nearby user to send them a file</p>
        </div>
      {/if}

      <!-- Transaction History -->
      {#if showHistory}
        <div class="bg-white rounded-xl border border-gray-200 shadow-sm p-4 flex-1 min-h-0 flex flex-col">
          <h3 class="font-semibold text-gray-900 mb-3">Transaction History</h3>
          <div class="flex-1 overflow-y-auto space-y-2">
            {#if $transferHistory.length === 0}
              <p class="text-gray-500 text-sm text-center py-4">No transfers yet</p>
            {:else}
              {#each $transferHistory as transfer (transfer.id)}
                <div class="bg-gray-50 rounded-lg p-3">
                  <div class="flex items-start gap-2">
                    <FileIcon class="w-4 h-4 text-gray-400 flex-shrink-0 mt-0.5" />
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium text-gray-900 truncate">{transfer.fileName}</p>
                      <p class="text-xs text-gray-500">
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
