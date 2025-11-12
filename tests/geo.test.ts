import { describe, it, expect } from 'vitest';
import {
  GEO_REGIONS,
  normalizeRegion,
  projectToMap,
  UNKNOWN_REGION_ID,
  type GeoRegionConfig,
} from '../src/lib/geo';

describe('geo.ts', () => {
  describe('GEO_REGIONS', () => {
    it('should have all required regions defined', () => {
      expect(GEO_REGIONS.length).toBeGreaterThan(0);
      
      const requiredRegions = [
        'usEast',
        'usWest',
        'euWest',
        'euCentral',
        'asiaPacific',
        'asiaEast',
        'southAmerica',
        'africa',
        'oceania',
        'unknown',
      ];

      const regionIds = GEO_REGIONS.map((r) => r.id);
      requiredRegions.forEach((regionId) => {
        expect(regionIds).toContain(regionId);
      });
    });

    it('should have valid coordinates for all regions', () => {
      GEO_REGIONS.forEach((region) => {
        expect(region.lat).toBeGreaterThanOrEqual(-90);
        expect(region.lat).toBeLessThanOrEqual(90);
        expect(region.lng).toBeGreaterThanOrEqual(-180);
        expect(region.lng).toBeLessThanOrEqual(180);
      });
    });

    it('should have valid color codes for all regions', () => {
      const hexColorRegex = /^#[0-9a-f]{6}$/i;
      GEO_REGIONS.forEach((region) => {
        expect(region.color).toMatch(hexColorRegex);
      });
    });

    it('should have unique IDs', () => {
      const ids = GEO_REGIONS.map((r) => r.id);
      const uniqueIds = new Set(ids);
      expect(uniqueIds.size).toBe(ids.length);
    });

    it('should have at least one alias per region', () => {
      GEO_REGIONS.forEach((region) => {
        expect(region.aliases.length).toBeGreaterThan(0);
      });
    });
  });

  describe('normalizeRegion', () => {
    it('should return unknown region for null input', () => {
      const result = normalizeRegion(null);
      expect(result.id).toBe(UNKNOWN_REGION_ID);
    });

    it('should return unknown region for undefined input', () => {
      const result = normalizeRegion(undefined);
      expect(result.id).toBe(UNKNOWN_REGION_ID);
    });

    it('should return unknown region for empty string', () => {
      const result = normalizeRegion('');
      expect(result.id).toBe(UNKNOWN_REGION_ID);
    });

    it('should return unknown region for whitespace-only string', () => {
      const result = normalizeRegion('   ');
      expect(result.id).toBe(UNKNOWN_REGION_ID);
    });

    it('should normalize region by exact ID match', () => {
      const result = normalizeRegion('usEast');
      expect(result.id).toBe('usEast');
      expect(result.label).toBe('US-East');
    });

    it('should normalize region case-insensitively', () => {
      const result = normalizeRegion('USEAST');
      expect(result.id).toBe('usEast');
    });

    it('should normalize region by alias', () => {
      const result = normalizeRegion('us-east');
      expect(result.id).toBe('usEast');
    });

    it('should handle aliases with special characters', () => {
      const result = normalizeRegion('US_EAST');
      expect(result.id).toBe('usEast');
    });

    it('should normalize all US regions correctly', () => {
      expect(normalizeRegion('useast').id).toBe('usEast');
      expect(normalizeRegion('us-east').id).toBe('usEast');
      expect(normalizeRegion('na-east').id).toBe('usEast');
      expect(normalizeRegion('uswest').id).toBe('usWest');
      expect(normalizeRegion('us-west').id).toBe('usWest');
    });

    it('should normalize all EU regions correctly', () => {
      expect(normalizeRegion('euwest').id).toBe('euWest');
      expect(normalizeRegion('eu-west').id).toBe('euWest');
      expect(normalizeRegion('eucentral').id).toBe('euCentral');
      expect(normalizeRegion('eu').id).toBe('euCentral');
    });

    it('should normalize all Asia regions correctly', () => {
      expect(normalizeRegion('asia-pacific').id).toBe('asiaPacific');
      expect(normalizeRegion('apac').id).toBe('asiaPacific');
      expect(normalizeRegion('asia').id).toBe('asiaPacific');
      expect(normalizeRegion('asia-east').id).toBe('asiaEast');
      expect(normalizeRegion('jp').id).toBe('asiaEast');
    });

    it('should normalize South America region', () => {
      expect(normalizeRegion('southamerica').id).toBe('southAmerica');
      expect(normalizeRegion('south-america').id).toBe('southAmerica');
      expect(normalizeRegion('latam').id).toBe('southAmerica');
      expect(normalizeRegion('sa').id).toBe('southAmerica');
    });

    it('should normalize Africa region', () => {
      expect(normalizeRegion('africa').id).toBe('africa');
      expect(normalizeRegion('za').id).toBe('africa');
      expect(normalizeRegion('nigeria').id).toBe('africa');
    });

    it('should normalize Oceania region', () => {
      expect(normalizeRegion('oceania').id).toBe('oceania');
      expect(normalizeRegion('australia').id).toBe('oceania');
      expect(normalizeRegion('anz').id).toBe('oceania');
      expect(normalizeRegion('nz').id).toBe('oceania');
    });

    it('should handle special characters and normalize', () => {
      const result = normalizeRegion('US-East!!!');
      expect(result.id).toBe('usEast');
    });

    it('should return unknown for unrecognized regions', () => {
      const result = normalizeRegion('mars');
      expect(result.id).toBe(UNKNOWN_REGION_ID);
    });

    it('should trim whitespace before normalizing', () => {
      const result = normalizeRegion('  useast  ');
      expect(result.id).toBe('usEast');
    });
  });

  describe('projectToMap', () => {
    it('should project coordinates to map correctly', () => {
      const width = 1000;
      const height = 500;

      // Test center of map (0°, 0°)
      const center = projectToMap(0, 0, width, height);
      expect(center.x).toBe(500); // (0 + 180) / 360 * 1000
      expect(center.y).toBe(250); // (90 - 0) / 180 * 500
    });

    it('should project North Pole correctly', () => {
      const width = 1000;
      const height = 500;
      
      const northPole = projectToMap(90, 0, width, height);
      expect(northPole.x).toBe(500);
      expect(northPole.y).toBe(0);
    });

    it('should project South Pole correctly', () => {
      const width = 1000;
      const height = 500;
      
      const southPole = projectToMap(-90, 0, width, height);
      expect(southPole.x).toBe(500);
      expect(southPole.y).toBe(500);
    });

    it('should project western edge correctly', () => {
      const width = 1000;
      const height = 500;
      
      const west = projectToMap(0, -180, width, height);
      expect(west.x).toBe(0);
      expect(west.y).toBe(250);
    });

    it('should project eastern edge correctly', () => {
      const width = 1000;
      const height = 500;
      
      const east = projectToMap(0, 180, width, height);
      expect(east.x).toBe(1000);
      expect(east.y).toBe(250);
    });

    it('should project US-East coordinates correctly', () => {
      const width = 1000;
      const height = 500;
      const usEast = GEO_REGIONS.find((r) => r.id === 'usEast')!;
      
      const projected = projectToMap(usEast.lat, usEast.lng, width, height);
      
      expect(projected.x).toBeGreaterThan(0);
      expect(projected.x).toBeLessThan(width);
      expect(projected.y).toBeGreaterThan(0);
      expect(projected.y).toBeLessThan(height);
    });

    it('should project all regions within bounds', () => {
      const width = 1000;
      const height = 500;

      GEO_REGIONS.forEach((region) => {
        const projected = projectToMap(region.lat, region.lng, width, height);
        
        expect(projected.x).toBeGreaterThanOrEqual(0);
        expect(projected.x).toBeLessThanOrEqual(width);
        expect(projected.y).toBeGreaterThanOrEqual(0);
        expect(projected.y).toBeLessThanOrEqual(height);
      });
    });

    it('should scale correctly with different dimensions', () => {
      const lat = 45;
      const lng = 90;

      const small = projectToMap(lat, lng, 100, 50);
      const large = projectToMap(lat, lng, 1000, 500);

      // Large should be 10x the small dimensions
      expect(large.x).toBeCloseTo(small.x * 10, 1);
      expect(large.y).toBeCloseTo(small.y * 10, 1);
    });

    it('should handle zero dimensions gracefully', () => {
      const result = projectToMap(0, 0, 0, 0);
      expect(result.x).toBe(0);
      expect(result.y).toBe(0);
    });

    it('should produce consistent results for same inputs', () => {
      const width = 800;
      const height = 400;
      const lat = 35.7;
      const lng = 139.7;

      const result1 = projectToMap(lat, lng, width, height);
      const result2 = projectToMap(lat, lng, width, height);

      expect(result1.x).toBe(result2.x);
      expect(result1.y).toBe(result2.y);
    });
  });
});