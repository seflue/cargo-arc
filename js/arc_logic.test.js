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

/** The subpaths of `M x,y L x,y … M …` path data, each a list of points. */
function subpaths(d) {
  return d
    .split('M')
    .filter((s) => s.trim())
    .map((s) =>
      s
        .split('L')
        .map((p) => p.trim().split(',').map(Number))
        .map(([x, y]) => [x, y]),
    );
}

function length(points) {
  let sum = 0;
  for (let i = 1; i < points.length; i++) {
    sum += Math.hypot(
      points[i][0] - points[i - 1][0],
      points[i][1] - points[i - 1][1],
    );
  }
  return sum;
}

/** Distance from `p` to the nearest of many points sampled on both curves. */
function distanceToCurve(p, curve) {
  const [x0, y0, cx1, cy1, x1, y1, cx2, cy2, x2, y2] = curve;
  let best = Infinity;
  for (const [a, c, b] of [
    [
      [x0, y0],
      [cx1, cy1],
      [x1, y1],
    ],
    [
      [x1, y1],
      [cx2, cy2],
      [x2, y2],
    ],
  ]) {
    for (let i = 0; i <= 20000; i++) {
      const t = i / 20000;
      const u = 1 - t;
      const x = u * u * a[0] + 2 * u * t * c[0] + t * t * b[0];
      const y = u * u * a[1] + 2 * u * t * c[1] + t * t * b[1];
      best = Math.min(best, Math.hypot(x - p[0], y - p[1]));
    }
  }
  return best;
}

describe('ArcLogic.hairlinePieces', () => {
  // A downward arc over about 200 rows: two quadratic curves of 2,500 px each.
  const curve = [100, 0, 300, 0, 300, 2500, 300, 5000, 100, 5000];
  const longArc = 'M 100,0 Q 300,0 300,2500 Q 300,5000 100,5000';
  // A straight vertical arc, so every normal points along x.
  const straightArc = 'M 0,0 Q 0,0 0,50 Q 0,100 0,100';

  const lineCount = (piece) =>
    1 + (piece.outer === null ? 0 : subpaths(piece.outer).length);

  test.each([
    [0.5, 1.6, 1],
    [1.0, 1.6, 2],
    [2.5, 1.6, 4],
    [2.5, 1, 3],
    [0.5, 2, 1],
  ])('a %f px arc at density %f is drawn as %i hairlines', (width, dpr, n) => {
    const pieces = ArcLogic.hairlinePieces(longArc, width, dpr);
    for (const piece of pieces) expect(lineCount(piece)).toBe(n);
  });

  test('a long arc is split into pieces that share their boundary points', () => {
    const pieces = ArcLogic.hairlinePieces(longArc, 0.5, 1);
    expect(pieces.length).toBeGreaterThanOrEqual(3);
    const lines = pieces.map((piece) => subpaths(piece.middle)[0]);
    for (const line of lines) {
      let longestSegment = 0;
      for (let i = 1; i < line.length; i++) {
        longestSegment = Math.max(
          longestSegment,
          length(line.slice(i - 1, i + 1)),
        );
      }
      expect(length(line)).toBeLessThanOrEqual(2048 + longestSegment);
    }
    for (let i = 1; i < lines.length; i++) {
      expect(lines[i][0]).toEqual(lines[i - 1].at(-1));
    }
    expect(lines[0][0]).toEqual([100, 0]);
    expect(lines.at(-1).at(-1)).toEqual([100, 5000]);
  });

  test('a single hairline stays within 0.15 px of the curve', () => {
    const pieces = ArcLogic.hairlinePieces(longArc, 0.5, 1);
    for (const piece of pieces) {
      expect(piece.outer).toBeNull();
      const line = subpaths(piece.middle)[0];
      for (let i = 1; i < line.length; i++) {
        const [a, b] = [line[i - 1], line[i]];
        const middle = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
        // 0.01 for the rounding of the coordinates.
        expect(distanceToCurve(middle, curve)).toBeLessThanOrEqual(0.16);
      }
    }
  });

  test('several hairlines lie symmetric around the curve and fill its width', () => {
    // 2.5 px at density 1.6 are 4 hairlines 0.625 px apart.
    const [piece] = ArcLogic.hairlinePieces(straightArc, 2.5, 1.6);
    const xOf = (line) => {
      const xs = new Set(line.map(([x]) => x));
      expect(xs.size).toBe(1);
      return [...xs][0];
    };
    const middle = subpaths(piece.middle);
    const outer = subpaths(piece.outer);
    expect(middle).toHaveLength(1);
    expect(outer).toHaveLength(3);
    const xs = [...middle, ...outer].map(xOf).sort((a, b) => a - b);
    // ±(1.25 - 0.3125) and ±0.3125, rounded to two places.
    expect(xs).toEqual([-0.94, -0.31, 0.31, 0.94]);
  });

  test('coordinates carry at most two decimal places', () => {
    const pieces = ArcLogic.hairlinePieces(longArc, 2.5, 1.6);
    for (const piece of pieces) {
      const numbers = `${piece.middle} ${piece.outer}`.match(/-?[\d.]+/g);
      for (const n of numbers) expect(n).toMatch(/^-?\d+(\.\d{1,2})?$/);
    }
  });
});
