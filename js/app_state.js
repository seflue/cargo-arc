// @module AppState
// @deps
// @config
// app_state.js - Unified application state management
// Consolidates CollapseState and HighlightState into single state object
// No DOM dependencies - pure state operations
//
// Selection model: two independent slots (clickSelection, hoverSelection).
// clickSelection is persistent (survives hover changes), hoverSelection is transient.
// getSelection() returns click-priority-over-hover semantics.

const AppState = {
  /**
   * Create new AppState
   * @returns {{
   *   collapsed: Set<string>,
   *   clickSelection: { type: 'node'|'arc'|null, id: string|null },
   *   hoverSelection: { type: 'node'|'arc'|null, id: string|null },
   *   hiddenArcIds: Set<string>
   * }}
   */
  create() {
    return {
      collapsed: new Set(),
      clickSelection: { type: null, id: null },
      hoverSelection: { type: null, id: null },
      hiddenArcIds: new Set(),
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
   * Resolve the current selection with click-priority-over-hover semantics.
   * Returns clickSelection if present, otherwise hoverSelection, otherwise none.
   * @param {Object} state
   * @returns {{ mode: 'click'|'hover'|'none', type: string|null, id: string|null }}
   */
  getSelection(state) {
    if (state.clickSelection.type !== null) {
      return {
        mode: 'click',
        type: state.clickSelection.type,
        id: state.clickSelection.id,
      };
    }
    if (state.hoverSelection.type !== null) {
      return {
        mode: 'hover',
        type: state.hoverSelection.type,
        id: state.hoverSelection.id,
      };
    }
    return { mode: 'none', type: null, id: null };
  },

  /**
   * Set click selection (persistent, survives hover changes)
   * @param {Object} state
   * @param {'node'|'arc'} type
   * @param {string} id
   */
  setSelection(state, type, id) {
    state.clickSelection = { type, id };
  },

  /**
   * Set hover selection (transient, does NOT touch clickSelection)
   * @param {Object} state
   * @param {'node'|'arc'} type
   * @param {string} id
   */
  setHover(state, type, id) {
    state.hoverSelection = { type, id };
  },

  /**
   * Clear click selection only (hover selection is unaffected)
   * @param {Object} state
   */
  clearSelection(state) {
    state.clickSelection = { type: null, id: null };
  },

  /**
   * Clear hover selection only (click selection is unaffected)
   * @param {Object} state
   */
  clearHover(state) {
    state.hoverSelection = { type: null, id: null };
  },

  /**
   * Check if specific element is click-selected (pinned)
   * @param {Object} state
   * @param {'node'|'arc'} type
   * @param {string} id
   * @returns {boolean}
   */
  isSelected(state, type, id) {
    return state.clickSelection.type === type && state.clickSelection.id === id;
  },

  /**
   * Check if anything is pinned (has a click selection)
   * @param {Object} state
   * @returns {boolean}
   */
  hasPinnedSelection(state) {
    return state.clickSelection.type !== null;
  },

  /**
   * Toggle click selection for element.
   * If same element is click-selected, deselects. Otherwise selects new element.
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

  // === Arc Filter Operations ===

  /**
   * Mark arc as hidden by filter
   * @param {Object} state
   * @param {string} arcId
   */
  hideArc(state, arcId) {
    state.hiddenArcIds.add(arcId);
  },

  /**
   * Mark arc as visible (remove from hidden set)
   * @param {Object} state
   * @param {string} arcId
   */
  showArc(state, arcId) {
    state.hiddenArcIds.delete(arcId);
  },

  /**
   * Check if arc is hidden by filter
   * @param {Object} state
   * @param {string} arcId
   * @returns {boolean}
   */
  isArcHidden(state, arcId) {
    return state.hiddenArcIds.has(arcId);
  },

  // === Legacy Compatibility ===
  // These mirror HighlightState API for easier migration

  /**
   * Get pinned (click) selection (legacy API)
   * @param {Object} state
   * @returns {null|{type: string, id: string}}
   */
  getPinned(state) {
    if (state.clickSelection.type === null) return null;
    return { ...state.clickSelection };
  },

  /**
   * Toggle pinned state (legacy API, delegates to toggleSelection)
   * @param {Object} state
   * @param {'node'|'arc'} type
   * @param {string} id
   * @returns {boolean} - true if newly pinned, false if unpinned
   */
  togglePinned(state, type, id) {
    return this.toggleSelection(state, type, id);
  },

  /**
   * Clear pinned selection (legacy API, delegates to clearSelection)
   * @param {Object} state
   */
  clearPinned(state) {
    this.clearSelection(state);
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { AppState };
}
