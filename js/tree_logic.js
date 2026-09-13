// @module TreeLogic
// @deps
// @config
// tree_logic.js - Pure tree traversal logic
// No DOM dependencies - operates on Maps/Sets
// State management moved to AppState module

const TreeLogic = {
  /**
   * Get all descendants of a node (recursive)
   * @param {string} nodeId
   * @param {Map<string, string>} parentMap - childId -> parentId
   * @returns {string[]}
   */
  getDescendants(nodeId, parentMap) {
    const descendants = [];
    for (const [childId, parentId] of parentMap) {
      if (parentId === nodeId) {
        descendants.push(childId);
        descendants.push(...this.getDescendants(childId, parentMap));
      }
    }
    return descendants;
  },

  /**
   * Count all descendants
   * @param {string} nodeId
   * @param {Map<string, string>} parentMap
   * @returns {number}
   */
  countDescendants(nodeId, parentMap) {
    return this.getDescendants(nodeId, parentMap).length;
  },

  /**
   * Whether any descendant (not the node itself) lies on a cycle.
   * @param {string} nodeId
   * @param {Map<string, string>} parentMap
   * @param {Set<string>} cycleNodeIds - Nodes lying on a cycle
   * @returns {boolean}
   */
  hasCycleDescendant(nodeId, parentMap, cycleNodeIds) {
    return this.getDescendants(nodeId, parentMap).some((id) =>
      cycleNodeIds.has(id),
    );
  },

  /**
   * Find visible ancestor (or self if visible)
   * A node is hidden if ANY ancestor is collapsed.
   * @param {string} nodeId
   * @param {Set<string>} collapsedSet - Set of collapsed node IDs
   * @param {Map<string, string>} parentMap - childId -> parentId
   * @returns {string|null}
   */
  getVisibleAncestor(nodeId, collapsedSet, parentMap) {
    const parentId = parentMap.get(nodeId);
    if (!parentId) return nodeId;

    if (collapsedSet.has(parentId)) {
      return this.getVisibleAncestor(parentId, collapsedSet, parentMap);
    }

    const parentsVisibleAncestor = this.getVisibleAncestor(
      parentId,
      collapsedSet,
      parentMap,
    );
    if (parentsVisibleAncestor !== parentId) {
      return parentsVisibleAncestor;
    }

    return nodeId;
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { TreeLogic };
}
