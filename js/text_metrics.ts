// text_metrics.ts - Pure text width estimation without DOM
// Replaces getBBox() calls for text measurement

const TextMeasure = {
  // Monospace character widths (measured from actual font rendering)
  CHAR_WIDTH_12PX: 7.2, // Monospace 12px (node labels) - matches CHAR_WIDTH in render.rs
  CHAR_WIDTH_11PX: 6.6, // Monospace 11px (floating label)
  CHAR_WIDTH_10PX: 6.0, // Monospace 10px (arc-count)

  /**
   * Estimate text width based on character count and font size.
   * @param text - Text to measure
   * @param fontSize - Font size in pixels (default: 11)
   * @returns - Estimated width in pixels
   */
  estimateWidth(text: string, fontSize: number = 11): number {
    if (!text) return 0;
    let charWidth: number;
    switch (fontSize) {
      case 10:
        charWidth = this.CHAR_WIDTH_10PX;
        break;
      case 12:
        charWidth = this.CHAR_WIDTH_12PX;
        break;
      default:
        charWidth = this.CHAR_WIDTH_11PX;
        break;
    }
    return text.length * charWidth;
  },
};

export { TextMeasure };
