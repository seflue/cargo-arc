/**
 * Selectors - CSS selector generators for SVG elements
 * Pure functions, no DOM access - testable in isolation
 */
const Selectors = {
  nodeId: (id: string) => `node-${id}`,
  countId: (id: string) => `count-${id}`,
  cycleMarkerId: (id: string) => `cycle-marker-${id}`,

  baseArc: (arcId: string) => {
    const c = STATIC_DATA.classes;
    return `.${c.direction.depArc}[data-arc-id="${arcId}"], .${c.direction.cycleArc}[data-arc-id="${arcId}"]`;
  },
  hitarea: (arcId: string) =>
    `.${STATIC_DATA.classes.direction.arcHitarea}[data-arc-id="${arcId}"]`,
  arrows: (arcId: string) => `polygon[data-edge="${arcId}"]`,
  virtualArrows: (arcId: string) =>
    `[data-vedge="${arcId}"]:not(.${STATIC_DATA.classes.labels.arcCount})`,
  virtualArc: (from: string, to: string) =>
    `.${STATIC_DATA.classes.direction.virtualArc}[data-from="${from}"][data-to="${to}"]`,
  labelGroup: (arcId: string) =>
    `.${STATIC_DATA.classes.labels.arcCountGroup}[data-vedge="${arcId}"]`,

  // Identity selectors (parametrized)
  collapseToggle: (nodeId: string) =>
    `.${STATIC_DATA.classes.nodes.collapseToggle}[data-target="${nodeId}"]`,
  treeLineChild: (nodeId: string) => `line[data-child="${nodeId}"]`,
  treeLineParent: (nodeId: string) => `line[data-parent="${nodeId}"]`,

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
