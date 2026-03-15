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
 if (!bytes) return'—';
 if (bytes < 1024) return `${bytes} B`;
 if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
 if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
 return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
 }

 function formatDate(ts: number): string {
 return new Date(ts).toLocaleDateString(undefined, { month:'short', day:'numeric', year:'numeric' });
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
<tr
 class="group hover:bg-[var(--surface-2)] transition cursor-pointer select-none border-b border-[var(--border)]/60"
 ondblclick={() => onOpen(item)}
 oncontextmenu={(e) => { e.preventDefault(); onContextMenu(item, e); }}
>
 <td class="py-2.5 px-3">
 <div class="flex items-center gap-3">
 {#if item.type ==='folder'}
 <Folder class="w-5 h-5 {getFolderColor()} fill-current opacity-80 shrink-0" />
 {:else}
 {@const Icon = getFileIcon(item.name)}
 <svelte:component this={Icon} class="w-5 h-5 {getFileColor(item.name)} shrink-0" />
 {/if}
 <span class="text-sm font-medium text-white truncate">{item.name}</span>
 {#if item.starred}
 <Star class="w-3.5 h-3.5 text-yellow-500 fill-yellow-500 shrink-0" />
 {/if}
 {#if item.shared}
 {#if item.isPublic}
 <Link class="w-3.5 h-3.5 text-violet-400 shrink-0" />
 {:else}
 <EyeOff class="w-3.5 h-3.5 text-orange-500 shrink-0" />
 {/if}
 {/if}
 {#if item.seeding && $networkConnected}
 <span class="inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-medium rounded bg-emerald-500/10 text-emerald-400 shrink-0">
 <span class="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse"></span>
 Seeding{#if item.protocol} ({item.protocol}){/if}
 </span>
 {/if}
 {#if getPriceLabel(item)}
 {@const priceLabel = getPriceLabel(item)}
 <span class="inline-flex items-center px-1.5 py-0.5 text-[10px] font-medium rounded bg-amber-500/10 text-amber-400 shrink-0">
 {priceLabel}
 </span>
 {/if}
 </div>
 </td>
 <td class="py-2.5 px-3 text-sm text-[var(--text-secondary)]">
 {item.type ==='folder' ?'—' : formatSize(item.size)}
 </td>
 <td class="py-2.5 px-3 text-sm text-[var(--text-secondary)]">
 {formatDate(item.modifiedAt)}
 </td>
 <td class="py-2.5 px-3 text-right">
 <button
 onclick={(e) => { e.stopPropagation(); onContextMenu(item, e); }}
 class="p-1 hover:bg-[var(--surface-3)] rounded opacity-0 group-hover:opacity-100 transition-opacity"
 >
 <MoreVertical class="w-4 h-4 text-[var(--text-tertiary)]" />
 </button>
 </td>
</tr>
