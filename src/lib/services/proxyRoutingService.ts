/**
 * Proxy Routing Service
 * Integrates proxyLoadBalancer and privacyService to provide comprehensive proxy routing
 * Handles proxy selection based on privacy preferences and load balancing
 */

import { privacyService } from './privacyService';
import { proxyLoadBalancer } from './proxyLoadBalancer';
import { writable, derived } from 'svelte/store';

export interface ProxyRoute {
  proxyAddress: string;
  anonymousMode: boolean;
  multiHopEnabled: boolean;
  timestamp: number;
}

export interface ProxyRoutingConfig {
  preferredProxy?: string;
  useLoadBalancing: boolean;
  enablePrivacyMode: boolean;
  multiHopCount?: number;
}

class ProxyRoutingService {
  private proxyRoutes: ProxyRoute[] = [];
  private maxRouteHistory: number = 100;
  private config: ProxyRoutingConfig = {
    useLoadBalancing: true,
    enablePrivacyMode: false,
    multiHopCount: 1
  };

  /**
   * Configure proxy routing behavior
   */
  configure(config: Partial<ProxyRoutingConfig>): void {
    this.config = { ...this.config, ...config };
  }

  /**
   * Get next proxy route based on current privacy and load balancing settings
   */
  getNextRoute(): ProxyRoute | null {
    const proxyAddress = this.config.useLoadBalancing
      ? proxyLoadBalancer.getNextProxy()
      : this.config.preferredProxy;

    if (!proxyAddress) {
      console.warn('[ProxyRoutingService] No proxy available');
      return null;
    }

    const route: ProxyRoute = {
      proxyAddress,
      anonymousMode: privacyService.isAnonymousMode(),
      multiHopEnabled: privacyService.isMultiHopEnabled(),
      timestamp: Date.now()
    };

    // Track route history
    this.proxyRoutes.push(route);
    if (this.proxyRoutes.length > this.maxRouteHistory) {
      this.proxyRoutes.shift();
    }

    return route;
  }

  /**
   * Get the highest weight proxy (prioritized routing)
   */
  getPrioritizedRoute(): ProxyRoute | null {
    const proxyAddress = proxyLoadBalancer.getHighestWeightProxy();

    if (!proxyAddress) {
      console.warn('[ProxyRoutingService] No prioritized proxy available');
      return null;
    }

    return {
      proxyAddress,
      anonymousMode: privacyService.isAnonymousMode(),
      multiHopEnabled: privacyService.isMultiHopEnabled(),
      timestamp: Date.now()
    };
  }

  /**
   * Get all available proxies with current privacy settings
   */
  getAllRoutes(): ProxyRoute[] {
    return proxyLoadBalancer.getAllProxies().map(proxy => ({
      proxyAddress: proxy,
      anonymousMode: privacyService.isAnonymousMode(),
      multiHopEnabled: privacyService.isMultiHopEnabled(),
      timestamp: Date.now()
    }));
  }

  /**
   * Update proxy list and optionally set weights
   */
  updateProxies(proxies: string[]): void;
  updateProxies(proxies: { address: string; weight: number }[]): void;
  updateProxies(proxies: any): void {
    if (proxies.length === 0) return;

    // Check if weighted proxies
    if (proxies[0].weight !== undefined) {
      proxyLoadBalancer.setWeightedProxies(proxies);
    } else {
      proxyLoadBalancer.setProxies(proxies);
    }
  }

  /**
   * Set anonymous mode globally for all routes
   */
  setAnonymousMode(enabled: boolean): void {
    privacyService.setAnonymousMode(enabled);
  }

  /**
   * Set multi-hop routing globally for all routes
   */
  setMultiHop(enabled: boolean): void {
    privacyService.setMultiHop(enabled);
  }

  /**
   * Get current privacy profile
   */
  getPrivacyProfile(): { anonymous: boolean; multiHop: boolean } {
    return {
      anonymous: privacyService.isAnonymousMode(),
      multiHop: privacyService.isMultiHopEnabled()
    };
  }

  /**
   * Set privacy profile atomically
   */
  setPrivacyProfile(profile: { anonymous: boolean; multiHop: boolean }): void {
    privacyService.setPrivacyProfile(profile);
  }

  /**
   * Get routing history for debugging/analytics
   */
  getRouteHistory(): ProxyRoute[] {
    return [...this.proxyRoutes];
  }

  /**
   * Clear routing history
   */
  clearHistory(): void {
    this.proxyRoutes = [];
  }

  /**
   * Get statistics about proxy usage
   */
  getStatistics(): {
    totalRoutes: number;
    currentConfig: ProxyRoutingConfig;
    privacyProfile: { anonymous: boolean; multiHop: boolean };
    availableProxies: number;
  } {
    return {
      totalRoutes: this.proxyRoutes.length,
      currentConfig: this.config,
      privacyProfile: this.getPrivacyProfile(),
      availableProxies: proxyLoadBalancer.getAllProxies().length
    };
  }
}

// Singleton instance
export const proxyRoutingService = new ProxyRoutingService();

// Svelte stores for reactive UI bindings
export const privacyStore = writable({
  anonymousMode: false,
  multiHopEnabled: false
});

export const proxyStore = derived(
  [privacyStore],
  ([$privacy]) => ({
    currentRoute: proxyRoutingService.getNextRoute(),
    availableProxies: proxyLoadBalancer.getAllProxies(),
    privacy: $privacy
  })
);

/**
 * Initialize proxy routing service with privacy preferences from localStorage
 */
export function initializeProxyRouting(): void {
  const stored = localStorage.getItem('proxyRoutingConfig');
  if (stored) {
    try {
      const config = JSON.parse(stored);
      proxyRoutingService.configure(config);
    } catch (e) {
      console.warn('[ProxyRoutingService] Failed to load config from storage:', e);
    }
  }

  const storedPrivacy = localStorage.getItem('privacyProfile');
  if (storedPrivacy) {
    try {
      const profile = JSON.parse(storedPrivacy);
      proxyRoutingService.setPrivacyProfile(profile);
      privacyStore.set(profile);
    } catch (e) {
      console.warn('[ProxyRoutingService] Failed to load privacy profile from storage:', e);
    }
  }
}

/**
 * Persist proxy routing configuration to localStorage
 */
export function persistProxyRouting(config: ProxyRoutingConfig): void {
  try {
    localStorage.setItem('proxyRoutingConfig', JSON.stringify(config));
  } catch (e) {
    console.warn('[ProxyRoutingService] Failed to persist config:', e);
  }
}

/**
 * Persist privacy profile to localStorage
 */
export function persistPrivacyProfile(profile: { anonymous: boolean; multiHop: boolean }): void {
  try {
    localStorage.setItem('privacyProfile', JSON.stringify(profile));
    privacyStore.set({
      anonymousMode: profile.anonymous,
      multiHopEnabled: profile.multiHop
    });
  } catch (e) {
    console.warn('[ProxyRoutingService] Failed to persist privacy profile:', e);
  }
}
