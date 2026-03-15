<script lang="ts">
 import { Copy, Check, X, Link, Globe, Trash2, Loader2, Eye, EyeOff } from'lucide-svelte';
 import { driveStore, type DriveItem, type DriveManifest } from'$lib/stores/driveStore';
 import type { ShareLink } from'$lib/services/driveApiService';
 import { toasts } from'$lib/toastStore';

 let {
 item,
 manifest,
 onClose,
 }: {
 item: DriveItem;
 manifest: DriveManifest;
 onClose: () => void;
 } = $props();

 let isPublic = $state(true);
 let creating = $state(false);
 let copied = $state<string | null>(null);

 const existingShares = $derived(driveStore.getSharesForItem(item.id, manifest));
 const currentItem = $derived(manifest.items.find(i => i.id === item.id));
 const isItemPublic = $derived(currentItem?.isPublic ?? true);
 const itemPrice = $derived(currentItem?.priceChi ?? item.priceChi);
 const hasPrice = $derived(itemPrice && parseFloat(itemPrice) > 0);

 let justCreatedUrl = $state<string | null>(null);
 let toggling = $state(false);

 async function toggleVisibility() {
 toggling = true;
 try {
 await driveStore.toggleVisibility(item.id);
 toasts.show(isItemPublic ?'File is now private' :'File is now public','success');
 } finally {
 toggling = false;
 }
 }

 async function createLink() {
 const price = hasPrice ? itemPrice! :'0';

 creating = true;
 try {
 const share = await driveStore.createShareLink(item.id, price, isPublic);
 if (share) {
 const url = driveStore.getShareUrl(share.id);
 justCreatedUrl = url;
 try {
 await navigator.clipboard.writeText(url);
 toasts.show('Share link created & copied!','success');
 } catch {
 toasts.show('Share link created. Copy it below.','success');
 }
 }
 } finally {
 creating = false;
 }
 }

 async function copyUrl(share: ShareLink) {
 const url = driveStore.getShareUrl(share.id);
 try {
 await navigator.clipboard.writeText(url);
 copied = share.id;
 setTimeout(() => copied = null, 2000);
 } catch {
 toasts.show('Failed to copy URL','error');
 }
 }

 async function revokeLink(share: ShareLink) {
 await driveStore.revokeShareLink(share.id);
 toasts.show('Share link revoked','success');
 }

 function formatDate(ts: number): string {
 return new Date(ts).toLocaleDateString(undefined, { month:'short', day:'numeric', year:'numeric' });
 }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-50 flex items-center justify-center bg-[var(--surface-0)]/40" onclick={onClose}>
 <div
 class="bg-[var(--surface-0)] rounded-xl w-full max-w-lg mx-4 p-6"
 onclick={(e) => e.stopPropagation()}
 >
 <div class="flex items-center justify-between mb-4">
 <h3 class="text-lg font-semibold text-white">Share"{item.name}"</h3>
 <button onclick={onClose} class="p-1 hover:bg-[var(--surface-1)] rounded-lg transition">
 <X class="w-5 h-5 text-[var(--text-tertiary)]" />
 </button>
 </div>

 {#if existingShares.length > 0}
 <div class="flex items-center justify-between p-3 bg-[var(--surface-1)] rounded-lg mb-4">
 <div class="flex items-center gap-2">
 {#if isItemPublic}
 <Eye class="w-4 h-4 text-green-500" />
 <span class="text-sm text-white/70">Public — share links are active</span>
 {:else}
 <EyeOff class="w-4 h-4 text-orange-500" />
 <span class="text-sm text-white/70">Private — share links are blocked</span>
 {/if}
 </div>
 <button
 onclick={toggleVisibility}
 disabled={toggling}
 class="px-3 py-1.5 text-sm font-medium rounded-lg transition disabled:opacity-50 {isItemPublic
 ?'bg-orange-100 text-orange-700 hover:bg-orange-200'
 :'bg-green-500/[0.08] text-green-400 hover:bg-green-200'}"
 >
 {isItemPublic ?'Make Private' :'Make Public'}
 </button>
 </div>
 {/if}

 <div class="space-y-3 mb-6">
 <p class="text-sm text-[var(--text-secondary)]">
 {#if hasPrice}
 Share at <strong class="text-emerald-400">{itemPrice} CHI</strong>. Recipients must pay before previewing or downloading.
 {:else}
 Share for free. Use"Edit Price" from the right-click menu to set a price.
 {/if}
 </p>

 <button
 onclick={createLink}
 disabled={creating}
 class="flex items-center gap-2 px-4 py-2 bg-indigo-600 hover:bg-indigo-700 disabled:opacity-50 text-white rounded-lg transition text-sm font-medium"
 >
 {#if creating}
 <Loader2 class="w-4 h-4 animate-spin" />
 Creating...
 {:else}
 <Link class="w-4 h-4" />
 Create Share Link
 {/if}
 </button>

 {#if !isItemPublic}
 <div class="p-3 bg-yellow-50 border border-yellow-800 rounded-lg">
 <p class="text-xs text-yellow-300">
 This file is currently private. Share links won't work until you make it public.
 </p>
 </div>
 {/if}

 {#if justCreatedUrl}
 <div class="flex items-center gap-2 p-3 bg-green-500/[0.08] border border-green-800 rounded-lg">
 <Check class="w-4 h-4 text-green-500 shrink-0" />
 <code class="flex-1 text-xs font-mono text-green-400 break-all select-all">{justCreatedUrl}</code>
 <button
 onclick={async () => {
 try {
 await navigator.clipboard.writeText(justCreatedUrl!);
 toasts.show('Copied!','success');
 } catch {
 toasts.show('Failed to copy','error');
 }
 }}
 class="p-1.5 hover:bg-green-900/30 rounded transition shrink-0"
 title="Copy link"
 >
 <Copy class="w-4 h-4 text-green-400" />
 </button>
 </div>
 {/if}
 </div>

 {#if existingShares.length > 0}
 <div class="border-t border-[var(--border)]/60 pt-4">
 <h4 class="text-sm font-medium text-white/70 mb-3">
 Active Links ({existingShares.length})
 </h4>
 <div class="space-y-2 max-h-48 overflow-y-auto">
 {#each existingShares as share (share.id)}
 <div class="flex items-center gap-2 p-2 bg-[var(--surface-1)] rounded-lg">
 <div class="flex-1 min-w-0">
 <code class="text-xs text-[var(--text-secondary)] font-mono truncate block">
 {driveStore.getShareUrl(share.id)}
 </code>
 <div class="flex flex-wrap items-center gap-2 mt-0.5">
 <span class="text-xs text-[var(--text-secondary)]">
 Created {formatDate(share.createdAt)}
 </span>
 {#if share.isPublic}
 <Globe class="w-3 h-3 text-green-500" />
 {/if}
 <span class="text-xs text-emerald-500">
 {share.priceChi} CHI
 </span>
 <span class="text-xs text-[var(--text-tertiary)] break-all">
 to {share.recipientWallet}
 </span>
 <span class="text-xs text-[var(--text-secondary)]">
 {share.downloadCount} download{share.downloadCount !== 1 ?'s' :''}
 </span>
 </div>
 </div>
 <button
 onclick={() => copyUrl(share)}
 class="p-1.5 hover:bg-[var(--surface-1)] rounded transition"
 title="Copy link"
 >
 {#if copied === share.id}
 <Check class="w-4 h-4 text-green-500" />
 {:else}
 <Copy class="w-4 h-4 text-[var(--text-tertiary)]" />
 {/if}
 </button>
 <button
 onclick={() => revokeLink(share)}
 class="p-1.5 hover:bg-red-900/30 rounded transition"
 title="Revoke link"
 >
 <Trash2 class="w-4 h-4 text-red-500" />
 </button>
 </div>
 {/each}
 </div>
 </div>
 {/if}

 <div class="mt-4 p-3 bg-indigo-50 rounded-lg">
 <p class="text-xs text-indigo-300">
 <strong>File:</strong> {item.name}
 {#if item.size}
 <span class="ml-2 text-indigo-500">({(item.size / (1024 * 1024)).toFixed(1)} MB)</span>
 {/if}
 </p>
 </div>
 </div>
</div>
