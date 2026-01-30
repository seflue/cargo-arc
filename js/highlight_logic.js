// @module HighlightLogic
// @deps ArcLogic
// @config
// highlight_logic.js - Pure calculation functions for highlight effects
// No DOM dependencies
// Assumes ArcLogic is available globally (loaded before this in browser, or via test setup)

const HighlightLogic = {
  // Constants
  HIGHLIGHT_SCALE: 1.3,
  SHADOW_MULTIPLIER: 4,

  /**
   * Calculate highlighted stroke width from base width.
   * @param {number} base - Original stroke width
   * @returns {number} - Highlighted stroke width
   */
  calculateHighlightWidth(base) {
    return base * this.HIGHLIGHT_SCALE;
  },

  /**
   * Calculate shadow stroke width from arc width.
   * @param {number} arcWidth - Arc stroke width
   * @returns {number} - Shadow stroke width (4× arc)
   */
  calculateShadowWidth(arcWidth) {
    return arcWidth * this.SHADOW_MULTIPLIER;
  },

  /**
   * Calculate shadow overhang per end.
   * The shadow is wider than the arc, so it extends beyond arc endpoints.
   * @param {number} shadowWidth - Shadow stroke width
   * @param {number} arcWidth - Arc stroke width
   * @returns {number} - Overhang distance per side
   */
  calculateShadowOverhang(shadowWidth, arcWidth) {
    return (shadowWidth - arcWidth) / 2;
  },

  /**
   * Calculate visible shadow length after compensating for overhang.
   * @param {number} pathLength - Total path length
   * @param {number} overhang - Overhang per side
   * @returns {number} - Visible length (minimum 0)
   */
  calculateVisibleLength(pathLength, overhang) {
    return Math.max(0, pathLength - overhang * 2);
  },

  /**
   * Calculate dash offset to shorten shadow from both ends.
   * @param {number} overhang - Overhang per side
   * @returns {number} - Negative overhang value (or 0 for zero input)
   */
  calculateDashOffset(overhang) {
    return overhang === 0 ? 0 : -overhang;
  },

  /**
   * Determine relation type for highlight coloring.
   * @param {string} fromId - Source node ID
   * @param {string} toId - Target node ID
   * @param {string} selectedId - Currently selected node ID
   * @returns {'dep'|'reverse'} - 'dep' if outgoing (from=selected), 'reverse' if incoming
   */
  determineRelationType(fromId, toId, selectedId) {
    return fromId === selectedId ? 'dep' : 'reverse';
  },

  /**
   * Calculate arrow scale for virtual arrows based on stroke width.
   * Virtual arrows use stroke-based scaling (strokeWidth / 1.5).
   * @param {number} strokeWidth - Current stroke width
   * @returns {number} - Arrow scale factor
   */
  calculateVirtualArrowScale(strokeWidth) {
    return ArcLogic.scaleFromStrokeWidth(strokeWidth);
  },

  /**
   * Calculate all shadow-related data in one call.
   * @param {number} originalWidth - Original arc stroke width (not highlighted)
   * @param {number} pathLength - Total path length from getTotalLength()
   * @returns {{shadowWidth: number, overhang: number, visibleLength: number, dashOffset: number}}
   */
  calculateShadowData(originalWidth, pathLength) {
    const shadowWidth = this.calculateShadowWidth(originalWidth);
    const overhang = this.calculateShadowOverhang(shadowWidth, originalWidth);
    const visibleLength = this.calculateVisibleLength(pathLength, overhang);
    const dashOffset = this.calculateDashOffset(overhang);

    return { shadowWidth, overhang, visibleLength, dashOffset };
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { HighlightLogic };
}
