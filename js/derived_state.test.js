import { test, expect, describe, beforeEach } from "bun:test";
import { TreeLogic } from "./tree_logic.js";
import { ArcLogic, ArrowLogic } from "./arc_logic.js";
import { HighlightLogic } from "./highlight_logic.js";
import { AppState } from "./app_state.js";

// Set globals (simulating browser environment where modules are loaded before derived_state.js)
global.TreeLogic = TreeLogic;
global.ArcLogic = ArcLogic;
global.ArrowLogic = ArrowLogic;
global.HighlightLogic = HighlightLogic;
global.AppState = AppState;

import { DerivedState } from "./derived_state.js";

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
    crate: { type: "crate", parent: null, x: 0, y: 0, width: 100, height: 24, hasChildren: true },
    mod_a: { type: "module", parent: "crate", x: 20, y: 50, width: 100, height: 20, hasChildren: true },
    mod_b: { type: "module", parent: "crate", x: 20, y: 150, width: 100, height: 20, hasChildren: true },
    fn_1: { type: "function", parent: "mod_a", x: 40, y: 60, width: 100, height: 20, hasChildren: false },
    fn_2: { type: "function", parent: "mod_a", x: 40, y: 80, width: 100, height: 20, hasChildren: false },
    fn_3: { type: "function", parent: "mod_b", x: 40, y: 160, width: 100, height: 20, hasChildren: false }
  },
  arcs: {
    "fn_1-fn_2": { from: "fn_1", to: "fn_2", usages: ["mod_a.rs:10"] },
    "fn_1-fn_3": { from: "fn_1", to: "fn_3", usages: ["mod_a.rs:15", "mod_a.rs:20"] },
    "mod_b-mod_a": { from: "mod_b", to: "mod_a", usages: ["lib.rs:5"] }
  }
};

// Mock StaticData accessor for tests
function createMockStaticData(data = TEST_STATIC_DATA) {
  return {
    getNode: (id) => data.nodes[id],
    getArc: (id) => data.arcs[id],
    getOriginalPosition: (nodeId) => {
      const node = data.nodes[nodeId];
      return node ? { x: node.x, y: node.y, width: node.width, height: node.height } : null;
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
    }
  };
}

describe("DerivedState", () => {
  let staticData;

  beforeEach(() => {
    staticData = createMockStaticData();
  });

  describe("deriveNodeVisibility", () => {
    test("nothing collapsed: all nodes visible", () => {
      const collapsed = new Set();
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has("crate")).toBe(true);
      expect(result.has("mod_a")).toBe(true);
      expect(result.has("mod_b")).toBe(true);
      expect(result.has("fn_1")).toBe(true);
      expect(result.has("fn_2")).toBe(true);
      expect(result.has("fn_3")).toBe(true);
      expect(result.size).toBe(6);
    });

    test("collapsed node is visible", () => {
      const collapsed = new Set(["mod_a"]);
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has("mod_a")).toBe(true);
    });

    test("children of collapsed node are hidden", () => {
      const collapsed = new Set(["mod_a"]);
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has("fn_1")).toBe(false);
      expect(result.has("fn_2")).toBe(false);
    });

    test("siblings of collapsed node remain visible", () => {
      const collapsed = new Set(["mod_a"]);
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has("mod_b")).toBe(true);
      expect(result.has("fn_3")).toBe(true);
    });

    test("deeply nested children are hidden", () => {
      // Collapse crate -> all children hidden
      const collapsed = new Set(["crate"]);
      const result = DerivedState.deriveNodeVisibility(collapsed, staticData);

      expect(result.has("crate")).toBe(true);
      expect(result.has("mod_a")).toBe(false);
      expect(result.has("mod_b")).toBe(false);
      expect(result.has("fn_1")).toBe(false);
      expect(result.has("fn_2")).toBe(false);
      expect(result.has("fn_3")).toBe(false);
      expect(result.size).toBe(1);
    });
  });

  describe("computeCurrentPositions", () => {
    const MARGIN = 20;
    const TOOLBAR_HEIGHT = 40;
    const ROW_HEIGHT = 30;

    test("returns positions for all visible nodes", () => {
      const collapsed = new Set();
      const result = DerivedState.computeCurrentPositions(
        collapsed, staticData, MARGIN, TOOLBAR_HEIGHT, ROW_HEIGHT
      );

      expect(result.size).toBe(6); // All nodes visible
      expect(result.has("crate")).toBe(true);
      expect(result.has("fn_1")).toBe(true);
    });

    test("positions start at margin + toolbar height", () => {
      const collapsed = new Set();
      const result = DerivedState.computeCurrentPositions(
        collapsed, staticData, MARGIN, TOOLBAR_HEIGHT, ROW_HEIGHT
      );

      // First node should be at y = 20 + 40 = 60
      const firstY = Math.min(...[...result.values()].map(p => p.y));
      expect(firstY).toBe(60);
    });

    test("positions are spaced by row height", () => {
      const collapsed = new Set();
      const result = DerivedState.computeCurrentPositions(
        collapsed, staticData, MARGIN, TOOLBAR_HEIGHT, ROW_HEIGHT
      );

      const yValues = [...result.values()].map(p => p.y).sort((a, b) => a - b);
      // Check spacing between consecutive nodes
      for (let i = 1; i < yValues.length; i++) {
        expect(yValues[i] - yValues[i - 1]).toBe(ROW_HEIGHT);
      }
    });

    test("collapsed children are not included", () => {
      const collapsed = new Set(["mod_a"]);
      const result = DerivedState.computeCurrentPositions(
        collapsed, staticData, MARGIN, TOOLBAR_HEIGHT, ROW_HEIGHT
      );

      expect(result.has("mod_a")).toBe(true); // Parent visible
      expect(result.has("fn_1")).toBe(false); // Child hidden
      expect(result.has("fn_2")).toBe(false); // Child hidden
    });

    test("preserves original x coordinate and dimensions", () => {
      const collapsed = new Set();
      const result = DerivedState.computeCurrentPositions(
        collapsed, staticData, MARGIN, TOOLBAR_HEIGHT, ROW_HEIGHT
      );

      const modA = result.get("mod_a");
      expect(modA.x).toBe(20); // Original x
      expect(modA.width).toBe(100);
      expect(modA.height).toBe(20);
    });
  });

  describe("computeMaxRight", () => {
    test("returns max x + width", () => {
      const positions = new Map([
        ["a", { x: 10, width: 50 }],
        ["b", { x: 30, width: 80 }],
        ["c", { x: 5, width: 20 }]
      ]);

      expect(DerivedState.computeMaxRight(positions)).toBe(110); // 30 + 80
    });

    test("returns 0 for empty positions", () => {
      expect(DerivedState.computeMaxRight(new Map())).toBe(0);
    });
  });

  describe("deriveHighlightSet", () => {
    test("collapsed node → only {nodeId}", () => {
      const collapsed = new Set(["mod_a"]);
      const result = DerivedState.deriveHighlightSet("mod_a", collapsed, staticData);

      expect(result.size).toBe(1);
      expect(result.has("mod_a")).toBe(true);
    });

    test("expanded parent → {nodeId, child1, child2, ...}", () => {
      const collapsed = new Set();
      const result = DerivedState.deriveHighlightSet("mod_a", collapsed, staticData);

      expect(result.has("mod_a")).toBe(true);
      expect(result.has("fn_1")).toBe(true);
      expect(result.has("fn_2")).toBe(true);
      expect(result.size).toBe(3);
    });

    test("nested expansion includes grandchildren", () => {
      // crate expanded, mod_a expanded, mod_b expanded → all descendants
      const collapsed = new Set();
      const result = DerivedState.deriveHighlightSet("crate", collapsed, staticData);

      expect(result.has("crate")).toBe(true);
      expect(result.has("mod_a")).toBe(true);
      expect(result.has("mod_b")).toBe(true);
      expect(result.has("fn_1")).toBe(true);
      expect(result.has("fn_2")).toBe(true);
      expect(result.has("fn_3")).toBe(true);
      expect(result.size).toBe(6);
    });

    test("partially collapsed subtree excludes hidden descendants", () => {
      // crate expanded, mod_a collapsed → mod_a visible but fn_1/fn_2 hidden
      const collapsed = new Set(["mod_a"]);
      const result = DerivedState.deriveHighlightSet("crate", collapsed, staticData);

      expect(result.has("crate")).toBe(true);
      expect(result.has("mod_a")).toBe(true);
      expect(result.has("mod_b")).toBe(true);
      expect(result.has("fn_3")).toBe(true);
      // fn_1 and fn_2 are hidden (mod_a is collapsed)
      expect(result.has("fn_1")).toBe(false);
      expect(result.has("fn_2")).toBe(false);
      expect(result.size).toBe(4);
    });

    test("leaf node → only {nodeId}", () => {
      const collapsed = new Set();
      const result = DerivedState.deriveHighlightSet("fn_1", collapsed, staticData);

      expect(result.size).toBe(1);
      expect(result.has("fn_1")).toBe(true);
    });
  });

  describe("deriveHighlightState", () => {
    const ROW_HEIGHT = 30;
    let appState;
    let positions;
    let emptyVirtualArcs;
    let emptyHidden;

    beforeEach(() => {
      appState = AppState.create();
      // Build positions for all nodes
      positions = new Map([
        ["crate", { x: 0, y: 60, width: 100, height: 24 }],
        ["mod_a", { x: 20, y: 90, width: 100, height: 20 }],
        ["mod_b", { x: 20, y: 120, width: 100, height: 20 }],
        ["fn_1", { x: 40, y: 150, width: 100, height: 20 }],
        ["fn_2", { x: 40, y: 180, width: 100, height: 20 }],
        ["fn_3", { x: 40, y: 210, width: 100, height: 20 }],
      ]);
      emptyVirtualArcs = new Map();
      emptyHidden = new Set();
    });

    test("no selection returns null", () => {
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );
      expect(result).toBeNull();
    });

    test("node click: selected node has 'current' role", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get("fn_1")).toEqual({
        role: 'current', cssClass: 'selectedModule'
      });
    });

    test("node click: connected arcs in arcHighlights", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      // fn_1 has outgoing arcs to fn_2 and fn_3
      expect(result.arcHighlights.has("fn_1-fn_2")).toBe(true);
      expect(result.arcHighlights.has("fn_1-fn_3")).toBe(true);
      expect(result.arcHighlights.has("mod_b-mod_a")).toBe(false);
    });

    test("node click: arc entries have correct structure", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      const arcEntry = result.arcHighlights.get("fn_1-fn_2");
      expect(arcEntry.highlightWidth).toBeGreaterThan(0);
      expect(arcEntry.arrowScale).toBeGreaterThan(0);
      expect(arcEntry.relationType).toBeDefined();
      expect(arcEntry.isVirtual).toBe(false);
    });

    test("node click: endpoint nodes get dependency/dependent roles", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      // fn_2 and fn_3 are targets of outgoing arcs
      expect(result.nodeHighlights.has("fn_2")).toBe(true);
      expect(result.nodeHighlights.has("fn_3")).toBe(true);
    });

    test("node click: crate type gets selectedCrate class", () => {
      AppState.setSelection(appState, 'node', 'crate');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result.nodeHighlights.get("crate")).toEqual({
        role: 'current', cssClass: 'selectedCrate'
      });
    });

    test("node click: shadow data generated for arcs with positions", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      // fn_1-fn_2 has both endpoints in positions → shadow generated
      expect(result.shadowData.has("fn_1-fn_2")).toBe(true);
      const shadow = result.shadowData.get("fn_1-fn_2");
      expect(shadow.shadowWidth).toBeGreaterThan(0);
      expect(shadow.visibleLength).toBeGreaterThanOrEqual(0);
      expect(shadow.glowClass).toBeDefined();
    });

    test("node click: regular arcs promoted to hitareas", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result.promotedHitareas.has("fn_1-fn_2")).toBe(true);
      expect(result.promotedHitareas.has("fn_1-fn_3")).toBe(true);
    });

    test("group highlight: expanded parent includes descendants", () => {
      // mod_a is expanded → highlight set = {mod_a, fn_1, fn_2}
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      // mod_a is current
      expect(result.nodeHighlights.get("mod_a").role).toBe("current");
      // fn_1-fn_2 is internal to highlight set
      expect(result.arcHighlights.has("fn_1-fn_2")).toBe(true);
    });

    test("group highlight: children get group-member role", () => {
      // mod_a expanded → highlight set = {mod_a, fn_1, fn_2}
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      // mod_a is the primary node → 'current' role
      expect(result.nodeHighlights.get("mod_a").role).toBe("current");
      // fn_1 and fn_2 are children → 'group-member' role
      expect(result.nodeHighlights.get("fn_1")).toEqual({
        role: 'group-member', cssClass: 'groupMember'
      });
      expect(result.nodeHighlights.get("fn_2")).toEqual({
        role: 'group-member', cssClass: 'groupMember'
      });
    });

    test("group highlight: child→external arcs suppressed", () => {
      // mod_a expanded → highlight set = {mod_a, fn_1, fn_2}
      // fn_1→fn_3 is external (fn_3 not in set) → should NOT be highlighted
      // fn_1→fn_2 is internal (both in set) → should be highlighted
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      // Internal arc: both endpoints in highlight set → included
      expect(result.arcHighlights.has("fn_1-fn_2")).toBe(true);
      // External arc: fn_1 in set, fn_3 not → suppressed in group mode
      expect(result.arcHighlights.has("fn_1-fn_3")).toBe(false);
      // mod_b→mod_a: mod_a in set, mod_b not → suppressed in group mode
      expect(result.arcHighlights.has("mod_b-mod_a")).toBe(false);
    });

    test("group highlight: virtual child→external arcs suppressed", () => {
      // mod_a expanded → highlight set = {mod_a, fn_1, fn_2}
      // Virtual arc fn_1→fn_3: fn_3 not in set → should NOT be highlighted
      AppState.setSelection(appState, 'node', 'mod_a');
      const virtualArcUsages = new Map([
        ["fn_1-fn_3", [
          { symbol: "virt", modulePath: null, locations: [{ file: "v.rs", line: 1 }] }
        ]]
      ]);

      const result = DerivedState.deriveHighlightState(
        appState, staticData, virtualArcUsages, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      // Virtual external arc suppressed in group mode
      expect(result.arcHighlights.has("v:fn_1-fn_3")).toBe(false);
    });

    test("single-node selection: leaf shows all connected arcs (no group suppression)", () => {
      // fn_1 is a leaf → highlightSet = {fn_1} (size 1, NOT group mode)
      // fn_1→fn_2 and fn_1→fn_3 should both be highlighted
      AppState.setSelection(appState, 'node', 'fn_1');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get("fn_1").role).toBe("current");
      // Both arcs included — no group suppression for single-node
      expect(result.arcHighlights.has("fn_1-fn_2")).toBe(true);
      expect(result.arcHighlights.has("fn_1-fn_3")).toBe(true);
      // Endpoint nodes get dependency roles
      expect(result.nodeHighlights.has("fn_2")).toBe(true);
      expect(result.nodeHighlights.has("fn_3")).toBe(true);
    });

    test("single-node selection: collapsed parent shows all connected arcs", () => {
      // mod_a collapsed → highlightSet = {mod_a} (size 1, NOT group mode)
      // mod_b→mod_a should be highlighted
      appState.collapsed = new Set(["mod_a"]);
      AppState.setSelection(appState, 'node', 'mod_a');
      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get("mod_a").role).toBe("current");
      // mod_b→mod_a: mod_a in set → should be highlighted (single-node, no suppression)
      expect(result.arcHighlights.has("mod_b-mod_a")).toBe(true);
    });

    test("hover vs click: click takes priority", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      AppState.setHover(appState, 'node', 'fn_3');

      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      // Should use click selection (fn_1), not hover (fn_3)
      expect(result.nodeHighlights.get("fn_1").role).toBe("current");
      expect(result.arcHighlights.has("fn_1-fn_2")).toBe(true);
    });

    test("hover only: hover selection used when no click", () => {
      AppState.setHover(appState, 'node', 'fn_3');

      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get("fn_3").role).toBe("current");
    });

    test("hiddenByFilter: filtered arcs excluded", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const hidden = new Set(["fn_1-fn_2"]);

      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, hidden, positions, ROW_HEIGHT
      );

      expect(result.arcHighlights.has("fn_1-fn_2")).toBe(false);
      // fn_1-fn_3 should still be present
      expect(result.arcHighlights.has("fn_1-fn_3")).toBe(true);
    });

    test("arc selection: both endpoints get roles", () => {
      AppState.setSelection(appState, 'arc', 'fn_1-fn_3');

      const result = DerivedState.deriveHighlightState(
        appState, staticData, emptyVirtualArcs, emptyHidden, positions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      expect(result.nodeHighlights.get("fn_1").cssClass).toBe("dependentNode");
      expect(result.nodeHighlights.get("fn_3").cssClass).toBe("depNode");
      expect(result.arcHighlights.has("fn_1-fn_3")).toBe(true);
    });

    test("virtual arcs included when connected to highlight set", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const virtualArcUsages = new Map([
        ["fn_1-fn_3", [
          { symbol: "virt", modulePath: null, locations: [{ file: "v.rs", line: 1 }] }
        ]]
      ]);

      const result = DerivedState.deriveHighlightState(
        appState, staticData, virtualArcUsages, emptyHidden, positions, ROW_HEIGHT
      );

      // Virtual arc keyed with "v:" prefix
      expect(result.arcHighlights.has("v:fn_1-fn_3")).toBe(true);
      const vArc = result.arcHighlights.get("v:fn_1-fn_3");
      expect(vArc.isVirtual).toBe(true);
    });

    test("virtual arcs not promoted to hitareas", () => {
      AppState.setSelection(appState, 'node', 'fn_1');
      const virtualArcUsages = new Map([
        ["fn_1-fn_3", [
          { symbol: "virt", modulePath: null, locations: [{ file: "v.rs", line: 1 }] }
        ]]
      ]);

      const result = DerivedState.deriveHighlightState(
        appState, staticData, virtualArcUsages, emptyHidden, positions, ROW_HEIGHT
      );

      // Virtual arcs should NOT be in promotedHitareas
      expect(result.promotedHitareas.has("fn_1-fn_3")).toBe(true);  // regular
      expect(result.promotedHitareas.has("v:fn_1-fn_3")).toBe(false); // virtual not promoted
    });

    test("test arc does not override node roles from production arc", () => {
      // Scenario: Production A→B, Test B→A
      // When B is selected, A should be "dependency" (from production A→B),
      // NOT "dependent" (from test B→A processed first).
      const testData = {
        nodes: {
          A: { type: "module", parent: null, x: 20, y: 60, width: 100, height: 20, hasChildren: false },
          B: { type: "module", parent: null, x: 20, y: 90, width: 100, height: 20, hasChildren: false },
        },
        arcs: {
          "A-B": { from: "A", to: "B", isTest: false, usages: [
            { symbol: "prod_fn", modulePath: null, locations: [{ file: "a.rs", line: 1 }] }
          ]},
          "B-A": { from: "B", to: "A", isTest: true, usages: [
            { symbol: "test_fn", modulePath: null, locations: [{ file: "b.rs", line: 1 }] }
          ]}
        }
      };
      const sd = createMockStaticData(testData);
      const testPositions = new Map([
        ["A", { x: 20, y: 60, width: 100, height: 20 }],
        ["B", { x: 20, y: 90, width: 100, height: 20 }],
      ]);

      const state = AppState.create();
      AppState.setSelection(state, 'node', 'B');

      const result = DerivedState.deriveHighlightState(
        state, sd, new Map(), new Set(), testPositions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      // B is selected
      expect(result.nodeHighlights.get("B").role).toBe("current");
      // Both arcs should be highlighted (test arc still gets visual highlight)
      expect(result.arcHighlights.has("A-B")).toBe(true);
      expect(result.arcHighlights.has("B-A")).toBe(true);
      // A should be "dependent" (from production A→B: A depends on B),
      // NOT "dependency" (test B→A must not override with wrong role)
      expect(result.nodeHighlights.get("A").role).toBe("dependent");
      expect(result.nodeHighlights.get("A").cssClass).toBe("dependentNode");
    });

    test("test arc gets visual highlight but no node role assignment", () => {
      // A single test arc: when highlighted, it should appear in arcHighlights
      // but must NOT mark endpoint nodes
      const testData = {
        nodes: {
          X: { type: "module", parent: null, x: 20, y: 60, width: 100, height: 20, hasChildren: false },
          Y: { type: "module", parent: null, x: 20, y: 90, width: 100, height: 20, hasChildren: false },
        },
        arcs: {
          "X-Y": { from: "X", to: "Y", isTest: true, usages: [
            { symbol: "test_only", modulePath: null, locations: [{ file: "x.rs", line: 1 }] }
          ]}
        }
      };
      const sd = createMockStaticData(testData);
      const testPositions = new Map([
        ["X", { x: 20, y: 60, width: 100, height: 20 }],
        ["Y", { x: 20, y: 90, width: 100, height: 20 }],
      ]);

      const state = AppState.create();
      AppState.setSelection(state, 'node', 'X');

      const result = DerivedState.deriveHighlightState(
        state, sd, new Map(), new Set(), testPositions, ROW_HEIGHT
      );

      expect(result).not.toBeNull();
      // X is selected
      expect(result.nodeHighlights.get("X").role).toBe("current");
      // Test arc is visually highlighted
      expect(result.arcHighlights.has("X-Y")).toBe(true);
      // Y should NOT get a node role from the test arc
      expect(result.nodeHighlights.has("Y")).toBe(false);
    });
  });
});
