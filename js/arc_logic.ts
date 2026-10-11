// arc_logic.ts - Pure geometry functions for arcs and arrows
// No DOM dependencies

// Arrow base dimensions (at scale 1.0)
const ARROW_LENGTH = 8;
const ARROW_HALF_WIDTH = 4;
const BASE_STROKE_WIDTH = 1.5;

const ArcLogic = {
  // === Arrow geometry ===

  /**
   * Generate scaled arrow polygon points string.
   * Arrow tip is at tip position, pointing left.
   * @param tip - Arrow tip coordinates
   * @param scale - Scale factor (1.0 = original size)
   * @returns - SVG polygon points string
   */
  getArrowPoints(tip: { x: number; y: number }, scale: number): string {
    const len = ARROW_LENGTH * scale;
    const hw = ARROW_HALF_WIDTH * scale;
    return `${tip.x + len},${tip.y - hw} ${tip.x},${tip.y} ${tip.x + len},${tip.y + hw}`;
  },

  /**
   * Parse arrow tip position from SVG points string.
   * @param points - SVG polygon points (format: "x1,y1 tipX,tipY x2,y2")
   */
  parseTipFromPoints(points: string): { x: number; y: number } | null {
    const parts = points.split(' ');
    if (parts.length >= 2) {
      const [x, y] = parts[1].split(',').map(Number);
      if (Number.isNaN(x) || Number.isNaN(y)) return null;
      return { x, y };
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
   * condition for visibility.
   */
  matchesActiveFilters(
    arcFilters: {
      isPackageDep?: boolean;
      isModuleDep?: boolean;
      isReexport?: boolean;
      isCycle?: boolean;
    },
    activeFilters: {
      packageDep?: boolean;
      moduleDep?: boolean;
      reexport?: boolean;
      cycle?: boolean;
    },
  ): boolean {
    return Boolean(
      (arcFilters.isPackageDep && activeFilters.packageDep) ||
        (arcFilters.isModuleDep && activeFilters.moduleDep) ||
        (arcFilters.isReexport && activeFilters.reexport) ||
        (arcFilters.isCycle && activeFilters.cycle),
    );
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
   * Calculate SVG path for an arc between two points
   * @param fromX - Start X coordinate
   * @param fromY - Start Y coordinate
   * @param toX - End X coordinate
   * @param toY - End Y coordinate
   * @param maxRight - Rightmost X coordinate of visible nodes
   * @param rowHeight - Height of each row
   */
  calculateArcPath(
    fromX: number,
    fromY: number,
    toX: number,
    toY: number,
    maxRight: number,
    rowHeight: number,
  ): { path: string; toX: number; toY: number; ctrlX: number; midY: number } {
    const rowsSpanned = Math.max(
      1,
      Math.round(Math.abs(toY - fromY) / rowHeight),
    );
    const arcOffset = this.getArcOffset(rowsSpanned);
    const ctrlX = maxRight + arcOffset;
    const midY = (fromY + toY) / 2;

    return {
      path: `M ${fromX},${fromY} Q ${ctrlX},${fromY} ${ctrlX},${midY} Q ${ctrlX},${toY} ${toX},${toY}`,
      toX,
      toY,
      ctrlX,
      midY,
    };
  },

  /**
   * Calculate arc path from position objects (convenience wrapper).
   * Converts position rects + yOffset to raw coordinates, then delegates to calculateArcPath.
   * @param yOffset - Y offset for arc endpoints
   * @param maxRight - Rightmost X coordinate
   * @param rowHeight - Row height for arc offset calculation
   */
  calculateArcPathFromPositions(
    fromPos: { x: number; y: number; width: number; height: number },
    toPos: { x: number; y: number; width: number; height: number },
    yOffset: number,
    maxRight: number,
    rowHeight: number,
  ): { path: string; toX: number; toY: number; ctrlX: number; midY: number } {
    const fromX = fromPos.x + fromPos.width;
    const fromY = fromPos.y + fromPos.height / 2 + yOffset;
    const toX = toPos.x + toPos.width;
    const toY = toPos.y + toPos.height / 2 - yOffset;
    return this.calculateArcPath(fromX, fromY, toX, toY, maxRight, rowHeight);
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
    const MIN = 0.5,
      MAX = 2.5,
      CAP = 50;
    if (locationCount <= 0) return MIN;
    const count = Math.min(locationCount, CAP);
    return MIN + ((MAX - MIN) * Math.log(count)) / Math.log(CAP);
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

export { ArcLogic };
