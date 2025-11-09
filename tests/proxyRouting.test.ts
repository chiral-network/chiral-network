import { describe, it, expect, beforeEach } from 'vitest';
import { proxyRoutingService, privacyStore } from '$lib/services/proxyRoutingService';
import { get } from 'svelte/store';

/**
 * Test Suite for Proxy Routing Service
 * Tests the integration between proxyLoadBalancer and privacyService
 */

describe('ProxyRoutingService', () => {
  beforeEach(() => {
    proxyRoutingService.clearHistory();
    proxyRoutingService.configure({
      useLoadBalancing: true,
      enablePrivacyMode: false,
      multiHopCount: 1
    });
  });

  describe('Proxy Routing', () => {
    it('should return null when no proxies configured', () => {
      const route = proxyRoutingService.getNextRoute();
      expect(route).toBeNull();
    });

    it('should return valid route when proxies are set', () => {
      proxyRoutingService.updateProxies(['proxy1.com:8080', 'proxy2.com:8080']);
      const route = proxyRoutingService.getNextRoute();
      
      expect(route).not.toBeNull();
      expect(route?.proxyAddress).toBeDefined();
      expect(['proxy1.com:8080', 'proxy2.com:8080']).toContain(route?.proxyAddress);
    });

    it('should round-robin through proxies', () => {
      proxyRoutingService.updateProxies(['proxy1.com:8080', 'proxy2.com:8080']);
      
      const route1 = proxyRoutingService.getNextRoute();
      const route2 = proxyRoutingService.getNextRoute();
      const route3 = proxyRoutingService.getNextRoute();
      
      expect(route1?.proxyAddress).toBeDefined();
      expect(route2?.proxyAddress).toBeDefined();
      expect(route3?.proxyAddress).toBe(route1?.proxyAddress); // Should cycle back
    });

    it('should support weighted proxies', () => {
      proxyRoutingService.updateProxies([
        { address: 'proxy1.com:8080', weight: 10 },
        { address: 'proxy2.com:8080', weight: 5 }
      ]);
      
      const route = proxyRoutingService.getNextRoute();
      expect(route).not.toBeNull();
    });
  });

  describe('Privacy Settings', () => {
    it('should track anonymous mode', () => {
      proxyRoutingService.setAnonymousMode(true);
      const profile = proxyRoutingService.getPrivacyProfile();
      
      expect(profile.anonymous).toBe(true);
    });

    it('should track multi-hop setting', () => {
      proxyRoutingService.setMultiHop(true);
      const profile = proxyRoutingService.getPrivacyProfile();
      
      expect(profile.multiHop).toBe(true);
    });

    it('should set privacy profile atomically', () => {
      const newProfile = { anonymous: true, multiHop: true };
      proxyRoutingService.setPrivacyProfile(newProfile);
      const profile = proxyRoutingService.getPrivacyProfile();
      
      expect(profile).toEqual(newProfile);
    });

    it('should include privacy settings in routes', () => {
      proxyRoutingService.updateProxies(['proxy1.com:8080']);
      proxyRoutingService.setAnonymousMode(true);
      proxyRoutingService.setMultiHop(true);
      
      const route = proxyRoutingService.getNextRoute();
      
      expect(route?.anonymousMode).toBe(true);
      expect(route?.multiHopEnabled).toBe(true);
    });
  });

  describe('Route History', () => {
    it('should maintain route history', () => {
      proxyRoutingService.updateProxies(['proxy1.com:8080']);
      
      proxyRoutingService.getNextRoute();
      proxyRoutingService.getNextRoute();
      
      const history = proxyRoutingService.getRouteHistory();
      expect(history.length).toBe(2);
    });

    it('should limit history size', () => {
      proxyRoutingService.updateProxies(['proxy1.com:8080']);
      
      // Generate more than max routes
      for (let i = 0; i < 150; i++) {
        proxyRoutingService.getNextRoute();
      }
      
      const history = proxyRoutingService.getRouteHistory();
      expect(history.length).toBeLessThanOrEqual(100);
    });

    it('should clear history', () => {
      proxyRoutingService.updateProxies(['proxy1.com:8080']);
      proxyRoutingService.getNextRoute();
      
      proxyRoutingService.clearHistory();
      const history = proxyRoutingService.getRouteHistory();
      
      expect(history.length).toBe(0);
    });
  });

  describe('Statistics', () => {
    it('should return valid statistics', () => {
      proxyRoutingService.updateProxies(['proxy1.com:8080', 'proxy2.com:8080']);
      proxyRoutingService.setAnonymousMode(true);
      
      const stats = proxyRoutingService.getStatistics();
      
      expect(stats.totalRoutes).toBeDefined();
      expect(stats.availableProxies).toBe(2);
      expect(stats.privacyProfile.anonymous).toBe(true);
      expect(stats.currentConfig).toBeDefined();
    });
  });

  describe('Prioritized Routing', () => {
    it('should return highest weight proxy', () => {
      proxyRoutingService.updateProxies([
        { address: 'proxy1.com:8080', weight: 10 },
        { address: 'proxy2.com:8080', weight: 5 },
        { address: 'proxy3.com:8080', weight: 1 }
      ]);
      
      const route = proxyRoutingService.getPrioritizedRoute();
      expect(route?.proxyAddress).toBe('proxy1.com:8080');
    });
  });

  describe('Configuration', () => {
    it('should apply configuration changes', () => {
      proxyRoutingService.configure({
        useLoadBalancing: false,
        enablePrivacyMode: true,
        preferredProxy: 'myproxy.com:8080'
      });
      
      const stats = proxyRoutingService.getStatistics();
      expect(stats.currentConfig.useLoadBalancing).toBe(false);
      expect(stats.currentConfig.enablePrivacyMode).toBe(true);
    });
  });
});

describe('Privacy Store Bindings', () => {
  it('should have privacy store reactive updates', () => {
    const privacyValue = get(privacyStore);
    expect(privacyValue).toHaveProperty('anonymousMode');
    expect(privacyValue).toHaveProperty('multiHopEnabled');
  });
});
