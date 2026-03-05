import { describe, it, expect } from 'vitest';
import {
  computeContextMenuPlacement,
  computeAnchoredDropdownPlacement,
} from '$lib/utils/uiPositioning';

describe('uiPositioning', () => {
  describe('computeContextMenuPlacement', () => {
    it('clamps menu within right/bottom viewport edges', () => {
      const placement = computeContextMenuPlacement({
        pointerX: 790,
        pointerY: 590,
        menuWidth: 220,
        menuHeight: 240,
        viewportWidth: 800,
        viewportHeight: 600,
      });

      expect(placement.left).toBeLessThanOrEqual(800 - 220 - 8);
      expect(placement.top).toBeLessThanOrEqual(600 - 240 - 8);
      expect(placement.maxHeight).toBe(584);
    });

    it('opens upward when there is no space below but enough above', () => {
      const placement = computeContextMenuPlacement({
        pointerX: 300,
        pointerY: 560,
        menuWidth: 180,
        menuHeight: 260,
        viewportWidth: 900,
        viewportHeight: 600,
      });

      expect(placement.top).toBe(300); // 560 - 260
    });

    it('enforces minimum menu height for tiny viewports', () => {
      const placement = computeContextMenuPlacement({
        pointerX: 10,
        pointerY: 10,
        menuWidth: 150,
        menuHeight: 600,
        viewportWidth: 300,
        viewportHeight: 120,
      });

      expect(placement.maxHeight).toBe(140);
      expect(placement.left).toBe(10);
      expect(placement.top).toBe(8);
    });
  });

  describe('computeAnchoredDropdownPlacement', () => {
    it('opens below when there is enough vertical space', () => {
      const placement = computeAnchoredDropdownPlacement({
        anchorTop: 20,
        anchorBottom: 52,
        anchorRight: 700,
        menuWidth: 192,
        preferredHeight: 240,
        viewportWidth: 1024,
        viewportHeight: 768,
      });

      expect(placement.openUp).toBe(false);
      expect(placement.top).toBe(58); // anchorBottom + gap(6)
      expect(placement.left).toBe(508); // 700 - 192
      expect(placement.maxHeight).toBeGreaterThan(500);
    });

    it('opens above when there is not enough space below', () => {
      const placement = computeAnchoredDropdownPlacement({
        anchorTop: 720,
        anchorBottom: 748,
        anchorRight: 980,
        menuWidth: 192,
        preferredHeight: 300,
        viewportWidth: 1024,
        viewportHeight: 768,
      });

      expect(placement.openUp).toBe(true);
      expect(placement.top).toBeLessThan(720);
      expect(placement.maxHeight).toBeGreaterThanOrEqual(140);
    });

    it('clamps horizontally when anchor is near the left edge', () => {
      const placement = computeAnchoredDropdownPlacement({
        anchorTop: 50,
        anchorBottom: 80,
        anchorRight: 100,
        menuWidth: 220,
        preferredHeight: 180,
        viewportWidth: 320,
        viewportHeight: 640,
      });

      expect(placement.left).toBeGreaterThanOrEqual(8);
    });
  });
});
