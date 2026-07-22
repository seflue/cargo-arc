import { describe, expect, test } from 'bun:test';
import { AppState } from './app_state.js';

describe('AppState', () => {
  describe('create', () => {
    test('creates state with empty collapsed set', () => {
      const state = AppState.create();
      expect(state.collapsed).toBeInstanceOf(Set);
      expect(state.collapsed.size).toBe(0);
    });

    test('creates state with default selection', () => {
      const state = AppState.create();
      expect(state.clickSelection).toEqual({ type: null, id: null });
      expect(state.hoverSelection).toEqual({ type: null, id: null });
      expect(AppState.getSelection(state)).toEqual({
        mode: 'none',
        type: null,
        id: null,
      });
    });
  });

  describe('collapse operations', () => {
    test('isCollapsed returns false by default', () => {
      const state = AppState.create();
      expect(AppState.isCollapsed(state, 'any-node')).toBe(false);
    });

    test('setCollapsed adds to set when true', () => {
      const state = AppState.create();
      AppState.setCollapsed(state, 'node1', true);
      expect(AppState.isCollapsed(state, 'node1')).toBe(true);
    });

    test('setCollapsed removes from set when false', () => {
      const state = AppState.create();
      AppState.setCollapsed(state, 'node1', true);
      AppState.setCollapsed(state, 'node1', false);
      expect(AppState.isCollapsed(state, 'node1')).toBe(false);
    });

    test('toggleCollapsed changes state and returns new value', () => {
      const state = AppState.create();

      // First toggle: false -> true
      const result1 = AppState.toggleCollapsed(state, 'node1');
      expect(result1).toBe(true);
      expect(AppState.isCollapsed(state, 'node1')).toBe(true);

      // Second toggle: true -> false
      const result2 = AppState.toggleCollapsed(state, 'node1');
      expect(result2).toBe(false);
      expect(AppState.isCollapsed(state, 'node1')).toBe(false);
    });
  });

  describe('selection operations', () => {
    test('getSelection returns default selection', () => {
      const state = AppState.create();
      expect(AppState.getSelection(state)).toEqual({
        mode: 'none',
        type: null,
        id: null,
      });
    });

    test('setSelection sets click mode', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'node-1');
      expect(AppState.getSelection(state)).toEqual({
        mode: 'click',
        type: 'node',
        id: 'node-1',
      });
    });

    test('setHover sets hover mode', () => {
      const state = AppState.create();
      AppState.setHover(state, 'arc', '1-2');
      expect(AppState.getSelection(state)).toEqual({
        mode: 'hover',
        type: 'arc',
        id: '1-2',
      });
    });

    test('clearSelection resets to none', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'node-1');
      AppState.clearSelection(state);
      expect(AppState.getSelection(state)).toEqual({
        mode: 'none',
        type: null,
        id: null,
      });
    });

    test('isSelected returns true for matching click selection', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'node-1');
      expect(AppState.isSelected(state, 'node', 'node-1')).toBe(true);
    });

    test('isSelected returns false for different id', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'node-1');
      expect(AppState.isSelected(state, 'node', 'node-2')).toBe(false);
    });

    test('isSelected returns false for different type', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'node-1');
      expect(AppState.isSelected(state, 'arc', 'node-1')).toBe(false);
    });

    test('isSelected returns false for hover mode', () => {
      const state = AppState.create();
      AppState.setHover(state, 'node', 'node-1');
      expect(AppState.isSelected(state, 'node', 'node-1')).toBe(false);
    });

    test('hasPinnedSelection returns true for click mode', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'node-1');
      expect(AppState.hasPinnedSelection(state)).toBe(true);
    });

    test('hasPinnedSelection returns false for hover mode', () => {
      const state = AppState.create();
      AppState.setHover(state, 'node', 'node-1');
      expect(AppState.hasPinnedSelection(state)).toBe(false);
    });

    test('hasPinnedSelection returns false for none mode', () => {
      const state = AppState.create();
      expect(AppState.hasPinnedSelection(state)).toBe(false);
    });

    test('toggleSelection selects when not selected', () => {
      const state = AppState.create();
      const result = AppState.toggleSelection(state, 'node', 'node-1');
      expect(result).toBe(true);
      expect(AppState.isSelected(state, 'node', 'node-1')).toBe(true);
    });

    test('toggleSelection deselects when already selected', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'node-1');
      const result = AppState.toggleSelection(state, 'node', 'node-1');
      expect(result).toBe(false);
      expect(AppState.isSelected(state, 'node', 'node-1')).toBe(false);
    });

    test('toggleSelection switches to new element', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'node-1');
      const result = AppState.toggleSelection(state, 'node', 'node-2');
      expect(result).toBe(true);
      expect(AppState.isSelected(state, 'node', 'node-2')).toBe(true);
      expect(AppState.isSelected(state, 'node', 'node-1')).toBe(false);
    });

    test('click selection takes priority over hover', () => {
      const state = AppState.create();
      AppState.setHover(state, 'node', 'hover-node');
      AppState.setSelection(state, 'node', 'click-node');
      const sel = AppState.getSelection(state);
      expect(sel.mode).toBe('click');
      expect(sel.id).toBe('click-node');
    });

    test('clearHover removes hover without affecting click', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'pinned');
      AppState.setHover(state, 'arc', '1-2');
      AppState.clearHover(state);
      const sel = AppState.getSelection(state);
      expect(sel.mode).toBe('click');
      expect(sel.id).toBe('pinned');
    });

    test('clearHover with no click returns none', () => {
      const state = AppState.create();
      AppState.setHover(state, 'node', 'tmp');
      AppState.clearHover(state);
      expect(AppState.getSelection(state).mode).toBe('none');
    });
  });

  describe('legacy API compatibility', () => {
    test('getPinned returns null when no selection', () => {
      const state = AppState.create();
      expect(AppState.getPinned(state)).toBeNull();
    });

    test('getPinned returns object when selected', () => {
      const state = AppState.create();
      AppState.setSelection(state, 'arc', '1-2');
      expect(AppState.getPinned(state)).toEqual({ type: 'arc', id: '1-2' });
    });

    test('getPinned returns null for hover mode', () => {
      const state = AppState.create();
      AppState.setHover(state, 'node', 'node-1');
      expect(AppState.getPinned(state)).toBeNull();
    });
  });

  describe('deselect clears stale hover (ca-0301 regression)', () => {
    test('clearSelection without clearHover leaves stale hover', () => {
      const state = AppState.create();
      AppState.setHover(state, 'node', 'A');
      AppState.setSelection(state, 'node', 'A');
      AppState.clearSelection(state);
      const sel = AppState.getSelection(state);
      expect(sel.mode).toBe('hover');
      expect(sel.id).toBe('A');
    });

    test('full deselect clears both click and hover', () => {
      const state = AppState.create();
      AppState.setHover(state, 'node', 'A');
      AppState.setSelection(state, 'node', 'A');
      AppState.clearSelection(state);
      AppState.clearHover(state);
      const sel = AppState.getSelection(state);
      expect(sel.mode).toBe('none');
    });
  });

  describe('arc filter operations', () => {
    test('hideArc/showArc/isArcHidden', () => {
      const state = AppState.create();
      expect(AppState.isArcHidden(state, '1-2')).toBe(false);
      AppState.hideArc(state, '1-2');
      expect(AppState.isArcHidden(state, '1-2')).toBe(true);
      AppState.showArc(state, '1-2');
      expect(AppState.isArcHidden(state, '1-2')).toBe(false);
    });
  });

  describe('cycle-mode selection (selectedScc)', () => {
    test('create defaults selectedScc to null', () => {
      const state = AppState.create();
      expect(AppState.getSelectedScc(state)).toBeNull();
    });

    test('clickEdge on cycle edge with nothing selected sets selectedScc, no pin', () => {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7);
      expect(AppState.getSelectedScc(state)).toBe(7);
      expect(AppState.hasPinnedSelection(state)).toBe(false);
    });

    test('clickEdge on same SCC pins the edge', () => {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7);
      AppState.clickEdge(state, '1-2', 7);
      expect(AppState.getSelectedScc(state)).toBe(7);
      expect(AppState.isSelected(state, 'arc', '1-2')).toBe(true);
    });

    test('clickEdge on same pinned edge again unpins it, SCC stays selected', () => {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7);
      AppState.clickEdge(state, '1-2', 7);
      AppState.clickEdge(state, '1-2', 7);
      expect(AppState.getSelectedScc(state)).toBe(7);
      expect(AppState.hasPinnedSelection(state)).toBe(false);
    });

    test('clickEdge on a different SCC switches selection and clears the pin', () => {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7);
      AppState.clickEdge(state, '1-2', 7); // pin edge in SCC 7
      AppState.clickEdge(state, '3-4', 9); // different SCC
      expect(AppState.getSelectedScc(state)).toBe(9);
      expect(AppState.hasPinnedSelection(state)).toBe(false);
    });

    test('clickEmpty resets selectedScc, pin and hover', () => {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7);
      AppState.clickEdge(state, '1-2', 7);
      AppState.setHover(state, 'arc', '3-4');
      AppState.clickEmpty(state);
      expect(AppState.getSelectedScc(state)).toBeNull();
      expect(AppState.getSelection(state)).toEqual({
        mode: 'none',
        type: null,
        id: null,
      });
    });

    test('clickEdge on a non-cycle edge clears selectedScc and toggles normal selection', () => {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7);
      AppState.clickEdge(state, '5-6', null);
      expect(AppState.getSelectedScc(state)).toBeNull();
      expect(AppState.isSelected(state, 'arc', '5-6')).toBe(true);

      AppState.clickEdge(state, '5-6', null);
      expect(AppState.hasPinnedSelection(state)).toBe(false);
    });

    test('pin takes priority over hover once a cycle edge is pinned', () => {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7);
      AppState.clickEdge(state, '1-2', 7); // pin '1-2'
      AppState.setHover(state, 'arc', '3-4');
      const sel = AppState.getSelection(state);
      expect(sel.mode).toBe('click');
      expect(sel.id).toBe('1-2');
    });

    test('first click clears the hover a preceding mouseenter set', () => {
      const state = AppState.create();
      // Real mouse flow: mouseenter fires before click, hovering this edge.
      AppState.setHover(state, 'arc', '1-2');
      AppState.clickEdge(state, '1-2', 7);
      expect(AppState.getSelectedScc(state)).toBe(7);
      // Overview, not focus: the lingering hover is gone.
      expect(AppState.getSelection(state).mode).toBe('none');
    });

    test('unpinning returns to the overview by clearing the lingering hover', () => {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7); // select SCC
      AppState.clickEdge(state, '1-2', 7); // pin
      AppState.setHover(state, 'arc', '1-2'); // mouse still on the edge
      AppState.clickEdge(state, '1-2', 7); // unpin
      expect(AppState.getSelection(state).mode).toBe('none');
    });

    test('switching SCC clears the hover from the old edge', () => {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7);
      AppState.setHover(state, 'arc', '3-4');
      AppState.clickEdge(state, '3-4', 9); // switch SCC
      expect(AppState.getSelectedScc(state)).toBe(9);
      expect(AppState.getSelection(state).mode).toBe('none');
    });
  });

  describe('clickClusterRow (sidebar row: pin couples to expand)', () => {
    // All start with the SCC already selected (cluster view open).
    function selected() {
      const state = AppState.create();
      AppState.clickEdge(state, '1-2', 7);
      return state;
    }

    test('collapsed + unpinned -> pins, ends expanded', () => {
      const state = selected();
      const end = AppState.clickClusterRow(state, '1-2', 7, true, false);
      expect(AppState.isSelected(state, 'arc', '1-2')).toBe(true);
      expect(end).toBe(true);
    });

    test('expanded + unpinned -> pins, stays expanded', () => {
      const state = selected();
      const end = AppState.clickClusterRow(state, '1-2', 7, true, true);
      expect(AppState.isSelected(state, 'arc', '1-2')).toBe(true);
      expect(end).toBe(true);
    });

    test('expanded + pinned -> unpins, collapses', () => {
      const state = selected();
      AppState.clickClusterRow(state, '1-2', 7, true, false); // pin
      const end = AppState.clickClusterRow(state, '1-2', 7, true, true);
      expect(AppState.isSelected(state, 'arc', '1-2')).toBe(false);
      expect(end).toBe(false);
    });

    test('collapsed + pinned (after collapse-all) -> keeps pin, only re-expands', () => {
      const state = selected();
      AppState.clickClusterRow(state, '1-2', 7, true, false); // pin
      const end = AppState.clickClusterRow(state, '1-2', 7, true, false);
      expect(AppState.isSelected(state, 'arc', '1-2')).toBe(true);
      expect(end).toBe(true);
    });

    test('non-expandable row toggles the pin normally', () => {
      const state = selected();
      AppState.clickClusterRow(state, '1-2', 7, false, false); // pin
      expect(AppState.isSelected(state, 'arc', '1-2')).toBe(true);
      const end = AppState.clickClusterRow(state, '1-2', 7, false, false);
      expect(AppState.isSelected(state, 'arc', '1-2')).toBe(false); // unpin
      expect(end).toBe(false);
    });
  });

  describe('cluster mode', () => {
    test('defaults to on', () => {
      const state = AppState.create();
      expect(AppState.isClusterMode(state)).toBe(true);
    });

    test('setClusterMode updates the flag', () => {
      const state = AppState.create();
      AppState.setClusterMode(state, false);
      expect(AppState.isClusterMode(state)).toBe(false);
      AppState.setClusterMode(state, true);
      expect(AppState.isClusterMode(state)).toBe(true);
    });
  });
});
