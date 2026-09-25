// @module AppState
// @deps RowState
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
   *   selectedScc: number|null,
   *   expandedRows: Set<string>
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
      expandedRows: new Set(),
    };
  },

  // === Collapse Operations ===

  /** @param {Object} state @param {string} nodeId @returns {boolean} */
  isCollapsed(state, nodeId) {
    return state.collapsed.has(nodeId);
  },

  /** @param {Object} state @param {string} arcId @returns {boolean} */
  isRowExpanded(state, arcId) {
    return state.expandedRows.has(arcId);
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
      this._enterScc(state, null);
      this.toggleSelection(state, 'arc', arcId);
      return;
    }
    if (state.selectedScc == null) {
      // First click selects the SCC only: the cluster reads as one unit. Drop
      // the hover the preceding mouseenter set on this very edge, so the
      // overview shows before any inner edge is focused.
      this._enterScc(state, sccId);
      this.clearHover(state);
      return;
    }
    if (state.selectedScc === sccId) {
      // Any edge click while this SCC is already open, same edge or a
      // different one (the pin moves here from wherever it was): the row
      // follows via the same pinned/unpinned events an outside pin change
      // sends it, so unpinning here leaves the row expanded instead of
      // collapsing it.
      const wasPinned = this.isSelected(state, 'arc', arcId);
      const current = this.rowState(state, arcId);
      const next = wasPinned
        ? RowState[current].unpinned()
        : RowState[current].pinned();
      this.applyRowState(state, arcId, next);
      // Unpin returns to the cluster overview: drop the lingering hover so no
      // inner edge stays focused.
      if (!this.isSelected(state, 'arc', arcId)) this.clearHover(state);
      return;
    }
    // Switching SCCs opens a fresh overview: drop pin and hover.
    this._enterScc(state, sccId);
    this.clearSelection(state);
    this.clearHover(state);
  },

  /**
   * Click into empty space: reset SCC selection, pin and hover.
   * @param {Object} state
   */
  clickEmpty(state) {
    this._enterScc(state, null);
    this.clearSelection(state);
    this.clearHover(state);
  },

  // === Tangle sidebar row state (RowState: expand/collapse x pin/unpin) ===
  //
  // A row's state is not stored; it is read from the two bits that already
  // exist (expandedRows, the arc pin slot) and written back the same way, so
  // there is no second copy of "is this row pinned" beside clickSelection.

  /**
   * Enter a (possibly unchanged) SCC selection. A different tangle has
   * different rows, so its expandedRows is meaningless once the sidebar
   * moves on — dropped on every selectedScc change, including to null.
   * @param {Object} state
   * @param {number|null} sccId
   */
  _enterScc(state, sccId) {
    if (state.selectedScc === sccId) return;
    state.selectedScc = sccId;
    state.expandedRows.clear();
  },

  /**
   * Read a tangle row's current state from its two bits.
   * @param {Object} state
   * @param {string} arcId
   * @returns {'CU'|'EU'|'CP'|'EP'}
   */
  rowState(state, arcId) {
    const expanded = this.isRowExpanded(state, arcId);
    const pinned = this.isSelected(state, 'arc', arcId);
    if (expanded) return pinned ? 'EP' : 'EU';
    return pinned ? 'CP' : 'CU';
  },

  /**
   * Write a row's next state back into expandedRows and the pin slot.
   * @param {Object} state
   * @param {string} arcId
   * @param {'CU'|'EU'|'CP'|'EP'} next
   */
  applyRowState(state, arcId, next) {
    if (next === 'EU' || next === 'EP') state.expandedRows.add(arcId);
    else state.expandedRows.delete(arcId);
    if (next === 'CP' || next === 'EP') {
      this.setSelection(state, 'arc', arcId);
    } else if (this.isSelected(state, 'arc', arcId)) {
      this.clearSelection(state);
    }
  },

  /**
   * Triangle click: flip a row's expand bit, the pin stays untouched.
   * @param {Object} state
   * @param {string} arcId
   */
  rowToggle(state, arcId) {
    this.applyRowState(
      state,
      arcId,
      RowState[this.rowState(state, arcId)].toggle(),
    );
  },

  /**
   * Head click (outside the triangle): pin this row, expanding it, unless it
   * already held the pin, in which case it unpins without collapsing. A pin
   * taken from another row sends that row the same unpinned event a graph
   * unpin would, so it too stays expanded.
   * @param {Object} state
   * @param {string} arcId
   * @param {number|null|undefined} sccId - SCC of the row's edge (open cluster)
   */
  rowPinClick(state, arcId, sccId) {
    if (sccId != null) this._enterScc(state, sccId);
    const next = RowState[this.rowState(state, arcId)].pinClick();
    if (next === 'EP') {
      const previous = this.getPinned(state);
      if (previous && previous.type === 'arc' && previous.id !== arcId) {
        const prevState = this.rowState(state, previous.id);
        this.applyRowState(state, previous.id, RowState[prevState].unpinned());
      }
    }
    this.applyRowState(state, arcId, next);
  },

  /**
   * Expand every given row, whatever its pin. Used by the sidebar's
   * collapse-all control; the caller resolves which arc ids belong to the
   * open tangle.
   * @param {Object} state
   * @param {Iterable<string>} arcIds
   */
  rowsExpandAll(state, arcIds) {
    for (const arcId of arcIds) state.expandedRows.add(arcId);
  },

  /**
   * Collapse every given row, whatever its pin.
   * @param {Object} state
   * @param {Iterable<string>} arcIds
   */
  rowsCollapseAll(state, arcIds) {
    for (const arcId of arcIds) state.expandedRows.delete(arcId);
  },

  /**
   * The editor's cursor landed on a row's jump target: expand it, pin
   * untouched.
   * @param {Object} state
   * @param {string} arcId
   */
  rowFollow(state, arcId) {
    this.applyRowState(
      state,
      arcId,
      RowState[this.rowState(state, arcId)].follow(),
    );
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
