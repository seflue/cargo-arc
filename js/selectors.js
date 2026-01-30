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
  visibleArc: (arcId) => `.dep-arc[data-arc-id="${arcId}"], .cycle-arc[data-arc-id="${arcId}"]`,
  hitarea: (arcId) => `.arc-hitarea[data-arc-id="${arcId}"]`,
  arrows: (arcId) => `[data-edge="${arcId}"]`,
  virtualArrows: (arcId) => `[data-vedge="${arcId}"]:not(.arc-count)`,
  virtualArc: (from, to) => `.virtual-arc[data-from="${from}"][data-to="${to}"]`,
  connectedHitareas: (nodeId) => `.arc-hitarea[data-from="${nodeId}"], .arc-hitarea[data-to="${nodeId}"]`,
  labelGroup: (arcId) => `.arc-count-group[data-vedge="${arcId}"]`,

  // Identity selectors (parametrized)
  collapseToggle: (nodeId) => `.collapse-toggle[data-target="${nodeId}"]`,
  treeLineChild: (nodeId) => `line[data-child="${nodeId}"]`,
  treeLineParent: (nodeId) => `line[data-parent="${nodeId}"]`,

  // Category selectors (batch operations)
  allHitareas: () => '.arc-hitarea',
  allVirtualElements: () => '.virtual-arc, .virtual-hitarea, .virtual-arrow, .arc-count, .arc-count-group, .arc-count-bg',
  allBaseEdges: () => '.arc-hitarea, .dep-arc, .cycle-arc',
  allBaseArrows: () => '.dep-arrow, .cycle-arrow',

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
