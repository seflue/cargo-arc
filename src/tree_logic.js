// tree_logic.js - Pure tree traversal and collapse state management
// No DOM dependencies - uses Maps as data structures

const TreeLogic = {
  /**
   * Builds parentMap from DOM elements (once at init)
   * @param {Object} domAdapter - DOM adapter with querySelectorAll
   * @returns {Map<string, string>} childId -> parentId
   */
  buildParentMap(domAdapter) {
    const parentMap = new Map();
    const elements = domAdapter.querySelectorAll('[data-parent]');
    elements.forEach(el => {
      if (el.tagName === 'rect') {
        const childId = el.id.replace('node-', '');
        const parentId = el.dataset.parent;
        if (parentId) parentMap.set(childId, parentId);
      }
    });
    return parentMap;
  },

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
   * @param {Map<string, boolean>} collapseState - nodeId -> collapsed
   * @param {Map<string, string>} parentMap - childId -> parentId
   * @returns {string|null}
   */
  getVisibleAncestor(nodeId, collapseState, parentMap) {
    const parentId = parentMap.get(nodeId);
    if (!parentId) return nodeId; // Root - always visible

    // Check if parent is collapsed
    if (collapseState.get(parentId)) {
      // Parent is collapsed -> return parent's visible ancestor
      return this.getVisibleAncestor(parentId, collapseState, parentMap);
    }

    // Parent is not collapsed, but check if parent is visible
    // (i.e., no ancestor of parent is collapsed)
    const parentsVisibleAncestor = this.getVisibleAncestor(parentId, collapseState, parentMap);
    if (parentsVisibleAncestor !== parentId) {
      // Parent is hidden (has collapsed ancestor) -> return that ancestor
      return parentsVisibleAncestor;
    }

    return nodeId; // This node is visible
  }
};

const CollapseState = {
  /**
   * Create new CollapseState
   * @returns {{collapsed: Map<string, boolean>, originalPositions: Map<string, {x: number, y: number}>}}
   */
  create() {
    return {
      collapsed: new Map(),        // nodeId -> boolean
      originalPositions: new Map() // nodeId -> {x, y}
    };
  },

  /**
   * Check if node is collapsed
   * @param {Object} state - CollapseState object
   * @param {string} nodeId
   * @returns {boolean}
   */
  isCollapsed(state, nodeId) {
    return state.collapsed.get(nodeId) ?? false;
  },

  /**
   * Set collapsed state
   * @param {Object} state
   * @param {string} nodeId
   * @param {boolean} collapsed
   */
  setCollapsed(state, nodeId, collapsed) {
    state.collapsed.set(nodeId, collapsed);
  },

  /**
   * Toggle collapsed state
   * @param {Object} state
   * @param {string} nodeId
   * @returns {boolean} - New collapsed state
   */
  toggle(state, nodeId) {
    const current = this.isCollapsed(state, nodeId);
    this.setCollapsed(state, nodeId, !current);
    return !current;
  },

  /**
   * Store original position
   * @param {Object} state
   * @param {string} nodeId
   * @param {number} x
   * @param {number} y
   */
  storePosition(state, nodeId, x, y) {
    state.originalPositions.set(nodeId, { x, y });
  },

  /**
   * Get stored position
   * @param {Object} state
   * @param {string} nodeId
   * @returns {{x: number, y: number}|undefined}
   */
  getPosition(state, nodeId) {
    return state.originalPositions.get(nodeId);
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { TreeLogic, CollapseState };
}
