import { describe, it, expect, beforeEach } from 'vitest';
import { PrivacyService, privacyService } from '../src/lib/services/privacyService';

describe('privacyService.ts', () => {
  describe('PrivacyService Class', () => {
    let service: PrivacyService;

    beforeEach(() => {
      service = new PrivacyService();
    });

    describe('Anonymous Mode', () => {
      it('should default to anonymous mode disabled', () => {
        expect(service.isAnonymousMode()).toBe(false);
      });

      it('should enable anonymous mode', () => {
        service.setAnonymousMode(true);
        expect(service.isAnonymousMode()).toBe(true);
      });

      it('should disable anonymous mode', () => {
        service.setAnonymousMode(true);
        service.setAnonymousMode(false);
        expect(service.isAnonymousMode()).toBe(false);
      });

      it('should toggle anonymous mode multiple times', () => {
        service.setAnonymousMode(true);
        expect(service.isAnonymousMode()).toBe(true);
        
        service.setAnonymousMode(false);
        expect(service.isAnonymousMode()).toBe(false);
        
        service.setAnonymousMode(true);
        expect(service.isAnonymousMode()).toBe(true);
      });

      it('should maintain state after being set', () => {
        service.setAnonymousMode(true);
        
        // Check multiple times to ensure state persistence
        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isAnonymousMode()).toBe(true);
      });
    });

    describe('Multi-Hop', () => {
      it('should default to multi-hop disabled', () => {
        expect(service.isMultiHopEnabled()).toBe(false);
      });

      it('should enable multi-hop', () => {
        service.setMultiHop(true);
        expect(service.isMultiHopEnabled()).toBe(true);
      });

      it('should disable multi-hop', () => {
        service.setMultiHop(true);
        service.setMultiHop(false);
        expect(service.isMultiHopEnabled()).toBe(false);
      });

      it('should toggle multi-hop multiple times', () => {
        service.setMultiHop(true);
        expect(service.isMultiHopEnabled()).toBe(true);
        
        service.setMultiHop(false);
        expect(service.isMultiHopEnabled()).toBe(false);
        
        service.setMultiHop(true);
        expect(service.isMultiHopEnabled()).toBe(true);
      });

      it('should maintain state after being set', () => {
        service.setMultiHop(true);
        
        // Check multiple times to ensure state persistence
        expect(service.isMultiHopEnabled()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(true);
      });
    });

    describe('Privacy Profile', () => {
      it('should set both anonymous and multi-hop via profile', () => {
        service.setPrivacyProfile({
          anonymous: true,
          multiHop: true,
        });

        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(true);
      });

      it('should set only anonymous mode via profile', () => {
        service.setPrivacyProfile({
          anonymous: true,
          multiHop: false,
        });

        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(false);
      });

      it('should set only multi-hop via profile', () => {
        service.setPrivacyProfile({
          anonymous: false,
          multiHop: true,
        });

        expect(service.isAnonymousMode()).toBe(false);
        expect(service.isMultiHopEnabled()).toBe(true);
      });

      it('should disable both via profile', () => {
        // First enable both
        service.setAnonymousMode(true);
        service.setMultiHop(true);

        // Then disable via profile
        service.setPrivacyProfile({
          anonymous: false,
          multiHop: false,
        });

        expect(service.isAnonymousMode()).toBe(false);
        expect(service.isMultiHopEnabled()).toBe(false);
      });

      it('should override individual settings with profile', () => {
        service.setAnonymousMode(true);
        service.setMultiHop(false);

        service.setPrivacyProfile({
          anonymous: false,
          multiHop: true,
        });

        expect(service.isAnonymousMode()).toBe(false);
        expect(service.isMultiHopEnabled()).toBe(true);
      });

      it('should allow profile changes multiple times', () => {
        service.setPrivacyProfile({
          anonymous: true,
          multiHop: true,
        });
        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(true);

        service.setPrivacyProfile({
          anonymous: false,
          multiHop: false,
        });
        expect(service.isAnonymousMode()).toBe(false);
        expect(service.isMultiHopEnabled()).toBe(false);

        service.setPrivacyProfile({
          anonymous: true,
          multiHop: false,
        });
        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(false);
      });
    });

    describe('Settings Independence', () => {
      it('should keep anonymous mode independent from multi-hop', () => {
        service.setAnonymousMode(true);
        expect(service.isMultiHopEnabled()).toBe(false);
      });

      it('should keep multi-hop independent from anonymous mode', () => {
        service.setMultiHop(true);
        expect(service.isAnonymousMode()).toBe(false);
      });

      it('should allow independent toggling of settings', () => {
        service.setAnonymousMode(true);
        service.setMultiHop(false);
        
        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(false);

        service.setMultiHop(true);
        
        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(true);

        service.setAnonymousMode(false);
        
        expect(service.isAnonymousMode()).toBe(false);
        expect(service.isMultiHopEnabled()).toBe(true);
      });
    });

    describe('applyPrivacySettings', () => {
      it('should return the request unchanged', () => {
        const request = { url: 'http://example.com', data: 'test' };
        const result = service.applyPrivacySettings(request);
        
        expect(result).toBe(request);
        expect(result).toEqual({ url: 'http://example.com', data: 'test' });
      });

      it('should handle null request', () => {
        const result = service.applyPrivacySettings(null);
        expect(result).toBeNull();
      });

      it('should handle undefined request', () => {
        const result = service.applyPrivacySettings(undefined);
        expect(result).toBeUndefined();
      });

      it('should handle empty object request', () => {
        const request = {};
        const result = service.applyPrivacySettings(request);
        expect(result).toEqual({});
      });

      it('should handle complex request object', () => {
        const request = {
          headers: { 'Content-Type': 'application/json' },
          body: { data: 'test' },
          metadata: { timestamp: Date.now() },
        };
        const result = service.applyPrivacySettings(request);
        expect(result).toBe(request);
      });

      it('should not modify request when anonymous mode is enabled', () => {
        service.setAnonymousMode(true);
        const request = { url: 'http://example.com' };
        const result = service.applyPrivacySettings(request);
        
        expect(result).toBe(request);
      });

      it('should not modify request when multi-hop is enabled', () => {
        service.setMultiHop(true);
        const request = { url: 'http://example.com' };
        const result = service.applyPrivacySettings(request);
        
        expect(result).toBe(request);
      });
    });

    describe('State Combinations', () => {
      it('should handle all four state combinations', () => {
        // State 1: both off
        expect(service.isAnonymousMode()).toBe(false);
        expect(service.isMultiHopEnabled()).toBe(false);

        // State 2: anonymous on, multi-hop off
        service.setAnonymousMode(true);
        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(false);

        // State 3: both on
        service.setMultiHop(true);
        expect(service.isAnonymousMode()).toBe(true);
        expect(service.isMultiHopEnabled()).toBe(true);

        // State 4: anonymous off, multi-hop on
        service.setAnonymousMode(false);
        expect(service.isAnonymousMode()).toBe(false);
        expect(service.isMultiHopEnabled()).toBe(true);
      });
    });
  });

  describe('Singleton Instance', () => {
    it('should export a singleton instance', () => {
      expect(privacyService).toBeInstanceOf(PrivacyService);
    });

    it('should maintain state across calls to singleton', () => {
      privacyService.setAnonymousMode(true);
      expect(privacyService.isAnonymousMode()).toBe(true);
      
      privacyService.setAnonymousMode(false);
      expect(privacyService.isAnonymousMode()).toBe(false);
    });

    it('should allow independent instances to have different states', () => {
      const instance1 = new PrivacyService();
      const instance2 = new PrivacyService();

      instance1.setAnonymousMode(true);
      instance2.setAnonymousMode(false);

      expect(instance1.isAnonymousMode()).toBe(true);
      expect(instance2.isAnonymousMode()).toBe(false);
    });

    it('should not share state between new instances and singleton', () => {
      const newInstance = new PrivacyService();
      
      privacyService.setAnonymousMode(true);
      newInstance.setAnonymousMode(false);

      expect(privacyService.isAnonymousMode()).toBe(true);
      expect(newInstance.isAnonymousMode()).toBe(false);
    });
  });

  describe('Edge Cases', () => {
    let service: PrivacyService;

    beforeEach(() => {
      service = new PrivacyService();
    });

    it('should handle rapid state changes', () => {
      for (let i = 0; i < 100; i++) {
        service.setAnonymousMode(i % 2 === 0);
      }
      expect(service.isAnonymousMode()).toBe(false);
    });

    it('should handle setting same value repeatedly', () => {
      service.setAnonymousMode(true);
      service.setAnonymousMode(true);
      service.setAnonymousMode(true);
      
      expect(service.isAnonymousMode()).toBe(true);
    });

    it('should handle profile with same values repeatedly', () => {
      const profile = { anonymous: true, multiHop: true };
      
      service.setPrivacyProfile(profile);
      service.setPrivacyProfile(profile);
      service.setPrivacyProfile(profile);

      expect(service.isAnonymousMode()).toBe(true);
      expect(service.isMultiHopEnabled()).toBe(true);
    });
  });
});