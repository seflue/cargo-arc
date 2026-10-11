import { RowState } from './row_state.js';

// app_state.ts - Unified application state management
// Consolidates CollapseState and HighlightState into single state object
// No DOM dependencies - pure state operations
//
// Selection model: two independent slots (clickSelection, hoverSelection).
// clickSelection is persistent (survives hover changes), hoverSelection is transient.
// getSelection() returns click-priority-over-hover semantics.

type AppState = {
  collapsed: Set<string>;
  clickSelection: { type: 'node' | 'arc' | null; id: string | null };
  hoverSelection: { type: 'node' | 'arc' | null; id: string | null };
  hiddenArcIds: Set<string>;
  clusterMode: boolean;
  selectedScc: number | null;
  expandedRows: Set<string>;
};

const AppState = {
  /**
   * Create new AppState
   */
  create(): AppState {
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

  isCollapsed(state: AppState, nodeId: string): boolean {
    return state.collapsed.has(nodeId);
  },

  isRowExpanded(state: AppState, arcId: string): boolean {
    return state.expandedRows.has(arcId);
  },

  /**
   * Set collapsed state for node
   */
  setCollapsed(state: AppState, nodeId: string, collapsed: boolean) {
    if (collapsed) {
      state.collapsed.add(nodeId);
    } else {
      state.collapsed.delete(nodeId);
    }
  },

  /**
   * Toggle collapsed state
   * @returns - New collapsed state
   */
  toggleCollapsed(state: AppState, nodeId: string): boolean {
    const wasCollapsed = this.isCollapsed(state, nodeId);
    this.setCollapsed(state, nodeId, !wasCollapsed);
    return !wasCollapsed;
  },

  // === Selection Operations ===

  /**
   * Resolve the current selection with click-priority-over-hover semantics.
   * Returns clickSelection if present, otherwise hoverSelection, otherwise none.
   */
  getSelection(state: AppState): {
    mode: 'click' | 'hover' | 'none';
    type: string | null;
    id: string | null;
  } {
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
   */
  setSelection(state: AppState, type: 'node' | 'arc', id: string) {
    state.clickSelection = { type, id };
  },

  /**
   * Set hover selection (transient, does NOT touch clickSelection)
   */
  setHover(state: AppState, type: 'node' | 'arc', id: string) {
    state.hoverSelection = { type, id };
  },

  /**
   * Clear click selection only (hover selection is unaffected)
   */
  clearSelection(state: AppState) {
    state.clickSelection = { type: null, id: null };
  },

  /**
   * Clear hover selection only (click selection is unaffected)
   */
  clearHover(state: AppState) {
    state.hoverSelection = { type: null, id: null };
  },

  /**
   * Check if specific element is click-selected (pinned)
   */
  isSelected(state: AppState, type: 'node' | 'arc', id: string): boolean {
    return state.clickSelection.type === type && state.clickSelection.id === id;
  },

  /**
   * Check if anything is pinned (has a click selection)
   */
  hasPinnedSelection(state: AppState): boolean {
    return state.clickSelection.type !== null;
  },

  /**
   * Toggle click selection for element.
   * If same element is click-selected, deselects. Otherwise selects new element.
   * @returns - true if newly selected, false if deselected
   */
  toggleSelection(state: AppState, type: 'node' | 'arc', id: string): boolean {
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
  // that gate belongs to the derivation in derived_state.ts (it already reads
  // selectedScc + sccId there), not here, since hoverSelection is a generic
  // slot shared with non-cycle hovers.

  /**
   * Get the currently selected SCC (Ebene 1).
   */
  getSelectedScc(state: AppState): number | null {
    return state.selectedScc;
  },

  /**
   * Transition function for a click on an arc, cycle or not.
   * @param sccId - SCC of the clicked arc, if any
   */
  clickEdge(state: AppState, arcId: string, sccId: number | null | undefined) {
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
   */
  clickEmpty(state: AppState) {
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
   */
  _enterScc(state: AppState, sccId: number | null) {
    if (state.selectedScc === sccId) return;
    state.selectedScc = sccId;
    state.expandedRows.clear();
  },

  /**
   * Read a tangle row's current state from its two bits.
   */
  rowState(state: AppState, arcId: string): 'CU' | 'EU' | 'CP' | 'EP' {
    const expanded = this.isRowExpanded(state, arcId);
    const pinned = this.isSelected(state, 'arc', arcId);
    if (expanded) return pinned ? 'EP' : 'EU';
    return pinned ? 'CP' : 'CU';
  },

  /**
   * Write a row's next state back into expandedRows and the pin slot.
   */
  applyRowState(
    state: AppState,
    arcId: string,
    next: 'CU' | 'EU' | 'CP' | 'EP',
  ) {
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
   */
  rowToggle(state: AppState, arcId: string) {
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
   * @param sccId - SCC of the row's edge (open cluster)
   */
  rowPinClick(
    state: AppState,
    arcId: string,
    sccId: number | null | undefined,
  ) {
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
   */
  rowsExpandAll(state: AppState, arcIds: Iterable<string>) {
    for (const arcId of arcIds) state.expandedRows.add(arcId);
  },

  /**
   * Collapse every given row, whatever its pin.
   */
  rowsCollapseAll(state: AppState, arcIds: Iterable<string>) {
    for (const arcId of arcIds) state.expandedRows.delete(arcId);
  },

  /**
   * The editor's cursor landed on a row's jump target: expand it, pin
   * untouched.
   */
  rowFollow(state: AppState, arcId: string) {
    this.applyRowState(
      state,
      arcId,
      RowState[this.rowState(state, arcId)].follow(),
    );
  },

  // === Arc Filter Operations ===

  /**
   * Mark arc as hidden by filter
   */
  hideArc(state: AppState, arcId: string) {
    state.hiddenArcIds.add(arcId);
  },

  /**
   * Mark arc as visible (remove from hidden set)
   */
  showArc(state: AppState, arcId: string) {
    state.hiddenArcIds.delete(arcId);
  },

  isArcHidden(state: AppState, arcId: string): boolean {
    return state.hiddenArcIds.has(arcId);
  },

  /**
   * Get pinned (click) selection.
   */
  getPinned(state: AppState): null | { type: 'node' | 'arc'; id: string } {
    const { type, id } = state.clickSelection;
    if (type === null || id === null) return null;
    return { type, id };
  },

  // === Cluster Mode ===

  isClusterMode(state: AppState): boolean {
    return state.clusterMode;
  },

  setClusterMode(state: AppState, on: boolean) {
    state.clusterMode = on;
  },
};

export { AppState };
