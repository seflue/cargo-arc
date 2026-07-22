// @module LayerManager
// @deps
// @config
const LAYER_TABLE = [
  {
    check: (el, cls) =>
      el.classList?.contains(cls.depArc) ||
      el.classList?.contains(cls.cycleArc) ||
      el.classList?.contains(cls.virtualArc) ||
      el.tagName === 'polygon',
    base: 'BASE_ARCS',
    highlight: 'HIGHLIGHT_ARCS',
  },
  {
    check: (el, cls) => el.classList?.contains(cls.arcCountGroup),
    base: 'BASE_LABELS',
    highlight: 'HIGHLIGHT_LABELS',
  },
  {
    check: (el, cls) => el.classList?.contains(cls.arcHitarea),
    base: 'HITAREAS',
    highlight: 'HIGHLIGHT_HITAREAS',
  },
];

/**
 * LayerManager - SVG layer management for highlight/base layer switching
 * Pure functions where possible, DOM operations via DomAdapter
 */
const LayerManager = {
  LAYERS: {
    BASE_ARCS: 'base-arcs-layer',
    BASE_LABELS: 'base-labels-layer',
    HIGHLIGHT_ARCS: 'highlight-arcs-layer',
    HIGHLIGHT_LABELS: 'highlight-labels-layer',
    HITAREAS: 'hitareas-layer',
    HIGHLIGHT_HITAREAS: 'highlight-hitareas-layer',
    SHADOWS: 'highlight-shadows',
  },

  /**
   * Determine which layer an element belongs to based on its type
   * @param {Element|null} element - DOM element to check
   * @param {boolean} highlighted - Whether to return highlight or base layer
   * @returns {string|null} - Layer ID or null if element type unknown
   */
  getLayerForElement(element, highlighted) {
    if (!element) return null;

    const cls = STATIC_DATA.classes;
    const entry = LAYER_TABLE.find(({ check }) => check(element, cls));
    return entry
      ? this.LAYERS[highlighted ? entry.highlight : entry.base]
      : null;
  },

  /**
   * Move element to a specific layer
   * @param {Element|null} element - Element to move
   * @param {string} layerId - Target layer ID
   * @param {Object} domAdapter - DomAdapter instance
   */
  moveToLayer(element, layerId, domAdapter) {
    if (element && layerId) {
      domAdapter.getElementById(layerId)?.appendChild(element);
    }
  },

  /**
   * Clear all children from a layer
   * @param {string} layerId - Layer ID to clear
   * @param {Object} domAdapter - DomAdapter instance
   */
  clearLayer(layerId, domAdapter) {
    const layer = domAdapter.getElementById(layerId);
    if (layer) {
      while (layer.firstChild) layer.removeChild(layer.firstChild);
    }
  },

  /**
   * Move element to its appropriate highlight layer
   * @param {Element|null} element - Element to move
   * @param {Object} domAdapter - DomAdapter instance
   */
  moveToHighlightLayer(element, domAdapter) {
    const layerId = this.getLayerForElement(element, true);
    if (layerId) this.moveToLayer(element, layerId, domAdapter);
  },
};

// Export for Bun/Node
if (typeof module !== 'undefined' && module.exports) {
  module.exports = { LayerManager };
}
