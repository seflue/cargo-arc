import { test, expect, describe, beforeEach } from "bun:test";
import { TreeLogic } from "./tree_logic.js";

// Set TreeLogic globally (simulating browser environment where it's loaded before derived_state.js)
global.TreeLogic = TreeLogic;

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
    getParent: (nodeId) => data.nodes[nodeId]?.parent ?? null,
    getOriginalPosition: (nodeId) => {
      const node = data.nodes[nodeId];
      return node ? { x: node.x, y: node.y, width: node.width, height: node.height } : null;
    },
    getAllNodeIds: () => Object.keys(data.nodes),
    getAllArcIds: () => Object.keys(data.arcs),
    hasChildren: (nodeId) => data.nodes[nodeId]?.hasChildren ?? false,
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

  describe("deriveHighlights", () => {
    test("mode='none' returns empty Maps/Sets", () => {
      const selection = { mode: "none", type: null, id: null };
      const result = DerivedState.deriveHighlights(selection, staticData);

      expect(result.nodeRoles.size).toBe(0);
      expect(result.highlightedArcs.size).toBe(0);
    });

    test("node selection: selected node is 'current'", () => {
      const selection = { mode: "click", type: "node", id: "fn_1" };
      const result = DerivedState.deriveHighlights(selection, staticData);

      expect(result.nodeRoles.get("fn_1")).toBe("current");
    });

    test("node selection: outgoing arc targets are 'dependency'", () => {
      // fn_1 has outgoing arcs to fn_2 and fn_3
      const selection = { mode: "click", type: "node", id: "fn_1" };
      const result = DerivedState.deriveHighlights(selection, staticData);

      expect(result.nodeRoles.get("fn_2")).toBe("dependency");
      expect(result.nodeRoles.get("fn_3")).toBe("dependency");
    });

    test("node selection: incoming arc sources are 'dependent'", () => {
      // fn_2 has incoming arc from fn_1
      const selection = { mode: "click", type: "node", id: "fn_2" };
      const result = DerivedState.deriveHighlights(selection, staticData);

      expect(result.nodeRoles.get("fn_2")).toBe("current");
      expect(result.nodeRoles.get("fn_1")).toBe("dependent");
    });

    test("node selection: related arcs are highlighted", () => {
      const selection = { mode: "click", type: "node", id: "fn_1" };
      const result = DerivedState.deriveHighlights(selection, staticData);

      expect(result.highlightedArcs.has("fn_1-fn_2")).toBe(true);
      expect(result.highlightedArcs.has("fn_1-fn_3")).toBe(true);
      expect(result.highlightedArcs.has("mod_b-mod_a")).toBe(false);
    });

    test("arc selection: from node is 'dependent', to node is 'dependency'", () => {
      const selection = { mode: "click", type: "arc", id: "fn_1-fn_3" };
      const result = DerivedState.deriveHighlights(selection, staticData);

      expect(result.nodeRoles.get("fn_1")).toBe("dependent");
      expect(result.nodeRoles.get("fn_3")).toBe("dependency");
    });

    test("arc selection: selected arc is highlighted", () => {
      const selection = { mode: "click", type: "arc", id: "fn_1-fn_2" };
      const result = DerivedState.deriveHighlights(selection, staticData);

      expect(result.highlightedArcs.has("fn_1-fn_2")).toBe(true);
      expect(result.highlightedArcs.size).toBe(1);
    });

    test("node without connections: only 'current', no other roles", () => {
      // Use modified data with isolated node
      const isolatedData = {
        nodes: {
          ...TEST_STATIC_DATA.nodes,
          isolated: { type: "function", parent: "crate", x: 100, y: 100, hasChildren: false }
        },
        arcs: TEST_STATIC_DATA.arcs
      };
      const sd = createMockStaticData(isolatedData);

      const selection = { mode: "click", type: "node", id: "isolated" };
      const result = DerivedState.deriveHighlights(selection, sd);

      expect(result.nodeRoles.get("isolated")).toBe("current");
      expect(result.nodeRoles.size).toBe(1);
      expect(result.highlightedArcs.size).toBe(0);
    });

    test("hover mode works same as click mode", () => {
      const selection = { mode: "hover", type: "node", id: "fn_1" };
      const result = DerivedState.deriveHighlights(selection, staticData);

      expect(result.nodeRoles.get("fn_1")).toBe("current");
      expect(result.nodeRoles.get("fn_2")).toBe("dependency");
    });
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

  describe("deriveArcVisibility", () => {
    test("both endpoints visible: arc visible", () => {
      const visibleNodes = new Set(["fn_1", "fn_2", "fn_3", "mod_a", "mod_b", "crate"]);
      const result = DerivedState.deriveArcVisibility(visibleNodes, staticData);

      expect(result.has("fn_1-fn_2")).toBe(true);
      expect(result.has("fn_1-fn_3")).toBe(true);
      expect(result.has("mod_b-mod_a")).toBe(true);
    });

    test("from node hidden: arc hidden", () => {
      const visibleNodes = new Set(["fn_2", "fn_3", "mod_a", "mod_b", "crate"]);
      const result = DerivedState.deriveArcVisibility(visibleNodes, staticData);

      // fn_1 is hidden, so fn_1-fn_2 and fn_1-fn_3 are hidden
      expect(result.has("fn_1-fn_2")).toBe(false);
      expect(result.has("fn_1-fn_3")).toBe(false);
      // mod_b-mod_a still visible
      expect(result.has("mod_b-mod_a")).toBe(true);
    });

    test("to node hidden: arc hidden", () => {
      const visibleNodes = new Set(["fn_1", "fn_3", "mod_a", "mod_b", "crate"]);
      const result = DerivedState.deriveArcVisibility(visibleNodes, staticData);

      // fn_2 is hidden, so fn_1-fn_2 is hidden
      expect(result.has("fn_1-fn_2")).toBe(false);
      // fn_1-fn_3 still visible
      expect(result.has("fn_1-fn_3")).toBe(true);
    });

    test("empty visible nodes: no arcs visible", () => {
      const visibleNodes = new Set();
      const result = DerivedState.deriveArcVisibility(visibleNodes, staticData);

      expect(result.size).toBe(0);
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
});
