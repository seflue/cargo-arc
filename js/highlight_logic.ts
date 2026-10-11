// highlight_logic.ts - Pure calculation functions for highlight effects
// No DOM dependencies

const HighlightLogic = {
  // Constants
  HIGHLIGHT_SCALE: 1.3,
  SHADOW_MULTIPLIER: 4,

  /**
   * Calculate highlighted stroke width from base width.
   * @param base - Original stroke width
   * @returns - Highlighted stroke width
   */
  calculateHighlightWidth(base: number): number {
    return base * this.HIGHLIGHT_SCALE;
  },

  /**
   * Calculate shadow stroke width from arc width.
   * @param arcWidth - Arc stroke width
   * @returns - Shadow stroke width (4× arc)
   */
  calculateShadowWidth(arcWidth: number): number {
    return arcWidth * this.SHADOW_MULTIPLIER;
  },

  /**
   * Calculate shadow overhang per end.
   * The shadow is wider than the arc, so it extends beyond arc endpoints.
   * @param shadowWidth - Shadow stroke width
   * @param arcWidth - Arc stroke width
   * @returns - Overhang distance per side
   */
  calculateShadowOverhang(shadowWidth: number, arcWidth: number): number {
    return (shadowWidth - arcWidth) / 2;
  },

  /**
   * Calculate visible shadow length after compensating for overhang.
   * @param pathLength - Total path length
   * @param overhang - Overhang per side
   * @returns - Visible length (minimum 0)
   */
  calculateVisibleLength(pathLength: number, overhang: number): number {
    return Math.max(0, pathLength - overhang * 2);
  },

  /**
   * Calculate dash offset to shorten shadow from both ends.
   * @param overhang - Overhang per side
   * @returns - Negative overhang value (or 0 for zero input)
   */
  calculateDashOffset(overhang: number): number {
    return -overhang;
  },

  /**
   * @param originalWidth - Original arc stroke width (not highlighted)
   * @param pathLength - Total path length
   */
  calculateShadowData(
    originalWidth: number,
    pathLength: number,
  ): {
    shadowWidth: number;
    overhang: number;
    visibleLength: number;
    dashOffset: number;
  } {
    const shadowWidth = this.calculateShadowWidth(originalWidth);
    const overhang = this.calculateShadowOverhang(shadowWidth, originalWidth);
    const visibleLength = this.calculateVisibleLength(pathLength, overhang);
    const dashOffset = this.calculateDashOffset(overhang);

    return { shadowWidth, overhang, visibleLength, dashOffset };
  },
};

export { HighlightLogic };
