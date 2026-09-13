import { describe, expect, test } from 'bun:test';
import { ArcLogic } from './arc_logic.js';
import { jumpIdFromClick } from './svg_script.js';

describe('jumpIdFromClick', () => {
  test('returns the numeric jump id from the closest row with data-jump', () => {
    const target = {
      closest: (sel) =>
        sel.includes('.sidebar-location[data-jump]')
          ? { dataset: { jump: '5' } }
          : null,
    };
    expect(jumpIdFromClick(target)).toBe(5);
  });

  test('returns the jump id of a definition chip', () => {
    const target = {
      closest: (sel) =>
        sel.includes('.sidebar-definition[data-jump]')
          ? { dataset: { jump: '9' } }
          : null,
    };
    expect(jumpIdFromClick(target)).toBe(9);
  });

  test('returns null when no ancestor row has data-jump', () => {
    const target = { closest: () => null };
    expect(jumpIdFromClick(target)).toBeNull();
  });
});

describe('ArcLogic', () => {
  describe('getArcOffset', () => {
    test('calculates correct offset for 1 row', () => {
      expect(ArcLogic.getArcOffset(1)).toBe(35); // 20 + 1*15
    });

    test('calculates correct offset for 3 rows', () => {
      expect(ArcLogic.getArcOffset(3)).toBe(65); // 20 + 3*15
    });

    test('calculates correct offset for 0 rows', () => {
      expect(ArcLogic.getArcOffset(0)).toBe(20); // 20 + 0*15
    });

    test('calculates correct offset for 10 rows', () => {
      expect(ArcLogic.getArcOffset(10)).toBe(170); // 20 + 10*15
    });
  });

  describe('calculateArcPath', () => {
    test('returns valid SVG path with M and Q commands', () => {
      const result = ArcLogic.calculateArcPath(100, 50, 100, 150, 200, 24);
      expect(result.path).toContain('M ');
      expect(result.path).toContain('Q ');
    });

    test('returns correct toX and toY coordinates', () => {
      const result = ArcLogic.calculateArcPath(100, 50, 150, 200, 200, 24);
      expect(result.toX).toBe(150);
      expect(result.toY).toBe(200);
    });

    test('calculates midY as average of fromY and toY', () => {
      const result = ArcLogic.calculateArcPath(100, 50, 100, 150, 200, 24);
      expect(result.midY).toBe(100); // (50 + 150) / 2
    });

    test('calculates ctrlX based on maxRight and arc offset', () => {
      // 150 distance, rowHeight 24 => rowsSpanned = max(1, round(150/24)) = 6
      // arcOffset = 20 + 6*15 = 110
      // ctrlX = 200 + 110 = 310
      const result = ArcLogic.calculateArcPath(100, 0, 100, 150, 200, 24);
      expect(result.ctrlX).toBe(310);
    });

    test('minimum rowsSpanned is 1', () => {
      // Even with small distance, rowsSpanned should be at least 1
      const result = ArcLogic.calculateArcPath(100, 50, 100, 55, 200, 24);
      // rowsSpanned = max(1, round(5/24)) = max(1, 0) = 1
      // arcOffset = 20 + 1*15 = 35
      // ctrlX = 200 + 35 = 235
      expect(result.ctrlX).toBe(235);
    });

    test('path format is correct quadratic bezier', () => {
      const result = ArcLogic.calculateArcPath(100, 50, 150, 200, 200, 24);
      // Path should be: M fromX,fromY Q ctrlX,fromY ctrlX,midY Q ctrlX,toY toX,toY
      const pathRegex = /^M \d+,\d+ Q \d+,\d+ \d+,\d+ Q \d+,\d+ \d+,\d+$/;
      expect(result.path).toMatch(pathRegex);
    });
  });

  describe('estimatePathLength', () => {
    test('returns 100 for empty/null path', () => {
      expect(ArcLogic.estimatePathLength('')).toBe(100);
      expect(ArcLogic.estimatePathLength(null)).toBe(100);
      expect(ArcLogic.estimatePathLength(undefined)).toBe(100);
    });

    test('returns 100 for invalid path with insufficient coordinates', () => {
      expect(ArcLogic.estimatePathLength('M 0,0')).toBe(100);
      expect(ArcLogic.estimatePathLength('invalid')).toBe(100);
    });

    test('estimates length from valid S-curve path using bezier approximation', () => {
      // Path: M fromX,fromY Q ctrlX,fromY ctrlX,midY Q ctrlX,toY toX,toY
      // Uses quadratic bezier approximation for accurate S-curve length
      const path = 'M 100,50 Q 300,50 300,100 Q 300,150 100,150';
      // Horizontal extent 200, vertical extent 100 → bezier length ≈ 456
      expect(ArcLogic.estimatePathLength(path)).toBeCloseTo(456, 0);
    });

    test('handles large vertical distance', () => {
      // Horizontal extent 300, vertical extent 500 → bezier length ≈ 941
      const path = 'M 100,0 Q 400,0 400,250 Q 400,500 100,500';
      expect(ArcLogic.estimatePathLength(path)).toBeCloseTo(941, 0);
    });

    test('handles negative coordinates', () => {
      // Same geometry as first test, just shifted → ≈ 456
      const path = 'M 100,-50 Q 300,-50 300,0 Q 300,50 100,50';
      expect(ArcLogic.estimatePathLength(path)).toBeCloseTo(456, 0);
    });
  });
});
