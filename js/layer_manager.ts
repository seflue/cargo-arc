const LAYER_TABLE = [
  {
    check: (el, cls) =>
      el.classList?.contains(cls.direction.depArc) ||
      el.classList?.contains(cls.direction.cycleArc) ||
      el.classList?.contains(cls.direction.virtualArc) ||
      el.tagName === 'polygon',
    base: 'BASE_ARCS',
    highlight: 'HIGHLIGHT_ARCS',
  },
  {
    check: (el, cls) => el.classList?.contains(cls.labels.arcCountGroup),
    base: 'BASE_LABELS',
    highlight: 'HIGHLIGHT_LABELS',
  },
  {
    check: (el, cls) => el.classList?.contains(cls.direction.arcHitarea),
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
   * @param element - DOM element to check
   * @param highlighted - Whether to return highlight or base layer
   * @returns - Layer ID or null if element type unknown
   */
  getLayerForElement(
    element: Element | null,
    highlighted: boolean,
  ): string | null {
    if (!element) return null;

    const cls = STATIC_DATA.classes;
    const entry = LAYER_TABLE.find(({ check }) => check(element, cls));
    return entry
      ? this.LAYERS[highlighted ? entry.highlight : entry.base]
      : null;
  },

  /**
   * Move element to a specific layer
   * @param element - Element to move
   * @param layerId - Target layer ID
   * @param domAdapter - DomAdapter instance
   */
  moveToLayer(element: Element | null, layerId: string, domAdapter: any) {
    if (element && layerId) {
      domAdapter.getElementById(layerId)?.appendChild(element);
    }
  },

  /**
   * Clear all children from a layer
   * @param layerId - Layer ID to clear
   * @param domAdapter - DomAdapter instance
   */
  clearLayer(layerId: string, domAdapter: any) {
    const layer = domAdapter.getElementById(layerId);
    if (layer) {
      while (layer.firstChild) layer.removeChild(layer.firstChild);
    }
  },

  /**
   * Move element to its appropriate highlight layer
   * @param element - Element to move
   * @param domAdapter - DomAdapter instance
   */
  moveToHighlightLayer(element: Element | null, domAdapter: any) {
    const layerId = this.getLayerForElement(element, true);
    if (layerId) this.moveToLayer(element, layerId, domAdapter);
  },
};

export { LayerManager };
