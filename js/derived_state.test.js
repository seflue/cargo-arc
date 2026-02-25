import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { AppState } from './app_state.js';
import { ArcLogic } from './arc_logic.js';
import { HighlightLogic } from './highlight_logic.js';
import { TreeLogic } from './tree_logic.js';

// Set globals (simulating browser environment where modules are loaded before derived_state.js)
global.TreeLogic = TreeLogic;
global.ArcLogic = ArcLogic;
global.HighlightLogic = HighlightLogic;
global.AppState = AppState;

import { DerivedState } from './derived_state.js';

// Test data representing a mini crate structure:
//
// crate
// ├── mod_a
// │   ├── fn_1
// │   └── fn_2
// └── mod_b
//     └── fn_3
//
// Arcs:
// fn_1 -> fn_2 (internal mod_a)
// fn_1 -> fn_3 (cross-module)
// mod_b -> mod_a (module-level)

const TEST_STATIC_DATA = {
  nodes: {
    crate: {
      type: 'crate',
      parent: null,
      x: 0,
      y: 0,
      width: 100,
      height: 24,
      hasChildren: true,
    },
    mod_a: {
      type: 'module',
      parent: 'crate',
      x: 20,
      y: 50,
      width: 100,
      height: 20,
      hasChildren: true,
    },
    mod_b: {
      type: 'module',
      parent: 'crate',
      x: 20,
      y: 150,
      width: 100,
      height: 20,
      hasChildren: true,
    },
    fn_1: {
      type: 'function',
      parent: 'mod_a',
      x: 40,
      y: 60,
      width: 100,
      height: 20,
      hasChildren: false,
    },
    fn_2: {
      type: 'function',
      parent: 'mod_a',
      x: 40,
      y: 80,
      width: 100,
      height: 20,
      hasChildren: false,
    },
    fn_3: {
      type: 'function',
      parent: 'mod_b',
      x: 40,
      y: 160,
      width: 100,
      height: 20,
      hasChildren: false,
    },
  },
  arcs: {
    'fn_1-fn_2': {
      from: 'fn_1',
      to: 'fn_2',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'helper',
          modulePath: null,
          locations: [{ file: 'mod_a.rs', line: 10 }],
        },
      ],
    },
    'fn_1-fn_3': {
      from: 'fn_1',
      to: 'fn_3',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'process',
          modulePath: null,
          locations: [
            { file: 'mod_a.rs', line: 15 },
            { file: 'mod_a.rs', line: 20 },
          ],
        },
      ],
    },
    'mod_b-mod_a': {
      from: 'mod_b',
      to: 'mod_a',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'init',
          modulePath: null,
          locations: [{ file: 'lib.rs', line: 5 }],
        },
      ],
    },
    'crate-mod_a': {
      from: 'crate',
      to: 'mod_a',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'mod_a',
          modulePath: null,
          locations: [{ file: 'lib.rs', line: 1 }],
        },
      ],
    },
    'crate-mod_b': {
      from: 'crate',
      to: 'mod_b',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'mod_b',
          modulePath: null,
          locations: [{ file: 'lib.rs', line: 2 }],
        },
      ],
    },
    'mod_a-fn_1': {
      from: 'mod_a',
      to: 'fn_1',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'fn_1',
          modulePath: null,
          locations: [{ file: 'mod_a.rs', line: 1 }],
        },
      ],
    },
    'mod_a-fn_2': {
      from: 'mod_a',
      to: 'fn_2',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'fn_2',
          modulePath: null,
          locations: [{ file: 'mod_a.rs', line: 2 }],
        },
      ],
    },
    'mod_b-fn_3': {
      from: 'mod_b',
      to: 'fn_3',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'fn_3',
          modulePath: null,
          locations: [{ file: 'mod_b.rs', line: 1 }],
        },
      ],
    },
  },
};

// Mock StaticData accessor for tests
function createMockStaticData(data = TEST_STATIC_DATA) {
  return {
    getNode: (id) => data.nodes[id],
    getArc: (id) => data.arcs[id],
    getOriginalPosition: (nodeId) => {
      const node = data.nodes[nodeId];
      return node
        ? { x: node.x, y: node.y, width: node.width, height: node.height }
        : null;
    },
    getAllNodeIds: () => Object.keys(data.nodes),
    getAllArcIds: () => Object.keys(data.arcs),
    hasChildren: (nodeId) => data.nodes[nodeId]?.hasChildren ?? false,
    getArcStrokeWidth: (arcId) => {
      const arc = data.arcs[arcId];
      if (!arc || !arc.usages) return ArcLogic.calculateStrokeWidth(0);
      const count = arc.usages.reduce((sum, g) => sum + g.locations.length, 0);
      return ArcLogic.calculateStrokeWidth(count);
    },
    buildParentMap: () => {
      const parentMap = new Map();
      for (const [nodeId, node] of Object.entries(data.nodes)) {
        if (node.parent !== null) {
          parentMap.set(nodeId, node.parent);
        }
      }
      return parentMap;
    },
  };
}

// Fixture for parent→child arc filtering tests.
// Parent `p` with 4 children: c1, c2, c3, c4
// p has arcs to c1 and c2 (directly used)
// c1→c3 exists (child-to-child, but c3 is NOT a direct target of p)
// No arcs from p to c3 or c4
const PARENT_ARC_DATA = {
  nodes: {
    p: {
      type: 'module',
      parent: null,
      x: 0,
      y: 0,
      width: 100,
      height: 20,
      hasChildren: true,
    },
    c1: {
      type: 'function',
      parent: 'p',
      x: 20,
      y: 30,
      width: 100,
      height: 20,
      hasChildren: false,
    },
    c2: {
      type: 'function',
      parent: 'p',
      x: 20,
      y: 60,
      width: 100,
      height: 20,
      hasChildren: false,
    },
    c3: {
      type: 'function',
      parent: 'p',
      x: 20,
      y: 90,
      width: 100,
      height: 20,
      hasChildren: false,
    },
    c4: {
      type: 'function',
      parent: 'p',
      x: 20,
      y: 120,
      width: 100,
      height: 20,
      hasChildren: false,
    },
  },
  arcs: {
    'p-c1': {
      from: 'p',
      to: 'c1',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'c1_fn',
          modulePath: null,
          locations: [{ file: 'p.rs', line: 1 }],
        },
      ],
    },
    'p-c2': {
      from: 'p',
      to: 'c2',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'c2_fn',
          modulePath: null,
          locations: [{ file: 'p.rs', line: 2 }],
        },
      ],
    },
    'c1-c3': {
      from: 'c1',
      to: 'c3',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'c3_fn',
          modulePath: null,
          locations: [{ file: 'c1.rs', line: 5 }],
        },
      ],
    },
  },
};

describe('DerivedState', () => {
  let staticData;

  beforeEach(() => {
    staticData = createMockStaticData();
  });

  describe('deriveNodeVisibility', () => {
    test('nothing collapsed: all nodes visible', () => {
      const collapsed = new Set();
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has('crate')).toBe(true);
      expect(result.has('mod_a')).toBe(true);
      expect(result.has('mod_b')).toBe(true);
      expect(result.has('fn_1')).toBe(true);
      expect(result.has('fn_2')).toBe(true);
      expect(result.has('fn_3')).toBe(true);
      expect(result.size).toBe(6);
    });

    test('collapsed node is visible', () => {
      const collapsed = new Set(['mod_a']);
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has('mod_a')).toBe(true);
    });

    test('children of collapsed node are hidden', () => {
      const collapsed = new Set(['mod_a']);
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has('fn_1')).toBe(false);
      expect(result.has('fn_2')).toBe(false);
    });

    test('siblings of collapsed node remain visible', () => {
      const collapsed = new Set(['mod_a']);
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has('mod_b')).toBe(true);
      expect(result.has('fn_3')).toBe(true);
    });

    test('deeply nested children are hidden', () => {
      // Collapse crate -> all children hidden
      const collapsed = new Set(['crate']);
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has('crate')).toBe(true);
      expect(result.has('mod_a')).toBe(false);
      expect(result.has('mod_b')).toBe(false);
      expect(result.has('fn_1')).toBe(false);
      expect(result.has('fn_2')).toBe(false);
      expect(result.has('fn_3')).toBe(false);
      expect(result.size).toBe(1);
    });
  });

  describe('computeCurrentPositions', () => {
    const MARGIN = 20;
    const TOOLBAR_HEIGHT = 40;
    const ROW_HEIGHT = 30;

    test('returns positions for all visible nodes', () => {
      const collapsed = new Set();
      const result = DerivedState.computeCurrentPositions(
        collapsed,
        staticData,
        MARGIN,
        TOOLBAR_HEIGHT,
        ROW_HEIGHT,
      );

      expect(result.size).toBe(6); // All nodes visible
      expect(result.has('crate')).toBe(true);
      expect(result.has('fn_1')).toBe(true);
    });

    test('positions start at margin + toolbar height', () => {
      const collapsed = new Set();
      const result = DerivedState.computeCurrentPositions(
        collapsed,
        staticData,
        MARGIN,
        TOOLBAR_HEIGHT,
        ROW_HEIGHT,
      );

      // First node should be at y = 20 + 40 = 60
      const firstY = Math.min(...[...result.values()].map((p) => p.y));
      expect(firstY).toBe(60);
    });

    test('positions are spaced by row height', () => {
      const collapsed = new Set();
      const result = DerivedState.computeCurrentPositions(
        collapsed,
        staticData,
        MARGIN,
        TOOLBAR_HEIGHT,
        ROW_HEIGHT,
      );

      const yValues = [...result.values()]
        .map((p) => p.y)
        .sort((a, b) => a - b);
      // Check spacing between consecutive nodes
      for (let i = 1; i < yValues.length; i++) {
        expect(yValues[i] - yValues[i - 1]).toBe(ROW_HEIGHT);
      }
    });

    test('collapsed children are not included', () => {
      const collapsed = new Set(['mod_a']);
      const result = DerivedState.computeCurrentPositions(
        collapsed,
        staticData,
        MARGIN,
        TOOLBAR_HEIGHT,
        ROW_HEIGHT,
      );

      expect(result.has('mod_a')).toBe(true); // Parent visible
      expect(result.has('fn_1')).toBe(false); // Child hidden
      expect(result.has('fn_2')).toBe(false); // Child hidden
    });

    test('preserves original x coordinate and dimensions', () => {
      const collapsed = new Set();
      const result = DerivedState.computeCurrentPositions(
        collapsed,
        staticData,
        MARGIN,
        TOOLBAR_HEIGHT,
        ROW_HEIGHT,
      );

      const modA = result.get('mod_a');
      expect(modA.x).toBe(20); // Original x
      expect(modA.width).toBe(100);
      expect(modA.height).toBe(20);
    });
  });

  describe('computeMaxRight', () => {
    test('returns max x + width', () => {
      const positions = new Map([
        ['a', { x: 10, width: 50 }],
        ['b', { x: 30, width: 80 }],
        ['c', { x: 5, width: 20 }],
      ]);

      expect(DerivedState.computeMaxRight(positions)).toBe(110); // 30 + 80
    });

    test('returns 0 for empty positions', () => {
      expect(DerivedState.computeMaxRight(new Map())).toBe(0);
    });
  });

  describe('deriveHighlightSet', () => {
    test('collapsed node → only {nodeId}', () => {
      const collapsed = new Set(['mod_a']);
      const result = DerivedState.deriveHighlightSet(
        'mod_a',
        collapsed,
        staticData,
      );

      expect(result.size).toBe(1);
      expect(result.has('mod_a')).toBe(true);
    });

    test('expanded parent → {nodeId, child1, child2, ...}', () => {
      const collapsed = new Set();
      const result = DerivedState.deriveHighlightSet(
        'mod_a',
        collapsed,
        staticData,
      );

      expect(result.has('mod_a')).toBe(true);
      expect(result.has('fn_1')).toBe(true);
      expect(result.has('fn_2')).toBe(true);
      expect(result.size).toBe(3);
    });

    test('nested expansion includes grandchildren', () => {
      // crate expanded, mod_a expanded, mod_b expanded → all descendants
      const collapsed = new Set();
      const result = DerivedState.deriveHighlightSet(
        'crate',
        collapsed,
        staticData,
      );

      expect(result.has('crate')).toBe(true);
      expect(result.has('mod_a')).toBe(true);
      expect(result.has('mod_b')).toBe(true);
      expect(result.has('fn_1')).toBe(true);
      expect(result.has('fn_2')).toBe(true);
      expect(result.has('fn_3')).toBe(true);
      expect(result.size).toBe(6);
    });

    test('partially collapsed subtree excludes hidden descendants', () => {
      // crate expanded, mod_a collapsed → mod_a visible but fn_1/fn_2 hidden
      const collapsed = new Set(['mod_a']);
      const result = DerivedState.deriveHighlightSet(
        'crate',
        collapsed,
        staticData,
      );

      expect(result.has('crate')).toBe(true);
      expect(result.has('mod_a')).toBe(true);
      expect(result.has('mod_b')).toBe(true);
      expect(result.has('fn_3')).toBe(true);
      // fn_1 and fn_2 are hidden (mod_a is collapsed)
      expect(result.has('fn_1')).toBe(false);
      expect(result.has('fn_2')).toBe(false);
      expect(result.size).toBe(4);
    });

    test('leaf node → only {nodeId}', () => {
      const collapsed = new Set();
      const result = DerivedState.deriveHighlightSet(
        'fn_1',
        collapsed,
        staticData,
      );

      expect(result.size).toBe(1);
      expect(result.has('fn_1')).toBe(true);
    });

    test('expanded parent includes only arc-connected descendants', () => {
      const sd = createMockStaticData(PARENT_ARC_DATA);
      const collapsed = new Set();
      const result = DerivedState.deriveHighlightSet('p', collapsed, sd);

      // p has arcs to c1 and c2 only
      expect(result.has('p')).toBe(true);
      expect(result.has('c1')).toBe(true);
      expect(result.has('c2')).toBe(true);
      // c3 and c4 are NOT direct targets of p
      expect(result.has('c3')).toBe(false);
      expect(result.has('c4')).toBe(false);
      expect(result.size).toBe(3);
    });

    test('expanded parent without outgoing arcs returns only {nodeId}', () => {
      // Parent with children but no outgoing arcs from parent itself
      const noArcData = {
        nodes: {
          parent: {
            type: 'module',
            parent: null,
            x: 0,
            y: 0,
            width: 100,
            height: 20,
            hasChildren: true,
          },
          child1: {
            type: 'function',
            parent: 'parent',
            x: 20,
            y: 30,
            width: 100,
            height: 20,
            hasChildren: false,
          },
          child2: {
            type: 'function',
            parent: 'parent',
            x: 20,
            y: 60,
            width: 100,
            height: 20,
            hasChildren: false,
          },
        },
        arcs: {
          // Only child-to-child arc, no parent→child arcs
          'child1-child2': {
            from: 'child1',
            to: 'child2',
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'helper',
                modulePath: null,
                locations: [{ file: 'test.rs', line: 1 }],
              },
            ],
          },
        },
      };
      const sd = createMockStaticData(noArcData);
      const collapsed = new Set();
      const result = DerivedState.deriveHighlightSet('parent', collapsed, sd);

      expect(result.has('parent')).toBe(true);
      expect(result.size).toBe(1);
    });
  });

  describe('deriveHighlightState', () => {
    const ROW_HEIGHT = 30;
    let appState;
    let positions;
    let emptyVirtualArcs;
    let emptyHidden;

    beforeEach(() => {
      appState = AppState.create();
      // Build positions for all nodes
      positions = new Map([
        ['crate', { x: 0, y: 60, width: 100, height: 24 }],
        ['mod_a', { x: 20, y: 90, width: 100, height: 20 }],
        ['mod_b', { x: 20, y: 120, width: 100, height: 20 }],
        ['fn_1', { x: 40, y: 150, width: 100, height: 20 }],
        ['fn_2', { x: 40, y: 180, width: 100, height: 20 }],
        ['fn_3', { x: 40, y: 210, width: 100, height: 20 }],
      ]);
      emptyVirtualArcs = new Map();
      emptyHidden = new Set();
    });

    test('no selection returns null', () => {
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );
      expect(result).toBeNull();
    });

    test("node click: selected node has 'current' role", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('fn_1')).toEqual({
        role: 'current',
        cssClass: 'selectedModule',
      });
    });

    test('node click: connected arcs in arcHighlights', () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      // fn_1 has outgoing arcs to fn_2 and fn_3
      expect(result.arcHighlights.has('fn_1-fn_2')).toBe(true);
      expect(result.arcHighlights.has('fn_1-fn_3')).toBe(true);
      expect(result.arcHighlights.has('mod_b-mod_a')).toBe(false);
    });

    test('node click: arc entries have correct structure', () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      const arcEntry = result.arcHighlights.get('fn_1-fn_2');
      expect(arcEntry.highlightWidth).toBeGreaterThan(0);
      expect(arcEntry.arrowScale).toBeGreaterThan(0);
      expect(arcEntry.relationType).toBeDefined();
      expect(arcEntry.isVirtual).toBe(false);
    });

    test('node click: endpoint nodes get dependency/dependent roles', () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      // fn_2 and fn_3 are targets of outgoing arcs
      expect(result.nodeHighlights.has('fn_2')).toBe(true);
      expect(result.nodeHighlights.has('fn_3')).toBe(true);
    });

    test('node click: crate type gets selectedCrate class', () => {
      AppState.setSelection(appState, 'node', 'crate');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result.nodeHighlights.get('crate')).toEqual({
        role: 'current',
        cssClass: 'selectedCrate',
      });
    });

    test('node click: shadow data generated for arcs with positions', () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      // fn_1-fn_2 has both endpoints in positions → shadow generated
      expect(result.shadowData.has('fn_1-fn_2')).toBe(true);
      const shadow = result.shadowData.get('fn_1-fn_2');
      expect(shadow.shadowWidth).toBeGreaterThan(0);
      expect(shadow.visibleLength).toBeGreaterThanOrEqual(0);
      expect(shadow.glowClass).toBeDefined();
    });

    test('node click: regular arcs promoted to hitareas', () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result.promotedHitareas.has('fn_1-fn_2')).toBe(true);
      expect(result.promotedHitareas.has('fn_1-fn_3')).toBe(true);
    });

    test("expanded parent: only parent's own arcs shown, child→external suppressed", () => {
      // mod_a expanded → highlightSet = {mod_a, fn_1, fn_2}
      // fn_1→fn_3: child→external, neither endpoint is mod_a → suppressed
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // Child→external arc suppressed (only parent's own arcs shown)
      expect(result.arcHighlights.has('fn_1-fn_3')).toBe(false);
      // mod_a's own arcs are shown
      expect(result.arcHighlights.has('mod_a-fn_1')).toBe(true);
      expect(result.arcHighlights.has('mod_a-fn_2')).toBe(true);
      // External→parent arc kept (mod_b→mod_a: to=selectionId)
      expect(result.arcHighlights.has('mod_b-mod_a')).toBe(true);
    });

    test('expanded parent: internal child-to-child arcs suppressed', () => {
      // mod_a expanded → highlightSet = {mod_a, fn_1, fn_2}
      // fn_1→fn_2: both in set → internal arc, should be suppressed
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // Internal child-to-child arc must NOT be highlighted in group mode
      expect(result.arcHighlights.has('fn_1-fn_2')).toBe(false);
    });

    test("expanded crate: only crate's own arcs shown", () => {
      // Extended fixture with ext_crate outside the crate
      const extData = {
        nodes: {
          ...TEST_STATIC_DATA.nodes,
          ext_crate: {
            type: 'crate',
            parent: null,
            x: 200,
            y: 300,
            width: 100,
            height: 24,
            hasChildren: false,
          },
        },
        arcs: {
          ...TEST_STATIC_DATA.arcs,
          'crate-ext_crate': {
            from: 'crate',
            to: 'ext_crate',
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'external_call',
                modulePath: null,
                locations: [{ file: 'lib.rs', line: 30 }],
              },
            ],
          },
        },
      };
      const sd = createMockStaticData(extData);
      const extPositions = new Map([
        ...positions,
        ['ext_crate', { x: 200, y: 300, width: 100, height: 24 }],
      ]);

      // crate expanded → highlightSet = {crate, mod_a, mod_b, fn_1, fn_2, fn_3}
      AppState.setSelection(appState, 'node', 'crate');
      const result = DerivedState.deriveHighlightState(
        appState,
        sd,
        emptyVirtualArcs,
        emptyHidden,
        extPositions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // Crate's own arc to ext_crate highlighted (from=selectionId)
      expect(result.arcHighlights.has('crate-ext_crate')).toBe(true);
      // Crate's own arcs to children highlighted
      expect(result.arcHighlights.has('crate-mod_a')).toBe(true);
      expect(result.arcHighlights.has('crate-mod_b')).toBe(true);
      // Child→child and child→external NOT highlighted (neither endpoint is crate)
      expect(result.arcHighlights.has('fn_1-fn_2')).toBe(false);
    });

    test('group highlight: expanded parent highlights descendants, suppresses internal arcs', () => {
      // mod_a is expanded → highlight set = {mod_a, fn_1, fn_2}
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // mod_a is current
      expect(result.nodeHighlights.get('mod_a').role).toBe('current');
      // fn_1-fn_2 is internal to highlight set → suppressed in group mode
      expect(result.arcHighlights.has('fn_1-fn_2')).toBe(false);
    });

    test('group highlight: children get group-member role', () => {
      // mod_a expanded → highlight set = {mod_a, fn_1, fn_2}
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // mod_a is the primary node → 'current' role
      expect(result.nodeHighlights.get('mod_a').role).toBe('current');
      // fn_1 and fn_2 are arc targets of mod_a → depNode (green border)
      expect(result.nodeHighlights.get('fn_1')).toEqual({
        role: 'dependency',
        cssClass: 'depNode',
      });
      expect(result.nodeHighlights.get('fn_2')).toEqual({
        role: 'dependency',
        cssClass: 'depNode',
      });
    });

    test('group highlight: only parent edges kept, all child arcs suppressed', () => {
      // mod_a expanded → highlight set = {mod_a, fn_1, fn_2}
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // Child arcs suppressed (neither endpoint is mod_a)
      expect(result.arcHighlights.has('fn_1-fn_2')).toBe(false);
      expect(result.arcHighlights.has('fn_1-fn_3')).toBe(false);
      // Parent's own arcs kept (from=selectionId)
      expect(result.arcHighlights.has('mod_a-fn_1')).toBe(true);
      expect(result.arcHighlights.has('mod_a-fn_2')).toBe(true);
      // External→parent: mod_b→mod_a kept (to=selectionId)
      expect(result.arcHighlights.has('mod_b-mod_a')).toBe(true);
    });

    test('group highlight: virtual arcs suppressed unless parent is endpoint', () => {
      // mod_a expanded → highlight set = {mod_a, fn_1, fn_2}
      // Virtual arc fn_1→fn_3: neither endpoint is mod_a → suppressed
      // Virtual arc mod_a→fn_3: from=selectionId → shown
      AppState.setSelection(appState, 'node', 'mod_a');
      const virtualArcUsages = new Map([
        [
          'fn_1-fn_3',
          [
            {
              symbol: 'virt',
              modulePath: null,
              locations: [{ file: 'v.rs', line: 1 }],
            },
          ],
        ],
        [
          'mod_a-fn_3',
          [
            {
              symbol: 'virt2',
              modulePath: null,
              locations: [{ file: 'v.rs', line: 2 }],
            },
          ],
        ],
      ]);

      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        virtualArcUsages,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // Child virtual arc suppressed (neither endpoint is mod_a)
      expect(result.arcHighlights.has('v:fn_1-fn_3')).toBe(false);
      // Parent virtual arc shown (from=selectionId)
      expect(result.arcHighlights.has('v:mod_a-fn_3')).toBe(true);
    });

    test('single-node selection: leaf shows all connected arcs (no group suppression)', () => {
      // fn_1 is a leaf → highlightSet = {fn_1} (size 1, NOT group mode)
      // fn_1→fn_2 and fn_1→fn_3 should both be highlighted
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('fn_1').role).toBe('current');
      // Both arcs included — no group suppression for single-node
      expect(result.arcHighlights.has('fn_1-fn_2')).toBe(true);
      expect(result.arcHighlights.has('fn_1-fn_3')).toBe(true);
      // Endpoint nodes get dependency roles
      expect(result.nodeHighlights.has('fn_2')).toBe(true);
      expect(result.nodeHighlights.has('fn_3')).toBe(true);
    });

    test('single-node selection: collapsed parent shows all connected arcs', () => {
      // mod_a collapsed → highlightSet = {mod_a} (size 1, NOT group mode)
      // mod_b→mod_a should be highlighted
      appState.collapsed = new Set(['mod_a']);
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('mod_a').role).toBe('current');
      // mod_b→mod_a: mod_a in set → should be highlighted (single-node, no suppression)
      expect(result.arcHighlights.has('mod_b-mod_a')).toBe(true);
    });

    test('hover vs click: click takes priority', () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      AppState.setHover(appState, 'node', 'fn_3');

      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      // Should use click selection (fn_1), not hover (fn_3)
      expect(result.nodeHighlights.get('fn_1').role).toBe('current');
      expect(result.arcHighlights.has('fn_1-fn_2')).toBe(true);
    });

    test('hover only: hover selection used when no click', () => {
      AppState.setHover(appState, 'node', 'fn_3');

      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('fn_3').role).toBe('current');
    });

    test('hiddenByFilter: filtered arcs excluded', () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const hidden = new Set(['fn_1-fn_2']);

      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        hidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result.arcHighlights.has('fn_1-fn_2')).toBe(false);
      // fn_1-fn_3 should still be present
      expect(result.arcHighlights.has('fn_1-fn_3')).toBe(true);
    });

    test('arc selection: both endpoints get roles', () => {
      AppState.setSelection(appState, 'arc', 'fn_1-fn_3');

      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        emptyVirtualArcs,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('fn_1').cssClass).toBe('dependentNode');
      expect(result.nodeHighlights.get('fn_3').cssClass).toBe('depNode');
      expect(result.arcHighlights.has('fn_1-fn_3')).toBe(true);
    });

    test('virtual arcs included when connected to highlight set', () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const virtualArcUsages = new Map([
        [
          'fn_1-fn_3',
          [
            {
              symbol: 'virt',
              modulePath: null,
              locations: [{ file: 'v.rs', line: 1 }],
            },
          ],
        ],
      ]);

      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        virtualArcUsages,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      // Virtual arc keyed with "v:" prefix
      expect(result.arcHighlights.has('v:fn_1-fn_3')).toBe(true);
      const vArc = result.arcHighlights.get('v:fn_1-fn_3');
      expect(vArc.isVirtual).toBe(true);
    });

    test('virtual arcs not promoted to hitareas', () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const virtualArcUsages = new Map([
        [
          'fn_1-fn_3',
          [
            {
              symbol: 'virt',
              modulePath: null,
              locations: [{ file: 'v.rs', line: 1 }],
            },
          ],
        ],
      ]);

      const result = DerivedState.deriveHighlightState(
        appState,
        staticData,
        virtualArcUsages,
        emptyHidden,
        positions,
        ROW_HEIGHT,
      );

      // Virtual arcs should NOT be in promotedHitareas
      expect(result.promotedHitareas.has('fn_1-fn_3')).toBe(true); // regular
      expect(result.promotedHitareas.has('v:fn_1-fn_3')).toBe(false); // virtual not promoted
    });

    test('test arc does not override node roles from production arc', () => {
      // Scenario: Production A→B, Test B→A
      // When B is selected, A should be "dependency" (from production A→B),
      // NOT "dependent" (from test B→A processed first).
      const testData = {
        nodes: {
          A: {
            type: 'module',
            parent: null,
            x: 20,
            y: 60,
            width: 100,
            height: 20,
            hasChildren: false,
          },
          B: {
            type: 'module',
            parent: null,
            x: 20,
            y: 90,
            width: 100,
            height: 20,
            hasChildren: false,
          },
        },
        arcs: {
          'A-B': {
            from: 'A',
            to: 'B',
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'prod_fn',
                modulePath: null,
                locations: [{ file: 'a.rs', line: 1 }],
              },
            ],
          },
          'B-A': {
            from: 'B',
            to: 'A',
            context: { kind: 'test', subKind: 'unit', features: [] },
            usages: [
              {
                symbol: 'test_fn',
                modulePath: null,
                locations: [{ file: 'b.rs', line: 1 }],
              },
            ],
          },
        },
      };
      const sd = createMockStaticData(testData);
      const testPositions = new Map([
        ['A', { x: 20, y: 60, width: 100, height: 20 }],
        ['B', { x: 20, y: 90, width: 100, height: 20 }],
      ]);

      const state = AppState.create();
      AppState.setSelection(state, 'node', 'B');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        testPositions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // B is selected
      expect(result.nodeHighlights.get('B').role).toBe('current');
      // Both arcs should be highlighted (test arc still gets visual highlight)
      expect(result.arcHighlights.has('A-B')).toBe(true);
      expect(result.arcHighlights.has('B-A')).toBe(true);
      // A should be "dependent" (from production A→B: A depends on B),
      // NOT "dependency" (test B→A must not override with wrong role)
      expect(result.nodeHighlights.get('A').role).toBe('dependent');
      expect(result.nodeHighlights.get('A').cssClass).toBe('dependentNode');
    });

    test('test arc gets visual highlight and marks endpoint nodes', () => {
      // A single test arc: when highlighted, it should appear in arcHighlights
      // AND mark endpoint nodes (non-production arcs highlight endpoints too)
      const testData = {
        nodes: {
          X: {
            type: 'module',
            parent: null,
            x: 20,
            y: 60,
            width: 100,
            height: 20,
            hasChildren: false,
          },
          Y: {
            type: 'module',
            parent: null,
            x: 20,
            y: 90,
            width: 100,
            height: 20,
            hasChildren: false,
          },
        },
        arcs: {
          'X-Y': {
            from: 'X',
            to: 'Y',
            context: { kind: 'test', subKind: 'unit', features: [] },
            usages: [
              {
                symbol: 'test_only',
                modulePath: null,
                locations: [{ file: 'x.rs', line: 1 }],
              },
            ],
          },
        },
      };
      const sd = createMockStaticData(testData);
      const testPositions = new Map([
        ['X', { x: 20, y: 60, width: 100, height: 20 }],
        ['Y', { x: 20, y: 90, width: 100, height: 20 }],
      ]);

      const state = AppState.create();
      AppState.setSelection(state, 'node', 'X');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        testPositions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // X is selected
      expect(result.nodeHighlights.get('X').role).toBe('current');
      // Test arc is visually highlighted
      expect(result.arcHighlights.has('X-Y')).toBe(true);
      // Y gets endpoint role from the test arc (non-production arcs mark endpoints)
      expect(result.nodeHighlights.get('Y').role).toBe('dependency');
      expect(result.nodeHighlights.get('Y').cssClass).toBe('depNode');
    });

    test('expanded parent: unconnected children not in nodeHighlights', () => {
      const sd = createMockStaticData(PARENT_ARC_DATA);
      const parentPositions = new Map([
        ['p', { x: 0, y: 60, width: 100, height: 20 }],
        ['c1', { x: 20, y: 90, width: 100, height: 20 }],
        ['c2', { x: 20, y: 120, width: 100, height: 20 }],
        ['c3', { x: 20, y: 150, width: 100, height: 20 }],
        ['c4', { x: 20, y: 180, width: 100, height: 20 }],
      ]);

      const state = AppState.create();
      AppState.setSelection(state, 'node', 'p');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        parentPositions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // p is current
      expect(result.nodeHighlights.get('p').role).toBe('current');
      // c1 and c2 are arc targets of p → depNode (green border)
      expect(result.nodeHighlights.get('c1')).toEqual({
        role: 'dependency',
        cssClass: 'depNode',
      });
      expect(result.nodeHighlights.get('c2')).toEqual({
        role: 'dependency',
        cssClass: 'depNode',
      });
      // c3 and c4 NOT in nodeHighlights (child arcs suppressed, only p's own arcs shown)
      expect(result.nodeHighlights.has('c3')).toBe(false);
      expect(result.nodeHighlights.has('c4')).toBe(false);
    });

    test("expanded parent: only parent's own arcs highlighted", () => {
      const sd = createMockStaticData(PARENT_ARC_DATA);
      const parentPositions = new Map([
        ['p', { x: 0, y: 60, width: 100, height: 20 }],
        ['c1', { x: 20, y: 90, width: 100, height: 20 }],
        ['c2', { x: 20, y: 120, width: 100, height: 20 }],
        ['c3', { x: 20, y: 150, width: 100, height: 20 }],
        ['c4', { x: 20, y: 180, width: 100, height: 20 }],
      ]);

      const state = AppState.create();
      AppState.setSelection(state, 'node', 'p');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        parentPositions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // Parent's own arcs: p→c1 and p→c2 highlighted
      expect(result.arcHighlights.has('p-c1')).toBe(true);
      expect(result.arcHighlights.has('p-c2')).toBe(true);
      // c1→c3: neither endpoint is p → suppressed in group mode
      expect(result.arcHighlights.has('c1-c3')).toBe(false);
    });
  });

  describe('cycle expansion (arc selection)', () => {
    const ROW_HEIGHT = 30;

    // Cycle: A → B → C → A (cycleIds=[0]), plus non-cycle A → D
    const CYCLE_DATA = {
      nodes: {
        A: {
          type: 'module',
          parent: null,
          x: 20,
          y: 60,
          width: 100,
          height: 20,
          hasChildren: false,
        },
        B: {
          type: 'module',
          parent: null,
          x: 20,
          y: 90,
          width: 100,
          height: 20,
          hasChildren: false,
        },
        C: {
          type: 'module',
          parent: null,
          x: 20,
          y: 120,
          width: 100,
          height: 20,
          hasChildren: false,
        },
        D: {
          type: 'module',
          parent: null,
          x: 20,
          y: 150,
          width: 100,
          height: 20,
          hasChildren: false,
        },
      },
      arcs: {
        'A-B': {
          from: 'A',
          to: 'B',
          cycleIds: [0],
          context: { kind: 'production', subKind: null, features: [] },
          usages: [
            {
              symbol: 'sym1',
              modulePath: null,
              locations: [{ file: 'a.rs', line: 1 }],
            },
          ],
        },
        'B-C': {
          from: 'B',
          to: 'C',
          cycleIds: [0],
          context: { kind: 'production', subKind: null, features: [] },
          usages: [
            {
              symbol: 'sym2',
              modulePath: null,
              locations: [
                { file: 'b.rs', line: 1 },
                { file: 'b.rs', line: 2 },
              ],
            },
          ],
        },
        'C-A': {
          from: 'C',
          to: 'A',
          cycleIds: [0],
          context: { kind: 'production', subKind: null, features: [] },
          usages: [
            {
              symbol: 'sym3',
              modulePath: null,
              locations: [{ file: 'c.rs', line: 1 }],
            },
          ],
        },
        'A-D': {
          from: 'A',
          to: 'D',
          context: { kind: 'production', subKind: null, features: [] },
          usages: [
            {
              symbol: 'sym4',
              modulePath: null,
              locations: [{ file: 'a.rs', line: 5 }],
            },
          ],
        },
      },
    };

    const CYCLE_POSITIONS = new Map([
      ['A', { x: 20, y: 60, width: 100, height: 20 }],
      ['B', { x: 20, y: 90, width: 100, height: 20 }],
      ['C', { x: 20, y: 120, width: 100, height: 20 }],
      ['D', { x: 20, y: 150, width: 100, height: 20 }],
    ]);

    let savedStaticData;

    beforeEach(() => {
      savedStaticData = globalThis.STATIC_DATA;
      globalThis.STATIC_DATA = {
        cycles: [{ nodes: ['A', 'B', 'C'], arcs: ['A-B', 'B-C', 'C-A'] }],
      };
    });

    afterEach(() => {
      globalThis.STATIC_DATA = savedStaticData;
    });

    test('cycle-arc selection: all cycle arcs in arcHighlights', () => {
      const sd = createMockStaticData(CYCLE_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'arc', 'A-B');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        CYCLE_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.arcHighlights.has('A-B')).toBe(true);
      expect(result.arcHighlights.has('B-C')).toBe(true);
      expect(result.arcHighlights.has('C-A')).toBe(true);
    });

    test('cycle-arc selection: cycle nodes get cycle-member role', () => {
      const sd = createMockStaticData(CYCLE_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'arc', 'A-B');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        CYCLE_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // Primary arc endpoints keep their roles
      expect(result.nodeHighlights.get('A').role).toBe('dependent');
      expect(result.nodeHighlights.get('B').role).toBe('dependency');
      // Other cycle node gets cycle-member
      expect(result.nodeHighlights.get('C')).toEqual({
        role: 'cycle-member',
        cssClass: 'cycleMember',
      });
    });

    test('non-cycle arc: no cycle expansion (regression)', () => {
      const sd = createMockStaticData(CYCLE_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'arc', 'A-D');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        CYCLE_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.arcHighlights.has('A-D')).toBe(true);
      expect(result.arcHighlights.has('A-B')).toBe(false);
      expect(result.arcHighlights.has('B-C')).toBe(false);
      expect(result.arcHighlights.has('C-A')).toBe(false);
      expect(result.nodeHighlights.has('C')).toBe(false);
    });

    test('cycle-arc selection: shadow uses glowCycle class', () => {
      const sd = createMockStaticData(CYCLE_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'arc', 'A-B');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        CYCLE_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // All cycle arcs should use glowCycle, not glowIncoming/glowOutgoing
      for (const cycleArcId of ['A-B', 'B-C', 'C-A']) {
        const shadow = result.shadowData.get(cycleArcId);
        expect(shadow).toBeDefined();
        expect(shadow.glowClass).toBe('glowCycle');
      }
    });

    test('non-cycle arc: shadow uses glowIncoming, not glowCycle', () => {
      const sd = createMockStaticData(CYCLE_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'arc', 'A-D');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        CYCLE_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      const shadow = result.shadowData.get('A-D');
      expect(shadow).toBeDefined();
      expect(shadow.glowClass).toBe('glowIncoming');
    });

    test('multi-cycle arc: union of all cycle members highlighted', () => {
      // Arc B-C belongs to both cycle 0 (A→B→C→A) and cycle 1 (B→C→E→B)
      const MULTI_CYCLE_DATA = {
        nodes: {
          A: {
            type: 'module',
            parent: null,
            x: 20,
            y: 60,
            width: 100,
            height: 20,
            hasChildren: false,
          },
          B: {
            type: 'module',
            parent: null,
            x: 20,
            y: 90,
            width: 100,
            height: 20,
            hasChildren: false,
          },
          C: {
            type: 'module',
            parent: null,
            x: 20,
            y: 120,
            width: 100,
            height: 20,
            hasChildren: false,
          },
          E: {
            type: 'module',
            parent: null,
            x: 20,
            y: 150,
            width: 100,
            height: 20,
            hasChildren: false,
          },
        },
        arcs: {
          'A-B': {
            from: 'A',
            to: 'B',
            cycleIds: [0],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym1',
                modulePath: null,
                locations: [{ file: 'a.rs', line: 1 }],
              },
            ],
          },
          'B-C': {
            from: 'B',
            to: 'C',
            cycleIds: [0, 1],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym2',
                modulePath: null,
                locations: [{ file: 'b.rs', line: 1 }],
              },
            ],
          },
          'C-A': {
            from: 'C',
            to: 'A',
            cycleIds: [0],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym3',
                modulePath: null,
                locations: [{ file: 'c.rs', line: 1 }],
              },
            ],
          },
          'C-E': {
            from: 'C',
            to: 'E',
            cycleIds: [1],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym4',
                modulePath: null,
                locations: [{ file: 'c.rs', line: 5 }],
              },
            ],
          },
          'E-B': {
            from: 'E',
            to: 'B',
            cycleIds: [1],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym5',
                modulePath: null,
                locations: [{ file: 'e.rs', line: 1 }],
              },
            ],
          },
        },
      };
      const multiPositions = new Map([
        ['A', { x: 20, y: 60, width: 100, height: 20 }],
        ['B', { x: 20, y: 90, width: 100, height: 20 }],
        ['C', { x: 20, y: 120, width: 100, height: 20 }],
        ['E', { x: 20, y: 150, width: 100, height: 20 }],
      ]);

      globalThis.STATIC_DATA = {
        cycles: [
          { nodes: ['A', 'B', 'C'], arcs: ['A-B', 'B-C', 'C-A'] },
          { nodes: ['B', 'C', 'E'], arcs: ['B-C', 'C-E', 'E-B'] },
        ],
      };

      const sd = createMockStaticData(MULTI_CYCLE_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'arc', 'B-C');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        multiPositions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // All arcs from both cycles should be highlighted
      expect(result.arcHighlights.has('B-C')).toBe(true);
      expect(result.arcHighlights.has('A-B')).toBe(true);
      expect(result.arcHighlights.has('C-A')).toBe(true);
      expect(result.arcHighlights.has('C-E')).toBe(true);
      expect(result.arcHighlights.has('E-B')).toBe(true);
      // All cycle nodes from union should be in nodeHighlights
      // B and C are primary arc endpoints
      expect(result.nodeHighlights.has('B')).toBe(true);
      expect(result.nodeHighlights.has('C')).toBe(true);
      // A and E are cycle-members from the two cycles
      expect(result.nodeHighlights.get('A')).toEqual({
        role: 'cycle-member',
        cssClass: 'cycleMember',
      });
      expect(result.nodeHighlights.get('E')).toEqual({
        role: 'cycle-member',
        cssClass: 'cycleMember',
      });
    });
  });

  describe('node hover direct-cycle', () => {
    const ROW_HEIGHT = 30;
    let savedStaticData;

    beforeEach(() => {
      savedStaticData = globalThis.STATIC_DATA;
    });

    afterEach(() => {
      globalThis.STATIC_DATA = savedStaticData;
    });

    test('direct-cycle node hover: partner gets cycle-member', () => {
      // Direct cycle: X ↔ Y (both directions, cycleIds=[0])
      const DIRECT_DATA = {
        nodes: {
          X: {
            type: 'module',
            parent: null,
            x: 20,
            y: 60,
            width: 100,
            height: 20,
            hasChildren: false,
          },
          Y: {
            type: 'module',
            parent: null,
            x: 20,
            y: 90,
            width: 100,
            height: 20,
            hasChildren: false,
          },
        },
        arcs: {
          'X-Y': {
            from: 'X',
            to: 'Y',
            cycleIds: [0],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym1',
                modulePath: null,
                locations: [{ file: 'x.rs', line: 1 }],
              },
            ],
          },
          'Y-X': {
            from: 'Y',
            to: 'X',
            cycleIds: [0],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym2',
                modulePath: null,
                locations: [{ file: 'y.rs', line: 1 }],
              },
            ],
          },
        },
      };
      globalThis.STATIC_DATA = {
        cycles: [{ nodes: ['X', 'Y'], arcs: ['X-Y', 'Y-X'] }],
      };
      const sd = createMockStaticData(DIRECT_DATA);
      const positions = new Map([
        ['X', { x: 20, y: 60, width: 100, height: 20 }],
        ['Y', { x: 20, y: 90, width: 100, height: 20 }],
      ]);
      const state = AppState.create();
      AppState.setHover(state, 'node', 'X');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('X').role).toBe('current');
      expect(result.nodeHighlights.get('Y')).toEqual({
        role: 'cycle-member',
        cssClass: 'cycleMember',
      });
    });

    test('transitive-only cycle: no cycle-member on partner', () => {
      // Transitive cycle: P → Q → R → P (no direct P↔Q)
      const TRANS_DATA = {
        nodes: {
          P: {
            type: 'module',
            parent: null,
            x: 20,
            y: 60,
            width: 100,
            height: 20,
            hasChildren: false,
          },
          Q: {
            type: 'module',
            parent: null,
            x: 20,
            y: 90,
            width: 100,
            height: 20,
            hasChildren: false,
          },
          R: {
            type: 'module',
            parent: null,
            x: 20,
            y: 120,
            width: 100,
            height: 20,
            hasChildren: false,
          },
        },
        arcs: {
          'P-Q': {
            from: 'P',
            to: 'Q',
            cycleIds: [0],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym1',
                modulePath: null,
                locations: [{ file: 'p.rs', line: 1 }],
              },
            ],
          },
          'Q-R': {
            from: 'Q',
            to: 'R',
            cycleIds: [0],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym2',
                modulePath: null,
                locations: [{ file: 'q.rs', line: 1 }],
              },
            ],
          },
          'R-P': {
            from: 'R',
            to: 'P',
            cycleIds: [0],
            context: { kind: 'production', subKind: null, features: [] },
            usages: [
              {
                symbol: 'sym3',
                modulePath: null,
                locations: [{ file: 'r.rs', line: 1 }],
              },
            ],
          },
        },
      };
      globalThis.STATIC_DATA = {
        cycles: [{ nodes: ['P', 'Q', 'R'], arcs: ['P-Q', 'Q-R', 'R-P'] }],
      };
      const sd = createMockStaticData(TRANS_DATA);
      const positions = new Map([
        ['P', { x: 20, y: 60, width: 100, height: 20 }],
        ['Q', { x: 20, y: 90, width: 100, height: 20 }],
        ['R', { x: 20, y: 120, width: 100, height: 20 }],
      ]);
      const state = AppState.create();
      AppState.setHover(state, 'node', 'P');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        positions,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('P').role).toBe('current');
      // No cycle-member on partners (transitive only)
      const qRole = result.nodeHighlights.get('Q')?.role;
      expect(qRole).not.toBe('cycle-member');
      const rRole = result.nodeHighlights.get('R')?.role;
      expect(rRole).not.toBe('cycle-member');
    });
  });

  describe('external node highlighting', () => {
    const ROW_HEIGHT = 30;

    // Fixture: internal module with arcs to external crates
    //
    // crate
    // └── mod_a
    //
    // external-section
    // ├── serde (external)
    // └── tokio (external)
    //
    // Arcs:
    // mod_a -> serde (production)
    // mod_a -> tokio (production)
    const EXTERNAL_DATA = {
      nodes: {
        crate: {
          type: 'crate',
          parent: null,
          x: 0,
          y: 0,
          width: 100,
          height: 24,
          hasChildren: true,
        },
        mod_a: {
          type: 'module',
          parent: 'crate',
          x: 20,
          y: 30,
          width: 100,
          height: 20,
          hasChildren: false,
        },
        ext_section: {
          type: 'external-section',
          parent: null,
          x: 0,
          y: 100,
          width: 100,
          height: 24,
          hasChildren: true,
        },
        serde: {
          type: 'external',
          parent: 'ext_section',
          x: 20,
          y: 130,
          width: 100,
          height: 20,
          hasChildren: false,
        },
        tokio: {
          type: 'external',
          parent: 'ext_section',
          x: 20,
          y: 160,
          width: 100,
          height: 20,
          hasChildren: false,
        },
      },
      arcs: {
        'mod_a-serde': {
          from: 'mod_a',
          to: 'serde',
          context: { kind: 'production', subKind: null, features: [] },
          usages: [
            {
              symbol: 'Serialize',
              modulePath: null,
              locations: [{ file: 'mod_a.rs', line: 1 }],
            },
          ],
        },
        'mod_a-tokio': {
          from: 'mod_a',
          to: 'tokio',
          context: { kind: 'production', subKind: null, features: [] },
          usages: [
            {
              symbol: 'spawn',
              modulePath: null,
              locations: [
                { file: 'mod_a.rs', line: 5 },
                { file: 'mod_a.rs', line: 10 },
              ],
            },
          ],
        },
        'crate-mod_a': {
          from: 'crate',
          to: 'mod_a',
          context: { kind: 'production', subKind: null, features: [] },
          usages: [
            {
              symbol: 'mod_a',
              modulePath: null,
              locations: [{ file: 'lib.rs', line: 1 }],
            },
          ],
        },
      },
    };

    const EXTERNAL_POSITIONS = new Map([
      ['crate', { x: 0, y: 60, width: 100, height: 24 }],
      ['mod_a', { x: 20, y: 90, width: 100, height: 20 }],
      ['ext_section', { x: 0, y: 120, width: 100, height: 24 }],
      ['serde', { x: 20, y: 150, width: 100, height: 20 }],
      ['tokio', { x: 20, y: 180, width: 100, height: 20 }],
    ]);

    test('click external crate: connected internal modules highlighted', () => {
      const sd = createMockStaticData(EXTERNAL_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'serde');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        EXTERNAL_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // serde is selected (external type gets selectedExternal class)
      expect(result.nodeHighlights.get('serde')).toEqual({
        role: 'current',
        cssClass: 'selectedExternal',
      });
      // mod_a→serde arc is highlighted
      expect(result.arcHighlights.has('mod_a-serde')).toBe(true);
      // mod_a is marked as dependent (it uses serde)
      expect(result.nodeHighlights.get('mod_a').cssClass).toBe('dependentNode');
    });

    test('click internal module: connected external crates highlighted', () => {
      const sd = createMockStaticData(EXTERNAL_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'mod_a');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        EXTERNAL_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('mod_a').role).toBe('current');
      // Both external arcs highlighted
      expect(result.arcHighlights.has('mod_a-serde')).toBe(true);
      expect(result.arcHighlights.has('mod_a-tokio')).toBe(true);
      // Both external crates marked as dependencies
      expect(result.nodeHighlights.get('serde').cssClass).toBe('depNode');
      expect(result.nodeHighlights.get('tokio').cssClass).toBe('depNode');
    });

    test('external arcs hidden by filter: excluded from highlights', () => {
      const sd = createMockStaticData(EXTERNAL_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'mod_a');
      const hidden = new Set(['mod_a-serde', 'mod_a-tokio']);

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        hidden,
        EXTERNAL_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      // External arcs filtered out
      expect(result.arcHighlights.has('mod_a-serde')).toBe(false);
      expect(result.arcHighlights.has('mod_a-tokio')).toBe(false);
      // Internal arc still visible
      expect(result.arcHighlights.has('crate-mod_a')).toBe(true);
    });

    test('click arc to external crate: both endpoints get roles', () => {
      const sd = createMockStaticData(EXTERNAL_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'arc', 'mod_a-serde');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        EXTERNAL_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('mod_a').cssClass).toBe('dependentNode');
      expect(result.nodeHighlights.get('serde').cssClass).toBe('depNode');
      expect(result.arcHighlights.has('mod_a-serde')).toBe(true);
    });

    test('transitive external crate gets selectedExternalTransitive class', () => {
      const transitiveData = structuredClone(EXTERNAL_DATA);
      transitiveData.nodes.tokio.type = 'external-transitive';
      const sd = createMockStaticData(transitiveData);
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'tokio');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        EXTERNAL_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('tokio')).toEqual({
        role: 'current',
        cssClass: 'selectedExternalTransitive',
      });
    });

    test('external-section node gets selectedExternal class', () => {
      const sd = createMockStaticData(EXTERNAL_DATA);
      const state = AppState.create();
      AppState.setSelection(state, 'node', 'ext_section');

      const result = DerivedState.deriveHighlightState(
        state,
        sd,
        new Map(),
        new Set(),
        EXTERNAL_POSITIONS,
        ROW_HEIGHT,
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get('ext_section')).toEqual({
        role: 'current',
        cssClass: 'selectedExternal',
      });
    });
  });
});
