// @module ArcLogic
// @deps
// @config
// arc_logic.js - Pure geometry functions for arcs and arrows
// No DOM dependencies

// Arrow base dimensions (at scale 1.0)
const ARROW_LENGTH = 8;
const ARROW_HALF_WIDTH = 4;

const ArcLogic = {
  // === Arrow geometry ===

  /**
   * Generate scaled arrow polygon points string.
   * Arrow tip is at tip position, pointing left.
   * @param {{x: number, y: number}} tip - Arrow tip coordinates
   * @param {number} scale - Scale factor (1.0 = original size)
   * @returns {string} - SVG polygon points string
   */
  getArrowPoints(tip, scale) {
    const len = ARROW_LENGTH * scale;
    const hw = ARROW_HALF_WIDTH * scale;
    return `${tip.x + len},${tip.y - hw} ${tip.x},${tip.y} ${tip.x + len},${tip.y + hw}`;
  },

  /**
   * Parse arrow tip position from SVG points string.
   * @param {string} points - SVG polygon points (format: "x1,y1 tipX,tipY x2,y2")
   * @returns {{x: number, y: number}|null}
   */
  parseTipFromPoints(points) {
    const parts = points.split(' ');
    if (parts.length >= 2) {
      const [x, y] = parts[1].split(',').map(Number);
      if (isNaN(x) || isNaN(y)) return null;
      return { x, y };
    }
    return null;
  },

  /**
   * Calculate scale factor from stroke width.
   * @param {number} strokeWidth
   * @returns {number}
   */
  scaleFromStrokeWidth(strokeWidth) {
    return strokeWidth / 1.5; // 1.5 was the original base stroke-width
  },

  // Expose constants for testing
  ARROW_LENGTH,
  ARROW_HALF_WIDTH,

  // === Arc geometry (from svg_script.js ArcLogic) ===

  /**
   * Calculate arc offset based on number of hops between nodes
   * @param {number} hops - Number of row hops between source and target
   * @returns {number} - Offset for arc control point
   */
  getArcOffset(hops) {
    return 20 + (hops * 15);
  },

  /**
   * Transform mouse event coordinates to SVG coordinates (scroll-aware)
   * Uses getBoundingClientRect() instead of getScreenCTM() to handle scrollable containers.
   * See: WebKit Bug #44083, D3.js Issue #1164
   * @param {number} clientX - Mouse clientX from event
   * @param {number} clientY - Mouse clientY from event
   * @param {{left: number, top: number, width: number, height: number}} svgRect - SVG bounding rect
   * @param {{x: number, y: number, width: number, height: number}|null} viewBox - SVG viewBox or null
   * @returns {{x: number, y: number}} - Coordinates in SVG coordinate space
   */
  getSvgCoords(clientX, clientY, svgRect, viewBox) {
    let x = clientX - svgRect.left;
    let y = clientY - svgRect.top;

    if (viewBox && viewBox.width > 0) {
      x = x * (viewBox.width / svgRect.width) + viewBox.x;
      y = y * (viewBox.height / svgRect.height) + viewBox.y;
    }

    return { x, y };
  },

  /**
   * Calculate SVG path for an arc between two points
   * @param {number} fromX - Start X coordinate
   * @param {number} fromY - Start Y coordinate
   * @param {number} toX - End X coordinate
   * @param {number} toY - End Y coordinate
   * @param {number} maxRight - Rightmost X coordinate of visible nodes
   * @param {number} rowHeight - Height of each row
   * @returns {{path: string, toX: number, toY: number, ctrlX: number, midY: number}}
   */
  calculateArcPath(fromX, fromY, toX, toY, maxRight, rowHeight) {
    const hops = Math.max(1, Math.round(Math.abs(toY - fromY) / rowHeight));
    const arcOffset = this.getArcOffset(hops);
    const ctrlX = maxRight + arcOffset;
    const midY = (fromY + toY) / 2;

    return {
      path: `M ${fromX},${fromY} Q ${ctrlX},${fromY} ${ctrlX},${midY} Q ${ctrlX},${toY} ${toX},${toY}`,
      toX,
      toY,
      ctrlX,
      midY
    };
  },

  /**
   * Count the number of source locations in a pipe-separated string.
   * Format: "Symbol  <- file:line|     <- file:line|Symbol2  <- file:line"
   * Each pipe-separated segment represents one location.
   * @param {string} locationsString - Pipe-separated location string
   * @returns {number} - Number of locations
   */
  countLocations(locationsString) {
    if (!locationsString) return 0;
    return locationsString.split('|').length;
  },

  /**
   * Calculate stroke width based on location count using logarithmic scaling.
   * @param {number} locationCount - Number of source locations
   * @returns {number} - Stroke width in pixels (0.5 to 2.5)
   */
  calculateStrokeWidth(locationCount) {
    const MIN = 0.5, MAX = 2.5, CAP = 50;
    if (locationCount <= 0) return MIN;
    const count = Math.min(locationCount, CAP);
    return MIN + (MAX - MIN) * Math.log(count) / Math.log(CAP);
  },

  /**
   * Estimate path length from SVG path string (no DOM read).
   * Parses S-curve path format: M fromX,fromY Q ctrlX,fromY ctrlX,midY Q ctrlX,toY toX,toY
   * Uses quadratic bezier approximation for each segment.
   * @param {string} pathD - SVG path 'd' attribute value
   * @returns {number} - Estimated path length in pixels
   */
  estimatePathLength(pathD) {
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
    const d1_01 = Math.hypot(ctrlX - fromX, 0);  // fromY to fromY = 0
    const d1_12 = Math.hypot(0, midY - fromY);    // ctrlX to ctrlX = 0
    const d1_02 = Math.hypot(ctrlX - fromX, midY - fromY);
    const len1 = (d1_01 + d1_12 + d1_02) / 2;

    // Second segment: (ctrlX,midY) -> control(ctrlX,toY) -> (toX,toY)
    const d2_01 = Math.hypot(0, toY - midY);      // ctrlX to ctrlX = 0
    const d2_12 = Math.hypot(toX - ctrlX, 0);     // toY to toY = 0
    const d2_02 = Math.hypot(toX - ctrlX, toY - midY);
    const len2 = (d2_01 + d2_12 + d2_02) / 2;

    return len1 + len2;
  },

  /**
   * Sort and group tooltip location strings by symbol name.
   * Re-sorts aggregated tooltip data for virtual arcs to ensure consistent display.
   *
   * Input format: Array of pipe-separated location strings, each containing
   * entries like "Symbol  <- file:line" or bare "file:line"
   *
   * @param {string[]} locStrings - Array of tooltip location strings
   * @returns {string} - Sorted and grouped locations joined by '|'
   */
  sortAndGroupLocations(locStrings) {
    const symbolRegex = /^(\S+)\s+←\s+(.+)$/;
    const bySymbol = {};  // symbol -> [locations]
    const bareLocations = [];

    // Parse all location entries
    for (const str of locStrings) {
      for (const entry of str.split('|')) {
        const trimmed = entry.trim();
        if (!trimmed) continue;

        const match = trimmed.match(symbolRegex);
        if (match) {
          const symbol = match[1];
          const location = match[2];
          if (!bySymbol[symbol]) bySymbol[symbol] = [];
          bySymbol[symbol].push(location);
        } else {
          // Bare location (no symbol prefix)
          bareLocations.push(trimmed);
        }
      }
    }

    // Sort symbols alphabetically
    const sortedSymbols = Object.keys(bySymbol).sort();

    // Sort locations within each symbol
    for (const symbol of sortedSymbols) {
      bySymbol[symbol].sort();
    }
    bareLocations.sort();

    // Find max symbol length for column alignment
    const maxLen = sortedSymbols.reduce((max, s) => Math.max(max, s.length), 0);

    // Build output
    const lines = [];

    // Bare locations first
    for (const loc of bareLocations) {
      lines.push(loc);
    }

    // Symbol-grouped locations with alignment
    for (const symbol of sortedSymbols) {
      const locs = bySymbol[symbol];
      for (let i = 0; i < locs.length; i++) {
        if (i === 0) {
          const padding = ' '.repeat(maxLen - symbol.length + 2);
          lines.push(`${symbol}${padding}← ${locs[i]}`);
        } else {
          const spaces = ' '.repeat(maxLen + 2);
          lines.push(`${spaces}← ${locs[i]}`);
        }
      }
    }

    return lines.join('|');
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { ArcLogic };
}
