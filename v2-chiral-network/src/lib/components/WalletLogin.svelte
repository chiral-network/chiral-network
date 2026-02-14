<script lang="ts">
  import { createWalletFromPrivateKey, createWalletFromMnemonic, isValidPrivateKey, isValidMnemonic } from '$lib/walletService';
  import { walletAccount, isAuthenticated } from '$lib/stores';
  import { ArrowLeft, KeyRound, FileText } from 'lucide-svelte';

  export let onBack: () => void;
  export let onComplete: () => void;

  let method: 'privateKey' | 'mnemonic' = 'privateKey';
  let privateKeyInput = '';
  let mnemonicInput = '';
  let error = '';

  function handleLogin() {
    error = '';

    try {
      if (method === 'privateKey') {
        if (!privateKeyInput.trim()) {
          error = 'Please enter your private key';
          return;
        }

        if (!isValidPrivateKey(privateKeyInput)) {
          error = 'Invalid private key format';
          return;
        }

        const wallet = createWalletFromPrivateKey(privateKeyInput);
        walletAccount.set({
          address: wallet.address,
          privateKey: wallet.privateKey
        });
        isAuthenticated.set(true);
        onComplete();
      } else {
        if (!mnemonicInput.trim()) {
          error = 'Please enter your recovery phrase';
          return;
        }

        if (!isValidMnemonic(mnemonicInput.trim())) {
          error = 'Invalid recovery phrase. Please check your words and try again.';
          return;
        }

        const wallet = createWalletFromMnemonic(mnemonicInput.trim());
        walletAccount.set({
          address: wallet.address,
          privateKey: wallet.privateKey
        });
        isAuthenticated.set(true);
        onComplete();
      }
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to login';
    }
  }
</script>

<div class="max-w-2xl mx-auto p-6">
  <div class="bg-white dark:bg-gray-800 rounded-lg shadow-lg p-8">
    <div class="flex items-center mb-6">
      <button on:click={onBack} class="mr-4 p-2 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition">
        <ArrowLeft class="w-5 h-5 dark:text-gray-300" />
      </button>
      <h2 class="text-2xl font-bold dark:text-white">Use Existing Wallet</h2>
    </div>

    <div class="flex gap-3 mb-6">
      <button
        on:click={() => method = 'privateKey'}
        class="flex-1 flex items-center justify-center gap-2 px-4 py-3 rounded-lg transition {method === 'privateKey' ? 'bg-blue-600 text-white' : 'bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 dark:text-gray-300'}"
      >
        <KeyRound class="w-5 h-5" />
        <span>Private Key</span>
      </button>

      <button
        on:click={() => method = 'mnemonic'}
        class="flex-1 flex items-center justify-center gap-2 px-4 py-3 rounded-lg transition {method === 'mnemonic' ? 'bg-blue-600 text-white' : 'bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 dark:text-gray-300'}"
      >
        <FileText class="w-5 h-5" />
        <span>Recovery Phrase</span>
      </button>
    </div>

    {#if method === 'privateKey'}
      <div class="mb-6">
        <label for="private-key-input" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
          Private Key
        </label>
        <input
          id="private-key-input"
          type="password"
          bind:value={privateKeyInput}
          on:keydown={(e) => e.key === 'Enter' && handleLogin()}
          class="w-full px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-transparent font-mono text-sm bg-white dark:bg-gray-700 dark:text-white"
          placeholder="Enter your private key (with or without 0x prefix)"
        />
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-2">
          Your private key should be a 64-character hexadecimal string
        </p>
      </div>
    {:else}
      <div class="mb-6">
        <label for="mnemonic-input" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
          Recovery Phrase (12 words)
        </label>
        <textarea
          id="mnemonic-input"
          bind:value={mnemonicInput}
          rows="3"
          class="w-full px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-transparent bg-white dark:bg-gray-700 dark:text-white"
          placeholder="Enter your 12-word recovery phrase"
        ></textarea>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-2">
          Enter all 12 words separated by spaces
        </p>
      </div>
    {/if}

    {#if error}
      <div class="bg-red-50 dark:bg-red-900/30 border-l-4 border-red-400 dark:border-red-600 p-4 mb-6">
        <p class="text-sm text-red-800 dark:text-red-200">{error}</p>
      </div>
    {/if}

    <div class="flex gap-3">
      <button
        on:click={onBack}
        class="flex-1 px-6 py-3 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition dark:text-gray-300"
      >
        Cancel
      </button>
      <button
        on:click={handleLogin}
        class="flex-1 px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition"
      >
        Login
      </button>
    </div>
  </div>
</div>
