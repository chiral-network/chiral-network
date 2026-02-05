import { describe, it, expect, vi } from 'vitest';
import { generateAlias, aliasFromPeerId, ALIAS_COLORS, type UserAlias } from '$lib/aliasService';

describe('aliasService', () => {
  describe('generateAlias', () => {
    it('should return a valid UserAlias object', () => {
      const alias = generateAlias();
      expect(alias).toHaveProperty('color');
      expect(alias).toHaveProperty('animal');
      expect(alias).toHaveProperty('displayName');
      expect(alias).toHaveProperty('colorHex');
    });

    it('should have displayName as "Color Animal" format', () => {
      const alias = generateAlias();
      expect(alias.displayName).toBe(`${alias.color} ${alias.animal}`);
    });

    it('should have a valid hex color code', () => {
      const alias = generateAlias();
      expect(alias.colorHex).toMatch(/^#[0-9a-fA-F]{6}$/);
    });

    it('should return a color from the ALIAS_COLORS map', () => {
      const alias = generateAlias();
      expect(ALIAS_COLORS[alias.color]).toBeDefined();
      expect(alias.colorHex).toBe(ALIAS_COLORS[alias.color]);
    });

    it('should generate different aliases over multiple calls (probabilistic)', () => {
      const aliases = new Set<string>();
      for (let i = 0; i < 50; i++) {
        aliases.add(generateAlias().displayName);
      }
      // With 24x24 = 576 combinations and 50 trials, we should get at least 2 unique
      expect(aliases.size).toBeGreaterThan(1);
    });
  });

  describe('aliasFromPeerId', () => {
    it('should return a valid UserAlias object', () => {
      const alias = aliasFromPeerId('12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE');
      expect(alias).toHaveProperty('color');
      expect(alias).toHaveProperty('animal');
      expect(alias).toHaveProperty('displayName');
      expect(alias).toHaveProperty('colorHex');
    });

    it('should produce deterministic results for the same peer ID', () => {
      const peerId = '12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE';
      const alias1 = aliasFromPeerId(peerId);
      const alias2 = aliasFromPeerId(peerId);
      expect(alias1.displayName).toBe(alias2.displayName);
      expect(alias1.color).toBe(alias2.color);
      expect(alias1.animal).toBe(alias2.animal);
      expect(alias1.colorHex).toBe(alias2.colorHex);
    });

    it('should produce different aliases for different peer IDs', () => {
      const alias1 = aliasFromPeerId('12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE');
      const alias2 = aliasFromPeerId('12D3KooWETLNJUVLbkAbenbSPPdwN9ZLkBU3TLfyAeEUW2dsVptr');
      // Different peer IDs should (very likely) produce different aliases
      // We can't guarantee it, but with 576 combinations it's highly unlikely they match
      expect(alias1.displayName !== alias2.displayName || alias1.color !== alias2.color).toBe(true);
    });

    it('should have a valid hex color for the generated alias', () => {
      const alias = aliasFromPeerId('somePeerId123');
      expect(alias.colorHex).toMatch(/^#[0-9a-fA-F]{6}$/);
    });

    it('should handle empty string peer ID', () => {
      const alias = aliasFromPeerId('');
      expect(alias).toHaveProperty('displayName');
      expect(alias.displayName.split(' ')).toHaveLength(2);
    });

    it('should handle very long peer ID strings', () => {
      const longPeerId = 'a'.repeat(10000);
      const alias = aliasFromPeerId(longPeerId);
      expect(alias).toHaveProperty('displayName');
      expect(alias.colorHex).toMatch(/^#[0-9a-fA-F]{6}$/);
    });
  });

  describe('ALIAS_COLORS', () => {
    it('should have 24 color entries', () => {
      expect(Object.keys(ALIAS_COLORS)).toHaveLength(24);
    });

    it('should have valid hex color codes for all entries', () => {
      for (const [name, hex] of Object.entries(ALIAS_COLORS)) {
        expect(hex).toMatch(/^#[0-9a-fA-F]{6}$/);
      }
    });

    it('should include common color names', () => {
      expect(ALIAS_COLORS).toHaveProperty('Red');
      expect(ALIAS_COLORS).toHaveProperty('Blue');
      expect(ALIAS_COLORS).toHaveProperty('Green');
    });
  });
});
