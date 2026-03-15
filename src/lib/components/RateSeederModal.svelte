<script lang="ts">
 import { Star } from'lucide-svelte';
 import { ratingApi, setRatingOwner } from'$lib/services/ratingApiService';
 import { walletAccount } from'$lib/stores';
 import { get } from'svelte/store';
 import { toasts } from'$lib/toastStore';

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
 toasts.show('Please select a rating','warning');
 return;
 }

 const wallet = get(walletAccount);
 if (!wallet?.address) {
 toasts.show('Wallet not connected','error');
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
 toasts.show('Rating submitted!','success');
 onclose();
 } catch (err: any) {
 toasts.show(`Failed to submit rating: ${err.message}`,'error');
 } finally {
 submitting = false;
 }
 }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
 class="fixed inset-0 bg-[#13111C]/40 flex items-center justify-center z-50"
 onkeydown={(e: KeyboardEvent) => { if (e.key ==='Escape') onclose(); }}
 onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) onclose(); }}
>
 <div class="bg-[var(--surface-1)] rounded-xl border border-[var(--border)] p-6 max-w-md w-full mx-4">
 <!-- Header -->
 <div class="mb-4">
 <h3 class="font-semibold text-lg text-white">Rate This Download</h3>
 <p class="text-sm text-white/50 mt-1">
 How was your experience downloading <span class="font-medium text-white/70">{fileName}</span>?
 </p>
 </div>

 <!-- Seeder info -->
 <div class="bg-[#13111C]/[0.05] rounded-lg p-3 mb-4">
 <div class="flex justify-between items-center">
 <span class="text-xs text-white/50">Seeder</span>
 <span class="text-sm font-mono text-white">{formatAddr(seederWallet)}</span>
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
 class="w-8 h-8 transition-colors {(hoveredScore || selectedScore) >= star ?'text-yellow-400 fill-yellow-400' :'text-white/70'}"
 />
 </button>
 {/each}
 </div>
 <span class="text-sm text-white/50 mt-1">
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
 <label for="rating-comment" class="block text-sm font-medium text-white/70 mb-1">
 Comment (optional)
 </label>
 <textarea
 id="rating-comment"
 bind:value={comment}
 maxlength={500}
 rows={3}
 placeholder="Share your experience..."
 class="w-full px-3 py-2 bg-[#13111C]/[0.07] border border-white/[0.06]/60 rounded-lg text-sm text-white placeholder:text-white/30 focus:outline-none focus:border-blue-400/40 resize-none"
 ></textarea>
 <p class="text-xs text-white/40 mt-1 text-right">{comment.length}/500</p>
 </div>

 <!-- Actions -->
 <div class="flex gap-3">
 <button
 onclick={onclose}
 class="flex-1 px-4 py-2.5 border border-white/[0.06]/60 rounded-lg text-sm font-medium text-white/70 hover:bg-[#13111C]/[0.05] transition-colors"
 >
 Skip
 </button>
 <button
 onclick={handleSubmit}
 disabled={selectedScore === 0 || submitting}
 class="flex-1 px-4 py-2.5 bg-indigo-600 text-white rounded-lg text-sm font-medium hover:bg-indigo-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
 >
 {submitting ?'Submitting...' :'Submit Rating'}
 </button>
 </div>
 </div>
</div>
