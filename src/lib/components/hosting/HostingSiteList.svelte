<script lang="ts">
 import { Server, Copy, ExternalLink, Trash2, Upload, CloudOff, Check, Globe } from'lucide-svelte';
 import {
 formatHostedFileSize as formatFileSize,
 formatHostedTimeAgo as timeAgo,
 buildHostedSiteUrl,
 getTotalHostedSiteSize as totalSize,
 } from'$lib/utils/hostingPageUtils';

 interface SiteFile {
 path: string;
 size: number;
 }

 interface HostedSite {
 id: string;
 name: string;
 directory: string;
 createdAt: number;
 files: SiteFile[];
 relayUrl?: string | null;
 }

 interface Props {
 sites: HostedSite[];
 serverAddress: string | null;
 port: number;
 publishingStates: Record<string, boolean>;
 onPublish: (siteId: string) => void;
 onUnpublish: (siteId: string) => void;
 onCopyUrl: (site: HostedSite) => void;
 onOpenSite: (site: HostedSite) => void;
 onDeleteSite: (siteId: string, siteName: string) => void;
 }

 let {
 sites,
 serverAddress,
 port,
 publishingStates,
 onPublish,
 onUnpublish,
 onCopyUrl,
 onOpenSite,
 onDeleteSite,
 }: Props = $props();

 function siteUrl(site: HostedSite): string {
 return buildHostedSiteUrl(site.id, site.relayUrl, serverAddress, port);
 }
</script>

<div class="rounded-xl bg-white/[0.05] p-5 shadow-black/5 border border-white/[0.06]">
 <h2 class="mb-4 text-base font-semibold text-white/90 flex items-center gap-2">
 <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-white/[0.05]">
 <Globe class="h-3.5 w-3.5 text-white/40" />
 </div>
 Hosted Sites
 {#if sites.length > 0}
 <span class="rounded-full bg-violet-900/20 px-2 py-0.5 text-xs font-medium text-primary-700">
 {sites.length}
 </span>
 {/if}
 </h2>

 {#if sites.length === 0}
 <div class="flex flex-col items-center justify-center py-16 text-white/50">
 <div class="flex h-16 w-16 items-center justify-center rounded-xl bg-white/[0.05] mb-4">
 <Server class="h-8 w-8 opacity-40" />
 </div>
 <p class="text-sm font-medium text-white/40">No hosted sites yet</p>
 <p class="text-xs mt-1 text-white/50">Create a site above to start hosting</p>
 </div>
 {:else}
 <div class="space-y-3">
 {#each sites as site (site.id)}
 <div class="group rounded-xl border border-white/[0.06] bg-white/[0.05] p-4 transition-all
 hover:border-white/[0.06] hover:shadow-sm">
 <div class="flex items-start justify-between gap-3">
 <div class="min-w-0 flex-1">
 <div class="flex items-center gap-2">
 <h3 class="font-semibold text-white/90">{site.name}</h3>
 {#if site.relayUrl}
 <span class="inline-flex items-center gap-1 rounded-full bg-green-500/[0.15] px-2 py-0.5 text-[10px] font-semibold text-green-400 uppercase tracking-wide">
 <Check class="h-2.5 w-2.5" />
 Published
 </span>
 {:else}
 <span class="inline-flex items-center rounded-full bg-white/[0.05] border border-white/[0.06] px-2 py-0.5 text-[10px] font-medium text-white/40 uppercase tracking-wide">
 Local
 </span>
 {/if}
 </div>

 <!-- URL -->
 <p class="mt-1 font-mono text-xs truncate
 {site.relayUrl ?'text-green-400' :'text-violet-500'}">
 {site.relayUrl || siteUrl(site)}
 </p>

 <!-- Meta -->
 <div class="mt-2 flex items-center gap-3 text-xs text-white/50">
 <span>{site.files.length} file{site.files.length === 1 ?'' :'s'}</span>
 <span aria-hidden="true" class="text-white/50">|</span>
 <span class="tabular-nums">{formatFileSize(totalSize(site.files))}</span>
 <span aria-hidden="true" class="text-white/50">|</span>
 <span>Created {timeAgo(site.createdAt)}</span>
 </div>

 <!-- File tags -->
 {#if site.files.length > 0}
 <div class="mt-2.5 flex flex-wrap gap-1.5">
 {#each site.files.slice(0, 5) as file}
 <span class="rounded-md bg-white/[0.05] border border-white/[0.06] px-1.5 py-0.5 text-[10px] font-medium text-white/40">
 {file.path}
 </span>
 {/each}
 {#if site.files.length > 5}
 <span class="rounded-md bg-white/[0.05] px-1.5 py-0.5 text-[10px] font-medium text-white/50">
 +{site.files.length - 5} more
 </span>
 {/if}
 </div>
 {/if}
 </div>

 <!-- Actions -->
 <div class="flex items-center gap-1 flex-shrink-0 opacity-70 group-hover:opacity-100 transition-opacity">
 {#if site.relayUrl}
 <button
 onclick={() => onUnpublish(site.id)}
 disabled={publishingStates[site.id]}
 title="Unpublish from network"
 aria-label="Unpublish {site.name} from network"
 class="rounded-lg p-2 text-white/50 transition-colors hover:bg-orange-50 hover:text-orange-500
 focus:outline-none disabled:opacity-50"
 >
 {#if publishingStates[site.id]}
 <div class="h-4 w-4 animate-spin rounded-full border-2 border-white/[0.2] border-t-orange-500"></div>
 {:else}
 <CloudOff class="h-4 w-4" />
 {/if}
 </button>
 {:else}
 <button
 onclick={() => onPublish(site.id)}
 disabled={publishingStates[site.id]}
 title="Publish to network"
 aria-label="Publish {site.name} to network"
 class="rounded-lg p-2 text-white/50 transition-colors hover:bg-green-50 hover:text-green-500
 focus:outline-none disabled:opacity-50"
 >
 {#if publishingStates[site.id]}
 <div class="h-4 w-4 animate-spin rounded-full border-2 border-white/[0.2] border-t-green-500"></div>
 {:else}
 <Upload class="h-4 w-4" />
 {/if}
 </button>
 {/if}

 <button
 onclick={() => onCopyUrl(site)}
 title="Copy URL"
 aria-label="Copy URL for {site.name}"
 class="rounded-lg p-2 text-white/50 transition-colors hover:bg-white/[0.05] hover:text-white/50 focus:outline-none"
 >
 <Copy class="h-4 w-4" />
 </button>
 <button
 onclick={() => onOpenSite(site)}
 title="Open in browser"
 aria-label="Open {site.name} in browser"
 class="rounded-lg p-2 text-white/50 transition-colors hover:bg-blue-50 hover:text-violet-400 focus:outline-none"
 >
 <ExternalLink class="h-4 w-4" />
 </button>
 <button
 onclick={() => onDeleteSite(site.id, site.name)}
 title="Delete site"
 aria-label="Delete {site.name}"
 class="rounded-lg p-2 text-white/50 transition-colors hover:bg-red-500/[0.1]0/[0.15] hover:text-red-500 focus:outline-none"
 >
 <Trash2 class="h-4 w-4" />
 </button>
 </div>
 </div>
 </div>
 {/each}
 </div>
 {/if}
</div>
