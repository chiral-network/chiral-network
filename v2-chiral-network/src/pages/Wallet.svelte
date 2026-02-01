<script lang="ts">
  import { Wallet, KeyRound, Plus } from 'lucide-svelte';
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

{#if mode === 'select'}
  <div class="flex items-center justify-center min-h-screen bg-gradient-to-br from-blue-500 to-purple-600 p-6">
    <div class="max-w-md w-full">
      <div class="text-center mb-8">
        <div class="inline-flex items-center justify-center w-20 h-20 bg-white rounded-full mb-4">
          <Wallet class="w-10 h-10 text-blue-600" />
        </div>
        <h1 class="text-4xl font-bold text-white mb-2">Chiral Network</h1>
        <p class="text-white/80">Decentralized File Sharing Network</p>
      </div>
      
      <div class="bg-white rounded-lg shadow-xl p-8">
        <h2 class="text-2xl font-bold text-gray-800 mb-6">Get Started</h2>
        
        <button
          on:click={handleCreateWallet}
          class="w-full flex items-center justify-center gap-3 px-6 py-4 mb-4 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition shadow-md hover:shadow-lg"
        >
          <Plus class="w-5 h-5" />
          <span class="font-medium">Create New Wallet</span>
        </button>
        
        <button
          on:click={handleUseExisting}
          class="w-full flex items-center justify-center gap-3 px-6 py-4 border-2 border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 transition"
        >
          <KeyRound class="w-5 h-5" />
          <span class="font-medium">Use Existing Wallet</span>
        </button>
      </div>
    </div>
  </div>
{:else if mode === 'create'}
  <div class="min-h-screen bg-gradient-to-br from-blue-500 to-purple-600 py-12">
    <WalletCreation onBack={handleBack} onComplete={handleComplete} />
  </div>
{:else if mode === 'login'}
  <div class="min-h-screen bg-gradient-to-br from-blue-500 to-purple-600 py-12">
    <WalletLogin onBack={handleBack} onComplete={handleComplete} />
  </div>
{/if}
