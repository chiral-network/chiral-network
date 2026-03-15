<script lang="ts">
 import { ShieldBan } from'lucide-svelte';

 interface Props {
 address: string;
 reason: string;
 action: string; // e.g."download this file" or"send 5.0 CHI"
 onconfirm: () => void;
 oncancel: () => void;
 }

 let { address, reason, action, onconfirm, oncancel }: Props = $props();

 function formatAddr(addr: string): string {
 if (addr.length <= 16) return addr;
 return `${addr.slice(0, 8)}...${addr.slice(-6)}`;
 }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
 class="fixed inset-0 bg-[var(--surface-0)]/40 flex items-center justify-center z-50"
 onkeydown={(e: KeyboardEvent) => { if (e.key ==='Escape') oncancel(); }}
 onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) oncancel(); }}
>
 <div class="bg-[var(--surface-1)] rounded-xl border border-[var(--border)] p-6 max-w-md w-full mx-4">
 <!-- Header -->
 <div class="flex items-center gap-3 mb-4">
 <div class="p-2.5 bg-red-500/10 rounded-full">
 <ShieldBan class="w-6 h-6 text-red-400" />
 </div>
 <div>
 <h3 class="font-semibold text-lg text-white">Blacklisted Address</h3>
 <p class="text-sm text-[var(--text-secondary)]">This address is on your blacklist</p>
 </div>
 </div>

 <!-- Details -->
 <div class="bg-[var(--surface-2)] rounded-lg p-4 mb-4 space-y-2">
 <div class="flex justify-between items-start gap-2">
 <span class="text-xs text-[var(--text-secondary)] shrink-0">Address</span>
 <span class="text-sm font-mono text-white text-right break-all">{formatAddr(address)}</span>
 </div>
 <div class="flex justify-between items-start gap-2">
 <span class="text-xs text-[var(--text-secondary)] shrink-0">Reason</span>
 <span class="text-sm text-white text-right">{reason}</span>
 </div>
 </div>

 <!-- Warning -->
 <div class="bg-amber-50 border border-amber-800 rounded-lg p-3 mb-5">
 <p class="text-sm text-amber-400">
 Are you sure you want to <strong>{action}</strong>?
 </p>
 </div>

 <!-- Actions -->
 <div class="flex gap-3">
 <button
 onclick={oncancel}
 class="flex-1 px-4 py-2.5 border border-[var(--border)]/60 rounded-lg text-sm font-medium text-[var(--text-secondary)] hover:bg-[var(--surface-2)] transition-colors"
 >
 Cancel
 </button>
 <button
 onclick={onconfirm}
 class="flex-1 px-4 py-2.5 bg-red-600 text-white rounded-lg text-sm font-medium hover:bg-red-700 transition-colors"
 >
 Proceed Anyway
 </button>
 </div>
 </div>
</div>
