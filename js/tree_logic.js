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
   * Find visible ancestor (or self if visible)
   * A node is hidden if ANY ancestor is collapsed.
   * @param {string} nodeId
   * @param {Set<string>} collapsedSet - Set of collapsed node IDs
   * @param {Map<string, string>} parentMap - childId -> parentId
   * @returns {string|null}
   */
  getVisibleAncestor(nodeId, collapsedSet, parentMap) {
    const parentId = parentMap.get(nodeId);
    if (!parentId) return nodeId; // Root - always visible

    // Check if parent is collapsed
    if (collapsedSet.has(parentId)) {
      // Parent is collapsed -> return parent's visible ancestor
      return this.getVisibleAncestor(parentId, collapsedSet, parentMap);
    }

    // Parent is not collapsed, but check if parent is visible
    // (i.e., no ancestor of parent is collapsed)
    const parentsVisibleAncestor = this.getVisibleAncestor(parentId, collapsedSet, parentMap);
    if (parentsVisibleAncestor !== parentId) {
      // Parent is hidden (has collapsed ancestor) -> return that ancestor
      return parentsVisibleAncestor;
    }

    return nodeId; // This node is visible
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { TreeLogic };
}
