import { test, expect, describe, beforeEach } from "bun:test";
import { DerivedState } from "./derived_state.js";
import { TreeLogic } from "./tree_logic.js";

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
    crate: { type: "crate", parent: null, x: 0, y: 0, hasChildren: true },
    mod_a: { type: "module", parent: "crate", x: 20, y: 50, hasChildren: true },
    mod_b: { type: "module", parent: "crate", x: 20, y: 150, hasChildren: true },
    fn_1: { type: "function", parent: "mod_a", x: 40, y: 60, hasChildren: false },
    fn_2: { type: "function", parent: "mod_a", x: 40, y: 80, hasChildren: false },
    fn_3: { type: "function", parent: "mod_b", x: 40, y: 160, hasChildren: false }
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
    getAllNodeIds: () => Object.keys(data.nodes),
    getAllArcIds: () => Object.keys(data.arcs),
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
});
