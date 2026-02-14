<script lang="ts">
  import { KeyRound, Plus, Globe, Shield, Coins } from 'lucide-svelte';
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
  <div class="relative flex items-center justify-center min-h-screen bg-gradient-to-b from-gray-950 via-gray-900 to-gray-950 p-6 overflow-hidden">
    <!-- Animated background orbs -->
    <div class="absolute inset-0 overflow-hidden pointer-events-none">
      <div class="orb orb-1"></div>
      <div class="orb orb-2"></div>
      <div class="orb orb-3"></div>
    </div>

    <div class="relative z-10 max-w-md w-full">
      <!-- Logo and title -->
      <div class="text-center mb-10">
        <div class="inline-flex items-center justify-center w-20 h-20 mb-6">
          <img src="/logo.png" alt="Chiral Network" class="w-20 h-20 drop-shadow-2xl" />
        </div>
        <h1 class="text-4xl font-bold bg-gradient-to-r from-blue-400 to-purple-400 bg-clip-text text-transparent mb-3">
          Chiral Network
        </h1>
        <p class="text-lg text-gray-400">Decentralized File Sharing Network</p>
      </div>

      <!-- Glass card -->
      <div class="bg-white/[0.07] backdrop-blur-xl border border-white/[0.12] rounded-2xl p-8 shadow-2xl shadow-black/20">
        <h2 class="text-2xl font-semibold text-white mb-6">Get Started</h2>

        <button
          on:click={handleCreateWallet}
          class="w-full flex items-start gap-4 px-5 py-4 mb-4 bg-gradient-to-r from-blue-500 to-purple-500 hover:from-blue-400 hover:to-purple-400 text-white rounded-xl transition-all shadow-lg shadow-blue-500/20 hover:shadow-blue-500/30"
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
          on:click={handleUseExisting}
          class="w-full flex items-start gap-4 px-5 py-4 border border-white/20 bg-white/5 hover:bg-white/10 text-white rounded-xl transition-all"
        >
          <div class="flex-shrink-0 mt-0.5 p-2 bg-white/10 rounded-lg">
            <KeyRound class="w-5 h-5" />
          </div>
          <div class="text-left">
            <span class="block font-semibold text-base">Import Existing Wallet</span>
            <span class="block text-sm text-gray-400 mt-0.5">Use your private key or recovery phrase</span>
          </div>
        </button>
      </div>

      <!-- Feature pills -->
      <div class="flex items-center justify-center gap-3 mt-8">
        <div class="flex items-center gap-1.5 px-3 py-1.5 bg-white/[0.06] border border-white/10 rounded-full text-xs text-gray-400">
          <Globe class="w-3.5 h-3.5" />
          <span>Decentralized</span>
        </div>
        <div class="flex items-center gap-1.5 px-3 py-1.5 bg-white/[0.06] border border-white/10 rounded-full text-xs text-gray-400">
          <Shield class="w-3.5 h-3.5" />
          <span>Encrypted</span>
        </div>
        <div class="flex items-center gap-1.5 px-3 py-1.5 bg-white/[0.06] border border-white/10 rounded-full text-xs text-gray-400">
          <Coins class="w-3.5 h-3.5" />
          <span>Token Economy</span>
        </div>
      </div>
    </div>
  </div>
{:else if mode === 'create'}
  <div class="min-h-screen bg-gradient-to-b from-gray-950 via-gray-900 to-gray-950 py-12">
    <WalletCreation onBack={handleBack} onComplete={handleComplete} />
  </div>
{:else if mode === 'login'}
  <div class="min-h-screen bg-gradient-to-b from-gray-950 via-gray-900 to-gray-950 py-12">
    <WalletLogin onBack={handleBack} onComplete={handleComplete} />
  </div>
{/if}

<style>
  .orb {
    position: absolute;
    border-radius: 50%;
    filter: blur(80px);
    opacity: 0.15;
  }

  .orb-1 {
    width: 400px;
    height: 400px;
    background: radial-gradient(circle, #3b82f6, transparent 70%);
    top: -10%;
    left: -10%;
    animation: float1 20s ease-in-out infinite;
  }

  .orb-2 {
    width: 350px;
    height: 350px;
    background: radial-gradient(circle, #8b5cf6, transparent 70%);
    bottom: -5%;
    right: -10%;
    animation: float2 25s ease-in-out infinite;
  }

  .orb-3 {
    width: 250px;
    height: 250px;
    background: radial-gradient(circle, #6366f1, transparent 70%);
    top: 40%;
    right: 20%;
    animation: float3 18s ease-in-out infinite;
  }

  @keyframes float1 {
    0%, 100% { transform: translate(0, 0); }
    33% { transform: translate(30px, 50px); }
    66% { transform: translate(-20px, 30px); }
  }

  @keyframes float2 {
    0%, 100% { transform: translate(0, 0); }
    33% { transform: translate(-40px, -30px); }
    66% { transform: translate(20px, -50px); }
  }

  @keyframes float3 {
    0%, 100% { transform: translate(0, 0); }
    33% { transform: translate(50px, -20px); }
    66% { transform: translate(-30px, 40px); }
  }
</style>
