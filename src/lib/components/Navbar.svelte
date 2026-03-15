<script lang="ts">
 import { Download, Wallet, Globe, Settings, LogOut, Send, Pickaxe, Bug, Menu, X, Server, ChevronDown, HardDrive } from'lucide-svelte';
 import { goto } from'@mateothegreat/svelte5-router';
 import { isAuthenticated, walletAccount, networkConnected } from'$lib/stores';
 import { computeAnchoredDropdownPlacement } from'$lib/utils/uiPositioning';
 let { currentPage ='download' }: { currentPage?: string } = $props();
 let mobileMenuOpen = $state(false);
 let moreMenuOpen = $state(false);
 let moreButtonEl = $state<HTMLButtonElement | null>(null);
 let moreMenuLeft = $state(0);
 let moreMenuTop = $state(0);
 let moreMenuMaxHeight = $state(320);
 function handleLogout() { isAuthenticated.set(false); walletAccount.set(null); window.location.href ='/'; }
 function navigate(path: string) { goto(path); mobileMenuOpen = false; moreMenuOpen = false; }
 function positionMoreMenu() {
 if (!moreButtonEl || typeof window ==='undefined') return;
 const MENU_WIDTH = 192; const ITEM_HEIGHT = 36;
 const rect = moreButtonEl.getBoundingClientRect();
 const preferredHeight = Math.min(320, moreItems.length * ITEM_HEIGHT + 8);
 const placement = computeAnchoredDropdownPlacement({ anchorTop: rect.top, anchorBottom: rect.bottom, anchorRight: rect.right, menuWidth: MENU_WIDTH, preferredHeight, viewportWidth: window.innerWidth, viewportHeight: window.innerHeight });
 moreMenuLeft = placement.left; moreMenuTop = placement.top; moreMenuMaxHeight = placement.maxHeight;
 }
 const MAX_VISIBLE = 6;
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
 const visibleItems = navItems.slice(0, MAX_VISIBLE);
 const moreItems = navItems.slice(MAX_VISIBLE);
 $effect(() => {
 if (!moreMenuOpen) return;
 function handleClick(e: MouseEvent) { const target = e.target as HTMLElement; if (!target.closest('.more-menu-container')) { moreMenuOpen = false; } }
 document.addEventListener('click', handleClick);
 return () => document.removeEventListener('click', handleClick);
 });
 $effect(() => {
 if (!moreMenuOpen || typeof window ==='undefined') return;
 moreItems.length;
 const rafId = window.requestAnimationFrame(positionMoreMenu);
 const handleViewportChange = () => positionMoreMenu();
 window.addEventListener('resize', handleViewportChange);
 window.addEventListener('scroll', handleViewportChange, true);
 return () => { window.cancelAnimationFrame(rafId); window.removeEventListener('resize', handleViewportChange); window.removeEventListener('scroll', handleViewportChange, true); };
 });
 const isMoreActive = $derived(moreItems.some(item => item.path === currentPage));
</script>
<nav class="sticky top-0 z-50 bg-[var(--surface-1)]/90 backdrop-blur-sm border-b border-[var(--border)]">
 <div class="max-w-7xl mx-auto px-3 sm:px-4">
 <div class="flex items-center justify-between h-14 gap-2">
 <div class="flex items-center gap-2 shrink-0">
 <img src="/logo.png" alt="Chiral Network" class="w-7 h-7 rounded-lg" />
 <span class="text-sm font-semibold text-[var(--text-primary)] hidden sm:inline">Chiral Network</span>
 </div>
 <!-- Icon-only nav (md to xl) -->
 <div class="hidden md:flex xl:hidden items-center gap-0.5 shrink-0">
 {#each navItems as item}
 <button onclick={() => navigate(item.path)} class="flex items-center px-2.5 py-1.5 rounded-lg transition text-sm {currentPage === item.path ?'bg-[var(--surface-2)] text-white' :'text-[var(--text-secondary)] hover:text-white hover:bg-[var(--surface-2)]/50'}" title={item.label}>
 <svelte:component this={item.icon} class="w-4 h-4" />
 </button>
 {/each}
 </div>
 <!-- Full nav (xl+) -->
 <div class="hidden xl:flex items-center gap-0.5 shrink-0">
 {#each visibleItems as item}
 <button onclick={() => navigate(item.path)} class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg transition text-sm whitespace-nowrap {currentPage === item.path ?'bg-[var(--surface-2)] text-white font-medium' :'text-[var(--text-secondary)] hover:text-white hover:bg-[var(--surface-2)]/50'}" title={item.label}>
 <svelte:component this={item.icon} class="w-4 h-4 shrink-0" /><span>{item.label}</span>
 </button>
 {/each}
 {#if moreItems.length > 0}
 <div class="relative more-menu-container">
 <button bind:this={moreButtonEl} onclick={() => moreMenuOpen = !moreMenuOpen} class="flex items-center gap-1 px-3 py-1.5 rounded-lg transition text-sm whitespace-nowrap {isMoreActive ?'bg-[var(--surface-2)] text-white font-medium' :'text-[var(--text-secondary)] hover:text-white hover:bg-[var(--surface-2)]/50'}" title="More">
 <span>More</span><ChevronDown class="w-4 h-4 transition-transform {moreMenuOpen ?'rotate-180' :''}" />
 </button>
 {#if moreMenuOpen}
 <div class="fixed w-48 overflow-y-auto bg-[var(--surface-1)] rounded-lg border border-[var(--border)] py-1 z-50" style="left: {moreMenuLeft}px; top: {moreMenuTop}px; max-height: {moreMenuMaxHeight}px;">
 {#each moreItems as item}
 <button onclick={() => navigate(item.path)} class="flex items-center gap-2.5 w-full px-3 py-2 text-sm transition {currentPage === item.path ?'text-white bg-[var(--surface-2)]' :'text-[var(--text-secondary)] hover:text-white hover:bg-[var(--surface-2)]/50'}">
 <svelte:component this={item.icon} class="w-4 h-4" /><span>{item.label}</span>
 </button>
 {/each}
 </div>
 {/if}
 </div>
 {/if}
 </div>
 <div class="flex items-center gap-2 sm:gap-3 shrink-0">
 <button onclick={handleLogout} class="hidden sm:flex items-center gap-1.5 px-3 py-1.5 text-[var(--text-secondary)] hover:text-white hover:bg-[var(--surface-2)] rounded-lg transition text-sm" title="Logout">
 <LogOut class="w-4 h-4" /><span class="hidden lg:inline">Logout</span>
 </button>
 <div class="flex items-center gap-1.5 px-2 py-1">
 <div class="w-2 h-2 rounded-full {$networkConnected ?'bg-emerald-400' :'bg-red-400'}"></div>
 <span class="text-xs font-medium hidden sm:inline {$networkConnected ?'text-emerald-400' :'text-red-400'}">{$networkConnected ?'Connected' :'Offline'}</span>
 </div>
 <button onclick={() => mobileMenuOpen = !mobileMenuOpen} class="md:hidden p-1.5 text-[var(--text-secondary)] hover:text-white hover:bg-[var(--surface-2)] rounded-lg transition">
 {#if mobileMenuOpen}<X class="w-5 h-5" />{:else}<Menu class="w-5 h-5" />{/if}
 </button>
 </div>
 </div>
 </div>
 {#if mobileMenuOpen}
 <div class="md:hidden border-t border-[var(--border)] bg-[var(--surface-1)] max-h-[calc(100vh-3.5rem)] overflow-y-auto">
 <div class="px-3 py-2 space-y-0.5">
 {#each navItems as item}
 <button onclick={() => navigate(item.path)} class="flex items-center gap-3 w-full px-3 py-2.5 rounded-lg transition text-sm {currentPage === item.path ?'text-white bg-[var(--surface-2)] font-medium' :'text-[var(--text-secondary)] hover:text-white hover:bg-[var(--surface-2)]/50'}">
 <svelte:component this={item.icon} class="w-4 h-4" /><span>{item.label}</span>
 </button>
 {/each}
 <hr class="border-[var(--border)] my-2" />
 <button onclick={handleLogout} class="flex items-center gap-3 w-full px-3 py-2.5 text-[var(--text-secondary)] hover:text-red-400 hover:bg-red-500/10 rounded-lg transition text-sm">
 <LogOut class="w-4 h-4" /><span>Logout</span>
 </button>
 </div>
 </div>
 {/if}
</nav>
