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
    updateTransferPayment,
    updateTransferByFileHash,
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
  let sendPrice = $state('');
  let unlistenPeerDiscovered: (() => void) | null = null;
  let unlistenFileReceived: (() => void) | null = null;
  let unlistenFileComplete: (() => void) | null = null;
  let unlistenFileReceivedComplete: (() => void) | null = null;
  let unlistenPaidRequest: (() => void) | null = null;
  let unlistenConnectionEstablished: (() => void) | null = null;
  let unlistenPaymentSent: (() => void) | null = null;
  let unlistenPaymentReceived: (() => void) | null = null;
  let unlistenDownloadComplete: (() => void) | null = null;
  let unlistenDownloadFailed: (() => void) | null = null;

  onMount(async () => {
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

        // Listen for payment sent (buyer side) — record in both histories
        unlistenPaymentSent = await listen<any>('chiraldrop-payment-sent', async (event) => {
          const { requestId, fileHash, fileName, txHash, priceWei, toWallet, balanceBefore, balanceAfter } = event.payload;
          log.info('Payment sent for ChiralDrop file:', fileName, 'tx:', txHash);

          // Update ChiralDrop transfer history with payment tx hash and balance
          updateTransferByFileHash(fileHash, 'accepted', txHash, balanceBefore, balanceAfter);

          // Record in Account transaction history
          try {
            const { invoke } = await import('@tauri-apps/api/core');
            await invoke('record_transaction_meta', {
              txHash,
              txType: 'file_payment',
              description: `📁 Paid for "${fileName}" (${formatPriceWei(priceWei)})`,
              recipientLabel: `Seeder (${toWallet.slice(0, 10)}...)`,
              balanceBefore,
              balanceAfter,
            });
          } catch (e) {
            log.warn('Failed to record payment metadata:', e);
          }
        });

        // Listen for payment received (seller side) — record in Account history
        unlistenPaymentReceived = await listen<any>('chiraldrop-payment-received', async (event) => {
          const { fileHash, txHash, priceWei, fromWallet } = event.payload;
          log.info('Payment received for file:', fileHash, 'tx:', txHash);

          // Record in Account transaction history (seller doesn't have balance snapshot from buyer's tx)
          try {
            const { invoke } = await import('@tauri-apps/api/core');
            await invoke('record_transaction_meta', {
              txHash,
              txType: 'file_sale',
              description: `💰 Received payment (${formatPriceWei(priceWei)}) for shared file`,
              recipientLabel: `Buyer (${fromWallet.slice(0, 10)}...)`,
              balanceBefore: null,
              balanceAfter: null,
            });
          } catch (e) {
            log.warn('Failed to record payment received metadata:', e);
          }
        });

        // Listen for download complete — update ChiralDrop transfer for paid downloads
        unlistenDownloadComplete = await listen<any>('file-download-complete', (event) => {
          const { fileHash, fileName } = event.payload;
          updateTransferByFileHash(fileHash, 'completed');
          log.info('Paid download complete:', fileName);
        });

        // Listen for download failed — update ChiralDrop transfer for paid downloads
        unlistenDownloadFailed = await listen<any>('file-download-failed', (event) => {
          const { fileHash, error } = event.payload;
          updateTransferByFileHash(fileHash, 'failed');
          log.warn('Paid download failed:', fileHash, error);
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
    if (unlistenPaymentSent) {
      unlistenPaymentSent();
    }
    if (unlistenPaymentReceived) {
      unlistenPaymentReceived();
    }
    if (unlistenDownloadComplete) {
      unlistenDownloadComplete();
    }
    if (unlistenDownloadFailed) {
      unlistenDownloadFailed();
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

  async function handleSendClick() {
    if (!$selectedPeer) return;

    const tauriAvailable = checkTauriAvailability();
    if (!tauriAvailable) {
      toasts.show('File transfer requires the desktop app', 'error');
      return;
    }

    try {
      const { invoke } = await import('@tauri-apps/api/core');

      // Use Tauri file dialog (browser file input doesn't work in Tauri 2)
      const selectedPaths = await invoke<string[]>('open_file_dialog', { multiple: false });
      if (!selectedPaths || selectedPaths.length === 0) return;

      const filePath = selectedPaths[0];
      const fileName = filePath.split(/[\\/]/).pop() || filePath;

      await sendFile(filePath, fileName, $selectedPeer);
    } catch (error) {
      log.error('Failed to open file dialog:', error);
      toasts.show(`Failed to open file dialog: ${error}`, 'error');
    }
  }

  async function sendFile(filePath: string, fileName: string, peer: NearbyPeer) {
    const transferId = generateTransferId();
    const fromAlias = $userAlias;
    const toAlias = peer.alias;
    const price = String(sendPrice ?? '').trim();
    const isPaid = price !== '' && parseFloat(price) > 0;

    // Validate wallet for paid transfers
    if (isPaid && !$walletAccount) {
      toasts.show('Connect your wallet to set a price on files', 'error');
      return;
    }

    // Add to pending transfers
    addPendingTransfer({
      id: transferId,
      fileName,
      fileSize: 0,
      fromPeerId: $localPeerId || '',
      fromAlias,
      toPeerId: peer.peerId,
      toAlias,
      status: 'pending',
      direction: 'outgoing',
      timestamp: Date.now()
    });

    try {
      const { invoke } = await import('@tauri-apps/api/core');

      if (isPaid) {
        // Paid transfer flow:
        // 1. Publish file from path (hashes, stores in memory, registers for chunked serving)
        // 2. Send metadata-only request via file_transfer protocol with pricing info

        // Convert CHR to wei
        const priceParts = price.split('.');
        const whole = BigInt(priceParts[0] || '0');
        const fracStr = (priceParts[1] || '').padEnd(18, '0').slice(0, 18);
        const frac = BigInt(fracStr);
        const priceWei = (whole * BigInt(1e18) + frac).toString();

        // Publish file to get hash and register for chunked serving
        const publishResult = await invoke<{ merkleRoot: string }>('publish_file', {
          filePath,
          fileName,
          priceChr: price,
          walletAddress: $walletAccount!.address
        });

        const fileHash = publishResult.merkleRoot;

        // Send metadata-only transfer request (empty file data) with pricing
        await invoke('send_file', {
          peerId: peer.peerId,
          fileName,
          fileData: [],
          transferId,
          priceWei,
          senderWallet: $walletAccount!.address,
          fileHash
        });

        updateTransferStatus(transferId, 'completed');
        toasts.show(`Paid file offer sent to ${toAlias.displayName} (${price} CHR)`, 'success');
      } else {
        // Free transfer: read file from disk and send directly
        await invoke('send_file_by_path', {
          peerId: peer.peerId,
          filePath,
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

<div class="p-4 sm:p-6 h-[calc(100vh-64px)] flex flex-col gap-4 sm:gap-6">
  <!-- Header -->
  <div class="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
    <div class="space-y-1">
      <h1 class="text-3xl font-bold tracking-tight text-gray-900 dark:text-white">ChiralDrop</h1>
      <p class="text-sm sm:text-base text-gray-600 dark:text-gray-400">
        Your alias:
        <span class="font-semibold" style="color: {$userAlias.colorHex}">{$userAlias.displayName}</span>
      </p>
    </div>
    <div class="flex flex-wrap items-center gap-2 sm:gap-3">
      <div class="inline-flex items-center rounded-full border border-blue-200/70 bg-blue-50/80 px-3 py-1 text-xs font-medium text-blue-700 dark:border-blue-800/60 dark:bg-blue-900/30 dark:text-blue-300">
        {$nearbyPeers.length} peers online
      </div>
      {#if $incomingPendingTransfers.length > 0}
        <div class="inline-flex items-center rounded-full border border-amber-200/70 bg-amber-50/80 px-3 py-1 text-xs font-medium text-amber-700 dark:border-amber-800/60 dark:bg-amber-900/30 dark:text-amber-300">
          {$incomingPendingTransfers.length} pending request{$incomingPendingTransfers.length === 1 ? '' : 's'}
        </div>
      {/if}
      <button
        onclick={() => showHistory = !showHistory}
        class="inline-flex items-center gap-2 rounded-xl border border-gray-200 bg-white px-4 py-2 text-sm font-medium text-gray-700 shadow-sm transition hover:bg-gray-50 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-300 dark:hover:bg-gray-700"
      >
        <History class="h-4 w-4" />
        <span>{showHistory ? 'Hide History' : 'Show History'}</span>
      </button>
    </div>
  </div>

  <div class="flex-1 min-h-0 grid grid-cols-1 xl:grid-cols-[minmax(0,1fr)_22rem] gap-4 sm:gap-6">
    <!-- Peer Map -->
    <div class="relative overflow-hidden rounded-2xl border border-gray-200/80 bg-white shadow-sm dark:border-gray-700 dark:bg-gray-800">
      <div class="absolute inset-0 bg-gradient-to-br from-slate-50 via-blue-50/70 to-cyan-50/60 dark:from-slate-900 dark:via-gray-900 dark:to-blue-950/50"></div>
      <div class="absolute -left-20 -top-24 h-72 w-72 rounded-full bg-blue-300/25 blur-3xl dark:bg-blue-500/20"></div>
      <div class="absolute -right-20 -bottom-20 h-72 w-72 rounded-full bg-cyan-300/30 blur-3xl dark:bg-cyan-500/20"></div>
      <div class="network-dot-grid absolute inset-0 opacity-45 dark:opacity-30"></div>

      <div class="absolute left-4 top-4 z-20 inline-flex items-center rounded-full bg-white/90 px-3 py-1 text-xs font-semibold text-slate-700 shadow-sm ring-1 ring-slate-200 backdrop-blur dark:bg-gray-900/70 dark:text-slate-200 dark:ring-gray-700">
        Live Peer Mesh
      </div>
      <div class="absolute right-4 top-4 z-20 inline-flex items-center rounded-full bg-white/85 px-3 py-1 text-xs text-slate-600 shadow-sm ring-1 ring-slate-200 backdrop-blur dark:bg-gray-900/70 dark:text-slate-300 dark:ring-gray-700">
        {$nearbyPeers.length} discovered
      </div>

      <!-- User -->
      <div class="absolute left-1/2 top-1/2 z-20 -translate-x-1/2 -translate-y-1/2 flex flex-col items-center">
        <span class="absolute -inset-4 rounded-full bg-blue-400/25 blur-xl dark:bg-blue-500/20"></span>
        <div
          class="relative h-16 w-16 rounded-full border-4 border-white shadow-xl ring-4 ring-blue-200/60 dark:border-gray-700 dark:ring-blue-900/60 flex items-center justify-center"
          style="background-color: {$userAlias.colorHex}"
        >
          <User class="h-8 w-8 text-white" />
        </div>
        <span class="mt-2 rounded-full bg-white/90 px-3 py-1 text-xs font-semibold text-gray-700 shadow-sm ring-1 ring-gray-200 backdrop-blur dark:bg-gray-700/80 dark:text-gray-100 dark:ring-gray-600">
          You
        </span>
      </div>

      <!-- Nearby Peers -->
      {#each $nearbyPeers as peer (peer.peerId)}
        <button
          onclick={() => handlePeerClick(peer)}
          class="peer-node group absolute z-20 focus:outline-none focus:ring-2 focus:ring-blue-400 focus:ring-offset-2"
          style="left: {peer.position.x}%; top: {peer.position.y}%; transform: translate(-50%, -50%);"
        >
          <span class="peer-glow absolute inset-0 rounded-full bg-blue-400/20 blur-md dark:bg-blue-500/20"></span>
          <div class="relative z-10 flex flex-col items-center">
            <div
              class="flex h-12 w-12 items-center justify-center rounded-full border-2 border-white shadow-md transition-transform duration-200 group-hover:scale-110 dark:border-gray-700 {$selectedPeer?.peerId === peer.peerId ? 'ring-4 ring-blue-300/60 dark:ring-blue-900/70' : ''}"
              style="background-color: {peer.alias.colorHex}"
            >
              <User class="h-6 w-6 text-white" />
            </div>
            <span class="mt-1 whitespace-nowrap rounded-full bg-white/90 px-2 py-0.5 text-xs font-medium text-gray-700 shadow-sm ring-1 ring-gray-200 backdrop-blur dark:bg-gray-700/80 dark:text-gray-100 dark:ring-gray-600">
              {peer.alias.displayName}
            </span>
          </div>
        </button>
      {/each}

      <!-- Empty state -->
      {#if $nearbyPeers.length === 0}
        <div class="absolute inset-0 flex items-center justify-center p-6">
          <div class="max-w-sm rounded-2xl border border-white/70 bg-white/75 p-6 text-center shadow-lg backdrop-blur dark:border-gray-700 dark:bg-gray-900/75">
            <div class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-slate-100 text-slate-500 dark:bg-gray-700 dark:text-gray-300">
              <User class="h-6 w-6" />
            </div>
            <p class="text-lg font-semibold text-gray-800 dark:text-gray-100">No nearby users found</p>
            <p class="mt-1 text-sm text-gray-500 dark:text-gray-400">Connect to the network to discover peers</p>
          </div>
        </div>
      {/if}
    </div>

    <!-- Side Panel -->
    <div class="flex min-h-0 flex-col gap-4">
      <!-- Incoming Transfer Requests -->
      {#if $incomingPendingTransfers.length > 0}
        <div class="rounded-2xl border border-amber-200/70 bg-white/90 p-4 shadow-sm backdrop-blur dark:border-amber-900/60 dark:bg-gray-800/85">
          <h3 class="mb-3 font-semibold text-gray-900 dark:text-white">Incoming Transfers</h3>
          <div class="space-y-3 max-h-56 overflow-y-auto pr-1">
            {#each $incomingPendingTransfers as transfer (transfer.id)}
              <div class="rounded-xl border border-amber-100 bg-amber-50/60 p-3 dark:border-amber-900/40 dark:bg-amber-900/20">
                <div class="flex items-start gap-3">
                  <div
                    class="h-8 w-8 rounded-full flex items-center justify-center flex-shrink-0 shadow-sm"
                    style="background-color: {transfer.fromAlias.colorHex}"
                  >
                    <User class="h-4 w-4 text-white" />
                  </div>
                  <div class="min-w-0 flex-1">
                    <p class="truncate text-sm font-medium text-gray-900 dark:text-white">{transfer.fileName}</p>
                    <p class="text-xs text-gray-600 dark:text-gray-400">
                      From {transfer.fromAlias.displayName} - {formatFileSize(transfer.fileSize)}
                    </p>
                    {#if transfer.priceWei && transfer.priceWei !== '0' && BigInt(transfer.priceWei) > 0}
                      <span class="mt-1 inline-flex items-center gap-1 rounded-full bg-amber-100 px-2 py-0.5 text-xs text-amber-700 dark:bg-amber-900/40 dark:text-amber-300">
                        <Coins class="h-3 w-3" />
                        {formatPriceWei(transfer.priceWei)}
                      </span>
                    {/if}
                  </div>
                </div>
                <div class="mt-3 flex gap-2">
                  <button
                    onclick={() => handleAccept(transfer)}
                    class="flex-1 rounded-lg bg-emerald-500 px-3 py-1.5 text-sm font-medium text-white transition hover:bg-emerald-600"
                  >
                    <span class="inline-flex items-center justify-center gap-1">
                      <Check class="h-4 w-4" />
                      {transfer.priceWei && transfer.priceWei !== '0' && BigInt(transfer.priceWei) > 0
                        ? `Pay & Accept`
                        : 'Accept'}
                    </span>
                  </button>
                  <button
                    onclick={() => handleDecline(transfer)}
                    class="flex-1 rounded-lg bg-rose-500 px-3 py-1.5 text-sm font-medium text-white transition hover:bg-rose-600"
                  >
                    <span class="inline-flex items-center justify-center gap-1">
                      <X class="h-4 w-4" />
                      Decline
                    </span>
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Selected Peer Panel -->
      {#if $selectedPeer}
        <div class="rounded-2xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <div class="mb-4 flex items-center justify-between">
            <h3 class="font-semibold text-gray-900 dark:text-white">Send to Peer</h3>
            <button
              onclick={() => selectPeer(null)}
              class="rounded-lg p-1 transition hover:bg-gray-100 dark:hover:bg-gray-700"
            >
              <X class="h-4 w-4 text-gray-500 dark:text-gray-400" />
            </button>
          </div>
          <div class="mb-4 flex items-center gap-3 rounded-xl bg-gray-50 p-3 dark:bg-gray-700/70">
            <div
              class="h-12 w-12 rounded-full flex items-center justify-center shadow-sm"
              style="background-color: {$selectedPeer.alias.colorHex}"
            >
              <User class="h-6 w-6 text-white" />
            </div>
            <div class="min-w-0">
              <p class="truncate font-medium text-gray-900 dark:text-white">{$selectedPeer.alias.displayName}</p>
              <p class="truncate text-xs text-gray-500 dark:text-gray-400">{$selectedPeer.peerId}</p>
            </div>
          </div>
          <div class="mb-3">
            <label for="chiraldrop-price" class="mb-1 block text-xs font-medium text-gray-500 dark:text-gray-400">
              Price (CHR) — leave empty for free
            </label>
            <div class="flex items-center gap-2">
              <Coins class="h-4 w-4 flex-shrink-0 text-amber-500" />
              <input
                id="chiraldrop-price"
                type="number"
                step="0.001"
                min="0"
                placeholder="0 (free)"
                bind:value={sendPrice}
                class="flex-1 rounded-lg border border-gray-200 bg-gray-50 px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-200 dark:border-gray-600 dark:bg-gray-700 dark:text-white dark:focus:border-blue-500 dark:focus:ring-blue-900/50"
              />
            </div>
            {#if sendPrice && parseFloat(sendPrice) > 0 && !$walletAccount}
              <p class="mt-1 text-xs text-amber-500">Connect wallet to set a price</p>
            {/if}
          </div>
          <button
            onclick={handleSendClick}
            class="w-full rounded-xl bg-blue-500 px-4 py-3 text-sm font-semibold text-white transition hover:bg-blue-600"
          >
            <span class="inline-flex items-center justify-center gap-2">
              <Send class="h-4 w-4" />
              {sendPrice && parseFloat(sendPrice) > 0 ? `Send for ${sendPrice} CHR` : 'Select File to Send'}
            </span>
          </button>
        </div>
      {:else}
        <div class="rounded-2xl border border-dashed border-gray-300 bg-white/80 p-5 text-center shadow-sm dark:border-gray-700 dark:bg-gray-800/85">
          <div class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-gray-100 dark:bg-gray-700">
            <Upload class="h-6 w-6 text-gray-400" />
          </div>
          <p class="text-sm text-gray-600 dark:text-gray-400">Select a nearby user to start a transfer</p>
        </div>
      {/if}

      <!-- Transaction History -->
      {#if showHistory}
        <div class="flex min-h-0 flex-1 flex-col rounded-2xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <h3 class="mb-3 font-semibold text-gray-900 dark:text-white">Transaction History</h3>
          <div class="flex-1 space-y-2 overflow-y-auto pr-1">
            {#if $transferHistory.length === 0}
              <p class="py-4 text-center text-sm text-gray-500 dark:text-gray-400">No transfers yet</p>
            {:else}
              {#each $transferHistory as transfer (transfer.id)}
                <div class="rounded-xl border border-gray-100 bg-gray-50/80 p-3 dark:border-gray-700 dark:bg-gray-700/60">
                  <div class="flex items-start gap-2.5">
                    <FileIcon class="mt-0.5 h-4 w-4 flex-shrink-0 text-gray-400" />
                    <div class="min-w-0 flex-1">
                      <p class="truncate text-sm font-medium text-gray-900 dark:text-white">{transfer.fileName}</p>
                      <p class="text-xs text-gray-500 dark:text-gray-400">
                        {transfer.direction === 'incoming' ? 'From' : 'To'} {transfer.direction === 'incoming' ? transfer.fromAlias.displayName : transfer.toAlias.displayName}
                      </p>
                      <div class="mt-1 flex items-center gap-2">
                        <span class="text-xs capitalize {getStatusColor(transfer.status)}">{transfer.status}</span>
                        <span class="text-xs text-gray-400">{formatFileSize(transfer.fileSize)}</span>
                        {#if transfer.priceWei && transfer.priceWei !== '0'}
                          <span class="text-xs text-amber-600 dark:text-amber-400">{formatPriceWei(transfer.priceWei)}</span>
                        {/if}
                      </div>
                      {#if transfer.paymentTxHash}
                        <p class="mt-1 truncate font-mono text-xs text-gray-400" title={transfer.paymentTxHash}>Tx: {transfer.paymentTxHash.slice(0, 18)}...</p>
                      {/if}
                      {#if transfer.balanceBefore && transfer.balanceAfter}
                        <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">
                          {transfer.balanceBefore} → {transfer.balanceAfter} CHR
                        </p>
                      {/if}
                      <p class="mt-1 text-xs text-gray-400">{formatTimestamp(transfer.timestamp)}</p>
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
  .network-dot-grid {
    background-image: radial-gradient(rgba(148, 163, 184, 0.45) 1px, transparent 1px);
    background-size: 22px 22px;
    mask-image: radial-gradient(circle at center, rgba(0, 0, 0, 1), rgba(0, 0, 0, 0.2) 75%, rgba(0, 0, 0, 0));
  }

  .peer-node {
    transition: transform 0.2s ease;
  }

  .peer-glow {
    animation: peerPulse 2.8s ease-in-out infinite;
  }

  @keyframes peerPulse {
    0%, 100% {
      opacity: 0.35;
      transform: scale(1);
    }
    50% {
      opacity: 0.75;
      transform: scale(1.12);
    }
  }
</style>
