// arc_logic.ts - Pure geometry functions for arcs and arrows
// No DOM dependencies

// Arrow base dimensions (at scale 1.0)
const ARROW_LENGTH = 8;
const ARROW_HALF_WIDTH = 4;
const BASE_STROKE_WIDTH = 1.5;
const STROKE_MIN = 0.5;
const STROKE_MAX = 2.5;
// The farthest an upward arc reaches to the left of the rows.
const UPWARD_ARC_MAX = 420;
// How far a straight segment may stray from the curve it replaces, in CSS px.
const FLATTEN_TOLERANCE = 0.15;
// The longest piece, in CSS px. Chromium leaves gaps in a hairline along a
// longer subpath.
const PIECE_LENGTH = 2048;

/** The outer edges of the visible rows, which the arcs bulge out from. */
type RowExtent = { left: number; right: number };

/** Where an arrow head ends. It points left unless `pointsRight` is set. */
type ArrowTip = { x: number; y: number; pointsRight?: boolean };

type ArcPath = {
  path: string;
  toX: number;
  toY: number;
  ctrlX: number;
  midY: number;
  tip: Required<ArrowTip>;
};

/**
 * One piece of an arc drawn as hairlines: the middle hairline, and the
 * others as subpaths of one path, or null when the arc is one line wide.
 */
type HairlinePiece = { middle: string; outer: string | null };

type Point = [number, number];

/** Flatten the two quadratic curves of an arc path into points, each segment within the tolerance. */
function flattenArc(path: string): Point[] {
  const [x0, y0, cx1, cy1, x1, y1, cx2, cy2, x2, y2] = (
    path.match(/-?\d+(\.\d+)?/g) ?? []
  ).map(Number);
  const points: Point[] = [[x0, y0]];
  const quadratics: [Point, Point, Point][] = [
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
  ];
  for (const [start, ctrl, end] of quadratics) {
    const bend = Math.hypot(
      start[0] - 2 * ctrl[0] + end[0],
      start[1] - 2 * ctrl[1] + end[1],
    );
    const steps = Math.max(
      1,
      Math.ceil(Math.sqrt(bend / (4 * FLATTEN_TOLERANCE))),
    );
    for (let i = 1; i <= steps; i++) {
      const t = i / steps;
      const u = 1 - t;
      points.push([
        u * u * start[0] + 2 * u * t * ctrl[0] + t * t * end[0],
        u * u * start[1] + 2 * u * t * ctrl[1] + t * t * end[1],
      ]);
    }
  }
  return points;
}

/** Split the points into index ranges [first, last], consecutive ones sharing a point. */
function pieceRanges(points: Point[]): [number, number][] {
  const ranges: [number, number][] = [];
  let first = 0;
  let length = 0;
  for (let i = 1; i < points.length; i++) {
    length += Math.hypot(
      points[i][0] - points[i - 1][0],
      points[i][1] - points[i - 1][1],
    );
    if (length >= PIECE_LENGTH && i < points.length - 1) {
      ranges.push([first, i]);
      first = i;
      length = 0;
    }
  }
  ranges.push([first, points.length - 1]);
  return ranges;
}

function round2(value: number): number {
  return Math.round(value * 100) / 100;
}

function lineData(points: Point[]): string {
  return points
    .map(([x, y], i) => `${i === 0 ? 'M' : 'L'} ${round2(x)},${round2(y)}`)
    .join(' ');
}

const ArcLogic = {
  // === Arrow geometry ===

  /**
   * Generate scaled arrow polygon points string.
   * @param tip - Arrow tip coordinates and direction
   * @param scale - Scale factor (1.0 = original size)
   * @returns - SVG polygon points string
   */
  getArrowPoints(tip: ArrowTip, scale: number): string {
    const len = ARROW_LENGTH * scale;
    const hw = ARROW_HALF_WIDTH * scale;
    const baseX = tip.pointsRight ? tip.x - len : tip.x + len;
    return `${baseX},${tip.y - hw} ${tip.x},${tip.y} ${baseX},${tip.y + hw}`;
  },

  /**
   * Parse arrow tip position and direction from SVG points string.
   * @param points - SVG polygon points (format: "x1,y1 tipX,tipY x2,y2")
   */
  parseTipFromPoints(points: string): Required<ArrowTip> | null {
    const parts = points.split(' ');
    if (parts.length >= 2) {
      const [baseX] = parts[0].split(',').map(Number);
      const [x, y] = parts[1].split(',').map(Number);
      if (Number.isNaN(x) || Number.isNaN(y)) return null;
      return { x, y, pointsRight: baseX < x };
    }
    return null;
  },

  /**
   * Calculate scale factor from stroke width.
   */
  scaleFromStrokeWidth(strokeWidth: number): number {
    return strokeWidth / BASE_STROKE_WIDTH;
  },

  /**
   * Scale existing arrows by updating their points attribute.
   * Convenience wrapper around parseTipFromPoints + getArrowPoints.
   * @param dom - DomAdapter instance
   * @param edgeId - Arc identifier
   * @param strokeWidth - Stroke width to scale to
   */
  scaleArrow(
    dom: { getVisibleArrows(arcId: string): Element[] },
    edgeId: string,
    strokeWidth: number,
  ) {
    const scale = this.scaleFromStrokeWidth(strokeWidth);
    dom.getVisibleArrows(edgeId).forEach((arrow) => {
      const points = arrow.getAttribute('points');
      if (points === null) return;
      const tip = this.parseTipFromPoints(points);
      if (tip) {
        arrow.setAttribute('points', this.getArrowPoints(tip, scale));
      }
    });
  },

  /**
   * Decide whether any active filter still covers this arc. An arc falls under
   * the filter for its type (package/module/reexport) and, if it lies on a cycle,
   * additionally under the cycles filter. The node filters are checked
   * separately, so a true answer here is a necessary and not a sufficient
   * condition for visibility. With the only-tangles filter on, an arc that is
   * not a cyclic edge is never covered.
   */
  matchesActiveFilters(
    arcFilters: {
      isPackageDep?: boolean;
      isModuleDep?: boolean;
      isReexport?: boolean;
      isCycle?: boolean;
      isCyclicEdge?: boolean;
    },
    activeFilters: {
      packageDep?: boolean;
      moduleDep?: boolean;
      reexport?: boolean;
      cycle?: boolean;
      onlyTangles?: boolean;
    },
  ): boolean {
    if (activeFilters.onlyTangles && !arcFilters.isCyclicEdge) return false;
    return Boolean(
      (arcFilters.isPackageDep && activeFilters.packageDep) ||
        (arcFilters.isModuleDep && activeFilters.moduleDep) ||
        (arcFilters.isReexport && activeFilters.reexport) ||
        (arcFilters.isCycle && activeFilters.cycle),
    );
  },

  /**
   * Whether a virtual arc stays under the only-tangles filter: at least one
   * of the arcs it aggregates is a cyclic edge.
   */
  virtualArcOnTangle(
    originalArcIds: string[],
    isCyclicEdge: (arcId: string) => boolean,
  ): boolean {
    return originalArcIds.some(isCyclicEdge);
  },

  // Expose constants for testing
  ARROW_LENGTH,
  ARROW_HALF_WIDTH,

  // === Arc geometry (from svg_script.ts ArcLogic) ===

  /**
   * Calculate arc offset based on the rows an arc spans
   * @param rowsSpanned - Number of rows between source and target
   * @returns - Offset for arc control point
   */
  getArcOffset(rowsSpanned: number): number {
    return 20 + rowsSpanned * 15;
  },

  /**
   * How far an upward arc reaches to the left of the rows.
   * @param rowsSpanned - Number of rows between source and target
   */
  getUpwardArcReach(rowsSpanned: number): number {
    return Math.min(this.getArcOffset(rowsSpanned), UPWARD_ARC_MAX);
  },

  /**
   * Calculate SVG path for an arc between two points
   * @param fromX - Start X coordinate
   * @param fromY - Start Y coordinate
   * @param toX - End X coordinate
   * @param toY - End Y coordinate
   * @param baseX - X the arc bulges out from: the rightmost edge of the
   *   visible rows, or their leftmost edge for an upward arc
   * @param rowHeight - Height of each row
   * @param upward - Whether the arc is an upward edge, drawn on the left
   */
  calculateArcPath(
    fromX: number,
    fromY: number,
    toX: number,
    toY: number,
    baseX: number,
    rowHeight: number,
    upward = false,
  ): ArcPath {
    const rowsSpanned = Math.max(
      1,
      Math.round(Math.abs(toY - fromY) / rowHeight),
    );
    const ctrlX = upward
      ? baseX - this.getUpwardArcReach(rowsSpanned)
      : baseX + this.getArcOffset(rowsSpanned);
    const midY = (fromY + toY) / 2;

    return {
      path: `M ${fromX},${fromY} Q ${ctrlX},${fromY} ${ctrlX},${midY} Q ${ctrlX},${toY} ${toX},${toY}`,
      toX,
      toY,
      ctrlX,
      midY,
      tip: { x: toX, y: toY, pointsRight: upward },
    };
  },

  /**
   * Calculate arc path from position objects (convenience wrapper).
   * Converts position rects + yOffset to raw coordinates, then delegates to calculateArcPath.
   * @param yOffset - Y offset for arc endpoints
   * @param extent - Outer edges of the visible rows
   * @param rowHeight - Row height for arc offset calculation
   * @param upward - Whether the arc is an upward edge, drawn on the left
   */
  calculateArcPathFromPositions(
    fromPos: { x: number; y: number; width: number; height: number },
    toPos: { x: number; y: number; width: number; height: number },
    yOffset: number,
    extent: RowExtent,
    rowHeight: number,
    upward = false,
  ): ArcPath {
    const fromY = fromPos.y + fromPos.height / 2 + yOffset;
    const toY = toPos.y + toPos.height / 2 - yOffset;
    if (upward) {
      return this.calculateArcPath(
        fromPos.x,
        fromY,
        toPos.x,
        toY,
        extent.left,
        rowHeight,
        true,
      );
    }
    return this.calculateArcPath(
      fromPos.x + fromPos.width,
      fromY,
      toPos.x + toPos.width,
      toY,
      extent.right,
      rowHeight,
    );
  },

  /**
   * Split an arc into pieces of straight segments, each piece drawn as
   * ⌈width · dpr⌉ parallel hairlines that together fill the arc's width.
   * @param path - Arc path as `calculateArcPath` writes it
   * @param width - Arc width in CSS px
   * @param dpr - Device pixels per CSS px
   */
  hairlinePieces(path: string, width: number, dpr: number): HairlinePiece[] {
    const points = flattenArc(path);
    const ranges = pieceRanges(points);
    // The subtraction keeps 2.5 · 1.6 = 4.000…01 at four lines.
    const n = Math.max(1, Math.ceil(width * dpr - 1e-6));
    if (n === 1) {
      return ranges.map(([first, last]) => ({
        middle: lineData(points.slice(first, last + 1)),
        outer: null,
      }));
    }
    // Normals over the whole arc, so the lines do not bend at piece boundaries.
    const normals = points.map((_, i) => {
      const a = points[Math.max(0, i - 1)];
      const b = points[Math.min(points.length - 1, i + 1)];
      const dx = b[0] - a[0];
      const dy = b[1] - a[1];
      const l = Math.hypot(dx, dy) || 1;
      return [-dy / l, dx / l];
    });
    const line = (k: number, first: number, last: number) => {
      const offset = (k - (n - 1) / 2) * (width / n);
      const shifted: Point[] = [];
      for (let i = first; i <= last; i++) {
        shifted.push([
          points[i][0] + normals[i][0] * offset,
          points[i][1] + normals[i][1] * offset,
        ]);
      }
      return lineData(shifted);
    };
    const middleLine = Math.floor(n / 2);
    return ranges.map(([first, last]) => {
      const outer = [];
      for (let k = 0; k < n; k++) {
        if (k !== middleLine) outer.push(line(k, first, last));
      }
      return { middle: line(middleLine, first, last), outer: outer.join(' ') };
    });
  },

  /**
   * Count total source locations from structured usage groups.
   */
  countLocations(
    usageGroups: Array<{
      symbol: string;
      locations: Array<{ file: string; line: number }>;
    }>,
  ): number {
    if (!usageGroups || !Array.isArray(usageGroups)) return 0;
    return usageGroups.reduce((sum, g) => sum + g.locations.length, 0);
  },

  /**
   * Calculate stroke width based on location count using logarithmic scaling.
   * @param locationCount - Number of source locations
   * @returns - Stroke width in pixels (0.5 to 2.5)
   */
  calculateStrokeWidth(locationCount: number): number {
    const CAP = 50;
    if (locationCount <= 0) return STROKE_MIN;
    const count = Math.min(locationCount, CAP);
    return (
      STROKE_MIN + ((STROKE_MAX - STROKE_MIN) * Math.log(count)) / Math.log(CAP)
    );
  },

  /**
   * Calculate stroke width from traffic: linear from the minimum width at
   * traffic 1 to the maximum width at the largest traffic of the diagram.
   * With a largest traffic of 1 every arc gets the minimum.
   * @param traffic - Counted cycles through the edge
   * @param maxTraffic - Largest traffic among the arcs of the diagram
   * @returns - Stroke width in pixels (0.5 to 2.5)
   */
  calculateTrafficStrokeWidth(traffic: number, maxTraffic: number): number {
    if (maxTraffic <= 1 || traffic <= 1) return STROKE_MIN;
    const share = Math.min(traffic, maxTraffic) - 1;
    return STROKE_MIN + ((STROKE_MAX - STROKE_MIN) * share) / (maxTraffic - 1);
  },

  /**
   * Estimate path length from SVG path string (no DOM read).
   * Parses S-curve path format: M fromX,fromY Q ctrlX,fromY ctrlX,midY Q ctrlX,toY toX,toY
   * Uses quadratic bezier approximation for each segment.
   * @param pathD - SVG path 'd' attribute value
   * @returns - Estimated path length in pixels
   */
  estimatePathLength(pathD: string): number {
    if (!pathD) return 100;

    // Parse: M fromX,fromY Q ctrlX,fromY ctrlX,midY Q ctrlX,toY toX,toY
    const coords = pathD.match(/-?\d+\.?\d*/g);
    if (!coords || coords.length < 10) return 100;

    const fromX = parseFloat(coords[0]);
    const fromY = parseFloat(coords[1]);
    const ctrlX = parseFloat(coords[2]);
    // coords[3] = fromY (control point 1 Y)
    // coords[4] = ctrlX (redundant)
    const midY = parseFloat(coords[5]);
    // coords[6] = ctrlX (redundant)
    // coords[7] = toY (control point 2 Y)
    const toX = parseFloat(coords[8]);
    const toY = parseFloat(coords[9]);

    // Quadratic bezier approximation: (|P0-P1| + |P1-P2| + |P0-P2|) / 2
    // First segment: (fromX,fromY) -> control(ctrlX,fromY) -> (ctrlX,midY)
    const d1_01 = Math.hypot(ctrlX - fromX, 0); // fromY to fromY = 0
    const d1_12 = Math.hypot(0, midY - fromY); // ctrlX to ctrlX = 0
    const d1_02 = Math.hypot(ctrlX - fromX, midY - fromY);
    const len1 = (d1_01 + d1_12 + d1_02) / 2;

    // Second segment: (ctrlX,midY) -> control(ctrlX,toY) -> (toX,toY)
    const d2_01 = Math.hypot(0, toY - midY); // ctrlX to ctrlX = 0
    const d2_12 = Math.hypot(toX - ctrlX, 0); // toY to toY = 0
    const d2_02 = Math.hypot(toX - ctrlX, toY - midY);
    const len2 = (d2_01 + d2_12 + d2_02) / 2;

    return len1 + len2;
  },
};

export { ArcLogic, type HairlinePiece, type RowExtent };
