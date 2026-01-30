// @module AppState
// @deps
// @config
// app_state.js - Unified application state management
// Consolidates CollapseState and HighlightState into single state object
// No DOM dependencies - pure state operations

const AppState = {
  /**
   * Create new AppState
   * @returns {{
   *   collapsed: Set<string>,
   *   selection: { mode: 'none'|'hover'|'click', type: 'node'|'arc'|null, id: string|null }
   * }}
   */
  create() {
    return {
      collapsed: new Set(),
      selection: { mode: 'none', type: null, id: null }
    };
  },

  // === Collapse Operations ===

  /**
   * Check if node is collapsed
   * @param {Object} state - AppState object
   * @param {string} nodeId
   * @returns {boolean}
   */
  isCollapsed(state, nodeId) {
    return state.collapsed.has(nodeId);
  },

  /**
   * Set collapsed state for node
   * @param {Object} state
   * @param {string} nodeId
   * @param {boolean} collapsed
   */
  setCollapsed(state, nodeId, collapsed) {
    if (collapsed) {
      state.collapsed.add(nodeId);
    } else {
      state.collapsed.delete(nodeId);
    }
  },

  /**
   * Toggle collapsed state
   * @param {Object} state
   * @param {string} nodeId
   * @returns {boolean} - New collapsed state
   */
  toggleCollapsed(state, nodeId) {
    const wasCollapsed = this.isCollapsed(state, nodeId);
    this.setCollapsed(state, nodeId, !wasCollapsed);
    return !wasCollapsed;
  },

  // === Selection Operations ===

  /**
   * Get current selection
   * @param {Object} state
   * @returns {{ mode: string, type: string|null, id: string|null }}
   */
  getSelection(state) {
    return state.selection;
  },

  /**
   * Set selection (pinned/clicked)
   * @param {Object} state
   * @param {'node'|'arc'} type
   * @param {string} id
   */
  setSelection(state, type, id) {
    state.selection = { mode: 'click', type, id };
  },

  /**
   * Set hover selection (temporary, not pinned)
   * @param {Object} state
   * @param {'node'|'arc'} type
   * @param {string} id
   */
  setHover(state, type, id) {
    state.selection = { mode: 'hover', type, id };
  },

  /**
   * Clear selection
   * @param {Object} state
   */
  clearSelection(state) {
    state.selection = { mode: 'none', type: null, id: null };
  },

  /**
   * Check if specific element is selected (pinned)
   * @param {Object} state
   * @param {'node'|'arc'} type
   * @param {string} id
   * @returns {boolean}
   */
  isSelected(state, type, id) {
    return state.selection.mode === 'click' &&
           state.selection.type === type &&
           state.selection.id === id;
  },

  /**
   * Check if anything is pinned (clicked selection)
   * @param {Object} state
   * @returns {boolean}
   */
  hasPinnedSelection(state) {
    return state.selection.mode === 'click';
  },

  /**
   * Toggle selection for element
   * If same element is selected, deselects. Otherwise selects new element.
   * @param {Object} state
   * @param {'node'|'arc'} type
   * @param {string} id
   * @returns {boolean} - true if newly selected, false if deselected
   */
  toggleSelection(state, type, id) {
    if (this.isSelected(state, type, id)) {
      this.clearSelection(state);
      return false;
    }
    this.setSelection(state, type, id);
    return true;
  },

  // === Legacy Compatibility ===
  // These mirror HighlightState API for easier migration

  /**
   * Get pinned selection (legacy API)
   * @param {Object} state
   * @returns {null|{type: string, id: string}}
   */
  getPinned(state) {
    if (state.selection.mode !== 'click') return null;
    return { type: state.selection.type, id: state.selection.id };
  },

  /**
   * Toggle pinned state (legacy API)
   * @param {Object} state
   * @param {'node'|'edge'} type
   * @param {string} id
   * @returns {boolean} - true if newly pinned, false if unpinned
   */
  togglePinned(state, type, id) {
    return this.toggleSelection(state, type, id);
  },

  /**
   * Clear pinned selection (legacy API)
   * @param {Object} state
   */
  clearPinned(state) {
    this.clearSelection(state);
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { AppState };
}
