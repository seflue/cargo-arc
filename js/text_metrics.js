// @module TextMetrics
// @deps
// @config
// text_metrics.js - Pure text width estimation without DOM
// Replaces getBBox() calls for text measurement

const TextMetrics = {
  // Monospace character widths (measured from actual font rendering)
  CHAR_WIDTH_12PX: 7.2,  // Monospace 12px (node labels) - matches CHAR_WIDTH in render.rs
  CHAR_WIDTH_11PX: 6.6,  // Monospace 11px (floating label)
  CHAR_WIDTH_10PX: 6.0,  // Monospace 10px (arc-count)

  /**
   * Estimate text width based on character count and font size.
   * @param {string} text - Text to measure
   * @param {number} fontSize - Font size in pixels (default: 11)
   * @returns {number} - Estimated width in pixels
   */
  estimateWidth(text, fontSize = 11) {
    if (!text) return 0;
    let charWidth;
    switch (fontSize) {
      case 10: charWidth = this.CHAR_WIDTH_10PX; break;
      case 12: charWidth = this.CHAR_WIDTH_12PX; break;
      default: charWidth = this.CHAR_WIDTH_11PX; break;
    }
    return text.length * charWidth;
  },

  /**
   * Estimate width for multi-line text (returns max line width).
   * @param {string} text - Text with | as line separator
   * @param {number} fontSize - Font size in pixels (default: 11)
   * @returns {number} - Estimated width of widest line
   */
  estimateMultilineWidth(text, fontSize = 11) {
    if (!text) return 0;
    const lines = text.split('|');
    return Math.max(...lines.map(line => this.estimateWidth(line, fontSize)));
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { TextMetrics };
}

// Browser export
if (typeof window !== 'undefined') {
  window.TextMetrics = TextMetrics;
}
