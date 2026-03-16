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
 if (score >= 80) return'text-emerald-600 dark:text-emerald-400';
 if (score >= 60) return'text-violet-600 dark:text-violet-400';
 if (score >= 40) return'text-yellow-600 dark:text-yellow-400';
 return'text-red-500';
 }

 function eloBg(score: number): string {
 if (score >= 80) return'bg-emerald-100 text-emerald-800';
 if (score >= 60) return'bg-violet-100 dark:bg-violet-900/20 text-violet-600 dark:text-violet-400';
 if (score >= 40) return'bg-yellow-500/100/10 text-yellow-600 dark:text-yellow-400';
 return'bg-red-500/10 text-red-600 dark:text-red-400';
 }
</script>

<!-- Host Marketplace Settings -->
<div class="rounded-xl border border-[var(--border)]/60 bg-[var(--surface-2)] overflow-hidden">
 <div class="flex items-center justify-between gap-4 p-5 pb-4">
 <div class="flex items-center gap-3">
 <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--surface-2)]">
 <Settings2 class="h-4.5 w-4.5 text-[var(--text-secondary)]" />
 </div>
 <div>
 <h2 class="font-semibold text-base text-[var(--text-primary)]">Host Settings</h2>
 <p class="text-xs text-[var(--text-secondary)] mt-0.5">
 Configure your hosting offer for the network
 </p>
 </div>
 </div>
 <button
 onclick={onToggleEnabled}
 class="relative w-12 h-7 rounded-full transition-colors focus:outline-none focus:/50 
 {$settings.hostingConfig.enabled ?'bg-violet-600' :'bg-[var(--surface-2)]'}"
 role="switch"
 aria-checked={$settings.hostingConfig.enabled}
 aria-label="Toggle hosting"
 >
 <span
 class="absolute top-0.5 left-0.5 w-6 h-6 bg-[var(--surface-2)] rounded-full transition-transform
 {$settings.hostingConfig.enabled ?'translate-x-5' :'translate-x-0'}"
 ></span>
 </button>
 </div>

 {#if $settings.hostingConfig.enabled}
 <div class="border-t border-[var(--border)]/60 px-5 py-4">
 <div class="grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-4">
 <div>
 <label for="host-max-storage-gb" class="block text-xs font-medium text-[var(--text-secondary)] mb-1.5 uppercase tracking-wide">
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
 class="w-24 px-3 py-2 text-sm bg-[var(--surface-2)] border border-[var(--border)]/60 rounded-lg text-[var(--text-primary)] tabular-nums
 focus:border-cyan-500/30 focus:outline-none focus:"
 />
 <span class="text-xs text-[var(--text-secondary)] font-medium">GB</span>
 </div>
 </div>

 <div>
 <label for="host-price-chi" class="block text-xs font-medium text-[var(--text-secondary)] mb-1.5 uppercase tracking-wide">
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
 class="w-32 px-3 py-2 text-sm bg-[var(--surface-2)] border border-[var(--border)]/60 rounded-lg text-[var(--text-primary)] tabular-nums
 focus:border-cyan-500/30 focus:outline-none focus:"
 />
 <span class="text-xs text-[var(--text-secondary)] font-medium">CHI/MB/day</span>
 </div>
 </div>

 <div>
 <label for="host-deposit-chi" class="block text-xs font-medium text-[var(--text-secondary)] mb-1.5 uppercase tracking-wide">
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
 class="w-32 px-3 py-2 text-sm bg-[var(--surface-2)] border border-[var(--border)]/60 rounded-lg text-[var(--text-primary)] tabular-nums
 focus:border-cyan-500/30 focus:outline-none focus:"
 />
 <span class="text-xs text-[var(--text-secondary)] font-medium">CHI</span>
 </div>
 </div>

 <div>
 <div class="flex items-center justify-between mb-1.5">
 <label for="host-auto-accept-elo" class="block text-xs font-medium text-[var(--text-secondary)] uppercase tracking-wide">
 Auto-Accept
 </label>
 <button
 onclick={() => updateAutoAcceptByElo(!$settings.hostingConfig.autoAcceptByElo)}
 class="relative w-9 h-5 rounded-full transition-colors focus:outline-none focus:/50
 {$settings.hostingConfig.autoAcceptByElo ?'bg-violet-600' :'bg-[var(--surface-2)]'}"
 role="switch"
 aria-checked={$settings.hostingConfig.autoAcceptByElo}
 aria-label="Toggle auto accept"
 >
 <span
 class="absolute top-0.5 left-0.5 w-4 h-4 bg-[var(--surface-2)] rounded-full transition-transform
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
 class="w-24 px-3 py-2 text-sm bg-[var(--surface-2)] border border-[var(--border)]/60 rounded-lg text-[var(--text-primary)] tabular-nums
 focus:border-cyan-500/30 focus:outline-none focus:
 disabled:opacity-40 disabled:cursor-not-allowed"
 />
 <span class="text-xs text-[var(--text-secondary)] font-medium">Min Elo</span>
 </div>
 <p class="text-[11px] text-[var(--text-secondary)] mt-1.5 leading-tight">
 Auto-accept proposals from peers at or above this reputation score.
 </p>
 </div>
 </div>

 <div class="mt-5 flex items-center gap-3 pt-4 border-t border-[var(--border)]/60">
 <button
 onclick={onPublish}
 disabled={hostingPublishing}
 class="flex items-center gap-2 px-4 py-2 text-sm font-medium bg-violet-600 hover:bg-violet-500 text-white rounded-lg transition-colors
 focus:outline-none focus:border-violet-500/50/50 
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
 class="px-4 py-2 text-sm font-medium text-[var(--text-secondary)] border border-[var(--border)]/60 rounded-lg
 hover:bg-[var(--surface-2)] transition-colors
 focus:outline-none focus:border-violet-500/50
 disabled:opacity-50 disabled:cursor-not-allowed"
 >
 Unpublish
 </button>
 </div>
 </div>
 {/if}
</div>

<!-- Available Hosts -->
<div class="rounded-xl border border-[var(--border)]/60 bg-[var(--surface-1)] p-5">
 <div class="flex items-center justify-between mb-4">
 <div class="flex items-center gap-3">
 <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-violet-500/10">
 <Users class="w-4.5 h-4.5 text-violet-600 dark:text-violet-400" />
 </div>
 <div>
 <h2 class="font-semibold text-base text-[var(--text-primary)]">Available Hosts</h2>
 <p class="text-xs text-[var(--text-secondary)] mt-0.5">
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
 class="text-xs bg-[var(--surface-2)] border border-[var(--border)]/60 rounded-lg px-2.5 py-1.5 text-[var(--text-secondary)]
 focus:border-cyan-500/30 focus:outline-none focus:"
 >
 <option value="reputation">Reputation</option>
 <option value="price">Price (low)</option>
 <option value="storage">Storage (high)</option>
 </select>
 <button
 onclick={onRefreshHosts}
 disabled={loadingHosts}
 class="p-2 text-[var(--text-secondary)] hover:text-[var(--text-secondary)] rounded-lg hover:bg-[var(--surface-2)] transition-colors
 focus:outline-none focus:border-violet-500/50 disabled:opacity-50"
 title="Refresh host list"
 aria-label="Refresh host list"
 >
 <RefreshCw class="w-4 h-4 {loadingHosts ?'animate-spin' :''}" />
 </button>
 </div>
 </div>

 {#if loadingHosts}
 <div class="flex flex-col items-center justify-center py-16">
 <Loader2 class="w-6 h-6 text-[var(--text-secondary)] animate-spin mb-3" />
 <span class="text-sm text-[var(--text-secondary)]">Discovering hosts on the network...</span>
 </div>
 {:else if sortedHostList.length === 0}
 <div class="flex flex-col items-center justify-center py-16 text-[var(--text-secondary)]">
 <div class="flex h-16 w-16 items-center justify-center rounded-xl bg-[var(--surface-2)] mb-4">
 <Users class="w-8 h-8 opacity-40" />
 </div>
 <p class="text-sm font-medium text-[var(--text-secondary)]">No hosts available</p>
 <p class="text-xs mt-1 text-[var(--text-secondary)]">
 Peers offering hosting services will appear here
 </p>
 </div>
 {:else}
 <div class="space-y-3">
 {#each sortedHostList as host (host.advertisement.peerId)}
 <div class="group p-4 rounded-xl border border-[var(--border)]/60 bg-[var(--surface-2)]
 hover:border-[var(--border)]  transition-all">
 <div class="flex items-start justify-between gap-4">
 <div class="min-w-0 flex-1">
 <!-- Header row -->
 <div class="flex items-center gap-2.5 flex-wrap">
 <span class="relative flex h-2.5 w-2.5 flex-shrink-0">
 {#if host.isOnline}
 <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-green-400 opacity-75"></span>
 {/if}
 <span class="relative inline-flex h-2.5 w-2.5 rounded-full {host.isOnline ?'bg-green-500' :'bg-[var(--surface-2)]'}"></span>
 </span>
 <span class="text-sm font-semibold text-[var(--text-primary)] font-mono">
 {formatPeerId(host.advertisement.peerId)}
 </span>
 <span class="inline-flex items-center px-2 py-0.5 rounded-full text-[11px] font-semibold tabular-nums {eloBg(host.reputationScore)}">
 {host.reputationScore.toFixed(0)} Elo
 </span>
 </div>

 <!-- Stats row -->
 <div class="flex items-center gap-4 mt-2.5 flex-wrap">
 <span class="flex items-center gap-1.5 text-xs text-[var(--text-secondary)]">
 <HardDrive class="w-3.5 h-3.5 text-[var(--text-secondary)]" />
 <span class="font-medium text-[var(--text-secondary)]">{formatBytes(host.availableStorageBytes)}</span>
 </span>
 <span class="flex items-center gap-1.5 text-xs text-[var(--text-secondary)]">
 <Coins class="w-3.5 h-3.5 text-[var(--text-secondary)]" />
 <span class="font-medium text-[var(--text-secondary)]">{formatWeiAsChi(host.advertisement.pricePerMbPerDayWei)}</span>
 <span class="text-[var(--text-secondary)]">/MB/day</span>
 </span>
 <span class="flex items-center gap-1.5 text-xs text-[var(--text-secondary)]">
 <Shield class="w-3.5 h-3.5 text-[var(--text-secondary)]" />
 <span>Deposit: {formatWeiAsChi(host.advertisement.minDepositWei)}</span>
 </span>
 <span class="flex items-center gap-1.5 text-xs text-[var(--text-secondary)]">
 <Clock class="w-3.5 h-3.5 text-[var(--text-secondary)]" />
 <span class="tabular-nums">{host.advertisement.uptimePercent.toFixed(0)}%</span> uptime
 </span>
 </div>
 </div>

 <button
 onclick={() => onPropose(host)}
 class="flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium bg-violet-600 hover:bg-violet-500 text-white rounded-xl transition-all flex-shrink-0
  active:scale-[0.98]
 focus:outline-none focus:border-violet-500/50/50"
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
