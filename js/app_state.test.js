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
