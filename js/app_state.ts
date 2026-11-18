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
    };
  },

  // === Collapse Operations ===

  isCollapsed(state: AppState, nodeId: string): boolean {
    return state.collapsed.has(nodeId);
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

  /**
   * Click into empty space: reset pin and hover.
   */
  clickEmpty(state: AppState) {
    this.clearSelection(state);
    this.clearHover(state);
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
};

export { AppState };
