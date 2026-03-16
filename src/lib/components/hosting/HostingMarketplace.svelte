<script lang="ts">
 import {
 Users, HardDrive, Clock, Coins, Shield,
 Loader2, RefreshCw, Rocket, Settings2
 } from'lucide-svelte';
 import { settings } from'$lib/stores';
 import {
 formatHostedFileSize as formatBytes,
 formatPeerId,
 formatWeiAsChi,
 weiToChiNumber,
 chiToWeiString,
 } from'$lib/utils/hostingPageUtils';
 import type { HostEntry } from'$lib/types/hosting';

 interface Props {
 hosts: HostEntry[];
 loadingHosts: boolean;
 hostingPublishing: boolean;
 sortBy:'reputation' |'price' |'storage';
 onSortChange: (sort:'reputation' |'price' |'storage') => void;
 onRefreshHosts: () => void;
 onPropose: (host: HostEntry) => void;
 onToggleEnabled: () => void;
 onPublish: () => void;
 onUnpublish: () => void;
 }

 let {
 hosts,
 loadingHosts,
 hostingPublishing,
 sortBy,
 onSortChange,
 onRefreshHosts,
 onPropose,
 onToggleEnabled,
 onPublish,
 onUnpublish,
 }: Props = $props();

 let sortedHostList = $derived(sortHosts(hosts, sortBy));

 function sortHosts(entries: HostEntry[], sort: string): HostEntry[] {
 return [...entries].sort((a, b) => {
 switch (sort) {
 case'price':
 return Number(BigInt(a.advertisement.pricePerMbPerDayWei) - BigInt(b.advertisement.pricePerMbPerDayWei));
 case'storage':
 return b.availableStorageBytes - a.availableStorageBytes;
 case'reputation':
 default:
 return b.reputationScore - a.reputationScore;
 }
 });
 }

 function updateMaxStorageGb(gb: number) {
 const boundedGb = Math.max(1, Math.min(10000, Math.floor(gb || 1)));
 settings.update((s) => ({
 ...s,
 hostingConfig: { ...s.hostingConfig, maxStorageBytes: boundedGb * 1024 * 1024 * 1024 },
 }));
 }

 function updatePriceChi(chiPerMbPerDay: number) {
 settings.update((s) => ({
 ...s,
 hostingConfig: {
 ...s.hostingConfig,
 pricePerMbPerDayWei: chiToWeiString(chiPerMbPerDay, s.hostingConfig.pricePerMbPerDayWei),
 },
 }));
 }

 function updateDepositChi(depositChi: number) {
 settings.update((s) => ({
 ...s,
 hostingConfig: {
 ...s.hostingConfig,
 minDepositWei: chiToWeiString(depositChi, s.hostingConfig.minDepositWei),
 },
 }));
 }

 function updateAutoAcceptByElo(enabled: boolean) {
 settings.update((s) => ({
 ...s,
 hostingConfig: { ...s.hostingConfig, autoAcceptByElo: enabled },
 }));
 }

 function updateAutoAcceptMinElo(elo: number) {
 const bounded = Math.max(0, Math.min(100, Math.round(elo || 0)));
 settings.update((s) => ({
 ...s,
 hostingConfig: { ...s.hostingConfig, minAutoAcceptElo: bounded },
 }));
 }

 function eloColor(score: number): string {
 if (score >= 80) return'text-emerald-600';
 if (score >= 60) return'text-blue-600';
 if (score >= 40) return'text-yellow-600 dark:text-yellow-400 dark:text-yellow-400';
 return'text-red-500';
 }

 function eloBg(score: number): string {
 if (score >= 80) return'bg-emerald-100 dark:bg-emerald-900/20 text-emerald-800 dark:text-emerald-200';
 if (score >= 60) return'bg-blue-100 dark:bg-blue-500/[0.15] text-blue-800';
 if (score >= 40) return'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-800 dark:text-yellow-200';
 return'bg-red-100 dark:bg-red-900/20 text-red-800 dark:text-red-200';
 }
</script>

<!-- Host Marketplace Settings -->
<div class="rounded-xl bg-white/70 dark:bg-white/[0.05] shadow-gray-200/50 dark:shadow-black/5 border border-gray-200/60 dark:border-white/[0.06] overflow-hidden">
 <div class="flex items-center justify-between gap-4 p-5 pb-4">
 <div class="flex items-center gap-3">
 <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-white/70 dark:bg-white/[0.05]">
 <Settings2 class="h-4.5 w-4.5 text-gray-400 dark:text-white/40" />
 </div>
 <div>
 <h2 class="font-semibold text-base text-gray-900 dark:text-white/90">Host Settings</h2>
 <p class="text-xs text-gray-400 dark:text-white/40 mt-0.5">
 Configure your hosting offer for the network
 </p>
 </div>
 </div>
 <button
 onclick={onToggleEnabled}
 class="relative w-12 h-7 rounded-full transition-colors focus:outline-none 
 {$settings.hostingConfig.enabled ?'bg-violet-500' :'bg-white/70 dark:bg-white/[0.05]'}"
 role="switch"
 aria-checked={$settings.hostingConfig.enabled}
 aria-label="Toggle hosting"
 >
 <span
 class="absolute top-0.5 left-0.5 w-6 h-6 bg-white/60 dark:bg-white/[0.03] rounded-full transition-transform
 {$settings.hostingConfig.enabled ?'translate-x-5' :'translate-x-0'}"
 ></span>
 </button>
 </div>

 {#if $settings.hostingConfig.enabled}
 <div class="border-t border-gray-200/60 dark:border-white/[0.06] px-5 py-4">
 <div class="grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-4">
 <div>
 <label for="host-max-storage-gb" class="block text-xs font-medium text-gray-500 dark:text-white/50 mb-1.5 uppercase tracking-wide">
 Max Storage
 </label>
 <div class="flex items-center gap-2">
 <input
 id="host-max-storage-gb"
 type="number"
 min="1"
 max="10000"
 step="1"
 value={Math.round($settings.hostingConfig.maxStorageBytes / (1024 * 1024 * 1024))}
 oninput={(e) => updateMaxStorageGb(Number(e.currentTarget.value))}
 class="w-24 px-3 py-2 text-sm bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-lg text-gray-900 dark:text-white/90 tabular-nums
 focus:border-primary-400 focus:outline-none"
 />
 <span class="text-xs text-gray-400 dark:text-white/40 font-medium">GB</span>
 </div>
 </div>

 <div>
 <label for="host-price-chi" class="block text-xs font-medium text-gray-500 dark:text-white/50 mb-1.5 uppercase tracking-wide">
 Price
 </label>
 <div class="flex items-center gap-2">
 <input
 id="host-price-chi"
 type="number"
 min="0.000001"
 max="100"
 step="0.000001"
 value={weiToChiNumber($settings.hostingConfig.pricePerMbPerDayWei, 0.001)}
 oninput={(e) => updatePriceChi(Number(e.currentTarget.value))}
 class="w-32 px-3 py-2 text-sm bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-lg text-gray-900 dark:text-white/90 tabular-nums
 focus:border-primary-400 focus:outline-none"
 />
 <span class="text-xs text-gray-400 dark:text-white/40 font-medium">CHI/MB/day</span>
 </div>
 </div>

 <div>
 <label for="host-deposit-chi" class="block text-xs font-medium text-gray-500 dark:text-white/50 mb-1.5 uppercase tracking-wide">
 Min Deposit
 </label>
 <div class="flex items-center gap-2">
 <input
 id="host-deposit-chi"
 type="number"
 min="0"
 max="100000"
 step="0.000001"
 value={weiToChiNumber($settings.hostingConfig.minDepositWei, 0.1)}
 oninput={(e) => updateDepositChi(Number(e.currentTarget.value))}
 class="w-32 px-3 py-2 text-sm bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-lg text-gray-900 dark:text-white/90 tabular-nums
 focus:border-primary-400 focus:outline-none"
 />
 <span class="text-xs text-gray-400 dark:text-white/40 font-medium">CHI</span>
 </div>
 </div>

 <div>
 <div class="flex items-center justify-between mb-1.5">
 <label for="host-auto-accept-elo" class="block text-xs font-medium text-gray-500 dark:text-white/50 uppercase tracking-wide">
 Auto-Accept
 </label>
 <button
 onclick={() => updateAutoAcceptByElo(!$settings.hostingConfig.autoAcceptByElo)}
 class="relative w-9 h-5 rounded-full transition-colors focus:outline-none 
 {$settings.hostingConfig.autoAcceptByElo ?'bg-violet-500' :'bg-white/70 dark:bg-white/[0.05]'}"
 role="switch"
 aria-checked={$settings.hostingConfig.autoAcceptByElo}
 aria-label="Toggle auto accept"
 >
 <span
 class="absolute top-0.5 left-0.5 w-4 h-4 bg-white/60 dark:bg-white/[0.03] rounded-full transition-transform
 {$settings.hostingConfig.autoAcceptByElo ?'translate-x-4' :'translate-x-0'}"
 ></span>
 </button>
 </div>
 <div class="flex items-center gap-2">
 <input
 id="host-auto-accept-elo"
 type="number"
 min="0"
 max="100"
 step="1"
 value={$settings.hostingConfig.minAutoAcceptElo}
 oninput={(e) => updateAutoAcceptMinElo(Number(e.currentTarget.value))}
 disabled={!$settings.hostingConfig.autoAcceptByElo}
 class="w-24 px-3 py-2 text-sm bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-lg text-gray-900 dark:text-white/90 tabular-nums
 focus:border-primary-400 focus:outline-none 
 disabled:opacity-40 disabled:cursor-not-allowed"
 />
 <span class="text-xs text-gray-400 dark:text-white/40 font-medium">Min Elo</span>
 </div>
 <p class="text-[11px] text-gray-500 dark:text-white/50 mt-1.5 leading-tight">
 Auto-accept proposals from peers at or above this reputation score.
 </p>
 </div>
 </div>

 <div class="mt-5 flex items-center gap-3 pt-4 border-t border-gray-200/60 dark:border-white/[0.06]">
 <button
 onclick={onPublish}
 disabled={hostingPublishing}
 class="flex items-center gap-2 px-4 py-2 text-sm font-medium bg-violet-500/80 border border-primary-400/30 hover:bg-violet-500/90 text-white rounded-lg transition-colors
 focus:outline-none 
 disabled:opacity-50 disabled:cursor-not-allowed"
 >
 {#if hostingPublishing}
 <Loader2 class="w-3.5 h-3.5 animate-spin" />
 {/if}
 {hostingPublishing ?'Publishing...' :'Publish to Network'}
 </button>
 <button
 onclick={onUnpublish}
 disabled={hostingPublishing}
 class="px-4 py-2 text-sm font-medium text-gray-500 dark:text-white/50 border border-gray-200/60 dark:border-white/[0.06] rounded-lg
 hover:bg-gray-100 dark:hover:bg-white/[0.05] transition-colors
 focus:outline-none 
 disabled:opacity-50 disabled:cursor-not-allowed"
 >
 Unpublish
 </button>
 </div>
 </div>
 {/if}
</div>

<!-- Available Hosts -->
<div class="rounded-xl bg-white/70 dark:bg-white/[0.05] p-5 shadow-gray-200/50 dark:shadow-black/5 border border-gray-200/60 dark:border-white/[0.06]">
 <div class="flex items-center justify-between mb-4">
 <div class="flex items-center gap-3">
 <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-purple-500/15">
 <Users class="w-4.5 h-4.5 text-purple-600 dark:text-purple-400" />
 </div>
 <div>
 <h2 class="font-semibold text-base text-gray-900 dark:text-white/90">Available Hosts</h2>
 <p class="text-xs text-gray-400 dark:text-white/40 mt-0.5">
 {#if loadingHosts}
 Searching the network...
 {:else}
 {hosts.length} host{hosts.length !== 1 ?'s' :''} discovered
 {/if}
 </p>
 </div>
 </div>

 <div class="flex items-center gap-2">
 <select
 value={sortBy}
 onchange={(e) => onSortChange(e.currentTarget.value as'reputation' |'price' |'storage')}
 aria-label="Sort hosts by"
 class="text-xs bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-lg px-2.5 py-1.5 text-gray-500 dark:text-white/50
 focus:border-primary-400 focus:outline-none"
 >
 <option value="reputation">Reputation</option>
 <option value="price">Price (low)</option>
 <option value="storage">Storage (high)</option>
 </select>
 <button
 onclick={onRefreshHosts}
 disabled={loadingHosts}
 class="p-2 text-gray-500 dark:text-white/50 hover:text-gray-500 dark:text-white/50 rounded-lg hover:bg-gray-100 dark:hover:bg-white/[0.05] transition-colors
 focus:outline-none disabled:opacity-50"
 title="Refresh host list"
 aria-label="Refresh host list"
 >
 <RefreshCw class="w-4 h-4 {loadingHosts ?'animate-spin' :''}" />
 </button>
 </div>
 </div>

 {#if loadingHosts}
 <div class="flex flex-col items-center justify-center py-16">
 <Loader2 class="w-6 h-6 text-gray-500 dark:text-white/50 animate-spin mb-3" />
 <span class="text-sm text-gray-500 dark:text-white/50">Discovering hosts on the network...</span>
 </div>
 {:else if sortedHostList.length === 0}
 <div class="flex flex-col items-center justify-center py-16 text-gray-500 dark:text-white/50">
 <div class="flex h-16 w-16 items-center justify-center rounded-xl bg-white/70 dark:bg-white/[0.05] mb-4">
 <Users class="w-8 h-8 opacity-40" />
 </div>
 <p class="text-sm font-medium text-gray-400 dark:text-white/40">No hosts available</p>
 <p class="text-xs mt-1 text-gray-500 dark:text-white/50">
 Peers offering hosting services will appear here
 </p>
 </div>
 {:else}
 <div class="space-y-3">
 {#each sortedHostList as host (host.advertisement.peerId)}
 <div class="group p-4 rounded-xl border border-gray-200/60 dark:border-white/[0.06] bg-white/70 dark:bg-white/[0.05]
 hover:border-gray-200/60 dark:border-white/[0.06] hover:shadow-sm transition-all">
 <div class="flex items-start justify-between gap-4">
 <div class="min-w-0 flex-1">
 <!-- Header row -->
 <div class="flex items-center gap-2.5 flex-wrap">
 <span class="relative flex h-2.5 w-2.5 flex-shrink-0">
 {#if host.isOnline}
 <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-green-400 opacity-75"></span>
 {/if}
 <span class="relative inline-flex h-2.5 w-2.5 rounded-full {host.isOnline ?'bg-green-500' :'bg-gray-200 dark:bg-white/[0.12]'}"></span>
 </span>
 <span class="text-sm font-semibold text-gray-900 dark:text-white/90 font-mono">
 {formatPeerId(host.advertisement.peerId)}
 </span>
 <span class="inline-flex items-center px-2 py-0.5 rounded-full text-[11px] font-semibold tabular-nums {eloBg(host.reputationScore)}">
 {host.reputationScore.toFixed(0)} Elo
 </span>
 </div>

 <!-- Stats row -->
 <div class="flex items-center gap-4 mt-2.5 flex-wrap">
 <span class="flex items-center gap-1.5 text-xs text-gray-400 dark:text-white/40">
 <HardDrive class="w-3.5 h-3.5 text-gray-500 dark:text-white/50" />
 <span class="font-medium text-gray-500 dark:text-white/50">{formatBytes(host.availableStorageBytes)}</span>
 </span>
 <span class="flex items-center gap-1.5 text-xs text-gray-400 dark:text-white/40">
 <Coins class="w-3.5 h-3.5 text-gray-500 dark:text-white/50" />
 <span class="font-medium text-gray-500 dark:text-white/50">{formatWeiAsChi(host.advertisement.pricePerMbPerDayWei)}</span>
 <span class="text-gray-500 dark:text-white/50">/MB/day</span>
 </span>
 <span class="flex items-center gap-1.5 text-xs text-gray-400 dark:text-white/40">
 <Shield class="w-3.5 h-3.5 text-gray-500 dark:text-white/50" />
 <span>Deposit: {formatWeiAsChi(host.advertisement.minDepositWei)}</span>
 </span>
 <span class="flex items-center gap-1.5 text-xs text-gray-400 dark:text-white/40">
 <Clock class="w-3.5 h-3.5 text-gray-500 dark:text-white/50" />
 <span class="tabular-nums">{host.advertisement.uptimePercent.toFixed(0)}%</span> uptime
 </span>
 </div>
 </div>

 <button
 onclick={() => onPropose(host)}
 class="flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium bg-violet-500/80 border border-primary-400/30 hover:bg-violet-500/90 text-white rounded-xl transition-all flex-shrink-0
 shadow-primary-500/10 hover:shadow-md hover:shadow-primary-500/20 active:scale-[0.98]
 focus:outline-none"
 >
 <Rocket class="w-3.5 h-3.5" />
 Propose
 </button>
 </div>
 </div>
 {/each}
 </div>
 {/if}
</div>
