import { describe, expect, test } from 'bun:test';
import { ArcLogic } from './arc_logic.js';

describe('ArcLogic (Arrow functions)', () => {
  describe('getArrowPoints', () => {
    test('generates correct points at scale 1.0', () => {
      const points = ArcLogic.getArrowPoints({ x: 100, y: 50 }, 1.0);
      // Arrow at (100, 50), len=8, hw=4
      // Format: "tip.x+len,tip.y-hw tip.x,tip.y tip.x+len,tip.y+hw"
      expect(points).toBe('108,46 100,50 108,54');
    });

    test('scales arrow dimensions correctly', () => {
      const points = ArcLogic.getArrowPoints({ x: 100, y: 50 }, 2.0);
      // Arrow at (100, 50), len=16, hw=8
      expect(points).toBe('116,42 100,50 116,58');
    });

    test('handles small scale factors', () => {
      const points = ArcLogic.getArrowPoints({ x: 100, y: 50 }, 0.5);
      // Arrow at (100, 50), len=4, hw=2
      expect(points).toBe('104,48 100,50 104,52');
    });
  });

  describe('parseTipFromPoints', () => {
    test('extracts tip coordinates from valid points string', () => {
      const tip = ArcLogic.parseTipFromPoints('108,46 100,50 108,54');
      expect(tip).toEqual({ x: 100, y: 50 });
    });

    test('returns null for single point (parts.length === 1)', () => {
      const tip = ArcLogic.parseTipFromPoints('108,46');
      expect(tip).toBeNull();
    });

    test('returns null for empty string', () => {
      const tip = ArcLogic.parseTipFromPoints('');
      expect(tip).toBeNull();
    });

    test('returns null for malformed coordinate pair', () => {
      const tip = ArcLogic.parseTipFromPoints('108,46 invalid 108,54');
      expect(tip).toBeNull();
    });
  });

  describe('scaleFromStrokeWidth', () => {
    test('calculates correct scale for base stroke width', () => {
      expect(ArcLogic.scaleFromStrokeWidth(1.5)).toBe(1.0);
    });

    test('calculates correct scale for larger stroke width', () => {
      expect(ArcLogic.scaleFromStrokeWidth(3.0)).toBe(2.0);
    });
  });

  describe('constants', () => {
    test('exports ARROW_LENGTH', () => {
      expect(ArcLogic.ARROW_LENGTH).toBe(8);
    });

    test('exports ARROW_HALF_WIDTH', () => {
      expect(ArcLogic.ARROW_HALF_WIDTH).toBe(4);
    });
  });
});

describe('ArcLogic.isArcVisibleForLayers', () => {
  test('cycle arc is visible via cluster layer when its dep-type layer is off', () => {
    const membership = { isModuleDep: true, isCycle: true };
    const active = { moduleDep: false, clusterMode: true };
    expect(ArcLogic.isArcVisibleForLayers(membership, active)).toBe(true);
  });

  test('cycle arc is visible via its dep-type layer when cluster is off', () => {
    const membership = { isModuleDep: true, isCycle: true };
    const active = { moduleDep: true, clusterMode: false };
    expect(ArcLogic.isArcVisibleForLayers(membership, active)).toBe(true);
  });

  test('cycle arc is hidden when both its dep-type and cluster layers are off', () => {
    const membership = { isModuleDep: true, isCycle: true };
    const active = { moduleDep: false, clusterMode: false };
    expect(ArcLogic.isArcVisibleForLayers(membership, active)).toBe(false);
  });

  test('non-cycle dep arc is hidden when its dep-type layer is off, even with cluster on', () => {
    const membership = { isModuleDep: true, isCycle: false };
    const active = { moduleDep: false, clusterMode: true };
    expect(ArcLogic.isArcVisibleForLayers(membership, active)).toBe(false);
  });

  test('crate-dep arc follows the crate layer', () => {
    const membership = { isCrateDep: true };
    expect(ArcLogic.isArcVisibleForLayers(membership, { crateDep: true })).toBe(
      true,
    );
    expect(
      ArcLogic.isArcVisibleForLayers(membership, { crateDep: false }),
    ).toBe(false);
  });

  test('reexport arc follows the reexport layer', () => {
    const membership = { isReexport: true };
    expect(ArcLogic.isArcVisibleForLayers(membership, { reexport: true })).toBe(
      true,
    );
    expect(
      ArcLogic.isArcVisibleForLayers(membership, { reexport: false }),
    ).toBe(false);
  });
});
