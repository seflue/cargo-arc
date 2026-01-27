import { test, expect, describe } from "bun:test";
import { HighlightState } from "./highlight_state.js";

describe("HighlightState", () => {
  describe("create", () => {
    test("returns state with null pinned and empty originalValues", () => {
      const state = HighlightState.create();
      expect(state.pinned).toBeNull();
      expect(state.originalValues).toBeInstanceOf(Map);
      expect(state.originalValues.size).toBe(0);
    });
  });

  describe("pinned highlight", () => {
    test("getPinned returns null initially", () => {
      const state = HighlightState.create();
      expect(HighlightState.getPinned(state)).toBeNull();
    });

    test("setPinned stores type and id", () => {
      const state = HighlightState.create();
      HighlightState.setPinned(state, "node", "crate1");
      expect(HighlightState.getPinned(state)).toEqual({ type: "node", id: "crate1" });
    });

    test("clearPinned sets pinned to null", () => {
      const state = HighlightState.create();
      HighlightState.setPinned(state, "edge", "a-b");
      HighlightState.clearPinned(state);
      expect(HighlightState.getPinned(state)).toBeNull();
    });

    test("isPinned returns true for matching type and id", () => {
      const state = HighlightState.create();
      HighlightState.setPinned(state, "node", "mod1");

      expect(HighlightState.isPinned(state, "node", "mod1")).toBe(true);
      expect(HighlightState.isPinned(state, "node", "other")).toBe(false);
      expect(HighlightState.isPinned(state, "edge", "mod1")).toBe(false);
    });

    test("isPinned returns false when nothing pinned", () => {
      const state = HighlightState.create();
      expect(HighlightState.isPinned(state, "node", "any")).toBe(false);
    });
  });

  describe("togglePinned", () => {
    test("pins new element, returns true", () => {
      const state = HighlightState.create();
      const result = HighlightState.togglePinned(state, "node", "crate1");

      expect(result).toBe(true);
      expect(HighlightState.getPinned(state)).toEqual({ type: "node", id: "crate1" });
    });

    test("unpins same element, returns false", () => {
      const state = HighlightState.create();
      HighlightState.setPinned(state, "edge", "a-b");

      const result = HighlightState.togglePinned(state, "edge", "a-b");

      expect(result).toBe(false);
      expect(HighlightState.getPinned(state)).toBeNull();
    });

    test("switches to different element, returns true", () => {
      const state = HighlightState.create();
      HighlightState.setPinned(state, "node", "old");

      const result = HighlightState.togglePinned(state, "node", "new");

      expect(result).toBe(true);
      expect(HighlightState.getPinned(state)).toEqual({ type: "node", id: "new" });
    });

    test("switches from node to edge, returns true", () => {
      const state = HighlightState.create();
      HighlightState.setPinned(state, "node", "crate1");

      const result = HighlightState.togglePinned(state, "edge", "a-b");

      expect(result).toBe(true);
      expect(HighlightState.getPinned(state)).toEqual({ type: "edge", id: "a-b" });
    });
  });

  describe("original values storage", () => {
    test("storeOriginal/getOriginal roundtrip", () => {
      const state = HighlightState.create();
      const values = { strokeWidth: 1.5, scale: 1.0, tipX: 100, tipY: 200 };

      HighlightState.storeOriginal(state, "arc1", values);

      expect(HighlightState.getOriginal(state, "arc1")).toEqual(values);
    });

    test("getOriginal returns undefined for unknown arc", () => {
      const state = HighlightState.create();
      expect(HighlightState.getOriginal(state, "nonexistent")).toBeUndefined();
    });

    test("hasOriginal returns true/false correctly", () => {
      const state = HighlightState.create();

      expect(HighlightState.hasOriginal(state, "arc1")).toBe(false);

      HighlightState.storeOriginal(state, "arc1", { strokeWidth: 1 });

      expect(HighlightState.hasOriginal(state, "arc1")).toBe(true);
      expect(HighlightState.hasOriginal(state, "arc2")).toBe(false);
    });

    test("storeOriginal only stores if not already stored", () => {
      const state = HighlightState.create();
      const original = { strokeWidth: 1.5, scale: 1.0 };
      const modified = { strokeWidth: 2.0, scale: 1.3 };

      HighlightState.storeOriginal(state, "arc1", original);
      HighlightState.storeOriginal(state, "arc1", modified); // Should NOT overwrite

      expect(HighlightState.getOriginal(state, "arc1")).toEqual(original);
    });

    test("clearAllOriginals removes all stored values", () => {
      const state = HighlightState.create();
      HighlightState.storeOriginal(state, "arc1", { strokeWidth: 1 });
      HighlightState.storeOriginal(state, "arc2", { strokeWidth: 2 });

      HighlightState.clearAllOriginals(state);

      expect(state.originalValues.size).toBe(0);
      expect(HighlightState.getOriginal(state, "arc1")).toBeUndefined();
    });
  });

  describe("iteration", () => {
    test("forEachOriginal iterates over all stored values", () => {
      const state = HighlightState.create();
      HighlightState.storeOriginal(state, "arc1", { strokeWidth: 1 });
      HighlightState.storeOriginal(state, "arc2", { strokeWidth: 2 });

      const collected = [];
      HighlightState.forEachOriginal(state, (arcId, values) => {
        collected.push({ arcId, values });
      });

      expect(collected).toHaveLength(2);
      expect(collected).toContainEqual({ arcId: "arc1", values: { strokeWidth: 1 } });
      expect(collected).toContainEqual({ arcId: "arc2", values: { strokeWidth: 2 } });
    });
  });
});
