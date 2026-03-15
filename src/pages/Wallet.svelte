<script lang="ts">
  import { KeyRound, Plus } from 'lucide-svelte';
  import WalletCreation from '$lib/components/WalletCreation.svelte';
  import WalletLogin from '$lib/components/WalletLogin.svelte';
  let mode: 'select' | 'create' | 'login' = 'select';
  function handleCreateWallet() { mode = 'create'; }
  function handleUseExisting() { mode = 'login'; }
  function handleBack() { mode = 'select'; }
  function handleComplete() {}
</script>
<svelte:head><title>Chiral Network</title></svelte:head>
{#if mode === 'select'}
  <div class="flex items-center justify-center min-h-screen bg-[var(--surface-0)] p-6">
    <div class="max-w-md w-full">
      <div class="text-center mb-10">
        <div class="inline-flex items-center justify-center w-20 h-20 mb-6">
          <img src="/logo.png" alt="Chiral Network" class="w-20 h-20" />
        </div>
        <h1 class="text-4xl font-bold text-[var(--text-primary)] mb-3">Chiral Network</h1>
        <p class="text-lg text-[var(--text-secondary)]">Decentralized File Sharing Network</p>
      </div>
      <div class="bg-[var(--surface-1)] rounded-xl border border-[var(--border)] p-8">
        <h2 class="text-2xl font-semibold text-[var(--text-primary)] mb-6">Get Started</h2>
        <button onclick={handleCreateWallet} class="w-full flex items-start gap-4 px-5 py-4 mb-4 bg-violet-600 hover:bg-violet-500 text-white rounded-xl transition-colors duration-150 font-medium focus:outline-none">
          <div class="flex-shrink-0 mt-0.5 p-2 bg-violet-700/30 rounded-lg"><Plus class="w-5 h-5" /></div>
          <div class="text-left">
            <span class="block font-semibold text-base">Create New Wallet</span>
            <span class="block text-sm text-white/70 mt-0.5">Generate a new wallet with a secure recovery phrase</span>
          </div>
        </button>
        <button onclick={handleUseExisting} class="w-full flex items-start gap-4 px-5 py-4 border border-[var(--border)] bg-[var(--surface-2)] hover:bg-[var(--surface-3)] text-[var(--text-primary)] rounded-xl transition-colors duration-150 focus:outline-none">
          <div class="flex-shrink-0 mt-0.5 p-2 bg-[var(--surface-3)] rounded-lg"><KeyRound class="w-5 h-5 text-violet-400" /></div>
          <div class="text-left">
            <span class="block font-semibold text-base">Import Existing Wallet</span>
            <span class="block text-sm text-[var(--text-secondary)] mt-0.5">Use your private key or recovery phrase</span>
          </div>
        </button>
      </div>
    </div>
  </div>
{:else if mode === 'create'}
  <div class="min-h-screen bg-[var(--surface-0)] py-12"><WalletCreation onBack={handleBack} onComplete={handleComplete} /></div>
{:else if mode === 'login'}
  <div class="min-h-screen bg-[var(--surface-0)] py-12"><WalletLogin onBack={handleBack} onComplete={handleComplete} /></div>
{/if}
