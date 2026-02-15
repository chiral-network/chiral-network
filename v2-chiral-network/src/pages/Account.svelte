<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { walletAccount, isAuthenticated, networkConnected } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import { walletService } from '$lib/services/walletService';
  import { dhtService } from '$lib/dhtService';
  import {
    Wallet,
    Copy,
    Eye,
    EyeOff,
    LogOut,
    AlertTriangle,
    Check,
    Key,
    RefreshCw,
    Send,
    History,
    ArrowUpRight,
    ArrowDownLeft,
    Loader2,
    Zap,
    Download,
    ChevronDown,
    ChevronUp,
    File as FileIcon,
    UserPlus,
    X
  } from 'lucide-svelte';
  import { logger } from '$lib/logger';
  const log = logger('Account');

  // Types
  interface Transaction {
    hash: string;
    from: string;
    to: string;
    value: string;
    valueWei: string;
    blockNumber: number;
    timestamp: number;
    status: string;
    gasUsed: number;
    // Enriched metadata
    txType: string;          // "send", "receive", "speed_tier_payment", "file_payment", "file_sale", "unknown"
    description: string;
    fileName?: string;
    fileHash?: string;
    speedTier?: string;
    recipientLabel?: string;
    balanceBefore?: string;
    balanceAfter?: string;
  }

  interface SavedRecipient {
    id: string;
    label: string;
    address: string;
    lastUsed: number;
  }

  // Saved recipients state
  const SAVED_RECIPIENTS_KEY = 'chiral_saved_recipients';
  let savedRecipients = $state<SavedRecipient[]>([]);
  let showAddRecipient = $state(false);
  let newRecipientLabel = $state('');

  function loadSavedRecipients() {
    try {
      const stored = localStorage.getItem(SAVED_RECIPIENTS_KEY);
      if (stored) {
        savedRecipients = JSON.parse(stored);
      }
    } catch (e) {
      log.error('Failed to load saved recipients:', e);
    }
  }

  function saveSavedRecipients() {
    try {
      localStorage.setItem(SAVED_RECIPIENTS_KEY, JSON.stringify(savedRecipients));
    } catch (e) {
      log.error('Failed to save recipients:', e);
    }
  }

  function addRecipient() {
    if (!newRecipientLabel.trim() || !recipientAddress) return;
    if (!recipientAddress.startsWith('0x') || recipientAddress.length !== 42) {
      toasts.show('Enter a valid 0x address before saving', 'error');
      return;
    }
    // Don't add duplicates
    if (savedRecipients.some(r => r.address.toLowerCase() === recipientAddress.toLowerCase())) {
      toasts.show('This address is already saved', 'info');
      showAddRecipient = false;
      newRecipientLabel = '';
      return;
    }
    savedRecipients = [...savedRecipients, {
      id: `r-${Date.now()}`,
      label: newRecipientLabel.trim(),
      address: recipientAddress,
      lastUsed: Date.now(),
    }];
    saveSavedRecipients();
    showAddRecipient = false;
    newRecipientLabel = '';
    toasts.show('Recipient saved', 'success');
  }

  function deleteRecipient(id: string) {
    savedRecipients = savedRecipients.filter(r => r.id !== id);
    saveSavedRecipients();
  }

  function selectRecipient(r: SavedRecipient) {
    recipientAddress = r.address;
  }

  function getRecipientLabel(address: string): string | null {
    const r = savedRecipients.find(r => r.address.toLowerCase() === address.toLowerCase());
    return r ? r.label : null;
  }

  // Geth connection state
  let gethConnected = $state(false);
  let gethCheckInterval: ReturnType<typeof setInterval> | null = null;

  // State
  let privateKeyVisible = $state(false);
  let copied = $state<'address' | 'privateKey' | null>(null);
  let showLogoutModal = $state(false);
  let balance = $state<string>('--');
  let isLoadingBalance = $state(false);

  // Send transaction state
  let recipientAddress = $state('');
  let sendAmount = $state('');
  let isSending = $state(false);
  let showConfirmSend = $state(false);

  // Transaction history state
  let transactions = $state<Transaction[]>([]);
  let isLoadingHistory = $state(false);
  let expandedTxHash = $state<string | null>(null);


  // Check if Tauri is available
  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  // Poll for transaction confirmation and refresh balance
  async function pollForConfirmation(txHash: string) {
    if (!isTauri()) return;
    const maxAttempts = 30; // Poll for up to 30 seconds
    for (let i = 0; i < maxAttempts; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      try {
        const receipt = await invoke<{ status: string } | null>('get_transaction_receipt', { txHash });
        if (receipt) {
          log.ok('Transaction confirmed:', txHash.slice(0, 10));
          walletService.clearCache($walletAccount?.address);
          loadBalance();
          loadTransactionHistory();
          return;
        }
      } catch {
        // Receipt not available yet
      }
    }
    // Timed out - refresh anyway
    log.warn('Transaction not confirmed after 30s, refreshing anyway');
    walletService.clearCache($walletAccount?.address);
    loadBalance();
    loadTransactionHistory();
  }

  // Check Geth connection status (used to track local mining node state)
  async function checkGethStatus() {
    if (!isTauri()) return;
    try {
      const status = await invoke<{ localRunning: boolean }>('get_geth_status');
      gethConnected = status.localRunning;
    } catch {
      gethConnected = false;
    }
  }

  // Load balance on mount and when wallet changes
  onMount(() => {
    checkGethStatus();
    gethCheckInterval = setInterval(checkGethStatus, 5000);
    loadSavedRecipients();
    if ($walletAccount?.address) {
      loadBalance();
      loadTransactionHistory();
    }
  });

  onDestroy(() => {
    if (gethCheckInterval) clearInterval(gethCheckInterval);
  });

  // Watch for wallet changes
  $effect(() => {
    if ($walletAccount?.address) {
      loadBalance();
      loadTransactionHistory();
    }
  });

  // Load wallet balance (always queries remote RPC — no local Geth needed)
  async function loadBalance() {
    log.info('[Account.loadBalance] Called, address:', $walletAccount?.address);
    if (!$walletAccount?.address) return;

    isLoadingBalance = true;
    try {
      const result = await walletService.getBalance($walletAccount.address);
      balance = result;
      log.info('[Account.loadBalance] Balance loaded:', result);
    } catch (error) {
      log.warn('[Account.loadBalance] Failed:', error);
      balance = '--';
    } finally {
      isLoadingBalance = false;
    }
  }

  // Load transaction history
  async function loadTransactionHistory() {
    if (!$walletAccount?.address || !isTauri()) return;

    isLoadingHistory = true;
    try {
      const result = await invoke<{ transactions: Transaction[] }>('get_transaction_history', {
        address: $walletAccount.address
      });
      transactions = result.transactions;
    } catch (error) {
      // Silent fail - Geth not running is expected initially
      transactions = [];
    } finally {
      isLoadingHistory = false;
    }
  }

  // Send CHR
  async function handleSend() {
    if (!$walletAccount || !recipientAddress || !sendAmount) return;

    const amount = parseFloat(sendAmount);
    if (isNaN(amount) || amount <= 0) {
      toasts.show('Please enter a valid amount', 'error');
      return;
    }

    if (amount > parseFloat(balance)) {
      toasts.show('Insufficient balance', 'error');
      return;
    }

    if (!recipientAddress.startsWith('0x') || recipientAddress.length !== 42) {
      toasts.show('Invalid recipient address', 'error');
      return;
    }

    showConfirmSend = true;
  }

  // Confirm and execute send
  async function confirmSend() {
    if (!$walletAccount || !isTauri()) return;

    isSending = true;
    try {
      const result = await invoke<{ hash: string; status: string; balanceBefore: string; balanceAfter: string }>('send_transaction', {
        fromAddress: $walletAccount.address,
        toAddress: recipientAddress,
        amount: String(sendAmount),
        privateKey: $walletAccount.privateKey
      });

      toasts.show(`Transaction sent! Hash: ${result.hash.slice(0, 10)}...`, 'success');

      // Record metadata for enriched transaction history
      try {
        await invoke('record_transaction_meta', {
          txHash: result.hash,
          txType: 'send',
          description: `💸 Sent ${sendAmount} CHR to ${recipientAddress.slice(0, 10)}...`,
          recipientLabel: getRecipientLabel(recipientAddress),
          balanceBefore: result.balanceBefore,
          balanceAfter: result.balanceAfter,
        });
      } catch (e) {
        log.warn('Failed to record tx metadata:', e);
      }

      // Update lastUsed for saved recipient
      const idx = savedRecipients.findIndex(r => r.address.toLowerCase() === recipientAddress.toLowerCase());
      if (idx !== -1) {
        savedRecipients[idx] = { ...savedRecipients[idx], lastUsed: Date.now() };
        savedRecipients = [...savedRecipients];
        saveSavedRecipients();
      }

      // Reset form
      recipientAddress = '';
      sendAmount = '';
      showConfirmSend = false;

      // Wait for transaction to be mined, then refresh
      pollForConfirmation(result.hash);
    } catch (error) {
      log.error('Failed to send transaction:', error);
      toasts.show(`Transaction failed: ${error}`, 'error');
    } finally {
      isSending = false;
    }
  }

  // Copy to clipboard
  async function copyToClipboard(text: string, type: 'address' | 'privateKey') {
    try {
      await navigator.clipboard.writeText(text);
      copied = type;
      toasts.show(`${type === 'address' ? 'Address' : 'Private key'} copied to clipboard`, 'success');
      setTimeout(() => copied = null, 2000);
    } catch (error) {
      log.error('Failed to copy:', error);
      toasts.show('Failed to copy to clipboard', 'error');
    }
  }

  // Logout
  async function logout() {
    // Stop the DHT backend before resetting UI state
    try {
      await dhtService.stop();
    } catch (e) {
      log.warn('Failed to stop DHT during logout:', e);
    }
    walletAccount.set(null);
    isAuthenticated.set(false);
    showLogoutModal = false;
    toasts.show('Logged out successfully', 'info');
  }

  // Format address for display
  function formatAddress(address: string): string {
    if (!address) return '';
    return `${address.slice(0, 6)}...${address.slice(-4)}`;
  }

  // Format balance for display
  function formatBalance(bal: string): string {
    const num = parseFloat(bal);
    if (isNaN(num)) return '0.00';
    return num.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 });
  }

  // Format timestamp
  function formatTimestamp(timestamp: number): string {
    if (!timestamp) return 'Unknown';
    const date = new Date(timestamp * 1000);
    return date.toLocaleString();
  }

  // Check if transaction is incoming
  function isIncoming(tx: Transaction): boolean {
    return tx.to.toLowerCase() === $walletAccount?.address.toLowerCase();
  }

  // Get transaction type icon and color
  function getTxTypeStyle(tx: Transaction): { bgColor: string; iconColor: string } {
    switch (tx.txType) {
      case 'speed_tier_payment':
        return { bgColor: 'bg-amber-100 dark:bg-amber-900/30', iconColor: 'text-amber-600 dark:text-amber-400' };
      case 'file_payment':
        return { bgColor: 'bg-purple-100 dark:bg-purple-900/30', iconColor: 'text-purple-600 dark:text-purple-400' };
      case 'file_sale':
        return { bgColor: 'bg-emerald-100 dark:bg-emerald-900/30', iconColor: 'text-emerald-600 dark:text-emerald-400' };
      case 'receive':
        return { bgColor: 'bg-green-100 dark:bg-green-900/30', iconColor: 'text-green-600 dark:text-green-400' };
      case 'send':
        return { bgColor: 'bg-red-100 dark:bg-red-900/30', iconColor: 'text-red-600 dark:text-red-400' };
      default:
        return isIncoming(tx)
          ? { bgColor: 'bg-green-100 dark:bg-green-900/30', iconColor: 'text-green-600 dark:text-green-400' }
          : { bgColor: 'bg-red-100 dark:bg-red-900/30', iconColor: 'text-red-600 dark:text-red-400' };
    }
  }

  // Get transaction type label
  function getTxTypeLabel(tx: Transaction): string {
    switch (tx.txType) {
      case 'speed_tier_payment': return 'Download Payment';
      case 'file_payment': return 'File Purchase';
      case 'file_sale': return 'File Sale';
      case 'send': return 'Sent';
      case 'receive': return 'Received';
      default: return isIncoming(tx) ? 'Received' : 'Sent';
    }
  }

</script>

<div class="p-6 space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold dark:text-white">Account</h1>
      <p class="text-gray-600 dark:text-gray-400 mt-1">Manage your wallet and account settings</p>
    </div>
    <button
      onclick={() => showLogoutModal = true}
      class="flex items-center gap-2 px-4 py-2 text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
    >
      <LogOut class="w-5 h-5" />
      Logout
    </button>
  </div>

  {#if $walletAccount}
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
    <!-- Wallet Overview Card -->
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 overflow-hidden">
      <div class="bg-gradient-to-r from-blue-600 to-blue-700 p-6 text-white">
        <div class="flex items-center justify-between mb-4">
          <div class="flex items-center gap-3">
            <div class="p-3 bg-white/20 rounded-full">
              <Wallet class="w-8 h-8" />
            </div>
            <div>
              <h2 class="text-xl font-semibold">Chiral Wallet</h2>
              <p class="text-blue-100 text-sm">Your decentralized identity</p>
            </div>
          </div>
          <span class="px-3 py-1 bg-white/20 rounded-full text-sm">
            {$networkConnected ? 'Connected' : 'Disconnected'}
          </span>
        </div>

        <!-- Balance Display -->
        <div class="bg-white/10 rounded-xl p-4 mt-4">
          <div class="flex items-center justify-between">
            <div>
              <p class="text-blue-100 text-sm mb-1">Balance</p>
              <div class="flex items-baseline gap-2">
                {#if isLoadingBalance}
                  <RefreshCw class="w-6 h-6 animate-spin" />
                {:else if balance === '--'}
                  <span class="text-xl font-bold text-blue-200/60">--</span>
                  <span class="text-blue-200/60 text-sm">Connecting to network...</span>
                {:else}
                  <span class="text-3xl font-bold">{formatBalance(balance)}</span>
                  <span class="text-blue-100">CHR</span>
                {/if}
              </div>
            </div>
            <button
              onclick={loadBalance}
              disabled={isLoadingBalance}
              class="p-2 hover:bg-white/10 rounded-lg transition-colors disabled:opacity-50"
              title="Refresh balance"
            >
              <RefreshCw class="w-5 h-5 {isLoadingBalance ? 'animate-spin' : ''}" />
            </button>
          </div>
        </div>

        <div class="flex items-center gap-2 mt-4">
          <span class="font-mono text-lg">{formatAddress($walletAccount.address)}</span>
        </div>
      </div>

      <div class="p-6 space-y-6">
        <!-- Wallet Address Section -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">Wallet Address</span>
          </div>
          <div class="flex items-center gap-2">
            <input
              type="text"
              readonly
              value={$walletAccount.address}
              class="flex-1 px-4 py-3 bg-gray-50 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg font-mono text-sm dark:text-gray-200"
            />
            <button
              onclick={() => copyToClipboard($walletAccount!.address, 'address')}
              class="p-3 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors border border-gray-200 dark:border-gray-600"
              title="Copy address"
            >
              {#if copied === 'address'}
                <Check class="w-5 h-5 text-green-600 dark:text-green-400" />
              {:else}
                <Copy class="w-5 h-5 text-gray-600 dark:text-gray-400" />
              {/if}
            </button>
          </div>
        </div>

        <!-- Private Key Section -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300 flex items-center gap-2">
              <Key class="w-4 h-4" />
              Private Key
            </span>
          </div>

          <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-800 rounded-lg p-3 mb-3">
            <div class="flex items-start gap-2">
              <AlertTriangle class="w-5 h-5 text-yellow-600 dark:text-yellow-400 flex-shrink-0 mt-0.5" />
              <p class="text-sm text-yellow-800 dark:text-yellow-300">
                <strong>Security Warning:</strong> Never share your private key with anyone. Anyone with your private key can access your wallet and all its contents.
              </p>
            </div>
          </div>

          <div class="flex items-center gap-2">
            <div class="flex-1 relative">
              <input
                type={privateKeyVisible ? 'text' : 'password'}
                readonly
                value={$walletAccount.privateKey}
                class="w-full px-4 py-3 bg-gray-50 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg font-mono text-sm pr-12 dark:text-gray-200"
              />
              <button
                onclick={() => privateKeyVisible = !privateKeyVisible}
                class="absolute right-3 top-1/2 -translate-y-1/2 p-1 hover:bg-gray-200 dark:hover:bg-gray-600 rounded transition-colors"
                title={privateKeyVisible ? 'Hide private key' : 'Show private key'}
              >
                {#if privateKeyVisible}
                  <EyeOff class="w-5 h-5 text-gray-600 dark:text-gray-400" />
                {:else}
                  <Eye class="w-5 h-5 text-gray-600 dark:text-gray-400" />
                {/if}
              </button>
            </div>
            <button
              onclick={() => copyToClipboard($walletAccount!.privateKey, 'privateKey')}
              class="p-3 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors border border-gray-200 dark:border-gray-600"
              title="Copy private key"
            >
              {#if copied === 'privateKey'}
                <Check class="w-5 h-5 text-green-600 dark:text-green-400" />
              {:else}
                <Copy class="w-5 h-5 text-gray-600 dark:text-gray-400" />
              {/if}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Send CHR Card -->
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-blue-100 dark:bg-blue-900/30 rounded-lg">
          <Send class="w-6 h-6 text-blue-600 dark:text-blue-400" />
        </div>
        <div>
          <h3 class="font-semibold dark:text-white">Send CHR</h3>
          <p class="text-sm text-gray-500 dark:text-gray-400">Transfer CHR to another address</p>
        </div>
      </div>

      {#if !showConfirmSend}
        <div class="space-y-4">
          <!-- Saved Recipients -->
          {#if savedRecipients.length > 0}
            <div>
              <span class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">Saved Recipients</span>
              <div class="flex flex-wrap gap-2">
                {#each [...savedRecipients].sort((a, b) => b.lastUsed - a.lastUsed) as r (r.id)}
                  <button
                    onclick={() => selectRecipient(r)}
                    class="group flex items-center gap-1.5 px-3 py-1.5 rounded-lg border text-sm transition-all {recipientAddress.toLowerCase() === r.address.toLowerCase() ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400' : 'border-gray-200 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'}"
                  >
                    <span class="font-medium">{r.label}</span>
                    <span class="text-xs text-gray-400 dark:text-gray-500 font-mono">{r.address.slice(0, 6)}...{r.address.slice(-4)}</span>
                    <span
                      role="button"
                      tabindex="0"
                      onclick={(e) => { e.stopPropagation(); deleteRecipient(r.id); }}
                      onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); deleteRecipient(r.id); } }}
                      class="ml-1 p-0.5 rounded hover:bg-red-100 dark:hover:bg-red-900/30 opacity-0 group-hover:opacity-100 transition-opacity"
                    >
                      <X class="w-3 h-3 text-gray-400 hover:text-red-500" />
                    </span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          <!-- Recipient Address -->
          <div>
            <div class="flex items-center justify-between mb-1">
              <label for="recipient" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                Recipient Address
              </label>
              {#if !showAddRecipient}
                <button
                  onclick={() => {
                    if (!recipientAddress || !recipientAddress.startsWith('0x') || recipientAddress.length !== 42) {
                      toasts.show('Enter a valid address first', 'error');
                      return;
                    }
                    showAddRecipient = true;
                  }}
                  class="flex items-center gap-1 text-xs text-blue-600 dark:text-blue-400 hover:text-blue-700 dark:hover:text-blue-300"
                >
                  <UserPlus class="w-3.5 h-3.5" />
                  Save
                </button>
              {/if}
            </div>
            {#if showAddRecipient}
              <div class="flex items-center gap-2 mb-2">
                <input
                  type="text"
                  bind:value={newRecipientLabel}
                  placeholder="Label (e.g. Alice)"
                  class="flex-1 px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 dark:bg-gray-700 dark:text-gray-200"
                  onkeydown={(e) => { if (e.key === 'Enter') addRecipient(); }}
                />
                <button
                  onclick={addRecipient}
                  disabled={!newRecipientLabel.trim()}
                  class="px-3 py-2 bg-blue-600 text-white text-sm rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
                >
                  Save
                </button>
                <button
                  onclick={() => { showAddRecipient = false; newRecipientLabel = ''; }}
                  class="p-2 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                >
                  <X class="w-4 h-4" />
                </button>
              </div>
            {/if}
            <input
              id="recipient"
              type="text"
              bind:value={recipientAddress}
              placeholder="0x..."
              class="w-full px-4 py-3 border border-gray-200 dark:border-gray-600 rounded-lg font-mono text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 dark:bg-gray-700 dark:text-gray-200"
            />
          </div>

          <!-- Amount -->
          <div>
            <label for="amount" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              Amount (CHR)
            </label>
            <div class="relative">
              <input
                id="amount"
                type="text"
                inputmode="decimal"
                pattern="[0-9]*\.?[0-9]*"
                bind:value={sendAmount}
                placeholder="0.00"
                class="w-full px-4 py-3 border border-gray-200 dark:border-gray-600 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 dark:bg-gray-700 dark:text-gray-200"
              />
              <button
                onclick={() => sendAmount = balance}
                class="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-blue-600 dark:text-blue-400 hover:text-blue-700 font-medium"
              >
                MAX
              </button>
            </div>
          </div>

          <button
            onclick={handleSend}
            disabled={!recipientAddress || !sendAmount}
            class="w-full px-4 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Send class="w-4 h-4" />
            Send CHR
          </button>
        </div>
      {:else}
        <!-- Confirmation -->
        <div class="space-y-4">
          <div class="flex items-center gap-2 mb-2">
            <AlertTriangle class="w-5 h-5 text-yellow-600 dark:text-yellow-400" />
            <span class="font-semibold dark:text-white">Confirm Transaction</span>
          </div>

          <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4 space-y-3">
            <div class="flex justify-between">
              <span class="text-sm text-gray-500 dark:text-gray-400">From</span>
              <span class="text-sm font-mono dark:text-gray-300">{formatAddress($walletAccount?.address || '')}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-sm text-gray-500 dark:text-gray-400">To</span>
              <span class="text-sm dark:text-gray-300">
                {#if getRecipientLabel(recipientAddress)}
                  <span class="font-medium">{getRecipientLabel(recipientAddress)}</span>
                  <span class="font-mono text-gray-400 dark:text-gray-500 ml-1">({formatAddress(recipientAddress)})</span>
                {:else}
                  <span class="font-mono">{formatAddress(recipientAddress)}</span>
                {/if}
              </span>
            </div>
            <div class="flex justify-between border-t border-gray-200 dark:border-gray-600 pt-3">
              <span class="text-sm text-gray-500 dark:text-gray-400">Amount</span>
              <span class="text-lg font-bold text-blue-600 dark:text-blue-400">{sendAmount} CHR</span>
            </div>
          </div>

          <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-800 rounded-lg p-3">
            <p class="text-sm text-yellow-800 dark:text-yellow-300">
              <strong>Warning:</strong> This transaction cannot be reversed.
            </p>
          </div>

          <div class="flex gap-3">
            <button
              onclick={() => showConfirmSend = false}
              disabled={isSending}
              class="flex-1 px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors disabled:opacity-50 dark:text-gray-300"
            >
              Back
            </button>
            <button
              onclick={confirmSend}
              disabled={isSending}
              class="flex-1 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors flex items-center justify-center gap-2 disabled:opacity-50"
            >
              {#if isSending}
                <Loader2 class="w-4 h-4 animate-spin" />
                Sending...
              {:else}
                <Check class="w-4 h-4" />
                Confirm Send
              {/if}
            </button>
          </div>
        </div>
      {/if}
    </div>
    </div>

    <!-- Transaction History Card -->
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-3">
          <div class="p-2 bg-indigo-100 dark:bg-indigo-900/30 rounded-lg">
            <History class="w-6 h-6 text-indigo-600 dark:text-indigo-400" />
          </div>
          <div>
            <h3 class="font-semibold dark:text-white">Transaction History</h3>
            <p class="text-sm text-gray-500 dark:text-gray-400">Recent transactions</p>
          </div>
        </div>
        <button
          onclick={loadTransactionHistory}
          disabled={isLoadingHistory}
          class="p-2 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors disabled:opacity-50 dark:text-gray-300"
          title="Refresh history"
        >
          <RefreshCw class="w-5 h-5 {isLoadingHistory ? 'animate-spin' : ''}" />
        </button>
      </div>

      {#if isLoadingHistory}
        <div class="flex items-center justify-center py-8">
          <Loader2 class="w-8 h-8 animate-spin text-gray-400" />
        </div>
      {:else if transactions.length === 0}
        <div class="text-center py-8 text-gray-500 dark:text-gray-400">
          <History class="w-12 h-12 mx-auto mb-2 opacity-50" />
          <p>No transactions yet</p>
          <p class="text-sm">Your transaction history will appear here</p>
        </div>
      {:else}
        <div class="space-y-3 max-h-[500px] overflow-y-auto">
          {#each transactions as tx}
            {@const style = getTxTypeStyle(tx)}
            {@const isExpanded = expandedTxHash === tx.hash}
            <div class="bg-gray-50 dark:bg-gray-700 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-600 transition-colors">
              <!-- Main row -->
              <button
                onclick={() => expandedTxHash = isExpanded ? null : tx.hash}
                class="w-full flex items-center gap-4 p-3 text-left"
              >
                <div class="p-2 {style.bgColor} rounded-full flex-shrink-0">
                  {#if tx.txType === 'speed_tier_payment'}
                    <Zap class="w-5 h-5 {style.iconColor}" />
                  {:else if tx.txType === 'file_payment' || tx.txType === 'file_sale'}
                    <FileIcon class="w-5 h-5 {style.iconColor}" />
                  {:else if isIncoming(tx)}
                    <ArrowDownLeft class="w-5 h-5 {style.iconColor}" />
                  {:else}
                    <ArrowUpRight class="w-5 h-5 {style.iconColor}" />
                  {/if}
                </div>
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <span class="font-medium {style.iconColor}">
                      {isIncoming(tx) ? '+' : '-'}{tx.value} CHR
                    </span>
                    <span class="text-xs px-2 py-0.5 rounded-full {
                      tx.txType === 'speed_tier_payment'
                        ? 'bg-amber-100 text-amber-800 dark:bg-amber-900/50 dark:text-amber-300'
                        : tx.txType === 'file_payment'
                          ? 'bg-purple-100 text-purple-800 dark:bg-purple-900/50 dark:text-purple-300'
                          : tx.txType === 'file_sale'
                            ? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/50 dark:text-emerald-300'
                            : 'bg-gray-200 dark:bg-gray-600 dark:text-gray-300'
                    }">
                      {getTxTypeLabel(tx)}
                    </span>
                    <span class="text-xs px-2 py-0.5 bg-gray-200 dark:bg-gray-600 dark:text-gray-300 rounded-full">{tx.status}</span>
                  </div>
                  <p class="text-sm text-gray-600 dark:text-gray-300 mt-0.5 truncate">
                    {tx.description || (isIncoming(tx) ? `From: ${formatAddress(tx.from)}` : `To: ${formatAddress(tx.to)}`)}
                  </p>
                  <div class="text-xs text-gray-400 mt-0.5">
                    {formatTimestamp(tx.timestamp)}
                  </div>
                </div>
                <div class="flex items-center gap-1 flex-shrink-0">
                  {#if isExpanded}
                    <ChevronUp class="w-4 h-4 text-gray-400" />
                  {:else}
                    <ChevronDown class="w-4 h-4 text-gray-400" />
                  {/if}
                </div>
              </button>

              <!-- Expanded details -->
              {#if isExpanded}
                <div class="px-4 pb-4 pt-1 border-t border-gray-200 dark:border-gray-600 space-y-2">
                  <!-- File info for download payments -->
                  {#if tx.fileName}
                    <div class="flex items-center gap-2 p-2 bg-amber-50 dark:bg-amber-900/20 rounded-lg">
                      <FileIcon class="w-4 h-4 text-amber-600 dark:text-amber-400 flex-shrink-0" />
                      <div class="min-w-0">
                        <p class="text-sm font-medium dark:text-white truncate">{tx.fileName}</p>
                        {#if tx.speedTier}
                          <p class="text-xs text-amber-600 dark:text-amber-400">⚡ {tx.speedTier.charAt(0).toUpperCase() + tx.speedTier.slice(1)} tier</p>
                        {/if}
                      </div>
                    </div>
                  {/if}

                  <!-- Transaction details grid -->
                  <div class="grid grid-cols-2 gap-2 text-xs">
                    <div>
                      <span class="text-gray-400">From</span>
                      <p class="font-mono text-gray-700 dark:text-gray-300 truncate">{tx.from}</p>
                    </div>
                    <div>
                      <span class="text-gray-400">To {tx.recipientLabel ? `(${tx.recipientLabel})` : ''}</span>
                      <p class="font-mono text-gray-700 dark:text-gray-300 truncate">{tx.to}</p>
                    </div>
                    <div>
                      <span class="text-gray-400">Block</span>
                      <p class="text-gray-700 dark:text-gray-300">#{tx.blockNumber}</p>
                    </div>
                    <div>
                      <span class="text-gray-400">Gas Used</span>
                      <p class="text-gray-700 dark:text-gray-300">{tx.gasUsed.toLocaleString()}</p>
                    </div>
                    {#if tx.balanceBefore && tx.balanceAfter}
                      <div>
                        <span class="text-gray-400">Balance Before</span>
                        <p class="text-gray-700 dark:text-gray-300">{tx.balanceBefore} CHR</p>
                      </div>
                      <div>
                        <span class="text-gray-400">Balance After</span>
                        <p class="text-gray-700 dark:text-gray-300">{tx.balanceAfter} CHR</p>
                      </div>
                    {/if}
                    {#if tx.fileHash}
                      <div class="col-span-2">
                        <span class="text-gray-400">File Hash</span>
                        <p class="font-mono text-gray-700 dark:text-gray-300 truncate">{tx.fileHash}</p>
                      </div>
                    {/if}
                    <div class="col-span-2">
                      <span class="text-gray-400">Transaction Hash</span>
                      <div class="flex items-center gap-2">
                        <p class="font-mono text-gray-700 dark:text-gray-300 truncate flex-1">{tx.hash}</p>
                        <button
                          onclick={(e) => { e.stopPropagation(); navigator.clipboard.writeText(tx.hash); toasts.show('Transaction hash copied', 'success'); }}
                          class="p-1 hover:bg-gray-200 dark:hover:bg-gray-600 rounded transition-colors flex-shrink-0"
                          title="Copy transaction hash"
                        >
                          <Copy class="w-3.5 h-3.5 text-gray-400" />
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>

  {:else}
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-12 text-center">
      <Wallet class="w-16 h-16 mx-auto text-gray-300 dark:text-gray-600 mb-4" />
      <h2 class="text-xl font-semibold text-gray-700 dark:text-gray-300 mb-2">No Wallet Connected</h2>
      <p class="text-gray-500 dark:text-gray-400 mb-6">Please create or import a wallet to view account details.</p>
    </div>
  {/if}
</div>

<!-- Logout Modal -->
{#if showLogoutModal}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50" role="dialog" aria-modal="true" tabindex="-1" onclick={() => showLogoutModal = false} onkeydown={(e) => e.key === 'Escape' && (showLogoutModal = false)}>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-xl p-6 max-w-md mx-4" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-red-100 dark:bg-red-900/30 rounded-lg">
          <LogOut class="w-6 h-6 text-red-600 dark:text-red-400" />
        </div>
        <h3 class="text-lg font-semibold dark:text-white">Logout</h3>
      </div>

      <p class="text-sm text-gray-600 dark:text-gray-400 mb-6">
        Are you sure you want to logout? Make sure you have saved your recovery phrase or exported your wallet before logging out.
      </p>

      <div class="flex gap-3">
        <button
          onclick={() => showLogoutModal = false}
          class="flex-1 px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors dark:text-gray-300"
        >
          Cancel
        </button>
        <button
          onclick={logout}
          class="flex-1 px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors flex items-center justify-center gap-2"
        >
          <LogOut class="w-4 h-4" />
          Logout
        </button>
      </div>
    </div>
  </div>
{/if}

