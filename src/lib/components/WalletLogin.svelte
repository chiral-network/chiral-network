<script lang="ts">
 import { createWalletFromPrivateKey, createWalletFromMnemonic, isValidPrivateKey, isValidMnemonic } from'$lib/walletService';
 import { walletAccount, isAuthenticated } from'$lib/stores';
 import { ArrowLeft, KeyRound, FileText } from'lucide-svelte';

 export let onBack: () => void;
 export let onComplete: () => void;

 let method:'privateKey' |'mnemonic' ='privateKey';
 let privateKeyInput ='';
 let mnemonicInput ='';
 let error ='';

 function handleLogin() {
 error ='';

 try {
 if (method ==='privateKey') {
 if (!privateKeyInput.trim()) {
 error ='Please enter your private key';
 return;
 }

 if (!isValidPrivateKey(privateKeyInput)) {
 error ='Invalid private key format';
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
 error ='Please enter your recovery phrase';
 return;
 }

 if (!isValidMnemonic(mnemonicInput.trim())) {
 error ='Invalid recovery phrase. Please check your words and try again.';
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
 error = err instanceof Error ? err.message :'Failed to login';
 }
 }
</script>

<div class="max-w-2xl mx-auto p-6">
 <div class="bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-xl shadow-gray-200/50 dark:shadow-black/5 p-8">
 <div class="flex items-center mb-6">
 <button on:click={onBack} class="mr-4 p-2 hover:bg-gray-100 dark:hover:bg-white/[0.05] rounded-lg transition text-gray-500 dark:text-white/50">
 <ArrowLeft class="w-5 h-5" />
 </button>
 <h2 class="text-2xl font-bold">Use Existing Wallet</h2>
 </div>

 <div class="flex gap-3 mb-6">
 <button
 on:click={() => method ='privateKey'}
 class="flex-1 flex items-center justify-center gap-2 px-4 py-3 rounded-lg transition {method ==='privateKey' ?' bg-violet-500/80 border border-primary-400/30 text-white' :' bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] hover:bg-gray-100 dark:hover:bg-white/[0.05] text-gray-500 dark:text-white/50'}"
 >
 <KeyRound class="w-5 h-5" />
 <span>Private Key</span>
 </button>

 <button
 on:click={() => method ='mnemonic'}
 class="flex-1 flex items-center justify-center gap-2 px-4 py-3 rounded-lg transition {method ==='mnemonic' ?' bg-violet-500/80 border border-primary-400/30 text-white' :' bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] hover:bg-gray-100 dark:hover:bg-white/[0.05] text-gray-500 dark:text-white/50'}"
 >
 <FileText class="w-5 h-5" />
 <span>Recovery Phrase</span>
 </button>
 </div>

 {#if method ==='privateKey'}
 <div class="mb-6">
 <label for="private-key-input" class="block text-sm font-medium text-gray-500 dark:text-white/50 mb-2">
 Private Key
 </label>
 <input
 id="private-key-input"
 type="password"
 bind:value={privateKeyInput}
 on:keydown={(e) => e.key ==='Enter' && handleLogin()}
 class="w-full px-4 py-2 bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-lg focus:border-gray-200/60 dark:border-white/[0.06] font-mono text-sm text-gray-900 dark:text-white/90 placeholder:text-gray-400 dark:text-white/40 outline-none"
 placeholder="Enter your private key (with or without 0x prefix)"
 />
 <p class="text-xs text-gray-400 dark:text-white/40 mt-2">
 Your private key should be a 64-character hexadecimal string
 </p>
 </div>
 {:else}
 <div class="mb-6">
 <label for="mnemonic-input" class="block text-sm font-medium text-gray-500 dark:text-white/50 mb-2">
 Recovery Phrase (12 words)
 </label>
 <textarea
 id="mnemonic-input"
 bind:value={mnemonicInput}
 rows="3"
 class="w-full px-4 py-2 bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-lg focus:border-gray-200/60 dark:border-white/[0.06] text-gray-900 dark:text-white/90 placeholder:text-gray-400 dark:text-white/40 outline-none"
 placeholder="Enter your 12-word recovery phrase"
 ></textarea>
 <p class="text-xs text-gray-400 dark:text-white/40 mt-2">
 Enter all 12 words separated by spaces
 </p>
 </div>
 {/if}

 {#if error}
 <div class="bg-red-500/[0.1]0/10 border border-red-400/20 rounded-lg p-4 mb-6">
 <p class="text-sm text-red-700 dark:text-red-300">{error}</p>
 </div>
 {/if}

 <div class="flex gap-3">
 <button
 on:click={onBack}
 class="flex-1 px-6 py-3 border border-gray-200/60 dark:border-white/[0.06] rounded-lg hover:bg-gray-100 dark:hover:bg-white/[0.05] transition text-gray-500 dark:text-white/50"
 >
 Cancel
 </button>
 <button
 on:click={handleLogin}
 class="flex-1 px-6 py-3 bg-violet-500/80 border border-primary-400/30 text-white rounded-lg hover:bg-violet-500/90 transition"
 >
 Login
 </button>
 </div>
 </div>
</div>
