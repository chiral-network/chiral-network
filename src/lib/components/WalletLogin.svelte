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
 <div class="bg-[var(--surface-0)] rounded-lg p-8">
 <div class="flex items-center mb-6">
 <button on:click={onBack} class="mr-4 p-2 hover:bg-[var(--surface-2)] rounded-lg transition">
 <ArrowLeft class="w-5 h-5" />
 </button>
 <h2 class="text-2xl font-bold">Use Existing Wallet</h2>
 </div>

 <div class="flex gap-3 mb-6">
 <button
 on:click={() => method ='privateKey'}
 class="flex-1 flex items-center justify-center gap-2 px-4 py-3 rounded-lg transition {method ==='privateKey' ?'bg-violet-500 text-white' :'bg-[var(--surface-0)] hover:bg-[var(--surface-2)]'}"
 >
 <KeyRound class="w-5 h-5" />
 <span>Private Key</span>
 </button>

 <button
 on:click={() => method ='mnemonic'}
 class="flex-1 flex items-center justify-center gap-2 px-4 py-3 rounded-lg transition {method ==='mnemonic' ?'bg-violet-500 text-white' :'bg-[var(--surface-0)] hover:bg-[var(--surface-2)]'}"
 >
 <FileText class="w-5 h-5" />
 <span>Recovery Phrase</span>
 </button>
 </div>

 {#if method ==='privateKey'}
 <div class="mb-6">
 <label for="private-key-input" class="block text-sm font-medium text-[var(--text-secondary)] mb-2">
 Private Key
 </label>
 <input
 id="private-key-input"
 type="password"
 bind:value={privateKeyInput}
 on:keydown={(e) => e.key ==='Enter' && handleLogin()}
 class="w-full px-4 py-2 border border-[var(--border)]/60 rounded-lg focus:border-violet-500/50 focus:border-transparent font-mono text-sm bg-[var(--surface-0)]"
 placeholder="Enter your private key (with or without 0x prefix)"
 />
 <p class="text-xs text-[var(--text-secondary)] mt-2">
 Your private key should be a 64-character hexadecimal string
 </p>
 </div>
 {:else}
 <div class="mb-6">
 <label for="mnemonic-input" class="block text-sm font-medium text-[var(--text-secondary)] mb-2">
 Recovery Phrase (12 words)
 </label>
 <textarea
 id="mnemonic-input"
 bind:value={mnemonicInput}
 rows="3"
 class="w-full px-4 py-2 border border-[var(--border)]/60 rounded-lg focus:border-violet-500/50 focus:border-transparent bg-[var(--surface-0)]"
 placeholder="Enter your 12-word recovery phrase"
 ></textarea>
 <p class="text-xs text-[var(--text-secondary)] mt-2">
 Enter all 12 words separated by spaces
 </p>
 </div>
 {/if}

 {#if error}
 <div class="bg-red-500/10 border-l-2 border-red-400 p-4 mb-6">
 <p class="text-sm text-red-400">{error}</p>
 </div>
 {/if}

 <div class="flex gap-3">
 <button
 on:click={onBack}
 class="flex-1 px-6 py-3 border border-[var(--border)]/60 rounded-lg hover:bg-[var(--surface-2)] transition"
 >
 Cancel
 </button>
 <button
 on:click={handleLogin}
 class="flex-1 px-6 py-3 bg-violet-600 text-white rounded-lg hover:bg-violet-500 transition"
 >
 Login
 </button>
 </div>
 </div>
</div>
