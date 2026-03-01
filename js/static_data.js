// @module StaticData
// @deps ArcLogic
// @config
// static_data.js - Helper for accessing STATIC_DATA
// Provides typed access to pre-rendered node/arc data from Rust
// Eliminates DOM reads for static properties (positions, parents, arc weights)

const StaticData = {
  /**
   * Get node data by ID
   * @param {string} id - Node ID
   * @returns {StaticNodeData|undefined}
   */
  getNode(id) {
    return STATIC_DATA.nodes[id];
  },

  /**
   * Get arc data by ID
   * @param {string} id - Arc ID (format: "from-to")
   * @returns {StaticArcData|undefined}
   */
  getArc(id) {
    return STATIC_DATA.arcs[id];
  },

  /**
   * Get original position and dimensions for node (from initial layout)
   * @param {string} nodeId
   * @returns {{ x: number, y: number, width: number, height: number }|null}
   */
  getOriginalPosition(nodeId) {
    const node = STATIC_DATA.nodes[nodeId];
    return node
      ? { x: node.x, y: node.y, width: node.width, height: node.height }
      : null;
  },

  /**
   * Get arc weight (number of usages)
   * @param {string} arcId
   * @returns {number}
   */
  getArcWeight(arcId) {
    const usages = STATIC_DATA.arcs[arcId]?.usages;
    if (!usages) return 0;
    return usages.reduce((sum, g) => sum + g.locations.length, 0);
  },

  /**
   * Get arc usages (source locations array)
   * @param {string} arcId
   * @returns {StaticArcData["usages"]}
   */
  getArcUsages(arcId) {
    return STATIC_DATA.arcs[arcId]?.usages ?? [];
  },

  /**
   * Get calculated stroke width for an arc based on usage count.
   * Uses ArcLogic.calculateStrokeWidth for consistent scaling.
   * @param {string} arcId
   * @returns {number} Stroke width in pixels (0.5 to 2.5)
   */
  getArcStrokeWidth(arcId) {
    return ArcLogic.calculateStrokeWidth(this.getArcWeight(arcId));
  },

  /**
   * Check if node has children
   * @param {string} nodeId
   * @returns {boolean}
   */
  hasChildren(nodeId) {
    return STATIC_DATA.nodes[nodeId]?.hasChildren ?? false;
  },

  /**
   * Get all node IDs
   * @returns {string[]}
   */
  getAllNodeIds() {
    return Object.keys(STATIC_DATA.nodes);
  },

  /**
   * Get all arc IDs
   * @returns {string[]}
   */
  getAllArcIds() {
    return Object.keys(STATIC_DATA.arcs);
  },

  /**
   * Build parent map for TreeLogic (replaces TreeLogic.buildParentMap)
   * @returns {Map<string, string>} childId -> parentId
   */
  buildParentMap() {
    const parentMap = new Map();
    for (const [nodeId, node] of Object.entries(STATIC_DATA.nodes)) {
      if (node.parent !== null) {
        parentMap.set(nodeId, node.parent);
      }
    }
    return parentMap;
  },

  /**
   * Check if a node is an external type (external-section or external crate).
   * @param {string} nodeId
   * @returns {boolean}
   */
  isExternalNode(nodeId) {
    const node = STATIC_DATA.nodes[nodeId];
    if (!node) return false;
    return (
      node.type === 'external-section' ||
      node.type === 'external' ||
      node.type === 'external-transitive'
    );
  },

  /**
   * Check if an arc connects to any external node.
   * @param {string} arcId
   * @returns {boolean}
   */
  isExternalArc(arcId) {
    const arc = STATIC_DATA.arcs[arcId];
    if (!arc) return false;
    return this.isExternalNode(arc.from) || this.isExternalNode(arc.to);
  },

  /**
   * Check if a node is a transitive external dependency.
   * @param {string} nodeId
   * @returns {boolean}
   */
  isTransitiveNode(nodeId) {
    const node = STATIC_DATA.nodes[nodeId];
    if (!node) return false;
    return node.type === 'external-transitive';
  },

  /**
   * Check if an arc connects to any transitive external node.
   * @param {string} arcId
   * @returns {boolean}
   */
  isTransitiveArc(arcId) {
    const arc = STATIC_DATA.arcs[arcId];
    if (!arc) return false;
    return this.isTransitiveNode(arc.from) || this.isTransitiveNode(arc.to);
  },

  /**
   * Get nesting level of a node.
   * @param {string} id - Node ID
   * @returns {number|undefined}
   */
  getNodeNesting(id) {
    return STATIC_DATA.nodes[id]?.nesting;
  },

  /**
   * Get the expand level from config (null if not set = all expanded).
   * @returns {number|null}
   */
  getExpandLevel() {
    return STATIC_DATA.expandLevel ?? null;
  },

  /**
   * Get cycle data by ID.
   * @param {number} cycleId
   * @returns {Object|null}
   */
  getCycle(cycleId) {
    return STATIC_DATA.cycles?.[cycleId] ?? null;
  },

  /**
   * Get external crate nodes grouped by name.
   * Crates with same name but different versions are grouped together.
   * @returns {Map<string, string[]>} name -> [nodeId, ...] sorted by version
   */
  getExternalGroups() {
    const groups = new Map();
    for (const [nodeId, node] of Object.entries(STATIC_DATA.nodes)) {
      if (node.type !== 'external' && node.type !== 'external-transitive')
        continue;
      const name = node.name;
      if (!groups.has(name)) groups.set(name, []);
      groups.get(name).push(nodeId);
    }
    // Only return groups with multiple versions
    const result = new Map();
    for (const [name, ids] of groups) {
      if (ids.length > 1) result.set(name, ids);
    }
    return result;
  },

  /**
   * Get all relations (incoming + outgoing arcs) for a node.
   * @param {string} nodeId
   * @returns {{ outgoing: Array<{targetId: string, weight: number, usages: Array, arcId: string}>, incoming: Array<{targetId: string, weight: number, usages: Array, arcId: string}> }}
   */
  getNodeRelations(nodeId) {
    const outgoing = [];
    const incoming = [];
    for (const [arcId, arc] of Object.entries(STATIC_DATA.arcs)) {
      if (arc.from === nodeId) {
        outgoing.push({
          targetId: arc.to,
          weight: this.getArcWeight(arcId),
          usages: arc.usages ?? [],
          arcId,
        });
      } else if (arc.to === nodeId) {
        incoming.push({
          targetId: arc.from,
          weight: this.getArcWeight(arcId),
          usages: arc.usages ?? [],
          arcId,
        });
      }
    }
    const byTreeOrder = (a, b) => {
      const yA = this.getOriginalPosition(a.targetId)?.y ?? Infinity;
      const yB = this.getOriginalPosition(b.targetId)?.y ?? Infinity;
      return yA - yB;
    };
    outgoing.sort(byTreeOrder);
    incoming.sort(byTreeOrder);
    return { outgoing, incoming };
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { StaticData };
}
