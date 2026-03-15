<script lang="ts">
 import { Star } from 'lucide-svelte';
 import { ratingApi, setRatingOwner } from '$lib/services/ratingApiService';
 import { walletAccount } from '$lib/stores';
 import { get } from 'svelte/store';
 import { toasts } from '$lib/toastStore';

 interface Props {
 transferId: string;
 seederWallet: string;
 fileHash: string;
 fileName: string;
 onclose: () => void;
 }

 let { transferId, seederWallet, fileHash, fileName, onclose }: Props = $props();

 let selectedScore = $state(0);
 let hoveredScore = $state(0);
 let comment = $state('');
 let submitting = $state(false);

 function formatAddr(addr: string): string {
 if (addr.length <= 16) return addr;
 return `${addr.slice(0, 8)}...${addr.slice(-6)}`;
 }

 async function handleSubmit() {
 if (selectedScore === 0) {
 toasts.show('Please select a rating', 'warning');
 return;
 }

 const wallet = get(walletAccount);
 if (!wallet?.address) {
 toasts.show('Wallet not connected', 'error');
 return;
 }

 submitting = true;
 try {
 setRatingOwner(wallet.address);
 await ratingApi.submitRating(
 transferId,
 seederWallet,
 fileHash,
 selectedScore,
 comment.trim() || undefined,
 );
 toasts.show('Rating submitted!', 'success');
 onclose();
 } catch (err: any) {
 toasts.show(`Failed to submit rating: ${err.message}`, 'error');
 } finally {
 submitting = false;
 }
 }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
 class="fixed inset-0 bg-[var(--surface-0)]/70 flex items-center justify-center z-50"
 onkeydown={(e: KeyboardEvent) => { if (e.key === 'Escape') onclose(); }}
 onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) onclose(); }}
>
 <div class=" bg-[var(--surface-1)] border border-[var(--border)] rounded-xl shadow-black/10 p-6 max-w-md w-full mx-4">
 <!-- Header -->
 <div class="mb-4">
 <h3 class="font-semibold text-lg text-gray-900">Rate This Download</h3>
 <p class="text-sm text-[var(--text-tertiary)] mt-1">
 How was your experience downloading <span class="font-medium text-[var(--text-secondary)]">{fileName}</span>?
 </p>
 </div>

 <!-- Seeder info -->
 <div class=" bg-[var(--surface-1)] border border-[var(--border)] rounded-lg p-3 mb-4">
 <div class="flex justify-between items-center">
 <span class="text-xs text-[var(--text-tertiary)]">Seeder</span>
 <span class="text-sm font-mono text-gray-900">{formatAddr(seederWallet)}</span>
 </div>
 </div>

 <!-- Star rating -->
 <div class="flex flex-col items-center mb-4">
 <div class="flex gap-1">
 {#each [1, 2, 3, 4, 5] as star}
 <button
 type="button"
 onclick={() => { selectedScore = star; }}
 onmouseenter={() => { hoveredScore = star; }}
 onmouseleave={() => { hoveredScore = 0; }}
 class="p-1 transition-transform hover:scale-110"
 >
 <Star
 class="w-8 h-8 transition-colors {(hoveredScore || selectedScore) >= star ? 'text-yellow-400 fill-yellow-400' : 'text-[var(--text-secondary)]/40 dark:text-[var(--text-tertiary)]/40'}"
 />
 </button>
 {/each}
 </div>
 <span class="text-sm text-[var(--text-tertiary)] mt-1">
 {#if selectedScore === 1}Poor
 {:else if selectedScore === 2}Fair
 {:else if selectedScore === 3}Good
 {:else if selectedScore === 4}Very Good
 {:else if selectedScore === 5}Excellent
 {:else}Select a rating
 {/if}
 </span>
 </div>

 <!-- Comment -->
 <div class="mb-5">
 <label for="rating-comment" class="block text-sm font-medium text-[var(--text-secondary)] mb-1">
 Comment (optional)
 </label>
 <textarea
 id="rating-comment"
 bind:value={comment}
 maxlength={500}
 rows={3}
 placeholder="Share your experience..."
 class="w-full px-3 py-2 bg-[var(--surface-1)] border border-[var(--border)] rounded-lg text-sm text-gray-900 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-violet-500/40 focus:border-[var(--border)] resize-none"
 ></textarea>
 <p class="text-xs text-[var(--text-secondary)] mt-1 text-right">{comment.length}/500</p>
 </div>

 <!-- Actions -->
 <div class="flex gap-3">
 <button
 onclick={onclose}
 class="flex-1 px-4 py-2.5 border border-[var(--border)] rounded-lg text-sm font-medium text-[var(--text-secondary)] hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] transition-colors"
 >
 Skip
 </button>
 <button
 onclick={handleSubmit}
 disabled={selectedScore === 0 || submitting}
 class="flex-1 px-4 py-2.5 bg-violet-500/80 border border-primary-400/30 text-white rounded-lg text-sm font-medium hover:bg-violet-500/90 dark:hover:bg-violet-600/80 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
 >
 {submitting ? 'Submitting...' : 'Submit Rating'}
 </button>
 </div>
 </div>
</div>
