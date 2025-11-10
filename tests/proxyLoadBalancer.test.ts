import { describe, it, expect, beforeEach } from 'vitest';
import { ProxyLoadBalancer, proxyLoadBalancer } from '../src/lib/services/proxyLoadBalancer';

describe('proxyLoadBalancer.ts', () => {
  describe('ProxyLoadBalancer Class', () => {
    let loadBalancer: ProxyLoadBalancer;

    beforeEach(() => {
      loadBalancer = new ProxyLoadBalancer();
    });

    describe('Basic Proxy Management', () => {
      it('should start with no proxies', () => {
        expect(loadBalancer.getAllProxies()).toEqual([]);
      });

      it('should return null when no proxies are set', () => {
        expect(loadBalancer.getNextProxy()).toBeNull();
      });

      it('should set proxies from array', () => {
        loadBalancer.setProxies(['proxy1.com:8080', 'proxy2.com:8080']);
        
        expect(loadBalancer.getAllProxies()).toEqual([
          'proxy1.com:8080',
          'proxy2.com:8080'
        ]);
      });

      it('should handle empty proxy list', () => {
        loadBalancer.setProxies([]);
        
        expect(loadBalancer.getAllProxies()).toEqual([]);
        expect(loadBalancer.getNextProxy()).toBeNull();
      });

      it('should handle single proxy', () => {
        loadBalancer.setProxies(['proxy1.com:8080']);
        
        expect(loadBalancer.getNextProxy()).toBe('proxy1.com:8080');
        expect(loadBalancer.getNextProxy()).toBe('proxy1.com:8080');
      });
    });

    describe('Round-Robin Load Balancing', () => {
      it('should rotate through proxies in order', () => {
        loadBalancer.setProxies([
          'proxy1.com:8080',
          'proxy2.com:8080',
          'proxy3.com:8080'
        ]);

        expect(loadBalancer.getNextProxy()).toBe('proxy1.com:8080');
        expect(loadBalancer.getNextProxy()).toBe('proxy2.com:8080');
        expect(loadBalancer.getNextProxy()).toBe('proxy3.com:8080');
      });

      it('should cycle back to first proxy', () => {
        loadBalancer.setProxies(['proxy1.com:8080', 'proxy2.com:8080']);

        loadBalancer.getNextProxy(); // proxy1
        loadBalancer.getNextProxy(); // proxy2
        
        expect(loadBalancer.getNextProxy()).toBe('proxy1.com:8080');
      });

      it('should maintain rotation state across multiple cycles', () => {
        loadBalancer.setProxies(['proxy1.com:8080', 'proxy2.com:8080']);

        for (let i = 0; i < 10; i++) {
          const proxy = loadBalancer.getNextProxy();
          const expected = i % 2 === 0 ? 'proxy1.com:8080' : 'proxy2.com:8080';
          expect(proxy).toBe(expected);
        }
      });

      it('should reset index when proxies are updated', () => {
        loadBalancer.setProxies(['proxy1.com:8080', 'proxy2.com:8080']);
        
        loadBalancer.getNextProxy(); // proxy1
        loadBalancer.getNextProxy(); // proxy2
        
        // Set new proxies - should reset to start
        loadBalancer.setProxies(['proxyA.com:8080', 'proxyB.com:8080']);
        
        expect(loadBalancer.getNextProxy()).toBe('proxyA.com:8080');
      });
    });

    describe('Weighted Proxy Management', () => {
      it('should set weighted proxies', () => {
        loadBalancer.setWeightedProxies([
          { address: 'proxy1.com:8080', weight: 10 },
          { address: 'proxy2.com:8080', weight: 5 }
        ]);

        const proxies = loadBalancer.getAllProxies();
        expect(proxies).toHaveLength(2);
        expect(proxies).toContain('proxy1.com:8080');
        expect(proxies).toContain('proxy2.com:8080');
      });

      it('should sort proxies by weight descending', () => {
        loadBalancer.setWeightedProxies([
          { address: 'proxy1.com:8080', weight: 5 },
          { address: 'proxy2.com:8080', weight: 10 },
          { address: 'proxy3.com:8080', weight: 3 }
        ]);

        const proxies = loadBalancer.getAllProxies();
        expect(proxies[0]).toBe('proxy2.com:8080'); // weight 10
        expect(proxies[1]).toBe('proxy1.com:8080'); // weight 5
        expect(proxies[2]).toBe('proxy3.com:8080'); // weight 3
      });

      it('should handle equal weights', () => {
        loadBalancer.setWeightedProxies([
          { address: 'proxy1.com:8080', weight: 5 },
          { address: 'proxy2.com:8080', weight: 5 },
          { address: 'proxy3.com:8080', weight: 5 }
        ]);

        const proxies = loadBalancer.getAllProxies();
        expect(proxies).toHaveLength(3);
      });

      it('should handle zero weights', () => {
        loadBalancer.setWeightedProxies([
          { address: 'proxy1.com:8080', weight: 10 },
          { address: 'proxy2.com:8080', weight: 0 }
        ]);

        const proxies = loadBalancer.getAllProxies();
        expect(proxies[0]).toBe('proxy1.com:8080');
        expect(proxies[1]).toBe('proxy2.com:8080');
      });

      it('should handle negative weights', () => {
        loadBalancer.setWeightedProxies([
          { address: 'proxy1.com:8080', weight: 10 },
          { address: 'proxy2.com:8080', weight: -5 }
        ]);

        const proxies = loadBalancer.getAllProxies();
        expect(proxies[0]).toBe('proxy1.com:8080');
      });

      it('should reset index when setting weighted proxies', () => {
        loadBalancer.setProxies(['proxy1.com:8080', 'proxy2.com:8080']);
        loadBalancer.getNextProxy(); // Move index
        
        loadBalancer.setWeightedProxies([
          { address: 'proxyA.com:8080', weight: 10 }
        ]);

        const proxies = loadBalancer.getAllProxies();
        expect(proxies[0]).toBe('proxyA.com:8080');
      });
    });

    describe('Prioritized Routing', () => {
      it('should return highest weight proxy', () => {
        loadBalancer.setWeightedProxies([
          { address: 'proxy1.com:8080', weight: 5 },
          { address: 'proxy2.com:8080', weight: 10 },
          { address: 'proxy3.com:8080', weight: 3 }
        ]);

        expect(loadBalancer.getHighestWeightProxy()).toBe('proxy2.com:8080');
      });

      it('should return null when no proxies', () => {
        expect(loadBalancer.getHighestWeightProxy()).toBeNull();
      });

      it('should return first proxy when using setProxies', () => {
        loadBalancer.setProxies([
          'proxy1.com:8080',
          'proxy2.com:8080',
          'proxy3.com:8080'
        ]);

        expect(loadBalancer.getHighestWeightProxy()).toBe('proxy1.com:8080');
      });

      it('should not affect round-robin state', () => {
        loadBalancer.setWeightedProxies([
          { address: 'proxy1.com:8080', weight: 10 },
          { address: 'proxy2.com:8080', weight: 5 }
        ]);

        loadBalancer.getHighestWeightProxy();
        
        // Should still start round-robin from beginning
        expect(loadBalancer.getNextProxy()).toBe('proxy1.com:8080');
      });
    });

    describe('Edge Cases', () => {
      it('should handle proxy addresses with various formats', () => {
        loadBalancer.setProxies([
          'proxy.com:8080',
          '192.168.1.1:3128',
          'https://proxy.example.com:443',
          'socks5://proxy.local:1080'
        ]);

        expect(loadBalancer.getAllProxies()).toHaveLength(4);
      });

      it('should handle very large proxy lists', () => {
        const largeList = Array.from({ length: 1000 }, (_, i) => `proxy${i}.com:8080`);
        loadBalancer.setProxies(largeList);

        expect(loadBalancer.getAllProxies()).toHaveLength(1000);
        expect(loadBalancer.getNextProxy()).toBe('proxy0.com:8080');
      });

      it('should handle rapid consecutive calls', () => {
        loadBalancer.setProxies(['proxy1.com:8080', 'proxy2.com:8080']);

        const results = [];
        for (let i = 0; i < 100; i++) {
          results.push(loadBalancer.getNextProxy());
        }

        expect(results).toHaveLength(100);
        expect(results.filter(p => p === 'proxy1.com:8080')).toHaveLength(50);
        expect(results.filter(p => p === 'proxy2.com:8080')).toHaveLength(50);
      });
    });
  });

  describe('Singleton Instance', () => {
    it('should export a singleton instance', () => {
      expect(proxyLoadBalancer).toBeInstanceOf(ProxyLoadBalancer);
    });

    it('should maintain state in singleton', () => {
      proxyLoadBalancer.setProxies(['singleton1.com:8080', 'singleton2.com:8080']);
      
      expect(proxyLoadBalancer.getNextProxy()).toBe('singleton1.com:8080');
      expect(proxyLoadBalancer.getNextProxy()).toBe('singleton2.com:8080');
    });

    it('should allow independent instances', () => {
      const instance1 = new ProxyLoadBalancer();
      const instance2 = new ProxyLoadBalancer();

      instance1.setProxies(['proxy1.com:8080']);
      instance2.setProxies(['proxy2.com:8080']);

      expect(instance1.getNextProxy()).toBe('proxy1.com:8080');
      expect(instance2.getNextProxy()).toBe('proxy2.com:8080');
    });
  });
});