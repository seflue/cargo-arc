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
   *   hiddenArcIds: Set<string>,
   *   clusterMode: boolean,
   *   selectedScc: number|null
   * }}
   */
  create() {
    return {
      collapsed: new Set(),
      clickSelection: { type: null, id: null },
      hoverSelection: { type: null, id: null },
      hiddenArcIds: new Set(),
      clusterMode: true,
      selectedScc: null,
    };
  },

  // === Collapse Operations ===

  /** @param {Object} state @param {string} nodeId @returns {boolean} */
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

  // === Cycle-Mode Selection (Ebene 1: SCC) ===
  //
  // selectedScc adds an outer selection layer on top of the click/hover slots
  // above, which continue to track the inner (focused) edge unchanged. Hover
  // only takes effect once an SCC is selected (no preview before selection);
  // that gate belongs to the derivation in derived_state.js (it already reads
  // selectedScc + sccId there), not here, since hoverSelection is a generic
  // slot shared with non-cycle hovers.

  /**
   * Get the currently selected SCC (Ebene 1).
   * @param {Object} state
   * @returns {number|null}
   */
  getSelectedScc(state) {
    return state.selectedScc;
  },

  /**
   * Transition function for a click on an arc, cycle or not.
   * @param {Object} state
   * @param {string} arcId
   * @param {number|null|undefined} sccId - SCC of the clicked arc, if any
   */
  clickEdge(state, arcId, sccId) {
    if (sccId == null) {
      // Non-cycle edge: leaves cluster selection, falls back to the
      // pre-existing single-edge toggle behavior.
      state.selectedScc = null;
      this.toggleSelection(state, 'arc', arcId);
      return;
    }
    if (state.selectedScc == null) {
      // First click selects the SCC only: the cluster reads as one unit. Drop
      // the hover the preceding mouseenter set on this very edge, so the
      // overview shows before any inner edge is focused.
      state.selectedScc = sccId;
      this.clearHover(state);
      return;
    }
    if (state.selectedScc === sccId) {
      const pinned = this.toggleSelection(state, 'arc', arcId);
      // Unpin returns to the cluster overview: drop the lingering hover so no
      // inner edge stays focused.
      if (!pinned) this.clearHover(state);
      return;
    }
    // Switching SCCs opens a fresh overview: drop pin and hover.
    state.selectedScc = sccId;
    this.clearSelection(state);
    this.clearHover(state);
  },

  /**
   * Transition for a click on a sidebar cluster row. Expansion and pin move
   * together: a click drives the row toward expanded+pinned, except when it is
   * already expanded+pinned, where it collapses+unpins. A row left collapsed
   * while pinned (via collapse-all) just re-expands, keeping the pin.
   * @param {Object} state
   * @param {string} arcId
   * @param {number|null|undefined} sccId - SCC of the row's edge (open cluster)
   * @param {boolean} expandable - Row has crossing symbols to show
   * @param {boolean} expanded - Row is currently expanded
   * @returns {boolean} whether the row ends expanded (mirrors the new pin state)
   */
  clickClusterRow(state, arcId, sccId, expandable, expanded) {
    if (expandable && !expanded && this.isSelected(state, 'arc', arcId)) {
      // Collapsed but pinned: re-expand only, pin and graph stay put.
      return true;
    }
    this.clickEdge(state, arcId, sccId);
    return this.isSelected(state, 'arc', arcId);
  },

  /**
   * Click into empty space: reset SCC selection, pin and hover.
   * @param {Object} state
   */
  clickEmpty(state) {
    state.selectedScc = null;
    this.clearSelection(state);
    this.clearHover(state);
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

  /** @param {Object} state @param {string} arcId @returns {boolean} */
  isArcHidden(state, arcId) {
    return state.hiddenArcIds.has(arcId);
  },

  /**
   * Get pinned (click) selection.
   * @param {Object} state
   * @returns {null|{type: string, id: string}}
   */
  getPinned(state) {
    if (state.clickSelection.type === null) return null;
    return { ...state.clickSelection };
  },

  // === Cluster Mode ===

  /** @param {Object} state @returns {boolean} */
  isClusterMode(state) {
    return state.clusterMode;
  },

  /** @param {Object} state @param {boolean} on */
  setClusterMode(state, on) {
    state.clusterMode = on;
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { AppState };
}
