<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { walletAccount, isAuthenticated, networkConnected } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import { walletService } from '$lib/services/walletService';
  import {
    Wallet,
    Copy,
    Eye,
    EyeOff,
    Download,
    LogOut,
    Shield,
    AlertTriangle,
    Check,
    Key,
    User,
    RefreshCw,
    Coins,
    Send,
    History,
    ArrowUpRight,
    ArrowDownLeft,
    Loader2,
    ExternalLink
  } from 'lucide-svelte';

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
  }

  // State
  let privateKeyVisible = $state(false);
  let copied = $state<'address' | 'privateKey' | null>(null);
  let showExportModal = $state(false);
  let showLogoutModal = $state(false);
  let showSendModal = $state(false);
  let balance = $state<string>('0.00');
  let isLoadingBalance = $state(false);

  // Send transaction state
  let recipientAddress = $state('');
  let sendAmount = $state('');
  let isSending = $state(false);
  let showConfirmSend = $state(false);

  // Transaction history state
  let transactions = $state<Transaction[]>([]);
  let isLoadingHistory = $state(false);

  // Faucet state
  let isRequestingFaucet = $state(false);

  // Check if Tauri is available
  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  // Load balance on mount and when wallet changes
  onMount(() => {
    if ($walletAccount?.address) {
      loadBalance();
      loadTransactionHistory();
    }
  });

  // Watch for wallet changes
  $effect(() => {
    if ($walletAccount?.address) {
      loadBalance();
      loadTransactionHistory();
    }
  });

  // Load wallet balance
  async function loadBalance() {
    if (!$walletAccount?.address) return;

    isLoadingBalance = true;
    try {
      const result = await walletService.getBalance($walletAccount.address);
      balance = result;
    } catch (error) {
      console.error('Failed to load balance:', error);
      balance = '0.00';
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
      console.error('Failed to load transaction history:', error);
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
      const result = await invoke<{ hash: string; status: string }>('send_transaction', {
        fromAddress: $walletAccount.address,
        toAddress: recipientAddress,
        amount: sendAmount,
        privateKey: $walletAccount.privateKey
      });

      toasts.show(`Transaction sent! Hash: ${result.hash.slice(0, 10)}...`, 'success');

      // Reset form
      recipientAddress = '';
      sendAmount = '';
      showConfirmSend = false;
      showSendModal = false;

      // Refresh balance and history
      setTimeout(() => {
        loadBalance();
        loadTransactionHistory();
      }, 2000);
    } catch (error) {
      console.error('Failed to send transaction:', error);
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
      console.error('Failed to copy:', error);
      toasts.show('Failed to copy to clipboard', 'error');
    }
  }

  // Export wallet
  function exportWallet() {
    if (!$walletAccount) return;

    const walletData = {
      address: $walletAccount.address,
      privateKey: $walletAccount.privateKey,
      exportedAt: new Date().toISOString(),
      network: 'Chiral Network'
    };

    const blob = new Blob([JSON.stringify(walletData, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `chiral-wallet-${$walletAccount.address.slice(0, 8)}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);

    showExportModal = false;
    toasts.show('Wallet exported successfully', 'success');
  }

  // Logout
  function logout() {
    walletAccount.set(null);
    isAuthenticated.set(false);
    networkConnected.set(false);
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

  // Request test CHR from faucet
  async function requestTestCHR() {
    if (!$walletAccount || !isTauri()) return;

    isRequestingFaucet = true;
    try {
      const result = await invoke<{ hash: string; status: string }>('request_faucet', {
        address: $walletAccount.address
      });

      toasts.show('Test CHR requested! Waiting for confirmation...', 'success');

      // Refresh balance after a delay
      setTimeout(() => {
        loadBalance();
        loadTransactionHistory();
      }, 3000);
    } catch (error) {
      console.error('Faucet request failed:', error);
      toasts.show(`${error}`, 'error');
    } finally {
      isRequestingFaucet = false;
    }
  }
</script>

<div class="p-6 space-y-6 max-w-4xl mx-auto">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">Account</h1>
      <p class="text-gray-600 mt-1">Manage your wallet and account settings</p>
    </div>
    <button
      onclick={() => showLogoutModal = true}
      class="flex items-center gap-2 px-4 py-2 text-red-600 hover:bg-red-50 rounded-lg transition-colors"
    >
      <LogOut class="w-5 h-5" />
      Logout
    </button>
  </div>

  {#if $walletAccount}
    <!-- Wallet Overview Card -->
    <div class="bg-white rounded-xl shadow-sm border border-gray-200 overflow-hidden">
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
            <span class="text-sm font-medium text-gray-700">Wallet Address</span>
          </div>
          <div class="flex items-center gap-2">
            <input
              type="text"
              readonly
              value={$walletAccount.address}
              class="flex-1 px-4 py-3 bg-gray-50 border border-gray-200 rounded-lg font-mono text-sm"
            />
            <button
              onclick={() => copyToClipboard($walletAccount!.address, 'address')}
              class="p-3 hover:bg-gray-100 rounded-lg transition-colors border border-gray-200"
              title="Copy address"
            >
              {#if copied === 'address'}
                <Check class="w-5 h-5 text-green-600" />
              {:else}
                <Copy class="w-5 h-5 text-gray-600" />
              {/if}
            </button>
          </div>
        </div>

        <!-- Private Key Section -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <span class="text-sm font-medium text-gray-700 flex items-center gap-2">
              <Key class="w-4 h-4" />
              Private Key
            </span>
          </div>

          <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-3 mb-3">
            <div class="flex items-start gap-2">
              <AlertTriangle class="w-5 h-5 text-yellow-600 flex-shrink-0 mt-0.5" />
              <p class="text-sm text-yellow-800">
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
                class="w-full px-4 py-3 bg-gray-50 border border-gray-200 rounded-lg font-mono text-sm pr-12"
              />
              <button
                onclick={() => privateKeyVisible = !privateKeyVisible}
                class="absolute right-3 top-1/2 -translate-y-1/2 p-1 hover:bg-gray-200 rounded transition-colors"
                title={privateKeyVisible ? 'Hide private key' : 'Show private key'}
              >
                {#if privateKeyVisible}
                  <EyeOff class="w-5 h-5 text-gray-600" />
                {:else}
                  <Eye class="w-5 h-5 text-gray-600" />
                {/if}
              </button>
            </div>
            <button
              onclick={() => copyToClipboard($walletAccount!.privateKey, 'privateKey')}
              class="p-3 hover:bg-gray-100 rounded-lg transition-colors border border-gray-200"
              title="Copy private key"
            >
              {#if copied === 'privateKey'}
                <Check class="w-5 h-5 text-green-600" />
              {:else}
                <Copy class="w-5 h-5 text-gray-600" />
              {/if}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Balance & Export Section -->
    <div class="grid md:grid-cols-2 gap-6">
      <!-- Balance Info Card -->
      <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
        <div class="flex items-center gap-3 mb-4">
          <div class="p-2 bg-yellow-100 rounded-lg">
            <Coins class="w-6 h-6 text-yellow-600" />
          </div>
          <div>
            <h3 class="font-semibold">CHR Token</h3>
            <p class="text-sm text-gray-500">Chiral Network native token</p>
          </div>
        </div>
        <div class="space-y-3">
          <div class="flex justify-between items-center py-2 border-b border-gray-100">
            <span class="text-sm text-gray-600">Available Balance</span>
            <span class="font-medium">{formatBalance(balance)} CHR</span>
          </div>
          <div class="flex justify-between items-center py-2 border-b border-gray-100">
            <span class="text-sm text-gray-600">Network</span>
            <span class="font-medium">Chiral Mainnet</span>
          </div>
          <div class="flex justify-between items-center py-2">
            <span class="text-sm text-gray-600">Token Symbol</span>
            <span class="font-medium">CHR</span>
          </div>
        </div>

        <!-- Get Test CHR Button (for testing) -->
        <button
          onclick={requestTestCHR}
          disabled={isRequestingFaucet || parseFloat(balance) > 0}
          class="w-full mt-4 px-4 py-2 bg-yellow-500 text-white rounded-lg hover:bg-yellow-600 transition-colors flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
          title={parseFloat(balance) > 0 ? 'You already have CHR' : 'Get 1 test CHR'}
        >
          {#if isRequestingFaucet}
            <Loader2 class="w-4 h-4 animate-spin" />
            Requesting...
          {:else}
            <Coins class="w-4 h-4" />
            Get Test CHR (1 CHR)
          {/if}
        </button>
        <p class="text-xs text-gray-500 mt-2 text-center">
          For testing only. Start Geth and mine blocks if faucet is unavailable.
        </p>
      </div>

      <!-- Export Wallet Card -->
      <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
        <div class="flex items-center gap-3 mb-4">
          <div class="p-2 bg-green-100 rounded-lg">
            <Download class="w-6 h-6 text-green-600" />
          </div>
          <div>
            <h3 class="font-semibold">Export Wallet</h3>
            <p class="text-sm text-gray-500">Download your wallet backup</p>
          </div>
        </div>
        <p class="text-sm text-gray-600 mb-4">
          Export your wallet to a JSON file for backup. Keep this file secure and never share it.
        </p>
        <button
          onclick={() => showExportModal = true}
          class="w-full px-4 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 transition-colors flex items-center justify-center gap-2"
        >
          <Download class="w-4 h-4" />
          Export Wallet
        </button>
      </div>
    </div>

    <!-- Send CHR Card -->
    <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-3">
          <div class="p-2 bg-blue-100 rounded-lg">
            <Send class="w-6 h-6 text-blue-600" />
          </div>
          <div>
            <h3 class="font-semibold">Send CHR</h3>
            <p class="text-sm text-gray-500">Transfer CHR to another address</p>
          </div>
        </div>
        <button
          onclick={() => showSendModal = true}
          class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors flex items-center gap-2"
        >
          <Send class="w-4 h-4" />
          Send
        </button>
      </div>
    </div>

    <!-- Transaction History Card -->
    <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-3">
          <div class="p-2 bg-indigo-100 rounded-lg">
            <History class="w-6 h-6 text-indigo-600" />
          </div>
          <div>
            <h3 class="font-semibold">Transaction History</h3>
            <p class="text-sm text-gray-500">Recent transactions</p>
          </div>
        </div>
        <button
          onclick={loadTransactionHistory}
          disabled={isLoadingHistory}
          class="p-2 hover:bg-gray-100 rounded-lg transition-colors disabled:opacity-50"
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
        <div class="text-center py-8 text-gray-500">
          <History class="w-12 h-12 mx-auto mb-2 opacity-50" />
          <p>No transactions yet</p>
          <p class="text-sm">Your transaction history will appear here</p>
        </div>
      {:else}
        <div class="space-y-3 max-h-80 overflow-y-auto">
          {#each transactions as tx}
            <div class="flex items-center gap-4 p-3 bg-gray-50 rounded-lg hover:bg-gray-100 transition-colors">
              <div class="p-2 {isIncoming(tx) ? 'bg-green-100' : 'bg-red-100'} rounded-full">
                {#if isIncoming(tx)}
                  <ArrowDownLeft class="w-5 h-5 text-green-600" />
                {:else}
                  <ArrowUpRight class="w-5 h-5 text-red-600" />
                {/if}
              </div>
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <span class="font-medium {isIncoming(tx) ? 'text-green-600' : 'text-red-600'}">
                    {isIncoming(tx) ? '+' : '-'}{tx.value} CHR
                  </span>
                  <span class="text-xs px-2 py-0.5 bg-gray-200 rounded-full">{tx.status}</span>
                </div>
                <div class="text-sm text-gray-500 truncate">
                  {isIncoming(tx) ? 'From:' : 'To:'} {formatAddress(isIncoming(tx) ? tx.from : tx.to)}
                </div>
                <div class="text-xs text-gray-400">
                  Block #{tx.blockNumber} | {formatTimestamp(tx.timestamp)}
                </div>
              </div>
              <button
                onclick={() => navigator.clipboard.writeText(tx.hash)}
                class="p-2 hover:bg-gray-200 rounded-lg transition-colors"
                title="Copy transaction hash"
              >
                <Copy class="w-4 h-4 text-gray-400" />
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Security & Account Details Section -->
    <div class="grid md:grid-cols-2 gap-6">
      <!-- Security Info Card -->
      <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
        <div class="flex items-center gap-3 mb-4">
          <div class="p-2 bg-blue-100 rounded-lg">
            <Shield class="w-6 h-6 text-blue-600" />
          </div>
          <div>
            <h3 class="font-semibold">Security Tips</h3>
            <p class="text-sm text-gray-500">Keep your wallet safe</p>
          </div>
        </div>
        <ul class="text-sm text-gray-600 space-y-2">
          <li class="flex items-start gap-2">
            <Check class="w-4 h-4 text-green-500 flex-shrink-0 mt-0.5" />
            <span>Store your recovery phrase offline</span>
          </li>
          <li class="flex items-start gap-2">
            <Check class="w-4 h-4 text-green-500 flex-shrink-0 mt-0.5" />
            <span>Never share your private key</span>
          </li>
          <li class="flex items-start gap-2">
            <Check class="w-4 h-4 text-green-500 flex-shrink-0 mt-0.5" />
            <span>Use strong passwords</span>
          </li>
          <li class="flex items-start gap-2">
            <Check class="w-4 h-4 text-green-500 flex-shrink-0 mt-0.5" />
            <span>Keep your software updated</span>
          </li>
        </ul>
      </div>

      <!-- Account Details Card -->
      <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
        <div class="flex items-center gap-3 mb-4">
          <div class="p-2 bg-purple-100 rounded-lg">
            <User class="w-6 h-6 text-purple-600" />
          </div>
          <div>
            <h3 class="font-semibold">Account Details</h3>
            <p class="text-sm text-gray-500">Technical information</p>
          </div>
        </div>

        <div class="space-y-3">
          <div class="flex justify-between items-center py-2 border-b border-gray-100">
            <span class="text-sm text-gray-600">Network</span>
            <span class="font-medium">Chiral Network</span>
          </div>
          <div class="flex justify-between items-center py-2 border-b border-gray-100">
            <span class="text-sm text-gray-600">Address Format</span>
            <span class="font-medium">EVM Compatible</span>
          </div>
          <div class="flex justify-between items-center py-2 border-b border-gray-100">
            <span class="text-sm text-gray-600">Connection</span>
            <span class="font-medium flex items-center gap-2">
              <span class="w-2 h-2 rounded-full {$networkConnected ? 'bg-green-500' : 'bg-red-500'}"></span>
              {$networkConnected ? 'Connected' : 'Disconnected'}
            </span>
          </div>
          <div class="flex justify-between items-center py-2">
            <span class="text-sm text-gray-600">Key Type</span>
            <span class="font-medium">secp256k1</span>
          </div>
        </div>
      </div>
    </div>

  {:else}
    <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-12 text-center">
      <Wallet class="w-16 h-16 mx-auto text-gray-300 mb-4" />
      <h2 class="text-xl font-semibold text-gray-700 mb-2">No Wallet Connected</h2>
      <p class="text-gray-500 mb-6">Please create or import a wallet to view account details.</p>
    </div>
  {/if}
</div>

<!-- Export Modal -->
{#if showExportModal}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50" role="dialog" aria-modal="true" tabindex="-1" onclick={() => showExportModal = false} onkeydown={(e) => e.key === 'Escape' && (showExportModal = false)}>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="bg-white rounded-xl shadow-xl p-6 max-w-md mx-4" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-yellow-100 rounded-lg">
          <AlertTriangle class="w-6 h-6 text-yellow-600" />
        </div>
        <h3 class="text-lg font-semibold">Export Wallet</h3>
      </div>

      <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-4 mb-4">
        <p class="text-sm text-yellow-800">
          <strong>Warning:</strong> This file will contain your private key. Anyone with access to this file can control your wallet. Store it securely and never share it.
        </p>
      </div>

      <p class="text-sm text-gray-600 mb-6">
        Your wallet will be exported as a JSON file containing your address and private key.
      </p>

      <div class="flex gap-3">
        <button
          onclick={() => showExportModal = false}
          class="flex-1 px-4 py-2 border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors"
        >
          Cancel
        </button>
        <button
          onclick={exportWallet}
          class="flex-1 px-4 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 transition-colors flex items-center justify-center gap-2"
        >
          <Download class="w-4 h-4" />
          Export
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Logout Modal -->
{#if showLogoutModal}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50" role="dialog" aria-modal="true" tabindex="-1" onclick={() => showLogoutModal = false} onkeydown={(e) => e.key === 'Escape' && (showLogoutModal = false)}>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="bg-white rounded-xl shadow-xl p-6 max-w-md mx-4" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-red-100 rounded-lg">
          <LogOut class="w-6 h-6 text-red-600" />
        </div>
        <h3 class="text-lg font-semibold">Logout</h3>
      </div>

      <p class="text-sm text-gray-600 mb-6">
        Are you sure you want to logout? Make sure you have saved your recovery phrase or exported your wallet before logging out.
      </p>

      <div class="flex gap-3">
        <button
          onclick={() => showLogoutModal = false}
          class="flex-1 px-4 py-2 border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors"
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

<!-- Send CHR Modal -->
{#if showSendModal}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50" role="dialog" aria-modal="true" tabindex="-1" onclick={() => { showSendModal = false; showConfirmSend = false; }} onkeydown={(e) => e.key === 'Escape' && (showSendModal = false)}>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="bg-white rounded-xl shadow-xl p-6 max-w-md w-full mx-4" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      {#if !showConfirmSend}
        <!-- Send Form -->
        <div class="flex items-center gap-3 mb-6">
          <div class="p-2 bg-blue-100 rounded-lg">
            <Send class="w-6 h-6 text-blue-600" />
          </div>
          <h3 class="text-lg font-semibold">Send CHR</h3>
        </div>

        <div class="space-y-4">
          <!-- Available Balance -->
          <div class="bg-gray-50 rounded-lg p-3">
            <p class="text-sm text-gray-500">Available Balance</p>
            <p class="text-xl font-bold">{formatBalance(balance)} CHR</p>
          </div>

          <!-- Recipient Address -->
          <div>
            <label for="recipient" class="block text-sm font-medium text-gray-700 mb-1">
              Recipient Address
            </label>
            <input
              id="recipient"
              type="text"
              bind:value={recipientAddress}
              placeholder="0x..."
              class="w-full px-4 py-3 border border-gray-200 rounded-lg font-mono text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>

          <!-- Amount -->
          <div>
            <label for="amount" class="block text-sm font-medium text-gray-700 mb-1">
              Amount (CHR)
            </label>
            <div class="relative">
              <input
                id="amount"
                type="number"
                step="0.000001"
                min="0"
                bind:value={sendAmount}
                placeholder="0.00"
                class="w-full px-4 py-3 border border-gray-200 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              />
              <button
                onclick={() => sendAmount = balance}
                class="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-blue-600 hover:text-blue-700 font-medium"
              >
                MAX
              </button>
            </div>
          </div>
        </div>

        <div class="flex gap-3 mt-6">
          <button
            onclick={() => showSendModal = false}
            class="flex-1 px-4 py-2 border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors"
          >
            Cancel
          </button>
          <button
            onclick={handleSend}
            disabled={!recipientAddress || !sendAmount}
            class="flex-1 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Send class="w-4 h-4" />
            Continue
          </button>
        </div>
      {:else}
        <!-- Confirmation Screen -->
        <div class="flex items-center gap-3 mb-6">
          <div class="p-2 bg-yellow-100 rounded-lg">
            <AlertTriangle class="w-6 h-6 text-yellow-600" />
          </div>
          <h3 class="text-lg font-semibold">Confirm Transaction</h3>
        </div>

        <div class="bg-gray-50 rounded-lg p-4 space-y-3 mb-6">
          <div class="flex justify-between">
            <span class="text-sm text-gray-500">From</span>
            <span class="text-sm font-mono">{formatAddress($walletAccount?.address || '')}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm text-gray-500">To</span>
            <span class="text-sm font-mono">{formatAddress(recipientAddress)}</span>
          </div>
          <div class="flex justify-between border-t border-gray-200 pt-3">
            <span class="text-sm text-gray-500">Amount</span>
            <span class="text-lg font-bold text-blue-600">{sendAmount} CHR</span>
          </div>
        </div>

        <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-3 mb-6">
          <p class="text-sm text-yellow-800">
            <strong>Warning:</strong> This transaction cannot be reversed. Please verify the recipient address and amount before confirming.
          </p>
        </div>

        <div class="flex gap-3">
          <button
            onclick={() => showConfirmSend = false}
            disabled={isSending}
            class="flex-1 px-4 py-2 border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors disabled:opacity-50"
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
      {/if}
    </div>
  </div>
{/if}
