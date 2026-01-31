// @module Selectors
// @deps
// @config
/**
 * Selectors - CSS selector generators for SVG elements
 * Pure functions, no DOM access - testable in isolation
 */
const Selectors = {
  // IDs
  nodeId: (id) => `node-${id}`,
  countId: (id) => `count-${id}`,

  // CSS Selectors
  visibleArc: (arcId) => { const c = STATIC_DATA.classes; return `.${c.depArc}[data-arc-id="${arcId}"], .${c.cycleArc}[data-arc-id="${arcId}"]`; },
  hitarea: (arcId) => `.${STATIC_DATA.classes.arcHitarea}[data-arc-id="${arcId}"]`,
  arrows: (arcId) => `[data-edge="${arcId}"]`,
  virtualArrows: (arcId) => `[data-vedge="${arcId}"]:not(.${STATIC_DATA.classes.arcCount})`,
  virtualArc: (from, to) => `.${STATIC_DATA.classes.virtualArc}[data-from="${from}"][data-to="${to}"]`,
  connectedHitareas: (nodeId) => { const c = STATIC_DATA.classes; return `.${c.arcHitarea}[data-from="${nodeId}"], .${c.arcHitarea}[data-to="${nodeId}"]`; },
  labelGroup: (arcId) => `.${STATIC_DATA.classes.arcCountGroup}[data-vedge="${arcId}"]`,

  // Identity selectors (parametrized)
  collapseToggle: (nodeId) => `.${STATIC_DATA.classes.collapseToggle}[data-target="${nodeId}"]`,
  treeLineChild: (nodeId) => `line[data-child="${nodeId}"]`,
  treeLineParent: (nodeId) => `line[data-parent="${nodeId}"]`,

  // Category selectors (batch operations)
  allHitareas: () => `.${STATIC_DATA.classes.arcHitarea}`,
  allVirtualElements: () => { const c = STATIC_DATA.classes; return `.${c.virtualArc}, .${c.virtualHitarea}, .${c.virtualArrow}, .${c.arcCount}, .${c.arcCountGroup}, .${c.arcCountBg}`; },
  allBaseEdges: () => { const c = STATIC_DATA.classes; return `.${c.arcHitarea}, .${c.depArc}, .${c.cycleArc}`; },
  allBaseArrows: () => { const c = STATIC_DATA.classes; return `.${c.depArrow}, .${c.cycleArrow}`; },

  // Layer selectors (for clearHighlights)
  highlightedArcs: () => '#highlight-arcs-layer > *',
  highlightedLabels: () => '#highlight-labels-layer > *',
  highlightedHitareas: () => '#highlight-hitareas-layer > *',
};

// Export for Browser
if (typeof window !== "undefined") {
  window.Selectors = Selectors;
}

// Export for Bun/Node
if (typeof module !== "undefined" && module.exports) {
  module.exports = { Selectors };
}
