<script lang="ts">
 import { generateMnemonic, createWalletFromMnemonic } from '$lib/walletService';
 import { walletAccount, isAuthenticated } from '$lib/stores';
 import { walletBackupService } from '$lib/services/walletBackupService';
 import { toasts } from '$lib/toastStore';
 import { Copy, RefreshCw, Download, ArrowLeft, Check, Mail } from 'lucide-svelte';

 export let onBack: () => void;
 export let onComplete: () => void;

 type WalletCreationStep = 'generate' | 'verify' | 'email';
 type PendingWallet = { address: string; privateKey: string };

 let step: WalletCreationStep = 'generate';
 let mnemonic = '';
 let mnemonicWords: string[] = [];
 let copied = false;
 let verificationIndices: number[] = [];
 let userInputs: string[] = ['', ''];
 let verificationError = '';

 let pendingWallet: PendingWallet | null = null;
 let emailInput = '';
 let emailError = '';
 let sendingEmail = false;

 function generateNewMnemonic() {
 mnemonic = generateMnemonic();
 mnemonicWords = mnemonic.split(' ');
 verificationError = '';
 emailError = '';
 userInputs = ['', ''];
 pendingWallet = null;
 emailInput = '';
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
 const indices = new Set<number>();
 while (indices.size < 2) {
 indices.add(Math.floor(Math.random() * 12));
 }
 verificationIndices = Array.from(indices).sort((a, b) => a - b);
 verificationError = '';
 emailError = '';
 userInputs = ['', ''];
 step = 'verify';
 }

 function verifyAndContinue() {
 verificationError = '';

 for (let i = 0; i < verificationIndices.length; i++) {
 const expectedWord = mnemonicWords[verificationIndices[i]];
 const userInput = userInputs[i].trim().toLowerCase();

 if (userInput !== expectedWord.toLowerCase()) {
 verificationError = `Incorrect word at position ${verificationIndices[i] + 1}`;
 return;
 }
 }

 pendingWallet = createWalletFromMnemonic(mnemonic);
 emailError = '';
 step = 'email';
 }

 function isValidEmail(value: string): boolean {
 return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.trim());
 }

 async function sendBackupAndComplete() {
 emailError = '';

 const email = emailInput.trim();
 if (!isValidEmail(email)) {
 emailError = 'Please enter a valid email address';
 return;
 }
 if (!pendingWallet) {
 emailError = 'Wallet verification expired. Please verify your recovery phrase again.';
 step = 'verify';
 return;
 }

 sendingEmail = true;
 try {
 await walletBackupService.sendBackupEmail({
 email,
 recoveryPhrase: mnemonic,
 walletAddress: pendingWallet.address,
 privateKey: pendingWallet.privateKey,
 });

 walletAccount.set({
 address: pendingWallet.address,
 privateKey: pendingWallet.privateKey,
 });
 isAuthenticated.set(true);
 emailInput = '';
 toasts.show('Backup email sent. Wallet created successfully.', 'success');
 onComplete();
 } catch (error) {
 emailError = walletBackupService.formatError(error);
 } finally {
 sendingEmail = false;
 }
 }

 generateNewMnemonic();
</script>

<div class="max-w-2xl mx-auto p-6">
 {#if step === 'generate'}
 <div class=" bg-[var(--surface-1)] border border-[var(--border)] rounded-xl shadow-black/5 ring-1 ring-white/10 p-8">
 <div class="flex items-center mb-6">
 <button on:click={onBack} class="mr-4 p-2 hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] rounded-lg transition text-[var(--text-secondary)]">
 <ArrowLeft class="w-5 h-5" />
 </button>
 <h2 class="text-2xl font-bold">Create New Wallet</h2>
 </div>

 <div class="mb-6">
 <p class="text-[var(--text-secondary)] mb-4">
 Write down these 12 words in order and keep them safe. You'll need them to recover your wallet.
 </p>

 <div class=" bg-yellow-500/10 border border-yellow-400/20 rounded-lg p-4 mb-4">
 <p class="text-sm text-yellow-800">
 <strong>Warning:</strong> Never share your recovery phrase with anyone. Anyone with these words can access your wallet.
 </p>
 </div>

 <div class="grid grid-cols-3 gap-3 bg-[var(--surface-1)] border border-[var(--border)] rounded-lg p-6 mb-4">
 {#each mnemonicWords as word, index}
 <div class=" bg-[var(--surface-1)] p-3 rounded-lg border border-[var(--border)]">
 <span class="text-xs text-[var(--text-tertiary)]">{index + 1}.</span>
 <span class="ml-2 font-mono text-gray-900">{word}</span>
 </div>
 {/each}
 </div>

 <div class="flex gap-3">
 <button
 on:click={copyToClipboard}
 class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-[var(--surface-1)] border border-[var(--border)] hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] rounded-lg transition text-[var(--text-secondary)]"
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
 class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-[var(--surface-1)] border border-[var(--border)] hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] rounded-lg transition text-[var(--text-secondary)]"
 >
 <RefreshCw class="w-4 h-4" />
 <span>Regenerate</span>
 </button>

 <button
 on:click={downloadAsText}
 class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-[var(--surface-1)] border border-[var(--border)] hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] rounded-lg transition text-[var(--text-secondary)]"
 >
 <Download class="w-4 h-4" />
 <span>Download</span>
 </button>
 </div>
 </div>

 <div class="flex gap-3">
 <button
 on:click={onBack}
 class="flex-1 px-6 py-3 border border-[var(--border)] rounded-lg hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] transition text-[var(--text-secondary)]"
 >
 Cancel
 </button>
 <button
 on:click={proceedToVerification}
 class="flex-1 px-6 py-3 bg-violet-500/80 border border-primary-400/30 text-white rounded-lg hover:bg-violet-500/90 dark:hover:bg-violet-600/80 transition"
 >
 I've Saved It
 </button>
 </div>
 </div>
 {:else if step === 'verify'}
 <div class=" bg-[var(--surface-1)] border border-[var(--border)] rounded-xl shadow-black/5 ring-1 ring-white/10 p-8">
 <h2 class="text-2xl font-bold mb-6">Verify Recovery Phrase</h2>

 <p class="text-[var(--text-secondary)] mb-6">
 To ensure you've saved your recovery phrase, please enter the following words:
 </p>

 {#each verificationIndices as index, i}
 <div class="mb-4">
 <label for="word-{index}" class="block text-sm font-medium text-[var(--text-secondary)] mb-2">
 Word #{index + 1}
 </label>
 <input
 id="word-{index}"
 type="text"
 bind:value={userInputs[i]}
 class="w-full px-4 py-2 bg-[var(--surface-1)] border border-[var(--border)] rounded-lg focus:ring-2 focus:ring-violet-500/40 focus:border-[var(--border)] text-gray-900 placeholder-gray-400 outline-none"
 placeholder="Enter word {index + 1}"
 />
 </div>
 {/each}

 {#if verificationError}
 <div class=" bg-red-500/10 border border-red-400/20 rounded-lg p-4 mb-4">
 <p class="text-sm text-red-700">{verificationError}</p>
 </div>
 {/if}

 <div class="flex gap-3">
 <button
 on:click={() => step = 'generate'}
 class="flex-1 px-6 py-3 border border-[var(--border)] rounded-lg hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] transition text-[var(--text-secondary)]"
 >
 Back
 </button>
 <button
 on:click={verifyAndContinue}
 class="flex-1 px-6 py-3 bg-violet-500/80 border border-primary-400/30 text-white rounded-lg hover:bg-violet-500/90 dark:hover:bg-violet-600/80 transition"
 >
 Continue
 </button>
 </div>
 </div>
 {:else}
 <div class=" bg-[var(--surface-1)] border border-[var(--border)] rounded-xl shadow-black/5 ring-1 ring-white/10 p-8">
 <div class="flex items-center gap-2 mb-3">
 <Mail class="w-5 h-5 text-violet-500" />
 <h2 class="text-2xl font-bold">One-Time Email Backup</h2>
 </div>

 <p class="text-[var(--text-secondary)] mb-3">
 Enter your email to receive a one-time copy of your recovery phrase and wallet credentials.
 </p>

 <div class=" bg-violet-600/10 border border-violet-400/20 rounded-lg p-4 mb-6">
 <p class="text-sm text-blue-800">
 This email address is only used to send this backup now. Chiral does not store it.
 </p>
 </div>

 <div class="mb-4">
 <label for="backup-email" class="block text-sm font-medium text-[var(--text-secondary)] mb-2">
 Backup Email Address
 </label>
 <input
 id="backup-email"
 type="email"
 bind:value={emailInput}
 class="w-full px-4 py-2 bg-[var(--surface-1)] border border-[var(--border)] rounded-lg focus:ring-2 focus:ring-violet-500/40 focus:border-[var(--border)] text-gray-900 placeholder-gray-400 outline-none"
 placeholder="you@example.com"
 autocomplete="email"
 />
 </div>

 {#if emailError}
 <div class=" bg-red-500/10 border border-red-400/20 rounded-lg p-4 mb-4">
 <p class="text-sm text-red-700">{emailError}</p>
 </div>
 {/if}

 <div class="flex gap-3">
 <button
 on:click={() => step = 'verify'}
 class="flex-1 px-6 py-3 border border-[var(--border)] rounded-lg hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] transition text-[var(--text-secondary)]"
 disabled={sendingEmail}
 >
 Back
 </button>
 <button
 on:click={sendBackupAndComplete}
 class="flex-1 px-6 py-3 bg-violet-500/80 border border-primary-400/30 text-white rounded-lg hover:bg-violet-500/90 dark:hover:bg-violet-600/80 transition disabled:opacity-60 disabled:cursor-not-allowed"
 disabled={sendingEmail}
 >
 {#if sendingEmail}
 Sending...
 {:else}
 Send Email & Create Wallet
 {/if}
 </button>
 </div>
 </div>
 {/if}
</div>

