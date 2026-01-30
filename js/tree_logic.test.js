import { test, expect, describe } from "bun:test";
import { TreeLogic } from "./tree_logic.js";

describe("TreeLogic", () => {
  // Helper: Create parentMap from simple structure
  // Structure: { parent: [child1, child2, ...] }
  function buildParentMap(structure) {
    const parentMap = new Map();
    for (const [parent, children] of Object.entries(structure)) {
      for (const child of children) {
        parentMap.set(child, parent);
      }
    }
    return parentMap;
  }

  describe("getDescendants", () => {
    test("returns empty array for leaf node", () => {
      const parentMap = buildParentMap({ root: ["leaf"] });
      expect(TreeLogic.getDescendants("leaf", parentMap)).toEqual([]);
    });

    test("finds direct children", () => {
      const parentMap = buildParentMap({ root: ["a", "b", "c"] });
      const descendants = TreeLogic.getDescendants("root", parentMap);
      expect(descendants).toHaveLength(3);
      expect(descendants).toContain("a");
      expect(descendants).toContain("b");
      expect(descendants).toContain("c");
    });

    test("finds nested descendants recursively", () => {
      // Tree: root -> a -> a1, a2
      //            -> b
      const parentMap = buildParentMap({
        root: ["a", "b"],
        a: ["a1", "a2"]
      });
      const descendants = TreeLogic.getDescendants("root", parentMap);
      expect(descendants).toHaveLength(4);
      expect(descendants).toContain("a");
      expect(descendants).toContain("b");
      expect(descendants).toContain("a1");
      expect(descendants).toContain("a2");
    });

    test("returns empty for non-existent node", () => {
      const parentMap = buildParentMap({ root: ["a"] });
      expect(TreeLogic.getDescendants("nonexistent", parentMap)).toEqual([]);
    });
  });

  describe("countDescendants", () => {
    test("returns correct count", () => {
      const parentMap = buildParentMap({
        root: ["a", "b"],
        a: ["a1", "a2", "a3"]
      });
      expect(TreeLogic.countDescendants("root", parentMap)).toBe(5);
      expect(TreeLogic.countDescendants("a", parentMap)).toBe(3);
      expect(TreeLogic.countDescendants("b", parentMap)).toBe(0);
    });
  });

  describe("getVisibleAncestor", () => {
    test("returns self when node is visible (parent not collapsed)", () => {
      const parentMap = buildParentMap({ root: ["child"] });
      const collapsedSet = new Set(); // nothing collapsed
      expect(TreeLogic.getVisibleAncestor("child", collapsedSet, parentMap)).toBe("child");
    });

    test("returns root for root node", () => {
      const parentMap = buildParentMap({ root: ["a"] });
      const collapsedSet = new Set();
      expect(TreeLogic.getVisibleAncestor("root", collapsedSet, parentMap)).toBe("root");
    });

    test("returns collapsed parent when parent is collapsed", () => {
      const parentMap = buildParentMap({ root: ["child"] });
      const collapsedSet = new Set(["root"]);
      expect(TreeLogic.getVisibleAncestor("child", collapsedSet, parentMap)).toBe("root");
    });

    test("traverses up to find visible ancestor", () => {
      // Tree: root -> a -> b -> c
      const parentMap = buildParentMap({
        root: ["a"],
        a: ["b"],
        b: ["c"]
      });
      // a is collapsed, so c's visible ancestor should be a
      const collapsedSet = new Set(["a"]);
      expect(TreeLogic.getVisibleAncestor("c", collapsedSet, parentMap)).toBe("a");
    });

    test("collapsed node is visible, its children are hidden", () => {
      const parentMap = buildParentMap({
        root: ["parent"],
        parent: ["child"]
      });
      const collapsedSet = new Set(["parent"]);

      // collapsed node returns self (visible as collapsed box)
      expect(TreeLogic.getVisibleAncestor("parent", collapsedSet, parentMap)).toBe("parent");

      // child of collapsed node returns collapsed ancestor (child is hidden)
      expect(TreeLogic.getVisibleAncestor("child", collapsedSet, parentMap)).toBe("parent");
    });

    test("visibility in nested tree structure", () => {
      const parentMap = buildParentMap({
        crate: ["mod_a", "mod_b"],
        mod_a: ["fn_1", "fn_2"]
      });
      const collapsedSet = new Set(["mod_a"]);

      const isHidden = (nodeId) =>
        TreeLogic.getVisibleAncestor(nodeId, collapsedSet, parentMap) !== nodeId;

      expect(isHidden("crate")).toBe(false);   // root
      expect(isHidden("mod_a")).toBe(false);   // collapsed but visible
      expect(isHidden("mod_b")).toBe(false);   // sibling of collapsed
      expect(isHidden("fn_1")).toBe(true);     // child of collapsed
      expect(isHidden("fn_2")).toBe(true);     // child of collapsed
    });
  });
});
