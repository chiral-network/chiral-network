<script lang="ts">
 import { Download, Wallet, Globe, Settings, LogOut, Send, Pickaxe, Bug, Menu, X, ChevronLeft, ChevronRight, Server, HardDrive } from'lucide-svelte';
 import { goto } from'@mateothegreat/svelte5-router';
 import { isAuthenticated, walletAccount, networkConnected } from'$lib/stores';
 let { currentPage ='download', collapsed = $bindable(false) }: { currentPage?: string; collapsed?: boolean } = $props();
 let mobileOpen = $state(false);
 const COLLAPSED_KEY ='chiral-sidebar-collapsed';
 if (typeof window !=='undefined' && localStorage.getItem(COLLAPSED_KEY) ==='true') { collapsed = true; }
 function toggleCollapse() { collapsed = !collapsed; if (typeof window !=='undefined') { localStorage.setItem(COLLAPSED_KEY, String(collapsed)); } }
 function handleLogout() { isAuthenticated.set(false); walletAccount.set(null); window.location.href ='/'; }
 function navigate(path: string) { goto(path); mobileOpen = false; }
 const navItems = [
 { path:'/download', label:'Download', icon: Download },
 { path:'/drive', label:'Drive', icon: HardDrive },
 { path:'/account', label:'Account', icon: Wallet },
 { path:'/hosts', label:'Hosts', icon: Server },
 { path:'/network', label:'Network', icon: Globe },
 { path:'/settings', label:'Settings', icon: Settings },
 { path:'/chiraldrop', label:'ChiralDrop', icon: Send },
 { path:'/diagnostics', label:'Diagnostics', icon: Bug },
 { path:'/mining', label:'Mining', icon: Pickaxe },
 ];
</script>
<div class="md:hidden sticky top-0 z-50 flex items-center justify-between h-14 px-3 bg-[#13111C] border-b border-white/[0.06]">
 <div class="flex items-center gap-2">
 <img src="/logo.png" alt="Chiral Network" class="w-7 h-7 rounded-lg" />
 <span class="text-lg font-bold text-violet-400">Chiral</span>
 </div>
 <div class="flex items-center gap-2">
 <div class="w-2 h-2 rounded-full shrink-0 {$networkConnected ?'bg-emerald-400' :'bg-red-400'}"></div>
 <button onclick={() => mobileOpen = !mobileOpen} class="p-1.5 text-white/[0.06] hover:text-white/[0.05] hover:bg-white/[0.04] rounded-lg transition">
 {#if mobileOpen}<X class="w-5 h-5" />{:else}<Menu class="w-5 h-5" />{/if}
 </button>
 </div>
</div>
{#if mobileOpen}
 <div class="md:hidden fixed inset-0 z-40 bg-[#13111C]/60" onclick={() => mobileOpen = false}></div>
 <div class="md:hidden fixed top-14 left-0 right-0 z-50 bg-[#13111C] border-b border-white/[0.06] max-h-[calc(100vh-3.5rem)] overflow-y-auto">
 <div class="px-3 py-2 space-y-0.5">
 {#each navItems as item}
 <button onclick={() => navigate(item.path)} class="flex items-center gap-3 w-full px-3 py-2.5 rounded-lg transition text-sm {currentPage === item.path ?'bg-violet-600/[0.06] text-violet-400 border-l-2 border-blue-400' :'text-white/[0.06] hover:text-white/[0.05] hover:bg-white/[0.02]'}">
 <svelte:component this={item.icon} class="w-4 h-4" /><span class="font-medium">{item.label}</span>
 </button>
 {/each}
 <hr class="border-[var(--border)] my-2" />
 <button onclick={handleLogout} class="flex items-center gap-3 w-full px-3 py-2.5 text-white/[0.06] hover:bg-red-500/10 hover:text-red-400 rounded-lg transition text-sm">
 <LogOut class="w-4 h-4" /><span class="font-medium">Logout</span>
 </button>
 </div>
 </div>
{/if}
<aside class="hidden md:flex fixed top-0 left-0 z-40 h-screen flex-col bg-[#13111C] border-r border-white/[0.06] transition-[width] duration-200 {collapsed ?'w-16' :'w-48'}">
 <div class="flex items-center h-14 px-3 border-b border-white/[0.06] shrink-0 overflow-hidden {collapsed ?'justify-center' :'justify-between'}">
 <div class="flex items-center gap-2 overflow-hidden">
 <img src="/logo.png" alt="Chiral Network" class="w-7 h-7 rounded-lg shrink-0" />
 {#if !collapsed}<span class="text-sm font-bold text-violet-400 whitespace-nowrap">Chiral Network</span>{/if}
 </div>
 {#if !collapsed}<button onclick={toggleCollapse} class="p-1 text-white/[0.08] hover:text-white/[0.06] hover:bg-white/[0.04] rounded-md transition shrink-0" title="Collapse sidebar"><ChevronLeft class="w-4 h-4" /></button>{/if}
 </div>
 <div class="flex items-center gap-3 px-3 py-2 border-b border-white/[0.06] shrink-0 {collapsed ?'justify-center' :''}">
 <div class="w-2 h-2 rounded-full shrink-0 {$networkConnected ?'bg-emerald-400' :'bg-red-400'}"></div>
 {#if !collapsed}<span class="text-xs font-medium whitespace-nowrap {$networkConnected ?'text-emerald-400' :'text-red-400'}">{$networkConnected ?'Connected' :'Offline'}</span>{/if}
 </div>
 <nav class="flex-1 px-2 py-3 space-y-0.5 overflow-y-auto overflow-x-hidden">
 {#each navItems as item}
 <button onclick={() => navigate(item.path)} class="flex items-center gap-3 w-full py-2 rounded-lg transition text-sm {collapsed ?'justify-center px-0' :'px-3'} {currentPage === item.path ?'bg-violet-600/[0.06] text-violet-400 border-l-2 border-blue-400' :'text-white/[0.06] hover:text-white/[0.05] hover:bg-white/[0.02]'}" title={collapsed ? item.label :''}>
 <svelte:component this={item.icon} class="w-4 h-4 shrink-0" />
 {#if !collapsed}<span class="font-medium whitespace-nowrap">{item.label}</span>{/if}
 </button>
 {/each}
 </nav>
 <div class="px-2 pb-3 space-y-1 border-t border-white/[0.06] pt-3 shrink-0">
 {#if collapsed}<button onclick={toggleCollapse} class="flex items-center justify-center w-full py-2 text-white/[0.08] hover:text-white/[0.06] hover:bg-white/[0.04] rounded-lg transition text-sm" title="Expand sidebar"><ChevronRight class="w-4 h-4 shrink-0" /></button>{/if}
 <button onclick={handleLogout} class="flex items-center gap-3 w-full py-2 text-white/[0.06] hover:bg-red-500/10 hover:text-red-400 rounded-lg transition text-sm {collapsed ?'justify-center px-0' :'px-3'}" title={collapsed ?'Logout' :''}>
 <LogOut class="w-4 h-4 shrink-0" />{#if !collapsed}<span class="font-medium whitespace-nowrap">Logout</span>{/if}
 </button>
 </div>
</aside>
