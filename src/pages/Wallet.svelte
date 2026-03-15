<script lang="ts">
  import { KeyRound, Plus } from 'lucide-svelte';
  import WalletCreation from '$lib/components/WalletCreation.svelte';
  import WalletLogin from '$lib/components/WalletLogin.svelte';

  let mode: 'select' | 'create' | 'login' = 'select';

  function handleCreateWallet() {
    mode = 'create';
  }

  function handleUseExisting() {
    mode = 'login';
  }

  function handleBack() {
    mode = 'select';
  }

  function handleComplete() {
    // Authentication state change will trigger navigation in App.svelte
  }
</script>

<svelte:head><title>Chiral Network</title></svelte:head>

{#if mode === 'select'}
  <div class="flex items-center justify-center min-h-screen bg-white dark:bg-gray-950 p-6">
    <div class="max-w-md w-full">
      <!-- Logo and title -->
      <div class="text-center mb-10">
        <div class="inline-flex items-center justify-center w-20 h-20 mb-6">
          <img src="/logo.png" alt="Chiral Network" class="w-20 h-20 drop-shadow-sm" />
        </div>
        <h1 class="text-4xl font-light text-gray-900 dark:text-white mb-3">
          Chiral Network
        </h1>
        <p class="text-lg text-gray-500 dark:text-gray-400">Decentralized File Sharing Network</p>
      </div>

      <!-- Card -->
      <div class="bg-white dark:bg-gray-950 border border-gray-200/60 dark:border-gray-800 rounded-2xl p-8 shadow-sm">
        <h2 class="text-2xl font-semibold text-gray-900 dark:text-white mb-6">Get Started</h2>

        <button
          onclick={handleCreateWallet}
          class="w-full flex items-start gap-4 px-5 py-4 mb-4 bg-primary-600 hover:bg-primary-700 text-white rounded-xl transition-all shadow-sm focus:outline-none focus:ring-2 focus:ring-primary-500/30"
        >
          <div class="flex-shrink-0 mt-0.5 p-2 bg-white/20 rounded-lg">
            <Plus class="w-5 h-5" />
          </div>
          <div class="text-left">
            <span class="block font-semibold text-base">Create New Wallet</span>
            <span class="block text-sm text-white/70 mt-0.5">Generate a new wallet with a secure recovery phrase</span>
          </div>
        </button>

        <button
          onclick={handleUseExisting}
          class="w-full flex items-start gap-4 px-5 py-4 border border-gray-200/60 dark:border-gray-800 bg-gray-50 dark:bg-gray-900/50 hover:bg-gray-100 dark:hover:bg-gray-900 text-gray-900 dark:text-white rounded-xl transition-all focus:outline-none focus:ring-2 focus:ring-gray-400/30"
        >
          <div class="flex-shrink-0 mt-0.5 p-2 bg-gray-200 dark:bg-gray-600 rounded-lg">
            <KeyRound class="w-5 h-5" />
          </div>
          <div class="text-left">
            <span class="block font-semibold text-base">Import Existing Wallet</span>
            <span class="block text-sm text-gray-500 dark:text-gray-400 mt-0.5">Use your private key or recovery phrase</span>
          </div>
        </button>
      </div>
    </div>
  </div>
{:else if mode === 'create'}
  <div class="min-h-screen bg-white dark:bg-gray-950 py-12">
    <WalletCreation onBack={handleBack} onComplete={handleComplete} />
  </div>
{:else if mode === 'login'}
  <div class="min-h-screen bg-white dark:bg-gray-950 py-12">
    <WalletLogin onBack={handleBack} onComplete={handleComplete} />
  </div>
{/if}
