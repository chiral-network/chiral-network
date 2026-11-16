import { Store, Sparkles, Server, KeyRound, Wallet, CircleUser, Globe, Settings, Bug } from 'lucide-svelte';
import type { IconComponent } from '$lib/components/ui/tones';

export type PageKey = 'marketplace' | 'playground' | 'provider' | 'usage' | 'wallet' | 'account' | 'network' | 'settings' | 'diagnostics';
export interface NavItem { key: PageKey; label: string; short: string; icon: IconComponent }

// Order is the visible priority: the top bar shows the first MAX_VISIBLE
// with labels and folds the rest into "More" (same as the legacy navbar).
export const navItems: NavItem[] = [
  { key: 'marketplace', label: 'Marketplace', short: 'Marketplace', icon: Store },
  { key: 'playground', label: 'Playground', short: 'Playground', icon: Sparkles },
  { key: 'usage', label: 'Usage & API keys', short: 'Usage', icon: KeyRound },
  { key: 'wallet', label: 'Wallet', short: 'Wallet', icon: Wallet },
  { key: 'provider', label: 'Provider', short: 'Provider', icon: Server },
  { key: 'settings', label: 'Settings', short: 'Settings', icon: Settings },
  { key: 'account', label: 'Account', short: 'Account', icon: CircleUser },
  { key: 'network', label: 'Network', short: 'Network', icon: Globe },
  { key: 'diagnostics', label: 'Diagnostics', short: 'Diagnostics', icon: Bug },
];
export const MAX_VISIBLE = 6;
