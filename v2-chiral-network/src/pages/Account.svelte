<script lang="ts">
  import { onMount } from 'svelte';
  import { walletAccount, isAuthenticated, networkConnected } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
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
    QrCode,
    Key,
    User,
    ExternalLink
  } from 'lucide-svelte';

  // State
  let privateKeyVisible = $state(false);
  let copied = $state<'address' | 'privateKey' | null>(null);
  let showExportModal = $state(false);
  let showLogoutModal = $state(false);
  let qrCodeDataUrl = $state('');
  let showQrModal = $state(false);

  // Generate QR code for address
  async function generateQrCode() {
    if (!$walletAccount?.address) return;

    try {
      // Using a simple QR code generation via canvas
      const QRCode = await import('qrcode');
      qrCodeDataUrl = await QRCode.toDataURL($walletAccount.address, {
        width: 256,
        margin: 2,
        color: {
          dark: '#000000',
          light: '#ffffff'
        }
      });
      showQrModal = true;
    } catch (error) {
      console.error('Failed to generate QR code:', error);
      toasts.show('Failed to generate QR code', 'error');
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
        <div class="flex items-center gap-3 mb-4">
          <div class="p-3 bg-white/20 rounded-full">
            <Wallet class="w-8 h-8" />
          </div>
          <div>
            <h2 class="text-xl font-semibold">Chiral Wallet</h2>
            <p class="text-blue-100 text-sm">Your decentralized identity</p>
          </div>
        </div>

        <div class="flex items-center gap-2">
          <span class="font-mono text-lg">{formatAddress($walletAccount.address)}</span>
          <span class="px-2 py-0.5 bg-white/20 rounded text-xs">
            {$networkConnected ? 'Connected' : 'Disconnected'}
          </span>
        </div>
      </div>

      <div class="p-6 space-y-6">
        <!-- Wallet Address Section -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <span class="text-sm font-medium text-gray-700">Wallet Address</span>
            <div class="flex gap-2">
              <button
                onclick={generateQrCode}
                class="flex items-center gap-1 px-2 py-1 text-xs text-gray-600 hover:bg-gray-100 rounded transition-colors"
                title="Show QR Code"
              >
                <QrCode class="w-4 h-4" />
                QR
              </button>
            </div>
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
            <label class="text-sm font-medium text-gray-700 flex items-center gap-2">
              <Key class="w-4 h-4" />
              Private Key
            </label>
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

    <!-- Security & Export Section -->
    <div class="grid md:grid-cols-2 gap-6">
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
    </div>

    <!-- Account Details Card -->
    <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-purple-100 rounded-lg">
          <User class="w-6 h-6 text-purple-600" />
        </div>
        <div>
          <h3 class="font-semibold">Account Details</h3>
          <p class="text-sm text-gray-500">Technical information about your account</p>
        </div>
      </div>

      <div class="grid md:grid-cols-2 gap-4">
        <div class="bg-gray-50 rounded-lg p-4">
          <p class="text-xs text-gray-500 mb-1">Network</p>
          <p class="font-medium">Chiral Network</p>
        </div>
        <div class="bg-gray-50 rounded-lg p-4">
          <p class="text-xs text-gray-500 mb-1">Address Format</p>
          <p class="font-medium">Ethereum-compatible (EVM)</p>
        </div>
        <div class="bg-gray-50 rounded-lg p-4">
          <p class="text-xs text-gray-500 mb-1">Connection Status</p>
          <p class="font-medium flex items-center gap-2">
            <span class="w-2 h-2 rounded-full {$networkConnected ? 'bg-green-500' : 'bg-red-500'}"></span>
            {$networkConnected ? 'Connected to DHT' : 'Not Connected'}
          </p>
        </div>
        <div class="bg-gray-50 rounded-lg p-4">
          <p class="text-xs text-gray-500 mb-1">Key Type</p>
          <p class="font-medium">secp256k1</p>
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

<!-- QR Code Modal -->
{#if showQrModal}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50" role="dialog" aria-modal="true" tabindex="-1" onclick={() => showQrModal = false} onkeydown={(e) => e.key === 'Escape' && (showQrModal = false)}>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="bg-white rounded-xl shadow-xl p-6 max-w-sm mx-4" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <h3 class="text-lg font-semibold mb-4 text-center">Wallet Address QR Code</h3>
      {#if qrCodeDataUrl}
        <div class="bg-white p-4 rounded-lg border border-gray-200 mb-4">
          <img src={qrCodeDataUrl} alt="QR Code" class="mx-auto" />
        </div>
      {/if}
      <p class="text-xs text-gray-500 text-center mb-4 font-mono break-all">
        {$walletAccount?.address}
      </p>
      <button
        onclick={() => showQrModal = false}
        class="w-full px-4 py-2 bg-gray-100 hover:bg-gray-200 rounded-lg transition-colors"
      >
        Close
      </button>
    </div>
  </div>
{/if}

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
