<script lang="ts">
  import { generateMnemonic, createWalletFromMnemonic } from '$lib/walletService';
  import { walletAccount, isAuthenticated } from '$lib/stores';
  import { Copy, RefreshCw, Download, ArrowLeft, Check } from 'lucide-svelte';
  
  export let onBack: () => void;
  export let onComplete: () => void;
  
  let step: 'generate' | 'verify' = 'generate';
  let mnemonic = '';
  let mnemonicWords: string[] = [];
  let copied = false;
  let verificationIndices: number[] = [];
  let userInputs: string[] = ['', ''];
  let verificationError = '';
  
  function generateNewMnemonic() {
    mnemonic = generateMnemonic();
    mnemonicWords = mnemonic.split(' ');
  }
  
  function copyToClipboard() {
    navigator.clipboard.writeText(mnemonic);
    copied = true;
    setTimeout(() => copied = false, 2000);
  }
  
  function downloadAsText() {
    const blob = new Blob([mnemonic], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'chiral-wallet-recovery-phrase.txt';
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }
  
  function proceedToVerification() {
    // Select 2 random words to verify
    const indices = new Set<number>();
    while (indices.size < 2) {
      indices.add(Math.floor(Math.random() * 12));
    }
    verificationIndices = Array.from(indices).sort((a, b) => a - b);
    step = 'verify';
  }
  
  function verifyAndCreateWallet() {
    verificationError = '';
    
    // Check if user entered the correct words
    for (let i = 0; i < verificationIndices.length; i++) {
      const expectedWord = mnemonicWords[verificationIndices[i]];
      const userInput = userInputs[i].trim().toLowerCase();
      
      if (userInput !== expectedWord.toLowerCase()) {
        verificationError = `Incorrect word at position ${verificationIndices[i] + 1}`;
        return;
      }
    }
    
    // Create wallet from mnemonic
    const wallet = createWalletFromMnemonic(mnemonic);
    walletAccount.set({
      address: wallet.address,
      privateKey: wallet.privateKey
    });
    isAuthenticated.set(true);
    onComplete();
  }
  
  // Initialize with a mnemonic
  generateNewMnemonic();
</script>

<div class="max-w-2xl mx-auto p-6">
  {#if step === 'generate'}
    <div class="bg-white rounded-lg shadow-lg p-8">
      <div class="flex items-center mb-6">
        <button on:click={onBack} class="mr-4 p-2 hover:bg-gray-100 rounded-lg transition">
          <ArrowLeft class="w-5 h-5" />
        </button>
        <h2 class="text-2xl font-bold">Create New Wallet</h2>
      </div>
      
      <div class="mb-6">
        <p class="text-gray-600 mb-4">
          Write down these 12 words in order and keep them safe. You'll need them to recover your wallet.
        </p>
        
        <div class="bg-yellow-50 border-l-4 border-yellow-400 p-4 mb-4">
          <p class="text-sm text-yellow-800">
            <strong>Warning:</strong> Never share your recovery phrase with anyone. Anyone with these words can access your wallet.
          </p>
        </div>
        
        <div class="grid grid-cols-3 gap-3 bg-gray-50 p-6 rounded-lg mb-4">
          {#each mnemonicWords as word, index}
            <div class="bg-white p-3 rounded border border-gray-200">
              <span class="text-xs text-gray-500">{index + 1}.</span>
              <span class="ml-2 font-mono">{word}</span>
            </div>
          {/each}
        </div>
        
        <div class="flex gap-3">
          <button
            on:click={copyToClipboard}
            class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-gray-100 hover:bg-gray-200 rounded-lg transition"
          >
            {#if copied}
              <Check class="w-4 h-4 text-green-600" />
              <span class="text-green-600">Copied!</span>
            {:else}
              <Copy class="w-4 h-4" />
              <span>Copy</span>
            {/if}
          </button>
          
          <button
            on:click={generateNewMnemonic}
            class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-gray-100 hover:bg-gray-200 rounded-lg transition"
          >
            <RefreshCw class="w-4 h-4" />
            <span>Regenerate</span>
          </button>
          
          <button
            on:click={downloadAsText}
            class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-gray-100 hover:bg-gray-200 rounded-lg transition"
          >
            <Download class="w-4 h-4" />
            <span>Download</span>
          </button>
        </div>
      </div>
      
      <div class="flex gap-3">
        <button
          on:click={onBack}
          class="flex-1 px-6 py-3 border border-gray-300 rounded-lg hover:bg-gray-50 transition"
        >
          Cancel
        </button>
        <button
          on:click={proceedToVerification}
          class="flex-1 px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition"
        >
          I've Saved It
        </button>
      </div>
    </div>
  {:else}
    <div class="bg-white rounded-lg shadow-lg p-8">
      <h2 class="text-2xl font-bold mb-6">Verify Recovery Phrase</h2>
      
      <p class="text-gray-600 mb-6">
        To ensure you've saved your recovery phrase, please enter the following words:
      </p>
      
      {#each verificationIndices as index, i}
        <div class="mb-4">
          <label for="word-{index}" class="block text-sm font-medium text-gray-700 mb-2">
            Word #{index + 1}
          </label>
          <input
            id="word-{index}"
            type="text"
            bind:value={userInputs[i]}
            class="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            placeholder="Enter word {index + 1}"
          />
        </div>
      {/each}
      
      {#if verificationError}
        <div class="bg-red-50 border-l-4 border-red-400 p-4 mb-4">
          <p class="text-sm text-red-800">{verificationError}</p>
        </div>
      {/if}
      
      <div class="flex gap-3">
        <button
          on:click={() => step = 'generate'}
          class="flex-1 px-6 py-3 border border-gray-300 rounded-lg hover:bg-gray-50 transition"
        >
          Back
        </button>
        <button
          on:click={verifyAndCreateWallet}
          class="flex-1 px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition"
        >
          Create Wallet
        </button>
      </div>
    </div>
  {/if}
</div>
