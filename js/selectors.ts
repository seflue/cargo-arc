/**
 * Selectors - CSS selector generators for SVG elements
 * Pure functions, no DOM access - testable in isolation
 */
const Selectors = {
  nodeId: (id) => `node-${id}`,
  countId: (id) => `count-${id}`,
  cycleMarkerId: (id) => `cycle-marker-${id}`,

  baseArc: (arcId) => {
    const c = STATIC_DATA.classes;
    return `.${c.direction.depArc}[data-arc-id="${arcId}"], .${c.direction.cycleArc}[data-arc-id="${arcId}"]`;
  },
  hitarea: (arcId) =>
    `.${STATIC_DATA.classes.direction.arcHitarea}[data-arc-id="${arcId}"]`,
  arrows: (arcId) => `polygon[data-edge="${arcId}"]`,
  virtualArrows: (arcId) =>
    `[data-vedge="${arcId}"]:not(.${STATIC_DATA.classes.labels.arcCount})`,
  virtualArc: (from, to) =>
    `.${STATIC_DATA.classes.direction.virtualArc}[data-from="${from}"][data-to="${to}"]`,
  labelGroup: (arcId) =>
    `.${STATIC_DATA.classes.labels.arcCountGroup}[data-vedge="${arcId}"]`,

  // Identity selectors (parametrized)
  collapseToggle: (nodeId) =>
    `.${STATIC_DATA.classes.nodes.collapseToggle}[data-target="${nodeId}"]`,
  treeLineChild: (nodeId) => `line[data-child="${nodeId}"]`,
  treeLineParent: (nodeId) => `line[data-parent="${nodeId}"]`,

  // Category selectors (batch operations)
  allHitareas: () => `.${STATIC_DATA.classes.direction.arcHitarea}`,
  allVirtualElements: () => {
    const c = STATIC_DATA.classes;
    return `.${c.direction.virtualArc}, .${c.direction.virtualHitarea}, .${c.direction.virtualArrow}, .${c.labels.arcCount}, .${c.labels.arcCountGroup}, .${c.labels.arcCountBg}`;
  },
  allBaseEdges: () => {
    const c = STATIC_DATA.classes;
    return `.${c.direction.arcHitarea}, .${c.direction.depArc}, .${c.direction.cycleArc}`;
  },
  allBaseArrows: () => {
    const c = STATIC_DATA.classes;
    return `.${c.direction.depArrow}, .${c.direction.upwardArrow}, .${c.direction.cycleArrow}`;
  },
  allArcPaths: () => {
    const c = STATIC_DATA.classes;
    return `.${c.direction.depArc}, .${c.direction.cycleArc}, .${c.direction.virtualArc}`;
  },
};

export { Selectors };
