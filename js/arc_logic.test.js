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

    test('puts the base left of the tip for an arrow that points right', () => {
      const points = ArcLogic.getArrowPoints(
        { x: 100, y: 50, pointsRight: true },
        1.0,
      );
      expect(points).toBe('92,46 100,50 92,54');
    });
  });

  describe('parseTipFromPoints', () => {
    test('extracts tip coordinates from valid points string', () => {
      const tip = ArcLogic.parseTipFromPoints('108,46 100,50 108,54');
      expect(tip).toEqual({ x: 100, y: 50, pointsRight: false });
    });

    test('keeps the direction of an arrow that points right', () => {
      const tip = ArcLogic.parseTipFromPoints('92,46 100,50 92,54');
      expect(tip).toEqual({ x: 100, y: 50, pointsRight: true });
      expect(ArcLogic.getArrowPoints(tip, 2.0)).toBe('84,42 100,50 84,58');
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

describe('ArcLogic.matchesActiveFilters', () => {
  test('cycle arc is covered by the cycles filter when its type filter is off', () => {
    const arcFilters = { isModuleDep: true, isCycle: true };
    const activeFilters = { moduleDep: false, cycle: true };
    expect(ArcLogic.matchesActiveFilters(arcFilters, activeFilters)).toBe(true);
  });

  test('cycle arc is covered by its type filter when the cycles filter is off', () => {
    const arcFilters = { isModuleDep: true, isCycle: true };
    const activeFilters = { moduleDep: true, cycle: false };
    expect(ArcLogic.matchesActiveFilters(arcFilters, activeFilters)).toBe(true);
  });

  test('cycle arc is covered by nothing when both filters are off', () => {
    const arcFilters = { isModuleDep: true, isCycle: true };
    const activeFilters = { moduleDep: false, cycle: false };
    expect(ArcLogic.matchesActiveFilters(arcFilters, activeFilters)).toBe(
      false,
    );
  });

  test('non-cycle arc is covered by nothing when its type filter is off, even with the cycles filter on', () => {
    const arcFilters = { isModuleDep: true, isCycle: false };
    const activeFilters = { moduleDep: false, cycle: true };
    expect(ArcLogic.matchesActiveFilters(arcFilters, activeFilters)).toBe(
      false,
    );
  });

  test('crate-dep arc follows the crate-dep filter', () => {
    const arcFilters = { isPackageDep: true };
    expect(
      ArcLogic.matchesActiveFilters(arcFilters, { packageDep: true }),
    ).toBe(true);
    expect(
      ArcLogic.matchesActiveFilters(arcFilters, { packageDep: false }),
    ).toBe(false);
  });

  test('reexport arc follows the reexport filter', () => {
    const arcFilters = { isReexport: true };
    expect(ArcLogic.matchesActiveFilters(arcFilters, { reexport: true })).toBe(
      true,
    );
    expect(ArcLogic.matchesActiveFilters(arcFilters, { reexport: false })).toBe(
      false,
    );
  });
});

describe('ArcLogic (upward arcs)', () => {
  const from = { x: 120, y: 100, width: 80, height: 20 };
  const to = { x: 100, y: 40, width: 80, height: 20 };
  const extent = { left: 100, right: 200 };

  test('an upward arc bulges to the left of its base x', () => {
    // 150 px at row height 30 are 5 rows: reach 20 + 5 * 15 = 95
    const result = ArcLogic.calculateArcPath(300, 0, 320, 150, 300, 30, true);
    expect(result.ctrlX).toBe(205);
    expect(result.path).toBe('M 300,0 Q 205,0 205,75 Q 205,150 320,150');
  });

  test('the arrow head of an upward arc points right', () => {
    const result = ArcLogic.calculateArcPath(300, 0, 320, 150, 300, 30, true);
    expect(result.tip).toEqual({ x: 320, y: 150, pointsRight: true });
  });

  test('the arrow head of a downward arc points left', () => {
    const result = ArcLogic.calculateArcPath(300, 0, 320, 150, 300, 30);
    expect(result.tip).toEqual({ x: 320, y: 150, pointsRight: false });
  });

  test('the reach of an upward arc stops at 420', () => {
    const result = ArcLogic.calculateArcPath(500, 0, 500, 3000, 500, 30, true);
    expect(result.ctrlX).toBe(80);
  });

  test('an upward arc joins the left edges of its rows', () => {
    const result = ArcLogic.calculateArcPathFromPositions(
      from,
      to,
      3,
      extent,
      30,
      true,
    );
    // From y 113 to y 47 are 2 rows: reach 50 left of the leftmost row.
    expect(result.path).toBe('M 120,113 Q 50,113 50,80 Q 50,47 100,47');
  });

  test('a downward arc joins the right edges of its rows', () => {
    const result = ArcLogic.calculateArcPathFromPositions(
      from,
      to,
      3,
      extent,
      30,
      false,
    );
    // 2 rows: 50 right of the rightmost row.
    expect(result.path).toBe('M 200,113 Q 250,113 250,80 Q 250,47 180,47');
  });
});
