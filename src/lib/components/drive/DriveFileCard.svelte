<script lang="ts">
 import { Star, MoreVertical, Link, Folder, EyeOff } from'lucide-svelte';
 import { getFileIcon, getFileColor, getFolderColor } from'$lib/utils/fileIcons';
 import type { DriveItem } from'$lib/stores/driveStore';
 import { networkConnected } from'$lib/stores';

 let {
 item,
 onOpen,
 onContextMenu,
 }: {
 item: DriveItem;
 onOpen: (item: DriveItem) => void;
 onContextMenu: (item: DriveItem, event: MouseEvent) => void;
 } = $props();

 function formatSize(bytes?: number): string {
 if (!bytes) return'';
 if (bytes < 1024) return `${bytes} B`;
 if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
 if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
 return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
 }

 function getPriceLabel(item: DriveItem): string | null {
 if (item.type !=='file') return null;
 const raw = item.priceChi?.trim();
 if (raw && raw !=='0') return `${raw} CHI`;
 if (item.seeding || raw ==='0') return'Free';
 return null;
 }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
 class="group relative bg-[var(--surface-0)] border border-[var(--border)]/60 rounded-xl p-4 hover:shadow-sm hover:border-indigo-300 transition cursor-pointer select-none"
 ondblclick={() => onOpen(item)}
 oncontextmenu={(e) => { e.preventDefault(); onContextMenu(item, e); }}
>
 <!-- Action buttons -->
 <div class="absolute top-2 right-2 flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
 {#if item.starred}
 <Star class="w-3.5 h-3.5 text-yellow-500 fill-yellow-500" />
 {/if}
 {#if item.shared}
 {#if item.isPublic}
 <Link class="w-3.5 h-3.5 text-indigo-500" />
 {:else}
 <EyeOff class="w-3.5 h-3.5 text-orange-500" />
 {/if}
 {/if}
 <button
 onclick={(e) => { e.stopPropagation(); onContextMenu(item, e); }}
 class="p-1 hover:bg-[var(--surface-0)] rounded"
 >
 <MoreVertical class="w-4 h-4 text-white/[0.08]" />
 </button>
 </div>

 <!-- Icon -->
 <div class="flex items-center justify-center w-12 h-12 mx-auto mb-3 rounded-lg {item.type ==='folder' ?'bg-yellow-50' :'bg-[var(--surface-0)]'}">
 {#if item.type ==='folder'}
 <Folder class="w-7 h-7 {getFolderColor()} fill-current opacity-80" />
 {:else}
 {@const Icon = getFileIcon(item.name)}
 <svelte:component this={Icon} class="w-6 h-6 {getFileColor(item.name)}" />
 {/if}
 </div>

 <!-- Name & metadata -->
 <div class="text-center">
 <p class="text-sm font-medium text-white truncate" title={item.name}>
 {item.name}
 </p>
 {#if item.type ==='file' && item.size}
 <p class="text-xs text-white/[0.06] mt-0.5">{formatSize(item.size)}</p>
 {/if}
 {#if getPriceLabel(item)}
 {@const priceLabel = getPriceLabel(item)}
 <p class="text-[11px] text-amber-300 font-medium mt-0.5">{priceLabel}</p>
 {/if}
 {#if item.seeding && $networkConnected}
 <div class="flex items-center justify-center gap-1 mt-1">
 <span class="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse"></span>
 <span class="text-[10px] text-green-400 font-medium">Seeding</span>
 {#if item.protocol}
 <span class="text-[10px] text-white/[0.06]">({item.protocol})</span>
 {/if}
 </div>
 {/if}
 </div>
</div>
