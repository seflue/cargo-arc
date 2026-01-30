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
   * @returns {{ type: string, parent: string|null, x: number, y: number, hasChildren: boolean }|undefined}
   */
  getNode(id) {
    return STATIC_DATA.nodes[id];
  },

  /**
   * Get arc data by ID
   * @param {string} id - Arc ID (format: "from-to")
   * @returns {{ from: string, to: string, usages: string[] }|undefined}
   */
  getArc(id) {
    return STATIC_DATA.arcs[id];
  },

  /**
   * Get parent node ID
   * @param {string} nodeId
   * @returns {string|null}
   */
  getParent(nodeId) {
    return STATIC_DATA.nodes[nodeId]?.parent ?? null;
  },

  /**
   * Get original position and dimensions for node (from initial layout)
   * @param {string} nodeId
   * @returns {{ x: number, y: number, width: number, height: number }|null}
   */
  getOriginalPosition(nodeId) {
    const node = STATIC_DATA.nodes[nodeId];
    return node ? { x: node.x, y: node.y, width: node.width, height: node.height } : null;
  },

  /**
   * Get arc weight (number of usages)
   * @param {string} arcId
   * @returns {number}
   */
  getArcWeight(arcId) {
    return STATIC_DATA.arcs[arcId]?.usages.length ?? 0;
  },

  /**
   * Get arc usages (source locations array)
   * @param {string} arcId
   * @returns {string[]}
   */
  getArcUsages(arcId) {
    return STATIC_DATA.arcs[arcId]?.usages ?? [];
  },

  /**
   * Get formatted usages as pipe-separated string (for tooltips)
   * @param {string} arcId
   * @returns {string}
   */
  getFormattedUsages(arcId) {
    return this.getArcUsages(arcId).join('|');
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
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { StaticData };
}
